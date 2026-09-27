//! Provenance joins and monotonic upstream history in the profile transaction.

use rusqlite::{params, Connection, OptionalExtension};
use zephium_core::extensions::{
    ExtensionInstallProvenance, ExtensionManifestDescriptor, ExtensionUpstreamCheckpoint,
    ExtensionUpstreamUpdateDisposition, EXTENSION_UPSTREAM_CHECKPOINT_BYTES,
    MAX_EXTENSION_INSTALL_PROVENANCE_BYTES,
};
use zephium_core::ids::ExtensionInstallId;

use super::invalid_data;

impl super::Hub {
    pub(crate) fn load_extension_install_provenance(
        &mut self,
        profile: zephium_core::ids::ProfileId,
        install: ExtensionInstallId,
    ) -> rusqlite::Result<zephium_core::ports::store::ExtensionInstallProvenanceLoadOutcome> {
        use zephium_core::ports::store::ExtensionInstallProvenanceLoadOutcome as Outcome;
        if !self.registry.contains(&profile) {
            return Ok(Outcome::NotRegistered);
        }
        if self.degraded_profiles.contains(&profile) {
            return Ok(Outcome::DegradedProfile);
        }
        let tx = self.profile_conn(profile)?.transaction()?;
        let value = load(&tx, install)?;
        tx.commit()?;
        Ok(Outcome::Loaded(value.map(Box::new)))
    }
    pub(crate) fn load_extension_upstream_checkpoint(
        &mut self,
        profile: zephium_core::ids::ProfileId,
        publisher: zephium_core::extensions::ExtensionPackageKey,
    ) -> rusqlite::Result<zephium_core::ports::store::ExtensionUpstreamCheckpointLoadOutcome> {
        use zephium_core::ports::store::ExtensionUpstreamCheckpointLoadOutcome as Outcome;
        if !self.registry.contains(&profile) {
            return Ok(Outcome::NotRegistered);
        }
        if self.degraded_profiles.contains(&profile) {
            return Ok(Outcome::DegradedProfile);
        }
        let tx = self.profile_conn(profile)?.transaction()?;
        let value = checkpoint(&tx, publisher.as_bytes())?;
        tx.commit()?;
        Ok(Outcome::Loaded(value))
    }
}

pub(super) fn validate_integrity(conn: &Connection) -> rusqlite::Result<()> {
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM (SELECT 1 FROM extension_install_provenance LIMIT 9)",
        [],
        |row| row.get(0),
    )?;
    if count > zephium_core::extensions::MAX_EXTENSION_INSTALLS_PER_PROFILE as i64 {
        return Err(invalid_data("extension provenance capacity exceeded"));
    }
    let orphan: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM extension_install_provenance p
         WHERE NOT EXISTS(SELECT 1 FROM extension_installs i WHERE i.id = p.install_id))",
        [],
        |row| row.get(0),
    )?;
    if orphan {
        return Err(invalid_data("extension provenance has no installation"));
    }
    Ok(())
}

pub(super) fn load(
    conn: &Connection,
    install: ExtensionInstallId,
) -> rusqlite::Result<Option<ExtensionInstallProvenance>> {
    let bytes: Option<Option<Vec<u8>>> = conn.query_row(
        "SELECT CASE WHEN typeof(provenance) = 'blob' AND length(provenance) BETWEEN 1 AND 1024 THEN provenance END
         FROM extension_install_provenance WHERE install_id = ?1",
        [&install.bytes()[..]], |row| row.get(0),
    ).optional()?;
    bytes
        .map(|bytes| {
            let value = bytes
                .as_deref()
                .and_then(ExtensionInstallProvenance::decode)
                .ok_or_else(|| invalid_data("extension provenance is malformed"))?;
            validate_high_water(conn, &value)?;
            Ok(value)
        })
        .transpose()
}

fn checkpoint(
    conn: &Connection,
    publisher: &[u8; 32],
) -> rusqlite::Result<Option<ExtensionUpstreamCheckpoint>> {
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM (SELECT 1 FROM extension_upstream_history LIMIT 129)",
        [],
        |row| row.get(0),
    )?;
    if count > zephium_core::extensions::MAX_EXTENSION_UPSTREAM_HISTORY_PER_PROFILE as i64 {
        return Err(invalid_data("extension upstream history capacity exceeded"));
    }
    let bytes: Option<Option<Vec<u8>>> = conn.query_row(
        "SELECT CASE WHEN typeof(checkpoint) = 'blob' AND length(checkpoint) = 105 THEN checkpoint END
         FROM extension_upstream_history WHERE publisher = ?1", [&publisher[..]], |row| row.get(0),
    ).optional()?;
    bytes
        .map(|bytes| {
            let value = bytes
                .as_deref()
                .and_then(ExtensionUpstreamCheckpoint::decode)
                .filter(|value| value.publisher().as_bytes() == publisher)
                .ok_or_else(|| invalid_data("extension upstream checkpoint is malformed"))?;
            Ok(value)
        })
        .transpose()
}

fn validate_high_water(
    conn: &Connection,
    provenance: &ExtensionInstallProvenance,
) -> rusqlite::Result<()> {
    let current = provenance.upstream();
    let high_water = checkpoint(conn, current.publisher().as_bytes())?
        .ok_or_else(|| invalid_data("extension provenance has no upstream high-water mark"))?;
    // A retained rollback artifact can be below the maximum, but may not
    // change bytes at the maximum or claim a version above the durable mark.
    if !matches!(
        high_water.classify(current),
        ExtensionUpstreamUpdateDisposition::Unchanged
            | ExtensionUpstreamUpdateDisposition::Rollback
    ) {
        return Err(invalid_data(
            "extension provenance disagrees with upstream high-water mark",
        ));
    }
    Ok(())
}

/// Compares complete independently reauthenticated provenance in the same
/// snapshot as install/grants. Missing evidence cannot masquerade as legacy.
pub(super) fn matches(
    conn: &Connection,
    install: ExtensionInstallId,
    expected: Option<&ExtensionInstallProvenance>,
) -> rusqlite::Result<bool> {
    Ok(load(conn, install)?.as_ref() == expected)
}

/// Reports a deliberate capacity refusal before an install is written. The
/// SQLite trigger independently enforces the same ceiling inside the transaction.
pub(super) fn history_has_capacity(
    conn: &Connection,
    value: &ExtensionInstallProvenance,
) -> rusqlite::Result<bool> {
    if checkpoint(conn, value.upstream().publisher().as_bytes())?.is_some() {
        return Ok(true);
    }
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM (SELECT 1 FROM extension_upstream_history LIMIT 129)",
        [],
        |row| row.get(0),
    )?;
    Ok(count < zephium_core::extensions::MAX_EXTENSION_UPSTREAM_HISTORY_PER_PROFILE as i64)
}

/// Secondary check used by native Begin/MayOwn and optional-grant updates.
/// Full source-byte reauthentication belongs to the repository/Beta authority.
pub(super) fn validate_manifest(
    conn: &Connection,
    install: ExtensionInstallId,
    manifest: &ExtensionManifestDescriptor,
) -> rusqlite::Result<()> {
    let provenance = load(conn, install)?;
    if (zephium_core::extensions::is_beta_extension_authority(manifest.package().authority())
        && provenance.is_none())
        || provenance.is_some_and(|value| !value.matches_manifest(manifest))
    {
        return Err(invalid_data(
            "extension provenance disagrees with runtime manifest",
        ));
    }
    Ok(())
}

/// Writes only after package/grant validation, within the caller's uncommitted
/// profile transaction. No standalone provenance mutation is exposed.
pub(super) fn persist(
    conn: &Connection,
    install: ExtensionInstallId,
    value: &ExtensionInstallProvenance,
) -> rusqlite::Result<bool> {
    let candidate = value.upstream();
    let previous = checkpoint(conn, candidate.publisher().as_bytes())?;
    if previous.is_some_and(|previous| {
        !matches!(
            previous.classify(candidate),
            ExtensionUpstreamUpdateDisposition::Advance
                | ExtensionUpstreamUpdateDisposition::Unchanged
        )
    }) {
        return Ok(false);
    }
    let bytes = value.encode();
    if bytes.len() > MAX_EXTENSION_INSTALL_PROVENANCE_BYTES {
        return Err(invalid_data(
            "extension provenance encoding exceeded its limit",
        ));
    }
    conn.execute(
        "INSERT INTO extension_upstream_history(publisher, checkpoint) VALUES (?1, ?2)
         ON CONFLICT(publisher) DO UPDATE SET checkpoint = excluded.checkpoint",
        params![&candidate.publisher().bytes()[..], &candidate.encode()[..]],
    )?;
    conn.execute(
        "INSERT INTO extension_install_provenance(install_id, provenance) VALUES (?1, ?2)
         ON CONFLICT(install_id) DO UPDATE SET provenance = excluded.provenance",
        params![&install.bytes()[..], &bytes[..]],
    )?;
    Ok(true)
}

const _: () = assert!(MAX_EXTENSION_INSTALL_PROVENANCE_BYTES == 1024);
const _: () = assert!(EXTENSION_UPSTREAM_CHECKPOINT_BYTES == 105);
const _: () = assert!(zephium_core::extensions::MAX_EXTENSION_UPSTREAM_HISTORY_PER_PROFILE == 128);

#[cfg(test)]
mod tests {
    use super::super::Hub;
    use super::*;
    use std::sync::Arc;
    use zephium_core::extensions::*;
    use zephium_core::ids::{ProfileId, SpaceId};
    use zephium_core::ports::store::*;
    use zephium_core::session::{PersistedProfile, PersistedSpace, SessionState};

    fn profile() -> ProfileId {
        ProfileId::from(1)
    }
    fn initialize(hub: &mut Hub) {
        hub.save(&SessionState {
            profiles: vec![PersistedProfile {
                id: profile(),
                name: "Fixture".into(),
                kind: zephium_core::profiles::ProfileKind::Default,
            }],
            spaces: vec![PersistedSpace {
                id: SpaceId::from(2),
                profile: profile(),
                name: "Fixture".into(),
            }],
            ..SessionState::default()
        })
        .unwrap();
    }
    fn manifest(revision: u64) -> Arc<ExtensionManifestDescriptor> {
        manifest_in_domain(revision, ExtensionAuthorityId::from_bytes([1; 32]))
    }
    fn manifest_in_domain(
        revision: u64,
        authority: ExtensionAuthorityId,
    ) -> Arc<ExtensionManifestDescriptor> {
        let package = ExtensionPackageIdentity::new(
            authority,
            ExtensionPackageKey::from_bytes([2; 32]),
            ExtensionPackageRevision::new(revision).unwrap(),
            ExtensionPackagePayloadIdentity::acquired_zip(
                17,
                ExtensionArchiveDigest::from_bytes([revision as u8; 32]),
            )
            .unwrap(),
            ExtensionManifestDigest::from_bytes([revision as u8; 32]),
            ExtensionTreeDigest::from_bytes([revision as u8; 32]),
        );
        let declarations = ExtensionManifestDeclarations::new(
            ExtensionApiPermissionSet::new(Vec::new()).unwrap(),
            ExtensionApiPermissionSet::new(Vec::new()).unwrap(),
            None,
            None,
            None,
            None,
            Vec::new(),
            ExtensionManifestExecutionSurfaces::new(
                Vec::new(),
                ExtensionContentSecurityPolicyDeclaration::new(
                    ExtensionManifestResourceDigest::from_bytes([7; 32]),
                ),
                None,
                Vec::new(),
            )
            .unwrap(),
            Vec::new(),
        )
        .unwrap();
        let compatibility = declarations
            .declaration_keys()
            .into_iter()
            .map(|declaration| {
                ExtensionCompatibilityClassification::new(
                    declaration,
                    ExtensionCompatibilityLevel::Compatible,
                )
            })
            .collect();
        Arc::new(
            ExtensionManifestDescriptor::new(
                package,
                3,
                declarations,
                ExtensionCompatibilityTargetId::parse_exact("macos.wkwebextension.v1").unwrap(),
                compatibility,
            )
            .unwrap(),
        )
    }
    fn evidence(
        manifest: &ExtensionManifestDescriptor,
        version: &str,
        crx: u8,
    ) -> Arc<ExtensionInstallProvenance> {
        Arc::new(
            ExtensionInstallProvenance::new(
                ExtensionProvenanceSource::ChromeWebStore,
                ExtensionUpstreamCheckpoint::from_parts(
                    if zephium_core::extensions::is_beta_extension_authority(
                        manifest.package().authority(),
                    ) {
                        manifest.package().key()
                    } else {
                        ExtensionPackageKey::from_bytes([3; 32])
                    },
                    ExtensionUpstreamVersion::parse(version).unwrap(),
                    [crx; 32],
                    [crx; 32],
                ),
                ExtensionSourceTreeIdentity {
                    manifest: [11; 32],
                    tree: [12; 32],
                    index: [13; 32],
                },
                ExtensionTransformProvenance::Compiled {
                    target: ExtensionCompatibilityTargetId::parse_exact("webkit.fixture.v1")
                        .unwrap(),
                    revision: std::num::NonZeroU32::new(1).unwrap(),
                    sha256: [14; 32],
                },
                manifest,
                [15; 32],
                ExtensionProvenancePolicy {
                    revision: std::num::NonZeroU64::new(1).unwrap(),
                    sha256: [16; 32],
                },
            )
            .unwrap(),
        )
    }
    fn grants(
        id: ExtensionInstallId,
        manifest: &ExtensionManifestDescriptor,
    ) -> Box<ExtensionGrantAuthority> {
        Box::new(
            ExtensionGrantAuthority::initialize(
                &ExtensionInstall::new(id, manifest.package().clone()),
                Vec::new(),
                Vec::new(),
                false,
                false,
                manifest,
            )
            .unwrap(),
        )
    }
    fn bindings(
        id: ExtensionInstallId,
        manifest: &Arc<ExtensionManifestDescriptor>,
        provenance: Option<&Arc<ExtensionInstallProvenance>>,
    ) -> ExtensionGrantManifestBindings {
        ExtensionGrantManifestBindings::new(vec![match provenance {
            Some(provenance) => ExtensionGrantManifestBinding::with_provenance(
                id,
                manifest.clone(),
                provenance.clone(),
            )
            .unwrap(),
            None => ExtensionGrantManifestBinding::new(id, manifest.clone()),
        }])
        .unwrap()
    }
    fn provision(
        hub: &mut Hub,
        id: ExtensionInstallId,
        manifest: &Arc<ExtensionManifestDescriptor>,
        provenance: &Arc<ExtensionInstallProvenance>,
    ) -> ExtensionInstallProvisionOutcome {
        let catalog =
            super::super::extensions::load_catalog(hub.profile_conn(profile()).unwrap()).unwrap();
        hub.provision_extension_install_with_provenance(
            profile(),
            catalog.revision(),
            id,
            manifest.clone(),
            grants(id, manifest),
            Some(Box::new((**provenance).clone())),
        )
        .unwrap()
    }
    fn assert_cohort(
        hub: &mut Hub,
        id: ExtensionInstallId,
        manifest: &Arc<ExtensionManifestDescriptor>,
        provenance: &Arc<ExtensionInstallProvenance>,
    ) {
        let ExtensionGrantCohortLoadOutcome::Loaded(cohort) = hub
            .load_extension_grant_cohort(profile(), bindings(id, manifest, Some(provenance)))
            .unwrap()
        else {
            panic!("exact provenance cohort did not load");
        };
        assert_eq!(
            cohort.grants().next().unwrap().0.provenance(),
            Some(provenance.as_ref())
        );
        assert!(cohort.resolve_entry(id).unwrap().authority_arc().is_some());
    }

    #[test]
    fn provenance_reopens_with_grants_and_old_data_path_cannot_omit_it() {
        let dir = tempfile::tempdir().unwrap();
        let id = ExtensionInstallId::from(1);
        let manifest = manifest(1);
        let provenance = evidence(&manifest, "1", 1);
        let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
        initialize(&mut hub);
        assert!(matches!(
            provision(&mut hub, id, &manifest, &provenance),
            ExtensionInstallProvisionOutcome::Applied(_)
        ));
        assert_cohort(&mut hub, id, &manifest, &provenance);
        assert_eq!(
            hub.load_extension_grant_cohort(profile(), bindings(id, &manifest, None))
                .unwrap(),
            ExtensionGrantCohortLoadOutcome::Invalid
        );
        drop(hub);
        let mut reopened = Hub::open(dir.path().to_path_buf()).unwrap();
        assert_cohort(&mut reopened, id, &manifest, &provenance);
        let mut changed = provenance.encode().to_vec();
        *changed.last_mut().unwrap() ^= 1; // Valid codec, wrong policy digest.
        assert!(ExtensionInstallProvenance::decode(&changed)
            .unwrap()
            .matches_manifest(&manifest));
        reopened
            .profile_conn(profile())
            .unwrap()
            .execute(
                "UPDATE extension_install_provenance SET provenance = ?1",
                [&changed],
            )
            .unwrap();
        assert_eq!(
            reopened
                .load_extension_grant_cohort(profile(), bindings(id, &manifest, Some(&provenance)))
                .unwrap(),
            ExtensionGrantCohortLoadOutcome::Invalid
        );
    }

    #[test]
    fn source_updates_are_atomic_and_uninstall_cannot_reset_the_high_water_mark() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
        initialize(&mut hub);
        let id = ExtensionInstallId::from(1);
        let first = manifest(1);
        let first_source = evidence(&first, "1.9", 1);
        let ExtensionInstallProvisionOutcome::Applied(initial) =
            provision(&mut hub, id, &first, &first_source)
        else {
            panic!("install");
        };
        let second = manifest(2);
        let second_source = evidence(&second, "1.10", 2);
        let ExtensionInstallUpdateOutcome::Applied(updated) = hub
            .update_extension_install_with_provenance(
                profile(),
                initial.catalog_revision,
                id,
                initial.install.revision(),
                initial.authority.revision(),
                ExtensionInstallUpdateGrantDecision::PreserveExisting,
                first.clone(),
                second.clone(),
                Some(Box::new(
                    ExtensionProvenanceUpdate::new(first_source.clone(), second_source.clone())
                        .unwrap(),
                )),
            )
            .unwrap()
        else {
            panic!("update");
        };
        assert_cohort(&mut hub, id, &second, &second_source);
        let next = manifest(3);
        for source in [evidence(&next, "1.9", 1), evidence(&next, "1.10", 3)] {
            assert_eq!(
                hub.update_extension_install_with_provenance(
                    profile(),
                    updated.catalog_revision,
                    id,
                    updated.install.revision(),
                    updated.authority.revision(),
                    ExtensionInstallUpdateGrantDecision::PreserveExisting,
                    second.clone(),
                    next.clone(),
                    Some(Box::new(
                        ExtensionProvenanceUpdate::new(second_source.clone(), source).unwrap()
                    ))
                )
                .unwrap(),
                ExtensionInstallUpdateOutcome::Invalid
            );
            assert_cohort(&mut hub, id, &second, &second_source);
        }
        assert_eq!(
            hub.update_extension_install(
                profile(),
                updated.catalog_revision,
                id,
                updated.install.revision(),
                updated.authority.revision(),
                ExtensionInstallUpdateGrantDecision::PreserveExisting,
                second.clone(),
                next
            )
            .unwrap(),
            ExtensionInstallUpdateOutcome::Invalid
        );
        assert!(matches!(
            hub.mutate_extension_install_catalog(
                profile(),
                updated.catalog_revision,
                ExtensionInstallCatalogMutation::Delete {
                    id,
                    expected: updated.install.revision()
                }
            )
            .unwrap(),
            ExtensionInstallCatalogMutationOutcome::Applied(_)
        ));
        let conn = hub.profile_conn(profile()).unwrap();
        assert!(load(conn, id).unwrap().is_none());
        assert_eq!(
            checkpoint(conn, &[3; 32]).unwrap(),
            Some(second_source.upstream())
        );
        // Recovery may inspect an explicitly retained older artifact without
        // lowering the mark. Forward admission of that artifact remains denied.
        assert!(validate_high_water(conn, &first_source).is_ok());
        drop(hub);
        let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
        assert_eq!(
            provision(&mut hub, ExtensionInstallId::from(2), &first, &first_source),
            ExtensionInstallProvisionOutcome::Invalid
        );
        assert!(matches!(
            provision(
                &mut hub,
                ExtensionInstallId::from(2),
                &second,
                &second_source
            ),
            ExtensionInstallProvisionOutcome::Applied(_)
        ));
        assert_cohort(
            &mut hub,
            ExtensionInstallId::from(2),
            &second,
            &second_source,
        );
    }

    #[test]
    fn failed_source_write_rolls_back_install_grants_and_history_and_ambiguous_commit_keeps_complete_cohort(
    ) {
        let mut hub = Hub::in_memory().unwrap();
        initialize(&mut hub);
        let id = ExtensionInstallId::from(1);
        let manifest = manifest(1);
        let provenance = evidence(&manifest, "1", 1);
        hub.profile_conn(profile()).unwrap().execute_batch("CREATE TRIGGER reject_provenance BEFORE INSERT ON extension_install_provenance BEGIN SELECT RAISE(ABORT, 'injected provenance write failure'); END;").unwrap();
        assert!(hub
            .provision_extension_install_with_provenance(
                profile(),
                ExtensionInstallCatalogRevision::INITIAL,
                id,
                manifest.clone(),
                grants(id, &manifest),
                Some(Box::new((*provenance).clone()))
            )
            .is_err());
        let conn = hub.profile_conn(profile()).unwrap();
        for table in [
            "extension_installs",
            "extension_grants",
            "extension_install_provenance",
            "extension_upstream_history",
        ] {
            assert_eq!(
                conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row
                    .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
        conn.execute_batch("DROP TRIGGER reject_provenance")
            .unwrap();
        hub.make_next_extension_install_commit_ambiguous();
        assert_eq!(
            provision(&mut hub, id, &manifest, &provenance),
            ExtensionInstallProvisionOutcome::OutcomeUnknown
        );
        assert_cohort(&mut hub, id, &manifest, &provenance);
    }

    #[test]
    fn history_capacity_is_finite_but_existing_publishers_can_advance_at_capacity() {
        let mut hub = Hub::in_memory().unwrap();
        initialize(&mut hub);
        let conn = hub.profile_conn(profile()).unwrap();
        for key in 0..MAX_EXTENSION_UPSTREAM_HISTORY_PER_PROFILE {
            let checkpoint = ExtensionUpstreamCheckpoint::from_parts(
                ExtensionPackageKey::from_bytes([key as u8; 32]),
                ExtensionUpstreamVersion::parse("1").unwrap(),
                [1; 32],
                [1; 32],
            );
            conn.execute(
                "INSERT INTO extension_upstream_history(publisher, checkpoint) VALUES (?1, ?2)",
                params![
                    &checkpoint.publisher().bytes()[..],
                    &checkpoint.encode()[..]
                ],
            )
            .unwrap();
        }
        let extra = ExtensionUpstreamCheckpoint::from_parts(
            ExtensionPackageKey::from_bytes([255; 32]),
            ExtensionUpstreamVersion::parse("1").unwrap(),
            [1; 32],
            [1; 32],
        );
        assert!(conn
            .execute(
                "INSERT INTO extension_upstream_history(publisher, checkpoint) VALUES (?1, ?2)",
                params![&extra.publisher().bytes()[..], &extra.encode()[..]]
            )
            .is_err());
        let manifest = manifest(1);
        let source = evidence(&manifest, "2", 2);
        let new_publisher = ExtensionInstallProvenance::new(
            source.source().clone(),
            extra,
            source.original(),
            source.transform().clone(),
            &manifest,
            source.output_index(),
            source.policy(),
        )
        .unwrap();
        assert_eq!(
            hub.provision_extension_install_with_provenance(
                profile(),
                ExtensionInstallCatalogRevision::INITIAL,
                ExtensionInstallId::from(1),
                manifest.clone(),
                grants(ExtensionInstallId::from(1), &manifest),
                Some(Box::new(new_publisher))
            )
            .unwrap(),
            ExtensionInstallProvisionOutcome::LimitReached
        );
        assert!(
            super::super::extensions::load_catalog(hub.profile_conn(profile()).unwrap())
                .unwrap()
                .installs()
                .is_empty()
        );
        assert!(matches!(
            provision(&mut hub, ExtensionInstallId::from(1), &manifest, &source),
            ExtensionInstallProvisionOutcome::Applied(_)
        ));
    }

    #[test]
    fn grant_write_failure_rolls_back_new_provenance_and_an_ambiguous_update_reopens_whole() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
        initialize(&mut hub);
        let id = ExtensionInstallId::from(1);
        let first = manifest(1);
        let old = evidence(&first, "1", 1);
        let ExtensionInstallProvisionOutcome::Applied(initial) =
            provision(&mut hub, id, &first, &old)
        else {
            panic!("install");
        };
        let second = manifest(2);
        let new = evidence(&second, "2", 2);
        hub.profile_conn(profile()).unwrap().execute_batch("CREATE TRIGGER reject_grant_rebind BEFORE UPDATE ON extension_grants BEGIN SELECT RAISE(ABORT, 'injected grant rebind failure'); END;").unwrap();
        assert!(hub
            .update_extension_install_with_provenance(
                profile(),
                initial.catalog_revision,
                id,
                initial.install.revision(),
                initial.authority.revision(),
                ExtensionInstallUpdateGrantDecision::PreserveExisting,
                first.clone(),
                second.clone(),
                Some(Box::new(
                    ExtensionProvenanceUpdate::new(old.clone(), new.clone()).unwrap()
                ))
            )
            .is_err());
        assert_cohort(&mut hub, id, &first, &old);
        assert_eq!(
            checkpoint(hub.profile_conn(profile()).unwrap(), &[3; 32]).unwrap(),
            Some(old.upstream())
        );
        hub.profile_conn(profile())
            .unwrap()
            .execute_batch("DROP TRIGGER reject_grant_rebind")
            .unwrap();
        hub.make_next_extension_install_commit_ambiguous();
        assert_eq!(
            hub.update_extension_install_with_provenance(
                profile(),
                initial.catalog_revision,
                id,
                initial.install.revision(),
                initial.authority.revision(),
                ExtensionInstallUpdateGrantDecision::PreserveExisting,
                first,
                second.clone(),
                Some(Box::new(
                    ExtensionProvenanceUpdate::new(old, new.clone()).unwrap()
                ))
            )
            .unwrap(),
            ExtensionInstallUpdateOutcome::OutcomeUnknown
        );
        drop(hub);
        let mut reopened = Hub::open(dir.path().to_path_buf()).unwrap();
        assert_cohort(&mut reopened, id, &second, &new);
        assert_eq!(
            checkpoint(reopened.profile_conn(profile()).unwrap(), &[3; 32]).unwrap(),
            Some(new.upstream())
        );
    }
    #[test]
    fn beta_install_cannot_omit_provenance_through_either_write_capability() {
        let root = tempfile::tempdir().unwrap();
        let mut hub = Hub::open(root.path().to_path_buf()).unwrap();
        initialize(&mut hub);
        let descriptor = manifest_in_domain(
            1,
            zephium_core::extensions::ExtensionBetaRuntimeTarget::MacosNative
                .authority(zephium_core::extensions::ExtensionBetaChannel::Staging),
        );
        let id = ExtensionInstallId::from(42);
        let catalog =
            super::super::extensions::load_catalog(hub.profile_conn(profile()).unwrap()).unwrap();
        assert!(matches!(
            hub.provision_extension_install_with_provenance(
                profile(),
                catalog.revision(),
                id,
                descriptor.clone(),
                grants(id, &descriptor),
                None
            )
            .unwrap(),
            ExtensionInstallProvisionOutcome::Invalid
        ));
        assert!(matches!(
            hub.mutate_extension_install_catalog(
                profile(),
                catalog.revision(),
                zephium_core::extensions::ExtensionInstallCatalogMutation::Install {
                    id,
                    package: descriptor.package().clone()
                }
            )
            .unwrap(),
            ExtensionInstallCatalogMutationOutcome::Invalid
        ));
        assert!(
            super::super::extensions::load_catalog(hub.profile_conn(profile()).unwrap())
                .unwrap()
                .installs()
                .is_empty()
        );
        let provenance = evidence(&descriptor, "1", 1);
        assert!(matches!(
            provision(&mut hub, id, &descriptor, &provenance),
            ExtensionInstallProvisionOutcome::Applied(_)
        ));
        assert_cohort(&mut hub, id, &descriptor, &provenance);
    }

    #[test]
    fn beta_provenance_deletion_cannot_masquerade_as_a_legacy_install_after_reload() {
        let root = tempfile::tempdir().unwrap();
        let mut hub = Hub::open(root.path().to_path_buf()).unwrap();
        initialize(&mut hub);
        let descriptor = manifest_in_domain(
            1,
            zephium_core::extensions::ExtensionBetaRuntimeTarget::MacosNative
                .authority(zephium_core::extensions::ExtensionBetaChannel::Stable),
        );
        let id = ExtensionInstallId::from(43);
        let provenance = evidence(&descriptor, "1", 1);
        assert!(matches!(
            provision(&mut hub, id, &descriptor, &provenance),
            ExtensionInstallProvisionOutcome::Applied(_)
        ));
        let conn = hub.profile_conn(profile()).unwrap();
        conn.execute(
            "DELETE FROM extension_install_provenance WHERE install_id = ?1",
            [&id.bytes()[..]],
        )
        .unwrap();
        assert!(validate_manifest(conn, id, &descriptor).is_err());
        assert!(super::super::extensions::load_catalog(conn).is_err());
        assert!(
            conn.query_row(
                "SELECT count(*) FROM extension_upstream_history",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap()
                > 0
        );
    }
}
