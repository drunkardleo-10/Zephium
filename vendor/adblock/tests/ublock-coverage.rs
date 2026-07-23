use adblock::Engine;
use adblock::blocker::{NetworkMatcherPreparationLimits, PreparedNetworkMatcherError};
use adblock::request::Request;
#[cfg(not(feature = "embedded-domain-resolver"))]
use adblock::url_parser::{ResolvesDomain, set_domain_resolver};

#[cfg(not(feature = "embedded-domain-resolver"))]
struct TestDomainResolver;

#[cfg(not(feature = "embedded-domain-resolver"))]
impl ResolvesDomain for TestDomainResolver {
    fn get_host_domain(&self, host: &str) -> (usize, usize) {
        let Some(last_dot) = host.rfind('.') else {
            return (0, host.len());
        };
        let start = host[..last_dot]
            .rfind('.')
            .map_or(0, |second_last_dot| second_last_dot + 1);
        (start, host.len())
    }
}

fn initialize_domain_resolver() {
    #[cfg(not(feature = "embedded-domain-resolver"))]
    {
        let _ = set_domain_resolver(Box::new(TestDomainResolver));
    }
}

fn prepared_limits() -> NetworkMatcherPreparationLimits {
    NetworkMatcherPreparationLimits {
        max_regexes: 1_024,
        max_pattern_bytes: 8 * 1024 * 1024,
        max_patterns_per_regex: 1_024,
        max_pattern_bytes_per_regex: 256 * 1024,
        max_regex_size_bytes: 1024 * 1024,
        max_regex_dfa_size_bytes: 1024 * 1024,
        max_filter_checks_per_request: 4_096,
    }
}

fn restored_prepared(engine: &Engine) -> Engine {
    let mut restored = Engine::default();
    restored
        .deserialize(&engine.serialize())
        .expect("the current fork artifact must deserialize");
    restored
        .prepare_and_freeze_network_matcher(prepared_limits())
        .expect("the authored corpus must fit the native matcher budgets");
    restored
}

#[test]
fn authored_scoped_exception_vectors_remain_exact() {
    initialize_domain_resolver();
    let engine = Engine::new_with_list_text(concat!(
        "||metrics.corpus.invalid^\n",
        "@@||metrics.corpus.invalid/client.js$script,",
        "domain=publisher.corpus.invalid|shop.corpus.invalid\n",
    ));

    let excepted = Request::new(
        "https://metrics.corpus.invalid/client.js",
        "https://shop.corpus.invalid/",
        "script",
        "get",
    )
    .unwrap();
    let blocked = Request::new(
        "https://metrics.corpus.invalid/client.js",
        "https://other.corpus.invalid/",
        "script",
        "get",
    )
    .unwrap();

    assert!(!engine.check_network_request(&excepted).should_block());
    assert!(engine.check_network_request(&blocked).should_block());

    let restored = restored_prepared(&engine);
    assert!(
        !restored
            .try_check_prepared_network_request(&excepted)
            .expect("the prepared matcher must be available")
            .should_block()
    );
    assert!(
        restored
            .try_check_prepared_network_request(&blocked)
            .expect("the prepared matcher must be available")
            .should_block()
    );
}

#[test]
fn authored_vectors_prove_conservative_unknown_attribution() {
    initialize_domain_resolver();
    let engine = Engine::new_with_list_text(concat!(
        "||miner.corpus.invalid^$third-party\n",
        "||always-block.corpus.invalid^\n",
    ));
    engine
        .prepare_and_freeze_network_matcher(prepared_limits())
        .expect("the authored corpus must fit the native matcher budgets");

    let third_party = Request::new(
        "https://miner.corpus.invalid/client.js",
        "https://publisher.other.invalid/",
        "script",
        "get",
    )
    .unwrap();
    let first_party = Request::new(
        "https://miner.corpus.invalid/client.js",
        "https://miner.corpus.invalid/",
        "script",
        "get",
    )
    .unwrap();
    let unknown_party =
        Request::new_source_independent("https://miner.corpus.invalid/client.js", "script", "get")
            .unwrap();
    let source_independent = Request::new_source_independent(
        "https://always-block.corpus.invalid/client.js",
        "script",
        "get",
    )
    .unwrap();

    assert!(
        engine
            .try_check_prepared_network_request(&third_party)
            .expect("the prepared matcher must be available")
            .should_block()
    );
    assert!(
        !engine
            .try_check_prepared_network_request(&first_party)
            .expect("the prepared matcher must be available")
            .should_block()
    );
    assert!(
        !engine
            .try_check_prepared_source_independent_network_request(&unknown_party)
            .expect("the prepared matcher must be available")
            .should_block(),
        "unknown first/third-party state must not be fabricated"
    );
    assert!(
        engine
            .try_check_prepared_source_independent_network_request(&source_independent)
            .expect("the prepared matcher must be available")
            .should_block(),
        "a rule with no attribution predicate remains enforceable"
    );

    let restored = restored_prepared(&engine);
    for (request, expected) in [(&unknown_party, false), (&source_independent, true)] {
        assert_eq!(
            restored
                .try_check_prepared_source_independent_network_request(request)
                .expect("the restored prepared matcher must be available")
                .should_block(),
            expected
        );
    }
}

#[test]
fn prepared_matcher_reports_candidate_budget_exhaustion() {
    initialize_domain_resolver();
    let engine = Engine::new_with_list_text(concat!(
        "||budget.corpus.invalid^\n",
        "@@||budget.corpus.invalid^$domain=publisher.corpus.invalid\n",
    ));
    let mut limits = prepared_limits();
    limits.max_filter_checks_per_request = 1;
    engine
        .prepare_and_freeze_network_matcher(limits)
        .expect("the authored corpus must fit the preparation budgets");

    let request = Request::new(
        "https://budget.corpus.invalid/client.js",
        "https://publisher.corpus.invalid/",
        "script",
        "get",
    )
    .unwrap();
    assert!(matches!(
        engine.try_check_prepared_network_request(&request),
        Err(PreparedNetworkMatcherError::CandidateBudgetExhausted)
    ));
}
