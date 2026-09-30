//! The built-in model list: a short, curated set of current models per
//! provider, lead and small, checked against each provider's own model and
//! pricing pages (and OpenRouter's catalog) on 2026-09-30; and the Zephium
//! Cloud catalog parser, whose list replaces this one when signed in.

use serde_json::Value;
use zephium_core::work::model::*;

use WorkModelProvider as P;
use WorkModelRole as R;

/// Built-in per-call fees for provider-native search, in micro-USD.
pub fn search_fee_micros(provider: WorkModelProvider) -> u64 {
    match provider {
        // $10 per 1,000 calls; search content bills as input tokens.
        P::OpenAi | P::Anthropic => 10_000,
        // Grounding with Google Search: $14 per 1,000 queries past the free tier.
        P::Google => 14_000,
        _ => 0,
    }
}

const fn usd(dollars_per_million: f64) -> u64 {
    (dollars_per_million * 1_000_000.0 + 0.5) as u64
}

struct Spec {
    provider: WorkModelProvider,
    model: &'static str,
    name: &'static str,
    roles: &'static [WorkModelRole],
    recommended: bool,
    context: u32,
    output: u32,
    vision: bool,
    search: bool,
    price: (f64, f64, f64),
}

const SPECS: &[Spec] = &[
    Spec {
        provider: P::Anthropic,
        model: "claude-opus-5-5",
        name: "Claude Opus 5.5",
        roles: &[R::Lead],
        recommended: true,
        context: 1_000_000,
        output: 128_000,
        vision: true,
        search: true,
        price: (4.0, 0.2, 20.0),
    },
    Spec {
        provider: P::Anthropic,
        model: "claude-sonnet-5-5",
        name: "Claude Sonnet 5.5",
        roles: &[R::Lead, R::Page],
        recommended: true,
        context: 1_000_000,
        output: 128_000,
        vision: true,
        search: true,
        price: (2.0, 0.2, 10.0),
    },
    Spec {
        provider: P::Anthropic,
        model: "claude-haiku-4-5-20251001",
        name: "Claude Haiku 4.5",
        roles: &[R::Page, R::Light],
        recommended: true,
        context: 200_000,
        output: 64_000,
        vision: true,
        search: true,
        price: (1.0, 0.1, 5.0),
    },
    Spec {
        provider: P::Anthropic,
        model: "claude-fable-5-1",
        name: "Claude Fable 5.1",
        roles: &[R::Lead],
        recommended: false,
        context: 1_000_000,
        output: 128_000,
        vision: true,
        search: true,
        price: (10.0, 0.25, 50.0),
    },
    // https://developers.openai.com/api/docs/models/gpt-5.6-terra
    Spec {
        provider: P::OpenAi,
        model: "gpt-5.6-terra",
        name: "GPT-5.6 Terra",
        roles: &[R::Lead, R::Page],
        recommended: false,
        context: 1_050_000,
        output: 128_000,
        vision: true,
        search: true,
        price: (2.0, 0.2, 12.0),
    },
    // https://developers.openai.com/api/docs/models/gpt-6.1-sol
    Spec {
        provider: P::OpenAi,
        model: "gpt-6.1-sol",
        name: "GPT-6.1 Sol",
        roles: &[R::Lead, R::Page],
        recommended: true,
        context: 1_050_000,
        output: 128_000,
        vision: true,
        search: true,
        price: (2.0, 0.1, 10.0),
    },
    Spec {
        provider: P::OpenAi,
        model: "gpt-6-luna",
        name: "GPT-6 Luna",
        roles: &[R::Lead, R::Page, R::Light],
        recommended: true,
        context: 1_050_000,
        output: 128_000,
        vision: true,
        search: true,
        price: (0.1, 0.01, 0.5),
    },
    Spec {
        provider: P::OpenAi,
        model: "gpt-6-sol",
        name: "GPT-6 Sol",
        roles: &[R::Lead, R::Page],
        recommended: true,
        context: 1_050_000,
        output: 128_000,
        vision: true,
        search: true,
        price: (2.0, 0.2, 10.0),
    },
    Spec {
        provider: P::OpenAi,
        model: "gpt-6-astra",
        name: "GPT-6 Astra",
        roles: &[R::Lead],
        recommended: false,
        context: 1_050_000,
        output: 128_000,
        vision: true,
        search: true,
        price: (10.0, 1.0, 50.0),
    },
    Spec {
        provider: P::Google,
        model: "gemini-3.1-pro-preview",
        name: "Gemini 3.1 Pro",
        roles: &[R::Lead],
        recommended: true,
        context: 1_048_576,
        output: 65_536,
        vision: true,
        search: true,
        price: (2.0, 0.2, 12.0),
    },
    Spec {
        provider: P::Google,
        model: "gemini-3.8-flash",
        name: "Gemini 3.8 Flash",
        roles: &[R::Lead, R::Page],
        recommended: true,
        context: 1_048_576,
        output: 65_536,
        vision: true,
        search: true,
        price: (0.75, 0.075, 3.75),
    },
    Spec {
        provider: P::Google,
        model: "gemini-3.5-flash-lite",
        name: "Gemini 3.5 Flash-Lite",
        roles: &[R::Page, R::Light],
        recommended: true,
        context: 1_048_576,
        output: 65_536,
        vision: true,
        search: true,
        price: (0.3, 0.03, 2.5),
    },
    // DeepSeek bills a lower off-peak rate; the peak rate keeps cost caps honest.
    Spec {
        provider: P::DeepSeek,
        model: "deepseek-v4-pro",
        name: "DeepSeek V4 Pro",
        roles: &[R::Lead],
        recommended: true,
        context: 1_000_000,
        output: 384_000,
        vision: false,
        search: false,
        price: (1.32, 0.044, 3.96),
    },
    Spec {
        provider: P::DeepSeek,
        model: "deepseek-flash",
        name: "DeepSeek V4.1 Flash",
        roles: &[R::Light],
        recommended: true,
        context: 1_000_000,
        output: 384_000,
        vision: false,
        search: false,
        price: (0.3, 0.006, 1.2),
    },
    // OpenRouter serves the same models under its own ids and prices.
    Spec {
        provider: P::OpenRouter,
        model: "anthropic/claude-sonnet-5.5",
        name: "Claude Sonnet 5.5",
        roles: &[R::Lead, R::Page],
        recommended: true,
        context: 1_000_000,
        output: 128_000,
        vision: true,
        search: false,
        price: (2.0, 0.2, 10.0),
    },
    Spec {
        provider: P::OpenRouter,
        model: "anthropic/claude-opus-5.5",
        name: "Claude Opus 5.5",
        roles: &[R::Lead],
        recommended: false,
        context: 1_000_000,
        output: 128_000,
        vision: true,
        search: false,
        price: (4.0, 0.2, 20.0),
    },
    Spec {
        provider: P::OpenRouter,
        model: "openai/gpt-6-luna",
        name: "GPT-6 Luna",
        roles: &[R::Lead, R::Page, R::Light],
        recommended: true,
        context: 1_050_000,
        output: 128_000,
        vision: true,
        search: false,
        price: (0.1, 0.01, 0.5),
    },
    Spec {
        provider: P::OpenRouter,
        model: "openai/gpt-6-sol",
        name: "GPT-6 Sol",
        roles: &[R::Lead, R::Page],
        recommended: false,
        context: 1_050_000,
        output: 128_000,
        vision: true,
        search: false,
        price: (2.0, 0.2, 10.0),
    },
    Spec {
        provider: P::OpenRouter,
        model: "google/gemini-3.8-flash",
        name: "Gemini 3.8 Flash",
        roles: &[R::Lead, R::Page],
        recommended: false,
        context: 1_048_576,
        output: 65_536,
        vision: true,
        search: false,
        price: (0.75, 0.075, 3.75),
    },
];

/// The wire a provider's own endpoint speaks.
pub fn wire(provider: WorkModelProvider) -> WorkModelWire {
    match provider {
        P::OpenAi => WorkModelWire::OpenAiResponses,
        P::Anthropic => WorkModelWire::AnthropicMessages,
        P::Google => WorkModelWire::Gemini,
        P::DeepSeek | P::OpenRouter | P::Compatible | P::Cloud => WorkModelWire::ChatCompletions,
    }
}

/// `provider/model`, the stable catalog id.
pub fn entry_id(provider: WorkModelProvider, model: &str) -> String {
    format!("{}/{model}", provider_slug(provider))
}

/// The lowercase provider name used in ids and settings.
pub fn provider_slug(provider: WorkModelProvider) -> &'static str {
    match provider {
        P::OpenAi => "openai",
        P::Anthropic => "anthropic",
        P::Google => "google",
        P::DeepSeek => "deepseek",
        P::OpenRouter => "openrouter",
        P::Compatible => "compatible",
        P::Cloud => "zephium",
    }
}

/// The inverse of [`provider_slug`].
pub fn provider_from_slug(slug: &str) -> Option<WorkModelProvider> {
    Some(match slug {
        "openai" => P::OpenAi,
        "anthropic" => P::Anthropic,
        "google" => P::Google,
        "deepseek" => P::DeepSeek,
        "openrouter" => P::OpenRouter,
        "compatible" => P::Compatible,
        "zephium" => P::Cloud,
        _ => return None,
    })
}

/// Every built-in model, recommended first within each provider.
pub fn builtin() -> Vec<WorkModelEntry> {
    SPECS
        .iter()
        .map(|spec| WorkModelEntry {
            id: entry_id(spec.provider, spec.model),
            model: WorkModelRef {
                provider: spec.provider,
                wire: wire(spec.provider),
                model: spec.model.to_owned(),
            },
            display_name: spec.name.to_owned(),
            roles: spec.roles.to_vec(),
            recommended: spec.recommended,
            context_window: spec.context,
            max_output: spec.output,
            supports: WorkModelSupports {
                tools: true,
                vision: spec.vision,
                prompt_cache: true,
                reasoning: true,
                native_search: spec.search,
            },
            price: Some(WorkModelPrice {
                input: usd(spec.price.0),
                cached_input: usd(spec.price.1),
                output: usd(spec.price.2),
            }),
        })
        .collect()
}

/// The price the built-in list knows for `model` from `provider`.
pub fn builtin_price(provider: WorkModelProvider, model: &str) -> Option<WorkModelPrice> {
    builtin()
        .into_iter()
        .find(|entry| entry.model.provider == provider && entry.model.model == model)
        .and_then(|entry| entry.price)
}

/// What a provider family runs for each role when the person chose nothing.
pub fn family_default(provider: WorkModelProvider, role: WorkModelRole) -> Option<&'static str> {
    Some(match (provider, role) {
        (P::Anthropic, R::Lead) => "claude-opus-5-5",
        (P::Anthropic, R::Page | R::Light) => "claude-haiku-4-5-20251001",
        (P::OpenAi, R::Lead) => "gpt-6-astra",
        (P::OpenAi, R::Page | R::Light) => "gpt-6-luna",
        (P::Google, R::Lead) => "gemini-3.1-pro-preview",
        (P::Google, R::Page) => "gemini-3.8-flash",
        (P::Google, R::Light) => "gemini-3.5-flash-lite",
        (P::DeepSeek, R::Lead | R::Page) => "deepseek-v4-pro",
        (P::DeepSeek, R::Light) => "deepseek-flash",
        (P::OpenRouter, R::Lead) => "anthropic/claude-sonnet-5.5",
        (P::OpenRouter, R::Page | R::Light) => "openai/gpt-6-luna",
        _ => return None,
    })
}

/// A Cloud catalog model with the upstream whose path it travels.
#[derive(Clone, Debug)]
pub struct CloudModel {
    /// The entry as the picker shows it; its provider is Cloud.
    pub entry: WorkModelEntry,
    /// The provider whose native wire and path it uses.
    pub upstream: WorkModelProvider,
    /// Smallest plan that may use it, as the server names it.
    pub plan_min: Option<String>,
}

const MAX_CLOUD_MODELS: usize = 200;

/// Parses `GET /v1/models`. Unknown providers, roles or malformed rows are
/// skipped; the server decides which models exist. Cloud prices are credits,
/// so cost estimates use the upstream's built-in price where one is known.
pub fn parse_cloud(body: &Value) -> Vec<CloudModel> {
    let rows = body
        .get("data")
        .or_else(|| body.get("models"))
        .unwrap_or(body)
        .as_array()
        .cloned()
        .unwrap_or_default();
    rows.iter()
        .filter_map(cloud_row)
        .take(MAX_CLOUD_MODELS)
        .collect()
}

fn cloud_row(row: &Value) -> Option<CloudModel> {
    let text = |key: &str| row.get(key).and_then(Value::as_str);
    let upstream = provider_from_slug(text("provider")?)?;
    if matches!(upstream, P::Cloud | P::Compatible) {
        return None;
    }
    let id = text("id").filter(|id| valid_text(id, 128))?;
    let model = text("upstream_model").filter(|m| valid_text(m, 128))?;
    let roles: Vec<WorkModelRole> = row
        .get("roles")?
        .as_array()?
        .iter()
        .filter_map(|role| match role.as_str()? {
            "lead" => Some(R::Lead),
            "page" => Some(R::Page),
            "light" => Some(R::Light),
            "decision" => Some(R::Decision),
            _ => None,
        })
        .collect();
    if roles.is_empty() {
        return None;
    }
    let supports = row.get("supports");
    let flag = |key: &str| {
        supports
            .and_then(|s| s.get(key))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    let number = |key: &str| {
        row.get(key)
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
    };
    Some(CloudModel {
        entry: WorkModelEntry {
            id: format!("zephium/{id}"),
            model: WorkModelRef {
                provider: P::Cloud,
                wire: wire(upstream),
                model: model.to_owned(),
            },
            display_name: text("display_name")
                .filter(|name| valid_text(name, 64))
                .unwrap_or(model)
                .to_owned(),
            roles,
            recommended: row
                .get("recommended")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            context_window: number("context_window").unwrap_or(128_000),
            max_output: number("max_output").unwrap_or(8_192),
            supports: WorkModelSupports {
                tools: flag("tools"),
                vision: flag("vision"),
                prompt_cache: flag("prompt_cache"),
                reasoning: flag("reasoning"),
                native_search: flag("native_search"),
            },
            price: builtin_price(upstream, model),
        },
        upstream,
        plan_min: text("plan_min")
            .filter(|plan| valid_text(plan, 32))
            .map(str::to_owned),
    })
}

pub(crate) fn valid_text(text: &str, max: usize) -> bool {
    !text.trim().is_empty() && text.len() <= max && !text.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_builtin_is_unique_priced_and_has_a_family_default_for_lead() {
        let entries = builtin();
        let mut ids: Vec<_> = entries.iter().map(|e| e.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), entries.len());
        for entry in &entries {
            assert!(entry.price.is_some());
            assert_eq!(entry.id, entry_id(entry.model.provider, &entry.model.model));
        }
        for provider in [
            P::OpenAi,
            P::Anthropic,
            P::Google,
            P::DeepSeek,
            P::OpenRouter,
        ] {
            for role in [R::Lead, R::Light] {
                let model = family_default(provider, role).unwrap();
                let entry = entries
                    .iter()
                    .find(|e| e.model.provider == provider && e.model.model == model)
                    .unwrap();
                assert!(entry.roles.contains(&role));
            }
        }
        assert_eq!(family_default(P::OpenAi, R::Lead), Some("gpt-6-astra"));
        for provider in [
            P::OpenAi,
            P::Anthropic,
            P::Google,
            P::DeepSeek,
            P::OpenRouter,
        ] {
            let listed = entries
                .iter()
                .filter(|e| e.model.provider == provider)
                .count();
            assert!((2..=6).contains(&listed), "{provider:?} lists {listed}");
        }
        assert_eq!(
            builtin_price(P::Anthropic, "claude-opus-5-5")
                .unwrap()
                .input,
            4_000_000
        );
    }

    #[test]
    fn the_cloud_catalog_keeps_known_rows_and_skips_the_rest() {
        let parsed = parse_cloud(&json!({"data": [
            {"id": "sonnet", "provider": "anthropic", "upstream_model": "claude-sonnet-5-5",
             "display_name": "Claude Sonnet 5.5", "roles": ["lead", "page"], "recommended": true,
             "context_window": 1000000, "max_output": 64000,
             "supports": {"tools": true, "vision": true, "native_search": true}, "plan_min": "free"},
            {"id": "x", "provider": "mystery", "upstream_model": "m", "roles": ["lead"]},
            {"id": "y", "provider": "openai", "upstream_model": "gpt-6-sol", "roles": []}
        ]}));
        assert_eq!(parsed.len(), 1);
        let model = &parsed[0];
        assert_eq!(model.entry.id, "zephium/sonnet");
        assert_eq!(model.entry.model.provider, P::Cloud);
        assert_eq!(model.entry.model.wire, WorkModelWire::AnthropicMessages);
        assert_eq!(model.upstream, P::Anthropic);
        assert!(model.entry.supports.native_search);
        assert_eq!(model.entry.price.as_ref().unwrap().output, 10_000_000);
    }
}
