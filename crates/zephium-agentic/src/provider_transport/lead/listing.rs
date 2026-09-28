//! A provider's own model list ("More models") and a cheap key check.

use futures_util::StreamExt as _;
use reqwest::header::HeaderMap;
use serde_json::Value;
use zephium_core::work::model::*;

use super::models::{builtin_price, entry_id, valid_text};
use super::{http, join, LeadSecret, LeadTarget};

const MAX_LIST_BYTES: usize = 8 * 1024 * 1024;
const MAX_LISTED: usize = 400;

/// What a key check found. It never carries provider text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LeadKeyCheck {
    /// The provider accepted the key.
    Valid,
    /// The provider refused the key.
    Invalid,
    /// The provider could not be asked (offline, outage, limits).
    Unreachable,
}

async fn get(
    target: &LeadTarget,
    path: &str,
    query: Option<&str>,
    secret: &LeadSecret,
) -> Result<(u16, Vec<u8>), WorkModelError> {
    let mut url = join(&target.base, path)?;
    url.set_query(query);
    let mut headers = HeaderMap::new();
    target.authorize(&mut headers, secret)?;
    let response = http(url.scheme() == "http")?
        .get(url)
        .headers(headers)
        .send()
        .await
        .map_err(|_| WorkModelError::Network)?;
    let status = response.status().as_u16();
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| WorkModelError::Network)?;
        if body.len() + chunk.len() > MAX_LIST_BYTES {
            return Err(WorkModelError::Protocol);
        }
        body.extend_from_slice(&chunk);
    }
    Ok((status, body))
}

/// Asks the provider whether it accepts `secret`, with one read-only request
/// that costs nothing (a model list, or OpenRouter's key endpoint).
pub async fn check_key(target: &LeadTarget, secret: &LeadSecret) -> LeadKeyCheck {
    let path = if target.upstream == WorkModelProvider::OpenRouter {
        "key"
    } else {
        "models"
    };
    match get(target, path, None, secret).await {
        Ok((200..=299, _)) => LeadKeyCheck::Valid,
        Ok((401 | 403, _)) => LeadKeyCheck::Invalid,
        // Gemini answers an unknown key with 400 API_KEY_INVALID.
        Ok((400, body)) if target.upstream == WorkModelProvider::Google => {
            let json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            let invalid = json
                .pointer("/error/details")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .any(|detail| {
                    detail.get("reason").and_then(Value::as_str) == Some("API_KEY_INVALID")
                });
            if invalid {
                LeadKeyCheck::Invalid
            } else {
                LeadKeyCheck::Unreachable
            }
        }
        _ => LeadKeyCheck::Unreachable,
    }
}

/// Lists the chat models the provider offers to this key, as catalog entries.
pub async fn list_models(
    target: &LeadTarget,
    secret: &LeadSecret,
) -> Result<Vec<WorkModelEntry>, WorkModelError> {
    let query = match target.upstream {
        WorkModelProvider::Anthropic => Some("limit=1000"),
        WorkModelProvider::Google => Some("pageSize=1000"),
        _ => None,
    };
    let (status, body) = get(target, "models", query, secret).await?;
    match status {
        200..=299 => {}
        401 | 403 => return Err(WorkModelError::Unauthorized),
        429 => {
            return Err(WorkModelError::RateLimited {
                retry_after_ms: None,
            })
        }
        _ => return Err(WorkModelError::Protocol),
    }
    let body: Value = serde_json::from_slice(&body).map_err(|_| WorkModelError::Protocol)?;
    Ok(parse_list(target, &body))
}

/// Fetches Zephium Cloud's `GET /v1/models` for a signed-in session.
pub async fn fetch_cloud_catalog(
    base: &str,
    secret: &LeadSecret,
) -> Result<Vec<super::models::CloudModel>, WorkModelError> {
    let root = reqwest::Url::parse(base).map_err(|_| WorkModelError::BadRequest)?;
    let target = LeadTarget {
        wire: WorkModelWire::ChatCompletions,
        upstream: WorkModelProvider::Cloud,
        base: join(&root, "v1")?,
        cloud: true,
    };
    let (status, body) = get(&target, "models", None, secret).await?;
    match status {
        200..=299 => {}
        401 | 403 => return Err(WorkModelError::Unauthorized),
        _ => return Err(WorkModelError::Protocol),
    }
    let body: Value = serde_json::from_slice(&body).map_err(|_| WorkModelError::Protocol)?;
    Ok(super::models::parse_cloud(&body))
}

pub(super) fn parse_list(target: &LeadTarget, body: &Value) -> Vec<WorkModelEntry> {
    let provider = target.upstream;
    let rows = body
        .get("data")
        .or_else(|| body.get("models"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut entries: Vec<WorkModelEntry> = rows
        .iter()
        .filter_map(|row| row_entry(provider, target.wire, row))
        .collect();
    entries.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    entries.dedup_by(|a, b| a.id == b.id);
    entries.truncate(MAX_LISTED);
    entries
}

fn row_entry(
    provider: WorkModelProvider,
    wire: WorkModelWire,
    row: &Value,
) -> Option<WorkModelEntry> {
    let text = |key: &str| row.get(key).and_then(Value::as_str);
    let number = |value: Option<&Value>| {
        value
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
    };
    let (model, name, context, output, vision, tools) = match provider {
        WorkModelProvider::OpenAi => {
            let id = text("id")?;
            if !chat_model(id, &["gpt-", "o1", "o3", "o4", "o5", "chatgpt-"]) {
                return None;
            }
            (id.to_owned(), id.to_owned(), None, None, true, true)
        }
        WorkModelProvider::Anthropic => {
            let id = text("id")?;
            (
                id.to_owned(),
                text("display_name").unwrap_or(id).to_owned(),
                number(row.get("max_input_tokens")),
                number(row.get("max_tokens")),
                true,
                true,
            )
        }
        WorkModelProvider::Google => {
            let id = text("name")?.strip_prefix("models/")?;
            let generates = row
                .get("supportedGenerationMethods")
                .and_then(Value::as_array)
                .is_some_and(|methods| {
                    methods
                        .iter()
                        .any(|m| m.as_str() == Some("generateContent"))
                });
            if !generates || !chat_model(id, &["gemini-"]) {
                return None;
            }
            (
                id.to_owned(),
                text("displayName").unwrap_or(id).to_owned(),
                number(row.get("inputTokenLimit")),
                number(row.get("outputTokenLimit")),
                true,
                true,
            )
        }
        WorkModelProvider::OpenRouter => {
            let id = text("id")?;
            let tools = row
                .get("supported_parameters")
                .and_then(Value::as_array)
                .is_some_and(|p| p.iter().any(|p| p.as_str() == Some("tools")));
            if !tools {
                return None;
            }
            let vision = row
                .pointer("/architecture/input_modalities")
                .and_then(Value::as_array)
                .is_some_and(|m| m.iter().any(|m| m.as_str() == Some("image")));
            (
                id.to_owned(),
                text("name").unwrap_or(id).to_owned(),
                number(row.get("context_length")),
                number(row.pointer("/top_provider/max_completion_tokens")),
                vision,
                true,
            )
        }
        WorkModelProvider::DeepSeek | WorkModelProvider::Compatible => {
            let id = text("id")?;
            (id.to_owned(), id.to_owned(), None, None, false, true)
        }
        WorkModelProvider::Cloud => return None,
    };
    if !valid_text(&model, 160) || !valid_text(&name, 96) {
        return None;
    }
    let price = if provider == WorkModelProvider::OpenRouter {
        openrouter_price(row)
    } else {
        builtin_price(provider, &model)
    };
    let mut roles = vec![WorkModelRole::Lead, WorkModelRole::Light];
    if vision {
        roles.insert(1, WorkModelRole::Page);
    }
    Some(WorkModelEntry {
        id: entry_id(provider, &model),
        model: WorkModelRef {
            provider,
            wire,
            model,
        },
        display_name: name,
        roles,
        recommended: false,
        context_window: context.unwrap_or(128_000),
        max_output: output.unwrap_or(16_384),
        supports: WorkModelSupports {
            tools,
            vision,
            prompt_cache: matches!(
                provider,
                WorkModelProvider::OpenAi
                    | WorkModelProvider::Anthropic
                    | WorkModelProvider::Google
                    | WorkModelProvider::DeepSeek
            ),
            reasoning: false,
            native_search: matches!(
                provider,
                WorkModelProvider::OpenAi
                    | WorkModelProvider::Anthropic
                    | WorkModelProvider::Google
            ),
        },
        price,
    })
}

fn chat_model(id: &str, prefixes: &[&str]) -> bool {
    const NOT_CHAT: &[&str] = &[
        "audio",
        "realtime",
        "transcribe",
        "tts",
        "image",
        "embedding",
        "moderation",
        "instruct",
        "search",
        "live",
        "dall-e",
        "whisper",
        "codex",
        "computer-use",
        "robotics",
    ];
    prefixes.iter().any(|prefix| id.starts_with(prefix))
        && !NOT_CHAT.iter().any(|word| id.contains(word))
}

/// OpenRouter prices are USD per token, as decimal strings.
fn openrouter_price(row: &Value) -> Option<WorkModelPrice> {
    let per_token = |key: &str| -> Option<u64> {
        let value: f64 = row
            .pointer(&format!("/pricing/{key}"))?
            .as_str()?
            .parse()
            .ok()?;
        (value.is_finite() && value >= 0.0).then(|| (value * 1e12).round() as u64)
    };
    let input = per_token("prompt")?;
    let output = per_token("completion")?;
    Some(WorkModelPrice {
        input,
        cached_input: per_token("input_cache_read").unwrap_or(input),
        output,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn listings_keep_chat_models_with_their_limits_and_prices() {
        let openai = LeadTarget::direct(WorkModelProvider::OpenAi).unwrap();
        let listed = parse_list(
            &openai,
            &json!({"data": [{"id": "gpt-6-sol"}, {"id": "gpt-realtime"}, {"id": "text-embedding-4"}]}),
        );
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "openai/gpt-6-sol");
        assert_eq!(listed[0].price.as_ref().unwrap().input, 2_000_000);

        let google = LeadTarget::direct(WorkModelProvider::Google).unwrap();
        let listed = parse_list(
            &google,
            &json!({"models": [
                {"name": "models/gemini-3.8-flash", "displayName": "Gemini 3.8 Flash",
                 "inputTokenLimit": 1048576, "outputTokenLimit": 65536,
                 "supportedGenerationMethods": ["generateContent", "countTokens"]},
                {"name": "models/gemini-embedding-2", "supportedGenerationMethods": ["embedContent"]}
            ]}),
        );
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].max_output, 65_536);

        let router = LeadTarget::direct(WorkModelProvider::OpenRouter).unwrap();
        let listed = parse_list(
            &router,
            &json!({"data": [
                {"id": "moonshotai/kimi-k3", "name": "Kimi K3", "context_length": 262144,
                 "supported_parameters": ["tools", "temperature"],
                 "architecture": {"input_modalities": ["text"]},
                 "pricing": {"prompt": "0.0000006", "completion": "0.0000025"}},
                {"id": "no/tools", "supported_parameters": []}
            ]}),
        );
        assert_eq!(listed.len(), 1);
        let price = listed[0].price.as_ref().unwrap();
        assert_eq!((price.input, price.output), (600_000, 2_500_000));
        assert!(!listed[0].roles.contains(&WorkModelRole::Page));
    }
}
