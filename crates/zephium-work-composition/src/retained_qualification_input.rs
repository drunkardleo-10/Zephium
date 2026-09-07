//! Shared fixed authority and accounting for the release-excluded retained witnesses.
use super::*;

struct Clock;
impl TerraControllerClock for Clock {
    fn now(&self) -> Result<AgentPolicyInstant, TerraControllerClockError> {
        zephium_engine::work_browser_monotonic_now().ok_or(TerraControllerClockError::Invalid)
    }
}
pub(super) fn input(
    identity: ContextIdentity,
    target: ContextNavigationTarget,
    issued: AgentPolicyInstant,
    expires: AgentPolicyInstant,
    deadline: Instant,
    objective: &str,
) -> Result<AgentWorkRunInput, &'static str> {
    let origin = SemanticOrigin::parse(target.as_url().as_str()).map_err(|_| "origin")?;
    let effects = AgentEffectScope::try_new(&[SemanticEffectClass::Read]).map_err(|_| "effects")?;
    // Luna reserves 77,830 micro-USD before exact whole-request counting. The
    // development witness must admit that reservation; actual provider usage
    // still settles against this same run/node ceiling and original ledger.
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
        AgentWorkContextSpec::try_new(identity, ContextProfileStorageClass::Ephemeral, target)
            .map_err(|_| "context")?,
        objective.into(),
        AgentWorkRunSettings::new(AgentBrowserModel::Luna, ids, Arc::new(Clock), deadline),
    )
    .map_err(|_| "input")
}
