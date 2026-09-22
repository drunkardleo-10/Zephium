use std::{
    io::Write as _,
    time::{Duration, Instant},
};

use zephium_agentic::{
    load_macos_development_openai_credential, load_macos_development_typesafe_credential,
    AgentProviderTransport, AgentProviderTransportConfig, DecisionCallOutput, JevDecisionClient,
    OpenAiDecisionClient, WorkPlanningConfig,
};
use zephium_decision::{
    evals::{fixtures, measure, measure_question, EvalFixture},
    AnswerValue, DecisionFallback, DecisionPurpose, FallbackReason, QuestionKind, ResolvedDecision,
};

use super::{ProbeFailure, ProbeModel};

pub(super) fn run() -> Result<(), ProbeFailure> {
    run_selected(None, ProbeModel::Luna)
}

pub(super) fn run_terra() -> Result<(), ProbeFailure> {
    run_selected(None, ProbeModel::Terra)
}

pub(super) fn run_case(case: &std::ffi::OsStr, model: ProbeModel) -> Result<(), ProbeFailure> {
    let index = match case.to_str() {
        Some("lego") => 0,
        Some("yc") => 1,
        Some("yc-json") => 2,
        Some("airbnb") => 3,
        Some("cloudflare") => 4,
        _ => return Err(ProbeFailure::Authority),
    };
    run_selected(Some(index), model)
}

fn run_selected(only: Option<usize>, model: ProbeModel) -> Result<(), ProbeFailure> {
    let _ = writeln!(
        std::io::stderr(),
        "decision_eval phase=keychain backend=jev"
    );
    let started = Instant::now();
    let jev_key =
        load_macos_development_typesafe_credential().map_err(|_| ProbeFailure::Keychain)?;
    let _ = writeln!(
        std::io::stderr(),
        "decision_eval phase=keychain_complete backend=jev elapsed_ms={}",
        started.elapsed().as_millis()
    );
    let _ = writeln!(
        std::io::stderr(),
        "decision_eval phase=keychain backend=emulation"
    );
    let started = Instant::now();
    let openai_key =
        load_macos_development_openai_credential().map_err(|_| ProbeFailure::Keychain)?;
    let _ = writeln!(
        std::io::stderr(),
        "decision_eval phase=keychain_complete backend=emulation elapsed_ms={}",
        started.elapsed().as_millis()
    );
    let transport = AgentProviderTransport::try_new(AgentProviderTransportConfig::STANDARD)
        .map_err(|_| ProbeFailure::Authority)?;
    let jev = JevDecisionClient::direct(transport.clone(), jev_key)
        .map_err(|_| ProbeFailure::Authority)?;
    let (config, model_revision) = match model {
        ProbeModel::Luna => (
            zephium_agent_model_catalog::try_luna_provider_exact_call_config(8192)
                .map_err(|_| ProbeFailure::Authority)?,
            zephium_agent_model_catalog::LUNA_MODEL_REVISION,
        ),
        ProbeModel::Terra => (
            zephium_agent_model_catalog::try_terra_provider_exact_call_config(4096)
                .map_err(|_| ProbeFailure::Authority)?,
            zephium_agent_model_catalog::TERRA_MODEL_REVISION,
        ),
    };
    let emulation = OpenAiDecisionClient::try_new(
        transport.clone(),
        openai_key,
        WorkPlanningConfig::try_new(config, 32_768, 100_000)
            .map_err(|_| ProbeFailure::Authority)?,
    )
    .map_err(|_| ProbeFailure::Authority)?;
    let fixtures = fixtures().map_err(|_| ProbeFailure::Verification)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| ProbeFailure::Runtime)?;
    runtime.block_on(async {
        let mut valid = true;
        for (index, fixture) in fixtures.iter().enumerate() {
            if only.is_some_and(|selected| selected != index) {
                continue;
            }
            for repetition in 0..3 {
                let output = jev
                    .evaluate_public_fixture(index)
                    .await
                    .map_err(|_| ProbeFailure::Verification)?;
                valid &= report(index, repetition, fixture, output, model_revision)?;
                let output = emulation
                    .evaluate_public_fixture(index)
                    .await
                    .map_err(|_| ProbeFailure::Verification)?;
                valid &= report(index, repetition, fixture, output, model_revision)?;
            }
        }
        let _proof = transport
            .seal_and_prove_shutdown_until(Instant::now() + Duration::from_secs(5))
            .map_err(|_| ProbeFailure::Authority)?
            .await
            .map_err(|_| ProbeFailure::Authority)?;
        if valid {
            Ok(())
        } else {
            Err(ProbeFailure::Verification)
        }
    })
}

fn report(
    index: usize,
    repetition: usize,
    fixture: &EvalFixture,
    output: DecisionCallOutput,
    emulation_model: &'static str,
) -> Result<bool, ProbeFailure> {
    let d = output.diagnostic;
    let model = match d.backend {
        zephium_agentic::DecisionBackendKind::Emulation => emulation_model,
        _ => zephium_decision::JEV_MODEL,
    };
    writeln!(std::io::stderr(), "decision_eval phase=call fixture={index} repetition={repetition} backend={:?} model={model} questions={} state_bytes={} input_tokens={:?} output_tokens={} elapsed_ms={} status={:?} attempts={:?} reason={:?} envelope={:?} cost_micro_usd={}", d.backend,d.question_count,d.state_bytes,d.input_tokens,output.charged_usage.output_tokens,d.elapsed_millis,d.http_status,d.attempts,d.failure,d.envelope_failure,output.cost_micro_usd).map_err(|_| ProbeFailure::Output)?;
    let completed = output.response.is_ok();
    let response = output
        .response
        .unwrap_or_else(|_| zephium_decision::DecisionResponse {
            answers: Default::default(),
            usage: Default::default(),
        });
    for (kind, metric) in measure(fixture, &response).into_iter().enumerate() {
        writeln!(std::io::stderr(), "decision_eval phase=accuracy fixture={index} repetition={repetition} backend={:?} kind={kind} total={} valid={} correct={} accuracy={:.6} label_brier={:.6} confidence_sum={:.6} calibration_error={:.6}", d.backend,metric.total,metric.valid,metric.correct,metric.accuracy(),metric.brier(),metric.confidence_sum,metric.calibration_error()).map_err(|_| ProbeFailure::Output)?;
        for (bucket, bin) in metric
            .calibration
            .iter()
            .enumerate()
            .filter(|(_, bin)| bin.count != 0)
        {
            writeln!(std::io::stderr(), "decision_eval phase=calibration fixture={index} repetition={repetition} backend={:?} kind={kind} bucket={bucket} count={} correct={} probability_sum={:.6}",d.backend,bin.count,bin.correct,bin.probability_sum).map_err(|_| ProbeFailure::Output)?;
        }
    }
    let mut scored = std::collections::BTreeMap::new();
    for (question, (key, answer)) in response.answers.iter().enumerate() {
        if let Some(metric) = measure_question(fixture, &response, key)
            .into_iter()
            .find(|metric| metric.total != 0)
        {
            scored.insert(key.clone(), metric.correct == 1);
            writeln!(std::io::stderr(), "decision_eval phase=scored fixture={index} repetition={repetition} question={question} backend={:?} valid={} correct={}",d.backend,metric.valid,metric.correct).map_err(|_| ProbeFailure::Output)?;
        }
        if let Ok(answer) = answer {
            let confidence = match answer.value() {
                AnswerValue::Noul { noul } => noul.max(1.0 - noul),
                AnswerValue::Choice { confidence, .. } | AnswerValue::Score { confidence, .. } => {
                    *confidence
                }
            };
            let bucket = if confidence < 0.5 {
                0
            } else if confidence < 0.8 {
                1
            } else if confidence < 0.95 {
                2
            } else {
                3
            };
            writeln!(std::io::stderr(), "decision_eval phase=answer fixture={index} question={question} backend={:?} kind={:?} confidence_bucket={bucket}", d.backend,answer.kind()).map_err(|_| ProbeFailure::Output)?;
        }
    }
    let valid = completed && response.answers.values().all(Result::is_ok);
    let purposes = fixture
        .request
        .questions()
        .iter()
        .map(|(key, question)| {
            let purpose = match key.as_str() {
                "challenge" => DecisionPurpose::Challenge,
                "relevant" | "more_below" => DecisionPurpose::Relevance,
                "done" => DecisionPurpose::Completion,
                "picture" | "tower_bridge_picture" => DecisionPurpose::Picture,
                "wall" => DecisionPurpose::Wall,
                "operation" | "click_target" | "type_target" | "scroll_target"
                | "dismiss_target" | "tower_bridge_link" => DecisionPurpose::Action,
                _ => match question.kind() {
                    QuestionKind::Noul => DecisionPurpose::Relevance,
                    QuestionKind::Choice => DecisionPurpose::Locate,
                    QuestionKind::Score => DecisionPurpose::OrderedScore,
                },
            };
            (key.clone(), purpose)
        })
        .collect();
    let primary = if completed {
        Ok(response)
    } else {
        Err(match d.failure {
            Some(zephium_agentic::DecisionCallFailure::RateLimited) => FallbackReason::RateLimited,
            Some(zephium_agentic::DecisionCallFailure::InvalidAnswer) => {
                FallbackReason::InvalidAnswer
            }
            _ => FallbackReason::Unavailable,
        })
    };
    let routing = DecisionFallback::assess(&fixture.request, purposes, primary)
        .map_err(|_| ProbeFailure::Verification)?;
    writeln!(std::io::stderr(), "decision_eval phase=routing fixture={index} repetition={repetition} backend={:?} policy={} fallback_questions={} low_confidence={}",
        d.backend, zephium_decision::CONFIDENCE_POLICY_REVISION, routing.reasons().len(),
        routing.reasons().values().filter(|reason| **reason == FallbackReason::LowConfidence).count())
        .map_err(|_| ProbeFailure::Output)?;
    let mut routed = routing.finish(None);
    let mut accepted = 0;
    let mut correct = 0;
    for (key, is_correct) in scored {
        if matches!(
            routed.take(&key),
            Some(ResolvedDecision::Answer { .. } | ResolvedDecision::Abstained { .. })
        ) {
            accepted += 1;
            correct += u32::from(is_correct);
        }
    }
    writeln!(std::io::stderr(), "decision_eval phase=accepted fixture={index} repetition={repetition} backend={:?} total={accepted} correct={correct}",d.backend).map_err(|_| ProbeFailure::Output)?;
    Ok(valid)
}
