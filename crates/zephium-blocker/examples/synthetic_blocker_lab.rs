use std::env;
use std::process::ExitCode;
use std::time::Instant;

use zephium_blocker::{
    CompileTarget, Compiler, FilterSource, MatchError, NetworkAction, NetworkRequest,
    RequestMethod, ResourceType, SourceFormat, SourceId,
};

const DEFAULT_RULES: usize = 50_000;
const DEFAULT_REQUESTS: usize = 100_000;
const MAX_LAB_RULES: usize = 250_000;
const MAX_LAB_REQUESTS: usize = 10_000_000;

#[derive(Clone, Copy)]
enum SelectedTarget {
    All,
    Runtime,
    WebKit,
}

struct Config {
    rules: usize,
    requests: usize,
    target: SelectedTarget,
}

#[derive(Clone, Copy)]
enum RuntimeAttribution {
    Exact,
    SourceIndependent,
}

impl RuntimeAttribution {
    const fn label(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::SourceIndependent => "source_independent",
        }
    }
}

struct RuntimeMeasurements {
    blocked: usize,
    allowed: usize,
    candidate_budget_exhausted: usize,
    other_errors: usize,
    samples: Vec<u128>,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("synthetic blocker lab: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let Some(config) = parse_args()? else {
        return Ok(());
    };
    let policy = synthetic_policy(config.rules);
    match config.target {
        SelectedTarget::All => {
            measure_runtime(&policy, config.rules, config.requests)?;
            measure_webkit(&policy, config.rules)?;
        }
        SelectedTarget::Runtime => measure_runtime(&policy, config.rules, config.requests)?,
        SelectedTarget::WebKit => measure_webkit(&policy, config.rules)?,
    }
    Ok(())
}

fn parse_args() -> Result<Option<Config>, String> {
    let mut config = Config {
        rules: DEFAULT_RULES,
        requests: DEFAULT_REQUESTS,
        target: SelectedTarget::All,
    };
    let mut arguments = env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--rules" => {
                config.rules = parse_count(
                    "--rules",
                    arguments
                        .next()
                        .ok_or_else(|| "--rules requires a value".to_owned())?,
                    MAX_LAB_RULES,
                )?;
            }
            "--requests" => {
                config.requests = parse_count(
                    "--requests",
                    arguments
                        .next()
                        .ok_or_else(|| "--requests requires a value".to_owned())?,
                    MAX_LAB_REQUESTS,
                )?;
            }
            "--target" => {
                config.target = match arguments
                    .next()
                    .ok_or_else(|| "--target requires a value".to_owned())?
                    .as_str()
                {
                    "all" => SelectedTarget::All,
                    "runtime" => SelectedTarget::Runtime,
                    "webkit" => SelectedTarget::WebKit,
                    _ => return Err("--target must be all, runtime, or webkit".to_owned()),
                };
            }
            "--help" | "-h" => {
                println!(
                    "Usage: cargo run --release -p zephium-blocker --example \
                     synthetic_blocker_lab -- [--rules 1..={MAX_LAB_RULES}] \
                     [--requests 1..={MAX_LAB_REQUESTS}] \
                     [--target all|runtime|webkit]"
                );
                return Ok(None);
            }
            _ => return Err(format!("unknown argument `{argument}`")),
        }
    }
    Ok(Some(config))
}

fn parse_count(name: &str, value: String, maximum: usize) -> Result<usize, String> {
    let value = value
        .parse::<usize>()
        .map_err(|_| format!("{name} must be a decimal integer"))?;
    if !(1..=maximum).contains(&value) {
        return Err(format!("{name} must be in 1..={maximum}"));
    }
    Ok(value)
}

fn synthetic_policy(count: usize) -> String {
    let mut output = String::with_capacity(count.saturating_mul(48));
    for index in 0..count {
        if index != 0 {
            output.push('\n');
        }
        if index % 16 == 15 {
            output.push_str(&format!(
                "@@||allow-{index}.ads.invalid^$image,domain=site-{}.invalid",
                index % 97
            ));
            continue;
        }
        let resource = match index % 7 {
            0 => "script",
            1 => "image",
            2 => "stylesheet",
            3 => "font",
            4 => "media",
            5 => "xmlhttprequest",
            _ => "ping",
        };
        output.push_str(&format!("||block-{index}.ads.invalid^${resource}"));
    }
    output
}

fn source(policy: &str) -> FilterSource {
    FilterSource::new(
        SourceId::new("zephium-synthetic-lab").expect("fixed source id is valid"),
        SourceFormat::Standard,
        policy.to_owned(),
    )
}

fn measure_runtime(policy: &str, rule_count: usize, request_count: usize) -> Result<(), String> {
    let started = Instant::now();
    let compiled = Compiler::default()
        .compile(CompileTarget::Runtime, vec![source(policy)])
        .map_err(|error| format!("runtime compilation failed: {error}"))?;
    let compile_ns = started.elapsed().as_nanos();
    let request_set = synthetic_requests(rule_count.clamp(1, 4_096));
    for attribution in [
        RuntimeAttribution::Exact,
        RuntimeAttribution::SourceIndependent,
    ] {
        let measurements =
            measure_runtime_requests(&compiled, &request_set, request_count, attribution)?;
        print_runtime_measurement(
            &compiled,
            policy,
            rule_count,
            request_count,
            compile_ns,
            attribution,
            &measurements,
        );
    }
    Ok(())
}

fn measure_runtime_requests(
    compiled: &zephium_blocker::CompiledRules,
    request_set: &[(String, String, ResourceType)],
    request_count: usize,
    attribution: RuntimeAttribution,
) -> Result<RuntimeMeasurements, String> {
    let mut samples = Vec::with_capacity(request_count);
    let mut blocked = 0usize;
    let mut allowed = 0usize;
    let mut candidate_budget_exhausted = 0usize;
    let mut other_errors = 0usize;
    for index in 0..request_count {
        let (url, source_url, resource) = &request_set[index % request_set.len()];
        let started = Instant::now();
        let result = match attribution {
            RuntimeAttribution::Exact => compiled.evaluate(NetworkRequest::new(
                url,
                source_url,
                *resource,
                RequestMethod::Get,
            )),
            RuntimeAttribution::SourceIndependent => compiled.evaluate_source_independent(
                NetworkRequest::source_independent(url, *resource, RequestMethod::Get),
            ),
        };
        samples.push(started.elapsed().as_nanos());
        match result {
            Ok(decision) if decision.action() == NetworkAction::Block => blocked += 1,
            Ok(_) => allowed += 1,
            Err(MatchError::CandidateBudgetExhausted) => candidate_budget_exhausted += 1,
            Err(_) => other_errors += 1,
        }
    }
    let errors = candidate_budget_exhausted.saturating_add(other_errors);
    if blocked == 0 || allowed.saturating_add(errors) == 0 || other_errors != 0 {
        return Err(format!(
            "runtime {} semantic probe was not representative: blocked={blocked}, \
             allowed={allowed}, candidate_budget_exhausted={candidate_budget_exhausted}, \
             other_errors={other_errors}",
            attribution.label(),
        ));
    }
    samples.sort_unstable();
    Ok(RuntimeMeasurements {
        blocked,
        allowed,
        candidate_budget_exhausted,
        other_errors,
        samples,
    })
}

fn print_runtime_measurement(
    compiled: &zephium_blocker::CompiledRules,
    policy: &str,
    rule_count: usize,
    request_count: usize,
    compile_ns: u128,
    attribution: RuntimeAttribution,
    measurements: &RuntimeMeasurements,
) {
    let report = compiled.report();
    let errors = measurements
        .candidate_budget_exhausted
        .saturating_add(measurements.other_errors);
    println!(
        "{{\"target\":\"runtime\",\"attribution\":\"{}\",\
         \"synthetic_namespace\":\".invalid\",\
         \"rules_requested\":{rule_count},\"source_bytes\":{},\
         \"candidate_rules\":{},\"accepted_rules\":{},\"rejected_rules\":{},\
         \"policy_digest\":\"{}\",\"compile_ns\":{compile_ns},\
         \"requests\":{request_count},\"blocked\":{},\"allowed\":{},\
         \"errors\":{errors},\
         \"candidate_budget_exhausted\":{},\
         \"other_errors\":{},\"match_ns_p50\":{},\"match_ns_p95\":{},\
         \"match_ns_p99\":{},\"match_ns_max\":{}}}",
        attribution.label(),
        policy.len(),
        report.candidate_rules(),
        report.accepted_rules(),
        report.rejected_rules(),
        compiled.digest(),
        measurements.blocked,
        measurements.allowed,
        measurements.candidate_budget_exhausted,
        measurements.other_errors,
        percentile(&measurements.samples, 50),
        percentile(&measurements.samples, 95),
        percentile(&measurements.samples, 99),
        measurements.samples.last().copied().unwrap_or(0),
    );
}

fn measure_webkit(policy: &str, rule_count: usize) -> Result<(), String> {
    let started = Instant::now();
    let compiled = Compiler::default()
        .compile(CompileTarget::WebKit, vec![source(policy)])
        .map_err(|error| format!("WebKit compilation failed: {error}"))?;
    let compile_ns = started.elapsed().as_nanos();
    let webkit = compiled
        .webkit()
        .ok_or_else(|| "WebKit compilation returned no declarative artifact".to_owned())?;
    let report = compiled.report();
    println!(
        "{{\"target\":\"webkit\",\"synthetic_namespace\":\".invalid\",\
         \"rules_requested\":{rule_count},\"source_bytes\":{},\
         \"candidate_rules\":{},\"accepted_rules\":{},\"rejected_rules\":{},\
         \"policy_digest\":\"{}\",\"artifact_digest\":\"{}\",\
         \"compile_ns\":{compile_ns},\"native_rules\":{},\"artifact_bytes\":{}}}",
        policy.len(),
        report.candidate_rules(),
        report.accepted_rules(),
        report.rejected_rules(),
        compiled.digest(),
        webkit.digest(),
        webkit.rule_count(),
        webkit.json().len(),
    );
    Ok(())
}

fn synthetic_requests(count: usize) -> Vec<(String, String, ResourceType)> {
    let mut requests = Vec::with_capacity(count.saturating_mul(2));
    for index in 0..count {
        let resource = match index % 7 {
            0 => ResourceType::Script,
            1 => ResourceType::Image,
            2 => ResourceType::Stylesheet,
            3 => ResourceType::Font,
            4 => ResourceType::Media,
            5 => ResourceType::XmlHttpRequest,
            _ => ResourceType::Beacon,
        };
        requests.push((
            format!("https://block-{index}.ads.invalid/resource"),
            format!("https://site-{}.invalid/", index % 97),
            resource,
        ));
        requests.push((
            format!("https://clean-{index}.content.invalid/resource"),
            format!("https://site-{}.invalid/", index % 97),
            resource,
        ));
    }
    requests
}

fn percentile(samples: &[u128], percentile: usize) -> u128 {
    if samples.is_empty() {
        return 0;
    }
    let index = (samples.len() - 1)
        .saturating_mul(percentile)
        .checked_div(100)
        .unwrap_or(0);
    samples[index]
}
