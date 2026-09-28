//! One model call for the lead or a helper: bounded retries on transient
//! failures, the run's deadline, and usage charged to the run in its own
//! accounting.
use std::sync::Arc;
use std::time::Duration;

use zephium_core::work::{model::*, runtime::*};
use zephium_ipc::work::WorkActivityV1;

use super::run::LeadRun;

/// A resolved model and its ready client.
#[derive(Clone)]
pub struct LeadModel {
    pub entry: WorkModelEntry,
    pub client: Arc<dyn WorkModelClient>,
}

/// The models a run uses, one per role.
#[derive(Clone)]
pub struct WorkLeadModels {
    pub lead: LeadModel,
    /// Helpers that act on pages.
    pub page: LeadModel,
    /// Helpers that read and summarize.
    pub light: LeadModel,
}
impl WorkLeadModels {
    pub fn for_role(&self, role: WorkModelRole) -> &LeadModel {
        match role {
            WorkModelRole::Page => &self.page,
            WorkModelRole::Light | WorkModelRole::Decision => &self.light,
            WorkModelRole::Lead => &self.lead,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CallFailure {
    /// Nothing was sent or nothing was charged; the note says why.
    Refused(WorkModelError),
    /// The call may have been charged, with no usable answer.
    Lost,
    /// The run stopped or ran out of time while the call ran.
    Stopped,
}

const CALL_TIMEOUT: Duration = Duration::from_secs(240);
const RETRIES: usize = 2;
/// Per million tokens, in micro-USD, when the catalog has no price.
const UNPRICED: WorkModelPrice = WorkModelPrice {
    input: 15_000_000,
    cached_input: 1_500_000,
    output: 60_000_000,
};

/// Tokens a call bills: fresh input and output. Cached input is charged in
/// the cost at its own price but not counted against the token limit.
pub(crate) fn usage(entry: &WorkModelEntry, usage: WorkModelUsage) -> WorkUsage {
    let cached = usage.cached_input_tokens.min(usage.input_tokens);
    let fresh = usage.input_tokens - cached;
    let tokens = fresh.saturating_add(usage.output_tokens);
    let (cost, accounting) = match (usage.cost_micros, &entry.price) {
        (Some(cost), _) => (cost, WorkUsageAccounting::Exact),
        (None, Some(price)) => (
            priced(price, fresh, cached, usage.output_tokens),
            WorkUsageAccounting::Exact,
        ),
        (None, None) => (
            priced(&UNPRICED, fresh, cached, usage.output_tokens),
            WorkUsageAccounting::ConservativeReservation,
        ),
    };
    WorkUsage {
        model_tokens: u32::try_from(tokens).unwrap_or(u32::MAX),
        cost_micro_usd: u32::try_from(cost).unwrap_or(u32::MAX),
        operations: 1,
        accounting,
    }
}
fn priced(price: &WorkModelPrice, fresh: u64, cached: u64, output: u64) -> u64 {
    let part = |tokens: u64, rate: u64| (u128::from(tokens) * u128::from(rate)).div_ceil(1_000_000);
    u64::try_from(
        part(fresh, price.input) + part(cached, price.cached_input) + part(output, price.output),
    )
    .unwrap_or(u64::MAX)
}

/// Runs one call and charges the run. Transient refusals retry twice,
/// honouring the provider's retry-after within the deadline.
pub(crate) async fn call(
    run: &LeadRun,
    model: &LeadModel,
    request: WorkModelRequest,
) -> Result<(WorkModelOutcome, WorkUsage), CallFailure> {
    let events = |event: WorkModelEvent| {
        if let WorkModelEvent::Search { .. } = event {
            run.activity(WorkActivityV1::Searching);
        }
    };
    let mut attempt = 0;
    loop {
        if run.cancelled().await {
            return Err(CallFailure::Stopped);
        }
        let deadline = run
            .probe
            .deadline()
            .min(std::time::Instant::now() + CALL_TIMEOUT);
        let result =
            tokio::time::timeout_at(deadline.into(), model.client.call(request.clone(), &events))
                .await;
        let error = match result {
            Ok(Ok(outcome)) => {
                let charged = usage(&model.entry, outcome.usage);
                run.charge(charged);
                return Ok((outcome, charged));
            }
            Ok(Err(error)) => error,
            Err(_) => {
                return Err(if run.cancelled().await {
                    CallFailure::Stopped
                } else {
                    CallFailure::Lost
                })
            }
        };
        run.report(super::WorkLeadDiagnostic::ModelRefused { error });
        let wait = match error {
            WorkModelError::RateLimited { retry_after_ms } => Some(Duration::from_millis(
                retry_after_ms.unwrap_or(4_000).clamp(1_000, 30_000),
            )),
            WorkModelError::Overloaded | WorkModelError::Network => {
                Some(Duration::from_secs(2 + 3 * attempt as u64))
            }
            WorkModelError::Cancelled => return Err(CallFailure::Stopped),
            WorkModelError::Protocol => return Err(CallFailure::Lost),
            _ => None,
        };
        match wait {
            Some(wait) if attempt < RETRIES => {
                attempt += 1;
                tokio::time::sleep(wait).await;
            }
            _ => return Err(CallFailure::Refused(error)),
        }
    }
}

/// The text of an assistant turn, joined.
pub(crate) fn text(parts: &[WorkModelPart]) -> String {
    parts
        .iter()
        .filter_map(|part| match part {
            WorkModelPart::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

pub(crate) fn tool_calls(parts: &[WorkModelPart]) -> Vec<WorkModelToolCall> {
    parts
        .iter()
        .filter_map(|part| match part {
            WorkModelPart::ToolCall(call) => Some(call.clone()),
            _ => None,
        })
        .collect()
}

/// A line for the person from what the model said: its first sentence-ish
/// line, bounded, never a paragraph.
pub(crate) fn say_line(text: &str) -> Option<String> {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?
        .trim_start_matches(['#', '*', '-', ' '])
        .trim();
    (!line.is_empty()).then(|| clip(line, 240))
}

/// At most `max` bytes, cut at a character boundary.
pub(crate) fn clip(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max.saturating_sub(1);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(price: Option<WorkModelPrice>) -> WorkModelEntry {
        WorkModelEntry {
            id: "openai/gpt-6".into(),
            model: WorkModelRef {
                provider: WorkModelProvider::OpenAi,
                wire: WorkModelWire::OpenAiResponses,
                model: "gpt-6".into(),
            },
            display_name: "GPT-6".into(),
            roles: vec![WorkModelRole::Lead],
            recommended: true,
            context_window: 400_000,
            max_output: 32_000,
            supports: WorkModelSupports {
                tools: true,
                vision: true,
                prompt_cache: true,
                reasoning: true,
                native_search: true,
            },
            price,
        }
    }

    #[test]
    fn cached_input_is_priced_but_not_counted_against_the_token_limit() {
        let price = WorkModelPrice {
            input: 2_000_000,
            cached_input: 200_000,
            output: 8_000_000,
        };
        let used = usage(
            &entry(Some(price)),
            WorkModelUsage {
                input_tokens: 10_000,
                cached_input_tokens: 8_000,
                output_tokens: 500,
                reasoning_tokens: 0,
                cost_micros: None,
            },
        );
        assert_eq!(used.model_tokens, 2_500);
        assert_eq!(used.cost_micro_usd, 4_000 + 1_600 + 4_000);
        assert_eq!(used.accounting, WorkUsageAccounting::Exact);
        let unpriced = usage(
            &entry(None),
            WorkModelUsage {
                input_tokens: 1_000,
                cached_input_tokens: 0,
                output_tokens: 100,
                reasoning_tokens: 0,
                cost_micros: None,
            },
        );
        assert_eq!(
            unpriced.accounting,
            WorkUsageAccounting::ConservativeReservation
        );
        assert_eq!(unpriced.cost_micro_usd, 15_000 + 6_000);
    }

    #[test]
    fn the_line_for_the_person_is_one_bounded_line() {
        assert_eq!(
            say_line("\n## Looking at flights\nmore"),
            Some("Looking at flights".into())
        );
        assert_eq!(say_line("  "), None);
        assert!(say_line(&"word ".repeat(200)).unwrap().len() <= 243);
    }
}
