use proptest::prelude::*;
use zephium_blocker::{CompileTarget, Compiler, FilterSource, SourceFormat, SourceId};
#[cfg(feature = "runtime")]
use zephium_blocker::{NetworkRequest, RequestMethod, ResourceType};

fn source(id: &str, contents: String) -> FilterSource {
    FilterSource::new(
        SourceId::new(id).expect("fixed synthetic source id is valid"),
        SourceFormat::Standard,
        contents,
    )
}

fn synthetic_rule(index: usize, variant: u8) -> String {
    let resource = match variant % 7 {
        0 => "script",
        1 => "image",
        2 => "stylesheet",
        3 => "font",
        4 => "media",
        5 => "xmlhttprequest",
        _ => "ping",
    };
    match variant % 5 {
        0 => format!("||asset-{index}.ads.invalid^${resource}"),
        1 => format!("||metric-{index}.ads.invalid^${resource},third-party"),
        2 => format!(
            "@@||allow-{index}.ads.invalid^${resource},domain=site-{}.invalid",
            index % 31
        ),
        3 => format!("|https://exact-{index}.ads.invalid/path/${resource}"),
        _ => format!("||generic-{index}.ads.invalid^"),
    }
}

#[derive(Debug, Eq, PartialEq)]
struct WebKitIdentity {
    digest: String,
    rules: usize,
    bytes: usize,
}

#[derive(Debug, Eq, PartialEq)]
struct CompiledIdentity {
    digest: String,
    candidates: usize,
    accepted: usize,
    rejected: usize,
    webkit: Option<WebKitIdentity>,
}

fn compiled_identity(
    target: CompileTarget,
    first: String,
    second: String,
    reverse: bool,
) -> Result<CompiledIdentity, String> {
    let mut sources = vec![source("synthetic-a", first), source("synthetic-b", second)];
    if reverse {
        sources.reverse();
    }
    Compiler::default()
        .compile(target, sources)
        .map(|rules| {
            let report = rules.report();
            CompiledIdentity {
                digest: rules.digest().to_string(),
                candidates: report.candidate_rules(),
                accepted: report.accepted_rules(),
                rejected: report.rejected_rules(),
                webkit: rules.webkit().map(|webkit| WebKitIdentity {
                    digest: webkit.digest().to_string(),
                    rules: webkit.rule_count(),
                    bytes: webkit.json().len(),
                }),
            }
        })
        .map_err(|error| format!("{error:?}\n{error}"))
}

fn line_strategy() -> impl Strategy<Value = String> {
    (
        0usize..4096,
        0u8..16,
        prop::collection::vec(
            prop_oneof![
                Just('a'),
                Just('Z'),
                Just('0'),
                Just('^'),
                Just('$'),
                Just('|'),
                Just('@'),
                Just(','),
                Just('='),
                Just('/'),
                Just('\\'),
                Just(' '),
                Just('\t'),
                Just('\u{0000}'),
                Just('é'),
            ],
            0..48,
        ),
    )
        .prop_map(|(index, variant, mutation)| {
            let mutation = mutation.into_iter().collect::<String>();
            match variant % 6 {
                0 => synthetic_rule(index, variant),
                1 => format!("||fuzz-{index}.invalid^{mutation}"),
                2 => format!("@@||fuzz-{index}.invalid^$domain=site.invalid{mutation}"),
                3 => format!("! Zephium synthetic comment {mutation}"),
                4 => format!("0.0.0.0 host-{index}.invalid {mutation}"),
                _ => mutation,
            }
        })
}

fn generated_policy(variants: &[u8]) -> String {
    let mut policy = vec!["||always-blocked.ads.invalid^".to_owned()];
    policy.extend(
        variants
            .iter()
            .enumerate()
            .map(|(index, variant)| synthetic_rule(index, *variant)),
    );
    policy.join("\n")
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        max_shrink_iters: 2_048,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn authored_invalid_domain_inputs_are_deterministic(
        lines in prop::collection::vec(line_strategy(), 1..256),
        split in 0usize..256,
    ) {
        let split = split.min(lines.len());
        let first = lines[..split].join("\n");
        let second = lines[split..].join("\n");
        for target in [CompileTarget::Runtime, CompileTarget::WebKit] {
            let canonical = compiled_identity(
                target,
                first.clone(),
                second.clone(),
                false,
            );
            let reordered = compiled_identity(
                target,
                first.clone(),
                second.clone(),
                true,
            );
            prop_assert_eq!(canonical, reordered);
        }
    }
}

#[cfg(feature = "runtime")]
proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        max_shrink_iters: 2_048,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn generated_valid_policies_have_stable_runtime_decisions(
        variants in prop::collection::vec(0u8..32, 1..192),
    ) {
        let rules = generated_policy(&variants);
        let runtime = Compiler::default()
            .compile(
                CompileTarget::Runtime,
                vec![source("synthetic-runtime", rules.clone())],
            )
            .expect("the generated policy contains blocking rules");
        let runtime_again = Compiler::default()
            .compile(
                CompileTarget::Runtime,
                vec![source("synthetic-runtime", rules.clone())],
            )
            .expect("the same generated policy remains compilable");
        prop_assert_eq!(runtime.digest(), runtime_again.digest());

        for index in 0..variants.len().min(64) {
            let url = format!("https://asset-{index}.ads.invalid/script.js");
            let source_url = format!("https://site-{}.invalid/", index % 31);
            let request = NetworkRequest::new(
                &url,
                &source_url,
                ResourceType::Script,
                RequestMethod::Get,
            );
            prop_assert_eq!(runtime.evaluate(request), runtime_again.evaluate(request));
        }
    }
}

#[cfg(feature = "webkit")]
proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        max_shrink_iters: 2_048,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn generated_valid_policies_have_stable_webkit_artifacts(
        variants in prop::collection::vec(0u8..32, 1..192),
    ) {
        let rules = generated_policy(&variants);
        let webkit = Compiler::default()
            .compile(
                CompileTarget::WebKit,
                vec![source("synthetic-webkit", rules.clone())],
            )
            .expect("the generated policy has WebKit-representable blocking rules");
        let webkit_again = Compiler::default()
            .compile(
                CompileTarget::WebKit,
                vec![source("synthetic-webkit", rules)],
            )
            .expect("the same generated policy remains WebKit-compilable");
        prop_assert_eq!(webkit.digest(), webkit_again.digest());
        let first = webkit.webkit().expect("WebKit target carries JSON");
        let second = webkit_again.webkit().expect("WebKit target carries JSON");
        prop_assert_eq!(first.digest(), second.digest());
        prop_assert_eq!(first.rule_count(), second.rule_count());
        prop_assert_eq!(first.json(), second.json());
        let decoded: serde_json::Value =
            serde_json::from_str(first.json()).expect("compiler emits valid canonical JSON");
        let native = decoded.as_array().expect("WebKit artifact is an array");
        prop_assert_eq!(native.len(), first.rule_count());
    }
}

#[test]
fn synthetic_quality_inputs_are_reserved_and_self_authored() {
    for index in 0..256 {
        let rule = synthetic_rule(index, index as u8);
        assert!(
            rule.contains(".invalid"),
            "synthetic rule escaped the reserved domain namespace"
        );
    }
}
