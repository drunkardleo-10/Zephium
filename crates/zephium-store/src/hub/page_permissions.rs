//! Exact, per-profile remembered page-permission persistence.

use super::*;

use zephium_core::ids::PagePermissionGrantId;
use zephium_core::permissions::{
    PageOrigin, PagePermissionApplyError, PagePermissionCatalog, PagePermissionCatalogRevision,
    PagePermissionChange, PagePermissionGrant, PagePermissionGrantRevision, PagePermissionKind,
    PagePermissionPatch, RememberedPagePermission, MAX_PAGE_ORIGIN_BYTES,
    MAX_PAGE_PERMISSION_CATALOG_RETAINED_BYTES, MAX_PAGE_PERMISSION_GRANTS_PER_PROFILE,
};
use zephium_core::ports::store::{
    PagePermissionCatalogLoadOutcome, PagePermissionCatalogMutationApplied,
    PagePermissionCatalogMutationOutcome,
};

impl Hub {
    pub(crate) fn load_page_permission_catalog(
        &mut self,
        profile: ProfileId,
    ) -> rusqlite::Result<PagePermissionCatalogLoadOutcome> {
        if !self.registry.contains(&profile) {
            return Ok(PagePermissionCatalogLoadOutcome::NotRegistered);
        }
        if self.degraded_profiles.contains(&profile) {
            return Ok(PagePermissionCatalogLoadOutcome::DegradedProfile);
        }
        let tx = self.profile_conn(profile)?.transaction()?;
        let catalog = load_catalog(&tx)?;
        tx.commit()?;
        Ok(PagePermissionCatalogLoadOutcome::Loaded(catalog))
    }

    pub(crate) fn mutate_page_permission_catalog(
        &mut self,
        profile: ProfileId,
        expected: PagePermissionCatalogRevision,
        patch: PagePermissionPatch,
    ) -> rusqlite::Result<PagePermissionCatalogMutationOutcome> {
        if self.recovery_required.is_some() {
            return Err(invalid_data("session recovery mode is read-only"));
        }
        if !self.registry.contains(&profile) {
            return Ok(PagePermissionCatalogMutationOutcome::NotRegistered);
        }
        if self.degraded_profiles.contains(&profile) {
            return Ok(PagePermissionCatalogMutationOutcome::DegradedProfile);
        }
        #[cfg(test)]
        let ambiguous_commit = std::mem::take(&mut self.ambiguous_page_permission_commit_once);

        let conn = self.profile_conn(profile)?;
        let tx = conn.transaction()?;
        let current = load_catalog(&tx)?;
        let current_revision = current.revision();
        if current_revision != expected {
            return Ok(PagePermissionCatalogMutationOutcome::Conflict {
                current: current_revision,
            });
        }

        let application = match current.apply_patch(&patch) {
            Ok(application) => application,
            Err(PagePermissionApplyError::Invalid) => {
                return Ok(PagePermissionCatalogMutationOutcome::Invalid)
            }
            Err(PagePermissionApplyError::LimitReached) => {
                return Ok(PagePermissionCatalogMutationOutcome::LimitReached)
            }
            Err(PagePermissionApplyError::RevisionExhausted) => {
                return Ok(PagePermissionCatalogMutationOutcome::RevisionExhausted)
            }
        };
        let (catalog, results, changed) = application.into_parts();
        let applied = PagePermissionCatalogMutationApplied {
            catalog_revision: catalog.revision(),
            results,
        };
        if !changed {
            return Ok(PagePermissionCatalogMutationOutcome::Applied(applied));
        }

        // Mirror the aggregate's set semantics, independent of caller order:
        // release replaced authority keys first, then update surviving rows,
        // then create new rows. Response results remain in original patch
        // order. This permits an atomic id replacement under UNIQUE(origin,
        // kind) without briefly duplicating authority.
        for change in patch.changes() {
            if let PagePermissionChange::Delete { id, expected } = change {
                let deleted = tx.execute(
                    "DELETE FROM page_permission_grants
                     WHERE id = ?1 AND revision = ?2",
                    params![id.to_string(), revision_i64(expected.get())?],
                )?;
                if deleted != 1 {
                    return Err(invalid_data(
                        "page-permission row changed during delete compare-and-swap",
                    ));
                }
            }
        }
        for (change, result) in patch.changes().iter().zip(applied.results.as_slice()) {
            if let PagePermissionChange::Update { id, expected, .. } = change {
                let grant = result.grant.as_deref().ok_or_else(|| {
                    invalid_data("page-permission update has no aggregate result")
                })?;
                if grant.revision == *expected {
                    continue;
                }
                let updated = tx.execute(
                    "UPDATE page_permission_grants
                     SET revision = ?3, decision = ?4
                     WHERE id = ?1 AND revision = ?2",
                    params![
                        id.to_string(),
                        revision_i64(expected.get())?,
                        revision_i64(grant.revision.get())?,
                        grant.decision.as_persisted(),
                    ],
                )?;
                if updated != 1 {
                    return Err(invalid_data(
                        "page-permission row changed during update compare-and-swap",
                    ));
                }
            }
        }
        for (change, result) in patch.changes().iter().zip(applied.results.as_slice()) {
            if let PagePermissionChange::Create { .. } = change {
                let grant = result.grant.as_deref().ok_or_else(|| {
                    invalid_data("page-permission create has no aggregate result")
                })?;
                let inserted = tx.execute(
                    "INSERT INTO page_permission_grants(
                         id, revision, origin, kind, decision
                     ) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        grant.id.to_string(),
                        revision_i64(grant.revision.get())?,
                        grant.origin.as_str(),
                        grant.kind.as_persisted(),
                        grant.decision.as_persisted(),
                    ],
                )?;
                if inserted != 1 {
                    return Err(invalid_data(
                        "page-permission row was not inserted exactly once",
                    ));
                }
            }
        }
        let catalog_updated = tx.execute(
            "UPDATE page_permission_catalog SET revision = ?2
             WHERE id = 1 AND revision = ?1",
            params![
                revision_i64(current_revision.get())?,
                revision_i64(catalog.revision().get())?,
            ],
        )?;
        if catalog_updated != 1 {
            return Err(invalid_data(
                "page-permission catalog changed during compare-and-swap",
            ));
        }

        let committed = tx.commit();
        #[cfg(test)]
        if ambiguous_commit {
            if let Err(error) = committed {
                eprintln!(
                    "store: injected profile {profile} page-permission commit ambiguity: {error}"
                );
            }
            return Ok(PagePermissionCatalogMutationOutcome::OutcomeUnknown);
        }
        match committed {
            Ok(()) => Ok(PagePermissionCatalogMutationOutcome::Applied(applied)),
            Err(error) => {
                eprintln!(
                    "store: profile {profile} page-permission catalog commit outcome is unknown: {error}"
                );
                Ok(PagePermissionCatalogMutationOutcome::OutcomeUnknown)
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn make_next_page_permission_commit_ambiguous(&mut self) {
        self.ambiguous_page_permission_commit_once = true;
    }
}

fn load_catalog(conn: &Connection) -> rusqlite::Result<PagePermissionCatalog> {
    let (state_rows, raw_revision): (i64, Option<i64>) = conn.query_row(
        "SELECT count(*), CASE WHEN count(*) = 1 THEN max(revision) END
         FROM page_permission_catalog",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if state_rows != 1 {
        return Err(invalid_data(
            "page-permission catalog has no unique revision authority",
        ));
    }
    let revision = raw_revision
        .and_then(revision_u64)
        .and_then(PagePermissionCatalogRevision::new)
        .ok_or_else(|| invalid_data("page-permission catalog revision is invalid"))?;

    let count = conn.query_row("SELECT count(*) FROM page_permission_grants", [], |row| {
        row.get::<_, i64>(0)
    })?;
    if !(0..=MAX_PAGE_PERMISSION_GRANTS_PER_PROFILE as i64).contains(&count) {
        return Err(invalid_data("page-permission catalog exceeds grant limit"));
    }
    let total_origin_bytes = conn.query_row(
        "SELECT COALESCE(SUM(length(CAST(origin AS BLOB))), 0)
         FROM page_permission_grants",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    if !(0..=MAX_PAGE_PERMISSION_CATALOG_RETAINED_BYTES as i64).contains(&total_origin_bytes) {
        return Err(invalid_data(
            "page-permission catalog exceeds origin-byte limit",
        ));
    }

    let mut statement = conn.prepare(
        "SELECT
             CASE WHEN length(CAST(id AS BLOB)) <= 26 THEN id END,
             revision,
             CASE WHEN length(CAST(origin AS BLOB)) <= ?1 THEN origin END,
             CASE WHEN length(CAST(kind AS BLOB)) <= 32 THEN kind END,
             CASE WHEN length(CAST(decision AS BLOB)) <= 8 THEN decision END
         FROM page_permission_grants ORDER BY id",
    )?;
    let rows = statement.query_map([MAX_PAGE_ORIGIN_BYTES as i64], |row| {
        Ok((
            row.get::<_, Option<String>>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
        ))
    })?;
    let mut grants = Vec::with_capacity(count as usize);
    for row in rows {
        let (raw_id, raw_revision, raw_origin, raw_kind, raw_decision) = row?;
        let raw_id = raw_id.ok_or_else(|| invalid_data("page-permission id exceeds limit"))?;
        let id = PagePermissionGrantId::parse(&raw_id)
            .filter(|id| id.to_string() == raw_id)
            .ok_or_else(|| invalid_data("page-permission id is not canonical"))?;
        let revision = revision_u64(raw_revision)
            .and_then(PagePermissionGrantRevision::new)
            .ok_or_else(|| invalid_data("page-permission row revision is invalid"))?;
        let raw_origin =
            raw_origin.ok_or_else(|| invalid_data("page-permission origin exceeds limit"))?;
        let origin = PageOrigin::parse_exact(&raw_origin)
            .map_err(|_| invalid_data("page-permission origin is not canonical"))?;
        let kind = raw_kind
            .as_deref()
            .and_then(PagePermissionKind::from_persisted)
            .ok_or_else(|| invalid_data("page-permission kind is invalid"))?;
        let decision = raw_decision
            .as_deref()
            .and_then(RememberedPagePermission::from_persisted)
            .ok_or_else(|| invalid_data("page-permission decision is invalid"))?;
        grants.push(PagePermissionGrant {
            id,
            revision,
            origin,
            kind,
            decision,
        });
    }
    if grants.len() != count as usize {
        return Err(invalid_data(
            "page-permission catalog changed while loading",
        ));
    }
    PagePermissionCatalog::new(revision, grants)
        .map_err(|_| invalid_data("page-permission catalog is invalid"))
}

fn revision_u64(value: i64) -> Option<u64> {
    u64::try_from(value).ok().filter(|revision| *revision != 0)
}

fn revision_i64(value: u64) -> rusqlite::Result<i64> {
    i64::try_from(value).map_err(|_| invalid_data("page-permission revision overflow"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        migrations::apply(&mut conn, migrations::PROFILE).unwrap();
        conn
    }

    #[test]
    fn load_rejects_every_malformed_row_instead_of_returning_a_subset() {
        let cases = [
            (
                "!!!!!!!!!!!!!!!!!!!!!!!!!!".to_owned(),
                1_i64,
                "https://example.com".to_owned(),
                "camera".to_owned(),
                "allow".to_owned(),
            ),
            (
                PagePermissionGrantId::from(2).to_string(),
                0,
                "https://example.com".to_owned(),
                "camera".to_owned(),
                "allow".to_owned(),
            ),
            (
                PagePermissionGrantId::from(3).to_string(),
                1,
                "https://EXAMPLE.com".to_owned(),
                "camera".to_owned(),
                "allow".to_owned(),
            ),
            (
                PagePermissionGrantId::from(4).to_string(),
                1,
                "https://example.com/path".to_owned(),
                "camera".to_owned(),
                "allow".to_owned(),
            ),
            (
                PagePermissionGrantId::from(5).to_string(),
                1,
                "x".repeat(MAX_PAGE_ORIGIN_BYTES + 1),
                "camera".to_owned(),
                "allow".to_owned(),
            ),
            (
                PagePermissionGrantId::from(6).to_string(),
                1,
                "https://example.com".to_owned(),
                "unknown".to_owned(),
                "allow".to_owned(),
            ),
            (
                PagePermissionGrantId::from(7).to_string(),
                1,
                "https://example.com".to_owned(),
                "camera".to_owned(),
                "ask".to_owned(),
            ),
        ];
        for (id, revision, origin, kind, decision) in cases {
            let conn = database();
            conn.pragma_update(None, "ignore_check_constraints", true)
                .unwrap();
            conn.execute(
                "INSERT INTO page_permission_grants(
                     id, revision, origin, kind, decision
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, revision, origin, kind, decision],
            )
            .unwrap();
            assert!(load_catalog(&conn).is_err());
        }
    }

    #[test]
    fn load_rejects_hostile_cardinality_before_materializing_rows() {
        let mut conn = database();
        let tx = conn.transaction().unwrap();
        for index in 1..=MAX_PAGE_PERMISSION_GRANTS_PER_PROFILE + 1 {
            tx.execute(
                "INSERT INTO page_permission_grants(
                     id, revision, origin, kind, decision
                 ) VALUES (?1, 1, ?2, 'camera', 'deny')",
                params![
                    PagePermissionGrantId::from(index as u128).to_string(),
                    format!("https://p{index}.example"),
                ],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        assert!(load_catalog(&conn).is_err());
    }

    #[test]
    fn load_requires_one_exact_catalog_revision_authority() {
        let conn = database();
        conn.execute("DELETE FROM page_permission_catalog", [])
            .unwrap();
        assert!(load_catalog(&conn).is_err());
    }

    #[test]
    fn valid_load_is_canonical_and_deterministically_sorted() {
        let conn = database();
        for (id, origin, kind, decision) in [
            (
                PagePermissionGrantId::from(2),
                "https://two.example",
                "microphone",
                "deny",
            ),
            (
                PagePermissionGrantId::from(1),
                "https://one.example",
                "camera",
                "allow",
            ),
        ] {
            conn.execute(
                "INSERT INTO page_permission_grants(
                     id, revision, origin, kind, decision
                 ) VALUES (?1, 1, ?2, ?3, ?4)",
                params![id.to_string(), origin, kind, decision],
            )
            .unwrap();
        }
        let catalog = load_catalog(&conn).unwrap();
        assert_eq!(catalog.grants().len(), 2);
        assert_eq!(catalog.grants()[0].id, PagePermissionGrantId::from(1));
        assert_eq!(catalog.grants()[1].id, PagePermissionGrantId::from(2));
    }
}
