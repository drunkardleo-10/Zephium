use std::io::Cursor;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use zephium_core::extensions::{
    ApiPermissionName, ExtensionApiPermissionSet, ExtensionArchiveDigest, ExtensionAuthorityId,
    ExtensionCatalogGenerationRole, ExtensionCatalogSetDigest,
    ExtensionCompatibilityClassification, ExtensionCompatibilityLevel,
    ExtensionCompatibilityTargetId, ExtensionContentSecurityPolicyDeclaration,
    ExtensionExpectedNativeOwnershipIdentity, ExtensionGrantAuthority,
    ExtensionGrantBrowsingContext, ExtensionGrantCohort, ExtensionGrantDigest,
    ExtensionGrantManifestBinding, ExtensionGrantManifestBindings, ExtensionGrantRevision,
    ExtensionHostPermissionSet, ExtensionInstall, ExtensionInstallCatalog,
    ExtensionInstallCatalogRevision, ExtensionInstallRevision, ExtensionManifestDeclarations,
    ExtensionManifestDescriptor, ExtensionManifestDigest, ExtensionManifestExecutionSurfaces,
    ExtensionManifestResourceDigest, ExtensionNativeGrantDecision, ExtensionNativeGrantRequirement,
    ExtensionNativeIncarnation, ExtensionNativeOwnershipEntry,
    ExtensionNativeOwnershipEntryRevision, ExtensionNativeOwnershipIdentity,
    ExtensionNativeOwnershipIntent, ExtensionNativeOwnershipJournalError,
    ExtensionNativeOwnershipKey, ExtensionNativeOwnershipOperation, ExtensionNativeOwnershipPhase,
    ExtensionPackageIdentity, ExtensionPackageKey, ExtensionPackagePayloadIdentity,
    ExtensionPackagePinAcquisitionBinding, ExtensionPackagePinHeldBinding,
    ExtensionPackageRevision, ExtensionRuntimeBackendTarget, ExtensionRuntimeFingerprint,
    ExtensionRuntimeGeneration, ExtensionRuntimeOperationAuthority, ExtensionTreeDigest,
};
use zephium_core::ids::{ExtensionInstallId, ProfileId};
use zephium_core::injection::{MatchOptions, MatchPattern, MatchSet};

use super::*;
use crate::{
    ExtensionPackageAccessError, ExtensionPackageAccessPort, ExtensionPackageAccessView,
    ExtensionRuntimeActivationDisposition, ExtensionRuntimeFailure,
    ExtensionRuntimeNativeRootLeasePort, ExtensionRuntimeNativeRootVisitor,
    ExtensionRuntimeOwnershipDisposition, ExtensionRuntimeResource,
    ExtensionRuntimeResourceBinding, ExtensionRuntimeResourcePlan, ExtensionRuntimeResourceVisitor,
    ExtensionRuntimeRetirementDisposition,
};

const ALL_URLS: &str = "<all_urls>";
const EXPECTED_NATIVE_ID: &str = "abcdefghijklmnopabcdefghijklmnop";
const OTHER_NATIVE_ID: &str = "ponmlkjihgfedcbaponmlkjihgfedcba";

fn api(names: &[&str]) -> ExtensionApiPermissionSet {
    ExtensionApiPermissionSet::new(
        names
            .iter()
            .map(|name| ApiPermissionName::parse_exact(name).expect("valid API permission"))
            .collect(),
    )
    .expect("canonical API set")
}

fn package(seed: u8) -> ExtensionPackageIdentity {
    ExtensionPackageIdentity::new(
        ExtensionAuthorityId::from_bytes([seed; 32]),
        ExtensionPackageKey::from_bytes([seed.wrapping_add(1); 32]),
        ExtensionPackageRevision::INITIAL,
        ExtensionPackagePayloadIdentity::acquired_zip(
            2,
            ExtensionArchiveDigest::from_bytes([seed.wrapping_add(2); 32]),
        )
        .expect("bounded archive"),
        ExtensionManifestDigest::from_bytes([seed.wrapping_add(3); 32]),
        ExtensionTreeDigest::from_bytes([seed.wrapping_add(4); 32]),
    )
}

fn manifest(seed: u8) -> Arc<ExtensionManifestDescriptor> {
    let declarations = ExtensionManifestDeclarations::new(
        api(&[]),
        api(&["activeTab", "scripting", "tabs"]),
        None,
        Some(
            ExtensionHostPermissionSet::new(
                MatchSet::parse(
                    [ALL_URLS],
                    std::iter::empty::<&str>(),
                    MatchOptions::default(),
                )
                .expect("all URLs match set"),
            )
            .expect("bounded host permissions"),
        ),
        None,
        None,
        Vec::new(),
        ExtensionManifestExecutionSurfaces::new(
            Vec::new(),
            ExtensionContentSecurityPolicyDeclaration::new(
                ExtensionManifestResourceDigest::from_bytes([seed.wrapping_add(5); 32]),
            ),
            None,
            Vec::new(),
        )
        .expect("bounded execution surfaces"),
        Vec::new(),
    )
    .expect("valid manifest declarations");
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
            package(seed),
            3,
            declarations,
            ExtensionCompatibilityTargetId::parse_exact("test.runtime.host.v1")
                .expect("compatibility target"),
            compatibility,
        )
        .expect("valid manifest"),
    )
}

fn eligibility(
    seed: u8,
    profile: ProfileId,
    install_id: ExtensionInstallId,
) -> zephium_core::extensions::ExtensionRuntimeEligibility {
    eligibility_at_revision(
        seed,
        profile,
        install_id,
        ExtensionGrantRevision::INITIAL,
        &["activeTab", "scripting"],
    )
}

fn eligibility_at_revision(
    seed: u8,
    profile: ProfileId,
    install_id: ExtensionInstallId,
    grant_revision: ExtensionGrantRevision,
    granted_api: &[&str],
) -> zephium_core::extensions::ExtensionRuntimeEligibility {
    let manifest = manifest(seed);
    let install = ExtensionInstall::from_persisted(
        install_id,
        ExtensionInstallRevision::new(u64::from(seed) + 10).expect("nonzero revision"),
        manifest.package().clone(),
        true,
    );
    let catalog = ExtensionInstallCatalog::from_persisted(
        ExtensionInstallCatalogRevision::new(u64::from(seed) + 20).expect("nonzero revision"),
        Some(install_id),
        vec![install.clone()],
    )
    .expect("valid install catalog");
    let bindings = ExtensionGrantManifestBindings::new(vec![ExtensionGrantManifestBinding::new(
        install_id,
        Arc::clone(&manifest),
    )])
    .expect("valid manifest bindings");
    let authority = ExtensionGrantAuthority::from_persisted(
        &install,
        grant_revision,
        manifest.package().clone(),
        granted_api
            .iter()
            .copied()
            .map(|name| ApiPermissionName::parse_exact(name).expect("valid API permission"))
            .collect(),
        vec![MatchPattern::parse(ALL_URLS).expect("all URLs pattern")],
        false,
        false,
        &manifest,
    )
    .expect("valid grant authority");
    ExtensionGrantCohort::from_persisted(
        profile,
        zephium_core::extensions::ExtensionProfilePolicy::initial(),
        catalog,
        bindings,
        vec![authority],
    )
    .expect("valid grant cohort")
    .runtime_eligibility(install_id, ExtensionGrantBrowsingContext::Regular)
    .expect("eligible runtime")
}

#[derive(Clone)]
struct EntryTemplate {
    key: ExtensionNativeOwnershipKey,
    package: ExtensionPackageIdentity,
    catalog_set_digest: ExtensionCatalogSetDigest,
    catalog_role: ExtensionCatalogGenerationRole,
    catalog_revision: ExtensionInstallCatalogRevision,
    install_revision: ExtensionInstallRevision,
    grant_revision: ExtensionGrantRevision,
    grant_digest: ExtensionGrantDigest,
    backend: ExtensionRuntimeBackendTarget,
}

impl EntryTemplate {
    fn from_eligibility(
        eligibility: &zephium_core::extensions::ExtensionRuntimeEligibility,
        seed: u8,
        backend: ExtensionRuntimeBackendTarget,
    ) -> Self {
        Self {
            key: ExtensionNativeOwnershipKey::new(
                eligibility.profile(),
                eligibility.install_id(),
                eligibility.browsing_context(),
            ),
            package: eligibility.package().clone(),
            catalog_set_digest: ExtensionCatalogSetDigest::from_bytes([seed.wrapping_add(6); 32]),
            catalog_role: ExtensionCatalogGenerationRole::Active,
            catalog_revision: eligibility.catalog_revision(),
            install_revision: eligibility.install_revision(),
            grant_revision: eligibility.grant_revision(),
            grant_digest: eligibility.grant_digest(),
            backend,
        }
    }

    fn entry(
        &self,
        revision: u64,
        intent: ExtensionNativeOwnershipIntent,
        phase: ExtensionNativeOwnershipPhase,
        native_identity: Option<ExtensionNativeOwnershipIdentity>,
    ) -> ExtensionNativeOwnershipEntry {
        self.entry_with_identities(revision, intent, phase, None, native_identity)
    }

    fn with_grants_from(
        &self,
        eligibility: &zephium_core::extensions::ExtensionRuntimeEligibility,
    ) -> Self {
        let mut rebound = self.clone();
        rebound.grant_revision = eligibility.grant_revision();
        rebound.grant_digest = eligibility.grant_digest();
        rebound
    }

    fn entry_with_identities(
        &self,
        revision: u64,
        intent: ExtensionNativeOwnershipIntent,
        phase: ExtensionNativeOwnershipPhase,
        expected_native_identity: Option<ExtensionExpectedNativeOwnershipIdentity>,
        native_identity: Option<ExtensionNativeOwnershipIdentity>,
    ) -> ExtensionNativeOwnershipEntry {
        ExtensionNativeOwnershipEntry::from_persisted_with_native_identities(
            self.key,
            ExtensionNativeOwnershipOperation::INITIAL,
            ExtensionNativeOwnershipEntryRevision::new(revision).expect("nonzero entry revision"),
            self.package.clone(),
            self.catalog_set_digest,
            self.catalog_role,
            self.catalog_revision,
            self.install_revision,
            self.grant_revision,
            self.grant_digest,
            self.backend,
            expected_native_identity,
            native_identity,
            ExtensionNativeIncarnation::INITIAL,
            intent,
            phase,
        )
        .expect("valid ownership row")
    }

    fn preparing(&self) -> ExtensionNativeOwnershipEntry {
        self.entry(
            1,
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeAbsentPreparing,
            None,
        )
    }

    fn may_own(&self) -> ExtensionNativeOwnershipEntry {
        self.entry(
            2,
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeMayOwn,
            None,
        )
    }

    fn owned(
        &self,
        identity: Option<ExtensionNativeOwnershipIdentity>,
    ) -> ExtensionNativeOwnershipEntry {
        self.entry(
            3,
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeOwned,
            identity,
        )
    }

    fn release_absent(
        &self,
        revision: u64,
        identity: Option<ExtensionNativeOwnershipIdentity>,
    ) -> ExtensionNativeOwnershipEntry {
        self.entry(
            revision,
            ExtensionNativeOwnershipIntent::Release,
            ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
            identity,
        )
    }
}

struct PinnedProvider {
    _held: ExtensionPackagePinHeldBinding,
    identity: Arc<()>,
    dropped: Arc<AtomicUsize>,
    retained_bytes: usize,
    native_root_lease: Option<Box<PinnedNativeRootLease>>,
}

struct PinnedNativeRootLease;

impl ExtensionRuntimeNativeRootLeasePort for PinnedNativeRootLease {
    fn visit_native_root(
        &mut self,
        visitor: &mut dyn ExtensionRuntimeNativeRootVisitor,
    ) -> Result<(), ExtensionPackageAccessError> {
        #[cfg(unix)]
        let root = Path::new("/private/var/zephium/extensions/runtime-host-test");
        #[cfg(windows)]
        let root = Path::new(r"C:\Zephium\extensions\runtime-host-test");
        let _ = visitor.visit(root);
        Ok(())
    }
}

impl Drop for PinnedProvider {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::Relaxed);
    }
}

impl ExtensionPackageAccessPort for PinnedProvider {
    fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    fn visit_resource(
        &mut self,
        resource: ExtensionRuntimeResource,
        visitor: &mut dyn ExtensionRuntimeResourceVisitor,
    ) -> Result<(), ExtensionPackageAccessError> {
        let bytes = vec![0_u8; resource.declared_bytes() as usize];
        let _ = visitor.visit(&mut Cursor::new(bytes));
        Ok(())
    }

    fn take_native_root_lease(
        &mut self,
        target: ExtensionRuntimeTarget,
    ) -> Result<Box<dyn ExtensionRuntimeNativeRootLeasePort>, ExtensionPackageAccessError> {
        if target != ExtensionRuntimeTarget::NativeWebExtension {
            return Err(ExtensionPackageAccessError::NativeRootUnavailable);
        }
        self.native_root_lease
            .take()
            .map(|lease| lease as Box<dyn ExtensionRuntimeNativeRootLeasePort>)
            .ok_or(ExtensionPackageAccessError::NativeRootUnavailable)
    }
}

struct ActivationFixture {
    template: EntryTemplate,
    initial: ExtensionNativeOwnershipEntry,
    access: ExtensionPackageAccess,
    authority: ExtensionRuntimeOperationAuthority,
    expectation: ExtensionRuntimeNativeIdentityExpectation,
    expected_native_identity: Option<ExtensionExpectedNativeOwnershipIdentity>,
    native_identity: Option<ExtensionNativeOwnershipIdentity>,
    evidence: ExtensionRuntimeOwnershipEvidence,
    provider_identity: Arc<()>,
    provider_dropped: Arc<AtomicUsize>,
}

impl ActivationFixture {
    fn compatibility(seed: u8) -> Self {
        Self::new(
            seed,
            ExtensionRuntimeBackendTarget::LinuxCompatibility,
            ExtensionRuntimeTarget::Compatibility,
            ExtensionRuntimeNativeIdentityExpectation::Compatibility,
            None,
            ExtensionRuntimeOwnershipEvidence::Compatibility,
            0,
        )
    }

    fn macos(seed: u8) -> Self {
        let owner = ExtensionRuntimeNativeOwnerId::parse_exact(EXPECTED_NATIVE_ID)
            .expect("canonical native owner ID");
        let native_identity = ExtensionNativeOwnershipIdentity::parse(
            ExtensionRuntimeBackendTarget::MacosNative,
            EXPECTED_NATIVE_ID,
        )
        .expect("canonical durable native ID");
        Self::new(
            seed,
            ExtensionRuntimeBackendTarget::MacosNative,
            ExtensionRuntimeTarget::NativeWebExtension,
            ExtensionRuntimeNativeIdentityExpectation::MacosWebExtension(owner),
            Some(native_identity),
            ExtensionRuntimeOwnershipEvidence::MacosWebExtension(owner),
            0,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        seed: u8,
        backend: ExtensionRuntimeBackendTarget,
        target: ExtensionRuntimeTarget,
        expectation: ExtensionRuntimeNativeIdentityExpectation,
        native_identity: Option<ExtensionNativeOwnershipIdentity>,
        evidence: ExtensionRuntimeOwnershipEvidence,
        provider_retained_bytes: usize,
    ) -> Self {
        let profile = ProfileId::from(u128::from(seed) + 100);
        let install_id = ExtensionInstallId::from(u128::from(seed) + 200);
        let eligibility = eligibility(seed, profile, install_id);
        let template = EntryTemplate::from_eligibility(&eligibility, seed, backend);
        let acquisition =
            ExtensionPackagePinAcquisitionBinding::mint(&template.preparing(), eligibility)
                .expect("authentic package-pin acquisition");
        let (held, authority) = acquisition
            .into_runtime_parts(
                ExtensionRuntimeGeneration::new(u64::from(seed) + 1)
                    .expect("nonzero runtime generation"),
            )
            .into_held_binding_and_operation_authority();
        let plan =
            ExtensionRuntimeResourcePlan::try_new(vec![ExtensionRuntimeResourceBinding::try_new(
                "manifest.json",
                2,
                [seed; 32],
            )
            .expect("manifest resource binding")])
            .expect("runtime resource plan");
        let provider_identity = Arc::new(());
        let provider_dropped = Arc::new(AtomicUsize::new(0));
        let access = ExtensionPackageAccess::from_delegated_provider(
            target,
            plan,
            Box::new(PinnedProvider {
                _held: held,
                identity: Arc::clone(&provider_identity),
                dropped: Arc::clone(&provider_dropped),
                retained_bytes: provider_retained_bytes,
                native_root_lease: Some(Box::new(PinnedNativeRootLease)),
            }),
        )
        .expect("bounded authenticated package access");
        let expected_native_identity = expectation
            .durable_expected_identity()
            .expect("runtime and Core native ID grammars agree");
        let initial = template.entry_with_identities(
            2,
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeMayOwn,
            expected_native_identity,
            None,
        );
        Self {
            template,
            initial,
            access,
            authority,
            expectation,
            expected_native_identity,
            native_identity,
            evidence,
            provider_identity,
            provider_dropped,
        }
    }

    fn runtime(&self) -> ExtensionRuntimeFingerprint {
        self.authority.fingerprint().clone()
    }

    fn owned(&self) -> ExtensionNativeOwnershipEntry {
        self.template.entry_with_identities(
            3,
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeOwned,
            self.expected_native_identity,
            self.native_identity,
        )
    }

    fn release(&self) -> ExtensionNativeOwnershipEntry {
        self.template.entry_with_identities(
            4,
            ExtensionNativeOwnershipIntent::Release,
            ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
            self.expected_native_identity,
            self.native_identity,
        )
    }

    fn into_binding(self) -> ExtensionRuntimeHostActivationBinding {
        ExtensionRuntimeHostActivationBinding::try_from_authenticated_repository(
            self.initial,
            self.access,
            self.authority,
            self.expectation,
        )
        .expect("valid authenticated host binding")
    }
}

fn same_fingerprint_different_lineage_authority(
    seed: u8,
    backend: ExtensionRuntimeBackendTarget,
) -> ExtensionRuntimeOperationAuthority {
    let profile = ProfileId::from(u128::from(seed) + 100);
    let install_id = ExtensionInstallId::from(u128::from(seed) + 200);
    let eligibility = eligibility(seed, profile, install_id);
    let mut template = EntryTemplate::from_eligibility(&eligibility, seed, backend);
    template.catalog_set_digest =
        ExtensionCatalogSetDigest::from_bytes([seed.wrapping_add(99); 32]);
    let acquisition =
        ExtensionPackagePinAcquisitionBinding::mint(&template.preparing(), eligibility)
            .expect("authentic alternate-lineage package pin");
    let (_held, authority) = acquisition
        .into_runtime_parts(
            ExtensionRuntimeGeneration::new(u64::from(seed) + 1)
                .expect("nonzero runtime generation"),
        )
        .into_held_binding_and_operation_authority();
    authority
}

#[derive(Default)]
struct HostProbe {
    activation_binds: AtomicUsize,
    consumed_activation_snapshots: AtomicUsize,
    recovery_binds: AtomicUsize,
    reservations: AtomicUsize,
    preattachment_restores: AtomicUsize,
    lifecycle_calls: AtomicUsize,
    publication_calls: AtomicUsize,
    reclaim_calls: AtomicUsize,
    active_witness_calls: AtomicUsize,
    document_witness_calls: AtomicUsize,
    publication_drops: AtomicUsize,
    last_activation_grants: Mutex<Option<ActivationGrantObservation>>,
    last_publication: Mutex<Option<PublicationObservation>>,
    last_witness: Mutex<Option<WitnessObservation>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ActivationGrantObservation {
    runtime: ExtensionRuntimeFingerprint,
    api: Vec<(
        String,
        ExtensionNativeGrantRequirement,
        ExtensionNativeGrantDecision,
    )>,
    hosts: Vec<(
        String,
        ExtensionNativeGrantRequirement,
        ExtensionNativeGrantDecision,
    )>,
    file_scheme_access: bool,
    private_context_access: bool,
}

#[derive(Clone)]
struct PublicationObservation {
    owner: ExtensionRuntimeOwnerAddress,
    generation: ExtensionRuntimeHostRegistryGeneration,
    owned_cas: zephium_core::extensions::ExtensionNativeOwnershipEntryCas,
    evidence: ExtensionRuntimeOwnershipEvidence,
}

#[derive(Clone)]
struct WitnessObservation {
    owner: ExtensionRuntimeOwnerAddress,
    generation: ExtensionRuntimeHostRegistryGeneration,
    runtime: ExtensionRuntimeFingerprint,
}

struct Reservation {
    probe: Arc<HostProbe>,
    attached: AtomicBool,
}

impl Reservation {
    fn new(probe: Arc<HostProbe>) -> Arc<Self> {
        probe.reservations.fetch_add(1, Ordering::Relaxed);
        Arc::new(Self {
            probe,
            attached: AtomicBool::new(false),
        })
    }

    fn attach(&self) {
        self.attached.store(true, Ordering::Relaxed);
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if !self.attached.load(Ordering::Relaxed) {
            self.probe
                .preattachment_restores
                .fetch_add(1, Ordering::Relaxed);
        }
    }
}

struct FakeLifecycle {
    probe: Arc<HostProbe>,
    reservation: Arc<Reservation>,
    retained_bytes: usize,
    absence_issuer: ExtensionRuntimeBoundAbsenceEvidenceIssuer,
    last_absence: Option<ExtensionRuntimeAbsenceEvidence>,
    next_absence_attempt: u64,
}

impl FakeLifecycle {
    fn mint_absence(&mut self) -> ExtensionRuntimeAbsenceEvidence {
        let attempt = std::num::NonZeroU64::new(self.next_absence_attempt)
            .expect("test absence attempt remains nonzero");
        self.next_absence_attempt = self
            .next_absence_attempt
            .checked_add(1)
            .expect("test absence attempt does not exhaust");
        let audit = ExtensionRuntimeCompatibilityAbsenceAudit::try_from_observations(
            true, true, true, true,
        )
        .expect("scripted compatibility runtime is fully quiescent");
        let evidence = self
            .absence_issuer
            .mint_compatibility_registry_absent_and_quiescent(attempt, audit)
            .expect("compatibility activation issuer accepts the complete audit");
        self.last_absence = Some(evidence);
        evidence
    }
}

impl ExtensionRuntimeOwnershipPort for FakeLifecycle {
    fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    fn accepts_absence_evidence(&self, evidence: ExtensionRuntimeAbsenceEvidence) -> bool {
        self.last_absence == Some(evidence)
            && self.absence_issuer.accepts(evidence, evidence.attempt())
    }

    fn retire_until(&mut self, _deadline: Instant) -> ExtensionRuntimeRetirementDisposition {
        self.reservation.attach();
        self.probe.lifecycle_calls.fetch_add(1, Ordering::Relaxed);
        ExtensionRuntimeRetirementDisposition::Retired(self.mint_absence())
    }

    fn reconcile_ownership_until(
        &mut self,
        _deadline: Instant,
    ) -> ExtensionRuntimeOwnershipDisposition {
        self.reservation.attach();
        self.probe.lifecycle_calls.fetch_add(1, Ordering::Relaxed);
        ExtensionRuntimeOwnershipDisposition::Absent(self.mint_absence())
    }
}

impl ExtensionRuntimeLifecyclePort for FakeLifecycle {
    fn activate_until(
        &mut self,
        access: &mut ExtensionPackageAccessView<'_>,
        _deadline: Instant,
    ) -> ExtensionRuntimeActivationDisposition {
        self.reservation.attach();
        self.probe.lifecycle_calls.fetch_add(1, Ordering::Relaxed);
        match access.target() {
            ExtensionRuntimeTarget::NativeWebExtension => {
                ExtensionRuntimeActivationDisposition::OwnershipUncertain {
                    failure: ExtensionRuntimeFailure::Internal,
                    evidence: None,
                }
            }
            ExtensionRuntimeTarget::Compatibility => {
                ExtensionRuntimeActivationDisposition::Activated(
                    ExtensionRuntimeOwnershipEvidence::Compatibility,
                )
            }
        }
    }
}

impl ExtensionRuntimeHostLifecyclePort for FakeLifecycle {}

struct FakeOwnership {
    probe: Arc<HostProbe>,
    reservation: Arc<Reservation>,
    retained_bytes: usize,
    absence_issuer: ExtensionRuntimeBoundAbsenceEvidenceIssuer,
    last_absence: Option<ExtensionRuntimeAbsenceEvidence>,
    next_absence_attempt: u64,
}

impl FakeOwnership {
    fn mint_absence(&mut self) -> ExtensionRuntimeAbsenceEvidence {
        let attempt = std::num::NonZeroU64::new(self.next_absence_attempt)
            .expect("test absence attempt remains nonzero");
        self.next_absence_attempt = self
            .next_absence_attempt
            .checked_add(1)
            .expect("test absence attempt does not exhaust");
        let audit = ExtensionRuntimeCompatibilityAbsenceAudit::try_from_observations(
            true, true, true, true,
        )
        .expect("scripted compatibility runtime is fully quiescent");
        let evidence = self
            .absence_issuer
            .mint_compatibility_registry_absent_and_quiescent(attempt, audit)
            .expect("compatibility recovery issuer accepts the complete audit");
        self.last_absence = Some(evidence);
        evidence
    }
}

impl ExtensionRuntimeOwnershipPort for FakeOwnership {
    fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    fn accepts_absence_evidence(&self, evidence: ExtensionRuntimeAbsenceEvidence) -> bool {
        self.last_absence == Some(evidence)
            && self.absence_issuer.accepts(evidence, evidence.attempt())
    }

    fn retire_until(&mut self, _deadline: Instant) -> ExtensionRuntimeRetirementDisposition {
        self.reservation.attach();
        self.probe.lifecycle_calls.fetch_add(1, Ordering::Relaxed);
        ExtensionRuntimeRetirementDisposition::Retired(self.mint_absence())
    }

    fn reconcile_ownership_until(
        &mut self,
        _deadline: Instant,
    ) -> ExtensionRuntimeOwnershipDisposition {
        self.reservation.attach();
        self.probe.lifecycle_calls.fetch_add(1, Ordering::Relaxed);
        ExtensionRuntimeOwnershipDisposition::Absent(self.mint_absence())
    }
}

impl ExtensionRuntimeHostOwnershipPort for FakeOwnership {}

#[derive(Clone, Copy, Default)]
enum PublicationBehavior {
    #[default]
    Normal,
    RefuseExact,
    RefuseSwapped,
    RefuseGrantExact,
    RefuseGrantSwapped,
    SwapReclaim,
    BadActiveWitness,
    BadDocumentWitness,
}

struct FakePublication {
    probe: Arc<HostProbe>,
    reservation: Arc<Reservation>,
    retained_bytes: usize,
    behavior: PublicationBehavior,
    authority: Option<ExtensionRuntimeOperationAuthority>,
    substitute: Option<ExtensionRuntimeOperationAuthority>,
    substitute_eligibility: Option<ExtensionRuntimeEligibility>,
    retained_eligibility: Option<ExtensionRuntimeEligibility>,
    reclaim_failures: usize,
}

impl Drop for FakePublication {
    fn drop(&mut self) {
        self.probe.publication_drops.fetch_add(1, Ordering::Relaxed);
    }
}

impl ExtensionRuntimeHostPublicationPort for FakePublication {
    fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    fn publish_operation_authority(
        &mut self,
        owner: ExtensionRuntimeOwnerAddress,
        generation: ExtensionRuntimeHostRegistryGeneration,
        owned_entry: &ExtensionNativeOwnershipEntry,
        evidence: ExtensionRuntimeOwnershipEvidence,
        authority: ExtensionRuntimeOperationAuthority,
    ) -> Result<(), ExtensionRuntimeHostPublicationPortRefusal> {
        self.probe.publication_calls.fetch_add(1, Ordering::Relaxed);
        *self.probe.last_publication.lock().expect("probe lock") = Some(PublicationObservation {
            owner,
            generation,
            owned_cas: owned_entry.cas(),
            evidence,
        });
        match self.behavior {
            PublicationBehavior::RefuseExact => {
                Err(ExtensionRuntimeHostPublicationPortRefusal::new(
                    ExtensionRuntimeHostBindError::Unavailable,
                    authority,
                ))
            }
            PublicationBehavior::RefuseSwapped => {
                self.authority = Some(authority);
                Err(ExtensionRuntimeHostPublicationPortRefusal::new(
                    ExtensionRuntimeHostBindError::Unavailable,
                    self.substitute.take().expect("substitute authority"),
                ))
            }
            PublicationBehavior::Normal
            | PublicationBehavior::RefuseGrantExact
            | PublicationBehavior::RefuseGrantSwapped
            | PublicationBehavior::SwapReclaim
            | PublicationBehavior::BadActiveWitness
            | PublicationBehavior::BadDocumentWitness => {
                self.reservation.attach();
                self.authority = Some(authority);
                Ok(())
            }
        }
    }

    fn rebind_operation_authority(
        &mut self,
        _owner: ExtensionRuntimeOwnerAddress,
        _generation: ExtensionRuntimeHostRegistryGeneration,
        current_entry: &ExtensionNativeOwnershipEntry,
        rebound_entry: &ExtensionNativeOwnershipEntry,
        eligibility: ExtensionRuntimeEligibility,
    ) -> Result<(), ExtensionRuntimeHostGrantRebindPortRefusal> {
        if matches!(self.behavior, PublicationBehavior::RefuseGrantExact) {
            return Err(ExtensionRuntimeHostGrantRebindPortRefusal::new(
                ExtensionRuntimeHostBindError::Unavailable,
                eligibility,
            ));
        }
        if matches!(self.behavior, PublicationBehavior::RefuseGrantSwapped) {
            self.retained_eligibility = Some(eligibility);
            return Err(ExtensionRuntimeHostGrantRebindPortRefusal::new(
                ExtensionRuntimeHostBindError::Unavailable,
                self.substitute_eligibility
                    .take()
                    .expect("substitute eligibility"),
            ));
        }
        let Some(authority) = self.authority.as_mut() else {
            return Err(ExtensionRuntimeHostGrantRebindPortRefusal::new(
                ExtensionRuntimeHostBindError::InternalInvariant,
                eligibility,
            ));
        };
        authority
            .try_rebind_grants(current_entry, rebound_entry, eligibility)
            .map(|_| ())
            .map_err(|refusal| {
                ExtensionRuntimeHostGrantRebindPortRefusal::new(
                    ExtensionRuntimeHostBindError::InternalInvariant,
                    refusal.into_eligibility(),
                )
            })
    }

    fn reclaim_operation_authority(
        &mut self,
        _owner: ExtensionRuntimeOwnerAddress,
        _generation: ExtensionRuntimeHostRegistryGeneration,
        _release_entry: &ExtensionNativeOwnershipEntry,
    ) -> Result<ExtensionRuntimeOperationAuthority, ExtensionRuntimeHostBindError> {
        self.probe.reclaim_calls.fetch_add(1, Ordering::Relaxed);
        if self.reclaim_failures > 0 {
            self.reclaim_failures -= 1;
            return Err(ExtensionRuntimeHostBindError::Unavailable);
        }
        if matches!(self.behavior, PublicationBehavior::SwapReclaim) {
            return Ok(self.substitute.take().expect("substitute authority"));
        }
        self.authority
            .take()
            .ok_or(ExtensionRuntimeHostBindError::InternalInvariant)
    }

    fn mint_active_tab_grant_witness(
        &mut self,
        owner: ExtensionRuntimeOwnerAddress,
        generation: ExtensionRuntimeHostRegistryGeneration,
        runtime: &ExtensionRuntimeFingerprint,
        invocation: ExtensionUserInvocationKind,
    ) -> Result<ExtensionActiveTabGrantWitness, ExtensionOperationAuthorityDenial> {
        self.probe
            .active_witness_calls
            .fetch_add(1, Ordering::Relaxed);
        *self.probe.last_witness.lock().expect("probe lock") = Some(WitnessObservation {
            owner,
            generation,
            runtime: runtime.clone(),
        });
        if matches!(self.behavior, PublicationBehavior::BadActiveWitness) {
            let substitute = self.substitute.as_ref().expect("substitute authority");
            return substitute.mint_active_tab_grant_witness(substitute.fingerprint(), invocation);
        }
        self.authority
            .as_ref()
            .expect("published authority")
            .mint_active_tab_grant_witness(runtime, invocation)
    }

    fn mint_document_authority_witness(
        &mut self,
        owner: ExtensionRuntimeOwnerAddress,
        generation: ExtensionRuntimeHostRegistryGeneration,
        runtime: &ExtensionRuntimeFingerprint,
        purpose: ExtensionDocumentPurpose,
    ) -> Result<ExtensionDocumentAuthorityWitness, ExtensionOperationAuthorityDenial> {
        self.probe
            .document_witness_calls
            .fetch_add(1, Ordering::Relaxed);
        *self.probe.last_witness.lock().expect("probe lock") = Some(WitnessObservation {
            owner,
            generation,
            runtime: runtime.clone(),
        });
        let returned_purpose = if matches!(self.behavior, PublicationBehavior::BadDocumentWitness) {
            ExtensionDocumentPurpose::InsertCss
        } else {
            purpose
        };
        self.authority
            .as_ref()
            .expect("published authority")
            .mint_document_authority_witness(runtime, returned_purpose)
    }
}

struct FakeFactoryPort {
    probe: Arc<HostProbe>,
    activation_error: Option<ExtensionRuntimeHostBindError>,
    consume_grants_before_activation_error: bool,
    recovery_error: Option<ExtensionRuntimeHostBindError>,
    lifecycle_retained_bytes: usize,
    publication_retained_bytes: usize,
    recovery_retained_bytes: usize,
    publication_behavior: PublicationBehavior,
    substitute: Option<ExtensionRuntimeOperationAuthority>,
    substitute_eligibility: Option<ExtensionRuntimeEligibility>,
    reclaim_failures: usize,
}

impl FakeFactoryPort {
    fn normal(probe: Arc<HostProbe>) -> Self {
        Self {
            probe,
            activation_error: None,
            consume_grants_before_activation_error: false,
            recovery_error: None,
            lifecycle_retained_bytes: 0,
            publication_retained_bytes: 0,
            recovery_retained_bytes: 0,
            publication_behavior: PublicationBehavior::Normal,
            substitute: None,
            substitute_eligibility: None,
            reclaim_failures: 0,
        }
    }
}

impl ExtensionRuntimeHostFactoryPort for FakeFactoryPort {
    fn bind_activation(
        &mut self,
        context: ExtensionRuntimeHostActivationContext<'_>,
    ) -> Result<ExtensionRuntimeHostActivationPorts, ExtensionRuntimeHostBindError> {
        let generation =
            ExtensionRuntimeHostRegistryGeneration::new(17).expect("nonzero host generation");
        let absence_issuer = context.absence_evidence_issuer().bind(generation);
        let grants = context.native_grants();
        *self
            .probe
            .last_activation_grants
            .lock()
            .expect("activation grant observation lock") = Some(ActivationGrantObservation {
            runtime: grants.runtime().clone(),
            api: grants
                .api_grants()
                .map(|grant| {
                    (
                        grant.name().as_str().to_owned(),
                        grant.requirement(),
                        grant.decision(),
                    )
                })
                .collect(),
            hosts: grants
                .host_grants()
                .map(|grant| {
                    (
                        grant.pattern().as_str().to_owned(),
                        grant.requirement(),
                        grant.decision(),
                    )
                })
                .collect(),
            file_scheme_access: grants.file_scheme_access_granted(),
            private_context_access: grants.private_context_access_granted(),
        });
        debug_assert_eq!(grants.runtime(), context.fingerprint());
        self.probe.activation_binds.fetch_add(1, Ordering::Relaxed);
        if self.consume_grants_before_activation_error {
            let snapshot = context.into_native_grant_snapshot();
            debug_assert_eq!(
                snapshot.runtime(),
                &self
                    .probe
                    .last_activation_grants
                    .lock()
                    .expect("activation grant observation lock")
                    .as_ref()
                    .expect("activation observation precedes snapshot retention")
                    .runtime
            );
            self.probe
                .consumed_activation_snapshots
                .fetch_add(1, Ordering::Relaxed);
            drop(snapshot);
        }
        if let Some(reason) = self.activation_error {
            return Err(reason);
        }
        let reservation = Reservation::new(Arc::clone(&self.probe));
        Ok(ExtensionRuntimeHostActivationPorts::new(
            generation,
            Box::new(FakeLifecycle {
                probe: Arc::clone(&self.probe),
                reservation: Arc::clone(&reservation),
                retained_bytes: self.lifecycle_retained_bytes,
                absence_issuer,
                last_absence: None,
                next_absence_attempt: 1,
            }),
            Box::new(FakePublication {
                probe: Arc::clone(&self.probe),
                reservation,
                retained_bytes: self.publication_retained_bytes,
                behavior: self.publication_behavior,
                authority: None,
                substitute: self.substitute.take(),
                substitute_eligibility: self.substitute_eligibility.take(),
                retained_eligibility: None,
                reclaim_failures: self.reclaim_failures,
            }),
        ))
    }

    fn bind_recovery(
        &mut self,
        context: ExtensionRuntimeHostRecoveryContext,
    ) -> Result<Box<dyn ExtensionRuntimeHostOwnershipPort>, ExtensionRuntimeHostBindError> {
        self.probe.recovery_binds.fetch_add(1, Ordering::Relaxed);
        if let Some(reason) = self.recovery_error {
            return Err(reason);
        }
        let reservation = Reservation::new(Arc::clone(&self.probe));
        let generation =
            ExtensionRuntimeHostRegistryGeneration::new(18).expect("nonzero host generation");
        Ok(Box::new(FakeOwnership {
            probe: Arc::clone(&self.probe),
            reservation,
            retained_bytes: self.recovery_retained_bytes,
            absence_issuer: context.absence_evidence_issuer().bind(generation),
            last_absence: None,
            next_absence_attempt: 1,
        }))
    }
}

#[derive(Default)]
struct ProfileAbsenceProbe {
    calls: AtomicUsize,
    observation: Mutex<Option<(ProfileId, Instant)>>,
}

struct ProfileAbsencePort {
    probe: Arc<ProfileAbsenceProbe>,
    observation: Result<(), ExtensionRuntimeHostProfileAbsenceDisposition>,
}

impl ExtensionRuntimeHostFactoryPort for ProfileAbsencePort {
    fn bind_activation(
        &mut self,
        _context: ExtensionRuntimeHostActivationContext<'_>,
    ) -> Result<ExtensionRuntimeHostActivationPorts, ExtensionRuntimeHostBindError> {
        Err(ExtensionRuntimeHostBindError::Unavailable)
    }

    fn bind_recovery(
        &mut self,
        _context: ExtensionRuntimeHostRecoveryContext,
    ) -> Result<Box<dyn ExtensionRuntimeHostOwnershipPort>, ExtensionRuntimeHostBindError> {
        Err(ExtensionRuntimeHostBindError::Unavailable)
    }

    fn profile_absence_until(
        &mut self,
        profile: ProfileId,
        deadline: Instant,
    ) -> Result<(), ExtensionRuntimeHostProfileAbsenceDisposition> {
        self.probe.calls.fetch_add(1, Ordering::Relaxed);
        *self.probe.observation.lock().expect("probe lock") = Some((profile, deadline));
        self.observation
    }
}

fn bind_activation_with(
    fixture: ActivationFixture,
    port: FakeFactoryPort,
) -> (
    ExtensionRuntimeHostActivation,
    Arc<HostProbe>,
    ExtensionRuntimeFingerprint,
    ExtensionNativeOwnershipEntry,
    ExtensionNativeOwnershipEntry,
    ExtensionRuntimeOwnershipEvidence,
) {
    let probe = Arc::clone(&port.probe);
    let runtime = fixture.runtime();
    let owned = fixture.owned();
    let release = fixture.release();
    let evidence = fixture.evidence;
    let mut factory = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(port));
    let activation = factory
        .bind_activation(fixture.into_binding())
        .expect("host activation binding");
    (activation, probe, runtime, owned, release, evidence)
}

#[test]
fn profile_absence_fence_refuses_an_expired_deadline_without_calling_the_port() {
    let probe = Arc::new(ProfileAbsenceProbe::default());
    let mut factory =
        ExtensionRuntimeHostFactory::from_trusted_port(Box::new(ProfileAbsencePort {
            probe: Arc::clone(&probe),
            observation: Ok(()),
        }));

    assert!(matches!(
        factory.profile_absence_until(ProfileId::from(7), Instant::now()),
        Err(ExtensionRuntimeHostProfileAbsenceDisposition::TimedOut)
    ));
    assert_eq!(probe.calls.load(Ordering::Relaxed), 0);
    assert!(probe.observation.lock().expect("probe lock").is_none());
}

#[test]
fn profile_absence_fence_forwards_the_exact_identity_deadline_and_closed_refusal() {
    let profile = ProfileId::from(7);
    let dispositions = [
        ExtensionRuntimeHostProfileAbsenceDisposition::ObligationsRemain,
        ExtensionRuntimeHostProfileAbsenceDisposition::TimedOut,
        ExtensionRuntimeHostProfileAbsenceDisposition::Unavailable,
        ExtensionRuntimeHostProfileAbsenceDisposition::InvariantFailed,
    ];

    for disposition in dispositions {
        let deadline = Instant::now() + std::time::Duration::from_secs(60);
        let probe = Arc::new(ProfileAbsenceProbe::default());
        let mut factory =
            ExtensionRuntimeHostFactory::from_trusted_port(Box::new(ProfileAbsencePort {
                probe: Arc::clone(&probe),
                observation: Err(disposition),
            }));

        assert!(matches!(
            factory.profile_absence_until(profile, deadline),
            Err(actual) if actual == disposition
        ));
        assert_eq!(probe.calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            *probe.observation.lock().expect("probe lock"),
            Some((profile, deadline))
        );
    }
}

#[test]
fn profile_absence_evidence_is_profile_bound_factory_bound_and_nonreplayable() {
    let profile = ProfileId::from(7);
    let other_profile = ProfileId::from(8);
    let first_probe = Arc::new(ProfileAbsenceProbe::default());
    let second_probe = Arc::new(ProfileAbsenceProbe::default());
    let mut first = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(ProfileAbsencePort {
        probe: Arc::clone(&first_probe),
        observation: Ok(()),
    }));
    let mut second = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(ProfileAbsencePort {
        probe: Arc::clone(&second_probe),
        observation: Ok(()),
    }));

    let first_evidence = first
        .profile_absence_until(profile, Instant::now() + std::time::Duration::from_secs(60))
        .expect("fresh first-factory evidence");
    let second_evidence = second
        .profile_absence_until(profile, Instant::now() + std::time::Duration::from_secs(60))
        .expect("fresh second-factory evidence");

    assert_eq!(first_evidence.profile(), profile);
    assert!(first_evidence.is_for_profile(profile));
    assert!(!first_evidence.is_for_profile(other_profile));
    assert_eq!(first_evidence.fence_generation(), 1);
    assert_eq!(second_evidence.fence_generation(), 1);
    assert!(!first_evidence.shares_factory_epoch(&second_evidence));
    drop(first_evidence);
    drop(second_evidence);

    let next = first
        .profile_absence_until(profile, Instant::now() + std::time::Duration::from_secs(60))
        .expect("later first-factory evidence");
    assert_eq!(next.fence_generation(), 2);
    assert_eq!(first_probe.calls.load(Ordering::Relaxed), 2);
    assert_eq!(second_probe.calls.load(Ordering::Relaxed), 1);
}

#[test]
fn profile_absence_fence_generation_never_wraps_or_reuses_identity() {
    let probe = Arc::new(ProfileAbsenceProbe::default());
    let mut factory =
        ExtensionRuntimeHostFactory::from_trusted_port(Box::new(ProfileAbsencePort {
            probe: Arc::clone(&probe),
            observation: Ok(()),
        }));
    factory.set_next_profile_fence_generation_for_test(u64::MAX);

    let final_evidence = factory
        .profile_absence_until(
            ProfileId::from(7),
            Instant::now() + std::time::Duration::from_secs(60),
        )
        .expect("final nonwrapping generation");
    assert_eq!(final_evidence.fence_generation(), u64::MAX);
    drop(final_evidence);

    assert!(matches!(
        factory.profile_absence_until(
            ProfileId::from(7),
            Instant::now() + std::time::Duration::from_secs(60),
        ),
        Err(ExtensionRuntimeHostProfileAbsenceDisposition::InvariantFailed)
    ));
    assert_eq!(probe.calls.load(Ordering::Relaxed), 1);
}

#[test]
fn factory_ports_without_an_absence_fence_make_no_absence_claim() {
    let probe = Arc::new(HostProbe::default());
    let mut factory =
        ExtensionRuntimeHostFactory::from_trusted_port(Box::new(FakeFactoryPort::normal(probe)));

    assert!(matches!(
        factory.profile_absence_until(
            ProfileId::from(7),
            Instant::now() + std::time::Duration::from_secs(60),
        ),
        Err(ExtensionRuntimeHostProfileAbsenceDisposition::Unavailable)
    ));
}

fn published_receipt_with(
    fixture: ActivationFixture,
    port: FakeFactoryPort,
) -> (
    ExtensionRuntimePublicationReceipt,
    Arc<HostProbe>,
    ExtensionRuntimeFingerprint,
    ExtensionNativeOwnershipEntry,
) {
    let (activation, probe, runtime, owned, release, evidence) =
        bind_activation_with(fixture, port);
    let (request, pending) = activation.into_parts();
    drop(request.cancel());
    let receipt = pending
        .authorize(owned, evidence)
        .expect("authorized publication")
        .publish()
        .expect("published authority");
    (receipt, probe, runtime, release)
}

fn assert_refused_binding_parts(
    provider_identity: Arc<()>,
    entry: ExtensionNativeOwnershipEntry,
    access: ExtensionPackageAccess,
    authority: ExtensionRuntimeOperationAuthority,
    expectation: ExtensionRuntimeNativeIdentityExpectation,
    expected: ExtensionRuntimeHostActivationBindingError,
) {
    let runtime = authority.fingerprint().clone();
    let refusal = ExtensionRuntimeHostActivationBinding::try_from_authenticated_repository(
        entry.clone(),
        access,
        authority,
        expectation,
    )
    .expect_err("binding must be refused");
    assert_eq!(refusal.reason(), expected);
    let (returned_entry, returned_access, returned_authority, returned_expectation) =
        refusal.into_parts();
    assert_eq!(returned_entry, entry);
    assert_eq!(returned_authority.fingerprint(), &runtime);
    assert_eq!(returned_expectation, expectation);
    let returned_provider = returned_access
        .try_into_delegated_provider::<PinnedProvider>()
        .expect("exact provider type returned");
    assert!(Arc::ptr_eq(&returned_provider.identity, &provider_identity));
}

#[test]
fn activation_binding_refuses_reachable_frontier_and_join_mismatches_losslessly() {
    let fixture = ActivationFixture::compatibility(1);
    let mut wrong = fixture.template.clone();
    wrong.package = package(77);
    let wrong_entry = wrong.may_own();
    let ActivationFixture {
        access,
        authority,
        expectation,
        provider_identity,
        ..
    } = fixture;
    assert_refused_binding_parts(
        provider_identity,
        wrong_entry,
        access,
        authority,
        expectation,
        ExtensionRuntimeHostActivationBindingError::RuntimeFingerprintMismatch,
    );

    let fixture = ActivationFixture::compatibility(2);
    let wrong_entry = fixture.template.entry(
        3,
        ExtensionNativeOwnershipIntent::Release,
        ExtensionNativeOwnershipPhase::NativeMayOwn,
        None,
    );
    let provider_identity = Arc::clone(&fixture.provider_identity);
    assert_refused_binding_parts(
        provider_identity,
        wrong_entry,
        fixture.access,
        fixture.authority,
        fixture.expectation,
        ExtensionRuntimeHostActivationBindingError::OwnershipIntentMismatch,
    );

    let fixture = ActivationFixture::compatibility(3);
    let wrong_entry = fixture.template.owned(None);
    assert_refused_binding_parts(
        Arc::clone(&fixture.provider_identity),
        wrong_entry,
        fixture.access,
        fixture.authority,
        fixture.expectation,
        ExtensionRuntimeHostActivationBindingError::OwnershipPhaseMismatch,
    );

    let fixture = ActivationFixture::macos(4);
    let identity = fixture.native_identity.expect("native identity");
    let wrong_entry = fixture.template.entry(
        3,
        ExtensionNativeOwnershipIntent::Acquire,
        ExtensionNativeOwnershipPhase::NativeMayOwn,
        Some(identity),
    );
    assert_refused_binding_parts(
        Arc::clone(&fixture.provider_identity),
        wrong_entry,
        fixture.access,
        fixture.authority,
        fixture.expectation,
        ExtensionRuntimeHostActivationBindingError::NativeIdentityAlreadyPresent,
    );

    let fixture = ActivationFixture::compatibility(5);
    let wrong_access_fixture = ActivationFixture::new(
        6,
        ExtensionRuntimeBackendTarget::LinuxCompatibility,
        ExtensionRuntimeTarget::NativeWebExtension,
        ExtensionRuntimeNativeIdentityExpectation::Compatibility,
        None,
        ExtensionRuntimeOwnershipEvidence::Compatibility,
        0,
    );
    assert_refused_binding_parts(
        Arc::clone(&wrong_access_fixture.provider_identity),
        fixture.initial,
        wrong_access_fixture.access,
        fixture.authority,
        fixture.expectation,
        ExtensionRuntimeHostActivationBindingError::RuntimeTargetMismatch,
    );

    let fixture = ActivationFixture::compatibility(7);
    let native_access_fixture = ActivationFixture::new(
        8,
        ExtensionRuntimeBackendTarget::LinuxCompatibility,
        ExtensionRuntimeTarget::NativeWebExtension,
        ExtensionRuntimeNativeIdentityExpectation::Compatibility,
        None,
        ExtensionRuntimeOwnershipEvidence::Compatibility,
        0,
    );
    let mac_id = ExtensionRuntimeNativeOwnerId::parse_exact(EXPECTED_NATIVE_ID).expect("native ID");
    assert_refused_binding_parts(
        Arc::clone(&native_access_fixture.provider_identity),
        fixture.initial,
        native_access_fixture.access,
        fixture.authority,
        ExtensionRuntimeNativeIdentityExpectation::MacosWebExtension(mac_id),
        ExtensionRuntimeHostActivationBindingError::NativeIdentityExpectationMismatch,
    );

    let fixture = ActivationFixture::macos(51);
    let legacy_identityless_entry = fixture.template.may_own();
    assert_refused_binding_parts(
        Arc::clone(&fixture.provider_identity),
        legacy_identityless_entry,
        fixture.access,
        fixture.authority,
        fixture.expectation,
        ExtensionRuntimeHostActivationBindingError::NativeIdentityExpectationMismatch,
    );

    let fixture = ActivationFixture::macos(52);
    let different_expected = ExtensionExpectedNativeOwnershipIdentity::parse(
        ExtensionRuntimeBackendTarget::MacosNative,
        OTHER_NATIVE_ID,
    )
    .expect("canonical different expected native ID");
    let mismatched_entry = fixture.template.entry_with_identities(
        2,
        ExtensionNativeOwnershipIntent::Acquire,
        ExtensionNativeOwnershipPhase::NativeMayOwn,
        Some(different_expected),
        None,
    );
    assert_refused_binding_parts(
        Arc::clone(&fixture.provider_identity),
        mismatched_entry,
        fixture.access,
        fixture.authority,
        fixture.expectation,
        ExtensionRuntimeHostActivationBindingError::NativeIdentityExpectationMismatch,
    );
}

#[test]
fn native_identity_expectations_project_exact_durable_core_identities() {
    let owner = ExtensionRuntimeNativeOwnerId::parse_exact(EXPECTED_NATIVE_ID)
        .expect("canonical runtime owner ID");
    for (expectation, backend) in [
        (
            ExtensionRuntimeNativeIdentityExpectation::MacosWebExtension(owner),
            ExtensionRuntimeBackendTarget::MacosNative,
        ),
        (
            ExtensionRuntimeNativeIdentityExpectation::WindowsWebView2Extension(owner),
            ExtensionRuntimeBackendTarget::WindowsNative,
        ),
    ] {
        let durable = expectation
            .durable_expected_identity()
            .expect("shared canonical grammar")
            .expect("native expectation has a durable identity");
        assert_eq!(durable.backend(), backend);
        assert_eq!(durable.bytes(), *EXPECTED_NATIVE_ID.as_bytes());
    }
    assert_eq!(
        ExtensionRuntimeNativeIdentityExpectation::Compatibility
            .durable_expected_identity()
            .expect("compatibility conversion is infallible"),
        None
    );
}

#[test]
fn activation_binding_refuses_combined_memory_excess_and_returns_exact_provider() {
    let seed = 9;
    let plan =
        ExtensionRuntimeResourcePlan::try_new(vec![ExtensionRuntimeResourceBinding::try_new(
            "manifest.json",
            2,
            [seed; 32],
        )
        .expect("manifest binding")])
        .expect("resource plan");
    let access_inline = std::mem::size_of::<ExtensionPackageAccess>();
    let plan_heap = plan
        .retained_bytes()
        .saturating_sub(std::mem::size_of::<ExtensionRuntimeResourcePlan>());
    let retained = MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
        .saturating_sub(access_inline)
        .saturating_sub(plan_heap);
    drop(plan);
    let fixture = ActivationFixture::new(
        seed,
        ExtensionRuntimeBackendTarget::LinuxCompatibility,
        ExtensionRuntimeTarget::Compatibility,
        ExtensionRuntimeNativeIdentityExpectation::Compatibility,
        None,
        ExtensionRuntimeOwnershipEvidence::Compatibility,
        retained,
    );
    let provider_identity = Arc::clone(&fixture.provider_identity);
    let refusal = ExtensionRuntimeHostActivationBinding::try_from_authenticated_repository(
        fixture.initial,
        fixture.access,
        fixture.authority,
        fixture.expectation,
    )
    .expect_err("combined binding exceeds owner budget");
    assert_eq!(
        refusal.reason(),
        ExtensionRuntimeHostActivationBindingError::RetainedBytesExceeded
    );
    let (_, access, _, _) = refusal.into_parts();
    let provider = access
        .try_into_delegated_provider::<PinnedProvider>()
        .expect("same provider is returned");
    assert!(Arc::ptr_eq(&provider.identity, &provider_identity));
}

#[test]
fn every_ordinary_host_control_state_stays_inside_the_owner_memory_ceiling() {
    let binding = ActivationFixture::compatibility(40).into_binding();
    assert!(binding.retained_bytes() <= MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES);
    drop(binding);

    let fixture = ActivationFixture::compatibility(41);
    let probe = Arc::new(HostProbe::default());
    let activation = bind_activation_with(fixture, FakeFactoryPort::normal(Arc::clone(&probe))).0;
    assert!(activation.retained_bytes() <= MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES);
    let (request, pending) = activation.into_parts();
    drop(request.cancel());
    assert!(pending.retained_bytes() <= MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES);
    let wrong_release = ActivationFixture::compatibility(42).release();
    let refusal = pending
        .recover_after_absence(&wrong_release)
        .expect_err("another owner is not a release frontier");
    assert!(refusal.retained_bytes() <= MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES);
    drop(refusal.into_pending());

    let fixture = ActivationFixture::compatibility(43);
    let probe = Arc::new(HostProbe::default());
    let (activation, _, _, _, _, evidence) =
        bind_activation_with(fixture, FakeFactoryPort::normal(Arc::clone(&probe)));
    let (request, pending) = activation.into_parts();
    drop(request.cancel());
    let wrong_state = ActivationFixture::compatibility(43).initial;
    let refusal = pending
        .authorize(wrong_state, evidence)
        .expect_err("possible owner cannot publish");
    assert!(refusal.retained_bytes() <= MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES);
    drop(refusal.into_parts());

    let fixture = ActivationFixture::compatibility(44);
    let probe = Arc::new(HostProbe::default());
    let (activation, _, _, owned, _, evidence) =
        bind_activation_with(fixture, FakeFactoryPort::normal(Arc::clone(&probe)));
    let (lifecycle, pending) = activation.into_parts();
    drop(lifecycle.cancel());
    let request = pending.authorize(owned, evidence).expect("authorization");
    assert!(request.retained_bytes() <= MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES);
    let wrong_release = ActivationFixture::compatibility(45).release();
    let refusal = request
        .recover_after_absence(&wrong_release)
        .expect_err("another owner is not a release frontier");
    assert!(refusal.retained_bytes() <= MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES);
    drop(refusal.into_request());

    let fixture = ActivationFixture::compatibility(46);
    let probe = Arc::new(HostProbe::default());
    let mut port = FakeFactoryPort::normal(Arc::clone(&probe));
    port.publication_behavior = PublicationBehavior::RefuseExact;
    let (activation, _, _, owned, _, evidence) = bind_activation_with(fixture, port);
    let (lifecycle, pending) = activation.into_parts();
    drop(lifecycle.cancel());
    let refusal = pending
        .authorize(owned, evidence)
        .expect("authorization")
        .publish()
        .expect_err("scripted ordinary refusal");
    assert!(!refusal.requires_fail_stop());
    assert!(refusal.retained_bytes() <= MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES);
    drop(refusal.try_into_request().expect("recoverable request"));

    let fixture = ActivationFixture::compatibility(47);
    let probe = Arc::new(HostProbe::default());
    let (receipt, _, _, _) =
        published_receipt_with(fixture, FakeFactoryPort::normal(Arc::clone(&probe)));
    assert!(receipt.retained_bytes() <= MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES);
    let wrong_release = ActivationFixture::compatibility(48).release();
    let refusal = receipt
        .reclaim_after_absence(&wrong_release)
        .expect_err("another owner is not a release frontier");
    assert!(!refusal.requires_fail_stop());
    assert!(refusal.retained_bytes() <= MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES);
    drop(refusal.try_into_receipt().expect("recoverable receipt"));

    let fixture = ActivationFixture::compatibility(49);
    let probe = Arc::new(HostProbe::default());
    let mut port = FakeFactoryPort::normal(Arc::clone(&probe));
    port.reclaim_failures = 1;
    let (receipt, _, _, release) = published_receipt_with(fixture, port);
    let refusal = receipt
        .reclaim_after_absence(&release)
        .expect_err("scripted ordinary reclaim refusal");
    assert!(!refusal.requires_fail_stop());
    assert!(refusal.retained_bytes() <= MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES);
    drop(refusal.try_into_receipt().expect("recoverable receipt"));
}

#[test]
fn recovery_binding_accepts_only_possible_owner_states_and_preserves_refusals() {
    let fixture = ActivationFixture::compatibility(10);
    let acquire_may_own = fixture.template.may_own();
    let acquire_owned = fixture.template.owned(None);
    let release_may_own = fixture.template.entry(
        3,
        ExtensionNativeOwnershipIntent::Release,
        ExtensionNativeOwnershipPhase::NativeMayOwn,
        None,
    );
    for entry in [acquire_may_own, acquire_owned, release_may_own] {
        let binding = ExtensionRuntimeHostRecoveryBinding::try_new(entry.clone())
            .expect("possible owner is recoverable");
        assert_eq!(binding.context().owner().cas(), entry.cas());
        assert_eq!(
            binding.context().expectation(),
            ExtensionRuntimeRecoveryExpectation::Compatibility
        );
    }

    for entry in [
        fixture.template.preparing(),
        fixture.template.release_absent(3, None),
    ] {
        let refusal = ExtensionRuntimeHostRecoveryBinding::try_new(entry.clone())
            .expect_err("definitely absent state cannot enter possible-owner recovery");
        assert_eq!(
            refusal.reason(),
            ExtensionRuntimeHostRecoveryBindingError::OwnershipStateMismatch
        );
        assert_eq!(refusal.into_entry(), entry);
    }

    let native = ActivationFixture::macos(11);
    let binding = ExtensionRuntimeHostRecoveryBinding::try_new(native.initial.clone())
        .expect("identityless native may-own recovery");
    assert_eq!(
        binding.context().expectation(),
        ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
            catalog_expected: Some(
                ExtensionRuntimeNativeOwnerId::parse_exact(EXPECTED_NATIVE_ID).expect("native ID")
            ),
            adapter_observed: None,
        }
    );
    let binding = ExtensionRuntimeHostRecoveryBinding::try_new(native.owned())
        .expect("identified native owned recovery");
    assert_eq!(
        binding.context().expectation(),
        ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
            catalog_expected: Some(
                ExtensionRuntimeNativeOwnerId::parse_exact(EXPECTED_NATIVE_ID).expect("native ID")
            ),
            adapter_observed: Some(
                ExtensionRuntimeNativeOwnerId::parse_exact(EXPECTED_NATIVE_ID).expect("native ID")
            ),
        }
    );

    let legacy_identityless = native.template.may_own();
    let binding = ExtensionRuntimeHostRecoveryBinding::try_new(legacy_identityless)
        .expect("legacy identityless native row remains cleanup-capable");
    assert_eq!(
        binding.context().expectation(),
        ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
            catalog_expected: None,
            adapter_observed: None,
        }
    );

    let observed = native.native_identity.expect("native identity");
    let legacy_observed = native.template.owned(Some(observed));
    let binding = ExtensionRuntimeHostRecoveryBinding::try_new(legacy_observed)
        .expect("legacy observed-only native row remains cleanup-capable");
    assert_eq!(
        binding.context().expectation(),
        ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
            catalog_expected: None,
            adapter_observed: Some(
                ExtensionRuntimeNativeOwnerId::parse_exact(EXPECTED_NATIVE_ID).expect("native ID")
            ),
        }
    );

    let different_observed = ExtensionNativeOwnershipIdentity::parse(
        ExtensionRuntimeBackendTarget::MacosNative,
        OTHER_NATIVE_ID,
    )
    .expect("canonical different observed ID");
    let mismatched = native.template.entry_with_identities(
        3,
        ExtensionNativeOwnershipIntent::Acquire,
        ExtensionNativeOwnershipPhase::NativeMayOwn,
        native.expected_native_identity,
        Some(different_observed),
    );
    let binding = ExtensionRuntimeHostRecoveryBinding::try_new(mismatched)
        .expect("mismatched durable claims remain absence-cleanup capable");
    assert!(binding.context().expectation().has_identity_conflict());
}

#[test]
fn recovery_issuer_cannot_mint_or_accept_activation_never_entered() {
    let fixture = ActivationFixture::compatibility(59);
    let entry = fixture.template.may_own();
    let binding = ExtensionRuntimeHostRecoveryBinding::try_new(entry.clone())
        .expect("possible compatibility owner is recoverable");
    let generation = ExtensionRuntimeHostRegistryGeneration::new(3).expect("nonzero generation");
    let attempt = std::num::NonZeroU64::new(5).expect("nonzero attempt");
    let issuer = binding.context().absence_evidence_issuer().bind(generation);

    assert_eq!(issuer.mint_activation_never_entered(attempt), None);
    let structurally_exact_but_wrong_provenance =
        ExtensionRuntimeAbsenceEvidence::for_test_host_lineage(
            entry.cas(),
            ExtensionRuntimeBackendTarget::LinuxCompatibility,
            ExtensionRuntimeTarget::Compatibility,
            generation,
            attempt,
            ExtensionRuntimeAbsenceProofKind::ActivationNeverEntered,
            None,
            None,
        );
    assert!(!issuer.accepts(structurally_exact_but_wrong_provenance, attempt));
}

#[test]
fn controller_namespace_absence_preserves_lineage_without_guessing_identity() {
    let generation = ExtensionRuntimeHostRegistryGeneration::new(4).expect("nonzero generation");
    let attempt = std::num::NonZeroU64::new(6).expect("nonzero attempt");
    let audit = ExtensionRuntimeMacosControllerAbsenceAudit::try_from_observations(
        true, true, true, true, true,
    )
    .expect("complete empty-controller audit");
    let native = ActivationFixture::macos(63);

    let catalog_only = native.initial.clone();
    let binding = ExtensionRuntimeHostRecoveryBinding::try_new(catalog_only.clone())
        .expect("catalog-only macOS row is recoverable");
    let issuer = binding.context().absence_evidence_issuer().bind(generation);
    let evidence = issuer
        .mint_macos_controller_namespace_absent(attempt, audit)
        .expect("empty exact controller proves catalog-only owner absent");
    assert_eq!(
        evidence.expected_native_identity(),
        Some(
            ExtensionRuntimeNativeOwnerId::parse_exact(EXPECTED_NATIVE_ID)
                .expect("canonical expected ID")
        )
    );
    assert_eq!(evidence.observed_native_identity(), None);
    assert!(issuer.accepts(evidence, attempt));
    assert!(evidence.structurally_matches_entry(&catalog_only));

    let observed_only = native
        .template
        .owned(Some(native.native_identity.expect("native identity")));
    let binding = ExtensionRuntimeHostRecoveryBinding::try_new(observed_only.clone())
        .expect("observed-only macOS row is recoverable");
    let issuer = binding.context().absence_evidence_issuer().bind(generation);
    let evidence = issuer
        .mint_macos_controller_namespace_absent(attempt, audit)
        .expect("empty exact controller proves observed-only owner absent");
    assert_eq!(evidence.expected_native_identity(), None);
    assert_eq!(
        evidence.observed_native_identity(),
        Some(
            ExtensionRuntimeNativeOwnerId::parse_exact(EXPECTED_NATIVE_ID)
                .expect("canonical observed ID")
        )
    );
    assert!(issuer.accepts(evidence, attempt));
    assert!(evidence.structurally_matches_entry(&observed_only));

    let identityless = native.template.may_own();
    let binding = ExtensionRuntimeHostRecoveryBinding::try_new(identityless.clone())
        .expect("identityless legacy macOS row is recoverable");
    let issuer = binding.context().absence_evidence_issuer().bind(generation);
    let evidence = issuer
        .mint_macos_controller_namespace_absent(attempt, audit)
        .expect("profile-wide empty controller needs no guessed identity");
    assert_eq!(evidence.expected_native_identity(), None);
    assert_eq!(evidence.observed_native_identity(), None);
    assert!(issuer.accepts(evidence, attempt));
    assert!(evidence.structurally_matches_entry(&identityless));
    assert_eq!(
        issuer.mint_macos_zero_grants_and_unloaded(
            attempt,
            ExtensionRuntimeNativeOwnerId::parse_exact(EXPECTED_NATIVE_ID)
                .expect("canonical native ID"),
            ExtensionRuntimeMacosAbsenceAudit::try_from_observations(
                true, true, true, true, false, false, true, true, true, true,
            )
            .expect("complete context audit"),
        ),
        None,
        "context-scoped proof must remain unavailable without an identity anchor"
    );

    let conflicting_observed = ExtensionNativeOwnershipIdentity::parse(
        ExtensionRuntimeBackendTarget::MacosNative,
        OTHER_NATIVE_ID,
    )
    .expect("canonical conflicting observed ID");
    let conflicting = native.template.entry_with_identities(
        3,
        ExtensionNativeOwnershipIntent::Acquire,
        ExtensionNativeOwnershipPhase::NativeMayOwn,
        native.expected_native_identity,
        Some(conflicting_observed),
    );
    let binding = ExtensionRuntimeHostRecoveryBinding::try_new(conflicting)
        .expect("conflicting durable row remains cleanup-capable");
    let issuer = binding.context().absence_evidence_issuer().bind(generation);
    assert_eq!(
        issuer.mint_macos_controller_namespace_absent(attempt, audit),
        None,
        "empty namespace cannot silently reconcile conflicting identity claims"
    );

    let compatibility = ActivationFixture::compatibility(64);
    let binding = ExtensionRuntimeHostRecoveryBinding::try_new(compatibility.initial)
        .expect("compatibility row is recoverable");
    let issuer = binding.context().absence_evidence_issuer().bind(generation);
    assert_eq!(
        issuer.mint_macos_controller_namespace_absent(attempt, audit),
        None,
        "a macOS native proof cannot cross backend families"
    );
}

#[test]
fn absence_evidence_structural_matcher_checks_every_representable_lineage_dimension() {
    #[allow(clippy::too_many_arguments)]
    fn try_rebuild(
        source: &ExtensionNativeOwnershipEntry,
        key: ExtensionNativeOwnershipKey,
        operation: ExtensionNativeOwnershipOperation,
        revision: u64,
        backend: ExtensionRuntimeBackendTarget,
        expected: Option<ExtensionExpectedNativeOwnershipIdentity>,
        observed: Option<ExtensionNativeOwnershipIdentity>,
        incarnation: ExtensionNativeIncarnation,
        intent: ExtensionNativeOwnershipIntent,
        phase: ExtensionNativeOwnershipPhase,
    ) -> Result<ExtensionNativeOwnershipEntry, ExtensionNativeOwnershipJournalError> {
        ExtensionNativeOwnershipEntry::from_persisted_with_native_identities(
            key,
            operation,
            ExtensionNativeOwnershipEntryRevision::new(revision).expect("nonzero revision"),
            source.package().clone(),
            source.catalog_set_digest(),
            source.catalog_role(),
            source.store_catalog_revision(),
            source.store_install_revision(),
            source.store_grant_revision(),
            source.grant_digest(),
            backend,
            expected,
            observed,
            incarnation,
            intent,
            phase,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn rebuild(
        source: &ExtensionNativeOwnershipEntry,
        key: ExtensionNativeOwnershipKey,
        operation: ExtensionNativeOwnershipOperation,
        revision: u64,
        backend: ExtensionRuntimeBackendTarget,
        expected: Option<ExtensionExpectedNativeOwnershipIdentity>,
        observed: Option<ExtensionNativeOwnershipIdentity>,
        incarnation: ExtensionNativeIncarnation,
        intent: ExtensionNativeOwnershipIntent,
        phase: ExtensionNativeOwnershipPhase,
    ) -> ExtensionNativeOwnershipEntry {
        try_rebuild(
            source,
            key,
            operation,
            revision,
            backend,
            expected,
            observed,
            incarnation,
            intent,
            phase,
        )
        .expect("valid structural matcher fixture row")
    }

    fn evidence(
        entry: &ExtensionNativeOwnershipEntry,
        backend: ExtensionRuntimeBackendTarget,
        target: ExtensionRuntimeTarget,
        proof: ExtensionRuntimeAbsenceProofKind,
        expected: Option<ExtensionRuntimeNativeOwnerId>,
        observed: Option<ExtensionRuntimeNativeOwnerId>,
    ) -> ExtensionRuntimeAbsenceEvidence {
        ExtensionRuntimeAbsenceEvidence::for_test_host_lineage(
            entry.cas(),
            backend,
            target,
            ExtensionRuntimeHostRegistryGeneration::new(7).expect("nonzero generation"),
            std::num::NonZeroU64::new(9).expect("nonzero attempt"),
            proof,
            expected,
            observed,
        )
    }

    let native = ActivationFixture::macos(60);
    let initial = native.initial.clone();
    let release = native.release();
    let expected_id = ExtensionRuntimeNativeOwnerId::parse_exact(EXPECTED_NATIVE_ID)
        .expect("canonical expected ID");
    let other_id =
        ExtensionRuntimeNativeOwnerId::parse_exact(OTHER_NATIVE_ID).expect("canonical other ID");
    let exact = evidence(
        &initial,
        ExtensionRuntimeBackendTarget::MacosNative,
        ExtensionRuntimeTarget::NativeWebExtension,
        ExtensionRuntimeAbsenceProofKind::MacosZeroGrantsAndUnloaded,
        Some(expected_id),
        Some(expected_id),
    );
    assert!(exact.structurally_matches_entry(&initial));
    assert!(
        exact.structurally_matches_entry(&release),
        "an older proof may match the same owner after a monotonic row revision"
    );

    let newer_evidence = evidence(
        &release,
        ExtensionRuntimeBackendTarget::MacosNative,
        ExtensionRuntimeTarget::NativeWebExtension,
        ExtensionRuntimeAbsenceProofKind::MacosZeroGrantsAndUnloaded,
        Some(expected_id),
        Some(expected_id),
    );
    assert!(!newer_evidence.structurally_matches_entry(&initial));
    let different_key = ActivationFixture::macos(61).initial;
    assert_ne!(different_key.key(), initial.key());
    assert_eq!(different_key.operation(), initial.operation());
    assert_eq!(different_key.revision(), initial.revision());
    assert_eq!(
        different_key.native_incarnation(),
        initial.native_incarnation()
    );
    assert_eq!(different_key.runtime_backend(), initial.runtime_backend());
    assert_eq!(
        different_key.expected_native_identity(),
        initial.expected_native_identity()
    );
    assert_eq!(different_key.native_identity(), initial.native_identity());
    assert!(!exact.structurally_matches_entry(&different_key));

    let second_operation = ExtensionNativeOwnershipOperation::new(2).expect("operation");
    let second_incarnation = ExtensionNativeIncarnation::new(2).expect("incarnation");
    let different_operation = rebuild(
        &initial,
        initial.key(),
        second_operation,
        2,
        ExtensionRuntimeBackendTarget::MacosNative,
        native.expected_native_identity,
        None,
        second_incarnation,
        ExtensionNativeOwnershipIntent::Acquire,
        ExtensionNativeOwnershipPhase::NativeMayOwn,
    );
    assert!(!exact.structurally_matches_entry(&different_operation));
    assert!(matches!(
        try_rebuild(
            &initial,
            initial.key(),
            second_operation,
            2,
            ExtensionRuntimeBackendTarget::MacosNative,
            native.expected_native_identity,
            None,
            initial.native_incarnation(),
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeMayOwn,
        ),
        Err(ExtensionNativeOwnershipJournalError::OperationIncarnationMismatch)
    ));
    assert!(matches!(
        try_rebuild(
            &initial,
            initial.key(),
            initial.operation(),
            2,
            ExtensionRuntimeBackendTarget::MacosNative,
            native.expected_native_identity,
            None,
            second_incarnation,
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeMayOwn,
        ),
        Err(ExtensionNativeOwnershipJournalError::OperationIncarnationMismatch)
    ));

    let windows_expected = ExtensionExpectedNativeOwnershipIdentity::from_encoded_bytes(
        ExtensionRuntimeBackendTarget::WindowsNative,
        expected_id.encoded_bytes(),
    )
    .expect("canonical Windows expected ID");
    let different_backend = rebuild(
        &initial,
        initial.key(),
        initial.operation(),
        2,
        ExtensionRuntimeBackendTarget::WindowsNative,
        Some(windows_expected),
        None,
        initial.native_incarnation(),
        ExtensionNativeOwnershipIntent::Acquire,
        ExtensionNativeOwnershipPhase::NativeMayOwn,
    );
    assert!(!exact.structurally_matches_entry(&different_backend));

    let wrong_target = evidence(
        &initial,
        ExtensionRuntimeBackendTarget::MacosNative,
        ExtensionRuntimeTarget::Compatibility,
        ExtensionRuntimeAbsenceProofKind::ActivationNeverEntered,
        Some(expected_id),
        None,
    );
    assert!(!wrong_target.structurally_matches_entry(&initial));

    let other_expected = ExtensionExpectedNativeOwnershipIdentity::from_encoded_bytes(
        ExtensionRuntimeBackendTarget::MacosNative,
        other_id.encoded_bytes(),
    )
    .expect("canonical alternate expected ID");
    let expected_mismatch = rebuild(
        &initial,
        initial.key(),
        initial.operation(),
        2,
        ExtensionRuntimeBackendTarget::MacosNative,
        Some(other_expected),
        None,
        initial.native_incarnation(),
        ExtensionNativeOwnershipIntent::Acquire,
        ExtensionNativeOwnershipPhase::NativeMayOwn,
    );
    assert!(!exact.structurally_matches_entry(&expected_mismatch));

    let other_observed = ExtensionNativeOwnershipIdentity::from_encoded_bytes(
        ExtensionRuntimeBackendTarget::MacosNative,
        other_id.encoded_bytes(),
    )
    .expect("canonical alternate observed ID");
    let observed_mismatch = rebuild(
        &initial,
        initial.key(),
        initial.operation(),
        3,
        ExtensionRuntimeBackendTarget::MacosNative,
        native.expected_native_identity,
        Some(other_observed),
        initial.native_incarnation(),
        ExtensionNativeOwnershipIntent::Acquire,
        ExtensionNativeOwnershipPhase::NativeMayOwn,
    );
    assert!(!exact.structurally_matches_entry(&observed_mismatch));

    let activation_unentered = evidence(
        &initial,
        ExtensionRuntimeBackendTarget::MacosNative,
        ExtensionRuntimeTarget::NativeWebExtension,
        ExtensionRuntimeAbsenceProofKind::ActivationNeverEntered,
        Some(expected_id),
        None,
    );
    assert!(activation_unentered.structurally_matches_entry(&initial));
    let wrong_native_proof = evidence(
        &initial,
        ExtensionRuntimeBackendTarget::MacosNative,
        ExtensionRuntimeTarget::NativeWebExtension,
        ExtensionRuntimeAbsenceProofKind::CompatibilityRegistryAbsentAndQuiescent,
        None,
        None,
    );
    assert!(!wrong_native_proof.structurally_matches_entry(&initial));

    let legacy_observed = rebuild(
        &initial,
        initial.key(),
        initial.operation(),
        3,
        ExtensionRuntimeBackendTarget::MacosNative,
        None,
        native.native_identity,
        initial.native_incarnation(),
        ExtensionNativeOwnershipIntent::Acquire,
        ExtensionNativeOwnershipPhase::NativeMayOwn,
    );
    let legacy_absence = evidence(
        &legacy_observed,
        ExtensionRuntimeBackendTarget::MacosNative,
        ExtensionRuntimeTarget::NativeWebExtension,
        ExtensionRuntimeAbsenceProofKind::MacosZeroGrantsAndUnloaded,
        None,
        Some(expected_id),
    );
    assert!(legacy_absence.structurally_matches_entry(&legacy_observed));
    let identityless_absence = evidence(
        &initial,
        ExtensionRuntimeBackendTarget::MacosNative,
        ExtensionRuntimeTarget::NativeWebExtension,
        ExtensionRuntimeAbsenceProofKind::MacosZeroGrantsAndUnloaded,
        None,
        None,
    );
    assert!(!identityless_absence.structurally_matches_entry(&initial));

    let compatibility = ActivationFixture::compatibility(62);
    let compatibility_absence = evidence(
        &compatibility.initial,
        ExtensionRuntimeBackendTarget::LinuxCompatibility,
        ExtensionRuntimeTarget::Compatibility,
        ExtensionRuntimeAbsenceProofKind::CompatibilityRegistryAbsentAndQuiescent,
        None,
        None,
    );
    assert!(compatibility_absence.structurally_matches_entry(&compatibility.initial));
    let compatibility_as_macos = evidence(
        &compatibility.initial,
        ExtensionRuntimeBackendTarget::LinuxCompatibility,
        ExtensionRuntimeTarget::Compatibility,
        ExtensionRuntimeAbsenceProofKind::MacosZeroGrantsAndUnloaded,
        None,
        None,
    );
    assert!(!compatibility_as_macos.structurally_matches_entry(&compatibility.initial));
}

#[test]
fn factory_refusals_are_lossless_and_do_not_reserve_or_call_native_code() {
    let fixture = ActivationFixture::compatibility(12);
    let runtime = fixture.runtime();
    let entry = fixture.initial.clone();
    let expectation = fixture.expectation;
    let provider_identity = Arc::clone(&fixture.provider_identity);
    let probe = Arc::new(HostProbe::default());
    let mut port = FakeFactoryPort::normal(Arc::clone(&probe));
    port.activation_error = Some(ExtensionRuntimeHostBindError::UnsupportedBackend);
    port.consume_grants_before_activation_error = true;
    let mut factory = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(port));
    let refusal = factory
        .bind_activation(fixture.into_binding())
        .expect_err("unsupported engine refuses activation");
    assert_eq!(
        refusal.reason(),
        ExtensionRuntimeHostBindError::UnsupportedBackend
    );
    let (returned_entry, returned_access, returned_authority, returned_expectation) =
        refusal.cancel_into_parts();
    assert_eq!(returned_entry, entry);
    assert_eq!(returned_authority.fingerprint(), &runtime);
    assert!(returned_authority.matches_native_ownership_lineage(&entry));
    assert_eq!(returned_expectation, expectation);
    let returned_provider = returned_access
        .try_into_delegated_provider::<PinnedProvider>()
        .expect("exact provider returned from bind cancellation");
    assert!(Arc::ptr_eq(&returned_provider.identity, &provider_identity));
    assert_eq!(probe.activation_binds.load(Ordering::Relaxed), 1);
    assert_eq!(
        probe.consumed_activation_snapshots.load(Ordering::Relaxed),
        1
    );
    assert_eq!(probe.reservations.load(Ordering::Relaxed), 0);
    assert_eq!(probe.lifecycle_calls.load(Ordering::Relaxed), 0);
    assert_eq!(probe.publication_calls.load(Ordering::Relaxed), 0);

    let recovery_entry = ActivationFixture::compatibility(13).initial;
    let recovery = ExtensionRuntimeHostRecoveryBinding::try_new(recovery_entry.clone())
        .expect("recovery binding");
    let recovery_owner = recovery.context().owner();
    let probe = Arc::new(HostProbe::default());
    let mut port = FakeFactoryPort::normal(Arc::clone(&probe));
    port.recovery_error = Some(ExtensionRuntimeHostBindError::CapacityExceeded);
    let mut factory = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(port));
    let refusal = factory
        .bind_recovery(recovery)
        .expect_err("capacity refuses recovery");
    assert_eq!(
        refusal.reason(),
        ExtensionRuntimeHostBindError::CapacityExceeded
    );
    assert_eq!(refusal.into_binding().context().owner(), recovery_owner);
    assert_eq!(probe.recovery_binds.load(Ordering::Relaxed), 1);
    assert_eq!(probe.reservations.load(Ordering::Relaxed), 0);
    assert_eq!(probe.lifecycle_calls.load(Ordering::Relaxed), 0);
}

#[test]
fn activation_context_transports_exact_grants_without_native_or_lifecycle_work() {
    let fixture = ActivationFixture::compatibility(54);
    let runtime = fixture.runtime();
    let probe = Arc::new(HostProbe::default());
    let mut factory = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(
        FakeFactoryPort::normal(Arc::clone(&probe)),
    ));

    let activation = factory
        .bind_activation(fixture.into_binding())
        .expect("trusted host accepts exact activation context");
    let observation = probe
        .last_activation_grants
        .lock()
        .expect("activation grant observation lock")
        .clone()
        .expect("host observed native grants");

    assert_eq!(observation.runtime, runtime);
    assert_eq!(
        observation.api,
        vec![
            (
                "activeTab".to_owned(),
                ExtensionNativeGrantRequirement::Optional,
                ExtensionNativeGrantDecision::Granted,
            ),
            (
                "scripting".to_owned(),
                ExtensionNativeGrantRequirement::Optional,
                ExtensionNativeGrantDecision::Granted,
            ),
            (
                "tabs".to_owned(),
                ExtensionNativeGrantRequirement::Optional,
                ExtensionNativeGrantDecision::Denied,
            ),
        ]
    );
    assert_eq!(
        observation.hosts,
        vec![(
            ALL_URLS.to_owned(),
            ExtensionNativeGrantRequirement::Optional,
            ExtensionNativeGrantDecision::Granted,
        )]
    );
    assert!(!observation.file_scheme_access);
    assert!(!observation.private_context_access);
    assert_eq!(probe.lifecycle_calls.load(Ordering::Relaxed), 0);
    assert_eq!(probe.publication_calls.load(Ordering::Relaxed), 0);

    drop(activation.cancel_before_attempt());
    assert_eq!(probe.preattachment_restores.load(Ordering::Relaxed), 1);
}

#[test]
fn oversized_host_proxies_restore_provisional_slots_without_lifecycle_calls() {
    let fixture = ActivationFixture::compatibility(14);
    let runtime = fixture.runtime();
    let entry = fixture.initial.clone();
    let expectation = fixture.expectation;
    let provider_identity = Arc::clone(&fixture.provider_identity);
    let probe = Arc::new(HostProbe::default());
    let mut port = FakeFactoryPort::normal(Arc::clone(&probe));
    port.lifecycle_retained_bytes = usize::MAX;
    let mut factory = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(port));
    let refusal = factory
        .bind_activation(fixture.into_binding())
        .expect_err("unbounded activation proxy refused");
    assert_eq!(
        refusal.reason(),
        ExtensionRuntimeHostBindError::RetainedBytesOverflow
    );
    let (returned_entry, returned_access, returned_authority, returned_expectation) =
        refusal.cancel_into_parts();
    assert_eq!(returned_entry, entry);
    assert_eq!(returned_authority.fingerprint(), &runtime);
    assert!(returned_authority.matches_native_ownership_lineage(&entry));
    assert_eq!(returned_expectation, expectation);
    let returned_provider = returned_access
        .try_into_delegated_provider::<PinnedProvider>()
        .expect("exact provider returned from budget refusal");
    assert!(Arc::ptr_eq(&returned_provider.identity, &provider_identity));
    assert_eq!(probe.reservations.load(Ordering::Relaxed), 1);
    assert_eq!(probe.preattachment_restores.load(Ordering::Relaxed), 1);
    assert_eq!(probe.lifecycle_calls.load(Ordering::Relaxed), 0);
    assert_eq!(probe.publication_calls.load(Ordering::Relaxed), 0);

    let recovery =
        ExtensionRuntimeHostRecoveryBinding::try_new(ActivationFixture::compatibility(15).initial)
            .expect("recovery binding");
    let probe = Arc::new(HostProbe::default());
    let mut port = FakeFactoryPort::normal(Arc::clone(&probe));
    port.recovery_retained_bytes = usize::MAX;
    let mut factory = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(port));
    let refusal = factory
        .bind_recovery(recovery)
        .expect_err("unbounded recovery proxy refused");
    assert_eq!(
        refusal.reason(),
        ExtensionRuntimeHostBindError::RetainedBytesOverflow
    );
    assert_eq!(probe.reservations.load(Ordering::Relaxed), 1);
    assert_eq!(probe.preattachment_restores.load(Ordering::Relaxed), 1);
    assert_eq!(probe.lifecycle_calls.load(Ordering::Relaxed), 0);
}

#[test]
fn companion_state_is_charged_at_the_exact_factory_boundary_losslessly() {
    let baseline = ActivationFixture::compatibility(50);
    let baseline_binding = baseline.into_binding();
    let transient_without_companion = baseline_binding
        .retained_bytes()
        .checked_add(std::mem::size_of::<ExtensionRuntimeHostActivation>())
        .unwrap();
    let baseline_probe = Arc::new(HostProbe::default());
    let mut baseline_factory = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(
        FakeFactoryPort::normal(Arc::clone(&baseline_probe)),
    ));
    let baseline_activation = baseline_factory
        .bind_activation_with_companion_retained_bytes(baseline_binding, 0)
        .expect("zero-companion baseline");
    let maximum_without_companion =
        transient_without_companion.max(baseline_activation.maximum_future_retained_bytes());
    let exact_companion = MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
        .checked_sub(maximum_without_companion)
        .expect("baseline fits below the owner ceiling");
    assert_ne!(exact_companion, 0);
    drop(baseline_activation.cancel_before_attempt());
    assert_eq!(baseline_probe.lifecycle_calls.load(Ordering::Relaxed), 0);
    assert_eq!(baseline_probe.publication_calls.load(Ordering::Relaxed), 0);

    let exact = ActivationFixture::compatibility(51);
    let exact_binding = exact.into_binding();
    assert_eq!(
        exact_binding.retained_bytes(),
        transient_without_companion
            .checked_sub(std::mem::size_of::<ExtensionRuntimeHostActivation>())
            .unwrap()
    );
    let exact_probe = Arc::new(HostProbe::default());
    let mut exact_factory = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(
        FakeFactoryPort::normal(Arc::clone(&exact_probe)),
    ));
    let exact_activation = exact_factory
        .bind_activation_with_companion_retained_bytes(exact_binding, exact_companion)
        .expect("the exact complete owner ceiling must be accepted");
    assert_eq!(
        transient_without_companion
            .checked_add(exact_companion)
            .unwrap()
            .max(
                exact_activation
                    .maximum_future_retained_bytes()
                    .checked_add(exact_companion)
                    .unwrap()
            ),
        MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
    );
    drop(exact_activation.cancel_before_attempt());
    assert_eq!(exact_probe.activation_binds.load(Ordering::Relaxed), 1);
    assert_eq!(exact_probe.reservations.load(Ordering::Relaxed), 1);
    assert_eq!(
        exact_probe.preattachment_restores.load(Ordering::Relaxed),
        1
    );
    assert_eq!(exact_probe.lifecycle_calls.load(Ordering::Relaxed), 0);
    assert_eq!(exact_probe.publication_calls.load(Ordering::Relaxed), 0);

    let exceeded = ActivationFixture::compatibility(52);
    let exceeded_entry = exceeded.initial.clone();
    let exceeded_runtime = exceeded.runtime();
    let exceeded_expectation = exceeded.expectation;
    let exceeded_provider_identity = Arc::clone(&exceeded.provider_identity);
    let exceeded_probe = Arc::new(HostProbe::default());
    let mut exceeded_factory = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(
        FakeFactoryPort::normal(Arc::clone(&exceeded_probe)),
    ));
    let refusal = exceeded_factory
        .bind_activation_with_companion_retained_bytes(
            exceeded.into_binding(),
            exact_companion.checked_add(1).unwrap(),
        )
        .expect_err("one byte above the complete owner ceiling must be refused");
    assert_eq!(
        refusal.reason(),
        ExtensionRuntimeHostBindError::RetainedBytesExceeded
    );
    let (entry, access, authority, expectation) = refusal.cancel_into_parts();
    assert_eq!(entry, exceeded_entry);
    assert_eq!(authority.fingerprint(), &exceeded_runtime);
    assert!(authority.matches_native_ownership_lineage(&entry));
    assert_eq!(expectation, exceeded_expectation);
    let provider = access
        .try_into_delegated_provider::<PinnedProvider>()
        .expect("the exact provider returns after a companion excess");
    assert!(Arc::ptr_eq(&provider.identity, &exceeded_provider_identity));
    drop(provider);
    assert_eq!(exceeded_probe.activation_binds.load(Ordering::Relaxed), 0);
    assert_eq!(exceeded_probe.reservations.load(Ordering::Relaxed), 0);
    assert_eq!(
        exceeded_probe
            .preattachment_restores
            .load(Ordering::Relaxed),
        0
    );
    assert_eq!(exceeded_probe.lifecycle_calls.load(Ordering::Relaxed), 0);
    assert_eq!(exceeded_probe.publication_calls.load(Ordering::Relaxed), 0);
    assert_eq!(exceeded_probe.reclaim_calls.load(Ordering::Relaxed), 0);

    let overflow = ActivationFixture::compatibility(53);
    let overflow_entry = overflow.initial.clone();
    let overflow_runtime = overflow.runtime();
    let overflow_expectation = overflow.expectation;
    let overflow_provider_identity = Arc::clone(&overflow.provider_identity);
    let overflow_probe = Arc::new(HostProbe::default());
    let mut overflow_factory = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(
        FakeFactoryPort::normal(Arc::clone(&overflow_probe)),
    ));
    let refusal = overflow_factory
        .bind_activation_with_companion_retained_bytes(overflow.into_binding(), usize::MAX)
        .expect_err("overflowing companion accounting must be refused");
    assert_eq!(
        refusal.reason(),
        ExtensionRuntimeHostBindError::RetainedBytesOverflow
    );
    let (entry, access, authority, expectation) = refusal.cancel_into_parts();
    assert_eq!(entry, overflow_entry);
    assert_eq!(authority.fingerprint(), &overflow_runtime);
    assert!(authority.matches_native_ownership_lineage(&entry));
    assert_eq!(expectation, overflow_expectation);
    let provider = access
        .try_into_delegated_provider::<PinnedProvider>()
        .expect("the exact provider returns after companion overflow");
    assert!(Arc::ptr_eq(&provider.identity, &overflow_provider_identity));
    drop(provider);
    assert_eq!(overflow_probe.activation_binds.load(Ordering::Relaxed), 0);
    assert_eq!(overflow_probe.reservations.load(Ordering::Relaxed), 0);
    assert_eq!(
        overflow_probe
            .preattachment_restores
            .load(Ordering::Relaxed),
        0
    );
    assert_eq!(overflow_probe.lifecycle_calls.load(Ordering::Relaxed), 0);
    assert_eq!(overflow_probe.publication_calls.load(Ordering::Relaxed), 0);
    assert_eq!(overflow_probe.reclaim_calls.load(Ordering::Relaxed), 0);
}

#[test]
fn bind_only_transient_state_is_admitted_without_reducing_future_capacity() {
    let baseline = ActivationFixture::compatibility(54);
    let baseline_binding = baseline.into_binding();
    let prebind_without_transient = baseline_binding
        .retained_bytes()
        .checked_add(std::mem::size_of::<ExtensionRuntimeHostActivation>())
        .unwrap();
    let exact_bind_transient = MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
        .checked_sub(prebind_without_transient)
        .expect("the baseline binding fits below the owner ceiling");
    assert_ne!(exact_bind_transient, 0);

    let exact = ActivationFixture::compatibility(55);
    let exact_probe = Arc::new(HostProbe::default());
    let mut exact_factory = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(
        FakeFactoryPort::normal(Arc::clone(&exact_probe)),
    ));
    let activation = exact_factory
        .bind_activation_with_retained_byte_charges(exact.into_binding(), 0, exact_bind_transient)
        .expect("the exact bind-only transient ceiling must be accepted");
    assert_eq!(
        prebind_without_transient
            .checked_add(exact_bind_transient)
            .unwrap(),
        MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
    );
    assert!(
        activation.maximum_future_retained_bytes() < MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES,
        "bind-only state must not consume future host capacity"
    );
    drop(activation.cancel_before_attempt());
    assert_eq!(exact_probe.activation_binds.load(Ordering::Relaxed), 1);
    assert_eq!(
        exact_probe.preattachment_restores.load(Ordering::Relaxed),
        1
    );

    let exceeded = ActivationFixture::compatibility(56);
    let exceeded_entry = exceeded.initial.clone();
    let exceeded_runtime = exceeded.runtime();
    let exceeded_probe = Arc::new(HostProbe::default());
    let mut exceeded_factory = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(
        FakeFactoryPort::normal(Arc::clone(&exceeded_probe)),
    ));
    let refusal = exceeded_factory
        .bind_activation_with_retained_byte_charges(
            exceeded.into_binding(),
            0,
            exact_bind_transient.checked_add(1).unwrap(),
        )
        .expect_err("one bind-only byte above the ceiling must be refused");
    assert_eq!(
        refusal.reason(),
        ExtensionRuntimeHostBindError::RetainedBytesExceeded
    );
    let (entry, access, authority, _) = refusal.cancel_into_parts();
    assert_eq!(entry, exceeded_entry);
    assert_eq!(authority.fingerprint(), &exceeded_runtime);
    drop(access);
    drop(authority);
    assert_eq!(exceeded_probe.activation_binds.load(Ordering::Relaxed), 0);
    assert_eq!(exceeded_probe.reservations.load(Ordering::Relaxed), 0);

    let overflow = ActivationFixture::compatibility(57);
    let overflow_entry = overflow.initial.clone();
    let overflow_runtime = overflow.runtime();
    let overflow_probe = Arc::new(HostProbe::default());
    let mut overflow_factory = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(
        FakeFactoryPort::normal(Arc::clone(&overflow_probe)),
    ));
    let refusal = overflow_factory
        .bind_activation_with_retained_byte_charges(overflow.into_binding(), 1, usize::MAX)
        .expect_err("combined stable and bind-only arithmetic must not wrap");
    assert_eq!(
        refusal.reason(),
        ExtensionRuntimeHostBindError::RetainedBytesOverflow
    );
    let (entry, access, authority, _) = refusal.cancel_into_parts();
    assert_eq!(entry, overflow_entry);
    assert_eq!(authority.fingerprint(), &overflow_runtime);
    drop(access);
    drop(authority);
    assert_eq!(overflow_probe.activation_binds.load(Ordering::Relaxed), 0);
    assert_eq!(overflow_probe.reservations.load(Ordering::Relaxed), 0);
}

#[test]
fn dropping_unattached_host_values_is_passive_and_restores_only_local_admission() {
    let fixture = ActivationFixture::compatibility(16);
    let provider_dropped = Arc::clone(&fixture.provider_dropped);
    let probe = Arc::new(HostProbe::default());
    let port = FakeFactoryPort::normal(Arc::clone(&probe));
    let mut factory = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(port));
    let activation = factory
        .bind_activation(fixture.into_binding())
        .expect("host activation");
    drop(activation);
    assert_eq!(probe.preattachment_restores.load(Ordering::Relaxed), 1);
    assert_eq!(probe.lifecycle_calls.load(Ordering::Relaxed), 0);
    assert_eq!(probe.publication_calls.load(Ordering::Relaxed), 0);
    assert_eq!(probe.reclaim_calls.load(Ordering::Relaxed), 0);
    assert_eq!(probe.publication_drops.load(Ordering::Relaxed), 1);
    assert_eq!(provider_dropped.load(Ordering::Relaxed), 1);

    let recovery =
        ExtensionRuntimeHostRecoveryBinding::try_new(ActivationFixture::compatibility(17).initial)
            .expect("recovery binding");
    let probe = Arc::new(HostProbe::default());
    let port = FakeFactoryPort::normal(Arc::clone(&probe));
    let mut factory = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(port));
    let request = factory.bind_recovery(recovery).expect("recovery request");
    drop(request);
    assert_eq!(probe.preattachment_restores.load(Ordering::Relaxed), 1);
    assert_eq!(probe.lifecycle_calls.load(Ordering::Relaxed), 0);
}

#[test]
fn pre_attempt_cancellation_restores_exact_binding_without_native_or_publication_calls() {
    let fixture = ActivationFixture::compatibility(44);
    let runtime = fixture.runtime();
    let entry = fixture.initial.clone();
    let expectation = fixture.expectation;
    let provider_identity = Arc::clone(&fixture.provider_identity);
    let provider_dropped = Arc::clone(&fixture.provider_dropped);
    let probe = Arc::new(HostProbe::default());
    let mut port = FakeFactoryPort::normal(Arc::clone(&probe));
    port.lifecycle_retained_bytes = 37;
    port.publication_retained_bytes = 41;
    let mut factory = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(port));
    let activation = factory
        .bind_activation(fixture.into_binding())
        .expect("host activation");

    assert!(activation.maximum_future_retained_bytes() >= activation.retained_bytes());
    assert!(
        activation.maximum_future_retained_bytes() <= MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
    );
    let (returned_entry, returned_access, returned_authority, returned_expectation) =
        activation.cancel_before_attempt();

    assert_eq!(returned_entry, entry);
    assert_eq!(returned_authority.fingerprint(), &runtime);
    assert!(returned_authority.matches_native_ownership_lineage(&entry));
    assert_eq!(returned_expectation, expectation);
    let returned_provider = returned_access
        .try_into_delegated_provider::<PinnedProvider>()
        .expect("exact provider returned from pre-attempt cancellation");
    assert!(Arc::ptr_eq(&returned_provider.identity, &provider_identity));
    assert_eq!(probe.activation_binds.load(Ordering::Relaxed), 1);
    assert_eq!(probe.reservations.load(Ordering::Relaxed), 1);
    assert_eq!(probe.preattachment_restores.load(Ordering::Relaxed), 1);
    assert_eq!(probe.lifecycle_calls.load(Ordering::Relaxed), 0);
    assert_eq!(probe.publication_calls.load(Ordering::Relaxed), 0);
    assert_eq!(probe.reclaim_calls.load(Ordering::Relaxed), 0);
    assert_eq!(probe.publication_drops.load(Ordering::Relaxed), 1);
    assert_eq!(provider_dropped.load(Ordering::Relaxed), 0);
    drop(returned_provider);
    assert_eq!(provider_dropped.load(Ordering::Relaxed), 1);
}

#[test]
fn publication_authorization_checks_state_lineage_and_native_identity_losslessly() {
    let fixture = ActivationFixture::macos(18);
    let probe = Arc::new(HostProbe::default());
    let port = FakeFactoryPort::normal(Arc::clone(&probe));
    let (activation, _, _, owned, _, evidence) = bind_activation_with(fixture, port);
    let (request, pending) = activation.into_parts();
    drop(request.cancel());
    let wrong_state = ActivationFixture::macos(18).initial;
    let refusal = pending
        .authorize(wrong_state.clone(), evidence)
        .expect_err("may-own row cannot publish");
    assert_eq!(
        refusal.reason(),
        ExtensionRuntimePublicationAuthorizationError::OwnershipStateMismatch
    );
    let (pending, returned_entry, returned_evidence) = refusal.into_parts();
    assert_eq!(returned_entry, wrong_state);
    assert_eq!(returned_evidence, evidence);

    let mut wrong_lineage_template = ActivationFixture::macos(18).template;
    wrong_lineage_template.catalog_set_digest = ExtensionCatalogSetDigest::from_bytes([99; 32]);
    let wrong_lineage = wrong_lineage_template.owned(Some(
        ExtensionNativeOwnershipIdentity::parse(
            ExtensionRuntimeBackendTarget::MacosNative,
            EXPECTED_NATIVE_ID,
        )
        .expect("native ID"),
    ));
    let refusal = pending
        .authorize(wrong_lineage.clone(), evidence)
        .expect_err("different lineage cannot publish");
    assert_eq!(
        refusal.reason(),
        ExtensionRuntimePublicationAuthorizationError::OwnershipLineageMismatch
    );
    let (pending, returned_entry, _) = refusal.into_parts();
    assert_eq!(returned_entry, wrong_lineage);

    let substituted_expected = ExtensionExpectedNativeOwnershipIdentity::parse(
        ExtensionRuntimeBackendTarget::MacosNative,
        OTHER_NATIVE_ID,
    )
    .expect("canonical substituted expectation");
    let substituted_observed = ExtensionNativeOwnershipIdentity::parse(
        ExtensionRuntimeBackendTarget::MacosNative,
        OTHER_NATIVE_ID,
    )
    .expect("canonical substituted observation");
    let substituted_expectation = ActivationFixture::macos(18).template.entry_with_identities(
        3,
        ExtensionNativeOwnershipIntent::Acquire,
        ExtensionNativeOwnershipPhase::NativeOwned,
        Some(substituted_expected),
        Some(substituted_observed),
    );
    let refusal = pending
        .authorize(substituted_expectation.clone(), evidence)
        .expect_err("catalog expectation substitution changes owner lineage");
    assert_eq!(
        refusal.reason(),
        ExtensionRuntimePublicationAuthorizationError::OwnershipLineageMismatch
    );
    let (pending, returned_entry, _) = refusal.into_parts();
    assert_eq!(returned_entry, substituted_expectation);

    let refusal = pending
        .authorize(
            owned.clone(),
            ExtensionRuntimeOwnershipEvidence::Compatibility,
        )
        .expect_err("wrong evidence family cannot publish");
    assert_eq!(
        refusal.reason(),
        ExtensionRuntimePublicationAuthorizationError::NativeEvidenceMismatch
    );
    let (pending, returned_entry, returned_evidence) = refusal.into_parts();
    assert_eq!(returned_entry, owned);
    assert_eq!(
        returned_evidence,
        ExtensionRuntimeOwnershipEvidence::Compatibility
    );

    let other_id = ExtensionNativeOwnershipIdentity::parse(
        ExtensionRuntimeBackendTarget::MacosNative,
        OTHER_NATIVE_ID,
    )
    .expect("other native identity");
    let wrong_identity = ActivationFixture::macos(18).template.owned(Some(other_id));
    let refusal = pending
        .authorize(wrong_identity.clone(), evidence)
        .expect_err("erasing the catalog expectation changes owner lineage");
    assert_eq!(
        refusal.reason(),
        ExtensionRuntimePublicationAuthorizationError::OwnershipLineageMismatch
    );
    let (pending, returned_entry, _) = refusal.into_parts();
    assert_eq!(returned_entry, wrong_identity);

    let request = pending
        .authorize(owned, evidence)
        .expect("exact identity and lineage authorize publication");
    drop(request);
    assert_eq!(probe.publication_calls.load(Ordering::Relaxed), 0);
}

#[test]
fn native_release_frontier_accepts_expected_optional_observation_and_rejects_substitution() {
    for (seed, identity) in [(19, None), (20, Some(EXPECTED_NATIVE_ID))] {
        let fixture = ActivationFixture::macos(seed);
        let probe = Arc::new(HostProbe::default());
        let port = FakeFactoryPort::normal(Arc::clone(&probe));
        let (activation, _, runtime, _, _, _) = bind_activation_with(fixture, port);
        let (request, pending) = activation.into_parts();
        drop(request.cancel());
        let release_fixture = ActivationFixture::macos(seed);
        let durable_identity = identity.map(|value| {
            ExtensionNativeOwnershipIdentity::parse(
                ExtensionRuntimeBackendTarget::MacosNative,
                value,
            )
            .expect("native identity")
        });
        let revision = if durable_identity.is_some() { 4 } else { 3 };
        let release = release_fixture.template.entry_with_identities(
            revision,
            ExtensionNativeOwnershipIntent::Release,
            ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
            release_fixture.expected_native_identity,
            durable_identity,
        );
        let authority = pending
            .recover_after_absence(&release)
            .expect("optional expected observation accepted");
        assert_eq!(authority.fingerprint(), &runtime);
    }

    let fixture = ActivationFixture::macos(21);
    let probe = Arc::new(HostProbe::default());
    let port = FakeFactoryPort::normal(Arc::clone(&probe));
    let (activation, _, _, _, _, _) = bind_activation_with(fixture, port);
    let (request, pending) = activation.into_parts();
    drop(request.cancel());
    let wrong_identity = ExtensionNativeOwnershipIdentity::parse(
        ExtensionRuntimeBackendTarget::MacosNative,
        OTHER_NATIVE_ID,
    )
    .expect("wrong native identity");
    let release_fixture = ActivationFixture::macos(21);
    let release = release_fixture.template.entry_with_identities(
        4,
        ExtensionNativeOwnershipIntent::Release,
        ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
        release_fixture.expected_native_identity,
        Some(wrong_identity),
    );
    let refusal = pending
        .recover_after_absence(&release)
        .expect_err("substituted native identity rejected");
    assert_eq!(
        refusal.reason(),
        ExtensionRuntimePublicationAuthorizationError::ReleaseFrontierMismatch
    );
    assert_eq!(
        refusal
            .into_pending()
            .owner_address()
            .cas()
            .revision()
            .get(),
        2
    );

    let fixture = ActivationFixture::macos(23);
    let probe = Arc::new(HostProbe::default());
    let port = FakeFactoryPort::normal(Arc::clone(&probe));
    let (activation, _, _, _, _, _) = bind_activation_with(fixture, port);
    let (request, pending) = activation.into_parts();
    drop(request.cancel());
    let release_fixture = ActivationFixture::macos(23);
    let substituted_expected = ExtensionExpectedNativeOwnershipIdentity::parse(
        ExtensionRuntimeBackendTarget::MacosNative,
        OTHER_NATIVE_ID,
    )
    .expect("canonical substituted expectation");
    let release = release_fixture.template.entry_with_identities(
        3,
        ExtensionNativeOwnershipIntent::Release,
        ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
        Some(substituted_expected),
        None,
    );
    let refusal = pending
        .recover_after_absence(&release)
        .expect_err("catalog expectation substitution must be rejected");
    assert_eq!(
        refusal.reason(),
        ExtensionRuntimePublicationAuthorizationError::ReleaseFrontierMismatch
    );
}

#[test]
fn publication_refusal_returns_exact_authority_but_substitution_is_quarantined() {
    let fixture = ActivationFixture::compatibility(22);
    let probe = Arc::new(HostProbe::default());
    let mut port = FakeFactoryPort::normal(Arc::clone(&probe));
    port.publication_behavior = PublicationBehavior::RefuseExact;
    let (activation, _, _, owned, _, evidence) = bind_activation_with(fixture, port);
    let (request, pending) = activation.into_parts();
    drop(request.cancel());
    let request = pending.authorize(owned, evidence).expect("authorization");
    let refusal = request.publish().expect_err("host refusal");
    assert_eq!(refusal.reason(), ExtensionRuntimeHostBindError::Unavailable);
    let request = refusal
        .try_into_request()
        .expect("ordinary refusal returns exact request");
    drop(request);

    let fixture = ActivationFixture::compatibility(24);
    let expected_runtime = fixture.runtime();
    let substitute = same_fingerprint_different_lineage_authority(
        24,
        ExtensionRuntimeBackendTarget::LinuxCompatibility,
    );
    assert_eq!(substitute.fingerprint(), &expected_runtime);
    assert!(!substitute.matches_native_ownership_lineage(&fixture.initial));
    let probe = Arc::new(HostProbe::default());
    let mut port = FakeFactoryPort::normal(Arc::clone(&probe));
    port.publication_behavior = PublicationBehavior::RefuseSwapped;
    port.substitute = Some(substitute);
    let (activation, _, _, owned, _, evidence) = bind_activation_with(fixture, port);
    let (request, pending) = activation.into_parts();
    drop(request.cancel());
    let refusal = pending
        .authorize(owned, evidence)
        .expect("authorization")
        .publish()
        .expect_err("swapped host refusal");
    assert_eq!(
        refusal.reason(),
        ExtensionRuntimeHostBindError::InternalInvariant
    );
    assert!(refusal.requires_fail_stop());
    assert!(refusal.retained_bytes() <= MAX_EXTENSION_RUNTIME_HOST_QUARANTINE_RETAINED_BYTES);
    assert!(refusal.try_into_request().is_err());
}

#[test]
fn published_grant_rebind_preserves_owner_and_advances_exact_operation_authority() {
    let seed = 34;
    let fixture = ActivationFixture::compatibility(seed);
    let template = fixture.template.clone();
    let profile = template.key.profile();
    let install_id = template.key.install_id();
    let probe = Arc::new(HostProbe::default());
    let port = FakeFactoryPort::normal(Arc::clone(&probe));
    let (mut receipt, _, current_runtime, _) = published_receipt_with(fixture, port);
    let stale_witness = receipt
        .mint_active_tab_grant_witness(&current_runtime, ExtensionUserInvocationKind::ToolbarAction)
        .expect("current witness");

    let next_eligibility = eligibility_at_revision(
        seed,
        profile,
        install_id,
        current_runtime.grant_revision().next().unwrap(),
        &["activeTab", "scripting", "tabs"],
    );
    let next_runtime = next_eligibility.fingerprint(current_runtime.instance().generation());
    let rebound = template.with_grants_from(&next_eligibility);
    let receipt = receipt
        .rebind_grants(rebound.owned(None), next_eligibility)
        .expect("exact additive grant rebind");

    assert_eq!(receipt.registry_generation().get(), 17);
    assert!(receipt.retained_bytes() <= MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES);
    assert!(!stale_witness.matches(&next_runtime, ExtensionUserInvocationKind::ToolbarAction));
    let mut receipt = receipt;
    assert!(matches!(
        receipt.mint_active_tab_grant_witness(
            &current_runtime,
            ExtensionUserInvocationKind::ToolbarAction,
        ),
        Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch)
    ));
    assert!(receipt
        .mint_active_tab_grant_witness(&next_runtime, ExtensionUserInvocationKind::ToolbarAction,)
        .is_ok());

    let authority = receipt
        .reclaim_after_absence(&rebound.release_absent(4, None))
        .expect("rebound authority remains exactly reclaimable");
    assert_eq!(authority.fingerprint(), &next_runtime);
}

#[test]
fn published_grant_rebind_refusal_is_lossless_and_substitution_is_quarantined() {
    let seed = 35;
    let fixture = ActivationFixture::compatibility(seed);
    let template = fixture.template.clone();
    let profile = template.key.profile();
    let install_id = template.key.install_id();
    let probe = Arc::new(HostProbe::default());
    let mut port = FakeFactoryPort::normal(Arc::clone(&probe));
    port.publication_behavior = PublicationBehavior::RefuseGrantExact;
    let (receipt, _, current_runtime, _) = published_receipt_with(fixture, port);
    let next_eligibility = eligibility_at_revision(
        seed,
        profile,
        install_id,
        current_runtime.grant_revision().next().unwrap(),
        &["activeTab", "scripting", "tabs"],
    );
    let next_runtime = next_eligibility.fingerprint(current_runtime.instance().generation());
    let rebound = template.with_grants_from(&next_eligibility);
    let refusal = receipt
        .rebind_grants(rebound.owned(None), next_eligibility)
        .expect_err("ordinary host refusal");
    assert_eq!(refusal.reason(), ExtensionRuntimeHostBindError::Unavailable);
    assert!(!refusal.requires_fail_stop());
    let (mut receipt, eligibility) = refusal
        .try_into_parts()
        .expect("ordinary refusal returns both exact inputs");
    assert_eq!(
        eligibility.fingerprint(current_runtime.instance().generation()),
        next_runtime
    );
    assert!(
        receipt
            .mint_active_tab_grant_witness(
                &current_runtime,
                ExtensionUserInvocationKind::ToolbarAction,
            )
            .is_ok()
    );

    let seed = 36;
    let fixture = ActivationFixture::compatibility(seed);
    let template = fixture.template.clone();
    let profile = template.key.profile();
    let install_id = template.key.install_id();
    let probe = Arc::new(HostProbe::default());
    let mut port = FakeFactoryPort::normal(probe);
    port.publication_behavior = PublicationBehavior::RefuseGrantSwapped;
    port.substitute_eligibility = Some(eligibility_at_revision(
        seed.wrapping_add(1),
        ProfileId::from(900),
        ExtensionInstallId::from(901),
        ExtensionGrantRevision::INITIAL.next().unwrap(),
        &["activeTab", "scripting", "tabs"],
    ));
    let (receipt, _, current_runtime, _) = published_receipt_with(fixture, port);
    let next_eligibility = eligibility_at_revision(
        seed,
        profile,
        install_id,
        current_runtime.grant_revision().next().unwrap(),
        &["activeTab", "scripting", "tabs"],
    );
    let rebound = template.with_grants_from(&next_eligibility);
    let refusal = receipt
        .rebind_grants(rebound.owned(None), next_eligibility)
        .expect_err("substituted eligibility must be quarantined");
    assert_eq!(
        refusal.reason(),
        ExtensionRuntimeHostBindError::InternalInvariant
    );
    assert!(refusal.requires_fail_stop());
    assert!(refusal.retained_bytes() <= MAX_EXTENSION_RUNTIME_HOST_QUARANTINE_RETAINED_BYTES);
    assert!(refusal.try_into_parts().is_err());
}

#[test]
fn published_witness_ingress_prevalidates_runtime_and_validates_host_output() {
    let fixture = ActivationFixture::compatibility(25);
    let expected_owner = ExtensionRuntimeOwnerAddress::from_entry(&fixture.initial);
    let expected_owned = fixture.owned();
    let probe = Arc::new(HostProbe::default());
    let port = FakeFactoryPort::normal(Arc::clone(&probe));
    let (mut receipt, _, runtime, _) = published_receipt_with(fixture, port);
    let wrong_runtime = ActivationFixture::compatibility(26).runtime();
    assert!(matches!(
        receipt.mint_active_tab_grant_witness(
            &wrong_runtime,
            ExtensionUserInvocationKind::ToolbarAction,
        ),
        Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch)
    ));
    assert_eq!(probe.active_witness_calls.load(Ordering::Relaxed), 0);

    let witness = receipt
        .mint_active_tab_grant_witness(&runtime, ExtensionUserInvocationKind::ToolbarAction)
        .expect("exact active-tab witness");
    assert!(witness.matches(&runtime, ExtensionUserInvocationKind::ToolbarAction));
    let document = receipt
        .mint_document_authority_witness(&runtime, ExtensionDocumentPurpose::ExecuteScript)
        .expect("exact document witness");
    assert!(document.matches(&runtime, ExtensionDocumentPurpose::ExecuteScript));
    let publication = probe
        .last_publication
        .lock()
        .expect("probe lock")
        .clone()
        .expect("publication observation");
    assert_eq!(publication.owner, expected_owner);
    assert_eq!(publication.generation, receipt.registry_generation());
    assert_eq!(publication.owned_cas, expected_owned.cas());
    assert_eq!(
        publication.evidence,
        ExtensionRuntimeOwnershipEvidence::Compatibility
    );
    let call = probe
        .last_witness
        .lock()
        .expect("probe lock")
        .clone()
        .expect("witness observation");
    assert_eq!(call.owner, expected_owner);
    assert_eq!(call.generation, receipt.registry_generation());
    assert_eq!(call.runtime, runtime);

    let substitute = ActivationFixture::compatibility(27).authority;
    let fixture = ActivationFixture::compatibility(28);
    let probe = Arc::new(HostProbe::default());
    let mut port = FakeFactoryPort::normal(Arc::clone(&probe));
    port.publication_behavior = PublicationBehavior::BadActiveWitness;
    port.substitute = Some(substitute);
    let (mut receipt, _, runtime, _) = published_receipt_with(fixture, port);
    assert!(matches!(
        receipt
            .mint_active_tab_grant_witness(&runtime, ExtensionUserInvocationKind::ToolbarAction,),
        Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch)
    ));

    let fixture = ActivationFixture::compatibility(29);
    let probe = Arc::new(HostProbe::default());
    let mut port = FakeFactoryPort::normal(Arc::clone(&probe));
    port.publication_behavior = PublicationBehavior::BadDocumentWitness;
    let (mut receipt, _, runtime, _) = published_receipt_with(fixture, port);
    assert!(matches!(
        receipt.mint_document_authority_witness(&runtime, ExtensionDocumentPurpose::ExecuteScript,),
        Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch)
    ));
}

#[test]
fn reclaim_is_frontier_bound_retryable_and_quarantines_substituted_authority() {
    let fixture = ActivationFixture::compatibility(30);
    let probe = Arc::new(HostProbe::default());
    let mut port = FakeFactoryPort::normal(Arc::clone(&probe));
    port.reclaim_failures = 1;
    let (receipt, _, runtime, release) = published_receipt_with(fixture, port);
    let wrong_release = ActivationFixture::compatibility(30).initial;
    let refusal = receipt
        .reclaim_after_absence(&wrong_release)
        .expect_err("wrong frontier rejected before host");
    assert_eq!(
        refusal.reason(),
        ExtensionRuntimePublicationReclaimError::Authorization(
            ExtensionRuntimePublicationAuthorizationError::ReleaseFrontierMismatch
        )
    );
    assert_eq!(probe.reclaim_calls.load(Ordering::Relaxed), 0);
    let receipt = refusal
        .try_into_receipt()
        .expect("authorization refusal retains receipt");
    let refusal = receipt
        .reclaim_after_absence(&release)
        .expect_err("transient host reclaim refusal");
    assert_eq!(
        refusal.reason(),
        ExtensionRuntimePublicationReclaimError::Host(ExtensionRuntimeHostBindError::Unavailable)
    );
    let receipt = refusal
        .try_into_receipt()
        .expect("host refusal retains receipt for retry");
    let authority = receipt
        .reclaim_after_absence(&release)
        .expect("retry returns exact authority");
    assert_eq!(authority.fingerprint(), &runtime);
    assert_eq!(probe.reclaim_calls.load(Ordering::Relaxed), 2);

    let fixture = ActivationFixture::compatibility(32);
    let expected_runtime = fixture.runtime();
    let substitute = same_fingerprint_different_lineage_authority(
        32,
        ExtensionRuntimeBackendTarget::LinuxCompatibility,
    );
    assert_eq!(substitute.fingerprint(), &expected_runtime);
    assert!(!substitute.matches_native_ownership_lineage(&fixture.initial));
    let probe = Arc::new(HostProbe::default());
    let mut port = FakeFactoryPort::normal(Arc::clone(&probe));
    port.publication_behavior = PublicationBehavior::SwapReclaim;
    port.substitute = Some(substitute);
    let (receipt, _, _, release) = published_receipt_with(fixture, port);
    let refusal = receipt
        .reclaim_after_absence(&release)
        .expect_err("substituted reclaim authority quarantined");
    assert_eq!(
        refusal.reason(),
        ExtensionRuntimePublicationReclaimError::Host(
            ExtensionRuntimeHostBindError::InternalInvariant
        )
    );
    assert!(refusal.requires_fail_stop());
    assert!(refusal.retained_bytes() <= MAX_EXTENSION_RUNTIME_HOST_QUARANTINE_RETAINED_BYTES);
    assert!(refusal.try_into_receipt().is_err());
}

#[test]
fn unpublished_authority_recovery_requires_exact_release_lineage() {
    let fixture = ActivationFixture::compatibility(33);
    let probe = Arc::new(HostProbe::default());
    let port = FakeFactoryPort::normal(Arc::clone(&probe));
    let (activation, _, runtime, owned, release, evidence) = bind_activation_with(fixture, port);
    let (request, pending) = activation.into_parts();
    drop(request.cancel());
    let wrong_release = ActivationFixture::compatibility(34).release();
    let refusal = pending
        .recover_after_absence(&wrong_release)
        .expect_err("pending recovery rejects another owner");
    assert_eq!(
        refusal.reason(),
        ExtensionRuntimePublicationAuthorizationError::ReleaseFrontierMismatch
    );
    let pending = refusal.into_pending();
    let request = pending
        .authorize(owned, evidence)
        .expect("exact owned row authorizes");
    let wrong_release = ActivationFixture::compatibility(35).release();
    let refusal = request
        .recover_after_absence(&wrong_release)
        .expect_err("authorized request rejects another owner");
    assert_eq!(
        refusal.reason(),
        ExtensionRuntimePublicationAuthorizationError::ReleaseFrontierMismatch
    );
    let authority = refusal
        .into_request()
        .recover_after_absence(&release)
        .expect("exact release returns unpublished authority");
    assert_eq!(authority.fingerprint(), &runtime);
    assert_eq!(probe.publication_calls.load(Ordering::Relaxed), 0);
}

#[test]
fn dropping_published_receipt_never_reclaims_or_invokes_lifecycle() {
    let fixture = ActivationFixture::compatibility(36);
    let probe = Arc::new(HostProbe::default());
    let port = FakeFactoryPort::normal(Arc::clone(&probe));
    let (receipt, _, _, _) = published_receipt_with(fixture, port);
    assert_eq!(probe.publication_calls.load(Ordering::Relaxed), 1);
    drop(receipt);
    assert_eq!(probe.publication_drops.load(Ordering::Relaxed), 1);
    assert_eq!(probe.reclaim_calls.load(Ordering::Relaxed), 0);
    assert_eq!(probe.lifecycle_calls.load(Ordering::Relaxed), 0);
    assert_eq!(probe.preattachment_restores.load(Ordering::Relaxed), 0);
}
