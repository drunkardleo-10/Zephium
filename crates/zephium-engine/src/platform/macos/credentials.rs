//! On-demand access to macOS browser credential capabilities.
//!
//! WKWebView owns password AutoFill and WebAuthn presentation. Zephium only
//! queries the browser-level passkey authorization state so privileged chrome
//! can explain whether platform credentials are available before offering a
//! user-initiated authorization request. This path allocates no manager and
//! performs no framework call until explicitly queried.

use objc2_authentication_services::{
    ASAuthorizationWebBrowserPublicKeyCredentialManager as NativeManager,
    ASAuthorizationWebBrowserPublicKeyCredentialManagerAuthorizationState as NativeState,
};
use objc2_core_foundation::{CFBoolean, CFString};
use objc2_foundation::MainThreadMarker;
use objc2_security::SecTask;
use std::cell::RefCell;
use std::panic::AssertUnwindSafe;
use std::rc::Rc;
use std::sync::OnceLock;

const WEB_BROWSER_PASSKEY_ENTITLEMENT: &str =
    "com.apple.developer.web-browser.public-key-credential";
static WEB_BROWSER_PASSKEY_ENTITLEMENT_PRESENT: OnceLock<bool> = OnceLock::new();

/// Fail-closed projection of AuthenticationServices' browser authorization.
///
/// `Unknown` preserves forward compatibility without accidentally treating a
/// newly introduced native value as permission to use platform credentials.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MacosPasskeyAuthorizationState {
    Authorized,
    Denied,
    NotDetermined,
    Unknown,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MacosPasskeyAuthorizationRequestFailure {
    WrongThread,
    MissingEntitlement,
    NativeException,
}

/// Reads the current platform-credential authorization without prompting.
///
/// Password AutoFill itself remains WebKit-owned. This function does not read
/// credentials, enumerate relying parties, show UI, or change authorization.
pub fn passkey_authorization_state() -> MacosPasskeyAuthorizationState {
    if !has_web_browser_passkey_entitlement() {
        return MacosPasskeyAuthorizationState::Unavailable;
    }
    objc2::exception::catch(AssertUnwindSafe(|| {
        // SAFETY: a fresh manager is uniquely owned for this single non-atomic
        // property read; no other thread can concurrently mutate this instance.
        let manager = unsafe { NativeManager::new() };
        classify(unsafe { manager.authorizationStateForPlatformCredentials() })
    }))
    .unwrap_or(MacosPasskeyAuthorizationState::Unavailable)
}

/// Requests browser passkey authorization from a trusted, user-initiated main
/// thread action. The native completion is consumed at most once.
pub fn request_passkey_authorization(
    completion: impl FnOnce(MacosPasskeyAuthorizationState) + 'static,
) -> Result<(), MacosPasskeyAuthorizationRequestFailure> {
    MainThreadMarker::new().ok_or(MacosPasskeyAuthorizationRequestFailure::WrongThread)?;
    if !has_web_browser_passkey_entitlement() {
        return Err(MacosPasskeyAuthorizationRequestFailure::MissingEntitlement);
    }
    let completion = Rc::new(RefCell::new(Some(completion)));
    objc2::exception::catch(AssertUnwindSafe(|| {
        // SAFETY: construction and request both occur on the process main
        // thread. The copied block retains the manager until settlement.
        let manager = unsafe { NativeManager::new() };
        let retained_manager = manager.clone();
        let completion_for_block = completion.clone();
        let block: block2::RcBlock<dyn Fn(NativeState)> = block2::RcBlock::new(move |state| {
            let _retained_until_settlement = &retained_manager;
            let callback = completion_for_block
                .try_borrow_mut()
                .ok()
                .and_then(|mut completion| completion.take());
            if let Some(callback) = callback {
                let _ = std::panic::catch_unwind(AssertUnwindSafe(|| callback(classify(state))));
            }
        });
        unsafe { manager.requestAuthorizationForPublicKeyCredentials(&block) };
    }))
    .map_err(|_| MacosPasskeyAuthorizationRequestFailure::NativeException)
}

/// Reads only the current process' signed managed capability.
///
/// Apple restricts arbitrary relying-party WebAuthn in browser apps to this
/// exact entitlement. Absence, a false value, an unexpected Core Foundation
/// type, task lookup failure, or a native exception all fail closed. This does
/// not query credentials, relying parties, keychain data, or user consent.
fn has_web_browser_passkey_entitlement() -> bool {
    *WEB_BROWSER_PASSKEY_ENTITLEMENT_PRESENT.get_or_init(read_web_browser_passkey_entitlement)
}

fn read_web_browser_passkey_entitlement() -> bool {
    objc2::exception::catch(AssertUnwindSafe(|| {
        // SAFETY: SecTask returns one retained representation of the current
        // process. The entitlement query receives an immutable exact key and a
        // null error sink; every returned Core Foundation object is retained.
        let Some(task) = (unsafe { SecTask::from_self(None) }) else {
            return false;
        };
        let key = CFString::from_static_str(WEB_BROWSER_PASSKEY_ENTITLEMENT);
        let Some(value) = (unsafe { task.value_for_entitlement(&key, std::ptr::null_mut()) })
        else {
            return false;
        };
        value
            .downcast::<CFBoolean>()
            .ok()
            .is_some_and(|value| value.value())
    }))
    .unwrap_or(false)
}

const fn classify(state: NativeState) -> MacosPasskeyAuthorizationState {
    if state.0 == NativeState::Authorized.0 {
        MacosPasskeyAuthorizationState::Authorized
    } else if state.0 == NativeState::Denied.0 {
        MacosPasskeyAuthorizationState::Denied
    } else if state.0 == NativeState::NotDetermined.0 {
        MacosPasskeyAuthorizationState::NotDetermined
    } else {
        MacosPasskeyAuthorizationState::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::{
        classify, has_web_browser_passkey_entitlement, passkey_authorization_state,
        MacosPasskeyAuthorizationState, NativeState, WEB_BROWSER_PASSKEY_ENTITLEMENT,
    };

    #[test]
    fn native_authorization_projection_is_exhaustive_and_fail_closed() {
        assert_eq!(
            classify(NativeState::Authorized),
            MacosPasskeyAuthorizationState::Authorized
        );
        assert_eq!(
            classify(NativeState::Denied),
            MacosPasskeyAuthorizationState::Denied
        );
        assert_eq!(
            classify(NativeState::NotDetermined),
            MacosPasskeyAuthorizationState::NotDetermined
        );
        assert_eq!(
            classify(NativeState(isize::MAX)),
            MacosPasskeyAuthorizationState::Unknown
        );
    }

    #[test]
    fn native_manager_returns_a_reviewed_authorization_state_without_prompting() {
        assert_eq!(
            WEB_BROWSER_PASSKEY_ENTITLEMENT,
            "com.apple.developer.web-browser.public-key-credential"
        );
        let state = passkey_authorization_state();
        eprintln!("macOS passkey authorization state: {state:?}");
        if has_web_browser_passkey_entitlement() {
            assert!(matches!(
                state,
                MacosPasskeyAuthorizationState::Authorized
                    | MacosPasskeyAuthorizationState::Denied
                    | MacosPasskeyAuthorizationState::NotDetermined
            ));
        } else {
            assert_eq!(state, MacosPasskeyAuthorizationState::Unavailable);
        }
    }
}
