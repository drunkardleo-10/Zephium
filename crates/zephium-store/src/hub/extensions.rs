//! Exact, per-profile extension-install persistence.
//!
//! This adapter is deliberately only a strict durable codec and atomic CAS
//! boundary. The core aggregate owns every transition. Stored package
//! identity remains structural data, never authentication or activation
//! authority.

use super::*;

use zephium_core::extensions::{
    ExtensionArchiveDigest, ExtensionAuthorityId, ExtensionInstall, ExtensionInstallCatalog,
    ExtensionInstallCatalogApplyError, ExtensionInstallCatalogMutation,
    ExtensionInstallCatalogRevision, ExtensionInstallRevision, ExtensionManifestDigest,
    ExtensionPackageIdentity, ExtensionPackageKey, ExtensionPackageRevision, ExtensionTreeDigest,
    EXTENSION_SHA256_BYTES, MAX_EXTENSION_INSTALLS_PER_PROFILE,
};
use zephium_core::ids::ExtensionInstallId;
use zephium_core::ports::store::{
    ExtensionInstallCatalogLoadOutcome, ExtensionInstallCatalogMutationApplied,
    ExtensionInstallCatalogMutationOutcome,
};

impl Hub {
    pub(crate) fn load_extension_install_catalog(
        &mut self,
        profile: ProfileId,
    ) -> rusqlite::Result<ExtensionInstallCatalogLoadOutcome> {
        if !self.registry.contains(&profile) {
            return Ok(ExtensionInstallCatalogLoadOutcome::NotRegistered);
        }
        if self.degraded_profiles.contains(&profile) {
            return Ok(ExtensionInstallCatalogLoadOutcome::DegradedProfile);
        }
        let tx = self.profile_conn(profile)?.transaction()?;
        let catalog = load_catalog(&tx)?;
        tx.commit()?;
        Ok(ExtensionInstallCatalogLoadOutcome::Loaded(catalog))
    }

    pub(crate) fn mutate_extension_install_catalog(
        &mut self,
        profile: ProfileId,
        expected: ExtensionInstallCatalogRevision,
        mutation: ExtensionInstallCatalogMutation,
    ) -> rusqlite::Result<ExtensionInstallCatalogMutationOutcome> {
        if self.recovery_required.is_some() {
            return Err(invalid_data("session recovery mode is read-only"));
        }
        if !self.registry.contains(&profile) {
            return Ok(ExtensionInstallCatalogMutationOutcome::NotRegistered);
        }
        if self.degraded_profiles.contains(&profile) {
            return Ok(ExtensionInstallCatalogMutationOutcome::DegradedProfile);
        }
        #[cfg(test)]
        let ambiguous_commit = std::mem::take(&mut self.ambiguous_extension_install_commit_once);

        let conn = self.profile_conn(profile)?;
        let tx = conn.transaction()?;
        let current = load_catalog(&tx)?;
        let current_revision = current.revision();
        let application = match current.apply(expected, mutation.clone()) {
            Ok(application) => application,
            Err(
                ExtensionInstallCatalogApplyError::CatalogRevisionConflict { .. }
                | ExtensionInstallCatalogApplyError::InstallRevisionConflict { .. },
            ) => {
                return Ok(ExtensionInstallCatalogMutationOutcome::Conflict {
                    current: current_revision,
                })
            }
            Err(ExtensionInstallCatalogApplyError::LimitReached { .. }) => {
                return Ok(ExtensionInstallCatalogMutationOutcome::LimitReached)
            }
            Err(
                ExtensionInstallCatalogApplyError::CatalogRevisionExhausted
                | ExtensionInstallCatalogApplyError::InstallRevisionExhausted { .. },
            ) => return Ok(ExtensionInstallCatalogMutationOutcome::RevisionExhausted),
            Err(
                ExtensionInstallCatalogApplyError::InstallAlreadyExists(_)
                | ExtensionInstallCatalogApplyError::InstallNotFound(_)
                | ExtensionInstallCatalogApplyError::PackageAlreadyInstalled { .. }
                | ExtensionInstallCatalogApplyError::CatalogRejected(_),
            ) => return Ok(ExtensionInstallCatalogMutationOutcome::Invalid),
        };

        let applied = ExtensionInstallCatalogMutationApplied {
            catalog_revision: application.catalog().revision(),
            install: application.install().cloned().map(Box::new),
        };
        if !application.changed() {
            return Ok(ExtensionInstallCatalogMutationOutcome::Applied(applied));
        }

        match &mutation {
            ExtensionInstallCatalogMutation::Install { id, .. } => {
                let install = application
                    .install()
                    .ok_or_else(|| invalid_data("extension install transition has no row"))?;
                if install.id() != *id {
                    return Err(invalid_data(
                        "extension install transition changed identity",
                    ));
                }
                insert_install(&tx, install)?;
            }
            ExtensionInstallCatalogMutation::SetDesiredEnabled { id, expected, .. } => {
                let install = application
                    .install()
                    .ok_or_else(|| invalid_data("extension enablement transition has no row"))?;
                let id_bytes = id.bytes();
                let updated = tx.execute(
                    "UPDATE extension_installs
                     SET revision = ?3, desired_enabled = ?4
                     WHERE id = ?1 AND revision = ?2",
                    params![
                        &id_bytes[..],
                        revision_i64(expected.get())?,
                        revision_i64(install.revision().get())?,
                        i64::from(install.desired_enabled()),
                    ],
                )?;
                if updated != 1 {
                    return Err(invalid_data(
                        "extension install changed during enablement compare-and-swap",
                    ));
                }
            }
            ExtensionInstallCatalogMutation::Delete { id, expected } => {
                if application.install().is_some() {
                    return Err(invalid_data("extension deletion retained its row"));
                }
                let id_bytes = id.bytes();
                let deleted = tx.execute(
                    "DELETE FROM extension_installs WHERE id = ?1 AND revision = ?2",
                    params![&id_bytes[..], revision_i64(expected.get())?],
                )?;
                if deleted != 1 {
                    return Err(invalid_data(
                        "extension install changed during delete compare-and-swap",
                    ));
                }
            }
        }

        let catalog_updated = tx.execute(
            "UPDATE extension_install_catalog SET revision = ?2
             WHERE id = 1 AND revision = ?1",
            params![
                revision_i64(current_revision.get())?,
                revision_i64(applied.catalog_revision.get())?,
            ],
        )?;
        if catalog_updated != 1 {
            return Err(invalid_data(
                "extension install catalog changed during compare-and-swap",
            ));
        }

        let committed = tx.commit();
        #[cfg(test)]
        if ambiguous_commit {
            if let Err(error) = committed {
                eprintln!(
                    "store: injected profile {profile} extension-install commit ambiguity: {error}"
                );
            }
            return Ok(ExtensionInstallCatalogMutationOutcome::OutcomeUnknown);
        }
        match committed {
            Ok(()) => Ok(ExtensionInstallCatalogMutationOutcome::Applied(applied)),
            Err(error) => {
                eprintln!(
                    "store: profile {profile} extension-install catalog commit outcome is unknown: {error}"
                );
                Ok(ExtensionInstallCatalogMutationOutcome::OutcomeUnknown)
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn make_next_extension_install_commit_ambiguous(&mut self) {
        self.ambiguous_extension_install_commit_once = true;
    }
}

fn insert_install(conn: &Connection, install: &ExtensionInstall) -> rusqlite::Result<()> {
    let id = install.id().bytes();
    let package = install.package();
    let authority = package.authority().bytes();
    let key = package.key().bytes();
    let archive = package.archive_sha256().bytes();
    let manifest = package.manifest_sha256().bytes();
    let tree = package.tree_sha256().bytes();
    let inserted = conn.execute(
        "INSERT INTO extension_installs(
             id, revision, authority, package_key, package_revision,
             archive_sha256, manifest_sha256, tree_sha256, desired_enabled
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            &id[..],
            revision_i64(install.revision().get())?,
            &authority[..],
            &key[..],
            revision_i64(package.revision().get())?,
            &archive[..],
            &manifest[..],
            &tree[..],
            i64::from(install.desired_enabled()),
        ],
    )?;
    if inserted != 1 {
        return Err(invalid_data(
            "extension install row was not inserted exactly once",
        ));
    }
    Ok(())
}

fn load_catalog(conn: &Connection) -> rusqlite::Result<ExtensionInstallCatalog> {
    let (state_rows, raw_revision): (i64, Option<i64>) = conn.query_row(
        "SELECT count(*), CASE WHEN count(*) = 1 THEN max(revision) END
         FROM extension_install_catalog",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if state_rows != 1 {
        return Err(invalid_data(
            "extension install catalog has no unique revision authority",
        ));
    }
    let revision = raw_revision
        .and_then(revision_u64)
        .and_then(ExtensionInstallCatalogRevision::new)
        .ok_or_else(|| invalid_data("extension install catalog revision is invalid"))?;

    let count = conn.query_row("SELECT count(*) FROM extension_installs", [], |row| {
        row.get::<_, i64>(0)
    })?;
    if !(0..=MAX_EXTENSION_INSTALLS_PER_PROFILE as i64).contains(&count) {
        return Err(invalid_data(
            "extension install catalog exceeds install limit",
        ));
    }

    // CASE guards ensure SQLite never materializes an attacker-sized BLOB
    // into Rust before the exact fixed-size codec rejects the whole catalog.
    let mut statement = conn.prepare(
        "SELECT
             CASE WHEN typeof(id) = 'blob' AND length(id) = 16 THEN id END,
             revision,
             CASE WHEN typeof(authority) = 'blob' AND length(authority) = 32 THEN authority END,
             CASE WHEN typeof(package_key) = 'blob' AND length(package_key) = 32 THEN package_key END,
             package_revision,
             CASE WHEN typeof(archive_sha256) = 'blob' AND length(archive_sha256) = 32 THEN archive_sha256 END,
             CASE WHEN typeof(manifest_sha256) = 'blob' AND length(manifest_sha256) = 32 THEN manifest_sha256 END,
             CASE WHEN typeof(tree_sha256) = 'blob' AND length(tree_sha256) = 32 THEN tree_sha256 END,
             desired_enabled
         FROM extension_installs ORDER BY id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, Option<Vec<u8>>>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, Option<Vec<u8>>>(2)?,
            row.get::<_, Option<Vec<u8>>>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, Option<Vec<u8>>>(5)?,
            row.get::<_, Option<Vec<u8>>>(6)?,
            row.get::<_, Option<Vec<u8>>>(7)?,
            row.get::<_, i64>(8)?,
        ))
    })?;
    let mut installs = Vec::with_capacity(count as usize);
    for row in rows {
        let (
            id,
            install_revision,
            authority,
            key,
            package_revision,
            archive,
            manifest,
            tree,
            desired_enabled,
        ) = row?;
        let id = exact_blob::<16>(id, "extension install id is invalid")?;
        let id = ExtensionInstallId::from(u128::from_be_bytes(id));
        let install_revision = revision_u64(install_revision)
            .and_then(ExtensionInstallRevision::new)
            .ok_or_else(|| invalid_data("extension install revision is invalid"))?;
        let authority = ExtensionAuthorityId::from_bytes(exact_blob::<EXTENSION_SHA256_BYTES>(
            authority,
            "extension authority id is invalid",
        )?);
        let key = ExtensionPackageKey::from_bytes(exact_blob::<EXTENSION_SHA256_BYTES>(
            key,
            "extension package key is invalid",
        )?);
        let package_revision = revision_u64(package_revision)
            .and_then(ExtensionPackageRevision::new)
            .ok_or_else(|| invalid_data("extension package revision is invalid"))?;
        let archive = ExtensionArchiveDigest::from_bytes(exact_blob::<EXTENSION_SHA256_BYTES>(
            archive,
            "extension archive digest is invalid",
        )?);
        let manifest = ExtensionManifestDigest::from_bytes(exact_blob::<EXTENSION_SHA256_BYTES>(
            manifest,
            "extension manifest digest is invalid",
        )?);
        let tree = ExtensionTreeDigest::from_bytes(exact_blob::<EXTENSION_SHA256_BYTES>(
            tree,
            "extension tree digest is invalid",
        )?);
        let desired_enabled = match desired_enabled {
            0 => false,
            1 => true,
            _ => return Err(invalid_data("extension desired-enabled value is invalid")),
        };
        let package = ExtensionPackageIdentity::new(
            authority,
            key,
            package_revision,
            archive,
            manifest,
            tree,
        );
        installs.push(ExtensionInstall::from_persisted(
            id,
            install_revision,
            package,
            desired_enabled,
        ));
    }
    if installs.len() != count as usize {
        return Err(invalid_data(
            "extension install catalog changed while loading",
        ));
    }
    ExtensionInstallCatalog::new(revision, installs)
        .map_err(|_| invalid_data("extension install catalog is invalid"))
}

fn exact_blob<const N: usize>(
    value: Option<Vec<u8>>,
    message: &'static str,
) -> rusqlite::Result<[u8; N]> {
    value
        .ok_or_else(|| invalid_data(message))?
        .try_into()
        .map_err(|_| invalid_data(message))
}

fn revision_u64(value: i64) -> Option<u64> {
    u64::try_from(value).ok().filter(|revision| *revision != 0)
}

fn revision_i64(value: u64) -> rusqlite::Result<i64> {
    i64::try_from(value).map_err(|_| invalid_data("extension revision overflow"))
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

    struct RawInstall {
        id: Vec<u8>,
        install_revision: i64,
        authority: Vec<u8>,
        key: Vec<u8>,
        package_revision: i64,
        archive: Vec<u8>,
        manifest: Vec<u8>,
        tree: Vec<u8>,
        desired_enabled: i64,
    }

    impl RawInstall {
        fn valid(id: u128, authority: u8, key: u8) -> Self {
            Self {
                id: id.to_be_bytes().to_vec(),
                install_revision: 1,
                authority: vec![authority; EXTENSION_SHA256_BYTES],
                key: vec![key; EXTENSION_SHA256_BYTES],
                package_revision: 1,
                archive: vec![2; EXTENSION_SHA256_BYTES],
                manifest: vec![3; EXTENSION_SHA256_BYTES],
                tree: vec![4; EXTENSION_SHA256_BYTES],
                desired_enabled: 0,
            }
        }
    }

    fn insert_raw(conn: &Connection, row: RawInstall) {
        conn.execute(
            "INSERT INTO extension_installs(
                 id, revision, authority, package_key, package_revision,
                 archive_sha256, manifest_sha256, tree_sha256, desired_enabled
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                row.id,
                row.install_revision,
                row.authority,
                row.key,
                row.package_revision,
                row.archive,
                row.manifest,
                row.tree,
                row.desired_enabled,
            ],
        )
        .unwrap();
    }

    #[test]
    fn load_rejects_every_malformed_fixed_width_or_revision_field() {
        #[derive(Clone, Copy)]
        enum Corruption {
            Id,
            InstallRevision,
            Authority,
            Key,
            PackageRevision,
            Archive,
            Manifest,
            Tree,
            DesiredEnabled,
        }

        for corruption in [
            Corruption::Id,
            Corruption::InstallRevision,
            Corruption::Authority,
            Corruption::Key,
            Corruption::PackageRevision,
            Corruption::Archive,
            Corruption::Manifest,
            Corruption::Tree,
            Corruption::DesiredEnabled,
        ] {
            let conn = database();
            conn.pragma_update(None, "ignore_check_constraints", true)
                .unwrap();
            let mut row = RawInstall::valid(1, 2, 3);
            match corruption {
                Corruption::Id => row.id.push(1),
                Corruption::InstallRevision => row.install_revision = 0,
                Corruption::Authority => row.authority.truncate(31),
                Corruption::Key => row.key.push(3),
                Corruption::PackageRevision => row.package_revision = 0,
                Corruption::Archive => row.archive.truncate(31),
                Corruption::Manifest => row.manifest.push(5),
                Corruption::Tree => row.tree.truncate(31),
                Corruption::DesiredEnabled => row.desired_enabled = 2,
            }
            insert_raw(&conn, row);
            assert!(load_catalog(&conn).is_err());
        }
    }

    #[test]
    fn load_rejects_missing_or_ambiguous_catalog_authority() {
        for extra_authority in [false, true] {
            let conn = database();
            conn.pragma_update(None, "ignore_check_constraints", true)
                .unwrap();
            if extra_authority {
                conn.execute(
                    "INSERT INTO extension_install_catalog(id, revision) VALUES (2, 1)",
                    [],
                )
                .unwrap();
            } else {
                conn.execute("DELETE FROM extension_install_catalog", [])
                    .unwrap();
            }
            assert!(load_catalog(&conn).is_err());
        }
    }

    #[test]
    fn load_rejects_more_than_the_bounded_install_limit_before_decoding() {
        let conn = database();
        for index in 0..=MAX_EXTENSION_INSTALLS_PER_PROFILE {
            insert_raw(
                &conn,
                RawInstall::valid(index as u128 + 1, index as u8, index as u8 + 1),
            );
        }
        assert!(load_catalog(&conn).is_err());
    }
}
