//! Crash-resumable profile deletion authorization, reconciliation, and purge.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use zephium_core::extensions::{
    ExtensionNativeNamespaceScope, MAX_EXTENSION_NATIVE_NAMESPACE_OBLIGATIONS,
};
use zephium_core::ports::store::{
    ExtensionNativeOwnershipJournalLoadOutcome, PendingProfileDeletion,
    ProfileDeletionAuthorizeOutcome,
};

use super::filesystem::profile_artifacts_absent;
use super::*;

const MAX_PROFILE_DELETION_JOURNAL: usize = core_session::MAX_SESSION_PROFILES;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ProfileDeletionJournalEntry {
    pub(super) profile: ProfileId,
    native_erasure_verified: bool,
    local_unlink_process: Option<ProfileId>,
    extension_native_namespace: Option<ExtensionNativeNamespaceScope>,
}

impl ProfileDeletionJournalEntry {
    fn pending(self) -> PendingProfileDeletion {
        PendingProfileDeletion {
            profile: self.profile,
            native_erasure_verified: self.native_erasure_verified,
            extension_native_namespace: self.extension_native_namespace,
        }
    }
}

impl Hub {
    #[cfg(test)]
    pub(crate) fn open_for_new_process(dir: PathBuf) -> rusqlite::Result<Self> {
        Self::open_with_deletion_process_generation(dir, new_deletion_process_generation())
    }

    /// Atomically publishes the exact canonical post-removal snapshot and its
    /// deletion journal row. No native erasure is authorized before this
    /// transaction commits.
    pub(crate) fn authorize_profile_deletion(
        &mut self,
        profile: ProfileId,
        filtered: &SessionState,
    ) -> rusqlite::Result<ProfileDeletionAuthorizeOutcome> {
        match self.has_extension_native_ownership_for_profile(profile) {
            Ok(false) => {}
            Ok(true) | Err(_) => {
                // The actor's outer authorization error path is reserved for
                // durability-ambiguous session/journal commits. Do not let a
                // corrupt or unreadable ownership cohort enter that path and
                // be mistaken for an already-authorized deletion after
                // reconciliation. Unknown ownership is pending ownership.
                return Ok(ProfileDeletionAuthorizeOutcome::ExtensionNativeOwnershipPending);
            }
        }
        let prepared = self.prepare_session(filtered)?;
        if prepared.registry.contains(&profile) {
            return Ok(ProfileDeletionAuthorizeOutcome::InvalidSession);
        }

        let journal = self.profile_deletion_journal_entries()?;
        let already_authorized = journal.iter().any(|deletion| deletion.profile == profile);
        if self.registry.contains(&profile) {
            let mut expected = self.registry.clone();
            expected.remove(&profile);
            if prepared.registry != expected {
                return Ok(ProfileDeletionAuthorizeOutcome::SessionConflict);
            }
            if journal.len() >= MAX_PROFILE_DELETION_JOURNAL {
                return Err(invalid_data(
                    "profile deletion journal exceeds persistence limit",
                ));
            }
            self.validate_session_transition(&prepared.registry)?;
            self.commit_prepared_session(prepared, Some(profile))?;
            Ok(ProfileDeletionAuthorizeOutcome::Authorized)
        } else if already_authorized {
            // A crash/retry may reach this path after the atomic transaction
            // but before native erasure was started or acknowledged. Allow a
            // newer exact snapshot of the same registry to commit while
            // preserving the original authorization row.
            if prepared.registry != self.registry {
                return Ok(ProfileDeletionAuthorizeOutcome::SessionConflict);
            }
            self.validate_session_transition(&prepared.registry)?;
            self.commit_prepared_session(prepared, None)?;
            Ok(ProfileDeletionAuthorizeOutcome::AlreadyAuthorized)
        } else {
            Ok(ProfileDeletionAuthorizeOutcome::NotRegistered)
        }
    }

    pub(super) fn profile_deletion_journal_entries(
        &self,
    ) -> rusqlite::Result<Vec<ProfileDeletionJournalEntry>> {
        let count =
            self.meta
                .query_row("SELECT count(*) FROM profile_deletion_journal", [], |row| {
                    row.get::<_, i64>(0)
                })?;
        if !(0..=MAX_PROFILE_DELETION_JOURNAL as i64).contains(&count) {
            return Err(invalid_data(
                "profile deletion journal exceeds persistence limit",
            ));
        }
        let mut statement = self.meta.prepare(
            "SELECT CASE
                        WHEN length(CAST(profile_id AS BLOB)) <= 26 THEN profile_id
                    END,
                    native_erasure_verified,
                    local_unlink_process IS NULL,
                    CASE
                        WHEN length(CAST(local_unlink_process AS BLOB)) <= 26
                        THEN local_unlink_process
                    END
             FROM profile_deletion_journal
             ORDER BY authorized_at, profile_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?;
        let mut profiles = Vec::with_capacity(count as usize);
        for row in rows {
            let (raw, native_erasure_verified, local_unlink_is_null, local_unlink_process) = row?;
            let raw = raw.ok_or_else(|| invalid_data("profile deletion id exceeds limit"))?;
            let profile = ProfileId::parse(&raw)
                .filter(|profile| profile.to_string() == raw)
                .ok_or_else(|| invalid_data("profile deletion journal has invalid id"))?;
            let native_erasure_verified = match native_erasure_verified {
                0 => false,
                1 => true,
                _ => {
                    return Err(invalid_data(
                        "profile deletion journal has invalid native-erasure state",
                    ))
                }
            };
            let local_unlink_process = match (local_unlink_is_null, local_unlink_process) {
                (1, None) => None,
                (0, Some(raw)) => {
                    let generation = ProfileId::parse(&raw)
                        .filter(|generation| generation.to_string() == raw)
                        .ok_or_else(|| {
                            invalid_data("profile deletion journal has invalid process generation")
                        })?;
                    Some(generation)
                }
                _ => {
                    return Err(invalid_data(
                        "profile deletion journal has invalid local-unlink state",
                    ));
                }
            };
            if local_unlink_process.is_some() && !native_erasure_verified {
                return Err(invalid_data(
                    "profile deletion journal completed local unlink without native proof",
                ));
            }
            if self.registry.contains(&profile) {
                return Err(invalid_data(
                    "profile deletion journal overlaps active registry",
                ));
            }
            profiles.push(ProfileDeletionJournalEntry {
                profile,
                native_erasure_verified,
                local_unlink_process,
                extension_native_namespace: None,
            });
        }
        if profiles.len() != count as usize {
            return Err(invalid_data(
                "profile deletion journal changed while loading",
            ));
        }
        self.attach_native_namespace_obligations(&mut profiles)?;
        Ok(profiles)
    }

    fn attach_native_namespace_obligations(
        &self,
        deletions: &mut [ProfileDeletionJournalEntry],
    ) -> rusqlite::Result<()> {
        let count = self.meta.query_row(
            "SELECT count(*) FROM extension_native_namespace_obligations",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        if !(0..=MAX_EXTENSION_NATIVE_NAMESPACE_OBLIGATIONS as i64).contains(&count) {
            return Err(invalid_data(
                "native extension namespace obligation cohort exceeds limit",
            ));
        }
        let mut statement = self.meta.prepare(
            "SELECT CASE
                        WHEN length(CAST(profile_id AS BLOB)) <= 26 THEN profile_id
                    END,
                    namespace_version
             FROM extension_native_namespace_obligations
             ORDER BY profile_id, namespace_version",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, Option<String>>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut observed = 0_usize;
        for row in rows {
            observed = observed
                .checked_add(1)
                .ok_or_else(|| invalid_data("native namespace obligation count overflow"))?;
            let (raw_profile, raw_version) = row?;
            let raw_profile = raw_profile
                .ok_or_else(|| invalid_data("native namespace profile id exceeds limit"))?;
            let profile = ProfileId::parse(&raw_profile)
                .filter(|profile| profile.to_string() == raw_profile)
                .ok_or_else(|| invalid_data("native namespace profile id is not canonical"))?;
            let version = u8::try_from(raw_version)
                .ok()
                .and_then(ExtensionNativeNamespaceScope::from_persisted_version)
                .ok_or_else(|| invalid_data("native namespace version is unsupported"))?;
            if self.registry.contains(&profile) {
                if deletions.iter().any(|deletion| deletion.profile == profile) {
                    return Err(invalid_data(
                        "native namespace obligation has ambiguous durable anchors",
                    ));
                }
                continue;
            }
            let deletion = deletions
                .iter_mut()
                .find(|deletion| deletion.profile == profile)
                .ok_or_else(|| {
                    invalid_data("native namespace obligation has no durable profile anchor")
                })?;
            if deletion.native_erasure_verified {
                return Err(invalid_data(
                    "native namespace obligation survives native-erasure proof",
                ));
            }
            if deletion
                .extension_native_namespace
                .replace(version)
                .is_some()
            {
                return Err(invalid_data(
                    "profile has multiple native namespace obligations",
                ));
            }
        }
        if observed != count as usize {
            return Err(invalid_data(
                "native namespace obligation cohort changed while loading",
            ));
        }
        Ok(())
    }

    fn pending_profile_deletion_entries(
        entries: &[ProfileDeletionJournalEntry],
    ) -> Vec<PendingProfileDeletion> {
        entries
            .iter()
            .copied()
            // A Windows unlink is reported complete to the current process,
            // while its authorization remains internally durable until a
            // later process start verifies absence. Do not make the shell
            // repeat a completed local operation during the same run.
            .filter(|entry| entry.local_unlink_process.is_none())
            .map(ProfileDeletionJournalEntry::pending)
            .collect()
    }

    pub(super) fn reconcile_completed_profile_deletion_tombstones(
        &mut self,
    ) -> rusqlite::Result<()> {
        let Some(dir) = self.dir.clone() else {
            return Ok(());
        };
        let completed: Vec<_> = self
            .profile_deletion_journal_entries()?
            .into_iter()
            .filter(|entry| entry.local_unlink_process != Some(self.deletion_process_generation))
            .filter(|entry| entry.local_unlink_process.is_some())
            .collect();
        if completed.is_empty() {
            return Ok(());
        }

        // Resolve filesystem truth before taking SQLite's write transaction.
        // A path that exists in any form (regular file, directory, symlink or
        // reparse-point-like entry) is not considered absent.
        let mut resolutions = Vec::with_capacity(completed.len());
        for entry in completed {
            let Some(prior_process) = entry.local_unlink_process else {
                return Err(invalid_data(
                    "completed profile deletion lost its process generation",
                ));
            };
            resolutions.push((
                entry.profile,
                prior_process,
                profile_artifacts_absent(&dir, entry.profile)?,
            ));
        }

        let tx = self.meta.transaction()?;
        for (profile, prior_process, absent) in resolutions {
            let changed = if absent {
                tx.execute(
                    "DELETE FROM profile_deletion_journal
                     WHERE profile_id = ?1
                       AND native_erasure_verified = 1
                       AND local_unlink_process = ?2",
                    params![profile.to_string(), prior_process.to_string()],
                )?
            } else {
                // The filesystem did not preserve the prior unlink across
                // restart. Keep native proof, reopen only the idempotent local
                // phase, and retain the original deletion authorization.
                tx.execute(
                    "UPDATE profile_deletion_journal
                     SET local_unlink_process = NULL
                     WHERE profile_id = ?1
                       AND native_erasure_verified = 1
                       AND local_unlink_process = ?2",
                    params![profile.to_string(), prior_process.to_string()],
                )?
            };
            if changed != 1 {
                return Err(invalid_data(
                    "profile deletion tombstone changed during restart reconciliation",
                ));
            }
        }
        tx.commit()
    }

    #[cfg(test)]
    pub(crate) fn pending_profile_deletions(
        &self,
    ) -> rusqlite::Result<Vec<PendingProfileDeletion>> {
        Ok(Self::pending_profile_deletion_entries(
            &self.profile_deletion_journal_entries()?,
        ))
    }

    #[cfg(test)]
    pub(crate) fn completed_profile_deletion_tombstones(&self) -> rusqlite::Result<Vec<ProfileId>> {
        Ok(self
            .profile_deletion_journal_entries()?
            .into_iter()
            .filter_map(|entry| {
                entry
                    .local_unlink_process
                    .is_some()
                    .then_some(entry.profile)
            })
            .collect())
    }

    /// Refreshes process-local registry truth from the durable transaction
    /// before interpreting the deletion journal. This is required after a
    /// commit error: SQLite/OS failures can leave the caller unable to infer
    /// whether COMMIT reached stable storage.
    pub(crate) fn reconcile_profile_deletion_journal(
        &mut self,
    ) -> rusqlite::Result<Vec<PendingProfileDeletion>> {
        self.load_registry()?;
        self.profiles
            .retain(|profile, _| self.registry.contains(profile));
        let journal = self.profile_deletion_journal_entries()?;
        let ExtensionNativeOwnershipJournalLoadOutcome::Loaded(native_ownership) =
            self.load_extension_native_ownership_journal()?
        else {
            return Err(invalid_data(
                "native extension ownership is unavailable during profile deletion recovery",
            ));
        };
        if journal.iter().any(|deletion| {
            native_ownership
                .entries()
                .iter()
                .any(|owner| owner.key().profile() == deletion.profile)
        }) {
            // A deletion authorization can predate the native-ownership
            // interlock (or survive an outcome-unknown boundary). Never hand
            // that stale capability to the application while cleanup still
            // has a possible native owner for the same profile. Loading the
            // complete bounded ownership cohort above also makes a malformed
            // sibling fail the whole reconciliation instead of hiding it
            // behind a targeted lookup.
            return Err(invalid_data(
                "profile deletion recovery is blocked by native extension ownership",
            ));
        }
        let pending = Self::pending_profile_deletion_entries(&journal);
        if !journal.is_empty() {
            let authoritative = self.meta.query_row(
                "SELECT EXISTS(SELECT 1 FROM session_snapshot WHERE id = 1)",
                [],
                |row| row.get::<_, bool>(0),
            )?;
            if !authoritative || self.load_authoritative()?.is_none() {
                return Err(invalid_data(
                    "profile deletion journal has no valid authoritative session",
                ));
            }
        }
        Ok(pending)
    }

    #[cfg(test)]
    pub(crate) fn fail_next_profile_deletion_commit_as_ambiguous(&mut self) {
        self.ambiguous_profile_deletion_commit_once = true;
    }

    #[cfg(test)]
    pub(crate) fn fail_next_profile_deletion_after_local_purge(&mut self) {
        self.fail_profile_deletion_after_local_purge_once = true;
    }

    pub(crate) fn finalize_profile_deletion(
        &mut self,
        profile: ProfileId,
    ) -> rusqlite::Result<bool> {
        self.finalize_profile_deletion_with_restart_confirmation(
            profile,
            cfg!(windows) && self.dir.is_some(),
        )
    }

    #[cfg(test)]
    pub(crate) fn finalize_profile_deletion_requiring_restart_confirmation(
        &mut self,
        profile: ProfileId,
    ) -> rusqlite::Result<bool> {
        self.finalize_profile_deletion_with_restart_confirmation(profile, self.dir.is_some())
    }

    fn finalize_profile_deletion_with_restart_confirmation(
        &mut self,
        profile: ProfileId,
        require_restart_confirmation: bool,
    ) -> rusqlite::Result<bool> {
        // Authorization cannot be used as a stale capability to purge the
        // local profile while a native extension owner remains unresolved.
        // Validate the complete global cohort; malformed siblings fail closed.
        if self.has_extension_native_ownership_for_profile(profile)? {
            return Err(invalid_data(
                "profile deletion is blocked by native extension ownership",
            ));
        }
        // Validate every row first. A malformed sibling must not be hidden by
        // a targeted query and later crowd a valid authorization out of the
        // bounded cohort.
        let journal = self.profile_deletion_journal_entries()?;
        let Some(deletion) = journal
            .into_iter()
            .find(|deletion| deletion.profile == profile)
        else {
            return Ok(false);
        };
        if self.registry.contains(&profile) {
            return Err(invalid_data("active profile cannot complete deletion"));
        }
        if deletion.local_unlink_process.is_some() && require_restart_confirmation {
            // The first process has already completed its local phase. Only a
            // Hub carrying a different process generation may clear this
            // tombstone after re-observing the recovered filesystem namespace.
            return Ok(true);
        }

        // Persist native proof before deleting the SQLite file. A crash after
        // this commit resumes only the local file phase; a crash before it
        // safely repeats the idempotent native verification.
        if !deletion.native_erasure_verified {
            let tx = self.meta.transaction()?;
            let profile_text = profile.to_string();
            let changed = match deletion.extension_native_namespace {
                Some(ExtensionNativeNamespaceScope::MacosControllerV1) => tx.execute(
                    "DELETE FROM extension_native_namespace_obligations
                     WHERE profile_id = ?1 AND namespace_version = 1",
                    [&profile_text],
                )?,
                Some(_) => {
                    return Err(invalid_data(
                        "profile deletion carries an unsupported native namespace",
                    ))
                }
                None => tx.execute(
                    "UPDATE profile_deletion_journal
                     SET native_erasure_verified = 1
                     WHERE profile_id = ?1 AND native_erasure_verified = 0",
                    [&profile_text],
                )?,
            };
            if changed != 1 {
                return Err(invalid_data(
                    "profile deletion native proof changed no exact durable obligation",
                ));
            }
            let settled = tx.query_row(
                "SELECT native_erasure_verified,
                        NOT EXISTS(
                            SELECT 1 FROM extension_native_namespace_obligations
                            WHERE profile_id = ?1
                        )
                 FROM profile_deletion_journal
                 WHERE profile_id = ?1",
                [&profile_text],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, bool>(1)?)),
            )?;
            if settled != (1, true) {
                return Err(invalid_data(
                    "profile deletion native proof did not settle namespace obligation",
                ));
            }
            tx.commit()?;
        }

        self.profiles.remove(&profile);
        if let Some(dir) = &self.dir {
            // Only this exact durable row authorizes unlinking. Arbitrary
            // profile-shaped orphans are never discovered or removed here.
            purge_profile_file(dir, profile)?;
        }

        #[cfg(test)]
        if std::mem::take(&mut self.fail_profile_deletion_after_local_purge_once) {
            return Err(invalid_data("injected failure after local profile purge"));
        }

        let changed = if require_restart_confirmation {
            self.meta.execute(
                "UPDATE profile_deletion_journal
                 SET local_unlink_process = ?2
                 WHERE profile_id = ?1
                   AND native_erasure_verified = 1
                   AND local_unlink_process IS NULL",
                params![
                    profile.to_string(),
                    self.deletion_process_generation.to_string()
                ],
            )?
        } else {
            self.meta.execute(
                "DELETE FROM profile_deletion_journal
                 WHERE profile_id = ?1 AND native_erasure_verified = 1",
                [profile.to_string()],
            )?
        };
        if changed != 1 {
            return Err(invalid_data(
                "profile deletion journal changed during completion",
            ));
        }
        self.degraded_profiles.remove(&profile);
        Ok(true)
    }
}

pub(super) fn deletion_process_generation() -> ProfileId {
    static GENERATION: OnceLock<ProfileId> = OnceLock::new();
    *GENERATION.get_or_init(new_deletion_process_generation)
}

fn new_deletion_process_generation() -> ProfileId {
    // Migration 9 reserves the zero ULID as the generation marker for a
    // completed version-8 unlink. Never mint it for a live process, making a
    // migrated tombstone provably eligible only for restart reconciliation.
    loop {
        let generation = ProfileId::generate();
        if generation != ProfileId::from(0) {
            return generation;
        }
    }
}

fn purge_profile_file(dir: &Path, profile: ProfileId) -> rusqlite::Result<()> {
    // This provides fail-closed logical deletion and overwrites SQLite cells
    // where the filesystem honors those writes. It is not a promise of
    // physical secure erasure on copy-on-write filesystems or SSD media.
    let path = dir.join(format!("profile-{profile}.sqlite"));
    if regular_file_exists(&path)? {
        let scrub = scrub_profile_database(&path);
        if let Err(error) = scrub {
            // A corrupt/unsupported database cannot be logically scrubbed
            // with SQLite, but it is still an exact journal-authorized file.
            // Continue to unlink it; retaining known private data forever is
            // not a safer fallback. NOFOLLOW/canonical-child validation keeps
            // authorization scoped to the owned data directory.
            eprintln!("store: unlinking unsrubbable deleted profile {profile}: {error}");
        }
    }

    // Remove sidecars first so a sidecar failure leaves the canonical file in
    // place and the next save/open can retry the whole cleanup.
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = path.as_os_str().to_owned();
        sidecar.push(suffix);
        remove_file_if_present(&PathBuf::from(sidecar))?;
    }
    remove_file_if_present(&path)?;
    // On Unix, order the file unlink before the journal authorization is
    // cleared. Otherwise a sudden power loss could retain a directory entry
    // while SQLite durably forgets that cleanup is pending. Win32 does not
    // document FlushFileBuffers for directory handles; its power-cut behavior
    // stays a packaged release gate rather than using an unsupported call
    // that would make every deletion fail.
    #[cfg(unix)]
    sync_directory(dir)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    Ok(())
}

/// Logically removes every current profile-owned data and authority domain
/// before the journal-authorized unlink. Keep this list in dependency order:
/// future PROFILE migrations that add durable user data must extend this
/// boundary and its direct inventory test in the same change.
fn scrub_profile_database(path: &Path) -> rusqlite::Result<()> {
    let mut conn = open_database(path)?;
    configure(&conn)?;
    migrations::apply(&mut conn, migrations::PROFILE)?;
    let tx = conn.transaction()?;
    tx.execute_batch(
        "INSERT INTO history_fts(history_fts, rank) VALUES('secure-delete', 1);
         DELETE FROM extension_profile_site_denials;
         DELETE FROM extension_profile_policy;
         DELETE FROM extension_grant_api_permissions;
         DELETE FROM extension_grant_host_permissions;
         DELETE FROM extension_grants;
         DELETE FROM extension_installs;
         DELETE FROM extension_install_catalog;
         DELETE FROM page_permission_grants;
         DELETE FROM page_permission_catalog;
         DELETE FROM task_list_receipts;
         DELETE FROM task_lists;
         DELETE FROM user_resource_receipts;
         DELETE FROM user_resources;
         DELETE FROM user_resource_usage;
         DELETE FROM userscripts;
         DELETE FROM userscript_catalog;
         DELETE FROM download_preferences;
         DELETE FROM downloads;
         DELETE FROM search_queries;
         DELETE FROM history;
         DELETE FROM history_usage;
         DELETE FROM favicons;
         DELETE FROM settings;
         DELETE FROM items;
         DELETE FROM spaces;
         DELETE FROM focus;
         DELETE FROM sqlite_sequence WHERE name = 'history';",
    )?;
    tx.commit()?;
    conn.execute_batch(
        "PRAGMA wal_checkpoint(TRUNCATE);
         VACUUM;
         PRAGMA journal_mode=DELETE;",
    )
}

fn remove_file_if_present(path: &Path) -> rusqlite::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(rusqlite::Error::ToSqlConversionFailure(Box::new(error))),
    }
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "store durability barrier target is not a direct directory",
        ));
    }
    std::fs::File::open(path)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection};
    use zephium_core::extensions::{
        ExtensionAuthorityId, ExtensionCatalogGenerationRole, ExtensionCatalogSetDigest,
        ExtensionGrantBrowsingContext, ExtensionGrantDigest, ExtensionGrantRevision,
        ExtensionInstallCatalogRevision, ExtensionInstallRevision, ExtensionManifestDigest,
        ExtensionNativeIncarnation, ExtensionNativeOwnershipEntry,
        ExtensionNativeOwnershipEntryRevision, ExtensionNativeOwnershipIntent,
        ExtensionNativeOwnershipKey, ExtensionNativeOwnershipOperation,
        ExtensionNativeOwnershipPhase, ExtensionPackageIdentity, ExtensionPackageKey,
        ExtensionPackagePayloadIdentity, ExtensionPackageRevision, ExtensionRuntimeBackendTarget,
        ExtensionTreeDigest,
    };
    use zephium_core::ids::ExtensionInstallId;
    use zephium_core::profiles::ProfileKind;
    use zephium_core::session::PersistedProfile;

    const PROFILE_SCRUB_MARKER: &str = "zephiumscrubmarker97613";

    fn native_entry(
        profile: ProfileId,
        install: ExtensionInstallId,
    ) -> ExtensionNativeOwnershipEntry {
        ExtensionNativeOwnershipEntry::from_persisted(
            ExtensionNativeOwnershipKey::new(
                profile,
                install,
                ExtensionGrantBrowsingContext::Regular,
            ),
            ExtensionNativeOwnershipOperation::INITIAL,
            ExtensionNativeOwnershipEntryRevision::new(2).unwrap(),
            ExtensionPackageIdentity::new(
                ExtensionAuthorityId::from_bytes([1; 32]),
                ExtensionPackageKey::from_bytes([2; 32]),
                ExtensionPackageRevision::INITIAL,
                ExtensionPackagePayloadIdentity::BundledTree,
                ExtensionManifestDigest::from_bytes([3; 32]),
                ExtensionTreeDigest::from_bytes([4; 32]),
            ),
            ExtensionCatalogSetDigest::from_bytes([5; 32]),
            ExtensionCatalogGenerationRole::Active,
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallRevision::INITIAL,
            ExtensionGrantRevision::INITIAL,
            ExtensionGrantDigest::from_bytes([6; 32]),
            ExtensionRuntimeBackendTarget::MacosNative,
            ExtensionNativeIncarnation::INITIAL,
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeMayOwn,
        )
        .unwrap()
    }

    fn deletion_sessions() -> (SessionState, SessionState) {
        let survivor = PersistedProfile {
            id: ProfileId::from(1),
            name: "Personal".into(),
            kind: ProfileKind::Default,
        };
        let deleted = PersistedProfile {
            id: ProfileId::from(2),
            name: "Work".into(),
            kind: ProfileKind::Named,
        };
        let filtered = SessionState {
            profiles: vec![survivor.clone()],
            spaces: Vec::new(),
            items: Vec::new(),
            active_space: None,
            active_item: None,
            splits: None,
            recently_closed: Vec::new(),
        };
        let mut full = filtered.clone();
        full.profiles.push(deleted);
        (full, filtered)
    }

    #[test]
    fn profile_scrub_covers_every_current_user_data_and_authority_table() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("profile-test.sqlite");
        let mut conn = open_database(&path).unwrap();
        configure(&conn).unwrap();
        migrations::apply(&mut conn, migrations::PROFILE).unwrap();
        conn.execute_batch(
            "INSERT INTO spaces(id, name, position) VALUES ('space', 'Personal', 0);
             INSERT INTO items(
                 id, parent_id, space_id, section, position, kind, name, url, title, zoom
             ) VALUES (
                 'item', NULL, 'space', 'today', 0, 'tab', NULL,
                 'https://private.example/path', 'Private title', 1
             );
             INSERT INTO focus(id, active_space, active_item, splits)
             VALUES (1, 'space', 'item', 'private-layout');
             INSERT INTO favicons(origin, content_type, icon, fetched_at)
             VALUES ('https://history.example', 'image/png', X'01020304', 1);
             INSERT INTO settings(key, value) VALUES ('private-setting', 'private-value');
             INSERT INTO downloads(id,session,revision,terminal,payload) VALUES ('00000000000000000000000001','00000000000000000000000002',1,1,'{\"private\":\"download-history\"}');
             INSERT INTO download_preferences(id,payload) VALUES (1,'{\"directory\":\"/private/downloads\"}');",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO history(url, title, visited_at) VALUES (?1, ?2, 1)",
            params![
                format!("https://history.example/{PROFILE_SCRUB_MARKER}"),
                PROFILE_SCRUB_MARKER
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO userscripts(
                 id, revision, enabled, metadata_format, source, source_sha256_v1
             ) VALUES (?1, 1, 1, 1, ?2, ?3)",
            params![
                "00000000000000000000000001",
                "// ==UserScript==\n// @name Secret\n// @match https://private.example/*\n// ==/UserScript==\n",
                vec![7_u8; 32]
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO page_permission_grants(
                 id, revision, origin, kind, decision
             ) VALUES (?1, 1, 'https://private.example', 'camera', 'allow')",
            ["00000000000000000000000002"],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO extension_installs(
                 id, revision, authority, package_key, package_revision,
                 payload_kind, archive_length, archive_sha256,
                 manifest_sha256, tree_sha256, desired_enabled
             ) VALUES (?1, 1, ?2, ?3, 1, 2, 17, ?4, ?5, ?6, 1)",
            params![
                vec![1_u8; 16],
                vec![2_u8; 32],
                vec![3_u8; 32],
                vec![4_u8; 32],
                vec![5_u8; 32],
                vec![6_u8; 32]
            ],
        )
        .unwrap();
        conn.execute(
            "UPDATE extension_install_catalog
             SET install_id_high_water = ?1 WHERE id = 1",
            [vec![1_u8; 16]],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO extension_grants(
                 install_id, revision, authority, package_key, package_revision,
                 payload_kind, archive_length, archive_sha256,
                 manifest_sha256, tree_sha256, grant_sha256,
                 file_access, private_access
             ) VALUES (?1, 1, ?2, ?3, 1, 2, 17, ?4, ?5, ?6, ?7, 1, 1)",
            params![
                vec![1_u8; 16],
                vec![2_u8; 32],
                vec![3_u8; 32],
                vec![4_u8; 32],
                vec![5_u8; 32],
                vec![6_u8; 32],
                vec![7_u8; 32]
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO extension_grant_api_permissions(install_id, name)
             VALUES (?1, ?2)",
            params![vec![1_u8; 16], PROFILE_SCRUB_MARKER],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO extension_grant_host_permissions(install_id, pattern)
             VALUES (?1, ?2)",
            params![
                vec![1_u8; 16],
                format!("https://{PROFILE_SCRUB_MARKER}.example/*")
            ],
        )
        .unwrap();
        let body = serde_json::to_string(&zephium_core::resources::ResourceDraft {
            title: PROFILE_SCRUB_MARKER.into(),
            pinned: false,
            related: vec![],
            content: zephium_core::resources::ResourceContent::Task {
                details: Default::default(),
                description: PROFILE_SCRUB_MARKER.into(),
                completed: false,
                due_date: None,
                due_time: None,
                status: zephium_core::resources::TaskStatus::Open,
                assignee: zephium_core::resources::TaskActor::User,
                origin: zephium_core::resources::TaskActor::User,
                context: None,
                sort_key: None,
                work: None,
            },
        })
        .unwrap();
        conn.execute("INSERT INTO user_resources(id,kind,revision,title,pinned,trashed,created_at,updated_at,body,search_text,completed) VALUES('00000000000000000000000001','task',1,?1,0,0,1,1,?2,?1,0)",params![PROFILE_SCRUB_MARKER,body]).unwrap();
        conn.execute("INSERT INTO user_resource_receipts(request_id,digest,resource_id,revision,retained) VALUES(?1,?2,'00000000000000000000000001',1,1)",params![PROFILE_SCRUB_MARKER,vec![1_u8;32]]).unwrap();
        conn.execute("INSERT INTO task_lists(id,title,revision,deleted) VALUES('00000000000000000000000002',?1,1,0)",[PROFILE_SCRUB_MARKER]).unwrap();
        conn.execute("INSERT INTO task_list_receipts(request_id,digest,list_id,retained) VALUES(?1,?2,'00000000000000000000000002',1)",params![PROFILE_SCRUB_MARKER,vec![2_u8;32]]).unwrap();
        drop(conn);

        scrub_profile_database(&path).unwrap();

        let conn = Connection::open(&path).unwrap();
        let actual_tables = conn
            .prepare(
                "SELECT name
                 FROM sqlite_schema
                 WHERE type = 'table'
                 ORDER BY name",
            )
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        let expected_tables = [
            "download_preferences",
            "downloads",
            "extension_grant_api_permissions",
            "extension_grant_host_permissions",
            "extension_grants",
            "extension_install_catalog",
            "extension_installs",
            "extension_profile_policy",
            "extension_profile_site_denials",
            "favicons",
            "focus",
            "history",
            "history_fts",
            "history_fts_config",
            "history_fts_data",
            "history_fts_docsize",
            "history_fts_idx",
            "history_usage",
            "items",
            "page_permission_catalog",
            "page_permission_grants",
            "resource_titles_fts",
            "resource_titles_fts_config",
            "resource_titles_fts_data",
            "resource_titles_fts_docsize",
            "resource_titles_fts_idx",
            "search_queries",
            "settings",
            "spaces",
            "sqlite_sequence",
            "task_list_receipts",
            "task_lists",
            "user_resource_receipts",
            "user_resource_usage",
            "user_resources",
            "userscript_catalog",
            "userscripts",
        ];
        assert_eq!(
            actual_tables,
            expected_tables.map(str::to_owned),
            "a PROFILE migration changed the scrub-owned table inventory"
        );

        for table in [
            "download_preferences",
            "downloads",
            "search_queries",
            "task_list_receipts",
            "task_lists",
            "user_resource_receipts",
            "user_resources",
            "user_resource_usage",
            "spaces",
            "items",
            "focus",
            "history",
            "history_usage",
            "favicons",
            "settings",
            "userscript_catalog",
            "userscripts",
            "page_permission_catalog",
            "page_permission_grants",
            "extension_install_catalog",
            "extension_installs",
            "extension_grants",
            "extension_grant_api_permissions",
            "extension_grant_host_permissions",
            "extension_profile_policy",
            "extension_profile_site_denials",
        ] {
            let count: i64 = conn
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 0, "profile scrub retained rows in {table}");
        }
        let retained_fts_terms: i64 = conn
            .query_row(
                "SELECT count(*) FROM history_fts WHERE history_fts MATCH ?1",
                [PROFILE_SCRUB_MARKER],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(retained_fts_terms, 0, "profile scrub retained FTS terms");
        let history_sequence: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_sequence WHERE name = 'history'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(history_sequence, 0);
        drop(conn);

        for database_file in database_files(&path) {
            if database_file.try_exists().unwrap() {
                let bytes = std::fs::read(&database_file).unwrap();
                assert!(
                    !bytes
                        .windows(PROFILE_SCRUB_MARKER.len())
                        .any(|window| window == PROFILE_SCRUB_MARKER.as_bytes()),
                    "profile scrub retained marker bytes in {}",
                    database_file.display()
                );
            }
        }
    }

    #[test]
    fn local_profile_purge_refuses_stale_authorization_while_native_owner_is_unresolved() {
        let mut hub = Hub::in_memory().unwrap();
        let profile = ProfileId::from(81);
        hub.meta
            .execute(
                "INSERT INTO profile_deletion_journal(
                     profile_id, authorized_at, native_erasure_verified,
                     local_unlink_completed, local_unlink_process
                 ) VALUES (?1, 1, 0, 0, NULL)",
                [profile.to_string()],
            )
            .unwrap();
        let entry = native_entry(profile, ExtensionInstallId::from(82));
        hub.inject_extension_native_ownership_entry_for_interlock_test(&entry)
            .unwrap();

        assert!(hub.finalize_profile_deletion(profile).is_err());
        let journal = hub.profile_deletion_journal_entries().unwrap();
        assert_eq!(journal.len(), 1);
        assert!(!journal[0].native_erasure_verified);
    }

    #[test]
    fn pending_deletion_recovery_refuses_a_profile_with_unresolved_native_ownership() {
        let mut hub = Hub::in_memory().unwrap();
        let (full, filtered) = deletion_sessions();
        let profile = ProfileId::from(2);
        hub.save(&full).unwrap();
        assert_eq!(
            hub.authorize_profile_deletion(profile, &filtered).unwrap(),
            ProfileDeletionAuthorizeOutcome::Authorized
        );
        let entry = native_entry(profile, ExtensionInstallId::from(83));
        hub.inject_extension_native_ownership_entry_for_interlock_test(&entry)
            .unwrap();

        let error = hub
            .reconcile_profile_deletion_journal()
            .expect_err("possible native ownership must hide stale deletion authority");
        assert!(
            error.to_string().contains("native extension ownership"),
            "{error}"
        );
        let journal = hub.profile_deletion_journal_entries().unwrap();
        assert_eq!(journal.len(), 1);
        assert!(!journal[0].native_erasure_verified);
    }

    #[test]
    fn pending_deletion_recovery_allows_valid_ownership_for_another_profile() {
        let mut hub = Hub::in_memory().unwrap();
        let (full, filtered) = deletion_sessions();
        let profile = ProfileId::from(2);
        hub.save(&full).unwrap();
        assert_eq!(
            hub.authorize_profile_deletion(profile, &filtered).unwrap(),
            ProfileDeletionAuthorizeOutcome::Authorized
        );
        let unrelated = native_entry(ProfileId::from(1), ExtensionInstallId::from(84));
        hub.inject_extension_native_ownership_entry_for_interlock_test(&unrelated)
            .unwrap();

        assert_eq!(
            hub.reconcile_profile_deletion_journal().unwrap(),
            vec![PendingProfileDeletion {
                profile,
                native_erasure_verified: false,
                extension_native_namespace: None,
            }]
        );
    }

    #[test]
    fn deletion_projects_and_atomically_settles_exact_native_namespace_scope() {
        let mut hub = Hub::in_memory().unwrap();
        let (full, filtered) = deletion_sessions();
        let profile = ProfileId::from(2);
        hub.save(&full).unwrap();
        hub.meta
            .execute(
                "INSERT INTO extension_native_namespace_obligations(
                     profile_id, namespace_version
                 ) VALUES (?1, 1)",
                [profile.to_string()],
            )
            .unwrap();
        // An ordinary authoritative save updates rows in place and must not
        // transiently orphan the retained native namespace.
        hub.save(&full).unwrap();

        assert_eq!(
            hub.authorize_profile_deletion(profile, &filtered).unwrap(),
            ProfileDeletionAuthorizeOutcome::Authorized
        );
        assert_eq!(
            hub.pending_profile_deletions().unwrap(),
            vec![PendingProfileDeletion {
                profile,
                native_erasure_verified: false,
                extension_native_namespace: Some(ExtensionNativeNamespaceScope::MacosControllerV1),
            }]
        );

        hub.fail_next_profile_deletion_after_local_purge();
        assert!(hub.finalize_profile_deletion(profile).is_err());
        assert_eq!(
            hub.pending_profile_deletions().unwrap(),
            vec![PendingProfileDeletion {
                profile,
                native_erasure_verified: true,
                extension_native_namespace: None,
            }],
            "a crash after proof must retain only the settled deletion tombstone"
        );
        assert_eq!(
            hub.meta
                .query_row(
                    "SELECT count(*) FROM extension_native_namespace_obligations
                     WHERE profile_id = ?1",
                    [profile.to_string()],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
        assert!(hub.finalize_profile_deletion(profile).unwrap());
        assert!(hub.pending_profile_deletions().unwrap().is_empty());
    }

    #[test]
    fn malformed_native_ownership_sibling_blocks_authorization_and_final_purge() {
        let mut hub = Hub::in_memory().unwrap();
        let (full, filtered) = deletion_sessions();
        hub.save(&full).unwrap();
        let target = ProfileId::from(2);
        let sibling = native_entry(ProfileId::from(1), ExtensionInstallId::from(1));
        hub.inject_extension_native_ownership_entry_for_interlock_test(&sibling)
            .unwrap();
        hub.meta
            .pragma_update(None, "ignore_check_constraints", true)
            .unwrap();
        hub.meta
            .execute(
                "UPDATE extension_native_ownership_journal SET phase = 'unknown'",
                [],
            )
            .unwrap();

        assert_eq!(
            hub.authorize_profile_deletion(target, &filtered).unwrap(),
            ProfileDeletionAuthorizeOutcome::ExtensionNativeOwnershipPending
        );
        assert!(hub.profile_deletion_journal_entries().unwrap().is_empty());

        // Reproduce a stale pre-interlock authorization without using the
        // production path, then prove finalization still validates every
        // ownership sibling before marking native proof or purging locally.
        hub.meta
            .execute("DELETE FROM profiles WHERE id = ?1", [target.to_string()])
            .unwrap();
        hub.load_registry().unwrap();
        hub.meta
            .execute(
                "INSERT INTO profile_deletion_journal(
                     profile_id, authorized_at, native_erasure_verified,
                     local_unlink_completed, local_unlink_process
                 ) VALUES (?1, 1, 0, 0, NULL)",
                [target.to_string()],
            )
            .unwrap();
        assert!(hub.reconcile_profile_deletion_journal().is_err());
        assert!(hub.finalize_profile_deletion(target).is_err());
        let journal = hub.profile_deletion_journal_entries().unwrap();
        assert_eq!(journal.len(), 1);
        assert!(!journal[0].native_erasure_verified);
    }

    fn database_files(path: &Path) -> [PathBuf; 3] {
        let mut wal = path.as_os_str().to_owned();
        wal.push("-wal");
        let mut shm = path.as_os_str().to_owned();
        shm.push("-shm");
        [path.to_path_buf(), PathBuf::from(wal), PathBuf::from(shm)]
    }
}
