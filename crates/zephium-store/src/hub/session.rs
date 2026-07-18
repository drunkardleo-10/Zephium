//! Authoritative session snapshots, bounded decoding, and recovery quarantine.

use super::*;

pub(super) struct PreparedSession {
    state: SessionState,
    snapshot: String,
    pub(super) registry: HashSet<ProfileId>,
}

/// Allocation-bounded wire representation for the authoritative snapshot.
/// These local types intentionally deny unknown fields; deserializing the
/// public core types directly would allow a damaged/newer snapshot to be
/// silently projected onto an older schema and then overwritten.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundedSessionState {
    #[serde(deserialize_with = "deserialize_bounded_profiles")]
    profiles: Vec<BoundedProfile>,
    #[serde(deserialize_with = "deserialize_bounded_spaces")]
    spaces: Vec<BoundedSpace>,
    #[serde(deserialize_with = "deserialize_bounded_items")]
    items: Vec<BoundedItem>,
    active_space: Option<SpaceId>,
    active_item: Option<ItemId>,
    splits: Option<bounded_json::BoundedPane>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundedProfile {
    id: ProfileId,
    name: String,
    kind: ProfileKind,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundedSpace {
    id: SpaceId,
    profile: ProfileId,
    name: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundedItem {
    id: ItemId,
    parent: Option<ItemId>,
    placement: BoundedPlacement,
    kind: BoundedKind,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
enum BoundedPlacement {
    Favorites {
        profile: ProfileId,
    },
    Space {
        space: SpaceId,
        section: SpaceSection,
    },
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
enum BoundedKind {
    Folder {
        name: String,
    },
    Tab {
        url: String,
        title: String,
        zoom: f64,
    },
}

fn deserialize_bounded_profiles<'de, D>(deserializer: D) -> Result<Vec<BoundedProfile>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    bounded_json::deserialize_bounded_vec(deserializer, MAX_SESSION_PROFILES, |profile| {
        profile.kind != ProfileKind::Incognito && profile.name.len() <= MAX_NAME_BYTES
    })
}

fn deserialize_bounded_spaces<'de, D>(deserializer: D) -> Result<Vec<BoundedSpace>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    bounded_json::deserialize_bounded_vec(deserializer, MAX_SESSION_SPACES, |space| {
        space.name.len() <= MAX_NAME_BYTES
    })
}

fn deserialize_bounded_items<'de, D>(deserializer: D) -> Result<Vec<BoundedItem>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    bounded_json::deserialize_bounded_vec(deserializer, MAX_SESSION_ITEMS, |item| {
        match &item.kind {
            BoundedKind::Folder { name } => name.len() <= MAX_NAME_BYTES,
            BoundedKind::Tab { url, title, zoom } => {
                url.len() <= MAX_URL_BYTES
                    && title.len() <= MAX_TITLE_BYTES
                    && zoom.is_finite()
                    && (0.3..=3.0).contains(zoom)
            }
        }
    })
}

impl From<BoundedSessionState> for SessionState {
    fn from(value: BoundedSessionState) -> Self {
        Self {
            profiles: value
                .profiles
                .into_iter()
                .map(|profile| PersistedProfile {
                    id: profile.id,
                    name: profile.name,
                    kind: profile.kind,
                })
                .collect(),
            spaces: value
                .spaces
                .into_iter()
                .map(|space| PersistedSpace {
                    id: space.id,
                    profile: space.profile,
                    name: space.name,
                })
                .collect(),
            items: value
                .items
                .into_iter()
                .map(|item| PersistedItem {
                    id: item.id,
                    parent: item.parent,
                    placement: match item.placement {
                        BoundedPlacement::Favorites { profile } => Placement::Favorites { profile },
                        BoundedPlacement::Space { space, section } => {
                            Placement::Space { space, section }
                        }
                    },
                    kind: match item.kind {
                        BoundedKind::Folder { name } => PersistedKind::Folder { name },
                        BoundedKind::Tab { url, title, zoom } => {
                            PersistedKind::Tab { url, title, zoom }
                        }
                    },
                })
                .collect(),
            active_space: value.active_space,
            active_item: value.active_item,
            splits: value.splits.map(|pane| pane.0),
        }
    }
}

fn decode_authoritative_snapshot(data: &str) -> Option<SessionState> {
    bounded_json::from_str::<BoundedSessionState>(data)
        .ok()
        .map(SessionState::from)
}

impl Hub {
    pub(crate) fn save(&mut self, s: &SessionState) -> rusqlite::Result<()> {
        let prepared = self.prepare_session(s)?;
        if self
            .registry
            .difference(&prepared.registry)
            .next()
            .is_some()
        {
            return Err(invalid_data(
                "profile removal requires explicit deletion authorization",
            ));
        }
        self.validate_session_transition(&prepared.registry)?;
        self.commit_prepared_session(prepared, None)
    }

    pub(super) fn prepare_session(&self, s: &SessionState) -> rusqlite::Result<PreparedSession> {
        if self.recovery_required.is_some() {
            return Err(invalid_data("session recovery mode is read-only"));
        }
        // Privacy is enforced again at the adapter boundary. Core normally
        // filters private profiles while constructing a snapshot, but a new
        // caller or regression must fail closed instead of silently restoring
        // an incognito profile as a persistent named profile.
        if s.profiles
            .iter()
            .any(|profile| profile.kind == ProfileKind::Incognito)
        {
            return Err(rusqlite::Error::InvalidParameterName(
                "incognito profiles cannot be persisted".into(),
            ));
        }
        let canonical = core_session::canonicalize(s.clone());
        if canonical != *s {
            return Err(invalid_data("refusing to persist a noncanonical session"));
        }
        let s = canonical;
        let snapshot = serde_json::to_string(&s)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        if snapshot.len() > MAX_SESSION_SNAPSHOT_BYTES {
            return Err(rusqlite::Error::InvalidParameterName(
                "session snapshot exceeds persistence limit".into(),
            ));
        }
        let registry: HashSet<ProfileId> = s.profiles.iter().map(|profile| profile.id).collect();
        Ok(PreparedSession {
            state: s,
            snapshot,
            registry,
        })
    }

    pub(super) fn validate_session_transition(
        &self,
        next_registry: &HashSet<ProfileId>,
    ) -> rusqlite::Result<()> {
        let pending_deletions = self.profile_deletion_journal_entries()?;
        if pending_deletions
            .iter()
            .any(|deletion| next_registry.contains(&deletion.profile))
        {
            return Err(invalid_data(
                "active profile collides with pending deletion authorization",
            ));
        }
        if let Some(dir) = &self.dir {
            for added in next_registry.difference(&self.registry) {
                if profile_artifacts_exist(dir, *added)? {
                    return Err(invalid_data(
                        "new profile id collides with unclaimed on-disk data",
                    ));
                }
            }
        }
        Ok(())
    }

    pub(super) fn commit_prepared_session(
        &mut self,
        prepared: PreparedSession,
        authorize_deletion: Option<ProfileId>,
    ) -> rusqlite::Result<()> {
        let PreparedSession {
            state,
            snapshot,
            registry,
        } = prepared;
        let tx = self.meta.transaction()?;
        tx.execute("DELETE FROM profiles", [])?;
        {
            let mut ins = tx.prepare_cached(
                "INSERT INTO profiles(id, name, kind, position) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for (i, p) in state.profiles.iter().enumerate() {
                ins.execute(params![
                    p.id.to_string(),
                    p.name,
                    kind_to_str(p.kind).ok_or_else(|| {
                        invalid_data("incognito profile reached persistent transaction")
                    })?,
                    i as i64
                ])?;
            }
        }
        let last = state
            .active_space
            .and_then(|sp| state.spaces.iter().find(|x| x.id == sp))
            .map(|x| x.profile)
            .or_else(|| state.profiles.first().map(|p| p.id));
        tx.execute(
            "INSERT INTO state(id, last_profile) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET last_profile = ?1",
            params![last.map(|p| p.to_string())],
        )?;
        // The complete restorable session has one authoritative transaction.
        // Per-profile databases remain isolation roots for history/favicons,
        // but are no longer part of a multi-file snapshot commit.
        tx.execute(
            "INSERT INTO session_snapshot(id, schema_version, data) VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET schema_version = ?1, data = ?2",
            params![SESSION_SCHEMA_VERSION, snapshot],
        )?;
        if let Some(profile) = authorize_deletion {
            let inserted = tx.execute(
                "INSERT INTO profile_deletion_journal(profile_id, authorized_at)
                 VALUES (?1, ?2)",
                params![profile.to_string(), now_secs()],
            )?;
            if inserted != 1 {
                return Err(invalid_data(
                    "profile deletion authorization was not inserted exactly once",
                ));
            }
        }
        tx.commit()?;
        #[cfg(test)]
        if authorize_deletion.is_some()
            && std::mem::take(&mut self.ambiguous_profile_deletion_commit_once)
        {
            // Model an OS/SQLite commit result whose durable outcome cannot be
            // inferred from the returned error. The transaction is committed,
            // but process-local registry state has deliberately not advanced.
            return Err(invalid_data(
                "injected ambiguous profile-deletion commit outcome",
            ));
        }
        self.registry = registry;
        if !self.legacy_state_purged {
            match self.purge_legacy_profile_state() {
                Ok(()) => self.legacy_state_purged = true,
                Err(error) => {
                    // The authoritative transaction is already durable. Do
                    // not report it as failed and tempt a caller to make an
                    // unsafe assumption; retry one-time legacy cleanup later.
                    eprintln!("store: deferred legacy profile cleanup failed: {error}");
                }
            }
        }
        // Release removed connections immediately. Their files remain until
        // exact journal authorization plus native-erasure proof permits purge.
        self.profiles
            .retain(|profile, _| self.registry.contains(profile));
        Ok(())
    }

    pub(crate) fn load(&mut self) -> rusqlite::Result<Option<SessionState>> {
        if self.recovery_required.is_some() {
            return Err(invalid_data("authoritative session requires recovery"));
        }
        let authoritative = self
            .meta
            .query_row(
                "SELECT schema_version,
                        length(CAST(data AS BLOB)),
                        CASE WHEN length(CAST(data AS BLOB)) <= ?1 THEN data END
                 FROM session_snapshot WHERE id = 1",
                [MAX_SESSION_SNAPSHOT_BYTES as i64],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?;
        if let Some((version, bytes, data)) = authoritative {
            if version != SESSION_SCHEMA_VERSION {
                self.quarantine_authoritative(
                    "unsupported authoritative session schema",
                    version,
                    data.as_deref().map(str::as_bytes),
                )?;
                return Err(invalid_data("unsupported session snapshot schema"));
            }
            let Some(data) = data else {
                let _ = bytes;
                self.quarantine_authoritative(
                    "authoritative session exceeds persistence limit",
                    version,
                    None,
                )?;
                return Err(invalid_data("session snapshot exceeds persistence limit"));
            };
            let state = match decode_authoritative_snapshot(&data) {
                Some(state) => state,
                None => {
                    self.quarantine_authoritative(
                        "corrupt authoritative session snapshot",
                        version,
                        Some(data.as_bytes()),
                    )?;
                    return Err(invalid_data("corrupt authoritative session snapshot"));
                }
            };
            if self.validate_authoritative_registry(&state).is_err() {
                self.quarantine_authoritative(
                    "authoritative snapshot does not match profile registry",
                    version,
                    Some(data.as_bytes()),
                )?;
                return Err(invalid_data(
                    "authoritative snapshot does not match profile registry",
                ));
            }
            if core_session::canonicalize(state.clone()) != state {
                self.quarantine_authoritative(
                    "authoritative session is not in exact canonical form",
                    version,
                    Some(data.as_bytes()),
                )?;
                return Err(invalid_data(
                    "authoritative session is not in exact canonical form",
                ));
            }
            return Ok(Some(state));
        }

        // One-time compatibility reader for databases created before the
        // atomic meta snapshot migration. The next successful save publishes
        // the complete session into session_snapshot.
        let profiles: Vec<PersistedProfile> = {
            let mut stmt = self.meta.prepare_cached(
                "SELECT CASE WHEN length(CAST(id AS BLOB)) <= 26 THEN id END,
                            CASE WHEN length(CAST(name AS BLOB)) <= ?1 THEN name END,
                            CASE WHEN length(CAST(kind AS BLOB)) <= 16 THEN kind END
                     FROM profiles ORDER BY position, id",
            )?;
            let rows = stmt.query_map([MAX_NAME_BYTES as i64], |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })?;
            let mut profiles = Vec::with_capacity(self.registry.len());
            let mut loaded = HashSet::with_capacity(self.registry.len());
            for row in rows {
                let (id, name, kind) = row?;
                let id =
                    id.ok_or_else(|| invalid_data("compatibility profile id exceeds limit"))?;
                let name =
                    name.ok_or_else(|| invalid_data("compatibility profile name exceeds limit"))?;
                let kind =
                    kind.ok_or_else(|| invalid_data("compatibility profile kind exceeds limit"))?;
                let id = ProfileId::parse(&id)
                    .filter(|profile| profile.to_string() == id)
                    .ok_or_else(|| invalid_data("compatibility profile has an invalid id"))?;
                let kind = kind_from_str(&kind)
                    .ok_or_else(|| invalid_data("compatibility profile has an invalid kind"))?;
                if !self.registry.contains(&id) || !loaded.insert(id) {
                    return Err(invalid_data("compatibility profile registry mismatch"));
                }
                profiles.push(PersistedProfile { id, name, kind });
            }
            if loaded != self.registry {
                return Err(invalid_data("compatibility profile registry is incomplete"));
            }
            profiles
        };
        if profiles.is_empty() {
            return Ok(None);
        }
        let last_row = self
            .meta
            .query_row(
                "SELECT last_profile IS NULL,
                        CASE
                            WHEN length(CAST(last_profile AS BLOB)) <= 26 THEN last_profile
                        END
                 FROM state WHERE id = 1",
                [],
                |r| Ok((r.get::<_, bool>(0)?, r.get::<_, Option<String>>(1)?)),
            )
            .optional()?;
        let fallback = profiles.first().map(|profile| profile.id);
        let last = match last_row {
            None => return Err(invalid_data("compatibility state row is missing")),
            Some((true, None)) => fallback,
            Some((false, Some(raw))) => {
                let profile = ProfileId::parse(&raw)
                    .filter(|profile| profile.to_string() == raw)
                    .filter(|profile| self.registry.contains(profile))
                    .ok_or_else(|| invalid_data("compatibility state has an invalid profile"))?;
                Some(profile)
            }
            _ => {
                return Err(invalid_data(
                    "compatibility state profile exceeds persistence limit",
                ));
            }
        };

        let mut out = SessionState {
            profiles,
            ..Default::default()
        };
        let ids: Vec<ProfileId> = out.profiles.iter().map(|p| p.id).collect();
        for id in ids {
            let space_budget = MAX_SESSION_SPACES.saturating_sub(out.spaces.len());
            let item_budget = MAX_SESSION_ITEMS.saturating_sub(out.items.len());
            self.load_profile(id, &mut out, last == Some(id), space_budget, item_budget)?;
        }
        // Legacy profile files store positions per container, while the
        // authoritative snapshot has one canonical cross-profile container
        // order: every profile's favorites, then every space's pinned/today
        // sections. Reorder only whole already-validated containers; do not use
        // canonicalization itself to drop or repair source rows.
        let mut by_placement: HashMap<Placement, Vec<PersistedItem>> = HashMap::new();
        for item in std::mem::take(&mut out.items) {
            by_placement.entry(item.placement).or_default().push(item);
        }
        for profile in &out.profiles {
            if let Some(mut items) = by_placement.remove(&Placement::Favorites {
                profile: profile.id,
            }) {
                out.items.append(&mut items);
            }
        }
        for space in &out.spaces {
            for section in [SpaceSection::Pinned, SpaceSection::Today] {
                if let Some(mut items) = by_placement.remove(&Placement::Space {
                    space: space.id,
                    section,
                }) {
                    out.items.append(&mut items);
                }
            }
        }
        if !by_placement.is_empty() {
            return Err(invalid_data(
                "compatibility session contains an unowned item placement",
            ));
        }
        let canonical = core_session::canonicalize(out.clone());
        if canonical != out {
            return Err(invalid_data(
                "compatibility session is not in exact canonical form",
            ));
        }
        Ok(Some(out))
    }

    pub(crate) fn recovery_reason(&self) -> Option<&str> {
        self.recovery_required.as_deref()
    }

    fn quarantine_authoritative(
        &mut self,
        reason: &str,
        schema_version: i64,
        data: Option<&[u8]>,
    ) -> rusqlite::Result<()> {
        let detected_at = now_secs();
        self.meta.execute(
            "INSERT INTO session_recovery(id, detected_at, reason, schema_version, data)
             VALUES (1, ?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO NOTHING",
            params![detected_at, reason, schema_version, data],
        )?;
        // Preserve the first diagnosis and exact bounded bytes. A later open
        // must stay in recovery mode until a dedicated recovery operation
        // explicitly resolves the marker.
        self.recovery_required = recovery_reason(&self.meta)?.or_else(|| Some(reason.into()));
        Ok(())
    }

    fn validate_authoritative_registry(&self, state: &SessionState) -> rusqlite::Result<()> {
        if state.profiles.len() > MAX_SESSION_PROFILES
            || state
                .profiles
                .iter()
                .any(|profile| profile.kind == ProfileKind::Incognito)
        {
            return Err(invalid_data("invalid profiles in authoritative snapshot"));
        }
        let snapshot: HashSet<ProfileId> =
            state.profiles.iter().map(|profile| profile.id).collect();
        if snapshot.len() != state.profiles.len() || snapshot != self.registry {
            return Err(invalid_data(
                "authoritative snapshot does not match profile registry",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod bounded_snapshot_tests {
    use super::*;

    fn snapshot_with_profiles(profiles: &str) -> String {
        format!(
            r#"{{"profiles":[{profiles}],"spaces":[],"items":[],"active_space":null,"active_item":null,"splits":null}}"#
        )
    }

    #[test]
    fn authoritative_collection_limit_is_enforced_by_the_sequence_visitor() {
        let profile = format!(
            r#"{{"id":"{}","name":"P","kind":"Default"}}"#,
            ProfileId::from(1)
        );
        let profiles = vec![profile; MAX_SESSION_PROFILES + 1].join(",");
        assert!(decode_authoritative_snapshot(&snapshot_with_profiles(&profiles)).is_none());
    }

    #[test]
    fn authoritative_nested_unknown_fields_are_not_silently_projected_away() {
        let profile = format!(
            r#"{{"id":"{}","name":"P","kind":"Default","future":true}}"#,
            ProfileId::from(1)
        );
        assert!(decode_authoritative_snapshot(&snapshot_with_profiles(&profile)).is_none());
    }

    #[test]
    fn near_snapshot_cap_oversized_string_is_rejected_by_lexical_preflight() {
        let name = "x".repeat(MAX_SESSION_SNAPSHOT_BYTES - 1024);
        let profile = format!(
            r#"{{"id":"{}","name":"{name}","kind":"Default"}}"#,
            ProfileId::from(1)
        );
        let snapshot = snapshot_with_profiles(&profile);
        assert!(snapshot.len() < MAX_SESSION_SNAPSHOT_BYTES);
        assert!(snapshot.len() > MAX_SESSION_SNAPSHOT_BYTES - 2048);
        assert!(decode_authoritative_snapshot(&snapshot).is_none());
    }
}
