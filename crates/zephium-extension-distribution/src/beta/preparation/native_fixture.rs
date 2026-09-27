//! Trusted host test double: it inspects the real delegated root, then refuses
//! before any OS-native activation. It never simulates successful native work.
use std::num::NonZeroU64;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Instant;
use zephium_core::extensions::*;
use zephium_extension_runtime_api::*;

pub(super) struct Factory {
    pub calls: Arc<AtomicUsize>,
    pub refuse: bool,
}
impl ExtensionRuntimeHostFactoryPort for Factory {
    fn bind_activation(
        &mut self,
        context: ExtensionRuntimeHostActivationContext<'_>,
    ) -> Result<ExtensionRuntimeHostActivationPorts, ExtensionRuntimeHostBindError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if self.refuse {
            return Err(ExtensionRuntimeHostBindError::Unavailable);
        }
        let generation = ExtensionRuntimeHostRegistryGeneration::new(1).unwrap();
        Ok(ExtensionRuntimeHostActivationPorts::new(
            generation,
            Box::new(Lifecycle {
                issuer: context.absence_evidence_issuer().bind(generation),
                last: None,
            }),
            Box::new(Publication),
        ))
    }
    fn bind_recovery(
        &mut self,
        _: ExtensionRuntimeHostRecoveryContext,
    ) -> Result<Box<dyn ExtensionRuntimeHostOwnershipPort>, ExtensionRuntimeHostBindError> {
        Err(ExtensionRuntimeHostBindError::Unavailable)
    }
}
struct Lifecycle {
    issuer: ExtensionRuntimeBoundAbsenceEvidenceIssuer,
    last: Option<ExtensionRuntimeAbsenceEvidence>,
}
impl ExtensionRuntimeOwnershipPort for Lifecycle {
    fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn accepts_absence_evidence(&self, evidence: ExtensionRuntimeAbsenceEvidence) -> bool {
        self.last == Some(evidence) && self.issuer.accepts(evidence, NonZeroU64::new(1).unwrap())
    }
    fn retire_until(&mut self, _: Instant) -> ExtensionRuntimeRetirementDisposition {
        panic!("fixture never activated a native owner")
    }
    fn reconcile_ownership_until(&mut self, _: Instant) -> ExtensionRuntimeOwnershipDisposition {
        panic!("fixture never activated a native owner")
    }
}
impl ExtensionRuntimeHostLifecyclePort for Lifecycle {}
impl ExtensionRuntimeLifecyclePort for Lifecycle {
    fn activate_until(
        &mut self,
        access: &mut ExtensionPackageAccessView<'_>,
        deadline: Instant,
    ) -> ExtensionRuntimeActivationDisposition {
        assert!(deadline > Instant::now());
        let mut root = access
            .take_native_root_lease()
            .expect("real Beta provider transfers its preallocated root");
        assert!(matches!(
            access.take_native_root_lease(),
            Err(ExtensionPackageAccessError::Inactive)
        ));
        let mut visits = 0;
        let mut visitor = |path: &std::path::Path| {
            visits += 1;
            assert!(path.is_absolute());
            let manifest = std::fs::read(path.join("manifest.json")).unwrap();
            let document: serde_json::Value = serde_json::from_slice(&manifest).unwrap();
            assert!(document["key"].is_string());
            Ok(())
        };
        root.with_verified_path(&mut visitor).unwrap().unwrap();
        assert!(matches!(
            root.with_verified_path(&mut visitor),
            Err(ExtensionPackageAccessError::Inactive)
        ));
        assert_eq!(visits, 1);
        drop(root);
        // Self-authored evidence: no OS-native API has been called here.
        let absence = self
            .issuer
            .mint_activation_never_entered(NonZeroU64::new(1).unwrap())
            .unwrap();
        self.last = Some(absence);
        ExtensionRuntimeActivationDisposition::Rejected {
            failure: ExtensionRuntimeFailure::Internal,
            absence,
        }
    }
}
struct Publication;
impl ExtensionRuntimeHostPublicationPort for Publication {
    fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn publish_operation_authority(
        &mut self,
        _: ExtensionRuntimeOwnerAddress,
        _: ExtensionRuntimeHostRegistryGeneration,
        _: &ExtensionNativeOwnershipEntry,
        _: ExtensionRuntimeOwnershipEvidence,
        authority: ExtensionRuntimeOperationAuthority,
    ) -> Result<(), ExtensionRuntimeHostPublicationPortRefusal> {
        Err(ExtensionRuntimeHostPublicationPortRefusal::new(
            ExtensionRuntimeHostBindError::Unavailable,
            authority,
        ))
    }
    fn rebind_operation_authority(
        &mut self,
        _: ExtensionRuntimeOwnerAddress,
        _: ExtensionRuntimeHostRegistryGeneration,
        _: &ExtensionNativeOwnershipEntry,
        _: &ExtensionNativeOwnershipEntry,
        eligibility: ExtensionRuntimeEligibility,
    ) -> Result<(), ExtensionRuntimeHostGrantRebindPortRefusal> {
        Err(ExtensionRuntimeHostGrantRebindPortRefusal::new(
            ExtensionRuntimeHostBindError::Unavailable,
            eligibility,
        ))
    }
    fn reclaim_operation_authority(
        &mut self,
        _: ExtensionRuntimeOwnerAddress,
        _: ExtensionRuntimeHostRegistryGeneration,
        _: &ExtensionNativeOwnershipEntry,
    ) -> Result<ExtensionRuntimeOperationAuthority, ExtensionRuntimeHostBindError> {
        Err(ExtensionRuntimeHostBindError::Unavailable)
    }
    fn mint_active_tab_grant_witness(
        &mut self,
        _: ExtensionRuntimeOwnerAddress,
        _: ExtensionRuntimeHostRegistryGeneration,
        _: &ExtensionRuntimeFingerprint,
        _: ExtensionUserInvocationKind,
    ) -> Result<ExtensionActiveTabGrantWitness, ExtensionOperationAuthorityDenial> {
        panic!("fixture cannot publish")
    }
    fn mint_document_authority_witness(
        &mut self,
        _: ExtensionRuntimeOwnerAddress,
        _: ExtensionRuntimeHostRegistryGeneration,
        _: &ExtensionRuntimeFingerprint,
        _: ExtensionDocumentPurpose,
    ) -> Result<ExtensionDocumentAuthorityWitness, ExtensionOperationAuthorityDenial> {
        panic!("fixture cannot publish")
    }
}
