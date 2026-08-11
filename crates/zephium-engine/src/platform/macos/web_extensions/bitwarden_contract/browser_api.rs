//! Behavioral evidence for browser APIs used by the pinned Bitwarden build.
//!
//! Parser acceptance and namespace presence are insufficient. This fixture
//! crosses the exact background/content/product-tab boundary and classifies
//! native behavior without embedding any upstream Bitwarden artifact.

use std::path::Path;

use serde_json::Value;

pub(super) const PROBE_TITLE: &str = "zephium-browser-api-pending";
const SETTLE_POLLS: u16 = 240;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DynamicResourceUrl {
    Opaque,
    Principal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExecutionWorldNamespace {
    Native,
    AdapterRequired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SandboxIsolation {
    Native,
    AdapterRequired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Observation {
    dynamic_resource_url: DynamicResourceUrl,
    execution_world_namespace: ExecutionWorldNamespace,
    sandbox_isolation: SandboxIsolation,
}

impl Observation {
    pub(super) const fn dynamic_resource_url(self) -> &'static str {
        match self.dynamic_resource_url {
            DynamicResourceUrl::Opaque => "opaque",
            DynamicResourceUrl::Principal => "principal",
        }
    }

    pub(super) const fn execution_world_namespace(self) -> &'static str {
        match self.execution_world_namespace {
            ExecutionWorldNamespace::Native => "native",
            ExecutionWorldNamespace::AdapterRequired => "adapter-required",
        }
    }

    pub(super) const fn sandbox_isolation(self) -> &'static str {
        match self.sandbox_isolation {
            SandboxIsolation::Native => "native",
            SandboxIsolation::AdapterRequired => "adapter-required",
        }
    }
}

pub(super) fn write_fixture_assets(path: &Path) -> Result<(), String> {
    for (name, contents) in [
        (
            "fido2-page-script.js",
            "document.documentElement.dataset.zephiumBitwardenDynamicResource = 'executed';",
        ),
        (
            "menu-button.html",
            "<!doctype html><title>button</title><script src=\"sandbox-probe.js\"></script>",
        ),
        (
            "browser-api-probe.html",
            "<!doctype html><meta charset=\"utf-8\"><title>zephium-browser-api-pending</title><script src=\"browser-api-probe.js\"></script>",
        ),
        (
            "sandbox-probe.js",
            "parent.postMessage({ kind: 'zephium-bitwarden-sandbox', chromeType: typeof globalThis.chrome, runtimeType: typeof globalThis.chrome?.runtime, storageLocalType: typeof globalThis.chrome?.storage?.local, origin: location.origin }, '*');",
        ),
        (
            "scripting-page-script.js",
            "document.documentElement.dataset.zephiumBitwardenProgrammaticScript = 'executed';",
        ),
    ] {
        super::write(path, name, contents)?;
    }
    super::write(path, "browser-api-probe.js", &probe_script())
}

pub(super) fn product_tab_content_script(title: &str) -> String {
    format!(
        r#"(() => {{
    const api = globalThis.browser ?? globalThis.chrome;
    const evidence = {{
        dynamicResourceLoad: "pending",
        dynamicResourceExecution: "pending",
        dynamicResourceUrl: "pending",
        programmaticScript: "pending",
        sandboxChrome: "pending",
        sandboxRuntime: "pending",
        sandboxStorageLocal: "pending",
        sandboxOrigin: "pending",
        settled: false
    }};
    addEventListener("DOMContentLoaded", () => {{
        document.title = {title};
        const dynamicUrl = api.runtime.getURL("fido2-page-script.js");
        const parsed = new URL(dynamicUrl);
        evidence.dynamicResourceUrl = parsed.host === api.runtime.id ? "principal" : "opaque";

        const script = document.createElement("script");
        script.src = dynamicUrl;
        script.addEventListener("load", () => {{ evidence.dynamicResourceLoad = "loaded"; }}, {{ once: true }});
        script.addEventListener("error", () => {{ evidence.dynamicResourceLoad = "rejected"; }}, {{ once: true }});
        document.documentElement.append(script);

        const sandbox = document.createElement("iframe");
        addEventListener("message", (event) => {{
            if (event.source !== sandbox.contentWindow || event.data?.kind !== "zephium-bitwarden-sandbox") return;
            evidence.sandboxChrome = event.data.chromeType;
            evidence.sandboxRuntime = event.data.runtimeType;
            evidence.sandboxStorageLocal = event.data.storageLocalType;
            evidence.sandboxOrigin = event.data.origin;
        }});
        sandbox.hidden = true;
        sandbox.src = api.runtime.getURL("menu-button.html");
        document.documentElement.append(sandbox);

        try {{
            const request = api.runtime.sendMessage({{ type: "zephium-bitwarden-programmatic-script" }});
            request?.catch?.(() => {{ evidence.programmaticScript = "request-rejected"; }});
        }} catch (_) {{
            evidence.programmaticScript = "request-rejected";
        }}

        let polls = 0;
        const settle = () => {{
            evidence.dynamicResourceExecution =
                document.documentElement.dataset.zephiumBitwardenDynamicResource ?? "pending";
            evidence.programmaticScript =
                document.documentElement.dataset.zephiumBitwardenProgrammaticScript
                ?? evidence.programmaticScript;
            const complete = evidence.dynamicResourceLoad !== "pending"
                && evidence.dynamicResourceExecution !== "pending"
                && evidence.programmaticScript !== "pending"
                && evidence.sandboxChrome !== "pending"
                && evidence.sandboxRuntime !== "pending"
                && evidence.sandboxStorageLocal !== "pending"
                && evidence.sandboxOrigin !== "pending";
            if (complete || polls >= {SETTLE_POLLS}) {{
                if (evidence.dynamicResourceExecution === "pending") {{
                    evidence.dynamicResourceExecution = "not-executed";
                }}
                if (evidence.programmaticScript === "pending") {{
                    evidence.programmaticScript = "not-executed";
                }}
                evidence.settled = evidence.dynamicResourceLoad !== "pending"
                    && evidence.sandboxChrome !== "pending"
                    && evidence.sandboxRuntime !== "pending"
                    && evidence.sandboxStorageLocal !== "pending"
                    && evidence.sandboxOrigin !== "pending";
                void api.storage.local.set({{ zephiumBitwardenContentApiProbe: evidence }});
                return;
            }}
            polls += 1;
            setTimeout(settle, 25);
        }};
        settle();
    }}, {{ once: true }});
}})();"#
    )
}

pub(super) fn probe_script() -> String {
    const TEMPLATE: &str = r#"(() => {
    const api = globalThis.browser ?? globalThis.chrome;
    const settle = (value) => { document.title = JSON.stringify(value); };
    let polls = 0;
    const poll = () => Promise.all([
        api?.storage?.local?.get("zephiumBitwardenBackgroundApiProbe"),
        api?.storage?.local?.get("zephiumBitwardenContentApiProbe")
    ]).then(([backgroundStored, contentStored]) => {
        const background = backgroundStored?.zephiumBitwardenBackgroundApiProbe;
        const content = contentStored?.zephiumBitwardenContentApiProbe;
        const ready = !!background && !!content && content.settled === true
            && background.executeScript !== "pending"
            && background.webNavigationCommitted !== "pending";
        if (ready || polls >= __ZEPHIUM_SETTLE_POLLS__) {
            settle({ background, content, settled: ready });
            return;
        }
        polls += 1;
        setTimeout(poll, 25);
    }, (error) => settle({ error: String(error?.message ?? error) }));
    poll();
})()"#;
    TEMPLATE.replace("__ZEPHIUM_SETTLE_POLLS__", &SETTLE_POLLS.to_string())
}

pub(super) fn validate(evidence: &Value) -> Result<Observation, String> {
    let background = evidence.get("background").and_then(Value::as_object);
    let content = evidence.get("content").and_then(Value::as_object);
    let execution_world_namespace = match (
        background
            .and_then(|value| value.get("executionWorldNamespace"))
            .and_then(Value::as_str),
        background
            .and_then(|value| value.get("mainWorldValue"))
            .and_then(Value::as_str),
    ) {
        (Some("object"), Some("MAIN")) => ExecutionWorldNamespace::Native,
        (Some("undefined"), Some("absent")) => ExecutionWorldNamespace::AdapterRequired,
        _ => {
            return Err(format!(
                "Bitwarden scripting execution-world contract drifted: {evidence}"
            ))
        }
    };
    let dynamic_resource_url = match content
        .and_then(|value| value.get("dynamicResourceUrl"))
        .and_then(Value::as_str)
    {
        Some("opaque") => DynamicResourceUrl::Opaque,
        Some("principal") => DynamicResourceUrl::Principal,
        _ => {
            return Err(format!(
                "Bitwarden dynamic-resource URL contract drifted: {evidence}"
            ))
        }
    };
    let sandbox_isolation = match (
        content
            .and_then(|value| value.get("sandboxChrome"))
            .and_then(Value::as_str),
        content
            .and_then(|value| value.get("sandboxRuntime"))
            .and_then(Value::as_str),
        content
            .and_then(|value| value.get("sandboxStorageLocal"))
            .and_then(Value::as_str),
        content
            .and_then(|value| value.get("sandboxOrigin"))
            .and_then(Value::as_str),
    ) {
        (Some("undefined"), Some("undefined"), Some("undefined"), Some("null")) => {
            SandboxIsolation::Native
        }
        (Some("object"), Some("object"), Some("object"), Some(origin))
            if origin.len() <= 128
                && origin
                    .strip_prefix("webkit-extension://")
                    .is_some_and(|principal| {
                        !principal.is_empty() && !principal.as_bytes().contains(&b'/')
                    }) =>
        {
            SandboxIsolation::AdapterRequired
        }
        _ => {
            return Err(format!(
                "Bitwarden sandbox-page trust-zone contract drifted: {evidence}"
            ))
        }
    };
    let matches = |object: Option<&serde_json::Map<String, Value>>, name: &str, expected: &str| {
        object
            .and_then(|value| value.get(name))
            .and_then(Value::as_str)
            == Some(expected)
    };
    if evidence.get("settled").and_then(Value::as_bool) != Some(true)
        || !matches(background, "messageSenderTab", "present")
        || !matches(background, "executeScript", "fulfilled")
        || !matches(background, "webNavigationCommitted", "observed")
        || !matches(content, "dynamicResourceLoad", "loaded")
        || !matches(content, "dynamicResourceExecution", "executed")
        || !matches(content, "programmaticScript", "executed")
        || content
            .and_then(|value| value.get("settled"))
            .and_then(Value::as_bool)
            != Some(true)
    {
        return Err(format!(
            "Bitwarden browser API behavior contract was not satisfied: {evidence}"
        ));
    }
    Ok(Observation {
        dynamic_resource_url,
        execution_world_namespace,
        sandbox_isolation,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::*;

    #[test]
    fn fixture_exercises_every_behavioral_boundary() {
        let content = product_tab_content_script("\"loaded\"");
        assert!(content.contains("DOMContentLoaded"));
        assert!(content.contains("runtime.getURL(\"fido2-page-script.js\")"));
        assert!(content.contains("runtime.getURL(\"menu-button.html\")"));
        assert!(content.contains("event.source !== sandbox.contentWindow"));
        let probe = probe_script();
        assert!(probe.contains("zephiumBitwardenBackgroundApiProbe"));
        assert!(probe.contains("zephiumBitwardenContentApiProbe"));
        assert!(probe.contains(&format!("polls >= {SETTLE_POLLS}")));
    }

    #[test]
    fn evidence_pins_native_adapter_requirements() {
        let observed = json!({
            "background": {
                "executionWorldNamespace": "undefined",
                "mainWorldValue": "absent",
                "messageSenderTab": "present",
                "executeScript": "fulfilled",
                "webNavigationCommitted": "observed"
            },
            "content": {
                "dynamicResourceLoad": "loaded",
                "dynamicResourceExecution": "executed",
                "dynamicResourceUrl": "opaque",
                "programmaticScript": "executed",
                "sandboxChrome": "object",
                "sandboxRuntime": "object",
                "sandboxStorageLocal": "object",
                "sandboxOrigin": "webkit-extension://00000000-0000-0000-0000-000000000000",
                "settled": true
            },
            "settled": true
        });
        assert_eq!(
            validate(&observed),
            Ok(Observation {
                dynamic_resource_url: DynamicResourceUrl::Opaque,
                execution_world_namespace: ExecutionWorldNamespace::AdapterRequired,
                sandbox_isolation: SandboxIsolation::AdapterRequired,
            })
        );

        let mut native = observed.clone();
        native["background"]["executionWorldNamespace"] = Value::String("object".to_owned());
        native["background"]["mainWorldValue"] = Value::String("MAIN".to_owned());
        native["content"]["sandboxChrome"] = Value::String("undefined".to_owned());
        native["content"]["sandboxRuntime"] = Value::String("undefined".to_owned());
        native["content"]["sandboxStorageLocal"] = Value::String("undefined".to_owned());
        native["content"]["sandboxOrigin"] = Value::String("null".to_owned());
        assert_eq!(
            validate(&native),
            Ok(Observation {
                dynamic_resource_url: DynamicResourceUrl::Opaque,
                execution_world_namespace: ExecutionWorldNamespace::Native,
                sandbox_isolation: SandboxIsolation::Native,
            })
        );

        let mut ambiguous = observed;
        ambiguous["content"]["sandboxOrigin"] = Value::String("https://example.invalid".to_owned());
        assert!(validate(&ambiguous).is_err());
    }
}
