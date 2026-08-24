//! Opt-in long-duration classification of WebKit's native MV3 alarm lifecycle.
//!
//! The ordinary contract gate covers create/read/clear. This module is kept
//! separate because it intentionally spends a real 30-second minimum delay.
//! It proves live delivery, then determines whether an armed alarm survives an
//! exact native context unload/reload without keeping a hidden view or worker.

use std::path::Path;
use std::time::{Duration, Instant};

use objc2_foundation::{MainThreadMarker, NSRunLoop};
use objc2_web_kit::{WKWebExtensionContext, WKWebExtensionController};
use serde_json::Value;

const PENDING_TITLE: &str = "zephium-alarm-delivery-pending";
const DELIVERY_TIMEOUT: Duration = Duration::from_secs(90);
const ALARM_NAME: &str = "zephium-bitwarden-contract-delivery";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RestartClassification {
    Persisted,
    Discarded,
}

pub(super) const BACKGROUND_FRAGMENT: &str = r#"
    const alarmDeliveryName = "zephium-bitwarden-contract-delivery";
    const alarmArmKey = "zephiumBitwardenAlarmArmProbe";
    const alarmDeliveryKey = "zephiumBitwardenAlarmDeliveryProbe";
    if (api?.alarms?.onAlarm && api?.storage?.local && api?.runtime?.onMessage) {
        api.alarms.onAlarm.addListener((alarm) => {
            if (alarm?.name !== alarmDeliveryName) return;
            void api.storage.local.set({
                [alarmDeliveryKey]: {
                    name: alarm.name,
                    scheduledTime: alarm.scheduledTime,
                    firedTime: Date.now()
                }
            });
        });
        api.runtime.onMessage.addListener((message) => {
            if (message?.type !== "zephium-bitwarden-arm-alarm-delivery") return;
            api.alarms.create(alarmDeliveryName, { delayInMinutes: 0.5 });
            api.alarms.get(alarmDeliveryName, (alarm) => {
                const error = api.runtime.lastError;
                if (error || alarm?.name !== alarmDeliveryName
                    || !Number.isFinite(alarm?.scheduledTime)) return;
                void api.storage.local.set({
                    [alarmArmKey]: {
                        name: alarm.name,
                        scheduledTime: alarm.scheduledTime
                    }
                });
            });
        });
    }
"#;

const ARM_SCRIPT: &str = r#"(() => {
    const api = globalThis.browser ?? globalThis.chrome;
    const armKey = "zephiumBitwardenAlarmArmProbe";
    const deliveryKey = "zephiumBitwardenAlarmDeliveryProbe";
    const settle = (value) => { document.title = JSON.stringify(value); };
    if (!api?.runtime?.sendMessage || !api?.storage?.local || !api?.storage?.onChanged) {
        settle({ error: "required alarm probe APIs are absent" });
        return;
    }
    const inspect = (value) => {
        if (value == null || typeof value !== "object") return false;
        settle({ name: value.name, scheduledTime: value.scheduledTime });
        return true;
    };
    api.storage.onChanged.addListener((changes, areaName) => {
        if (areaName === "local") inspect(changes?.[armKey]?.newValue);
    });
    api.storage.local.remove([armKey, deliveryKey], () => {
        const removeError = api.runtime.lastError;
        if (removeError) {
            settle({ error: String(removeError.message ?? removeError) });
            return;
        }
        api.runtime.sendMessage({ type: "zephium-bitwarden-arm-alarm-delivery" }, () => {
            const messageError = api.runtime.lastError;
            if (messageError) settle({ error: String(messageError.message ?? messageError) });
        });
    });
})()"#;

const OBSERVE_SCRIPT: &str = r#"(() => {
    const api = globalThis.browser ?? globalThis.chrome;
    const key = "zephiumBitwardenAlarmDeliveryProbe";
    const settle = (value) => {
        if (value == null || typeof value !== "object") return false;
        document.title = JSON.stringify({
            name: value.name,
            scheduledTime: value.scheduledTime,
            firedTime: value.firedTime
        });
        return true;
    };
    if (!api?.storage?.local || !api?.storage?.onChanged) {
        document.title = JSON.stringify({ error: "required alarm observation APIs are absent" });
        return;
    }
    api.storage.onChanged.addListener((changes, areaName) => {
        if (areaName === "local") settle(changes?.[key]?.newValue);
    });
    api.storage.local.get(key, (stored) => {
        const error = api.runtime.lastError;
        if (error) {
            document.title = JSON.stringify({ error: String(error.message ?? error) });
            return;
        }
        settle(stored?.[key]);
    });
})()"#;

const RESTART_STATE_SCRIPT: &str = r#"(() => {
    const api = globalThis.browser ?? globalThis.chrome;
    const name = "zephium-bitwarden-contract-delivery";
    const deliveryKey = "zephiumBitwardenAlarmDeliveryProbe";
    const settle = (alarm, delivered) => {
        document.title = JSON.stringify({
            name: alarm?.name ?? null,
            scheduledTime: Number.isFinite(alarm?.scheduledTime) ? alarm.scheduledTime : null,
            delivered
        });
    };
    if (!api?.alarms?.get || !api?.storage?.local) {
        document.title = JSON.stringify({ error: "required restart readback APIs are absent" });
        return;
    }
    api.alarms.get(name, (alarm) => {
        const alarmError = api.runtime.lastError;
        if (alarmError) {
            document.title = JSON.stringify({ error: String(alarmError.message ?? alarmError) });
            return;
        }
        api.storage.local.get(deliveryKey, (stored) => {
            const storageError = api.runtime.lastError;
            if (storageError) {
                document.title = JSON.stringify({ error: String(storageError.message ?? storageError) });
                return;
            }
            settle(alarm, stored?.[deliveryKey] != null);
        });
    });
})()"#;

pub(super) fn write_fixture_assets(path: &Path) -> Result<(), String> {
    for (name, script) in [
        ("alarm-delivery-arm.js", ARM_SCRIPT),
        ("alarm-delivery-observe.js", OBSERVE_SCRIPT),
        ("alarm-restart-state.js", RESTART_STATE_SCRIPT),
    ] {
        super::write(path, name, script)?;
    }
    for (name, script) in [
        ("alarm-delivery-arm.html", "alarm-delivery-arm.js"),
        ("alarm-delivery-observe.html", "alarm-delivery-observe.js"),
        ("alarm-restart-state.html", "alarm-restart-state.js"),
    ] {
        super::write(
            path,
            name,
            &format!(
                "<!doctype html><meta charset=\"utf-8\"><title>{PENDING_TITLE}</title><script src=\"{script}\"></script>"
            ),
        )?;
    }
    Ok(())
}

pub(super) fn run(
    context: &WKWebExtensionContext,
    controller: &WKWebExtensionController,
    registry: &mut super::super::super::extensions::PersistentControllerRegistry,
    run_loop: &NSRunLoop,
    mtm: MainThreadMarker,
    context_loaded: &mut bool,
) -> Result<(), String> {
    let live_scheduled = arm(context, run_loop, mtm)?;
    eprintln!("native-probe-bitwarden-alarm-delivery: live-scheduled-time={live_scheduled}");
    observe(context, run_loop, mtm)?;
    eprintln!("native-probe-bitwarden-alarm-delivery: live-worker=passed");

    let restart_scheduled = arm(context, run_loop, mtm)?;
    eprintln!("native-probe-bitwarden-alarm-delivery: restart-scheduled-time={restart_scheduled}");
    let unloaded =
        super::super::unload_context(controller, context, "Bitwarden alarm-delivery restart");
    *context_loaded = unsafe { context.isLoaded() };
    unloaded?;
    registry.refresh_command_monitor().map_err(|error| {
        format!("cannot retire command monitor for alarm-delivery restart: {error}")
    })?;
    let reloaded =
        super::super::load_context(controller, context, "Bitwarden alarm-delivery restart");
    *context_loaded = unsafe { context.isLoaded() };
    reloaded?;
    registry.ensure_command_monitor().map_err(|error| {
        format!("cannot restore command monitor after alarm-delivery restart: {error}")
    })?;
    match classify_restart(context, run_loop, mtm)? {
        RestartClassification::Persisted => {
            observe(context, run_loop, mtm)?;
            eprintln!("native-probe-bitwarden-alarm-delivery: context-restart-persistence=passed");
        }
        RestartClassification::Discarded => eprintln!(
            "native-probe-bitwarden-alarm-delivery: context-restart-persistence=unsupported"
        ),
    }
    Ok(())
}

fn arm(
    context: &WKWebExtensionContext,
    run_loop: &NSRunLoop,
    mtm: MainThreadMarker,
) -> Result<f64, String> {
    let evidence = probe_page(
        context,
        run_loop,
        mtm,
        "alarm-delivery-arm.html",
        "arm",
        super::super::PROBE_TIMEOUT,
    )?;
    let name = evidence.get("name").and_then(Value::as_str);
    let scheduled = evidence.get("scheduledTime").and_then(Value::as_f64);
    if name != Some(ALARM_NAME)
        || scheduled.is_none_or(|value| !value.is_finite() || value <= 0.0)
        || evidence.as_object().is_none_or(|value| value.len() != 2)
    {
        return Err(format!(
            "alarm-delivery arm returned invalid evidence: {evidence}"
        ));
    }
    Ok(scheduled.expect("validated scheduled time is present"))
}

fn observe(
    context: &WKWebExtensionContext,
    run_loop: &NSRunLoop,
    mtm: MainThreadMarker,
) -> Result<(), String> {
    let evidence = probe_page(
        context,
        run_loop,
        mtm,
        "alarm-delivery-observe.html",
        "observation",
        DELIVERY_TIMEOUT,
    )?;
    let name = evidence.get("name").and_then(Value::as_str);
    let scheduled = evidence.get("scheduledTime").and_then(Value::as_f64);
    let fired = evidence.get("firedTime").and_then(Value::as_f64);
    if name != Some(ALARM_NAME)
        || scheduled.is_none_or(|value| !value.is_finite() || value <= 0.0)
        || fired.is_none_or(|value| !value.is_finite() || value <= 0.0)
        || evidence.as_object().is_none_or(|value| value.len() != 3)
    {
        return Err(format!(
            "alarm delivery returned invalid evidence: {evidence}"
        ));
    }
    Ok(())
}

fn classify_restart(
    context: &WKWebExtensionContext,
    run_loop: &NSRunLoop,
    mtm: MainThreadMarker,
) -> Result<RestartClassification, String> {
    let evidence = probe_page(
        context,
        run_loop,
        mtm,
        "alarm-restart-state.html",
        "restart readback",
        super::super::PROBE_TIMEOUT,
    )?;
    if evidence.as_object().is_none_or(|value| value.len() != 3)
        || evidence.get("delivered").and_then(Value::as_bool) != Some(false)
    {
        return Err(format!(
            "alarm restart returned invalid evidence: {evidence}"
        ));
    }
    match (
        evidence.get("name"),
        evidence.get("scheduledTime").and_then(Value::as_f64),
    ) {
        (Some(Value::Null), None) => Ok(RestartClassification::Discarded),
        (Some(Value::String(name)), Some(scheduled))
            if name == ALARM_NAME && scheduled.is_finite() && scheduled > 0.0 =>
        {
            Ok(RestartClassification::Persisted)
        }
        _ => Err(format!(
            "alarm restart returned contradictory evidence: {evidence}"
        )),
    }
}

fn probe_page(
    context: &WKWebExtensionContext,
    run_loop: &NSRunLoop,
    mtm: MainThreadMarker,
    page: &str,
    phase: &str,
    timeout: Duration,
) -> Result<Value, String> {
    let configuration = unsafe { context.webViewConfiguration() }
        .ok_or_else(|| format!("loaded alarm contract returned no {phase} configuration"))?;
    let window = super::super::new_window(mtm)?;
    let host = super::super::profile_isolation::host_for_window(
        &window,
        &format!("Bitwarden alarm {phase}"),
    )?;
    let view = super::super::profile_isolation::build_profile_view(&host, configuration)?;
    window.orderFrontRegardless();
    let page = unsafe { context.baseURL() }
        .URLByAppendingPathComponent(&objc2_foundation::NSString::from_str(page))
        .and_then(|url| url.absoluteString())
        .ok_or_else(|| format!("alarm contract produced no {phase} URL"))?
        .to_string();
    view.load_url(&page)
        .map_err(|error| format!("cannot navigate alarm {phase} probe: {error}"))?;

    let deadline = Instant::now() + timeout;
    let result = loop {
        let title = view
            .document_title()
            .map_err(|error| format!("cannot inspect alarm {phase} title: {error}"))?;
        if let Some(title) = title
            .as_deref()
            .filter(|title| !title.is_empty() && *title != PENDING_TITLE)
        {
            break serde_json::from_str(title).map_err(|error| {
                format!("alarm {phase} returned invalid evidence {title:?}: {error}")
            })?;
        }
        super::super::validate_context_errors(context, &format!("alarm {phase} probe"))?;
        if Instant::now() >= deadline {
            return Err(format!(
                "alarm {phase} probe timed out at {:?}",
                view.url().ok()
            ));
        }
        super::super::drain_run_loop_once(run_loop);
    };
    drop(view);
    window.close();
    drop(window);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_bind_the_minimum_delay_and_exact_storage_protocol() {
        assert!(BACKGROUND_FRAGMENT.contains("delayInMinutes: 0.5"));
        assert!(BACKGROUND_FRAGMENT.contains("zephiumBitwardenAlarmArmProbe"));
        assert!(BACKGROUND_FRAGMENT.contains("zephiumBitwardenAlarmDeliveryProbe"));
        assert!(ARM_SCRIPT.contains("runtime.sendMessage"));
        assert!(ARM_SCRIPT.contains("storage.onChanged.addListener"));
        assert!(OBSERVE_SCRIPT.contains("storage.local.get"));
        assert!(OBSERVE_SCRIPT.contains("storage.onChanged.addListener"));
        assert!(RESTART_STATE_SCRIPT.contains("alarms.get"));
        assert!(RESTART_STATE_SCRIPT.contains("storage.local.get"));
    }
}
