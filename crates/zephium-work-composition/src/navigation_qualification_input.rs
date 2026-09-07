//! Shared frozen development admission; a static definition selects the task
//! and explicit retention without changing any runtime/native lifecycle.
use super::*;
use std::sync::Arc;
use std::time::{Duration, Instant};
use zephium_agent_provider_transport::{
    load_macos_development_openai_credential, AgentProviderCredential, AgentProviderTransportConfig,
};
use zephium_agent_runtime::AgentRuntimeConfig;

const TOTAL: Duration = Duration::from_secs(150);
const INITIAL: &str = "https://react.dev/learn";

struct Clock(Instant);
impl TerraControllerClock for Clock {
    fn now(&self) -> Result<AgentPolicyInstant, TerraControllerClockError> {
        let elapsed = u64::try_from(self.0.elapsed().as_millis())
            .map_err(|_| TerraControllerClockError::Invalid)?;
        Ok(AgentPolicyInstant::from_millis(
            1_000_u64
                .checked_add(elapsed)
                .ok_or(TerraControllerClockError::Invalid)?,
        ))
    }
}

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
    let credential = load_macos_development_openai_credential().map_err(|_| "credential")?;
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
    let input = input(
        identity,
        profile.storage_class(),
        started,
        deadline,
        definition,
    )?;
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
    started: Instant,
    deadline: Instant,
    definition: &QualificationDefinition,
) -> Result<AgentWorkRunInput, &'static str> {
    let origin = SemanticOrigin::parse(ORIGIN).map_err(|_| "origin")?;
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
    let expires = AgentPolicyInstant::from_millis(151_000);
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
        AgentPolicyInstant::from_millis(1_000),
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
        AgentWorkContextSpec::try_new(
            identity,
            storage,
            ContextNavigationTarget::parse(INITIAL).map_err(|_| "target")?,
        )
        .map_err(|_| "context")?,
        definition.objective.into(),
        AgentWorkRunSettings::new(
            AgentBrowserModel::Luna,
            ids,
            Arc::new(Clock(started)),
            deadline,
        ),
    )
    .map_err(|_| "input")
}

#[cfg(test)]
mod tests {
    use super::*;

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
            for definition in [
                &DEFINITION,
                #[cfg(feature = "discovery-qualification")]
                &crate::discovery_qualification::DEFINITION,
            ] {
                assert!(input(identity, storage, started, started + TOTAL, definition).is_ok());
                let task = (definition.task)(identity).unwrap();
                assert_eq!(task.navigation_discovery().is_some(), definition.inspection);
            }
        }
        let task = task(identity).unwrap();
        assert_eq!(task.navigation_target().is_some(), !DISCOVERY);
        assert_eq!(task.navigation_discovery().is_some(), DISCOVERY);
        assert!(task.navigation_route().is_none());
    }
}
