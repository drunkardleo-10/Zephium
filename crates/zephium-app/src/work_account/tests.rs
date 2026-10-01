use super::*;
use std::sync::{Arc, Mutex};
use zephium_agentic::*;
use zephium_core::profiles::{Profile, ProfileKind};

type Published =
    Arc<Mutex<Result<(SemanticOrigin, AgentContextAccountBinding), AgentWorkAccountFailure>>>;
struct Collector(Published);
impl AgentWorkAccountCollector for Collector {
    fn collect(
        &self,
        _: ContextJoin,
    ) -> Result<AgentWorkCollectedAccount, AgentWorkAccountFailure> {
        self.0
            .lock()
            .unwrap()
            .clone()
            .map(|(origin, binding)| AgentWorkCollectedAccount { origin, binding })
    }
}

fn fixture() -> (
    AgentWorkEnrolledAccount,
    Published,
    ContextRegistry,
    ContextJoin,
) {
    let profile = AgentWorkProfileBinding::from_profile(&Profile {
        id: 1_u128.into(),
        name: "private test profile".into(),
        kind: ProfileKind::Named,
    });
    let enrollment = AgentWorkAccountEnrollment::new(
        profile,
        SemanticOrigin::parse("https://app.notion.com").unwrap(),
        enrolled_id(),
    );
    let published = Arc::new(Mutex::new(Err(AgentWorkAccountFailure::Missing)));
    let source = enrollment.with_collector(Box::new(Collector(published.clone())));
    let identity = ContextIdentity::new(
        ContextId::generate(),
        ContextRunId::generate(),
        profile.profile(),
        ContextKind::Owned,
    );
    let mut registry = ContextRegistry::new();
    registry
        .reserve(
            identity,
            ContextCapabilities::try_new(ContextKind::Owned, &[]).unwrap(),
        )
        .unwrap();
    let construction = registry
        .begin_context(identity.id(), ContextOperationId::new(1).unwrap())
        .unwrap();
    registry
        .settle_construction(identity.id(), construction, ContextSettlement::Applied)
        .unwrap();
    (source, published, registry, construction.context())
}

fn binding(context: ContextJoin, millis: u64) -> AgentContextAccountBinding {
    AgentContextAccountBinding::new(
        AgentAccountAttestationId::generate(),
        context,
        AgentAccountScope::Authenticated(enrolled_id()),
        AgentPolicyInstant::from_millis(millis),
    )
}

fn enrolled_id() -> AgentAccountId {
    AgentAccountId::parse("00000000000000000000000033").unwrap()
}

fn publish(published: &Published, sample: AgentContextAccountBinding) {
    *published.lock().unwrap() = Ok((
        SemanticOrigin::parse("https://app.notion.com").unwrap(),
        sample,
    ));
}

#[test]
fn enrollment_does_not_mint_evidence_and_polling_preserves_the_original_sample() {
    let (source, published, _, context) = fixture();
    assert_eq!(
        source.sample_account(context),
        Err(AgentWorkAccountFailure::Missing)
    );
    let original = binding(context, 42);
    publish(&published, original);
    assert_eq!(source.sample(context), Ok(original));
    assert_eq!(source.sample(context), Ok(original));
    assert_eq!(
        format!("{source:?}"),
        "AgentWorkEnrolledAccount([owned, redacted])"
    );
    assert!(!format!("{:?}", source.enrollment()).contains("notion"));
}

#[test]
fn successor_navigation_requires_independently_collected_successor_evidence() {
    let (source, published, mut registry, context) = fixture();
    let first = binding(context, 42);
    publish(&published, first);
    assert_eq!(source.sample(context), Ok(first));
    let successor = registry
        .observe_navigation_replacement(context.identity().id(), context)
        .unwrap();
    *published.lock().unwrap() = Err(AgentWorkAccountFailure::Missing);
    assert_eq!(
        source.sample_account(successor),
        Err(AgentWorkAccountFailure::Missing)
    );
    let second = binding(successor, 43);
    publish(&published, second);
    assert_eq!(source.sample(successor), Ok(second));
    assert_eq!(second.account(), first.account());
    assert_ne!(second.attestation(), first.attestation());
}

#[test]
fn logout_account_switch_and_service_switch_permanently_refuse_the_source() {
    for candidate in 0..4 {
        let (source, published, _, context) = fixture();
        let original = binding(context, 42);
        publish(&published, original);
        assert_eq!(source.sample(context), Ok(original));
        match candidate {
            0 | 1 => publish(
                &published,
                AgentContextAccountBinding::new(
                    AgentAccountAttestationId::generate(),
                    context,
                    if candidate == 0 {
                        AgentAccountScope::Anonymous
                    } else {
                        AgentAccountScope::Authenticated(AgentAccountId::generate())
                    },
                    AgentPolicyInstant::from_millis(43),
                ),
            ),
            2 => {
                *published.lock().unwrap() = Ok((
                    SemanticOrigin::parse("https://other.test").unwrap(),
                    original,
                ))
            }
            _ => *published.lock().unwrap() = Err(AgentWorkAccountFailure::IdentityChanged),
        }
        assert_eq!(
            source.sample_account(context),
            Err(AgentWorkAccountFailure::IdentityChanged)
        );
        publish(&published, original);
        assert_eq!(
            source.sample_account(context),
            Err(AgentWorkAccountFailure::IdentityChanged)
        );
    }
}

#[test]
fn cached_pre_navigation_facts_are_not_rebound_to_a_successor() {
    let (source, published, mut registry, context) = fixture();
    publish(&published, binding(context, 42));
    source.sample(context).unwrap();
    let successor = registry
        .observe_navigation_replacement(context.identity().id(), context)
        .unwrap();
    assert_eq!(
        source.sample_account(successor),
        Err(AgentWorkAccountFailure::InvalidEvidence)
    );
}

#[test]
fn restamping_regression_and_cross_run_substitution_refuse_evidence() {
    for candidate in 0..3 {
        let (source, published, _, context) = fixture();
        let original = binding(context, 42);
        publish(&published, original);
        source.sample(context).unwrap();
        let changed = match candidate {
            0 => AgentContextAccountBinding::new(
                original.attestation(),
                context,
                original.account(),
                AgentPolicyInstant::from_millis(43),
            ),
            1 => binding(context, 41),
            _ => binding(fixture().3, 43),
        };
        publish(&published, changed);
        assert_eq!(
            source.sample_account(context),
            Err(AgentWorkAccountFailure::InvalidEvidence)
        );
        publish(&published, original);
        assert!(source.sample(context).is_err());
    }
}

#[test]
fn source_cannot_be_reused_by_another_run_even_in_the_same_profile() {
    let (source, published, _, context) = fixture();
    publish(&published, binding(context, 42));
    source.sample(context).unwrap();
    let other = fixture().3;
    publish(&published, binding(other, 43));
    assert_eq!(
        source.sample_account(other),
        Err(AgentWorkAccountFailure::InvalidEvidence)
    );
}

#[test]
fn ordinary_discovery_uses_the_enrolled_source_without_minting_freshness() {
    use zephium_agent_controller::{AgentWorkDiscoveryTask, AgentWorkTask};
    let (source, published, mut registry, context) = fixture();
    let task = AgentWorkDiscoveryTask::try_new_with_account_source(
        context.identity(),
        AgentNavigationDiscovery::try_new(
            ContextNavigationTarget::parse("https://app.notion.com/project").unwrap(),
            "/".into(),
            2,
        )
        .unwrap(),
        vec![SemanticExtractionFieldSchema::try_text("answer".into(), true, 64).unwrap()],
        AgentAccountScope::Authenticated(source.enrollment().account()),
        Box::new(source),
    )
    .unwrap();
    assert!(task
        .attest_account(context, AgentPolicyInstant::from_millis(42))
        .is_err());
    let original = binding(context, 42);
    publish(&published, original);
    // Policy, not the adapter or model clock, decides whether this age is usable.
    assert_eq!(
        task.attest_account(context, AgentPolicyInstant::from_millis(60_000)),
        Ok(original)
    );
    let successor = registry
        .observe_navigation_replacement(context.identity().id(), context)
        .unwrap();
    let next = binding(successor, 60_001);
    publish(&published, next);
    assert_eq!(
        task.attest_account(successor, AgentPolicyInstant::from_millis(60_001)),
        Ok(next)
    );
    *published.lock().unwrap() = Err(AgentWorkAccountFailure::IdentityChanged);
    assert!(task
        .attest_account(successor, AgentPolicyInstant::from_millis(60_002))
        .is_err());
    publish(&published, binding(successor, 60_003));
    assert!(task
        .attest_account(successor, AgentPolicyInstant::from_millis(60_003))
        .is_err());
}
