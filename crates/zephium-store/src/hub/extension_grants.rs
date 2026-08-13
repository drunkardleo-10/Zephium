//! Exact per-install extension-grant persistence.
//!
//! There is intentionally no grant-catalog row or profile-wide grant clock.
//! A load observes the complete bounded install catalog and every subordinate
//! grant in one SQLite snapshot; a mutation compares the install catalog, the
//! exact install row, and only the target grant row in one transaction.

use std::sync::Arc;

use rusqlite::{params, Connection, OptionalExtension};

use zephium_core::extensions::{
    ApiPermissionName, ExtensionAuthorityId, ExtensionGrantApplyError, ExtensionGrantAuthority,
    ExtensionGrantDigest, ExtensionGrantManifestBindings, ExtensionGrantMutation,
    ExtensionGrantRevision, ExtensionInstall, ExtensionInstallCatalogRevision,
    ExtensionInstallRevision, ExtensionManifestDescriptor, ExtensionManifestDigest,
    ExtensionPackageIdentity, ExtensionPackageKey, ExtensionPackageRevision, ExtensionTreeDigest,
    EXTENSION_SHA256_BYTES, MAX_EXTENSION_API_PERMISSIONS, MAX_EXTENSION_API_PERMISSION_NAME_BYTES,
    MAX_EXTENSION_HOST_GRANTS, MAX_EXTENSION_INSTALLS_PER_PROFILE,
};
use zephium_core::ids::ExtensionInstallId;
use zephium_core::injection::{MatchPattern, MAX_MATCH_PATTERN_BYTES};
use zephium_core::ports::store::{
    ExtensionGrantCohortLoadOutcome, ExtensionGrantConflict, ExtensionGrantMutationApplied,
    ExtensionGrantMutationOutcome, ExtensionGrantWrite,
};

use super::*;

// PROFILE v12 carries these byte ceilings forward in immutable CHECK constraints.
const _: () = assert!(MAX_EXTENSION_API_PERMISSION_NAME_BYTES == 96);
const _: () = assert!(MAX_MATCH_PATTERN_BYTES == 2048);

enum GrantPersistence {
    None,
    Initialize,
    Apply {
        expected: ExtensionGrantRevision,
        mutation: ExtensionGrantMutation,
    },
    Patch {
        expected: ExtensionGrantRevision,
    },
}

pub(super) fn ensure_install_id_has_no_grant_rows(
    conn: &Connection,
    install_id: ExtensionInstallId,
) -> rusqlite::Result<()> {
    let id = install_id.bytes();
    let exists = conn.query_row(
        "SELECT
             EXISTS(SELECT 1 FROM extension_grants WHERE install_id = ?1)
             OR EXISTS(SELECT 1 FROM extension_grant_api_permissions WHERE install_id = ?1)
             OR EXISTS(SELECT 1 FROM extension_grant_host_permissions WHERE install_id = ?1)",
        [&id[..]],
        |row| row.get::<_, bool>(0),
    )?;
    if exists {
        return Err(invalid_data(
            "new extension install id has stale subordinate grant rows",
        ));
    }
    Ok(())
}

/// Proves only the durable prerequisite for recording enabled intent: one
/// initialized row bound to the exact install/package exists. This is not an
/// activation proof; activation must still exact-load and validate the full
/// manifest-bound cohort, including child grants and digest.
pub(super) fn has_exact_grant_root(
    conn: &Connection,
    install: &ExtensionInstall,
) -> rusqlite::Result<bool> {
    validate_global_grant_integrity(conn)?;
    let id = install.id().bytes();
    let package = install.package();
    let authority = package.authority().bytes();
    let key = package.key().bytes();
    let payload = super::extensions::encode_package_payload(package.payload())?;
    let manifest = package.manifest_sha256().bytes();
    let tree = package.tree_sha256().bytes();
    let count = conn.query_row(
        "SELECT count(*) FROM extension_grants
         WHERE install_id = ?1
           AND authority = ?2 AND package_key = ?3 AND package_revision = ?4
           AND payload_kind = ?5
           AND (
               (?5 = 1 AND archive_length IS NULL AND archive_sha256 IS NULL)
               OR
               (?5 = 2 AND archive_length = ?6 AND archive_sha256 = ?7)
           )
           AND manifest_sha256 = ?8 AND tree_sha256 = ?9",
        params![
            &id[..],
            &authority[..],
            &key[..],
            super::extensions::revision_i64(package.revision().get())?,
            payload.kind,
            payload.archive_length,
            payload.archive_sha256.as_ref().map(|digest| &digest[..]),
            &manifest[..],
            &tree[..],
        ],
        |row| row.get::<_, i64>(0),
    )?;
    Ok(count == 1)
}

impl Hub {
    pub(crate) fn load_extension_grant_cohort(
        &mut self,
        profile: ProfileId,
        bindings: ExtensionGrantManifestBindings,
    ) -> rusqlite::Result<ExtensionGrantCohortLoadOutcome> {
        if !self.registry.contains(&profile) {
            return Ok(ExtensionGrantCohortLoadOutcome::NotRegistered);
        }
        if self.degraded_profiles.contains(&profile) {
            return Ok(ExtensionGrantCohortLoadOutcome::DegradedProfile);
        }
        let tx = self.profile_conn(profile)?.transaction()?;
        let catalog = super::extensions::load_catalog(&tx)?;
        if bindings.len() != catalog.installs().len()
            || catalog.installs().iter().any(|install| {
                bindings
                    .get(install.id())
                    .is_none_or(|manifest| manifest.package() != install.package())
            })
        {
            return Ok(ExtensionGrantCohortLoadOutcome::Invalid);
        }
        let authorities = load_all_authorities(&tx, &catalog, &bindings)?;
        let cohort = zephium_core::extensions::ExtensionGrantCohort::from_persisted(
            profile,
            catalog,
            bindings,
            authorities,
        )
        .map_err(|_| invalid_data("extension grant cohort is invalid"))?;
        tx.commit()?;
        Ok(ExtensionGrantCohortLoadOutcome::Loaded(cohort))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn mutate_extension_grants(
        &mut self,
        profile: ProfileId,
        expected_catalog: ExtensionInstallCatalogRevision,
        expected_install: ExtensionInstallRevision,
        install_id: ExtensionInstallId,
        manifest: Arc<ExtensionManifestDescriptor>,
        write: ExtensionGrantWrite,
    ) -> rusqlite::Result<ExtensionGrantMutationOutcome> {
        if self.recovery_required.is_some() {
            return Err(invalid_data("session recovery mode is read-only"));
        }
        if !self.registry.contains(&profile) {
            return Ok(ExtensionGrantMutationOutcome::NotRegistered);
        }
        if self.degraded_profiles.contains(&profile) {
            return Ok(ExtensionGrantMutationOutcome::DegradedProfile);
        }
        #[cfg(test)]
        let ambiguous_commit = std::mem::take(&mut self.ambiguous_extension_grant_commit_once);

        self.profile_conn(profile)?;
        let meta = &self.meta;
        let conn = self
            .profiles
            .get_mut(&profile)
            .ok_or_else(|| invalid_data("registered extension profile connection is absent"))?;
        let tx = conn.transaction()?;
        let catalog = super::extensions::load_catalog(&tx)?;
        validate_global_grant_integrity(&tx)?;
        let current_install = catalog.get(install_id);
        let current_grant_revision = load_grant_revision(&tx, install_id)?;
        if catalog.revision() != expected_catalog
            || current_install.map(ExtensionInstall::revision) != Some(expected_install)
        {
            return Ok(ExtensionGrantMutationOutcome::Conflict(
                ExtensionGrantConflict::new(
                    catalog.revision(),
                    current_install.map(ExtensionInstall::revision),
                    current_grant_revision,
                ),
            ));
        }
        let install = current_install
            .ok_or_else(|| invalid_data("extension install disappeared from loaded catalog"))?;
        if install.package() != manifest.package() {
            return Ok(ExtensionGrantMutationOutcome::Invalid);
        }

        let (authority, persistence, live_owner_validated) = match write {
            ExtensionGrantWrite::Initialize { authority } => {
                if current_grant_revision.is_some() {
                    return Ok(ExtensionGrantMutationOutcome::Conflict(
                        ExtensionGrantConflict::new(
                            catalog.revision(),
                            Some(install.revision()),
                            current_grant_revision,
                        ),
                    ));
                }
                // Reconstruct the submitted authority against the exact
                // admitted manifest. This refuses stale declarations and
                // independently recomputes retained-byte accounting/digest
                // before any durable write begins.
                let Some(verified) = verify_initial_authority(install, &manifest, &authority)
                else {
                    return Ok(ExtensionGrantMutationOutcome::Invalid);
                };
                (verified, GrantPersistence::Initialize, false)
            }
            ExtensionGrantWrite::Apply { expected, mutation } => {
                let Some(current) = load_authority(&tx, install, &manifest)? else {
                    return Ok(ExtensionGrantMutationOutcome::Uninitialized);
                };
                let mutation_for_storage = mutation.clone();
                let application = match current.apply(expected, &manifest, mutation) {
                    Ok(application) => application,
                    Err(ExtensionGrantApplyError::RevisionConflict { current, .. }) => {
                        return Ok(ExtensionGrantMutationOutcome::Conflict(
                            ExtensionGrantConflict::new(
                                catalog.revision(),
                                Some(install.revision()),
                                Some(current),
                            ),
                        ));
                    }
                    Err(ExtensionGrantApplyError::RevisionExhausted) => {
                        return Ok(ExtensionGrantMutationOutcome::RevisionExhausted)
                    }
                    Err(_) => return Ok(ExtensionGrantMutationOutcome::Invalid),
                };
                let persistence = if application.changed() {
                    GrantPersistence::Apply {
                        expected,
                        mutation: mutation_for_storage,
                    }
                } else {
                    GrantPersistence::None
                };
                let authority = application.into_authority();
                (authority, persistence, false)
            }
            ExtensionGrantWrite::ApplyPatch { expected, patch } => {
                let Some(current) = load_authority(&tx, install, &manifest)? else {
                    return Ok(ExtensionGrantMutationOutcome::Uninitialized);
                };
                let application = match current.apply_patch(expected, &manifest, patch) {
                    Ok(application) => application,
                    Err(ExtensionGrantApplyError::RevisionConflict { current, .. }) => {
                        return Ok(ExtensionGrantMutationOutcome::Conflict(
                            ExtensionGrantConflict::new(
                                catalog.revision(),
                                Some(install.revision()),
                                Some(current),
                            ),
                        ));
                    }
                    Err(ExtensionGrantApplyError::RevisionExhausted) => {
                        return Ok(ExtensionGrantMutationOutcome::RevisionExhausted)
                    }
                    Err(_) => return Ok(ExtensionGrantMutationOutcome::Invalid),
                };
                let persistence = if application.changed() {
                    GrantPersistence::Patch { expected }
                } else {
                    GrantPersistence::None
                };
                let authority = application.into_authority();
                (authority, persistence, false)
            }
            ExtensionGrantWrite::ApplyLivePatch {
                expected,
                patch,
                owner,
            } => {
                if owner.key().profile() != profile
                    || owner.key().install_id() != install_id
                    || !patch.changes().iter().all(|mutation| {
                        matches!(
                            mutation,
                            ExtensionGrantMutation::SetApi { granted: true, .. }
                                | ExtensionGrantMutation::SetHost { granted: true, .. }
                        )
                    })
                {
                    return Ok(ExtensionGrantMutationOutcome::Invalid);
                }
                let Some(current) = load_authority(&tx, install, &manifest)? else {
                    return Ok(ExtensionGrantMutationOutcome::Uninitialized);
                };
                if owner.store_grant_revision() != current.revision()
                    || owner.grant_digest() != current.digest()
                    || !super::native_ownership::has_exact_sole_live_owner(
                        meta, profile, install_id, owner,
                    )?
                {
                    return Ok(ExtensionGrantMutationOutcome::RuntimeOwnershipConflict);
                }
                let application = match current.apply_patch(expected, &manifest, patch) {
                    Ok(application) => application,
                    Err(ExtensionGrantApplyError::RevisionConflict { current, .. }) => {
                        return Ok(ExtensionGrantMutationOutcome::Conflict(
                            ExtensionGrantConflict::new(
                                catalog.revision(),
                                Some(install.revision()),
                                Some(current),
                            ),
                        ));
                    }
                    Err(ExtensionGrantApplyError::RevisionExhausted) => {
                        return Ok(ExtensionGrantMutationOutcome::RevisionExhausted)
                    }
                    Err(_) => return Ok(ExtensionGrantMutationOutcome::Invalid),
                };
                let persistence = if application.changed() {
                    GrantPersistence::Patch { expected }
                } else {
                    GrantPersistence::None
                };
                (application.into_authority(), persistence, true)
            }
        };

        if matches!(persistence, GrantPersistence::None) {
            return Ok(ExtensionGrantMutationOutcome::Applied(
                ExtensionGrantMutationApplied::new(
                    catalog.revision(),
                    Box::new(install.clone()),
                    Box::new(authority),
                ),
            ));
        }

        if !live_owner_validated
            && super::native_ownership::has_unresolved_native_ownership_for_install(
                meta, profile, install_id,
            )?
        {
            return Ok(ExtensionGrantMutationOutcome::RuntimeOwnershipConflict);
        }

        match persistence {
            GrantPersistence::None => unreachable!("grant no-op returned before persistence"),
            GrantPersistence::Initialize => insert_authority(&tx, &authority)?,
            GrantPersistence::Apply { expected, mutation } => {
                persist_authority_mutation(&tx, expected, &authority, &mutation)?;
            }
            GrantPersistence::Patch { expected } => {
                persist_authority_patch(&tx, expected, &authority)?;
            }
        }

        let applied = ExtensionGrantMutationApplied::new(
            catalog.revision(),
            Box::new(install.clone()),
            Box::new(authority),
        );

        let committed = tx.commit();
        #[cfg(test)]
        if ambiguous_commit {
            if let Err(error) = committed {
                eprintln!(
                    "store: injected profile {profile} extension-grant commit ambiguity: {error}"
                );
            }
            return Ok(ExtensionGrantMutationOutcome::OutcomeUnknown);
        }
        match committed {
            Ok(()) => Ok(ExtensionGrantMutationOutcome::Applied(applied)),
            Err(error) => {
                eprintln!(
                    "store: profile {profile} extension-grant commit outcome is unknown: {error}"
                );
                Ok(ExtensionGrantMutationOutcome::OutcomeUnknown)
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn make_next_extension_grant_commit_ambiguous(&mut self) {
        self.ambiguous_extension_grant_commit_once = true;
    }
}

fn load_all_authorities(
    conn: &Connection,
    catalog: &zephium_core::extensions::ExtensionInstallCatalog,
    bindings: &ExtensionGrantManifestBindings,
) -> rusqlite::Result<Vec<ExtensionGrantAuthority>> {
    if bindings.len() != catalog.installs().len()
        || catalog
            .installs()
            .iter()
            .any(|install| bindings.get(install.id()).is_none())
    {
        return Err(invalid_data(
            "extension manifest bindings do not exactly cover install catalog",
        ));
    }
    for install in catalog.installs() {
        let manifest = bindings
            .get(install.id())
            .ok_or_else(|| invalid_data("extension manifest binding is absent"))?;
        if manifest.package() != install.package() {
            return Err(invalid_data(
                "extension manifest binding package does not match install",
            ));
        }
    }

    validate_global_grant_integrity(conn)?;

    let count = conn.query_row("SELECT count(*) FROM extension_grants", [], |row| {
        row.get::<_, i64>(0)
    })?;
    if !(0..=MAX_EXTENSION_INSTALLS_PER_PROFILE as i64).contains(&count) {
        return Err(invalid_data("extension grant row count exceeds limit"));
    }
    let mut authorities = Vec::with_capacity(count as usize);
    for install in catalog.installs() {
        let manifest = bindings
            .get(install.id())
            .ok_or_else(|| invalid_data("extension manifest binding is absent"))?;
        if let Some(authority) = load_authority(conn, install, manifest)? {
            authorities.push(authority);
        }
    }
    if authorities.len() != count as usize {
        return Err(invalid_data(
            "extension grant rows are not an exact subset of installs",
        ));
    }
    Ok(authorities)
}

pub(super) fn validate_global_grant_integrity(conn: &Connection) -> rusqlite::Result<()> {
    let (root_count, root_orphans): (i64, i64) = conn.query_row(
        "SELECT count(*),
                COALESCE(sum(CASE WHEN i.id IS NULL THEN 1 ELSE 0 END), 0)
         FROM extension_grants g
         LEFT JOIN extension_installs i ON i.id = g.install_id",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if !(0..=MAX_EXTENSION_INSTALLS_PER_PROFILE as i64).contains(&root_count) || root_orphans != 0 {
        return Err(invalid_data(
            "profile extension grant roots are not an exact bounded install subset",
        ));
    }
    let max_api_count = MAX_EXTENSION_INSTALLS_PER_PROFILE
        .checked_mul(MAX_EXTENSION_API_PERMISSIONS)
        .ok_or_else(|| invalid_data("profile extension API grant limit overflow"))?;
    let max_api_bytes = max_api_count
        .checked_mul(MAX_EXTENSION_API_PERMISSION_NAME_BYTES)
        .ok_or_else(|| invalid_data("profile extension API grant byte limit overflow"))?;
    let max_host_count = MAX_EXTENSION_INSTALLS_PER_PROFILE
        .checked_mul(MAX_EXTENSION_HOST_GRANTS)
        .ok_or_else(|| invalid_data("profile extension host grant limit overflow"))?;
    let max_host_bytes = max_host_count
        .checked_mul(MAX_MATCH_PATTERN_BYTES)
        .ok_or_else(|| invalid_data("profile extension host grant byte limit overflow"))?;

    let (api_count, api_bytes, api_orphans): (i64, i64, i64) = conn.query_row(
        "SELECT count(*), COALESCE(sum(length(CAST(p.name AS BLOB))), 0),
                COALESCE(sum(CASE WHEN g.install_id IS NULL THEN 1 ELSE 0 END), 0)
         FROM extension_grant_api_permissions p
         LEFT JOIN extension_grants g ON g.install_id = p.install_id",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    if !(0..=max_api_count as i64).contains(&api_count)
        || !(0..=max_api_bytes as i64).contains(&api_bytes)
        || api_orphans != 0
    {
        return Err(invalid_data(
            "profile extension API grant children are not exact and bounded",
        ));
    }
    let (host_count, host_bytes, host_orphans): (i64, i64, i64) = conn.query_row(
        "SELECT count(*), COALESCE(sum(length(CAST(p.pattern AS BLOB))), 0),
                COALESCE(sum(CASE WHEN g.install_id IS NULL THEN 1 ELSE 0 END), 0)
         FROM extension_grant_host_permissions p
         LEFT JOIN extension_grants g ON g.install_id = p.install_id",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    if !(0..=max_host_count as i64).contains(&host_count)
        || !(0..=max_host_bytes as i64).contains(&host_bytes)
        || host_orphans != 0
    {
        return Err(invalid_data(
            "profile extension host grant children are not exact and bounded",
        ));
    }
    Ok(())
}

fn load_grant_revision(
    conn: &Connection,
    install_id: ExtensionInstallId,
) -> rusqlite::Result<Option<ExtensionGrantRevision>> {
    let id = install_id.bytes();
    let raw = conn
        .query_row(
            "SELECT revision FROM extension_grants WHERE install_id = ?1",
            [&id[..]],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    raw.map(|revision| {
        super::extensions::revision_u64(revision)
            .and_then(ExtensionGrantRevision::new)
            .ok_or_else(|| invalid_data("extension grant revision is invalid"))
    })
    .transpose()
}

/// Loads one complete package-bound grant authority after validating the
/// profile-wide root/child-table envelope. Fresh activation uses this narrow
/// helper so Begin and MayOwn share the same durable codec and integrity
/// checks as ordinary cohort loads.
pub(super) fn load_validated_runtime_authority(
    conn: &Connection,
    install: &ExtensionInstall,
    manifest: &ExtensionManifestDescriptor,
) -> rusqlite::Result<Option<ExtensionGrantAuthority>> {
    validate_global_grant_integrity(conn)?;
    load_authority(conn, install, manifest)
}

fn load_authority(
    conn: &Connection,
    install: &ExtensionInstall,
    manifest: &ExtensionManifestDescriptor,
) -> rusqlite::Result<Option<ExtensionGrantAuthority>> {
    let id = install.id().bytes();
    let raw = conn
        .query_row(
            "SELECT
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
                 CASE WHEN typeof(grant_sha256) = 'blob' AND length(grant_sha256) = 32 THEN grant_sha256 END,
                 file_access,
                 private_access
             FROM extension_grants WHERE install_id = ?1",
            [&id[..]],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<Vec<u8>>>(1)?,
                    row.get::<_, Option<Vec<u8>>>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<Vec<u8>>>(6)?,
                    row.get::<_, Option<Vec<u8>>>(7)?,
                    row.get::<_, Option<Vec<u8>>>(8)?,
                    row.get::<_, Option<Vec<u8>>>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, i64>(11)?,
                ))
            },
        )
        .optional()?;
    let Some((
        revision,
        authority,
        key,
        package_revision,
        payload_kind,
        archive_length,
        archive_sha256,
        manifest_digest,
        tree,
        grant_digest,
        file_access,
        private_access,
    )) = raw
    else {
        return Ok(None);
    };

    let revision = super::extensions::revision_u64(revision)
        .and_then(ExtensionGrantRevision::new)
        .ok_or_else(|| invalid_data("extension grant revision is invalid"))?;
    let package = decode_package(DurableGrantPackageRow {
        authority,
        key,
        package_revision,
        payload_kind,
        archive_length,
        archive_sha256,
        manifest_digest,
        tree,
    })?;
    if &package != install.package() || &package != manifest.package() {
        return Err(invalid_data(
            "extension grant package identity does not match install and manifest",
        ));
    }
    let persisted_digest =
        ExtensionGrantDigest::from_bytes(super::extensions::exact_blob::<EXTENSION_SHA256_BYTES>(
            grant_digest,
            "extension grant digest is invalid",
        )?);
    let file_access = durable_bool(file_access, "extension file-access grant is invalid")?;
    let private_access = durable_bool(private_access, "extension private-access grant is invalid")?;
    let api_grants = load_api_grants(conn, install.id())?;
    let host_grants = load_host_grants(conn, install.id())?;
    let authority = ExtensionGrantAuthority::from_persisted(
        install,
        revision,
        package,
        api_grants,
        host_grants,
        file_access,
        private_access,
        manifest,
    )
    .map_err(|_| invalid_data("extension grant authority is invalid"))?;
    if authority.digest() != persisted_digest {
        return Err(invalid_data(
            "extension grant digest does not match authority",
        ));
    }
    Ok(Some(authority))
}

struct DurableGrantPackageRow {
    authority: Option<Vec<u8>>,
    key: Option<Vec<u8>>,
    package_revision: i64,
    payload_kind: Option<i64>,
    archive_length: Option<i64>,
    archive_sha256: Option<Vec<u8>>,
    manifest_digest: Option<Vec<u8>>,
    tree: Option<Vec<u8>>,
}

fn decode_package(row: DurableGrantPackageRow) -> rusqlite::Result<ExtensionPackageIdentity> {
    let authority =
        ExtensionAuthorityId::from_bytes(super::extensions::exact_blob::<EXTENSION_SHA256_BYTES>(
            row.authority,
            "extension grant authority id is invalid",
        )?);
    let key = ExtensionPackageKey::from_bytes(super::extensions::exact_blob::<
        EXTENSION_SHA256_BYTES,
    >(
        row.key, "extension grant package key is invalid"
    )?);
    let revision = super::extensions::revision_u64(row.package_revision)
        .and_then(ExtensionPackageRevision::new)
        .ok_or_else(|| invalid_data("extension grant package revision is invalid"))?;
    let payload = super::extensions::decode_package_payload(
        row.payload_kind,
        row.archive_length,
        row.archive_sha256,
        "extension grant package payload identity is invalid",
    )?;
    let manifest = ExtensionManifestDigest::from_bytes(super::extensions::exact_blob::<
        EXTENSION_SHA256_BYTES,
    >(
        row.manifest_digest,
        "extension grant manifest digest is invalid",
    )?);
    let tree =
        ExtensionTreeDigest::from_bytes(super::extensions::exact_blob::<EXTENSION_SHA256_BYTES>(
            row.tree,
            "extension grant tree digest is invalid",
        )?);
    Ok(ExtensionPackageIdentity::new(
        authority, key, revision, payload, manifest, tree,
    ))
}

fn durable_bool(value: i64, message: &'static str) -> rusqlite::Result<bool> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(invalid_data(message)),
    }
}

fn load_api_grants(
    conn: &Connection,
    install_id: ExtensionInstallId,
) -> rusqlite::Result<Vec<ApiPermissionName>> {
    let id = install_id.bytes();
    let (count, bytes): (i64, i64) = conn.query_row(
        "SELECT count(*), COALESCE(sum(length(CAST(name AS BLOB))), 0)
         FROM extension_grant_api_permissions WHERE install_id = ?1",
        [&id[..]],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let max_bytes = MAX_EXTENSION_API_PERMISSIONS
        .checked_mul(MAX_EXTENSION_API_PERMISSION_NAME_BYTES)
        .ok_or_else(|| invalid_data("extension API grant byte limit overflow"))?;
    if !(0..=MAX_EXTENSION_API_PERMISSIONS as i64).contains(&count)
        || !(0..=max_bytes as i64).contains(&bytes)
    {
        return Err(invalid_data("extension API grants exceed durable limits"));
    }
    let mut statement = conn.prepare(
        "SELECT CASE
                    WHEN length(CAST(name AS BLOB)) BETWEEN 1 AND ?2 THEN name
                END
         FROM extension_grant_api_permissions
         WHERE install_id = ?1 ORDER BY name",
    )?;
    let rows = statement.query_map(
        params![&id[..], MAX_EXTENSION_API_PERMISSION_NAME_BYTES],
        |row| row.get::<_, Option<String>>(0),
    )?;
    let mut grants = Vec::with_capacity(count as usize);
    for row in rows {
        let raw = row?.ok_or_else(|| invalid_data("extension API grant is invalid"))?;
        let grant = ApiPermissionName::parse_exact(&raw)
            .map_err(|_| invalid_data("extension API grant is invalid"))?;
        if grant.as_str() != raw {
            return Err(invalid_data("extension API grant is not canonical"));
        }
        grants.push(grant);
    }
    if grants.len() != count as usize {
        return Err(invalid_data("extension API grants changed while loading"));
    }
    Ok(grants)
}

fn load_host_grants(
    conn: &Connection,
    install_id: ExtensionInstallId,
) -> rusqlite::Result<Vec<MatchPattern>> {
    let id = install_id.bytes();
    let (count, bytes): (i64, i64) = conn.query_row(
        "SELECT count(*), COALESCE(sum(length(CAST(pattern AS BLOB))), 0)
         FROM extension_grant_host_permissions WHERE install_id = ?1",
        [&id[..]],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let max_bytes = MAX_EXTENSION_HOST_GRANTS
        .checked_mul(MAX_MATCH_PATTERN_BYTES)
        .ok_or_else(|| invalid_data("extension host grant byte limit overflow"))?;
    if !(0..=MAX_EXTENSION_HOST_GRANTS as i64).contains(&count)
        || !(0..=max_bytes as i64).contains(&bytes)
    {
        return Err(invalid_data("extension host grants exceed durable limits"));
    }
    let mut statement = conn.prepare(
        "SELECT CASE
                    WHEN length(CAST(pattern AS BLOB)) BETWEEN 1 AND ?2 THEN pattern
                END
         FROM extension_grant_host_permissions
         WHERE install_id = ?1 ORDER BY pattern",
    )?;
    let rows = statement.query_map(params![&id[..], MAX_MATCH_PATTERN_BYTES], |row| {
        row.get::<_, Option<String>>(0)
    })?;
    let mut grants = Vec::with_capacity(count as usize);
    for row in rows {
        let raw = row?.ok_or_else(|| invalid_data("extension host grant is invalid"))?;
        let grant = MatchPattern::parse(&raw)
            .map_err(|_| invalid_data("extension host grant is invalid"))?;
        if grant.as_str() != raw {
            return Err(invalid_data("extension host grant is not canonical"));
        }
        grants.push(grant);
    }
    if grants.len() != count as usize {
        return Err(invalid_data("extension host grants changed while loading"));
    }
    Ok(grants)
}

/// Rebuilds a submitted initial authority against the exact installed row and
/// admitted manifest. The reconstruction independently checks declarations,
/// digest, canonical ordering, and retained-memory accounting before a write.
pub(super) fn verify_initial_authority(
    install: &ExtensionInstall,
    manifest: &ExtensionManifestDescriptor,
    authority: &ExtensionGrantAuthority,
) -> Option<ExtensionGrantAuthority> {
    let projection = authority.persistence_projection();
    if projection.install_id() != install.id()
        || projection.revision() != ExtensionGrantRevision::INITIAL
        || projection.package() != install.package()
        || manifest.package() != install.package()
    {
        return None;
    }
    ExtensionGrantAuthority::from_persisted(
        install,
        ExtensionGrantRevision::INITIAL,
        projection.package().clone(),
        projection.api_grants().cloned().collect(),
        projection.host_grants().cloned().collect(),
        projection.persisted_file_access(),
        projection.persisted_private_access(),
        manifest,
    )
    .ok()
    .filter(|verified| verified == authority)
}

pub(super) fn insert_authority(
    conn: &Connection,
    authority: &ExtensionGrantAuthority,
) -> rusqlite::Result<()> {
    let projection = authority.persistence_projection();
    let id = projection.install_id().bytes();
    let package = projection.package();
    let package_authority = package.authority().bytes();
    let key = package.key().bytes();
    let payload = super::extensions::encode_package_payload(package.payload())?;
    let manifest = package.manifest_sha256().bytes();
    let tree = package.tree_sha256().bytes();
    let digest = projection.digest().bytes();
    let inserted = conn.execute(
        "INSERT INTO extension_grants(
             install_id, revision, authority, package_key, package_revision,
             payload_kind, archive_length, archive_sha256,
             manifest_sha256, tree_sha256, grant_sha256,
             file_access, private_access
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            &id[..],
            super::extensions::revision_i64(projection.revision().get())?,
            &package_authority[..],
            &key[..],
            super::extensions::revision_i64(package.revision().get())?,
            payload.kind,
            payload.archive_length,
            payload.archive_sha256.as_ref().map(|digest| &digest[..]),
            &manifest[..],
            &tree[..],
            &digest[..],
            i64::from(projection.persisted_file_access()),
            i64::from(projection.persisted_private_access()),
        ],
    )?;
    if inserted != 1 {
        return Err(invalid_data(
            "extension grant authority was not inserted exactly once",
        ));
    }
    insert_authority_members(conn, authority, "initialized")
}

fn insert_authority_members(
    conn: &Connection,
    authority: &ExtensionGrantAuthority,
    operation: &str,
) -> rusqlite::Result<()> {
    let projection = authority.persistence_projection();
    let id = projection.install_id().bytes();
    for name in projection.api_grants() {
        let inserted = conn.execute(
            "INSERT INTO extension_grant_api_permissions(install_id, name)
             VALUES (?1, ?2)",
            params![&id[..], name.as_str()],
        )?;
        if inserted != 1 {
            return Err(invalid_data(&format!(
                "extension API grant was not {operation} exactly once"
            )));
        }
    }
    for pattern in projection.host_grants() {
        let inserted = conn.execute(
            "INSERT INTO extension_grant_host_permissions(install_id, pattern)
             VALUES (?1, ?2)",
            params![&id[..], pattern.as_str()],
        )?;
        if inserted != 1 {
            return Err(invalid_data(&format!(
                "extension host grant was not {operation} exactly once"
            )));
        }
    }
    Ok(())
}

fn persist_authority_patch(
    conn: &Connection,
    expected: ExtensionGrantRevision,
    authority: &ExtensionGrantAuthority,
) -> rusqlite::Result<()> {
    let projection = authority.persistence_projection();
    let id = projection.install_id().bytes();
    let digest = projection.digest().bytes();
    let updated = conn.execute(
        "UPDATE extension_grants
         SET revision = ?3, grant_sha256 = ?4,
             file_access = ?5, private_access = ?6
         WHERE install_id = ?1 AND revision = ?2",
        params![
            &id[..],
            super::extensions::revision_i64(expected.get())?,
            super::extensions::revision_i64(projection.revision().get())?,
            &digest[..],
            i64::from(projection.persisted_file_access()),
            i64::from(projection.persisted_private_access()),
        ],
    )?;
    if updated != 1 {
        return Err(invalid_data(
            "extension grant patch changed during compare-and-swap",
        ));
    }
    let deleted_api = conn.execute(
        "DELETE FROM extension_grant_api_permissions WHERE install_id = ?1",
        [&id[..]],
    )?;
    let deleted_hosts = conn.execute(
        "DELETE FROM extension_grant_host_permissions WHERE install_id = ?1",
        [&id[..]],
    )?;
    if deleted_api > MAX_EXTENSION_API_PERMISSIONS || deleted_hosts > MAX_EXTENSION_HOST_GRANTS {
        return Err(invalid_data(
            "extension grant patch replaced an over-limit authority",
        ));
    }
    insert_authority_members(conn, authority, "replaced")
}

fn persist_authority_mutation(
    conn: &Connection,
    expected: ExtensionGrantRevision,
    authority: &ExtensionGrantAuthority,
    mutation: &ExtensionGrantMutation,
) -> rusqlite::Result<()> {
    let id = authority.install_id().bytes();
    match mutation {
        ExtensionGrantMutation::SetApi { name, granted } => {
            let changed = if *granted {
                conn.execute(
                    "INSERT INTO extension_grant_api_permissions(install_id, name)
                     VALUES (?1, ?2)",
                    params![&id[..], name.as_str()],
                )?
            } else {
                conn.execute(
                    "DELETE FROM extension_grant_api_permissions
                     WHERE install_id = ?1 AND name = ?2",
                    params![&id[..], name.as_str()],
                )?
            };
            if changed != 1 {
                return Err(invalid_data(
                    "extension API grant did not change exactly once",
                ));
            }
        }
        ExtensionGrantMutation::SetHost { pattern, granted } => {
            let changed = if *granted {
                conn.execute(
                    "INSERT INTO extension_grant_host_permissions(install_id, pattern)
                     VALUES (?1, ?2)",
                    params![&id[..], pattern.as_str()],
                )?
            } else {
                conn.execute(
                    "DELETE FROM extension_grant_host_permissions
                     WHERE install_id = ?1 AND pattern = ?2",
                    params![&id[..], pattern.as_str()],
                )?
            };
            if changed != 1 {
                return Err(invalid_data(
                    "extension host grant did not change exactly once",
                ));
            }
        }
        ExtensionGrantMutation::SetFileAccess { .. }
        | ExtensionGrantMutation::SetPrivateAccess { .. } => {}
    }

    let projection = authority.persistence_projection();
    let digest = projection.digest().bytes();
    let updated = conn.execute(
        "UPDATE extension_grants
         SET revision = ?3, grant_sha256 = ?4,
             file_access = ?5, private_access = ?6
         WHERE install_id = ?1 AND revision = ?2",
        params![
            &id[..],
            super::extensions::revision_i64(expected.get())?,
            super::extensions::revision_i64(projection.revision().get())?,
            &digest[..],
            i64::from(projection.persisted_file_access()),
            i64::from(projection.persisted_private_access()),
        ],
    )?;
    if updated != 1 {
        return Err(invalid_data(
            "extension grant changed during compare-and-swap",
        ));
    }
    Ok(())
}
