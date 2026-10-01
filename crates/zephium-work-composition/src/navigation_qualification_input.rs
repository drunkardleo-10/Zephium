//! Shared frozen development admission; a static definition selects the task
//! and explicit retention without changing any runtime/native lifecycle.
use super::*;
use crate::native_work_clock::{authority_window, NativeWorkClock};
use std::sync::Arc;
use std::time::{Duration, Instant};
#[cfg(not(feature = "public-qualification"))]
use zephium_agent_provider_transport::load_macos_development_openai_credential;
#[cfg(feature = "public-qualification")]
use zephium_agent_provider_transport::load_macos_probe_openai_credential;
use zephium_agent_provider_transport::{AgentProviderCredential, AgentProviderTransportConfig};
use zephium_agent_runtime::AgentRuntimeConfig;

const TOTAL: Duration = Duration::from_secs(150);

/// Called only by the explicitly admitted development worker. The one absolute
/// deadline includes credential loading and is never restarted after a hop.
pub fn load_request(
    started: Instant,
    profile: zephium_app::AgentWorkProfileBinding,
) -> Result<crate::TrustedWorkRequest, &'static str> {
    load_configured_request(started, profile, &DEFINITION)
}

pub(crate) fn load_configured_request(
    started: Instant,
    profile: zephium_app::AgentWorkProfileBinding,
    definition: &'static QualificationDefinition,
) -> Result<crate::TrustedWorkRequest, &'static str> {
    #[cfg(not(feature = "public-qualification"))]
    let credential = load_macos_development_openai_credential().map_err(|_| "credential")?;
    // Public qualifications are release-forbidden and rebuilt frequently. Use
    // Apple's stable signed Keychain client so a new ad-hoc app signature does
    // not manufacture a fresh authorization prompt on every test build.
    #[cfg(feature = "public-qualification")]
    let credential = load_macos_probe_openai_credential().map_err(|_| "credential")?;
    request(credential, started, profile, definition)
}

fn request(
    credential: AgentProviderCredential,
    started: Instant,
    profile: zephium_app::AgentWorkProfileBinding,
    definition: &QualificationDefinition,
) -> Result<crate::TrustedWorkRequest, &'static str> {
    let identity = ContextIdentity::new(
        ContextId::generate(),
        ContextRunId::generate(),
        profile.profile(),
        ContextKind::Owned,
    );
    let deadline = started.checked_add(TOTAL).ok_or("deadline")?;
    if Instant::now() >= deadline {
        return Err("deadline");
    }
    let input = input(identity, profile.storage_class(), deadline, definition)?;
    Ok((definition.configure_request)(
        crate::TrustedWorkRequest::new(
            input,
            zephium_app::AgentWorkApplicationConfig::new(
                AgentRuntimeConfig::STANDARD,
                AgentProviderTransportConfig::STANDARD,
            ),
            credential,
            (definition.task)(identity).map_err(|_| "task")?,
        )
        .with_browser_profile(profile),
    ))
}

fn input(
    identity: ContextIdentity,
    storage: ContextProfileStorageClass,
    deadline: Instant,
    definition: &QualificationDefinition,
) -> Result<AgentWorkRunInput, &'static str> {
    let origin = SemanticOrigin::parse(definition.origin).map_err(|_| "origin")?;
    let effects = AgentEffectScope::try_new(&[SemanticEffectClass::Read]).map_err(|_| "effects")?;
    // Reservation ceiling, not expected charge. Luna's catalog-bounded initial
    // reservation is 77,830 micro-USD; all exact usage shares this original cap.
    let budget = AgentRunBudget::try_new(8, 100_000, 100_000, 1).map_err(|_| "budget")?;
    let node = AgentPlanNodeId::generate();
    let authority = AgentPlanNodeAuthority::try_new(
        vec![identity.profile()],
        vec![AgentAccountScope::Anonymous],
        vec![origin.clone()],
        SemanticSensitivity::Public,
        effects,
    )
    .map_err(|_| "authority")?;
    let authority = (definition.authority)(authority)?;
    let (issued, expires) = authority_window(deadline)?;
    let manifest = AgentRunManifest::try_new(
        AgentRunManifestId::generate(),
        identity.owner(),
        AgentRunScope::try_new(
            vec![identity.profile()],
            vec![AgentAccountScope::Anonymous],
            vec![origin],
            SemanticSensitivity::Public,
            effects,
            Vec::new(),
        )
        .map_err(|_| "scope")?,
        budget,
        issued,
        expires,
        vec![AgentPlanNodeScope::new(node, authority, budget, expires)],
    )
    .map_err(|_| "manifest")?;
    let ids = TerraControllerIds::try_new(
        AgentSupervisorId::new(1).ok_or("id")?,
        AgentSupervisorAttemptId::new(1).ok_or("id")?,
        AgentSupervisorCancellationId::new(1).ok_or("id")?,
        AgentModelCallId::new(1).ok_or("id")?,
        [1, 2, 3, 4].map(|id| AgentAuditEventId::new(id).expect("fixed nonzero ID")),
        AgentAuditDeliveryId::new(1).ok_or("id")?,
    )
    .map_err(|_| "ids")?;
    AgentWorkRunInput::try_new(
        manifest,
        AgentPlanLeaseBinding::new(AgentPlanLeaseId::generate(), node),
        AgentWorkContextSpec::try_new_with_document_policy(
            identity,
            storage,
            ContextNavigationTarget::parse(definition.initial).map_err(|_| "target")?,
            definition.document_policy,
        )
        .map_err(|_| "context")?,
        definition.objective.into(),
        AgentWorkRunSettings::new(
            AgentBrowserModel::Luna,
            ids,
            Arc::new(NativeWorkClock),
            deadline,
        ),
    )
    .map_err(|_| "input")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_and_retained_inputs_share_native_epoch_without_renewing_elapsed_time() {
        let started = Instant::now().checked_sub(Duration::from_secs(37)).unwrap();
        let deadline = started + TOTAL;
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            1_u128.into(),
            ContextKind::Owned,
        );
        #[allow(clippy::single_element_loop)] // Optional static witness matrix.
        for definition in [
            &DEFINITION,
            #[cfg(feature = "discovery-qualification")]
            &crate::discovery_qualification::DEFINITION,
            #[cfg(feature = "retained-product-qualification")]
            &crate::retained_product_qualification::DEFINITION,
        ] {
            let prepared = input(
                identity,
                ContextProfileStorageClass::Ephemeral,
                deadline,
                definition,
            )
            .unwrap();
            crate::native_work_clock::assert_native_timing(&prepared, deadline);
        }
    }

    #[test]
    fn configuration_diagnostic_matches_selected_static_witness() {
        assert_eq!(DEFINITION.document_policy, WorkBrowserDocumentPolicy::Exact);
        assert_eq!(super::super::configuration_diagnostic(), "work-application-navigation-config: provider=OpenAIResponses model=gpt-5.6-luna retention=stateless task=react-one-hop-v1");
        #[cfg(feature = "discovery-qualification")]
        {
            assert_eq!(
                crate::discovery_qualification::DEFINITION.document_policy,
                WorkBrowserDocumentPolicy::Exact
            );
            assert_eq!(crate::discovery_qualification::configuration_diagnostic(), "work-application-navigation-config: provider=OpenAIResponses model=gpt-5.6-luna retention=inspectable-public task=react-open-objective-v1");
        }
    }

    #[test]
    fn frozen_owned_input_is_valid_without_provider_or_native_activity() {
        let started = Instant::now();
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            zephium_core::ids::ProfileId::generate(),
            ContextKind::Owned,
        );
        for storage in [
            ContextProfileStorageClass::Durable,
            ContextProfileStorageClass::Ephemeral,
        ] {
            #[allow(clippy::single_element_loop)] // Optional static witness matrix.
            for definition in [
                &DEFINITION,
                #[cfg(feature = "discovery-qualification")]
                &crate::discovery_qualification::DEFINITION,
            ] {
                assert!(input(identity, storage, started + TOTAL, definition).is_ok());
                let task = (definition.task)(identity).unwrap();
                assert_eq!(task.navigation_discovery().is_some(), definition.inspection);
            }
        }
        let task = task(identity).unwrap();
        #[cfg(feature = "retained-product-qualification")]
        {
            let definition = &crate::retained_product_qualification::DEFINITION;
            for storage in [
                ContextProfileStorageClass::Durable,
                ContextProfileStorageClass::Ephemeral,
            ] {
                let prepared = input(identity, storage, started + TOTAL, definition).unwrap();
                assert!(
                    prepared.retained_resource_spec().is_ok(),
                    "the selected witness must remain valid for the retained product entry"
                );
                assert_eq!(
                    prepared.retained_resource_spec().unwrap().document_policy,
                    definition.document_policy
                );
            }
            let task = (definition.task)(identity).unwrap();
            assert!(task.navigation_target().is_none());
            assert_eq!(task.navigation_discovery().unwrap().max_hops(), 2);
        }
        assert_eq!(task.navigation_target().is_some(), !DISCOVERY);
        assert_eq!(task.navigation_discovery().is_some(), DISCOVERY);
        assert!(task.navigation_route().is_none());
    }
}
