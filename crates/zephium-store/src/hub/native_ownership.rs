//! Global durable native-extension ownership reconciliation journal.

use super::*;

use zephium_core::extensions::{
    ExtensionAuthorityId, ExtensionCatalogGenerationRole, ExtensionCatalogSetDigest,
    ExtensionGrantBrowsingContext, ExtensionGrantDigest, ExtensionGrantRevision,
    ExtensionInstallCatalogRevision, ExtensionInstallRevision, ExtensionManifestDigest,
    ExtensionNativeIncarnation, ExtensionNativeOwnershipApplyError, ExtensionNativeOwnershipEntry,
    ExtensionNativeOwnershipEntryRevision, ExtensionNativeOwnershipIdentity,
    ExtensionNativeOwnershipIntent, ExtensionNativeOwnershipJournal,
    ExtensionNativeOwnershipJournalMutation, ExtensionNativeOwnershipJournalRevision,
    ExtensionNativeOwnershipKey, ExtensionNativeOwnershipMutationKind,
    ExtensionNativeOwnershipOperation, ExtensionNativeOwnershipPhase, ExtensionPackageIdentity,
    ExtensionPackageKey, ExtensionPackageRevision, ExtensionRuntimeBackendTarget,
    ExtensionTreeDigest, EXTENSION_NATIVE_OWNERSHIP_ID_BYTES, EXTENSION_SHA256_BYTES,
    MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_ENTRIES,
};
use zephium_core::ids::ExtensionInstallId;
use zephium_core::ports::store::{
    ExtensionNativeOwnershipJournalLoadOutcome, ExtensionNativeOwnershipJournalMutationApplied,
    ExtensionNativeOwnershipJournalMutationOutcome,
};

use super::extensions::{
    decode_package_payload, encode_package_payload, exact_blob, revision_i64, revision_u64,
};

// Sort keys must be the bounded projections, never the raw durable values.
// SQLite uses an in-memory temporary B-tree for the context rank on supported
// builds; feeding attacker-sized corrupt TEXT/BLOB values into that sorter
// would defeat the complete cohort's retained-memory bound before Rust sees a
// row.
const LOAD_JOURNAL_ROWS_SQL: &str =
    "SELECT
         CASE WHEN length(CAST(profile_id AS BLOB)) <= 26 THEN profile_id END
             AS bounded_profile_id,
         CASE WHEN typeof(install_id) = 'blob' AND length(install_id) = 16 THEN install_id END
             AS bounded_install_id,
         CASE WHEN length(CAST(browsing_context AS BLOB)) <= 8 THEN browsing_context END
             AS bounded_browsing_context,
         operation,
         revision,
         CASE WHEN typeof(authority) = 'blob' AND length(authority) = 32 THEN authority END,
         CASE WHEN typeof(package_key) = 'blob' AND length(package_key) = 32 THEN package_key END,
         package_revision,
         CASE WHEN
             (payload_kind = 1 AND archive_length IS NULL AND archive_sha256 IS NULL)
             OR
             (payload_kind = 2
              AND typeof(archive_length) = 'integer'
              AND archive_length BETWEEN 1 AND 67108864
              AND typeof(archive_sha256) = 'blob'
              AND length(archive_sha256) = 32)
         THEN payload_kind END,
         CASE WHEN payload_kind = 2 THEN archive_length END,
         CASE WHEN payload_kind = 2
                    AND typeof(archive_sha256) = 'blob'
                    AND length(archive_sha256) = 32
              THEN archive_sha256 END,
         CASE WHEN typeof(manifest_sha256) = 'blob' AND length(manifest_sha256) = 32 THEN manifest_sha256 END,
         CASE WHEN typeof(tree_sha256) = 'blob' AND length(tree_sha256) = 32 THEN tree_sha256 END,
         CASE WHEN typeof(catalog_set_sha256) = 'blob' AND length(catalog_set_sha256) = 32 THEN catalog_set_sha256 END,
         CASE WHEN length(CAST(catalog_role AS BLOB)) <= 8 THEN catalog_role END,
         store_catalog_revision,
         store_install_revision,
         store_grant_revision,
         CASE WHEN typeof(grant_sha256) = 'blob' AND length(grant_sha256) = 32 THEN grant_sha256 END,
         CASE WHEN length(CAST(runtime_backend AS BLOB)) <= 32 THEN runtime_backend END,
         CASE
             WHEN native_identity_kind IS NULL AND native_identity IS NULL THEN 0
             WHEN typeof(native_identity_kind) = 'integer'
                  AND native_identity_kind IN (1, 2)
                  AND typeof(native_identity) = 'blob'
                  AND length(native_identity) = 32
             THEN 1
         END AS bounded_native_identity_valid,
         CASE WHEN typeof(native_identity_kind) = 'integer'
                        AND native_identity_kind IN (1, 2)
              THEN native_identity_kind END,
         CASE WHEN typeof(native_identity) = 'blob' AND length(native_identity) = 32
              THEN native_identity END,
         native_incarnation,
         CASE WHEN length(CAST(intent AS BLOB)) <= 8 THEN intent END,
         CASE WHEN length(CAST(phase AS BLOB)) <= 32 THEN phase END,
         CASE
             WHEN length(CAST(browsing_context AS BLOB)) <= 8
             THEN CASE browsing_context
                      WHEN 'regular' THEN 0
                      WHEN 'private' THEN 1
                      ELSE 2
                  END
         END AS bounded_context_rank
     FROM extension_native_ownership_journal
     ORDER BY bounded_profile_id, bounded_install_id, bounded_context_rank";

impl Hub {
    pub(crate) fn load_extension_native_ownership_journal(
        &mut self,
    ) -> rusqlite::Result<ExtensionNativeOwnershipJournalLoadOutcome> {
        let tx = self.meta.transaction()?;
        let journal = load_journal(&tx)?;
        tx.commit()?;
        Ok(ExtensionNativeOwnershipJournalLoadOutcome::Loaded(journal))
    }

    pub(crate) fn mutate_extension_native_ownership_journal(
        &mut self,
        expected: ExtensionNativeOwnershipJournalRevision,
        mutation: ExtensionNativeOwnershipJournalMutation,
    ) -> rusqlite::Result<ExtensionNativeOwnershipJournalMutationOutcome> {
        if self.recovery_required.is_some()
            && !matches!(
                &mutation,
                ExtensionNativeOwnershipJournalMutation::Transition {
                    intent: ExtensionNativeOwnershipIntent::Release,
                    ..
                } | ExtensionNativeOwnershipJournalMutation::Clear { .. }
            )
        {
            return Ok(ExtensionNativeOwnershipJournalMutationOutcome::SessionRecoveryRequired);
        }
        if matches!(&mutation, ExtensionNativeOwnershipJournalMutation::Begin(_)) {
            let profile = mutation.profile();
            if !self.registry.contains(&profile) {
                return Ok(ExtensionNativeOwnershipJournalMutationOutcome::NotRegistered);
            }
            if self.degraded_profiles.contains(&profile) {
                return Ok(ExtensionNativeOwnershipJournalMutationOutcome::DegradedProfile);
            }
        }
        #[cfg(test)]
        let ambiguous_commit =
            std::mem::take(&mut self.ambiguous_extension_native_ownership_commit_once);

        let persistence_mutation = mutation.clone();
        let tx = self.meta.transaction()?;
        let current = load_journal(&tx)?;
        let current_revision = current.revision();
        let current_operation_high_water = current.operation_high_water();
        let current_incarnation_high_water = current.native_incarnation_high_water();
        let application = match current.apply(expected, mutation) {
            Ok(application) => application,
            Err(ExtensionNativeOwnershipApplyError::Conflict { current }) => {
                return Ok(ExtensionNativeOwnershipJournalMutationOutcome::Conflict { current })
            }
            Err(ExtensionNativeOwnershipApplyError::LimitReached) => {
                return Ok(ExtensionNativeOwnershipJournalMutationOutcome::LimitReached)
            }
            Err(ExtensionNativeOwnershipApplyError::Invalid) => {
                return Ok(ExtensionNativeOwnershipJournalMutationOutcome::Invalid)
            }
            Err(ExtensionNativeOwnershipApplyError::RevisionExhausted) => {
                return Ok(ExtensionNativeOwnershipJournalMutationOutcome::RevisionExhausted)
            }
        };
        let next = application.journal();
        match (application.kind(), &persistence_mutation) {
            (
                ExtensionNativeOwnershipMutationKind::Begin,
                ExtensionNativeOwnershipJournalMutation::Begin(_),
            ) => insert_entry(
                &tx,
                application
                    .entry()
                    .ok_or_else(|| invalid_data("journal begin has no resulting row"))?,
            )?,
            (
                ExtensionNativeOwnershipMutationKind::Transition,
                ExtensionNativeOwnershipJournalMutation::Transition { expected, .. },
            ) => update_entry(
                &tx,
                *expected,
                application
                    .entry()
                    .ok_or_else(|| invalid_data("journal transition has no resulting row"))?,
            )?,
            (
                ExtensionNativeOwnershipMutationKind::Clear,
                ExtensionNativeOwnershipJournalMutation::Clear { expected },
            ) => delete_entry(&tx, *expected)?,
            _ => {
                return Err(invalid_data(
                    "native-ownership aggregate changed mutation identity",
                ))
            }
        }

        let state_updated = tx.execute(
            "UPDATE extension_native_ownership_journal_state
             SET revision = ?2,
                 operation_high_water = ?3,
                 native_incarnation_high_water = ?4
             WHERE id = 1
               AND revision = ?1
               AND operation_high_water = ?5
               AND native_incarnation_high_water = ?6",
            params![
                revision_i64(current_revision.get())?,
                revision_i64(next.revision().get())?,
                high_water_i64(next.operation_high_water().map(|value| value.get()))?,
                high_water_i64(
                    next.native_incarnation_high_water()
                        .map(|value| value.get())
                )?,
                high_water_i64(current_operation_high_water.map(|value| value.get()))?,
                high_water_i64(current_incarnation_high_water.map(|value| value.get()))?,
            ],
        )?;
        if state_updated != 1 {
            return Err(invalid_data(
                "native-ownership journal state changed during compare-and-swap",
            ));
        }

        let applied = ExtensionNativeOwnershipJournalMutationApplied {
            journal_revision: next.revision(),
            operation_high_water: next.operation_high_water(),
            native_incarnation_high_water: next.native_incarnation_high_water(),
            entry: application.entry().cloned().map(Box::new),
        };
        let committed = tx.commit();
        #[cfg(test)]
        if ambiguous_commit {
            if let Err(error) = committed {
                eprintln!("store: injected extension native-ownership commit ambiguity: {error}");
            }
            return Ok(ExtensionNativeOwnershipJournalMutationOutcome::OutcomeUnknown);
        }
        match committed {
            Ok(()) => Ok(ExtensionNativeOwnershipJournalMutationOutcome::Applied(
                applied,
            )),
            Err(error) => {
                eprintln!(
                    "store: extension native-ownership journal commit outcome is unknown: {error}"
                );
                Ok(ExtensionNativeOwnershipJournalMutationOutcome::OutcomeUnknown)
            }
        }
    }

    /// Returns whether any unresolved row references `profile` after first
    /// validating the complete cohort. Profile deletion must call this before
    /// authorization and again before local purge.
    pub(super) fn has_extension_native_ownership_for_profile(
        &mut self,
        profile: ProfileId,
    ) -> rusqlite::Result<bool> {
        let tx = self.meta.transaction()?;
        let journal = load_journal(&tx)?;
        let present = journal
            .entries()
            .iter()
            .any(|entry| entry.key().profile() == profile);
        tx.commit()?;
        Ok(present)
    }

    #[cfg(test)]
    pub(crate) fn make_next_extension_native_ownership_commit_ambiguous(&mut self) {
        self.ambiguous_extension_native_ownership_commit_once = true;
    }

    #[cfg(test)]
    pub(crate) fn inject_extension_native_ownership_entry_for_interlock_test(
        &mut self,
        entry: &ExtensionNativeOwnershipEntry,
    ) -> rusqlite::Result<()> {
        let tx = self.meta.transaction()?;
        let state = load_journal(&tx)?;
        if state != ExtensionNativeOwnershipJournal::empty() {
            return Err(invalid_data(
                "native-ownership test injection requires pristine journal state",
            ));
        }
        insert_entry(&tx, entry)?;
        if entry.operation().get() != entry.native_incarnation().get() {
            return Err(invalid_data(
                "native-ownership test injection has mismatched clocks",
            ));
        }
        // Seed the minimum reachable history for this one live operation:
        // every skipped operation is a prior minimally-settled owner.
        let skipped_operations = entry.operation().get() - 1;
        let journal_revision = 1_u64
            .checked_add(entry.revision().get())
            .and_then(|value| value.checked_add(skipped_operations.checked_mul(3)?))
            .and_then(ExtensionNativeOwnershipJournalRevision::new)
            .ok_or_else(|| invalid_data("native-ownership test injection revision overflow"))?;
        tx.execute(
            "UPDATE extension_native_ownership_journal_state
             SET revision = ?1,
                 operation_high_water = ?2,
                 native_incarnation_high_water = ?3",
            params![
                revision_i64(journal_revision.get())?,
                revision_i64(entry.operation().get())?,
                revision_i64(entry.native_incarnation().get())?,
            ],
        )?;
        tx.commit()
    }
}

fn load_journal(conn: &Connection) -> rusqlite::Result<ExtensionNativeOwnershipJournal> {
    let (state_count, revision, operation_high_water, incarnation_high_water): (
        i64,
        Option<i64>,
        Option<i64>,
        Option<i64>,
    ) = conn.query_row(
        "SELECT
             count(*),
             CASE WHEN count(*) = 1 THEN max(revision) END,
             CASE WHEN count(*) = 1 THEN max(operation_high_water) END,
             CASE WHEN count(*) = 1 THEN max(native_incarnation_high_water) END
         FROM extension_native_ownership_journal_state",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    if state_count != 1 {
        return Err(invalid_data(
            "native-ownership journal has no unique revision authority",
        ));
    }
    let revision = revision
        .and_then(revision_u64)
        .and_then(ExtensionNativeOwnershipJournalRevision::new)
        .ok_or_else(|| invalid_data("native-ownership journal revision is invalid"))?;
    let operation_high_water = decode_operation_high_water(operation_high_water)?;
    let incarnation_high_water = decode_incarnation_high_water(incarnation_high_water)?;

    let count = conn.query_row(
        "SELECT count(*) FROM extension_native_ownership_journal",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    if !(0..=MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_ENTRIES as i64).contains(&count) {
        return Err(invalid_data("native-ownership journal exceeds entry limit"));
    }

    // Every variable-width value is guarded before SQLite can materialize it
    // into Rust. Invalid siblings fail the complete cohort; no LIMIT or WHERE
    // clause may hide them.
    let mut statement = conn.prepare(LOAD_JOURNAL_ROWS_SQL)?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, Option<String>>(0)?,
            row.get::<_, Option<Vec<u8>>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, Option<Vec<u8>>>(5)?,
            row.get::<_, Option<Vec<u8>>>(6)?,
            row.get::<_, i64>(7)?,
            row.get::<_, Option<i64>>(8)?,
            row.get::<_, Option<i64>>(9)?,
            row.get::<_, Option<Vec<u8>>>(10)?,
            row.get::<_, Option<Vec<u8>>>(11)?,
            row.get::<_, Option<Vec<u8>>>(12)?,
            row.get::<_, Option<Vec<u8>>>(13)?,
            row.get::<_, Option<String>>(14)?,
            row.get::<_, i64>(15)?,
            row.get::<_, i64>(16)?,
            row.get::<_, i64>(17)?,
            row.get::<_, Option<Vec<u8>>>(18)?,
            row.get::<_, Option<String>>(19)?,
            row.get::<_, Option<i64>>(20)?,
            row.get::<_, Option<i64>>(21)?,
            row.get::<_, Option<Vec<u8>>>(22)?,
            row.get::<_, i64>(23)?,
            row.get::<_, Option<String>>(24)?,
            row.get::<_, Option<String>>(25)?,
        ))
    })?;
    let mut entries = Vec::with_capacity(count as usize);
    for row in rows {
        let (
            profile,
            install_id,
            browsing_context,
            operation,
            entry_revision,
            authority,
            package_key,
            package_revision,
            payload_kind,
            archive_length,
            archive_sha256,
            manifest,
            tree,
            catalog_set,
            catalog_role,
            store_catalog_revision,
            store_install_revision,
            store_grant_revision,
            grant_digest,
            runtime_backend,
            native_identity_valid,
            native_identity_kind,
            native_identity,
            native_incarnation,
            intent,
            phase,
        ) = row?;
        let profile = canonical_profile(profile)?;
        let install_id = ExtensionInstallId::from(u128::from_be_bytes(exact_blob::<16>(
            install_id,
            "native-ownership install id is invalid",
        )?));
        let browsing_context = match browsing_context.as_deref() {
            Some("regular") => ExtensionGrantBrowsingContext::Regular,
            Some("private") => ExtensionGrantBrowsingContext::Private,
            _ => return Err(invalid_data("native-ownership browsing context is invalid")),
        };
        let operation = revision_u64(operation)
            .and_then(ExtensionNativeOwnershipOperation::new)
            .ok_or_else(|| invalid_data("native-ownership operation is invalid"))?;
        let entry_revision = revision_u64(entry_revision)
            .and_then(ExtensionNativeOwnershipEntryRevision::new)
            .ok_or_else(|| invalid_data("native-ownership entry revision is invalid"))?;
        let authority = ExtensionAuthorityId::from_bytes(exact_blob::<EXTENSION_SHA256_BYTES>(
            authority,
            "native-ownership authority is invalid",
        )?);
        let package_key = ExtensionPackageKey::from_bytes(exact_blob::<EXTENSION_SHA256_BYTES>(
            package_key,
            "native-ownership package key is invalid",
        )?);
        let package_revision = revision_u64(package_revision)
            .and_then(ExtensionPackageRevision::new)
            .ok_or_else(|| invalid_data("native-ownership package revision is invalid"))?;
        let payload = decode_package_payload(
            payload_kind,
            archive_length,
            archive_sha256,
            "native-ownership package payload is invalid",
        )?;
        let manifest = ExtensionManifestDigest::from_bytes(exact_blob::<EXTENSION_SHA256_BYTES>(
            manifest,
            "native-ownership manifest digest is invalid",
        )?);
        let tree = ExtensionTreeDigest::from_bytes(exact_blob::<EXTENSION_SHA256_BYTES>(
            tree,
            "native-ownership tree digest is invalid",
        )?);
        let package = ExtensionPackageIdentity::new(
            authority,
            package_key,
            package_revision,
            payload,
            manifest,
            tree,
        );
        let catalog_set =
            ExtensionCatalogSetDigest::from_bytes(exact_blob::<EXTENSION_SHA256_BYTES>(
                catalog_set,
                "native-ownership catalog-set digest is invalid",
            )?);
        let catalog_role = catalog_role
            .as_deref()
            .and_then(ExtensionCatalogGenerationRole::from_persisted)
            .ok_or_else(|| invalid_data("native-ownership catalog role is invalid"))?;
        let store_catalog_revision = revision_u64(store_catalog_revision)
            .and_then(ExtensionInstallCatalogRevision::new)
            .ok_or_else(|| invalid_data("native-ownership store catalog revision is invalid"))?;
        let store_install_revision = revision_u64(store_install_revision)
            .and_then(ExtensionInstallRevision::new)
            .ok_or_else(|| invalid_data("native-ownership store install revision is invalid"))?;
        let store_grant_revision = revision_u64(store_grant_revision)
            .and_then(ExtensionGrantRevision::new)
            .ok_or_else(|| invalid_data("native-ownership store grant revision is invalid"))?;
        let grant_digest = ExtensionGrantDigest::from_bytes(exact_blob::<EXTENSION_SHA256_BYTES>(
            grant_digest,
            "native-ownership grant digest is invalid",
        )?);
        let runtime_backend = runtime_backend
            .as_deref()
            .and_then(ExtensionRuntimeBackendTarget::from_persisted)
            .ok_or_else(|| invalid_data("native-ownership runtime backend is invalid"))?;
        let native_identity =
            decode_native_identity(native_identity_valid, native_identity_kind, native_identity)?;
        let native_incarnation = revision_u64(native_incarnation)
            .and_then(ExtensionNativeIncarnation::new)
            .ok_or_else(|| invalid_data("native-ownership incarnation is invalid"))?;
        let intent = intent
            .as_deref()
            .and_then(ExtensionNativeOwnershipIntent::from_persisted)
            .ok_or_else(|| invalid_data("native-ownership intent is invalid"))?;
        let phase = phase
            .as_deref()
            .and_then(ExtensionNativeOwnershipPhase::from_persisted)
            .ok_or_else(|| invalid_data("native-ownership phase is invalid"))?;
        entries.push(
            ExtensionNativeOwnershipEntry::from_persisted_with_native_identity(
                ExtensionNativeOwnershipKey::new(profile, install_id, browsing_context),
                operation,
                entry_revision,
                package,
                catalog_set,
                catalog_role,
                store_catalog_revision,
                store_install_revision,
                store_grant_revision,
                grant_digest,
                runtime_backend,
                native_identity,
                native_incarnation,
                intent,
                phase,
            )
            .map_err(|_| invalid_data("native-ownership state combination is invalid"))?,
        );
    }
    if entries.len() != count as usize {
        return Err(invalid_data(
            "native-ownership journal changed while loading",
        ));
    }
    ExtensionNativeOwnershipJournal::from_persisted(
        revision,
        operation_high_water,
        incarnation_high_water,
        entries,
    )
    .map_err(|_| invalid_data("native-ownership journal cohort is invalid"))
}

fn insert_entry(conn: &Connection, entry: &ExtensionNativeOwnershipEntry) -> rusqlite::Result<()> {
    let key = entry.key();
    let install_id = key.install_id().bytes();
    let package = entry.package();
    let authority = package.authority().bytes();
    let package_key = package.key().bytes();
    let payload = encode_package_payload(package.payload())?;
    let manifest = package.manifest_sha256().bytes();
    let tree = package.tree_sha256().bytes();
    let catalog_set = entry.catalog_set_digest().bytes();
    let grant_digest = entry.grant_digest().bytes();
    let native_identity = entry.native_identity().map(|identity| identity.bytes());
    let inserted = conn.execute(
        "INSERT INTO extension_native_ownership_journal(
             profile_id, install_id, browsing_context, operation, revision,
             authority, package_key, package_revision,
             payload_kind, archive_length, archive_sha256,
             manifest_sha256, tree_sha256,
             catalog_set_sha256, catalog_role,
             store_catalog_revision, store_install_revision, store_grant_revision,
             grant_sha256, runtime_backend, native_identity_kind, native_identity,
             native_incarnation, intent, phase
         ) VALUES (
             ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
             ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23,
             ?24, ?25
         )",
        params![
            key.profile().to_string(),
            &install_id[..],
            context_str(key.browsing_context()),
            revision_i64(entry.operation().get())?,
            revision_i64(entry.revision().get())?,
            &authority[..],
            &package_key[..],
            revision_i64(package.revision().get())?,
            payload.kind,
            payload.archive_length,
            payload.archive_sha256.as_ref().map(|digest| &digest[..]),
            &manifest[..],
            &tree[..],
            &catalog_set[..],
            entry.catalog_role().as_persisted(),
            revision_i64(entry.store_catalog_revision().get())?,
            revision_i64(entry.store_install_revision().get())?,
            revision_i64(entry.store_grant_revision().get())?,
            &grant_digest[..],
            entry.runtime_backend().as_persisted(),
            entry
                .native_identity()
                .map(|identity| i64::from(identity.persisted_kind())),
            native_identity.as_ref().map(|identity| &identity[..]),
            revision_i64(entry.native_incarnation().get())?,
            entry.intent().as_persisted(),
            entry.phase().as_persisted(),
        ],
    )?;
    if inserted != 1 {
        return Err(invalid_data(
            "native-ownership row was not inserted exactly once",
        ));
    }
    Ok(())
}

fn update_entry(
    conn: &Connection,
    expected: zephium_core::extensions::ExtensionNativeOwnershipEntryCas,
    entry: &ExtensionNativeOwnershipEntry,
) -> rusqlite::Result<()> {
    if expected.key() != entry.key()
        || expected.operation() != entry.operation()
        || expected.native_incarnation() != entry.native_incarnation()
    {
        return Err(invalid_data(
            "native-ownership transition changed stable identity",
        ));
    }
    let key = expected.key();
    let install_id = key.install_id().bytes();
    let native_identity = entry.native_identity().map(|identity| identity.bytes());
    let updated = conn.execute(
        "UPDATE extension_native_ownership_journal
         SET revision = ?6,
             intent = ?7,
             phase = ?8,
             native_identity_kind = ?10,
             native_identity = ?11
         WHERE profile_id = ?1
           AND install_id = ?2
           AND browsing_context = ?3
           AND operation = ?4
           AND revision = ?5
           AND native_incarnation = ?9",
        params![
            key.profile().to_string(),
            &install_id[..],
            context_str(key.browsing_context()),
            revision_i64(expected.operation().get())?,
            revision_i64(expected.revision().get())?,
            revision_i64(entry.revision().get())?,
            entry.intent().as_persisted(),
            entry.phase().as_persisted(),
            revision_i64(expected.native_incarnation().get())?,
            entry
                .native_identity()
                .map(|identity| i64::from(identity.persisted_kind())),
            native_identity.as_ref().map(|identity| &identity[..]),
        ],
    )?;
    if updated != 1 {
        return Err(invalid_data(
            "native-ownership row changed during transition compare-and-swap",
        ));
    }
    Ok(())
}

fn delete_entry(
    conn: &Connection,
    expected: zephium_core::extensions::ExtensionNativeOwnershipEntryCas,
) -> rusqlite::Result<()> {
    let key = expected.key();
    let install_id = key.install_id().bytes();
    let deleted = conn.execute(
        "DELETE FROM extension_native_ownership_journal
         WHERE profile_id = ?1
           AND install_id = ?2
           AND browsing_context = ?3
           AND operation = ?4
           AND revision = ?5
           AND native_incarnation = ?6
           AND intent = 'release'
           AND phase = 'native_absent_release_pending'",
        params![
            key.profile().to_string(),
            &install_id[..],
            context_str(key.browsing_context()),
            revision_i64(expected.operation().get())?,
            revision_i64(expected.revision().get())?,
            revision_i64(expected.native_incarnation().get())?,
        ],
    )?;
    if deleted != 1 {
        return Err(invalid_data(
            "native-ownership row changed during clear compare-and-swap",
        ));
    }
    Ok(())
}

fn canonical_profile(raw: Option<String>) -> rusqlite::Result<ProfileId> {
    let raw = raw.ok_or_else(|| invalid_data("native-ownership profile id exceeds limit"))?;
    ProfileId::parse(&raw)
        .filter(|profile| profile.to_string() == raw)
        .ok_or_else(|| invalid_data("native-ownership profile id is not canonical"))
}

fn context_str(context: ExtensionGrantBrowsingContext) -> &'static str {
    match context {
        ExtensionGrantBrowsingContext::Regular => "regular",
        ExtensionGrantBrowsingContext::Private => "private",
    }
}

fn decode_native_identity(
    validity: Option<i64>,
    kind: Option<i64>,
    value: Option<Vec<u8>>,
) -> rusqlite::Result<Option<ExtensionNativeOwnershipIdentity>> {
    match (validity, kind, value) {
        (Some(0), None, None) => Ok(None),
        (Some(1), Some(kind @ 1..=2), Some(value)) => {
            let bytes = exact_blob::<EXTENSION_NATIVE_OWNERSHIP_ID_BYTES>(
                Some(value),
                "native-ownership native identity is invalid",
            )?;
            ExtensionNativeOwnershipIdentity::from_persisted(kind as u8, bytes)
                .map(Some)
                .map_err(|_| invalid_data("native-ownership native identity is invalid"))
        }
        _ => Err(invalid_data(
            "native-ownership native identity presence is invalid",
        )),
    }
}

fn decode_operation_high_water(
    value: Option<i64>,
) -> rusqlite::Result<Option<ExtensionNativeOwnershipOperation>> {
    match value {
        Some(0) => Ok(None),
        Some(value) => revision_u64(value)
            .and_then(ExtensionNativeOwnershipOperation::new)
            .map(Some)
            .ok_or_else(|| invalid_data("native-ownership operation high-water is invalid")),
        None => Err(invalid_data(
            "native-ownership operation high-water is absent",
        )),
    }
}

fn decode_incarnation_high_water(
    value: Option<i64>,
) -> rusqlite::Result<Option<ExtensionNativeIncarnation>> {
    match value {
        Some(0) => Ok(None),
        Some(value) => revision_u64(value)
            .and_then(ExtensionNativeIncarnation::new)
            .map(Some)
            .ok_or_else(|| invalid_data("native-ownership incarnation high-water is invalid")),
        None => Err(invalid_data(
            "native-ownership incarnation high-water is absent",
        )),
    }
}

fn high_water_i64(value: Option<u64>) -> rusqlite::Result<i64> {
    value.map_or(Ok(0), revision_i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrations;
    use zephium_core::extensions::{
        ExtensionNativeOwnershipPreparation, ExtensionPackagePayloadIdentity,
    };

    fn database() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        migrations::apply(&mut conn, migrations::META).unwrap();
        conn
    }

    fn package() -> ExtensionPackageIdentity {
        ExtensionPackageIdentity::new(
            ExtensionAuthorityId::from_bytes([1; 32]),
            ExtensionPackageKey::from_bytes([2; 32]),
            ExtensionPackageRevision::INITIAL,
            ExtensionPackagePayloadIdentity::BundledTree,
            ExtensionManifestDigest::from_bytes([3; 32]),
            ExtensionTreeDigest::from_bytes([4; 32]),
        )
    }

    fn native_identity() -> ExtensionNativeOwnershipIdentity {
        ExtensionNativeOwnershipIdentity::parse(
            ExtensionRuntimeBackendTarget::MacosNative,
            "abcdefghijklmnopabcdefghijklmnop",
        )
        .unwrap()
    }

    fn entry(profile: ProfileId, install: u128) -> ExtensionNativeOwnershipEntry {
        entry_with_state(
            profile,
            install,
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeAbsentPreparing,
        )
    }

    fn entry_with_state(
        profile: ProfileId,
        install: u128,
        intent: ExtensionNativeOwnershipIntent,
        phase: ExtensionNativeOwnershipPhase,
    ) -> ExtensionNativeOwnershipEntry {
        let entry_revision = match (intent, phase) {
            (
                ExtensionNativeOwnershipIntent::Acquire,
                ExtensionNativeOwnershipPhase::NativeAbsentPreparing,
            ) => 1,
            (
                ExtensionNativeOwnershipIntent::Acquire,
                ExtensionNativeOwnershipPhase::NativeMayOwn,
            ) => 2,
            (
                ExtensionNativeOwnershipIntent::Acquire,
                ExtensionNativeOwnershipPhase::NativeOwned,
            )
            | (
                ExtensionNativeOwnershipIntent::Release,
                ExtensionNativeOwnershipPhase::NativeMayOwn,
            ) => 3,
            (
                ExtensionNativeOwnershipIntent::Release,
                ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
            ) => 2,
            _ => panic!("invalid test native-ownership state"),
        };
        let native_identity =
            (phase == ExtensionNativeOwnershipPhase::NativeOwned).then(native_identity);
        ExtensionNativeOwnershipEntry::from_persisted_with_native_identity(
            ExtensionNativeOwnershipKey::new(
                profile,
                ExtensionInstallId::from(install),
                ExtensionGrantBrowsingContext::Regular,
            ),
            ExtensionNativeOwnershipOperation::new(install as u64).unwrap(),
            ExtensionNativeOwnershipEntryRevision::new(entry_revision).unwrap(),
            package(),
            ExtensionCatalogSetDigest::from_bytes([5; 32]),
            ExtensionCatalogGenerationRole::Active,
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallRevision::INITIAL,
            ExtensionGrantRevision::INITIAL,
            ExtensionGrantDigest::from_bytes([6; 32]),
            ExtensionRuntimeBackendTarget::MacosNative,
            native_identity,
            ExtensionNativeIncarnation::new(install as u64).unwrap(),
            intent,
            phase,
        )
        .unwrap()
    }

    fn preparation(profile: ProfileId, install: u128) -> ExtensionNativeOwnershipPreparation {
        ExtensionNativeOwnershipPreparation::new(
            ExtensionNativeOwnershipKey::new(
                profile,
                ExtensionInstallId::from(install),
                ExtensionGrantBrowsingContext::Regular,
            ),
            package(),
            ExtensionCatalogSetDigest::from_bytes([5; 32]),
            ExtensionCatalogGenerationRole::Active,
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallRevision::INITIAL,
            ExtensionGrantRevision::INITIAL,
            ExtensionGrantDigest::from_bytes([6; 32]),
            ExtensionRuntimeBackendTarget::MacosNative,
        )
    }

    #[test]
    fn load_rejects_unknown_or_malformed_rows_as_a_whole() {
        for column_update in [
            "runtime_backend = 'unknown'",
            "phase = 'unknown'",
            "profile_id = '!!!!!!!!!!!!!!!!!!!!!!!!!!'",
            "grant_sha256 = X'01'",
        ] {
            let conn = database();
            conn.pragma_update(None, "ignore_check_constraints", true)
                .unwrap();
            insert_entry(&conn, &entry(ProfileId::from(1), 1)).unwrap();
            conn.execute(
                &format!("UPDATE extension_native_ownership_journal SET {column_update}"),
                [],
            )
            .unwrap();
            conn.execute(
                "UPDATE extension_native_ownership_journal_state
                 SET revision = 2,
                     operation_high_water = 1,
                     native_incarnation_high_water = 1",
                [],
            )
            .unwrap();
            assert!(load_journal(&conn).is_err(), "accepted {column_update}");
        }
    }

    #[test]
    fn load_rejects_over_limit_cardinality_before_materializing_rows() {
        let mut conn = database();
        conn.execute(
            "DROP TRIGGER extension_native_ownership_journal_capacity",
            [],
        )
        .unwrap();
        let tx = conn.transaction().unwrap();
        for index in 1..=MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_ENTRIES + 1 {
            insert_entry(&tx, &entry(ProfileId::from(index as u128), index as u128)).unwrap();
        }
        tx.execute(
            "UPDATE extension_native_ownership_journal_state
             SET revision = ?1,
                 operation_high_water = ?2,
                 native_incarnation_high_water = ?2",
            params![
                MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_ENTRIES as i64 + 2,
                MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_ENTRIES as i64 + 1,
            ],
        )
        .unwrap();
        tx.commit().unwrap();
        assert!(load_journal(&conn).is_err());
    }

    #[test]
    fn load_sorter_materializes_only_bounded_key_projections() {
        let conn = database();
        insert_entry(&conn, &entry(ProfileId::from(1), 1)).unwrap();
        conn.execute(
            "UPDATE extension_native_ownership_journal_state
             SET revision = 2,
                 operation_high_water = 1,
                 native_incarnation_high_water = 1",
            [],
        )
        .unwrap();
        conn.pragma_update(None, "ignore_check_constraints", true)
            .unwrap();
        conn.pragma_update(None, "temp_store", "MEMORY").unwrap();
        conn.execute(
            "UPDATE extension_native_ownership_journal SET profile_id = ?1",
            ["x".repeat(16 * 1024)],
        )
        .unwrap();

        // Exercise the corrupt-key path and then pin the exact sorter inputs.
        // All three aliases project to at most 26 bytes, 16 bytes, and one
        // integer respectively; the 16 KiB raw key is never an ORDER BY term.
        let error = load_journal(&conn).unwrap_err();
        assert!(
            matches!(error, rusqlite::Error::InvalidParameterName(_)),
            "bounded projection was bypassed before decode: {error:?}"
        );
        let order_by = LOAD_JOURNAL_ROWS_SQL
            .split_once("ORDER BY")
            .map(|(_, clause)| clause)
            .unwrap();
        assert_eq!(
            order_by.trim(),
            "bounded_profile_id, bounded_install_id, bounded_context_rank"
        );
    }

    #[test]
    fn empty_migration_state_loads_exactly() {
        assert_eq!(
            load_journal(&database()).unwrap(),
            ExtensionNativeOwnershipJournal::empty()
        );
    }

    #[test]
    fn journal_rows_are_independent_of_profile_registry_lifetime() {
        let conn = database();
        let durable = entry(ProfileId::from(77), 1);
        insert_entry(&conn, &durable).unwrap();
        conn.execute(
            "UPDATE extension_native_ownership_journal_state
             SET revision = 2,
                 operation_high_water = 1,
                 native_incarnation_high_water = 1",
            [],
        )
        .unwrap();
        assert_eq!(
            conn.query_row("SELECT count(*) FROM profiles", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        let journal = load_journal(&conn).unwrap();
        assert_eq!(journal.entries(), &[durable]);
    }

    #[test]
    fn preparation_type_remains_path_free_and_fixed_size() {
        let preparation = preparation(ProfileId::from(1), 1);
        let debug = format!("{preparation:?}");
        assert!(!debug.contains('/'));
        assert!(!debug.contains("\\\\"));
    }

    #[test]
    fn session_recovery_blocks_begin_but_preserves_exact_cleanup_path() {
        let mut hub = Hub::in_memory().unwrap();
        let profile = ProfileId::from(81);
        let owned = entry_with_state(
            profile,
            1,
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeOwned,
        );
        hub.inject_extension_native_ownership_entry_for_interlock_test(&owned)
            .unwrap();
        let ExtensionNativeOwnershipJournalLoadOutcome::Loaded(seeded) =
            hub.load_extension_native_ownership_journal().unwrap()
        else {
            panic!("injected native-ownership journal did not load");
        };
        hub.recovery_required = Some("injected session recovery".into());

        let first = hub
            .mutate_extension_native_ownership_journal(
                seeded.revision(),
                ExtensionNativeOwnershipJournalMutation::transition(
                    owned.cas(),
                    ExtensionNativeOwnershipIntent::Release,
                    ExtensionNativeOwnershipPhase::NativeMayOwn,
                ),
            )
            .unwrap();
        let first = match first {
            ExtensionNativeOwnershipJournalMutationOutcome::Applied(applied) => applied,
            other => panic!("recovery blocked first cleanup transition: {other:?}"),
        };
        let releasing = *first.entry.unwrap();

        let second = hub
            .mutate_extension_native_ownership_journal(
                first.journal_revision,
                ExtensionNativeOwnershipJournalMutation::transition(
                    releasing.cas(),
                    ExtensionNativeOwnershipIntent::Release,
                    ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
                ),
            )
            .unwrap();
        let second = match second {
            ExtensionNativeOwnershipJournalMutationOutcome::Applied(applied) => applied,
            other => panic!("recovery blocked definite-absence transition: {other:?}"),
        };
        let release_pending = *second.entry.unwrap();

        let cleared = hub
            .mutate_extension_native_ownership_journal(
                second.journal_revision,
                ExtensionNativeOwnershipJournalMutation::clear(release_pending.cas()),
            )
            .unwrap();
        let cleared = match cleared {
            ExtensionNativeOwnershipJournalMutationOutcome::Applied(applied) => applied,
            other => panic!("recovery blocked exact journal clear: {other:?}"),
        };
        assert!(cleared.entry.is_none());

        assert_eq!(
            hub.mutate_extension_native_ownership_journal(
                cleared.journal_revision,
                ExtensionNativeOwnershipJournalMutation::begin(preparation(profile, 2)),
            )
            .unwrap(),
            ExtensionNativeOwnershipJournalMutationOutcome::SessionRecoveryRequired
        );
        let loaded = hub.load_extension_native_ownership_journal().unwrap();
        let ExtensionNativeOwnershipJournalLoadOutcome::Loaded(journal) = loaded else {
            panic!("journal became unreadable after recovery cleanup");
        };
        assert!(journal.entries().is_empty());
        assert_eq!(journal.revision(), cleared.journal_revision);
        assert_eq!(
            journal.operation_high_water(),
            Some(ExtensionNativeOwnershipOperation::INITIAL)
        );
        assert_eq!(
            journal.native_incarnation_high_water(),
            Some(ExtensionNativeIncarnation::INITIAL)
        );
    }

    #[test]
    fn session_recovery_rejects_acquire_directed_transition_without_mutation() {
        let mut hub = Hub::in_memory().unwrap();
        let preparing = entry(ProfileId::from(82), 1);
        hub.inject_extension_native_ownership_entry_for_interlock_test(&preparing)
            .unwrap();
        let ExtensionNativeOwnershipJournalLoadOutcome::Loaded(before) =
            hub.load_extension_native_ownership_journal().unwrap()
        else {
            panic!("injected native-ownership journal did not load");
        };
        hub.recovery_required = Some("injected session recovery".into());

        assert_eq!(
            hub.mutate_extension_native_ownership_journal(
                before.revision(),
                ExtensionNativeOwnershipJournalMutation::transition(
                    preparing.cas(),
                    ExtensionNativeOwnershipIntent::Acquire,
                    ExtensionNativeOwnershipPhase::NativeMayOwn,
                ),
            )
            .unwrap(),
            ExtensionNativeOwnershipJournalMutationOutcome::SessionRecoveryRequired
        );
        assert_eq!(
            hub.load_extension_native_ownership_journal().unwrap(),
            ExtensionNativeOwnershipJournalLoadOutcome::Loaded(before)
        );
    }

    #[test]
    fn load_preserves_absent_and_present_native_identity_exactly() {
        let conn = database();
        let may_own_without_identity = entry_with_state(
            ProfileId::from(91),
            1,
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeMayOwn,
        );
        let owned = entry_with_state(
            ProfileId::from(91),
            2,
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeOwned,
        );
        insert_entry(&conn, &may_own_without_identity).unwrap();
        insert_entry(&conn, &owned).unwrap();
        conn.execute(
            "UPDATE extension_native_ownership_journal_state
             SET revision = 6,
                 operation_high_water = 2,
                 native_incarnation_high_water = 2",
            [],
        )
        .unwrap();
        let loaded = load_journal(&conn).unwrap();
        assert_eq!(loaded.entries(), &[may_own_without_identity, owned]);
        assert_eq!(loaded.entries()[0].native_identity(), None);
        assert_eq!(
            loaded.entries()[1].native_identity(),
            Some(native_identity())
        );
    }

    #[test]
    fn load_rejects_malformed_oversize_or_cross_backend_native_identity() {
        for column_update in [
            "native_identity_kind = NULL",
            "native_identity = NULL",
            "native_identity = zeroblob(4096)",
            "native_identity = zeroblob(32)",
            "runtime_backend = 'windows_native'",
        ] {
            let conn = database();
            let owned = entry_with_state(
                ProfileId::from(92),
                1,
                ExtensionNativeOwnershipIntent::Acquire,
                ExtensionNativeOwnershipPhase::NativeOwned,
            );
            insert_entry(&conn, &owned).unwrap();
            conn.execute(
                "UPDATE extension_native_ownership_journal_state
                 SET revision = 4,
                     operation_high_water = 1,
                     native_incarnation_high_water = 1",
                [],
            )
            .unwrap();
            conn.pragma_update(None, "ignore_check_constraints", true)
                .unwrap();
            conn.execute(
                &format!("UPDATE extension_native_ownership_journal SET {column_update}"),
                [],
            )
            .unwrap();
            assert!(load_journal(&conn).is_err(), "accepted {column_update}");
        }
    }

    #[test]
    fn identity_commit_ambiguity_reloads_exactly_and_stale_replay_conflicts() {
        let mut hub = Hub::in_memory().unwrap();
        let profile = ProfileId::from(93);
        assert!(hub.registry.insert(profile));
        let ExtensionNativeOwnershipJournalLoadOutcome::Loaded(empty) =
            hub.load_extension_native_ownership_journal().unwrap()
        else {
            panic!("empty native-ownership journal did not load");
        };
        let begun = match hub
            .mutate_extension_native_ownership_journal(
                empty.revision(),
                ExtensionNativeOwnershipJournalMutation::begin(preparation(profile, 1)),
            )
            .unwrap()
        {
            ExtensionNativeOwnershipJournalMutationOutcome::Applied(applied) => applied,
            other => panic!("begin failed: {other:?}"),
        };
        let preparing = *begun.entry.unwrap();
        let may_own = match hub
            .mutate_extension_native_ownership_journal(
                begun.journal_revision,
                ExtensionNativeOwnershipJournalMutation::transition(
                    preparing.cas(),
                    ExtensionNativeOwnershipIntent::Acquire,
                    ExtensionNativeOwnershipPhase::NativeMayOwn,
                ),
            )
            .unwrap()
        {
            ExtensionNativeOwnershipJournalMutationOutcome::Applied(applied) => applied,
            other => panic!("may-own transition failed: {other:?}"),
        };
        let unresolved = *may_own.entry.unwrap();
        hub.make_next_extension_native_ownership_commit_ambiguous();
        assert_eq!(
            hub.mutate_extension_native_ownership_journal(
                may_own.journal_revision,
                ExtensionNativeOwnershipJournalMutation::transition_with_native_identity(
                    unresolved.cas(),
                    ExtensionNativeOwnershipIntent::Acquire,
                    ExtensionNativeOwnershipPhase::NativeOwned,
                    native_identity(),
                ),
            )
            .unwrap(),
            ExtensionNativeOwnershipJournalMutationOutcome::OutcomeUnknown
        );

        let ExtensionNativeOwnershipJournalLoadOutcome::Loaded(reloaded) =
            hub.load_extension_native_ownership_journal().unwrap()
        else {
            panic!("journal did not reload after ambiguous commit");
        };
        assert_eq!(
            reloaded.entries()[0].native_identity(),
            Some(native_identity())
        );
        assert_eq!(
            reloaded.entries()[0].phase(),
            ExtensionNativeOwnershipPhase::NativeOwned
        );
        assert!(hub
            .has_extension_native_ownership_for_profile(profile)
            .unwrap());
        assert_eq!(
            hub.mutate_extension_native_ownership_journal(
                may_own.journal_revision,
                ExtensionNativeOwnershipJournalMutation::transition_with_native_identity(
                    unresolved.cas(),
                    ExtensionNativeOwnershipIntent::Acquire,
                    ExtensionNativeOwnershipPhase::NativeOwned,
                    native_identity(),
                ),
            )
            .unwrap(),
            ExtensionNativeOwnershipJournalMutationOutcome::Conflict {
                current: reloaded.revision(),
            }
        );
    }
}
