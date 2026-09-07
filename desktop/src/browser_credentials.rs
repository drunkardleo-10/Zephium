//! On-demand browser credential capability projection.
//!
//! Password AutoFill remains owned by the native web engine. This module owns
//! only the privileged-chrome visibility boundary for browser-level passkey
//! authorization and never enumerates credentials or relying parties.

#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use tauri::{Manager as _, WebviewWindow};
use tauri_specta::Event;

use crate::{authorize, shutdown_started, CallerPolicy};
#[cfg(target_os = "macos")]
use crate::{emit_to_privileged, MAIN_LABEL};

#[cfg(target_os = "macos")]
const EVENT_BROWSER_CREDENTIAL_CAPABILITY: &str = "zephium:browser-credential-capability";
#[cfg(target_os = "macos")]
static PASSKEY_AUTHORIZATION_REQUEST_PENDING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type, Event)]
pub(crate) struct BrowserCredentialCapabilityChanged(BrowserCredentialCapabilityView);

#[derive(Clone, Copy, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
enum BrowserPasskeyAuthorizationView {
    Authorized,
    Denied,
    NotDetermined,
    Unknown,
    Unavailable,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, specta::Type)]
pub(crate) struct BrowserCredentialCapabilityView {
    system_password_autofill: bool,
    passkey_authorization: BrowserPasskeyAuthorizationView,
    can_request_passkey_authorization: bool,
}

#[cfg(target_os = "macos")]
fn capability_view() -> BrowserCredentialCapabilityView {
    capability_view_from_state(zephium_engine::macos_passkey_authorization_state())
}

#[cfg(target_os = "macos")]
fn capability_view_from_state(
    state: zephium_engine::MacosPasskeyAuthorizationState,
) -> BrowserCredentialCapabilityView {
    let passkey_authorization = match state {
        zephium_engine::MacosPasskeyAuthorizationState::Authorized => {
            BrowserPasskeyAuthorizationView::Authorized
        }
        zephium_engine::MacosPasskeyAuthorizationState::Denied => {
            BrowserPasskeyAuthorizationView::Denied
        }
        zephium_engine::MacosPasskeyAuthorizationState::NotDetermined => {
            BrowserPasskeyAuthorizationView::NotDetermined
        }
        zephium_engine::MacosPasskeyAuthorizationState::Unknown => {
            BrowserPasskeyAuthorizationView::Unknown
        }
        zephium_engine::MacosPasskeyAuthorizationState::Unavailable => {
            BrowserPasskeyAuthorizationView::Unavailable
        }
    };
    if !matches!(
        passkey_authorization,
        BrowserPasskeyAuthorizationView::NotDetermined
    ) {
        PASSKEY_AUTHORIZATION_REQUEST_PENDING.store(false, Ordering::Release);
    }
    BrowserCredentialCapabilityView {
        system_password_autofill: true,
        passkey_authorization,
        can_request_passkey_authorization: matches!(
            passkey_authorization,
            BrowserPasskeyAuthorizationView::NotDetermined
        ),
    }
}

#[cfg(not(target_os = "macos"))]
const fn capability_view() -> BrowserCredentialCapabilityView {
    BrowserCredentialCapabilityView {
        system_password_autofill: false,
        passkey_authorization: BrowserPasskeyAuthorizationView::Unsupported,
        can_request_passkey_authorization: false,
    }
}

/// Reads only platform capability state. It never enumerates credentials,
/// relying parties, or extension-owned vault data and never opens native UI.
#[tauri::command]
#[specta::specta]
pub(crate) fn browser_credential_capability(
    caller: WebviewWindow,
) -> BrowserCredentialCapabilityView {
    if !authorize(&caller, CallerPolicy::Main, "browser_credential_capability") {
        return BrowserCredentialCapabilityView {
            system_password_autofill: false,
            passkey_authorization: BrowserPasskeyAuthorizationView::Unavailable,
            can_request_passkey_authorization: false,
        };
    }
    capability_view()
}

/// Admits one user-initiated AuthenticationServices request. Native
/// settlement is projected back only to privileged browser chrome, and a
/// second request cannot overlap the first.
#[tauri::command]
#[specta::specta]
pub(crate) fn browser_passkey_authorization_request(caller: WebviewWindow) -> bool {
    if !authorize(
        &caller,
        CallerPolicy::Main,
        "browser_passkey_authorization_request",
    ) || shutdown_started(caller.app_handle())
    {
        return false;
    }

    #[cfg(not(target_os = "macos"))]
    {
        false
    }

    #[cfg(target_os = "macos")]
    {
        let current = capability_view();
        if !current.can_request_passkey_authorization
            || PASSKEY_AUTHORIZATION_REQUEST_PENDING
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return false;
        }
        let app = caller.app_handle().clone();
        let callback_app = app.clone();
        let scheduled = app.run_on_main_thread(move || {
            let completion_app = callback_app.clone();
            let result = zephium_engine::request_macos_passkey_authorization(move |state| {
                PASSKEY_AUTHORIZATION_REQUEST_PENDING.store(false, Ordering::Release);
                let view = capability_view_from_state(state);
                emit_to_privileged(
                    &completion_app,
                    MAIN_LABEL,
                    EVENT_BROWSER_CREDENTIAL_CAPABILITY,
                    &view,
                );
            });
            if result.is_err() {
                PASSKEY_AUTHORIZATION_REQUEST_PENDING.store(false, Ordering::Release);
                let view = BrowserCredentialCapabilityView {
                    system_password_autofill: true,
                    passkey_authorization: BrowserPasskeyAuthorizationView::Unavailable,
                    can_request_passkey_authorization: false,
                };
                emit_to_privileged(
                    &callback_app,
                    MAIN_LABEL,
                    EVENT_BROWSER_CREDENTIAL_CAPABILITY,
                    &view,
                );
            }
        });
        if scheduled.is_err() {
            PASSKEY_AUTHORIZATION_REQUEST_PENDING.store(false, Ordering::Release);
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn commands_are_main_only_on_demand_and_single_flight() {
        let source = include_str!("browser_credentials.rs");
        let query = source
            .split("fn browser_credential_capability(")
            .nth(1)
            .expect("browser credential query")
            .split("#[tauri::command]")
            .next()
            .expect("bounded browser credential query");
        assert!(query.contains("CallerPolicy::Main"));
        assert!(!query.contains("platformCredentialsForRelyingParty"));

        let request = source
            .split("fn browser_passkey_authorization_request(")
            .nth(1)
            .expect("browser passkey request")
            .split("#[cfg(test)]")
            .next()
            .expect("bounded browser passkey request");
        for required in [
            "CallerPolicy::Main",
            "shutdown_started",
            "PASSKEY_AUTHORIZATION_REQUEST_PENDING",
            "compare_exchange(false, true",
            "run_on_main_thread",
            "request_macos_passkey_authorization",
            "MAIN_LABEL",
            "EVENT_BROWSER_CREDENTIAL_CAPABILITY",
        ] {
            assert!(
                request.contains(required),
                "browser passkey request lost invariant: {required}"
            );
        }
        assert!(!request.contains("platformCredentialsForRelyingParty"));
    }
}
