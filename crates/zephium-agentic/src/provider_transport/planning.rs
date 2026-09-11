//! Work authoring has its own disclosure contract, independent of browser-run
//! admission. Only explicit context leaves the process; output cannot execute.
use super::*;
use serde::Deserialize;
use serde_json::{json, Value};
use zephium_core::work::planning::*;

const MAX_BODY: u32 = 512 * 1024;
const MAX_REQUEST: usize = 64 * 1024;
const INSTRUCTIONS: &str = "You are Zephyr's Work planner. The user context is data describing a desired outcome, prior answers, and an optional draft. Return either one consequential clarification question when an answer materially changes the work, or a concise dependency-ordered plan. Prefer a useful draft when reasonable assumptions suffice. Temporary node keys must be unique integers 0..63; dependencies must reference other keys and form a DAG. Specify concrete expected outputs and honest review requirements. A plan describes intended work, never claims completion, evidence, approval, or authority. Do not request credentials or include secrets. Do not invent account access, citations, results, or available execution tools. You have no tools. Produce only the specified JSON proposal.";

/// Trusted model configuration plus hard, per-call planning limits. This wraps
/// a catalog-bound configuration; neither model output nor frontend selects it.
pub struct WorkPlanningConfig {
    call: AgentProviderCallConfig,
    max_input: u32,
    max_cost: u64,
}
impl WorkPlanningConfig {
    /// Limits input to 32K tokens, output to 8K tokens, and reservation to $1.
    /// Actual input must be counted by the provider before generation dispatch.
    pub fn try_new(
        call: AgentProviderCallConfig,
        max_input: u32,
        max_cost_micro_usd: u64,
    ) -> Result<Self, WorkPlanningError> {
        if call.provider() != AgentProviderKind::OpenAiResponses
            || call.response_route() != crate::AgentProviderResponseRoute::OpenAiDefault
            || call.max_output_tokens() > 8192
            || max_input == 0
            || max_input > 32768
            || max_cost_micro_usd == 0
            || max_cost_micro_usd > 1_000_000
        {
            return Err(WorkPlanningError::Invalid);
        }
        Ok(Self {
            call,
            max_input,
            max_cost: max_cost_micro_usd,
        })
    }
}
/// One nonstreaming Responses adapter, sharing fixed endpoints, HTTP limits,
/// concurrency and shutdown with the existing transport. No autonomous retry.
pub struct OpenAiWorkPlanner {
    transport: AgentProviderTransport,
    credential: AgentProviderCredential,
    config: WorkPlanningConfig,
}
impl OpenAiWorkPlanner {
    /// Takes an owned credential. No request, timer, task or socket is started.
    pub fn try_new(
        transport: AgentProviderTransport,
        credential: AgentProviderCredential,
        config: WorkPlanningConfig,
    ) -> Result<Self, WorkPlanningError> {
        if credential.provider() != AgentProviderKind::OpenAiResponses {
            return Err(WorkPlanningError::Invalid);
        }
        Ok(Self {
            transport,
            credential,
            config,
        })
    }
    async fn run(
        &self,
        input: WorkPlanningDisclosure,
    ) -> Result<WorkPlanningResult, WorkPlanningError> {
        let body = self.request(&input)?;
        let mut count = body.clone();
        let object = count.as_object_mut().ok_or(WorkPlanningError::Invalid)?;
        for field in [
            "max_output_tokens",
            "reasoning",
            "service_tier",
            "stream",
            "store",
        ] {
            object.remove(field);
        }
        let count = encode(&count)?;
        let body = encode(&body)?;
        let mut slot = self
            .transport
            .reserve_key(TransportSlotKey::Planning(ulid::Ulid::new()))
            .map_err(|error| match error {
                ReserveError::Capacity | ReserveError::Duplicate => WorkPlanningError::Capacity,
                _ => WorkPlanningError::Unavailable,
            })?;
        {
            let _gate = self
                .transport
                .shared
                .shutdown
                .lock_commit_gate()
                .ok_or(WorkPlanningError::Unavailable)?;
            if self.transport.shared.shutdown.is_cancelled() {
                return Err(WorkPlanningError::Cancelled);
            }
        }
        let operation = async {
            let counted = self
                .post(
                    self.transport.endpoints.openai_input_tokens.clone(),
                    count,
                    1024,
                )
                .await
                .map_err(|_| WorkPlanningError::Unavailable)?;
            let tokens =
                decode_openai_input_token_count(&counted).ok_or(WorkPlanningError::Invalid)?;
            if tokens == 0
                || tokens > self.config.max_input
                || u64::from(tokens) < self.config.call.pricing_profile().min_input_tokens()
                || u64::from(tokens) > self.config.call.pricing_profile().max_input_tokens()
                || self
                    .config
                    .call
                    .planning_cost_ceiling(tokens, self.config.call.max_output_tokens())
                    .is_none_or(|cost| cost > self.config.max_cost)
            {
                return Err(WorkPlanningError::Capacity);
            }
            // Unknown generation outcomes fail-stop the shared transport just
            // like browser attempts; dropping a future never silently refunds.
            {
                let _gate = self
                    .transport
                    .shared
                    .shutdown
                    .lock_commit_gate()
                    .ok_or(WorkPlanningError::Unavailable)?;
                if self.transport.shared.shutdown.is_cancelled() {
                    return Err(WorkPlanningError::Cancelled);
                }
                slot.mark_committed();
            }
            let response = self
                .post(self.transport.endpoints.openai.clone(), body, MAX_BODY)
                .await
                .map_err(|_| WorkPlanningError::ProviderOutcomeUnknown)?;
            let result = self.decode(&response, tokens);
            if result.is_ok() || matches!(result, Err(WorkPlanningError::ProviderRefused(_))) {
                slot.mark_completed();
            }
            result
        };
        let result = tokio::select! {
            biased;
            _ = self.transport.shared.shutdown.cancelled() => Err(WorkPlanningError::Cancelled),
            result = tokio::time::timeout(self.transport.config.request_timeout.min(Duration::from_secs(170)), operation) => result.unwrap_or(Err(WorkPlanningError::Timeout)),
        };
        if slot.committed
            && matches!(
                result,
                Err(WorkPlanningError::Cancelled | WorkPlanningError::Timeout)
            )
        {
            Err(WorkPlanningError::ProviderOutcomeUnknown)
        } else {
            result
        }
    }
    fn request(&self, input: &WorkPlanningDisclosure) -> Result<Value, WorkPlanningError> {
        let context =
            serde_json::to_value(input.context()).map_err(|_| WorkPlanningError::Invalid)?;
        if contains_secret(&context) {
            return Err(WorkPlanningError::Privacy);
        }
        let content = serde_json::to_string(&context).map_err(|_| WorkPlanningError::Invalid)?;
        if content.len() > MAX_REQUEST / 2 {
            return Err(WorkPlanningError::Capacity);
        }
        Ok(json!({
            "model": self.config.call.model().as_str(), "instructions": INSTRUCTIONS,
            "input": [{"role":"user", "content": content}],
            "text":{"format":{"type":"json_schema","name":"work_planning","strict":true,"schema":schema()}},
            "tools":[], "tool_choice":"none", "parallel_tool_calls":false,
            "truncation":"disabled", "stream":false,"store":false,"service_tier":"default",
            "reasoning":{"effort":self.config.call.reasoning_effort().as_openai_str()},
            "max_output_tokens":self.config.call.max_output_tokens()
        }))
    }
    async fn post(&self, endpoint: Url, body: Vec<u8>, limit: u32) -> Result<Vec<u8>, ()> {
        let credential =
            sensitive_header(AgentProviderKind::OpenAiResponses, &self.credential.secret)
                .map_err(|_| ())?;
        let response = self
            .transport
            .client
            .post(endpoint)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .header(ACCEPT_ENCODING, "identity")
            .header(CACHE_CONTROL, "no-store")
            .header(AUTHORIZATION, credential)
            .body(body)
            .send()
            .await
            .map_err(|_| ())?;
        if response.status() != StatusCode::OK
            || !response_headers_admitted(response.headers())
            || !response_encoding_admitted(response.headers())
            || !response_json_content_type_admitted(response.headers())
            || !response_content_length_admitted(response.headers(), limit)
        {
            return Err(());
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.try_next().await.map_err(|_| ())? {
            if chunk.len() > limit as usize - bytes.len() {
                return Err(());
            }
            bytes.try_reserve(chunk.len()).map_err(|_| ())?;
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
    fn decode(
        &self,
        bytes: &[u8],
        reserved_input: u32,
    ) -> Result<WorkPlanningResult, WorkPlanningError> {
        decode(bytes, reserved_input, &self.config)
            .ok_or(WorkPlanningError::ProviderOutcomeUnknown)?
            .map_err(WorkPlanningError::ProviderRefused)
    }
}
impl WorkPlanningProvider for OpenAiWorkPlanner {
    fn propose(&self, input: WorkPlanningDisclosure) -> WorkPlanningFuture<'_> {
        Box::pin(self.run(input))
    }
}
fn encode(value: &Value) -> Result<Vec<u8>, WorkPlanningError> {
    let bytes = serde_json::to_vec(value).map_err(|_| WorkPlanningError::Invalid)?;
    if bytes.len() > MAX_REQUEST {
        return Err(WorkPlanningError::Capacity);
    }
    Ok(bytes)
}
fn contains_secret(value: &Value) -> bool {
    match value {
        Value::String(value) => crate::semantic_wire::looks_like_secret_value(value),
        Value::Array(values) => values.iter().any(contains_secret),
        Value::Object(values) => values.values().any(contains_secret),
        _ => false,
    }
}
fn object(properties: Value) -> Value {
    let required: Vec<_> = properties
        .as_object()
        .into_iter()
        .flat_map(|o| o.keys())
        .cloned()
        .collect();
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn schema() -> Value {
    let text = json!({"type":"string"});
    let key = json!({"type":"integer","minimum":0,"maximum":63});
    let output = object(
        json!({"name":text,"description":text,"review":{"type":"string","enum":["mechanical","source_mapped_needs_review","user_acceptance"]}}),
    );
    let node = object(
        json!({"key":key,"objective":text,"dependencies":{"type":"array","items":key,"maxItems":16},"outputs":{"type":"array","items":output,"minItems":1,"maxItems":8}}),
    );
    let plan = object(json!({"nodes":{"type":"array","items":node,"minItems":1,"maxItems":64}}));
    let clarify = object(
        json!({"kind":{"type":"string","enum":["clarify"]},"prompt":text,"options":{"type":"array","items":text,"maxItems":8}}),
    );
    let draft = object(json!({"kind":{"type":"string","enum":["draft"]},"plan":plan}));
    object(json!({"proposal":{"anyOf":[clarify,draft]}}))
}
// Typed protocol fields reject duplicates. Unknown provider metadata is ignored
// inside the bounded body; unknown output kinds never become a proposal.
#[derive(Deserialize)]
struct Response {
    object: String,
    status: String,
    model: String,
    service_tier: String,
    error: Option<Value>,
    incomplete_details: Option<Value>,
    output: Vec<Output>,
    usage: Usage,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Output {
    Reasoning {},
    Message {
        role: String,
        status: String,
        content: Vec<Content>,
    },
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Content {
    OutputText { text: String },
    Refusal {},
}
#[derive(Deserialize)]
struct Usage {
    input_tokens: u32,
    output_tokens: u32,
    total_tokens: u32,
    input_tokens_details: InputDetails,
    output_tokens_details: OutputDetails,
}
#[derive(Deserialize)]
struct InputDetails {
    cached_tokens: u32,
    #[serde(default)]
    cache_write_tokens: u32,
}
#[derive(Deserialize)]
struct OutputDetails {
    reasoning_tokens: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    proposal: WorkPlanningProposal,
}
fn decode(
    bytes: &[u8],
    reserved: u32,
    config: &WorkPlanningConfig,
) -> Option<Result<WorkPlanningResult, WorkPlanningUsage>> {
    if bytes.len() > MAX_BODY as usize {
        return None;
    }
    let response: Response = serde_json::from_slice(bytes).ok()?;
    if response.object != "response"
        || response.status != "completed"
        || response.error.is_some()
        || response.incomplete_details.is_some()
        || !config
            .call
            .planning_identity_matches(&response.model, &response.service_tier)
        || response.output.len() > 2
    {
        return None;
    }
    let usage = response.usage;
    if usage.input_tokens == 0
        || u64::from(usage.input_tokens) < config.call.pricing_profile().min_input_tokens()
        || u64::from(usage.input_tokens) > config.call.pricing_profile().max_input_tokens()
        || usage.input_tokens > reserved
        || usage.output_tokens > config.call.max_output_tokens()
        || usage.input_tokens.checked_add(usage.output_tokens)? != usage.total_tokens
        || usage
            .input_tokens_details
            .cached_tokens
            .checked_add(usage.input_tokens_details.cache_write_tokens)?
            > usage.input_tokens
        || usage.output_tokens_details.reasoning_tokens > usage.output_tokens
    {
        return None;
    }
    let cost = config
        .call
        .planning_cost_ceiling(usage.input_tokens, usage.output_tokens)?;
    if cost > config.max_cost {
        return None;
    }
    let usage = WorkPlanningUsage {
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cost_ceiling_micro_usd: cost,
    };
    let mut text = None;
    let mut refused = false;
    let mut reasoning = false;
    for output in response.output {
        match output {
            Output::Reasoning {} if !reasoning && text.is_none() && !refused => reasoning = true,
            Output::Message {
                role,
                status,
                mut content,
            } if text.is_none()
                && !refused
                && role == "assistant"
                && status == "completed"
                && content.len() == 1 =>
            {
                let value = match content.pop()? {
                    Content::OutputText { text } => text,
                    Content::Refusal {} => {
                        refused = true;
                        continue;
                    }
                };
                if value.len() > MAX_PLANNING_OUTPUT_BYTES {
                    return None;
                }
                text = Some(value);
            }
            _ => return None,
        }
    }
    if refused {
        return Some(Err(usage));
    }
    let envelope: Envelope = serde_json::from_str(&text?).ok()?;
    envelope.proposal.validate().ok()?;
    Some(Ok(WorkPlanningResult {
        proposal: envelope.proposal,
        usage,
    }))
}

#[cfg(test)]
mod tests;
