//! One live tool round-trip per provider whose key is in the Keychain, through
//! the lead transports. Prints closed facts only: never model text.

use std::io::Write as _;
use std::sync::{Arc, Mutex};

use serde_json::json;
use zephium_agentic::lead::{keys, models, LeadClient, LeadTarget};
use zephium_core::work::model::*;

const PROVIDERS: [WorkModelProvider; 5] = [
    WorkModelProvider::OpenAi,
    WorkModelProvider::Anthropic,
    WorkModelProvider::Google,
    WorkModelProvider::DeepSeek,
    WorkModelProvider::OpenRouter,
];

fn say(line: std::fmt::Arguments<'_>) {
    let _ = writeln!(std::io::stdout(), "{line}");
}

/// The provider's lead default; OpenRouter's cheapest listed tool model.
async fn model_for(
    provider: WorkModelProvider,
    target: &LeadTarget,
) -> Result<(String, Option<WorkModelPrice>), WorkModelError> {
    if provider != WorkModelProvider::OpenRouter {
        let model = models::family_default(provider, WorkModelRole::Lead)
            .ok_or(WorkModelError::BadRequest)?;
        return Ok((model.to_owned(), models::builtin_price(provider, model)));
    }
    let secret = keys::load(provider).map_err(|_| WorkModelError::MissingKey)?;
    let listed = zephium_agentic::lead::list_models(target, &secret).await?;
    listed
        .into_iter()
        .filter(|entry| entry.price.as_ref().is_some_and(|price| price.input > 0))
        .min_by_key(|entry| entry.price.as_ref().map(|price| price.input + price.output))
        .map(|entry| (entry.model.model, entry.price))
        .ok_or(WorkModelError::Protocol)
}

async fn round_trip(provider: WorkModelProvider, search: bool) -> Result<(), WorkModelError> {
    let target = LeadTarget::direct(provider)?;
    let (model, price) = model_for(provider, &target).await?;
    let model = model.as_str();
    let client = LeadClient::new(
        target.clone(),
        Arc::new(keys::KeychainCredential::new(provider)),
        price,
    );
    let mut request = WorkModelRequest {
        model: WorkModelRef {
            provider,
            wire: target.wire(),
            model: model.to_owned(),
        },
        system: vec![WorkModelSystemBlock {
            text: "You are a careful assistant. Use tools when asked.".into(),
            cache: true,
        }],
        tools: vec![WorkModelTool {
            name: "add".into(),
            description: "Adds two integers.".into(),
            schema: json!({"type": "object", "properties": {"a": {"type": "integer"}, "b": {"type": "integer"}}, "required": ["a", "b"]}),
        }],
        messages: vec![WorkModelMessage::User(vec![WorkModelPart::Text(
            "Call the add tool twice in parallel: 2+3 and 10+20. Then reply with both sums.".into(),
        )])],
        max_output_tokens: 4_096,
        reasoning: Some(WorkModelReasoning::Medium),
        native_search: false,
        parallel_tools: true,
    };
    let searches = Arc::new(Mutex::new(0_usize));
    let seen = searches.clone();
    let sink = move |event: WorkModelEvent| {
        if let WorkModelEvent::Search { hits, .. } = event {
            if let Ok(mut count) = seen.lock() {
                *count += hits.len();
            }
        }
    };
    let first = client.call(request.clone(), &sink).await?;
    let calls: Vec<WorkModelToolCall> = first
        .assistant
        .iter()
        .filter_map(|part| match part {
            WorkModelPart::ToolCall(call) => Some(call.clone()),
            _ => None,
        })
        .collect();
    let replays = first
        .assistant
        .iter()
        .filter(|part| matches!(part, WorkModelPart::Replay(_)))
        .count();
    say(format_args!(
        "lead-smoke: provider={provider:?} model={model} turn=1 stop={:?} tool_calls={} replays={replays} input={} cached={} output={} cost_micros={:?}",
        first.stop,
        calls.len(),
        first.usage.input_tokens,
        first.usage.cached_input_tokens,
        first.usage.output_tokens,
        first.usage.cost_micros,
    ));
    if calls.is_empty() {
        return Err(WorkModelError::Protocol);
    }
    let results = calls
        .iter()
        .map(|call| {
            let a = call.arguments["a"].as_i64().unwrap_or(0);
            let b = call.arguments["b"].as_i64().unwrap_or(0);
            WorkModelToolResult {
                call: call.id.clone(),
                content: (a + b).to_string(),
                is_error: false,
            }
        })
        .collect();
    request
        .messages
        .push(WorkModelMessage::Assistant(first.assistant));
    request
        .messages
        .push(WorkModelMessage::ToolResults(results));
    if search {
        request
            .messages
            .push(WorkModelMessage::User(vec![WorkModelPart::Text(
                "Now search the web once for the current Rust stable version and name it.".into(),
            )]));
        request.native_search = true;
    }
    let second = client.call(request, &sink).await?;
    let text: usize = second
        .assistant
        .iter()
        .map(|part| match part {
            WorkModelPart::Text(text) => text.len(),
            _ => 0,
        })
        .sum();
    say(format_args!(
        "lead-smoke: provider={provider:?} model={model} turn=2 stop={:?} text_bytes={text} search_hits={} input={} cached={} output={} cost_micros={:?}",
        second.stop,
        searches.lock().map(|count| *count).unwrap_or(0),
        second.usage.input_tokens,
        second.usage.cached_input_tokens,
        second.usage.output_tokens,
        second.usage.cost_micros,
    ));
    if text == 0 {
        return Err(WorkModelError::Protocol);
    }
    Ok(())
}

pub(super) fn run() -> Result<(), super::ProbeFailure> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| super::ProbeFailure::Runtime)?;
    let mut failed = false;
    for provider in PROVIDERS {
        if !keys::present(provider).unwrap_or(false) {
            say(format_args!(
                "lead-smoke: provider={provider:?} skipped=no_key"
            ));
            continue;
        }
        let search = matches!(
            provider,
            WorkModelProvider::OpenAi | WorkModelProvider::Anthropic | WorkModelProvider::Google
        );
        if let Err(error) = runtime.block_on(round_trip(provider, search)) {
            failed = true;
            say(format_args!(
                "lead-smoke: provider={provider:?} error={error:?}"
            ));
        }
    }
    if failed {
        Err(super::ProbeFailure::Verification)
    } else {
        Ok(())
    }
}
