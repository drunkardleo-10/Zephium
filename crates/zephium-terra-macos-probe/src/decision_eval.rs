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
    evals::{fixtures, measure, EvalFixture},
    AnswerValue,
};

use super::ProbeFailure;

pub(super) fn run() -> Result<(), ProbeFailure> {
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
    let emulation = OpenAiDecisionClient::try_new(
        transport.clone(),
        openai_key,
        WorkPlanningConfig::try_new(
            zephium_agent_model_catalog::try_luna_provider_exact_call_config(8192)
                .map_err(|_| ProbeFailure::Authority)?,
            32_768,
            100_000,
        )
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
            for repetition in 0..3 {
                let output = jev
                    .evaluate_public_fixture(index)
                    .await
                    .map_err(|_| ProbeFailure::Verification)?;
                valid &= report(index, repetition, fixture, output)?;
                let output = emulation
                    .evaluate_public_fixture(index)
                    .await
                    .map_err(|_| ProbeFailure::Verification)?;
                valid &= report(index, repetition, fixture, output)?;
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
) -> Result<bool, ProbeFailure> {
    let d = output.diagnostic;
    let model = match d.backend {
        zephium_agentic::DecisionBackendKind::Emulation => {
            zephium_agent_model_catalog::LUNA_MODEL_REVISION
        }
        _ => zephium_decision::JEV_MODEL,
    };
    writeln!(std::io::stderr(), "decision_eval phase=call fixture={index} repetition={repetition} backend={:?} model={model} questions={} state_bytes={} input_tokens={:?} output_tokens={} elapsed_ms={} status={:?} attempts={} reason={:?} cost_micro_usd={}", d.backend,d.question_count,d.state_bytes,d.input_tokens,output.charged_usage.output_tokens,d.elapsed_millis,d.http_status,d.attempts,d.failure,output.cost_micro_usd).map_err(|_| ProbeFailure::Output)?;
    let Ok(response) = output.response else {
        return Ok(false);
    };
    for (kind, metric) in measure(fixture, &response).into_iter().enumerate() {
        writeln!(std::io::stderr(), "decision_eval phase=accuracy fixture={index} repetition={repetition} backend={:?} kind={kind} total={} valid={} correct={} accuracy={:.6} label_brier={:.6} confidence_sum={:.6}", d.backend,metric.total,metric.valid,metric.correct,metric.accuracy(),metric.brier(),metric.confidence_sum).map_err(|_| ProbeFailure::Output)?;
    }
    for (question, answer) in response.answers.values().enumerate() {
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
    Ok(response.answers.values().all(Result::is_ok))
}
