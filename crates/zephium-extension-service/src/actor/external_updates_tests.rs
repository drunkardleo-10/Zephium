//! Signed synthetic source packages exercise the actual preparation/Store
//! update path. Disabled installs intentionally perform no native activation.
use super::*;
use ring::{
    rand::SystemRandom,
    signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_ASN1_SIGNING},
};
use std::io::{Cursor, Write};
use zephium_core::{
    profiles::ProfileKind,
    session::{PersistedProfile, SessionState},
};
use zephium_extension_package::Crx3SigningRequest;
use zephium_extension_runtime_api::*;
use zephium_store::SqliteStore;

struct Publisher {
    key: EcdsaKeyPair,
    public: Vec<u8>,
}
impl Publisher {
    fn new() -> Self {
        let random = SystemRandom::new();
        let der = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &random).unwrap();
        let key = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, der.as_ref(), &random)
            .unwrap();
        let mut public = vec![
            0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06,
            0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
        ];
        public.extend_from_slice(key.public_key().as_ref());
        Self { key, public }
    }
    fn package(
        &self,
        version: &str,
        extra_permission: Option<&str>,
        content: &[u8],
    ) -> (String, Box<[u8]>) {
        let permissions = extra_permission.map_or("\"storage\"".to_owned(), |permission| {
            format!("\"storage\",\"{permission}\"")
        });
        let manifest = format!(
            r#"{{"manifest_version":3,"name":"Source update fixture","version":"{version}","permissions":[{permissions}],"host_permissions":["https://example.test/*"],"content_scripts":[{{"matches":["https://example.test/*"],"js":["content.js"]}}]}}"#
        );
        self.sign_files(&[
            ("manifest.json", manifest.as_bytes()),
            ("content.js", content),
        ])
    }
    fn sign_files(&self, files: &[(&str, &[u8])]) -> (String, Box<[u8]>) {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for &(path, bytes) in files {
            zip.start_file(
                path,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
            zip.write_all(bytes).unwrap();
        }
        let zip = zip.finish().unwrap().into_inner();
        let request = Crx3SigningRequest::new_ecdsa_p256_sha256(&zip, &self.public).unwrap();
        let id = request.extension_id().as_str().to_owned();
        let signature = self
            .key
            .sign(
                &SystemRandom::new(),
                &request.signed_message_parts().concat(),
            )
            .unwrap();
        (
            id,
            request
                .finish(signature.as_ref())
                .unwrap()
                .into_boxed_slice(),
        )
    }
}
struct NoNative;
impl ExtensionRuntimeHostFactoryPort for NoNative {
    fn bind_activation(
        &mut self,
        _: ExtensionRuntimeHostActivationContext<'_>,
    ) -> Result<ExtensionRuntimeHostActivationPorts, ExtensionRuntimeHostBindError> {
        panic!("disabled update must not activate native code")
    }
    fn bind_recovery(
        &mut self,
        _: ExtensionRuntimeHostRecoveryContext,
    ) -> Result<Box<dyn ExtensionRuntimeHostOwnershipPort>, ExtensionRuntimeHostBindError> {
        panic!("empty journal must not recover native code")
    }
}
fn deadline() -> Instant {
    Instant::now() + std::time::Duration::from_secs(15)
}
struct Rig {
    startup: Option<WorkerStartupState>,
    runtime: RuntimeCoordinator,
    store: Arc<SqliteStore>,
    root: tempfile::TempDir,
    profile: ProfileId,
    install: zephium_core::ids::ExtensionInstallId,
}
impl Rig {
    fn new() -> Self {
        let root = tempfile::tempdir_in("/private/tmp").unwrap();
        let store = Arc::new(SqliteStore::open(root.path()).unwrap());
        let profile = ProfileId::from(1);
        store.save_session(SessionState {
            profiles: vec![PersistedProfile {
                id: profile,
                name: "Update fixture".into(),
                kind: ProfileKind::Default,
            }],
            ..SessionState::default()
        });
        assert!(store.flush_until(deadline()));
        let input = crate::ExtensionServiceLaunchInput::new(
            store.claim_extension_service_store_authority().unwrap(),
            crate::ExtensionRepositoryRoot::from_app_data_directory(root.path()).unwrap(),
            ExtensionRuntimeHostFactory::from_trusted_port(Box::new(NoNative)),
        );
        let mut startup = WorkerStartupState::new(input);
        startup.repository.open().unwrap();
        Self {
            startup: Some(startup),
            runtime: RuntimeCoordinator::new(),
            store,
            root,
            profile,
            install: zephium_core::ids::ExtensionInstallId::from(1),
        }
    }
    fn catalog(&self) -> ExtensionInstallCatalog {
        let Call::Completed(ExtensionInstallCatalogLoadOutcome::Loaded(catalog)) = self
            .startup
            .as_ref()
            .unwrap()
            .store
            .load_install_catalog_until(self.profile, deadline())
        else {
            panic!("catalog unavailable")
        };
        catalog
    }
    fn selector(&self) -> ExtensionInstallSelector {
        let catalog = self.catalog();
        ExtensionInstallSelector::new(
            self.profile,
            self.install,
            catalog.revision(),
            catalog.get(self.install).unwrap().revision(),
        )
    }
    fn install(&mut self, package: &(String, Box<[u8]>)) {
        let startup = self.startup.as_mut().unwrap();
        let request =
            ExtensionStorePackageRequest::new(package.0.clone(), package.1.clone()).unwrap();
        let ExtensionStorePackagePreparationOutcome::Prepared(review) = startup
            .repository
            .prepare_store_package(&startup.store, self.profile, request, deadline())
        else {
            panic!("fixture preparation failed")
        };
        let (manifest, provenance) = startup
            .repository
            .external_install_manifest(review.selector())
            .unwrap();
        let install = ExtensionInstall::new(self.install, manifest.package().clone());
        let grants = ExtensionGrantAuthority::initialize(
            &install,
            manifest.declarations().required_api().names().to_vec(),
            manifest
                .declarations()
                .required_host_authorities()
                .into_iter()
                .cloned()
                .collect(),
            false,
            false,
            &manifest,
        )
        .unwrap();
        assert!(matches!(
            startup.store.provision_install_with_provenance_until(
                self.profile,
                review.selector().expected_catalog_revision(),
                self.install,
                manifest,
                Box::new(grants),
                Some(provenance),
                deadline()
            ),
            Call::Completed(ExtensionInstallProvisionOutcome::Applied(_))
        ));
        startup
            .repository
            .clear_completed_external_candidate(review.selector());
    }
    fn prepare(
        &mut self,
        package: &(String, Box<[u8]>),
        selector: ExtensionInstallSelector,
    ) -> ExtensionStorePackagePreparationOutcome {
        let startup = self.startup.as_mut().unwrap();
        startup.repository.prepare_store_package(
            &startup.store,
            self.profile,
            ExtensionStorePackageRequest::for_update(
                package.0.clone(),
                package.1.clone(),
                selector,
            )
            .unwrap(),
            deadline(),
        )
    }
    fn settle(&mut self) -> ExtensionStorePackagePreparationOutcome {
        prepared(
            self.startup.as_mut().unwrap(),
            &mut self.runtime,
            self.profile,
            deadline(),
        )
    }
}
impl Drop for Rig {
    fn drop(&mut self) {
        drop(self.startup.take());
        assert_eq!(
            self.store.shutdown_until(deadline()),
            StoreShutdownOutcome::Clean
        );
        fn unseal(path: &std::path::Path) {
            use std::os::unix::fs::PermissionsExt;
            let Ok(meta) = std::fs::symlink_metadata(path) else {
                return;
            };
            if meta.is_dir() && !meta.file_type().is_symlink() {
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
                for entry in std::fs::read_dir(path).unwrap() {
                    unseal(&entry.unwrap().path());
                }
            }
        }
        unseal(self.root.path());
    }
}

#[test]
fn activation_failure_is_projected_for_the_installed_package_and_cleared_on_retirement() {
    use zephium_core::extensions::{ExtensionGrantBrowsingContext, ExtensionNativeOwnershipKey};
    use zephium_core::ports::extensions::{
        ExtensionActivationPendingReason, ExtensionManagementCatalogOutcome,
        ExtensionManagementRuntimeState,
    };
    let mut rig = Rig::new();
    rig.install(&Publisher::new().package("1.0", None, b"void 0;"));
    let selector = rig.selector();
    let startup = rig.startup.as_mut().unwrap();
    let _ = startup.store.set_install_enabled_until(
        rig.profile,
        selector.catalog_revision(),
        rig.install,
        selector.install_revision(),
        true,
        deadline(),
    );
    let catalog = rig.catalog();
    assert!(catalog.get(rig.install).unwrap().desired_enabled());
    let startup = rig.startup.as_mut().unwrap();
    let key = ExtensionNativeOwnershipKey::new(
        rig.profile,
        rig.install,
        ExtensionGrantBrowsingContext::Regular,
    );
    assert!(matches!(
        rig.runtime.activate_until(
            super::super::management::resources(startup),
            key,
            true,
            deadline()
        ),
        crate::runtime_coordinator::RuntimeActivationOutcome::ProfileFenced
    ));
    let ExtensionManagementCatalogOutcome::Loaded(view) = super::super::external_management::load(
        startup,
        &mut rig.runtime,
        rig.profile,
        catalog.clone(),
        deadline(),
    ) else {
        panic!("catalog");
    };
    assert_eq!(
        view.entries()[0].runtime(),
        ExtensionManagementRuntimeState::ActivationFailed(
            ExtensionActivationPendingReason::ProfileFenced
        )
    );
    let _ = rig.runtime.retire_profile_until(
        super::super::management::resources(startup),
        rig.profile,
        deadline(),
    );
    let ExtensionManagementCatalogOutcome::Loaded(view) = super::super::external_management::load(
        startup,
        &mut rig.runtime,
        rig.profile,
        catalog,
        deadline(),
    ) else {
        panic!("catalog");
    };
    assert_eq!(
        view.entries()[0].runtime(),
        ExtensionManagementRuntimeState::PendingActivation
    );
}

#[test]
fn source_update_preserves_disabled_state_and_requires_new_permission_consent() {
    let publisher = Publisher::new();
    let first = publisher.package("1.0", None, b"void 1;");
    let second = publisher.package("2.0", None, b"void 2;");
    let third = publisher.package("3.0", Some("tabs"), b"void 3;");
    let mut rig = Rig::new();
    rig.install(&first);
    let stale = rig.selector();
    assert!(matches!(
        rig.prepare(&first, stale),
        ExtensionStorePackagePreparationOutcome::UpToDate
    ));
    assert!(matches!(
        rig.prepare(&second, stale),
        ExtensionStorePackagePreparationOutcome::UpdateAvailable
    ));
    let ExtensionStorePackagePreparationOutcome::UpdateSettled(result) = rig.settle() else {
        panic!("no update settlement")
    };
    assert!(matches!(
        result.outcome(),
        ExtensionUpdateOutcome::Updated {
            runtime: ExtensionUpdateRuntimeState::Disabled
        }
    ));
    let catalog = rig.catalog();
    assert_eq!(
        catalog.get(rig.install).unwrap().package().revision().get(),
        2
    );
    assert!(!catalog.get(rig.install).unwrap().desired_enabled());
    assert!(matches!(
        rig.prepare(&third, stale),
        ExtensionStorePackagePreparationOutcome::Unavailable
    ));
    assert!(matches!(
        rig.prepare(&third, rig.selector()),
        ExtensionStorePackagePreparationOutcome::UpdateAvailable
    ));
    assert!(matches!(
        rig.settle(),
        ExtensionStorePackagePreparationOutcome::UpdateAvailable
    ));
    assert_eq!(
        rig.catalog(),
        catalog,
        "preparation must not grant new access or change installed bytes"
    );
    let prompt = pending_prompt(rig.startup.as_mut().unwrap(), rig.profile, deadline())
        .unwrap()
        .unwrap();
    assert_eq!(prompt.added_required_api(), &[Box::<str>::from("tabs")]);
    let result = approve(
        rig.startup.as_mut().unwrap(),
        &mut rig.runtime,
        prompt.selector().clone(),
        deadline(),
    );
    assert!(matches!(
        result.outcome(),
        ExtensionUpdateOutcome::Updated {
            runtime: ExtensionUpdateRuntimeState::Disabled
        }
    ));
    assert_eq!(
        rig.catalog()
            .get(rig.install)
            .unwrap()
            .package()
            .revision()
            .get(),
        3
    );
    assert!(matches!(
        rig.prepare(&first, rig.selector()),
        ExtensionStorePackagePreparationOutcome::InvalidPackage
    ));
}

#[test]
fn invalid_or_equivocating_replacement_cannot_mutate_an_install() {
    let publisher = Publisher::new();
    let first = publisher.package("1.0", None, b"void 1;");
    let mut rig = Rig::new();
    rig.install(&first);
    let original = rig.catalog();
    let different_bytes = publisher.package("1.0", None, b"void 99;");
    assert!(matches!(
        rig.prepare(&different_bytes, rig.selector()),
        ExtensionStorePackagePreparationOutcome::InvalidPackage
    ));
    let mut invalid = publisher.package("2.0", None, b"void 2;");
    invalid.1[100] ^= 1;
    assert!(matches!(
        rig.prepare(&invalid, rig.selector()),
        ExtensionStorePackagePreparationOutcome::InvalidPackage
    ));
    let unsupported = publisher.package("2.0", Some("offscreen"), b"void 2;");
    assert!(matches!(
        rig.prepare(&unsupported, rig.selector()),
        ExtensionStorePackagePreparationOutcome::Unsupported(_)
    ));
    assert_eq!(rig.catalog(), original);
}

#[test]
fn dismissed_update_review_cannot_later_grant_permissions() {
    let publisher = Publisher::new();
    let first = publisher.package("1.0", None, b"void 1;");
    let replacement = publisher.package("2.0", Some("tabs"), b"void 2;");
    let mut rig = Rig::new();
    rig.install(&first);
    let original = rig.catalog();
    assert!(matches!(
        rig.prepare(&replacement, rig.selector()),
        ExtensionStorePackagePreparationOutcome::UpdateAvailable
    ));
    assert!(matches!(
        rig.settle(),
        ExtensionStorePackagePreparationOutcome::UpdateAvailable
    ));
    let prompt = pending_prompt(rig.startup.as_mut().unwrap(), rig.profile, deadline())
        .unwrap()
        .unwrap();
    let selector = prompt.selector().clone();
    assert!(rig
        .startup
        .as_mut()
        .unwrap()
        .repository
        .dismiss_external_update(&selector));
    assert!(
        pending_prompt(rig.startup.as_mut().unwrap(), rig.profile, deadline())
            .unwrap()
            .is_none()
    );
    let late = approve(
        rig.startup.as_mut().unwrap(),
        &mut rig.runtime,
        selector,
        deadline(),
    );
    assert!(matches!(late.outcome(), ExtensionUpdateOutcome::Conflict));
    assert_eq!(rig.catalog(), original);
}

#[test]
fn unsuccessful_checks_preserve_pending_review_and_its_package_roots() {
    let publisher = Publisher::new();
    let first = publisher.package("1.0", None, b"void 1;");
    let replacement = publisher.package("2.0", Some("tabs"), b"void 2;");
    let mut rig = Rig::new();
    rig.install(&first);
    let original = rig.catalog();
    assert!(matches!(
        rig.prepare(&replacement, rig.selector()),
        ExtensionStorePackagePreparationOutcome::UpdateAvailable
    ));
    let prompt = pending_prompt(rig.startup.as_mut().unwrap(), rig.profile, deadline())
        .unwrap()
        .unwrap();
    assert!(matches!(
        rig.prepare(&first, rig.selector()),
        ExtensionStorePackagePreparationOutcome::UpToDate
    ));
    let unsupported = publisher.package("3.0", Some("offscreen"), b"void 3;");
    assert!(matches!(
        rig.prepare(&unsupported, rig.selector()),
        ExtensionStorePackagePreparationOutcome::Unsupported(_)
    ));
    let mut invalid = replacement.clone();
    invalid.1[100] ^= 1;
    assert!(matches!(
        rig.prepare(&invalid, rig.selector()),
        ExtensionStorePackagePreparationOutcome::InvalidPackage
    ));
    let startup = rig.startup.as_mut().unwrap();
    assert_eq!(
        startup
            .repository
            .collect_external_package_garbage(&startup.store, deadline())
            .unwrap()
            .removed(),
        0
    );
    let preserved = pending_prompt(startup, rig.profile, deadline())
        .unwrap()
        .unwrap();
    assert_eq!(preserved.selector(), prompt.selector());
    assert_eq!(rig.catalog(), original);
    let result = approve(
        rig.startup.as_mut().unwrap(),
        &mut rig.runtime,
        preserved.selector().clone(),
        deadline(),
    );
    assert!(matches!(
        result.outcome(),
        ExtensionUpdateOutcome::Updated {
            runtime: ExtensionUpdateRuntimeState::Disabled
        }
    ));
}

#[test]
fn automatic_package_check_cannot_replace_an_interactive_permission_review() {
    let publisher = Publisher::new();
    let mut rig = Rig::new();
    rig.install(&publisher.package("1.0", None, b"void 1;"));
    let replacement = publisher.package("2.0", Some("tabs"), b"void 2;");
    assert!(matches!(
        rig.prepare(&replacement, rig.selector()),
        ExtensionStorePackagePreparationOutcome::UpdateAvailable
    ));
    let prompt = pending_prompt(rig.startup.as_mut().unwrap(), rig.profile, deadline())
        .unwrap()
        .unwrap();
    let newer = publisher.package("3.0", None, b"void 3;");
    let request =
        ExtensionStorePackageRequest::for_background_update(newer.0, newer.1, rig.selector())
            .unwrap();
    let startup = rig.startup.as_mut().unwrap();
    assert!(matches!(
        startup
            .repository
            .prepare_store_package(&startup.store, rig.profile, request, deadline()),
        ExtensionStorePackagePreparationOutcome::Unavailable
    ));
    assert_eq!(
        pending_prompt(startup, rig.profile, deadline())
            .unwrap()
            .unwrap()
            .selector(),
        prompt.selector()
    );
}

#[test]
fn native_update_refusal_preserves_enabled_install_and_grants() {
    let publisher = Publisher::new();
    let first = publisher.package("1.0", None, b"void 1;");
    let replacement = publisher.package("2.0", None, b"void 2;");
    let mut rig = Rig::new();
    rig.install(&first);
    let selector = rig.selector();
    let startup = rig.startup.as_mut().unwrap();
    assert!(matches!(
        startup.store.set_install_enabled_until(
            rig.profile,
            selector.catalog_revision(),
            rig.install,
            selector.install_revision(),
            true,
            deadline()
        ),
        Call::Completed(ExtensionInstallCatalogMutationOutcome::Applied(_))
    ));
    let original = rig.catalog();
    assert!(matches!(
        rig.prepare(&replacement, rig.selector()),
        ExtensionStorePackagePreparationOutcome::UpdateAvailable
    ));
    let ExtensionStorePackagePreparationOutcome::UpdateSettled(result) = rig.settle() else {
        panic!("settlement");
    };
    assert!(matches!(
        result.outcome(),
        ExtensionUpdateOutcome::Unavailable
    ));
    assert_eq!(rig.catalog(), original);
    // NoNative would panic if refusal entered native activation or recovery.
    assert!(rig
        .startup
        .as_ref()
        .unwrap()
        .repository
        .external_update
        .is_some());
}

#[test]
fn dismissing_an_install_review_allows_background_updates_to_resume() {
    let publisher = Publisher::new();
    let mut rig = Rig::new();
    rig.install(&publisher.package("1.0", None, b"void 1;"));
    let other = Publisher::new().package("1.0", None, b"void 0;");
    let selector = rig.selector();
    let startup = rig.startup.as_mut().unwrap();
    let ExtensionStorePackagePreparationOutcome::Prepared(review) =
        startup.repository.prepare_store_package(
            &startup.store,
            rig.profile,
            ExtensionStorePackageRequest::new(other.0, other.1).unwrap(),
            deadline(),
        )
    else {
        panic!("install review");
    };
    let replacement = publisher.package("2.0", None, b"void 2;");
    let request = || {
        ExtensionStorePackageRequest::for_background_update(
            replacement.0.clone(),
            replacement.1.clone(),
            selector,
        )
        .unwrap()
    };
    assert!(matches!(
        startup.repository.prepare_store_package(
            &startup.store,
            rig.profile,
            request(),
            deadline()
        ),
        ExtensionStorePackagePreparationOutcome::Unavailable
    ));
    let stale = ExtensionInstallCandidateSelector::new(
        rig.profile,
        selector.catalog_revision().next().unwrap(),
        review.selector().catalog_set(),
        review.selector().package().clone(),
    );
    startup
        .repository
        .clear_completed_external_candidate(&stale);
    assert!(startup
        .repository
        .external_pending_review(rig.profile)
        .is_some());
    startup
        .repository
        .clear_completed_external_candidate(review.selector());
    assert!(matches!(
        startup.repository.prepare_store_package(
            &startup.store,
            rig.profile,
            request(),
            deadline()
        ),
        ExtensionStorePackagePreparationOutcome::UpdateAvailable
    ));
}

#[test]
fn collection_retains_disabled_and_pending_packages_then_reclaims_obsolete_versions() {
    let publisher = Publisher::new();
    let first = publisher.package("1.0", None, b"void 1;");
    let second = publisher.package("2.0", Some("tabs"), b"void 2;");
    let mut rig = Rig::new();
    rig.install(&first);
    let startup = rig.startup.as_mut().unwrap();
    assert_eq!(
        startup
            .repository
            .collect_external_package_garbage(&startup.store, deadline())
            .unwrap()
            .removed(),
        0
    );
    assert!(matches!(
        rig.prepare(&second, rig.selector()),
        ExtensionStorePackagePreparationOutcome::UpdateAvailable
    ));
    let startup = rig.startup.as_mut().unwrap();
    assert_eq!(
        startup
            .repository
            .collect_external_package_garbage(&startup.store, deadline())
            .unwrap()
            .removed(),
        0,
        "a pending permission review must retain its replacement"
    );
    let prompt = pending_prompt(startup, rig.profile, deadline())
        .unwrap()
        .unwrap();
    assert!(startup
        .repository
        .dismiss_external_update(prompt.selector()));
    assert_eq!(
        startup
            .repository
            .collect_external_package_garbage(&startup.store, deadline())
            .unwrap()
            .removed(),
        1
    );
    assert!(matches!(
        rig.prepare(&first, rig.selector()),
        ExtensionStorePackagePreparationOutcome::UpToDate
    ));
}

#[test]
fn package_roots_include_other_profiles_and_disabled_installs() {
    let publisher = Publisher::new();
    let package = publisher.package("1.0", None, b"void 1;");
    let mut rig = Rig::new();
    rig.install(&package);
    let first_profile = rig.profile;
    rig.profile = ProfileId::from(2);
    rig.store.save_session(SessionState {
        profiles: vec![
            PersistedProfile {
                id: first_profile,
                name: "First".into(),
                kind: ProfileKind::Default,
            },
            PersistedProfile {
                id: rig.profile,
                name: "Second".into(),
                kind: ProfileKind::Named,
            },
        ],
        ..SessionState::default()
    });
    assert!(rig.store.flush_until(deadline()));
    rig.install(&package);
    rig.profile = first_profile;
    let selector = rig.selector();
    let startup = rig.startup.as_mut().unwrap();
    assert!(matches!(
        startup.store.delete_install_until(
            selector.profile(),
            selector.catalog_revision(),
            selector.install(),
            selector.install_revision(),
            deadline()
        ),
        Call::Completed(ExtensionInstallCatalogMutationOutcome::Applied(_))
    ));
    assert_eq!(
        startup
            .repository
            .collect_external_package_garbage(&startup.store, deadline())
            .unwrap()
            .removed(),
        0,
        "the other profile still owns the disabled installation"
    );
    rig.profile = ProfileId::from(2);
    assert!(matches!(
        rig.prepare(&package, rig.selector()),
        ExtensionStorePackagePreparationOutcome::UpToDate
    ));
}

#[test]
fn another_valid_crx_envelope_for_identical_content_is_not_an_update() {
    let publisher = Publisher::new();
    let first = publisher.package("1.0", None, b"void 1;");
    let resigned = publisher.package("1.0", None, b"void 1;");
    let mut rig = Rig::new();
    rig.install(&first);
    let original = rig.catalog();
    assert!(matches!(
        rig.prepare(&resigned, rig.selector()),
        ExtensionStorePackagePreparationOutcome::UpToDate
    ));
    assert_eq!(rig.catalog(), original);
}

#[test]
fn startup_reuses_only_exact_sibling_metadata_and_still_rejects_selected_package_tampering() {
    use std::os::unix::fs::PermissionsExt;
    use zephium_core::extensions::{
        ExtensionBetaObjectDigest, ExtensionGrantBrowsingContext, ExtensionNativeOwnershipKey,
    };
    let mut rig = Rig::new();
    let first = rig.install;
    rig.install(&Publisher::new().package("1.0", None, b"void 0;"));
    rig.install = zephium_core::ids::ExtensionInstallId::from(2);
    let second = rig.install;
    rig.install(&Publisher::new().package("1.0", None, b"void 0;"));
    let catalog = rig.catalog();
    let startup = rig.startup.as_mut().unwrap();
    startup.repository.begin_startup_manifest_cache();
    let load = |repository: &mut crate::repository::ServiceRepository, install| {
        repository.authenticate_runtime_manifest_bindings_for_profile(
            &catalog,
            &startup.store,
            ExtensionNativeOwnershipKey::new(
                rig.profile,
                install,
                ExtensionGrantBrowsingContext::Regular,
            ),
            deadline(),
        )
    };
    let one = load(&mut startup.repository, first)
        .unwrap()
        .into_store_bindings_and_manifest(first)
        .ok()
        .unwrap();
    let first_manifest = Arc::clone(&one.2);
    drop(one);
    let two = load(&mut startup.repository, second)
        .unwrap()
        .into_store_bindings_and_manifest(second)
        .ok()
        .unwrap();
    let sibling = two
        .1
        .iter()
        .find(|binding| binding.install_id() == first)
        .unwrap();
    assert!(Arc::ptr_eq(&first_manifest, sibling.manifest_arc()));
    drop(two);
    startup.repository.end_startup_manifest_cache();
    let uncached = load(&mut startup.repository, second)
        .unwrap()
        .into_store_bindings_and_manifest(second)
        .ok()
        .unwrap();
    assert!(!Arc::ptr_eq(
        &first_manifest,
        uncached
            .1
            .iter()
            .find(|binding| binding.install_id() == first)
            .unwrap()
            .manifest_arc()
    ));
    drop(uncached);
    startup.repository.begin_startup_manifest_cache();
    drop(load(&mut startup.repository, first).unwrap());
    let Call::Completed(ExtensionInstallProvenanceLoadOutcome::Loaded(Some(provenance))) = startup
        .store
        .load_install_provenance_until(rig.profile, second, deadline())
    else {
        panic!("provenance");
    };
    let id: String = ExtensionBetaObjectDigest::from_provenance(&provenance)
        .bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let file = rig
        .root
        .path()
        .join("extension-packages-v1")
        .join(format!("beta-{id}"))
        .join("ready/extension/content.js");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::write(&file, b"void 9;").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o400)).unwrap();
    assert!(load(&mut startup.repository, second).is_err());
    startup.repository.end_startup_manifest_cache();
}

#[test]
fn bounded_storage_is_disclosed_on_install_and_when_an_update_adds_the_limitation() {
    let mut rig = Rig::new();
    let publisher = Publisher::new();
    rig.install(&publisher.package("1.0", None, b"void 0;"));
    let next = publisher.package("2.0", Some("unlimitedStorage"), b"void 0;");
    assert!(matches!(
        rig.prepare(&next, rig.selector()),
        ExtensionStorePackagePreparationOutcome::UpdateAvailable
    ));
    assert!(matches!(
        rig.settle(),
        ExtensionStorePackagePreparationOutcome::UpdateAvailable
    ));
    let prompt = pending_prompt(rig.startup.as_mut().unwrap(), rig.profile, deadline())
        .unwrap()
        .unwrap();
    let limitation = ExtensionManagementLimitation::api_permission("unlimitedStorage").unwrap();
    assert!(prompt.limitations().contains(&limitation));
    assert!(!prompt
        .added_required_api()
        .iter()
        .any(|name| name.as_ref() == "unlimitedStorage"));
    let candidate = &rig
        .startup
        .as_ref()
        .unwrap()
        .repository
        .external_update
        .as_ref()
        .unwrap()
        .replacement;
    let review = candidate.review().unwrap();
    assert!(review.limitations().contains(&limitation));
    assert!(!review
        .required_api()
        .iter()
        .any(|name| name.as_ref() == "unlimitedStorage"));
}

#[test]
fn adding_managed_storage_to_an_update_requires_review_before_changing_the_install() {
    let mut rig = Rig::new();
    let publisher = Publisher::new();
    rig.install(&publisher.package("1.0", None, b"void 0;"));
    let original = rig.catalog();
    let manifest = br#"{"manifest_version":3,"name":"Managed fixture","version":"2.0","permissions":["storage"],"host_permissions":["https://example.test/*"],"content_scripts":[{"matches":["https://example.test/*"],"js":["content.js"]}],"storage":{"managed_schema":"managed.json"}}"#;
    let package = publisher.sign_files(&[
        ("manifest.json", manifest),
        ("content.js", b"void 0;"),
        ("managed.json", br#"{"type":"object","properties":{}}"#),
    ]);
    assert!(matches!(
        rig.prepare(&package, rig.selector()),
        ExtensionStorePackagePreparationOutcome::UpdateAvailable
    ));
    assert!(matches!(
        rig.settle(),
        ExtensionStorePackagePreparationOutcome::UpdateAvailable
    ));
    assert_eq!(rig.catalog(), original);
    let prompt = pending_prompt(rig.startup.as_mut().unwrap(), rig.profile, deadline())
        .unwrap()
        .unwrap();
    assert!(prompt
        .limitations()
        .contains(&ExtensionManagementLimitation::ManagedStorage));
    assert!(prompt.added_required_api().is_empty());
}
