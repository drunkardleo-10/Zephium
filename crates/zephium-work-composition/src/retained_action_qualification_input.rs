//! One frozen local-only policy. No page or model input creates authority.
use super::*;
use std::{sync::Arc, time::Duration};
use zephium_agent_provider_transport::{
    load_macos_probe_openai_credential, AgentProviderTransportConfig,
};
use zephium_agent_runtime::AgentRuntimeConfig;
const TOTAL: Duration = Duration::from_secs(150);
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

pub fn load_request(
    started: Instant,
    profile: zephium_app::AgentWorkProfileBinding,
) -> Result<crate::TrustedWorkRequest, &'static str> {
    let deadline = started.checked_add(TOTAL).ok_or("deadline")?;
    if Instant::now() >= deadline {
        return Err("deadline");
    }
    let fixture = FixtureServer::start().map_err(|_| "fixture")?;
    let target = ContextNavigationTarget::parse(&fixture.url(FixtureRoute::RetainedLocalForm))
        .map_err(|_| "target")?;
    let origin = SemanticOrigin::parse(&fixture.url(FixtureRoute::RetainedLocalForm))
        .map_err(|_| "origin")?;
    let identity = ContextIdentity::new(
        ContextId::generate(),
        ContextRunId::generate(),
        profile.profile(),
        ContextKind::Owned,
    );
    let input = input(
        identity,
        profile.storage_class(),
        started,
        deadline,
        target,
        origin.clone(),
    )?;
    let task = LocalTask::new(identity, origin, fixture).map_err(|_| "task")?;
    let credential = load_macos_probe_openai_credential().map_err(|_| "credential")?;
    if Instant::now() >= deadline {
        return Err("deadline");
    }
    Ok(crate::TrustedWorkRequest::new(
        input,
        zephium_app::AgentWorkApplicationConfig::new(
            AgentRuntimeConfig::STANDARD,
            AgentProviderTransportConfig::STANDARD,
        ),
        credential,
        Box::new(task),
    )
    .with_browser_profile(profile)
    .with_public_qualification_retention())
}

fn input(
    identity: ContextIdentity,
    storage: ContextProfileStorageClass,
    started: Instant,
    deadline: Instant,
    target: ContextNavigationTarget,
    origin: SemanticOrigin,
) -> Result<AgentWorkRunInput, &'static str> {
    let effects =
        AgentEffectScope::try_new(&[SemanticEffectClass::Read, SemanticEffectClass::LocalWrite])
            .map_err(|_| "effects")?;
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
        AgentWorkContextSpec::try_new_with_document_policy(
            identity,
            storage,
            target,
            WorkBrowserDocumentPolicy::Exact,
        )
        .map_err(|_| "context")?,
        OBJECTIVE.into(),
        AgentWorkRunSettings::new(
            AgentBrowserModel::Luna,
            ids,
            Arc::new(Clock(started)),
            deadline,
        ),
    )
    .map_err(|_| "input")
}
