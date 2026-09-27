//! Local preparation and consent for explicitly requested store extensions.
use super::ServiceRepository;
use std::{sync::Arc, time::Instant};
use zephium_core::{
    extensions::*,
    ids::ProfileId,
    ports::{extensions::*, store::*},
};
#[cfg(feature = "capabilities-v2-qa")]
use zephium_extension_distribution::beta::admit_signed_bitwarden_capabilities_v2_qa;
#[cfg(feature = "capabilities-v2-qa")]
use zephium_extension_distribution::beta::signed_bitwarden_qa_transform_refresh;
use zephium_extension_distribution::beta::{
    admit_external_source, BetaCompatibilityLimitation, BetaPreparationWorkspace,
    BetaRuntimeTarget, BetaSourceAdmissionError,
};
use zephium_extension_package::{ChromiumExtensionId, ResolvedExtensionManifestMetadata};
use zephium_extension_repository::beta::StoredBetaPackage;
use zephium_private_fs::LockedPrivateNamespace;
use zephium_store::{ExtensionServiceStoreAuthority, ExtensionServiceStoreCallOutcome as Call};

pub(crate) struct ExternalCandidate {
    pub(crate) selector: ExtensionInstallCandidateSelector,
    pub(crate) package: StoredBetaPackage,
    pub(crate) manifest: Arc<ExtensionManifestDescriptor>,
    pub(crate) provenance: Arc<ExtensionInstallProvenance>,
    pub(crate) display: ResolvedExtensionManifestMetadata,
}
pub(crate) struct ExternalUpdateCandidate {
    pub(crate) selector: ExtensionInstallSelector,
    pub(crate) current: ExternalCandidate,
    pub(crate) replacement: ExternalCandidate,
}
impl ExternalCandidate {
    pub(crate) fn review(
        &self,
    ) -> Result<ExtensionInstallCandidateEntry, ExtensionStorePackagePreparationOutcome> {
        use ExtensionStorePackagePreparationOutcome::FailedClosed;
        let (mut compatibility, mut limitations) =
            crate::manifest_projection::compatibility(&self.manifest).ok_or(FailedClosed)?;
        for name in self.package.withheld_optional_permissions() {
            compatibility = ExtensionManagementCompatibility::Degraded;
            limitations.push(
                ExtensionManagementLimitation::unavailable_optional_api(name.as_str())
                    .map_err(|_| FailedClosed)?,
            );
        }
        if self.package.external_messaging_withheld() {
            compatibility = ExtensionManagementCompatibility::Degraded;
            limitations.push(ExtensionManagementLimitation::ExternalMessagingUnavailable);
        }
        for host in self.package.withheld_optional_hosts() {
            compatibility = ExtensionManagementCompatibility::Degraded;
            limitations.push(ExtensionManagementLimitation::OptionalHostUnavailable(
                host.clone(),
            ));
        }
        for limitation in self.package.limitations().map_err(|_| FailedClosed)? {
            let projected = project_beta_limitation(*limitation).map_err(|_| FailedClosed)?;
            if let Some(projected) = projected {
                compatibility = ExtensionManagementCompatibility::Degraded;
                if !limitations.contains(&projected) {
                    limitations.push(projected);
                }
            }
        }
        let original = self.package.manifest().map_err(|_| FailedClosed)?;
        let id = original.chromium_key().ok_or(FailedClosed)?.extension_id();
        let provenance = ExtensionManagementProvenance::new(
            format!("https://chromewebstore.google.com/detail/{}", id.as_str()),
            original.metadata().version(),
            "NOASSERTION",
            self.display
                .author()
                .map_or(self.display.name().as_str(), |author| author.as_str()),
        )
        .map_err(|_| FailedClosed)?;
        let declarations = self.manifest.declarations();
        ExtensionInstallCandidateEntry::new(
            self.selector.clone(),
            self.display.name().as_str(),
            self.display.description().map(|text| text.as_str().into()),
            self.display.author().map(|text| text.as_str().into()),
            original.metadata().version(),
            ExtensionManagementSource::ExternalCompatibility,
            None,
            Some(provenance),
            declarations
                .required_api()
                .names()
                .iter()
                .map(|name| name.as_str().into())
                .collect(),
            declarations
                .required_host_authorities()
                .into_iter()
                .map(|host| host.as_str().into())
                .collect(),
            declarations
                .optional_api()
                .names()
                .iter()
                .map(|name| name.as_str().into())
                .collect(),
            declarations
                .optional_hosts()
                .into_iter()
                .flat_map(|hosts| hosts.patterns())
                .map(|host| host.as_str().into())
                .collect(),
            false,
            false,
            compatibility,
            limitations,
        )
        .map_err(|_| FailedClosed)
    }
}

fn project_beta_limitation(
    limitation: BetaCompatibilityLimitation,
) -> Result<Option<ExtensionManagementLimitation>, ExtensionManagementProjectionError> {
    Ok(match limitation {
        BetaCompatibilityLimitation::BoundedStorageQuota => Some(
            ExtensionManagementLimitation::api_permission("unlimitedStorage")?,
        ),
        BetaCompatibilityLimitation::MainDocumentContentScriptsOnly => {
            Some(ExtensionManagementLimitation::MainDocumentContentScriptsOnly)
        }
        BetaCompatibilityLimitation::FragmentUrlContentScriptsUnavailable => {
            Some(ExtensionManagementLimitation::FragmentUrlContentScriptsUnavailable)
        }
        BetaCompatibilityLimitation::ContentScriptFontsUnavailable => {
            Some(ExtensionManagementLimitation::ContentScriptFontsUnavailable)
        }
        BetaCompatibilityLimitation::SidePanelUnavailable => {
            Some(ExtensionManagementLimitation::SidePanelUnavailable)
        }
        BetaCompatibilityLimitation::OffscreenLocalStorageOnly => {
            Some(ExtensionManagementLimitation::OffscreenLocalStorageOnly)
        }
        BetaCompatibilityLimitation::SandboxedPagesUnavailable => {
            Some(ExtensionManagementLimitation::SandboxedPagesUnavailable)
        }
        BetaCompatibilityLimitation::ClipboardReadUnavailable => {
            Some(ExtensionManagementLimitation::ClipboardReadUnavailable)
        }
        _ => None,
    })
}

#[cfg(test)]
mod limitation_tests {
    use super::*;

    #[test]
    fn exact_source_limitations_reach_install_and_installed_review_as_typed_rows() {
        for (source, projected) in [
            (
                BetaCompatibilityLimitation::MainDocumentContentScriptsOnly,
                ExtensionManagementLimitation::MainDocumentContentScriptsOnly,
            ),
            (
                BetaCompatibilityLimitation::FragmentUrlContentScriptsUnavailable,
                ExtensionManagementLimitation::FragmentUrlContentScriptsUnavailable,
            ),
            (
                BetaCompatibilityLimitation::ContentScriptFontsUnavailable,
                ExtensionManagementLimitation::ContentScriptFontsUnavailable,
            ),
            (
                BetaCompatibilityLimitation::SidePanelUnavailable,
                ExtensionManagementLimitation::SidePanelUnavailable,
            ),
            (
                BetaCompatibilityLimitation::OffscreenLocalStorageOnly,
                ExtensionManagementLimitation::OffscreenLocalStorageOnly,
            ),
            (
                BetaCompatibilityLimitation::SandboxedPagesUnavailable,
                ExtensionManagementLimitation::SandboxedPagesUnavailable,
            ),
            (
                BetaCompatibilityLimitation::ClipboardReadUnavailable,
                ExtensionManagementLimitation::ClipboardReadUnavailable,
            ),
        ] {
            assert_eq!(project_beta_limitation(source).unwrap(), Some(projected));
        }
    }
}
impl ServiceRepository {
    pub(crate) fn collect_external_package_garbage(
        &mut self,
        store: &ExtensionServiceStoreAuthority,
        deadline: Instant,
    ) -> Result<
        zephium_extension_repository::beta::BetaStorageCollection,
        ExtensionStorePackagePreparationOutcome,
    > {
        use zephium_store::ExtensionPackageStorageRootsLoadOutcome as Roots;
        let Call::Completed(Roots::Loaded(mut roots)) =
            store.load_package_storage_roots_until(deadline)
        else {
            return Err(ExtensionStorePackagePreparationOutcome::Unavailable);
        };
        if let Some(candidate) = &self.external_candidate {
            roots.push(ExtensionBetaObjectDigest::from_bytes(
                candidate.package.id().bytes(),
            ));
        }
        if let Some(pending) = &self.external_update {
            roots.push(ExtensionBetaObjectDigest::from_bytes(
                pending.current.package.id().bytes(),
            ));
            roots.push(ExtensionBetaObjectDigest::from_bytes(
                pending.replacement.package.id().bytes(),
            ));
        }
        self.external
            .as_mut()
            .ok_or(ExtensionStorePackagePreparationOutcome::Unavailable)?
            .collect_unreferenced(&roots, deadline)
            .map_err(repository_storage_outcome)
    }

    pub(crate) fn clear_completed_external_candidate(
        &mut self,
        selector: &ExtensionInstallCandidateSelector,
    ) {
        if self
            .external_candidate
            .as_ref()
            .is_some_and(|candidate| &candidate.selector == selector)
        {
            self.external_candidate = None;
        }
    }

    pub(crate) fn dismiss_external_update(
        &mut self,
        selector: &ExtensionInstallUpdateSelector,
    ) -> bool {
        if self.external_update.as_ref().is_some_and(|pending| {
            pending.selector == selector.install()
                && pending.replacement.manifest.package() == selector.replacement()
        }) {
            self.external_update = None;
            true
        } else {
            false
        }
    }

    pub(crate) fn prepare_store_package(
        &mut self,
        store: &ExtensionServiceStoreAuthority,
        profile: ProfileId,
        request: ExtensionStorePackageRequest,
        deadline: Instant,
    ) -> ExtensionStorePackagePreparationOutcome {
        use ExtensionStorePackagePreparationOutcome as Outcome;
        // Keep the existing review (and its garbage-collection roots) until a
        // replacement is completely authenticated and materialized. A failed,
        // stale or no-change check must not consume another user decision.
        if request.is_background_update()
            && (self.external_candidate.is_some() || self.external_update.is_some())
        {
            return Outcome::Unavailable;
        }
        let prepare = (|| {
            if Instant::now() >= deadline {
                return Err(Outcome::Unavailable);
            }
            self.collect_external_package_garbage(store, deadline)?;
            let Call::Completed(ExtensionInstallCatalogLoadOutcome::Loaded(catalog)) =
                store.load_install_catalog_until(profile, deadline)
            else {
                return Err(Outcome::Unavailable);
            };
            let id = ChromiumExtensionId::parse(request.extension_id())
                .map_err(|_| Outcome::InvalidPackage)?;
            #[cfg(feature = "capabilities-v2-qa")]
            let qa_trace = id.as_str() == "nngceckbapebfimnlniiiahkandclblb"
                && std::env::var("ZEPHIUM_OFFSCREEN_QA_TRACE").as_deref() == Ok("1");
            #[cfg(not(feature = "capabilities-v2-qa"))]
            let qa_trace = false;
            let mut archive = zephium_extension_acquisition::AcquiredExtensionArchive::authenticate_upstream_crx3(request.bytes(), &id, None).map_err(|_| {
                if qa_trace { eprintln!("bitwarden-qa-prepare: archive-authentication-refused"); }
                Outcome::InvalidPackage
            })?;
            let mut manifest = Vec::new();
            let mut receipts = Vec::with_capacity(archive.files().len());
            for i in 0..archive.files().len() {
                if Instant::now() >= deadline {
                    return Err(Outcome::Unavailable);
                }
                let receipt = if archive.files()[i].path().as_str() == "manifest.json" {
                    archive.copy_file(i, &mut manifest)
                } else {
                    archive.copy_file(i, &mut std::io::sink())
                }
                .map_err(|_| {
                    if qa_trace {
                        eprintln!("bitwarden-qa-prepare: archive-resource-refused");
                    }
                    Outcome::InvalidPackage
                })?;
                receipts.push(receipt);
            }
            let receipt = archive.finish_tree(receipts).map_err(|_| {
                if qa_trace {
                    eprintln!("bitwarden-qa-prepare: source-tree-refused");
                }
                Outcome::InvalidPackage
            })?;
            let publisher = ExtensionPackageKey::from_bytes(receipt.developer_key_sha256().bytes());
            let update_current = if let Some(selector) = request.update_selector() {
                if selector.profile() != profile
                    || selector.catalog_revision() != catalog.revision()
                {
                    return Err(Outcome::Unavailable);
                }
                let install = catalog
                    .get(selector.install())
                    .filter(|install| {
                        install.revision() == selector.install_revision()
                            && install.package().key() == publisher
                    })
                    .ok_or(Outcome::InvalidPackage)?;
                let current = self
                    .external_installed_candidate(
                        store,
                        profile,
                        install,
                        catalog.revision(),
                        deadline,
                    )
                    .ok_or(Outcome::FailedClosed)?;
                if current.provenance.source() != &ExtensionProvenanceSource::ChromeWebStore {
                    return Err(Outcome::InvalidPackage);
                }
                let incoming = receipt
                    .upstream_checkpoint(&manifest)
                    .map_err(|_| Outcome::InvalidPackage)?;
                let installed = current.provenance.upstream();
                // A newly signed CRX envelope around the exact same publisher,
                // version and ZIP payload is not an executable update. Keep the
                // original installed provenance; do not mint a new admission.
                if incoming.publisher() == installed.publisher()
                    && incoming.version() == installed.version()
                    && incoming.archive_sha256() == installed.archive_sha256()
                {
                    #[cfg(feature = "capabilities-v2-qa")]
                    let refresh = if id.as_str() == "nngceckbapebfimnlniiiahkandclblb"
                        && std::env::var("ZEPHIUM_BITWARDEN_CAPABILITIES_V2_QA").as_deref()
                            == Ok("1")
                        && !request.is_background_update()
                    {
                        signed_bitwarden_qa_same_source_refresh(current.provenance.transform())?
                    } else {
                        false
                    };
                    #[cfg(not(feature = "capabilities-v2-qa"))]
                    let refresh = false;
                    if !refresh {
                        return Err(Outcome::UpToDate);
                    }
                }
                Some((selector, current))
            } else {
                if catalog
                    .installs()
                    .iter()
                    .any(|install| install.package().key() == publisher)
                {
                    return Err(Outcome::AlreadyInstalled);
                }
                None
            };
            let revision = match &update_current {
                Some((_, current)) => current
                    .manifest
                    .package()
                    .revision()
                    .next()
                    .ok_or(Outcome::FailedClosed)?,
                None => ExtensionPackageRevision::INITIAL,
            };
            let Call::Completed(ExtensionUpstreamCheckpointLoadOutcome::Loaded(previous)) =
                store.load_upstream_checkpoint_until(profile, publisher, deadline)
            else {
                return Err(Outcome::Unavailable);
            };
            #[cfg(target_os = "macos")]
            let runtime = BetaRuntimeTarget::MacosNative;
            #[cfg(not(target_os = "macos"))]
            let runtime = BetaRuntimeTarget::WindowsNative;
            if !cfg!(any(target_os = "macos", target_os = "windows")) {
                return Err(Outcome::Unsupported(ExtensionUnsupportedFeatures::new([])));
            }
            #[cfg(feature = "capabilities-v2-qa")]
            let admitted = if id.as_str() == "nngceckbapebfimnlniiiahkandclblb"
                && std::env::var("ZEPHIUM_BITWARDEN_CAPABILITIES_V2_QA").as_deref() == Ok("1")
            {
                if qa_trace {
                    eprintln!("bitwarden-qa-prepare: exact-qa-admission-selected");
                }
                admit_signed_bitwarden_capabilities_v2_qa(
                    receipt, &manifest, runtime, revision, previous,
                )
            } else {
                admit_external_source(receipt, &manifest, runtime, revision, previous)
            };
            #[cfg(not(feature = "capabilities-v2-qa"))]
            let admitted = admit_external_source(receipt, &manifest, runtime, revision, previous);
            let source = admitted.map_err(|error| {
                if qa_trace {
                    let class = match &error {
                        BetaSourceAdmissionError::PolicyUnavailable => "policy-unavailable",
                        BetaSourceAdmissionError::TargetNotEnabled => "target-not-enabled",
                        BetaSourceAdmissionError::Revoked => "revoked",
                        BetaSourceAdmissionError::SourceMismatch => "source-mismatch",
                        BetaSourceAdmissionError::UpstreamRollback => "upstream-rollback",
                        BetaSourceAdmissionError::Unsupported(_) => "unsupported",
                        BetaSourceAdmissionError::Capacity => "capacity",
                    };
                    eprintln!("bitwarden-qa-prepare: admission-refused class={class}");
                }
                match error {
                    BetaSourceAdmissionError::Unsupported(features) => {
                        Outcome::Unsupported(features)
                    }
                    _ => Outcome::InvalidPackage,
                }
            })?;
            let root = self.root.path().with_file_name("extension-preparation-v1");
            let mut workspace = BetaPreparationWorkspace::open(
                LockedPrivateNamespace::open_or_create(root).map_err(|_| Outcome::FailedClosed)?,
            )
            .map_err(|_| Outcome::FailedClosed)?;
            workspace.discard().map_err(|_| Outcome::FailedClosed)?;
            let artifact = workspace
                .prepare_external(source, request.bytes())
                .map_err(|error| {
                    if qa_trace {
                        let class = match error {
                            zephium_extension_distribution::beta::BetaArtifactPreparationError::Transform => "transform",
                            zephium_extension_distribution::beta::BetaArtifactPreparationError::Source => "source",
                            zephium_extension_distribution::beta::BetaArtifactPreparationError::Occupied => "occupied",
                            zephium_extension_distribution::beta::BetaArtifactPreparationError::Busy => "busy",
                            zephium_extension_distribution::beta::BetaArtifactPreparationError::Policy => "policy",
                            zephium_extension_distribution::beta::BetaArtifactPreparationError::Stale => "stale",
                            zephium_extension_distribution::beta::BetaArtifactPreparationError::Storage => "storage",
                            zephium_extension_distribution::beta::BetaArtifactPreparationError::Integrity => "integrity",
                        };
                        eprintln!("bitwarden-qa-prepare: artifact-refused class={class}");
                    }
                    preparation_outcome(error)
                })?;
            if Instant::now() >= deadline {
                return Err(Outcome::Unavailable);
            }
            let package = self
                .external
                .as_mut()
                .ok_or(Outcome::Unavailable)?
                .materialize(artifact)
                .map_err(repository_storage_outcome)?;
            workspace.discard().map_err(|_| Outcome::FailedClosed)?;
            let manifest = Arc::new(
                package
                    .manifest()
                    .map_err(|_| Outcome::FailedClosed)?
                    .descriptor()
                    .clone(),
            );
            let provenance = Arc::new(
                package
                    .provenance(ExtensionProvenanceSource::ChromeWebStore)
                    .map_err(|_| Outcome::FailedClosed)?,
            );
            let display = package
                .resolve_metadata()
                .map_err(|_| Outcome::InvalidPackage)?;
            let selector = ExtensionInstallCandidateSelector::new(
                profile,
                catalog.revision(),
                ExtensionCatalogSetDigest::from_bytes(package.id().bytes()),
                manifest.package().clone(),
            );
            let candidate = ExternalCandidate {
                selector,
                package,
                manifest,
                provenance,
                display,
            };
            if let Some((selector, current)) = update_current {
                self.external_candidate = None;
                self.external_update = Some(Box::new(ExternalUpdateCandidate {
                    selector,
                    current,
                    replacement: candidate,
                }));
                return Err(Outcome::UpdateAvailable);
            }
            let review = candidate.review()?;
            self.external_update = None;
            self.external_candidate = Some(candidate);
            Ok(review)
        })();
        match prepare {
            Ok(review) => Outcome::Prepared(Box::new(review)),
            Err(outcome) => outcome,
        }
    }

    pub(crate) fn external_pending_review(
        &self,
        profile: ProfileId,
    ) -> Option<ExtensionInstallCandidateEntry> {
        self.external_candidate
            .as_ref()
            .filter(|candidate| candidate.selector.profile() == profile)?
            .review()
            .ok()
    }
    pub(crate) fn external_installed_candidate(
        &mut self,
        store: &ExtensionServiceStoreAuthority,
        profile: ProfileId,
        install: &ExtensionInstall,
        revision: ExtensionInstallCatalogRevision,
        deadline: Instant,
    ) -> Option<ExternalCandidate> {
        let Call::Completed(ExtensionInstallProvenanceLoadOutcome::Loaded(Some(provenance))) =
            store.load_install_provenance_until(profile, install.id(), deadline)
        else {
            return None;
        };
        let Call::Completed(ExtensionUpstreamCheckpointLoadOutcome::Loaded(Some(high_water))) =
            store.load_upstream_checkpoint_until(
                profile,
                provenance.upstream().publisher(),
                deadline,
            )
        else {
            return None;
        };
        if provenance.package() != install.package() {
            return None;
        }
        let runtime = BetaRuntimeTarget::from_local_compatibility_target(
            provenance.runtime_target().as_str(),
        )?;
        let package = self
            .external
            .as_mut()?
            .reopen_external_bound(&provenance, high_water, runtime)
            .ok()?;
        let manifest = Arc::new(package.manifest().ok()?.descriptor().clone());
        let display = package.resolve_metadata().ok()?;
        let selector = ExtensionInstallCandidateSelector::new(
            profile,
            revision,
            ExtensionCatalogSetDigest::from_bytes(package.id().bytes()),
            manifest.package().clone(),
        );
        Some(ExternalCandidate {
            selector,
            package,
            manifest,
            provenance: Arc::from(provenance),
            display,
        })
    }

    pub(crate) fn external_install_manifest(
        &self,
        selector: &ExtensionInstallCandidateSelector,
    ) -> Option<(
        Arc<ExtensionManifestDescriptor>,
        Box<ExtensionInstallProvenance>,
    )> {
        let candidate = self.external_candidate.as_ref()?;
        if &candidate.selector != selector || candidate.package.verify().is_err() {
            return None;
        }
        Some((
            Arc::clone(&candidate.manifest),
            Box::new((*candidate.provenance).clone()),
        ))
    }
}

fn repository_storage_outcome(
    error: zephium_extension_repository::beta::BetaRepositoryError,
) -> ExtensionStorePackagePreparationOutcome {
    match error {
        zephium_extension_repository::beta::BetaRepositoryError::Capacity => {
            ExtensionStorePackagePreparationOutcome::StorageLimit
        }
        zephium_extension_repository::beta::BetaRepositoryError::StorageUnavailable => {
            ExtensionStorePackagePreparationOutcome::Unavailable
        }
        _ => ExtensionStorePackagePreparationOutcome::FailedClosed,
    }
}

#[cfg(feature = "capabilities-v2-qa")]
fn signed_bitwarden_qa_same_source_refresh(
    transform: &ExtensionTransformProvenance,
) -> Result<bool, ExtensionStorePackagePreparationOutcome> {
    let ExtensionTransformProvenance::Compiled {
        target,
        revision,
        sha256,
    } = transform
    else {
        return Ok(false);
    };
    if target.as_str() != "local.webkit-capabilities.v2" {
        return Ok(false);
    }
    let (legacy, disposal_only, current) = signed_bitwarden_qa_transform_refresh();
    if revision.get() == 4 && *sha256 == legacy && legacy != current {
        return Ok(true);
    }
    if revision.get() == 5 && *sha256 == disposal_only && disposal_only != current {
        return Ok(true);
    }
    if revision.get() == 6 && *sha256 == current {
        return Ok(false);
    }
    Err(ExtensionStorePackagePreparationOutcome::InvalidPackage)
}

fn preparation_outcome(
    error: zephium_extension_distribution::beta::BetaArtifactPreparationError,
) -> ExtensionStorePackagePreparationOutcome {
    use zephium_extension_distribution::beta::BetaArtifactPreparationError as Error;
    use ExtensionStorePackagePreparationOutcome as Outcome;
    match error {
        Error::Transform => Outcome::Unsupported(ExtensionUnsupportedFeatures::new([])),
        Error::Source => Outcome::InvalidPackage,
        Error::Occupied | Error::Busy | Error::Policy | Error::Stale => Outcome::Unavailable,
        Error::Storage | Error::Integrity => Outcome::FailedClosed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "capabilities-v2-qa")]
    #[test]
    fn exact_qa_recipe_refresh_admits_only_the_pinned_legacy_transform() {
        let target = ExtensionCompatibilityTargetId::parse_exact("local.webkit-capabilities.v2")
            .unwrap();
        let (legacy, disposal_only, current) = signed_bitwarden_qa_transform_refresh();
        assert_ne!(legacy, current);
        let transform = |revision, sha256| ExtensionTransformProvenance::Compiled {
            target: target.clone(),
            revision: std::num::NonZeroU32::new(revision).unwrap(),
            sha256,
        };
        assert!(matches!(
            signed_bitwarden_qa_same_source_refresh(&transform(4, legacy)),
            Ok(true)
        ));
        assert!(matches!(
            signed_bitwarden_qa_same_source_refresh(&transform(5, disposal_only)),
            Ok(true)
        ));
        assert!(matches!(
            signed_bitwarden_qa_same_source_refresh(&transform(6, current)),
            Ok(false)
        ));
        for refused in [transform(4, [0; 32]), transform(5, legacy), transform(5, current)] {
            assert!(matches!(
                signed_bitwarden_qa_same_source_refresh(&refused),
                Err(ExtensionStorePackagePreparationOutcome::InvalidPackage)
            ));
        }
    }

    #[test]
    fn unavailable_compatibility_is_not_reported_as_storage_corruption() {
        use zephium_extension_distribution::beta::BetaArtifactPreparationError as Error;
        assert!(matches!(
            preparation_outcome(Error::Transform),
            ExtensionStorePackagePreparationOutcome::Unsupported(_)
        ));
        assert!(matches!(
            preparation_outcome(Error::Source),
            ExtensionStorePackagePreparationOutcome::InvalidPackage
        ));
        assert!(matches!(
            preparation_outcome(Error::Integrity),
            ExtensionStorePackagePreparationOutcome::FailedClosed
        ));
    }
}
