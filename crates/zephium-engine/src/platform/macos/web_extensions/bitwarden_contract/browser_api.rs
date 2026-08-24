//! Behavioral evidence for browser APIs used by the pinned Bitwarden build.
//!
//! Parser acceptance and namespace presence are insufficient. This fixture
//! crosses the exact background/content/product-tab boundary and classifies
//! native behavior without embedding any upstream Bitwarden artifact.

use std::path::Path;

use serde_json::Value;

pub(super) const PROBE_TITLE: &str = "zephium-browser-api-pending";
const SETTLE_POLLS: u16 = 240;
const EXPECTED_COMMAND_NAMES: [&str; 6] = [
    "_execute_action",
    "autofill_card",
    "autofill_identity",
    "autofill_login",
    "generate_password",
    "lock_vault",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DynamicResourceUrl {
    Opaque,
    Principal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExecutionWorldNamespace {
    Native,
    LiteralMainOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SandboxIsolation {
    SealedBlob,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimePortEarlyConnect {
    DeliveredAfterRegistration,
    RejectedNoListener,
    DisconnectedNoListener,
    DisconnectedWithoutDiagnostic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Observation {
    dynamic_resource_url: DynamicResourceUrl,
    execution_world_namespace: ExecutionWorldNamespace,
    sandbox_isolation: SandboxIsolation,
    runtime_port_early_connect: RuntimePortEarlyConnect,
}

#[derive(Clone, Copy)]
enum ContextMenuPhase {
    NativeInspection,
    NativeClicked,
    Cleanup,
}

impl ContextMenuPhase {
    const fn expected_lifecycle(self) -> &'static str {
        match self {
            Self::NativeInspection | Self::NativeClicked => "held",
            Self::Cleanup => "removed",
        }
    }

    const fn expected_click(self) -> &'static str {
        match self {
            Self::NativeInspection => "pending",
            Self::NativeClicked | Self::Cleanup => "clicked",
        }
    }
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
            ExecutionWorldNamespace::LiteralMainOnly => "literal-main-only",
        }
    }

    pub(super) const fn sandbox_isolation(self) -> &'static str {
        match self.sandbox_isolation {
            SandboxIsolation::SealedBlob => "sealed-blob",
        }
    }

    pub(super) const fn runtime_port_early_connect(self) -> &'static str {
        match self.runtime_port_early_connect {
            RuntimePortEarlyConnect::DeliveredAfterRegistration => "delivered-after-registration",
            RuntimePortEarlyConnect::RejectedNoListener => "rejected-no-listener",
            RuntimePortEarlyConnect::DisconnectedNoListener => "disconnected-no-listener",
            RuntimePortEarlyConnect::DisconnectedWithoutDiagnostic => {
                "disconnected-without-diagnostic"
            }
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
            "private-page-script.js",
            "document.documentElement.dataset.zephiumBitwardenPrivateResource = 'executed';",
        ),
        (
            "menu-button.html",
            "<!doctype html><title>unused privileged leaf</title>",
        ),
        (
            "menu-button.payload",
            "<!doctype html><title>button</title><script>parent.postMessage({ kind: 'zephium-bitwarden-sandbox', chromeType: typeof globalThis.chrome, runtimeType: typeof globalThis.chrome?.runtime, storageLocalType: typeof globalThis.chrome?.storage?.local, locationOrigin: location.origin }, '*');</script>",
        ),
        (
            "browser-api-probe.html",
            "<!doctype html><meta charset=\"utf-8\"><title>zephium-browser-api-pending</title><script src=\"browser-api-probe.js\"></script>",
        ),
        (
            "scripting-page-script.js",
            "document.documentElement.dataset.zephiumBitwardenProgrammaticScript = 'executed'; history.pushState(null, '', `${location.pathname}?zephium-same-document=1`);",
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
    // This complete object crosses the bounded document-title probe channel;
    // compact keys keep hostile-path evidence below WebKit's 1,000-character
    // title ceiling without dropping a security assertion.
    const evidence = {{
        dLoad: "pending",
        dExec: "pending",
        dUrl: "pending",
        pLoad: "pending",
        pExec: "pending",
        prog: "pending",
        sChrome: "pending",
        sRuntime: "pending",
        sStorage: "pending",
        sOrigin: "pending",
        sLocation: "pending",
        sDom: "pending",
        sLeaf: "not-exposed",
        sTransport: "pending",
        settled: false
    }};
    addEventListener("DOMContentLoaded", () => {{
        document.title = {title};
        const dynamicUrl = api.runtime.getURL("fido2-page-script.js");
        const parsed = new URL(dynamicUrl);
        evidence.dUrl = parsed.host === api.runtime.id ? "principal" : "opaque";

        const script = document.createElement("script");
        script.src = dynamicUrl;
        script.addEventListener("load", () => {{ evidence.dLoad = "loaded"; }}, {{ once: true }});
        script.addEventListener("error", () => {{ evidence.dLoad = "rejected"; }}, {{ once: true }});
        document.documentElement.append(script);

        const privateScript = document.createElement("script");
        privateScript.src = api.runtime.getURL("private-page-script.js");
        privateScript.addEventListener("load", () => {{ evidence.pLoad = "loaded"; }}, {{ once: true }});
        privateScript.addEventListener("error", () => {{ evidence.pLoad = "rejected"; }}, {{ once: true }});
        document.documentElement.append(privateScript);

        const sandbox = document.createElement("iframe");
        sandbox.setAttribute("sandbox", "allow-scripts");
        addEventListener("message", (event) => {{
            if (event.source === sandbox.contentWindow
                && event.data?.kind === "zephium-bitwarden-sandbox") {{
                evidence.sChrome = event.data.chromeType;
                evidence.sRuntime = event.data.runtimeType;
                evidence.sStorage = event.data.storageLocalType;
                evidence.sOrigin = event.origin;
                evidence.sLocation = event.data.locationOrigin;
                try {{
                    evidence.sDom = sandbox.contentDocument ? "present" : "absent";
                }} catch (_) {{
                    evidence.sDom = "denied";
                }}
                evidence.sTransport = "sandbox-message";
                return;
            }}
        }});
        sandbox.hidden = true;
        void (async () => {{
            try {{
                const response = await fetch(api.runtime.getURL("menu-button.payload"));
                if (!response.ok) throw new Error(`leaf-http-${{response.status}}`);
                const leafUrl = URL.createObjectURL(
                    new Blob([await response.text()], {{ type: "text/html" }})
                );
                sandbox.addEventListener("load", () => URL.revokeObjectURL(leafUrl), {{ once: true }});
                sandbox.src = leafUrl;
                document.documentElement.append(sandbox);
                evidence.sTransport = "blob-appended";
            }} catch (error) {{
                evidence.sTransport = `error:${{String(error?.message ?? error).slice(0, 80)}}`;
            }}
        }})();

        try {{
            const request = api.runtime.sendMessage({{ type: "zephium-bitwarden-programmatic-script" }});
            request?.catch?.(() => {{ evidence.prog = "request-rejected"; }});
        }} catch (_) {{
            evidence.prog = "request-rejected";
        }}

        let polls = 0;
        const settle = () => {{
            evidence.dExec =
                document.documentElement.dataset.zephiumBitwardenDynamicResource ?? "pending";
            evidence.pExec =
                document.documentElement.dataset.zephiumBitwardenPrivateResource ?? "pending";
            evidence.prog =
                document.documentElement.dataset.zephiumBitwardenProgrammaticScript
                ?? evidence.prog;
            if (polls >= 40) {{
                for (const key of [
                    "sChrome", "sRuntime", "sStorage", "sOrigin", "sLocation", "sDom"
                ]) {{
                    if (evidence[key] === "pending") evidence[key] = "unobserved";
                }}
                if (evidence.sTransport === "pending") evidence.sTransport = "timeout";
            }}
            const complete = evidence.dLoad !== "pending"
                && evidence.dExec !== "pending"
                && evidence.pLoad !== "pending"
                && evidence.prog !== "pending"
                && evidence.sChrome !== "pending"
                && evidence.sRuntime !== "pending"
                && evidence.sStorage !== "pending"
                && evidence.sOrigin !== "pending"
                && evidence.sLocation !== "pending"
                && evidence.sDom !== "pending"
                && evidence.sLeaf !== "pending"
                && evidence.sTransport !== "pending";
            if (complete || polls >= {SETTLE_POLLS}) {{
                if (evidence.dExec === "pending") {{
                    evidence.dExec = "not-executed";
                }}
                if (evidence.pExec === "pending") {{
                    evidence.pExec = "not-executed";
                }}
                if (evidence.prog === "pending") {{
                    evidence.prog = "not-executed";
                }}
                evidence.settled = evidence.dLoad !== "pending"
                    && evidence.pLoad !== "pending"
                    && evidence.sChrome !== "pending"
                    && evidence.sRuntime !== "pending"
                    && evidence.sStorage !== "pending"
                    && evidence.sOrigin !== "pending"
                    && evidence.sLocation !== "pending"
                    && evidence.sDom !== "pending"
                    && evidence.sLeaf !== "pending"
                    && evidence.sTransport !== "pending";
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
    const cleanup = new URLSearchParams(location.search).get("cleanup") === "context-menu";
    const requireClick = new URLSearchParams(location.search).get("clicked") === "context-menu";
    let cleanupRequested = false;
    let runtimePortRequested = false;
    let polls = 0;
    const poll = () => {
        if (cleanup && !cleanupRequested) {
            cleanupRequested = true;
            try {
                const request = api.runtime.sendMessage({ type: "zephium-bitwarden-context-menu-cleanup" });
                request?.catch?.((error) => settle({ error: String(error?.message ?? error) }));
            } catch (error) {
                settle({ error: String(error?.message ?? error) });
                return;
            }
        }
        Promise.all([
        api?.storage?.local?.get("zephiumBitwardenBackgroundApiProbe"),
        api?.storage?.local?.get("zephiumBitwardenContentApiProbe")
    ]).then(([backgroundStored, contentStored]) => {
        const background = backgroundStored?.zephiumBitwardenBackgroundApiProbe;
        const content = contentStored?.zephiumBitwardenContentApiProbe;
        const contextMenuReady = cleanup
            ? background?.menuLife === "removed"
            : background?.menuLife === "held";
        const contextMenuClickReady = !requireClick
            || background?.menuClick === "clicked";
        if (!cleanup && !runtimePortRequested && !!background) {
            runtimePortRequested = true;
            try {
                const port = api.runtime.connect({ name: "zephium-bitwarden-registered-port" });
                port.onMessage.addListener(() => {});
                port.postMessage({ kind: "zephium-bitwarden-port-ping" });
            } catch (error) {
                settle({ error: `registered-port:${String(error?.message ?? error)}` });
                return;
            }
        }
        const earlyPortSettled = !["pending", "returned"].includes(background?.runtimePortEarly);
        const ready = !!background && !!content && content.settled === true
            && background.executeScript !== "pending"
            && background.webNavigationCommitted !== "pending"
            && background.alarmsLifecycle !== "pending"
            && background.commandsReadback !== "pending"
            && background.commandDispatch !== "pending"
            && background.runtimePortRegistered === "round-trip"
            && earlyPortSettled
            && contextMenuReady
            && contextMenuClickReady;
        if (ready || polls >= __ZEPHIUM_SETTLE_POLLS__) {
            settle({ background, content, settled: ready });
            return;
        }
        polls += 1;
        setTimeout(poll, 25);
    }, (error) => settle({ error: String(error?.message ?? error) }));
    };
    poll();
})()"#;
    TEMPLATE.replace("__ZEPHIUM_SETTLE_POLLS__", &SETTLE_POLLS.to_string())
}

pub(super) fn validate_for_native_inspection(evidence: &Value) -> Result<Observation, String> {
    validate(evidence, ContextMenuPhase::NativeInspection)
}

pub(super) fn validate_after_native_click(evidence: &Value) -> Result<Observation, String> {
    validate(evidence, ContextMenuPhase::NativeClicked)
}

pub(super) fn validate_after_cleanup(evidence: &Value) -> Result<Observation, String> {
    validate(evidence, ContextMenuPhase::Cleanup)
}

fn validate(evidence: &Value, context_menu_phase: ContextMenuPhase) -> Result<Observation, String> {
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
        // The same behavioral gate requires the exact programmatic script to
        // have executed in the product page below. An absent enum therefore
        // means WebKit supports the literal protocol value, not that MAIN-world
        // execution itself is unavailable.
        (Some("undefined"), Some("absent")) => ExecutionWorldNamespace::LiteralMainOnly,
        _ => {
            return Err(format!(
                "Bitwarden scripting execution-world contract drifted: {evidence}"
            ))
        }
    };
    let dynamic_resource_url = match content
        .and_then(|value| value.get("dUrl"))
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
            .and_then(|value| value.get("sChrome"))
            .and_then(Value::as_str),
        content
            .and_then(|value| value.get("sRuntime"))
            .and_then(Value::as_str),
        content
            .and_then(|value| value.get("sStorage"))
            .and_then(Value::as_str),
        content
            .and_then(|value| value.get("sOrigin"))
            .and_then(Value::as_str),
        content
            .and_then(|value| value.get("sLeaf"))
            .and_then(Value::as_str),
        content
            .and_then(|value| value.get("sTransport"))
            .and_then(Value::as_str),
        content
            .and_then(|value| value.get("sDom"))
            .and_then(Value::as_str),
    ) {
        (
            Some("undefined"),
            Some("undefined"),
            Some("undefined"),
            Some("null"),
            Some("not-exposed"),
            Some("sandbox-message"),
            Some("absent" | "denied"),
        ) => SandboxIsolation::SealedBlob,
        _ => {
            return Err(format!(
                "Bitwarden sandbox-page trust-zone contract drifted: {evidence}"
            ))
        }
    };
    let runtime_port_early_connect = match background
        .and_then(|value| value.get("runtimePortEarly"))
        .and_then(Value::as_str)
    {
        Some("delivered-after-registration") => RuntimePortEarlyConnect::DeliveredAfterRegistration,
        Some("rejected-no-listener") => RuntimePortEarlyConnect::RejectedNoListener,
        Some("disconnected-no-listener") => RuntimePortEarlyConnect::DisconnectedNoListener,
        Some("disconnected-without-diagnostic") => {
            RuntimePortEarlyConnect::DisconnectedWithoutDiagnostic
        }
        _ => {
            return Err(format!(
                "Bitwarden early runtime-port contract drifted: {evidence}"
            ))
        }
    };
    let sandbox_location_origin = content
        .and_then(|value| value.get("sLocation"))
        .and_then(Value::as_str)
        .filter(|origin| origin.len() <= 2_048)
        .and_then(|origin| url::Url::parse(origin).ok());
    if sandbox_location_origin.is_none() {
        return Err(format!(
            "Bitwarden sandbox location-origin diagnostic drifted: {evidence}"
        ));
    }
    let matches = |object: Option<&serde_json::Map<String, Value>>, name: &str, expected: &str| {
        object
            .and_then(|value| value.get(name))
            .and_then(Value::as_str)
            == Some(expected)
    };
    let command_names = background
        .and_then(|value| value.get("commandNames"))
        .and_then(Value::as_array)
        .and_then(|names| names.iter().map(Value::as_str).collect::<Option<Vec<_>>>());
    if evidence.get("settled").and_then(Value::as_bool) != Some(true)
        || !matches(background, "messageSenderTab", "present")
        || !matches(background, "executeScript", "fulfilled")
        || !matches(background, "webNavigationCommitted", "observed")
        || !matches(background, "alarmsLifecycle", "created-read-cleared")
        || !matches(background, "commandsReadback", "fulfilled")
        || command_names.as_deref() != Some(EXPECTED_COMMAND_NAMES.as_slice())
        || !matches(background, "commandDispatch", "autofill_login")
        || !matches(background, "runtimePortRegistered", "round-trip")
        || !matches(
            background,
            "menuLife",
            context_menu_phase.expected_lifecycle(),
        )
        || !matches(background, "menuClick", context_menu_phase.expected_click())
        || !matches(content, "dLoad", "loaded")
        || !matches(content, "dExec", "executed")
        || !matches(content, "pLoad", "rejected")
        || !matches(content, "pExec", "not-executed")
        || !matches(content, "prog", "executed")
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
        runtime_port_early_connect,
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
        assert!(content.contains("runtime.getURL(\"private-page-script.js\")"));
        assert!(content.contains("fetch(api.runtime.getURL(\"menu-button.payload\"))"));
        assert!(content.contains("event.source === sandbox.contentWindow"));
        assert!(content.contains("sTransport"));
        let probe = probe_script();
        assert!(probe.contains("zephiumBitwardenBackgroundApiProbe"));
        assert!(probe.contains("zephiumBitwardenContentApiProbe"));
        assert!(probe.contains("zephium-bitwarden-registered-port"));
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
                "webNavigationCommitted": "observed",
                "alarmsLifecycle": "created-read-cleared",
                "commandNames": EXPECTED_COMMAND_NAMES,
                "commandsReadback": "fulfilled",
                "commandDispatch": "autofill_login",
                "runtimePortEarly": "disconnected-without-diagnostic",
                "runtimePortRegistered": "round-trip",
                "menuLife": "held",
                "menuClick": "pending"
            },
            "content": {
                "dLoad": "loaded",
                "dExec": "executed",
                "dUrl": "opaque",
                "pLoad": "rejected",
                "pExec": "not-executed",
                "prog": "executed",
                "sChrome": "undefined",
                "sRuntime": "undefined",
                "sStorage": "undefined",
                "sOrigin": "null",
                "sLocation": "https://fixture.invalid",
                "sDom": "absent",
                "sLeaf": "not-exposed",
                "sTransport": "sandbox-message",
                "settled": true
            },
            "settled": true
        });
        assert_eq!(
            validate_for_native_inspection(&observed),
            Ok(Observation {
                dynamic_resource_url: DynamicResourceUrl::Opaque,
                execution_world_namespace: ExecutionWorldNamespace::LiteralMainOnly,
                sandbox_isolation: SandboxIsolation::SealedBlob,
                runtime_port_early_connect: RuntimePortEarlyConnect::DisconnectedWithoutDiagnostic,
            })
        );

        let mut native = observed.clone();
        native["background"]["executionWorldNamespace"] = Value::String("object".to_owned());
        native["background"]["mainWorldValue"] = Value::String("MAIN".to_owned());
        native["content"]["sChrome"] = Value::String("undefined".to_owned());
        native["content"]["sRuntime"] = Value::String("undefined".to_owned());
        native["content"]["sStorage"] = Value::String("undefined".to_owned());
        native["content"]["sOrigin"] = Value::String("null".to_owned());
        native["content"]["sLeaf"] = Value::String("not-exposed".to_owned());
        assert_eq!(
            validate_for_native_inspection(&native),
            Ok(Observation {
                dynamic_resource_url: DynamicResourceUrl::Opaque,
                execution_world_namespace: ExecutionWorldNamespace::Native,
                sandbox_isolation: SandboxIsolation::SealedBlob,
                runtime_port_early_connect: RuntimePortEarlyConnect::DisconnectedWithoutDiagnostic,
            })
        );

        let mut ambiguous = observed;
        ambiguous["content"]["sOrigin"] = Value::String("https://example.invalid".to_owned());
        assert!(validate_for_native_inspection(&ambiguous).is_err());

        let mut cleaned = native;
        cleaned["background"]["menuClick"] = Value::String("clicked".to_owned());
        assert!(validate_after_native_click(&cleaned).is_ok());
        cleaned["background"]["menuLife"] = Value::String("removed".to_owned());
        assert!(validate_after_cleanup(&cleaned).is_ok());
    }
}
