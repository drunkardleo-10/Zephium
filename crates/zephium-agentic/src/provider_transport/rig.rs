//! Pinned Rig provider mapping, before Zephium's byte admission boundary.
//!
//! Deliberately use the pure request conversion, not Rig's HTTP/agent drivers:
//! those regenerate bodies, retain raw responses, and have different logging,
//! retry and stream completion semantics. No Rig value is execution authority.

use rig_core::completion::CompletionRequest;
use rig_core::message::{Message, ToolChoice};
use rig_core::providers::openai::responses_api::{
    self, ResponsesRequestParams, SystemInstructionsPlacement,
};
use serde_json::{json, Value};

use crate::AgentProviderCallConfig;
use zephium_core::work::planning::WorkPlanningError;

/// Converts only the closed, tool-free Work request vocabulary. The caller
/// bounds context first and serializes/counts the returned final body once.
pub(super) fn structured_request(
    call: &AgentProviderCallConfig,
    content: String,
    instructions: &'static str,
    name: &'static str,
    schema: Value,
) -> Result<Value, WorkPlanningError> {
    let request = CompletionRequest {
        model: None,
        preamble: Some(instructions.to_owned()),
        chat_history: vec![Message::user(content)],
        documents: Vec::new(),
        tools: Vec::new(),
        temperature: None,
        max_tokens: Some(u64::from(call.max_output_tokens())),
        tool_choice: Some(ToolChoice::None),
        additional_params: Some(json!({
            "stream":false,
            "store":false,
            "service_tier":"default",
            "parallel_tool_calls":false,
            "truncation":"disabled",
            "reasoning":{"effort":call.reasoning_effort().as_openai_str()}
        })),
        output_schema: None,
        record_telemetry_content: false,
    };
    request
        .validate_message_content()
        .map_err(|_| WorkPlanningError::Invalid)?;
    let mut wire = responses_api::CompletionRequest::try_from(ResponsesRequestParams {
        model: call.model().as_str().to_owned(),
        request,
        system_instructions_placement: SystemInstructionsPlacement::Instructions,
    })
    .map_err(|_| WorkPlanningError::Invalid)?;
    // Rig requests encrypted reasoning automatically when reasoning is enabled.
    // Work does not retain/replay provider reasoning; omit that extra payload.
    wire.additional_parameters.include = None;
    wire = wire.with_structured_outputs(name, schema);
    let mut body = serde_json::to_value(wire).map_err(|_| WorkPlanningError::Invalid)?;
    // Rig omits an empty tools list. Keep an explicit empty list as part of
    // Zephium's closed capability declaration and token-count request identity.
    body.as_object_mut()
        .ok_or(WorkPlanningError::Invalid)?
        .insert("tools".to_owned(), json!([]));
    Ok(body)
}

/// The public search vocabulary is separate from all tool-free Work requests.
pub(super) fn public_search_request(
    call: &AgentProviderCallConfig,
    query: String,
    instructions: &'static str,
) -> Result<Value, WorkPlanningError> {
    let mut body = structured_request(
        call,
        query,
        instructions,
        "unused",
        json!({"type":"object"}),
    )?;
    let object = body.as_object_mut().ok_or(WorkPlanningError::Invalid)?;
    object.remove("text");
    if call.model().as_str() == "gpt-4.1-mini" {
        object.remove("reasoning");
    }
    let tool = responses_api::ResponsesToolDefinition::web_search()
        .with_config("search_context_size", json!("medium"));
    object.insert("tools".into(), json!([tool]));
    object.insert("tool_choice".into(), json!("required"));
    object.insert("max_tool_calls".into(), json!(1));
    object.insert("background".into(), json!(false));
    Ok(body)
}
