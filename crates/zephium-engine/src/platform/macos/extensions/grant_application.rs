//! Exact native application and readback of one compiled macOS grant set.
//!
//! This module performs no grant decisions. It accepts only the output of the
//! pure compiler in `grants` (or the native probe's bounded equivalent),
//! replaces all four WebKit permission dictionaries synchronously on the main
//! thread, and accepts the mutation only after exact readback. Any partial
//! application is cleared and re-read; final retirement additionally requires
//! the context to be unloaded before the caller may claim absence.

#![cfg_attr(not(feature = "native-web-extension-probes"), allow(dead_code))]

use std::error::Error;
use std::ffi::{c_char, c_void};
use std::fmt;
use std::panic::AssertUnwindSafe;
use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2_foundation::{NSDate, NSDictionary, NSString};
use objc2_web_kit::{
    WKWebExtensionContext, WKWebExtensionContextPermissionStatus, WKWebExtensionMatchPattern,
    WKWebExtensionPermission,
};

use super::grants::{MacosNativeApiPermission, MacosNativeGrantPlan};

unsafe extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

const MAX_NATIVE_PERMISSION_ENTRIES: usize =
    zephium_core::extensions::MAX_EXTENSION_API_PERMISSIONS;
const MAX_NATIVE_PATTERN_ENTRIES: usize = zephium_core::extensions::MAX_EXTENSION_HOST_GRANTS * 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MacosGrantApplicationError {
    MainThreadRequired,
    ContextAlreadyLoaded,
    EntryLimitExceeded,
    NativeStateLimitExceeded,
    MissingPermissionSymbol,
    InvalidPattern,
    DuplicateEntry,
    NativeException,
    AppliedReadbackMismatch,
    RollbackReadbackMismatch,
}

impl fmt::Display for MacosGrantApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::MainThreadRequired => {
                "macOS extension grants must be mutated on the application main thread"
            }
            Self::ContextAlreadyLoaded => {
                "macOS extension grants may be erased only while the context is unloaded"
            }
            Self::EntryLimitExceeded => "macOS extension grant application exceeded its bound",
            Self::NativeStateLimitExceeded => {
                "existing macOS extension permission state exceeded its readback bound"
            }
            Self::MissingPermissionSymbol => {
                "the admitted WebKit runtime omitted a required permission symbol"
            }
            Self::InvalidPattern => "WebKit rejected a compiled extension host pattern",
            Self::DuplicateEntry => "the compiled macOS extension grant set contains duplicates",
            Self::NativeException => "WebKit raised an exception while applying extension grants",
            Self::AppliedReadbackMismatch => {
                "WebKit extension grant readback differed from the complete applied set"
            }
            Self::RollbackReadbackMismatch => {
                "WebKit extension grants could not be verified empty after rollback"
            }
        })
    }
}

impl Error for MacosGrantApplicationError {}

/// Exact resolved keys retained across apply, load, and later retirement.
///
/// The values are bounded native objects only; authority remains in the
/// reservation and Store grant snapshot that produced the compiled plan.
pub(crate) struct AppliedMacosGrantSet {
    permissions: Box<[Retained<NSString>]>,
    patterns: Box<[Retained<WKWebExtensionMatchPattern>]>,
}

impl AppliedMacosGrantSet {
    pub(crate) fn clear_and_verify(
        &self,
        context: &WKWebExtensionContext,
    ) -> Result<(), MacosGrantApplicationError> {
        if unsafe { context.isLoaded() } {
            return Err(MacosGrantApplicationError::ContextAlreadyLoaded);
        }
        clear_permission_state(context)?;
        verify_empty(context, &self.permissions, &self.patterns)
            .map_err(|_| MacosGrantApplicationError::RollbackReadbackMismatch)
    }
}

#[allow(dead_code)] // Consumed by the feature-gated native adapter slice.
pub(super) fn apply_compiled_grants(
    context: &WKWebExtensionContext,
    plan: &MacosNativeGrantPlan,
) -> Result<AppliedMacosGrantSet, MacosGrantApplicationError> {
    let permissions = plan.granted_api_permissions().iter().copied();
    // The compiler currently refuses private runtimes, so private-data access
    // is necessarily absent for every admitted production plan.
    apply_exact_grants(context, permissions, plan.granted_host_patterns(), false)
}

#[cfg(feature = "native-web-extension-probes")]
pub(crate) fn apply_probe_grants(
    context: &WKWebExtensionContext,
    permissions: &[MacosNativeApiPermission],
    patterns: &[&str],
    private_data_access: bool,
) -> Result<AppliedMacosGrantSet, MacosGrantApplicationError> {
    apply_exact_grants(
        context,
        permissions.iter().copied(),
        patterns.iter().copied(),
        private_data_access,
    )
}

fn apply_exact_grants<'a>(
    context: &WKWebExtensionContext,
    permissions: impl ExactSizeIterator<Item = MacosNativeApiPermission>,
    patterns: impl ExactSizeIterator<Item = &'a str>,
    private_data_access: bool,
) -> Result<AppliedMacosGrantSet, MacosGrantApplicationError> {
    if permissions.len() > MAX_NATIVE_PERMISSION_ENTRIES
        || patterns.len() > MAX_NATIVE_PATTERN_ENTRIES
    {
        return Err(MacosGrantApplicationError::EntryLimitExceeded);
    }

    let mut resolved_permissions = Vec::with_capacity(permissions.len());
    for permission in permissions {
        let resolved = resolve_permission(permission)?;
        if resolved_permissions
            .iter()
            .any(|existing: &Retained<NSString>| existing.isEqualToString(&resolved))
        {
            return Err(MacosGrantApplicationError::DuplicateEntry);
        }
        resolved_permissions.push(resolved);
    }

    let mtm = objc2_foundation::MainThreadMarker::new()
        .ok_or(MacosGrantApplicationError::MainThreadRequired)?;
    let mut resolved_patterns = Vec::with_capacity(patterns.len());
    for pattern in patterns {
        let pattern = unsafe {
            WKWebExtensionMatchPattern::matchPatternWithString(&NSString::from_str(pattern), mtm)
        }
        .ok_or(MacosGrantApplicationError::InvalidPattern)?;
        if resolved_patterns
            .iter()
            .any(|existing: &Retained<WKWebExtensionMatchPattern>| {
                let existing = unsafe { existing.string() };
                let candidate = unsafe { pattern.string() };
                existing.isEqualToString(&candidate)
            })
        {
            return Err(MacosGrantApplicationError::DuplicateEntry);
        }
        resolved_patterns.push(pattern);
    }

    let applied = apply_and_verify(
        context,
        &resolved_permissions,
        &resolved_patterns,
        private_data_access,
    );
    if let Err(error) = applied {
        let rollback = clear_permission_state(context)
            .and_then(|()| verify_empty(context, &resolved_permissions, &resolved_patterns));
        return match rollback {
            Ok(()) => Err(error),
            Err(_) => Err(MacosGrantApplicationError::RollbackReadbackMismatch),
        };
    }

    Ok(AppliedMacosGrantSet {
        permissions: resolved_permissions.into_boxed_slice(),
        patterns: resolved_patterns.into_boxed_slice(),
    })
}

fn apply_and_verify(
    context: &WKWebExtensionContext,
    permissions: &[Retained<NSString>],
    patterns: &[Retained<WKWebExtensionMatchPattern>],
    private_data_access: bool,
) -> Result<(), MacosGrantApplicationError> {
    let empty_permissions = NSDictionary::<WKWebExtensionPermission, NSDate>::new();
    let empty_patterns = NSDictionary::<WKWebExtensionMatchPattern, NSDate>::new();

    catch_native(|| unsafe {
        // Clear before granting so replacement cannot transiently retain a
        // stale capability from a previous generation.
        context.setGrantedPermissions(&empty_permissions);
        context.setDeniedPermissions(&empty_permissions);
        context.setGrantedPermissionMatchPatterns(&empty_patterns);
        context.setDeniedPermissionMatchPatterns(&empty_patterns);
        context.setHasRequestedOptionalAccessToAllHosts(false);
        context.setHasAccessToPrivateData(private_data_access);
        // WebKit's bulk dictionary setters restore persisted state, but they
        // do not recompute live content-script eligibility. The status APIs
        // update the same dictionaries and notify the runtime, so the exact
        // replacement must use them entry by entry after the bounded clear.
        for permission in permissions {
            context.setPermissionStatus_forPermission(
                WKWebExtensionContextPermissionStatus::GrantedExplicitly,
                permission,
            );
        }
        for pattern in patterns {
            context.setPermissionStatus_forMatchPattern(
                WKWebExtensionContextPermissionStatus::GrantedExplicitly,
                pattern,
            );
        }
        Ok(())
    })?;

    verify_applied(context, permissions, patterns, private_data_access)
}

fn clear_permission_state(
    context: &WKWebExtensionContext,
) -> Result<(), MacosGrantApplicationError> {
    let empty_permissions = NSDictionary::<WKWebExtensionPermission, NSDate>::new();
    let empty_patterns = NSDictionary::<WKWebExtensionMatchPattern, NSDate>::new();
    catch_native(|| unsafe {
        let granted_permissions = context.grantedPermissions();
        let denied_permissions = context.deniedPermissions();
        let granted_patterns = context.grantedPermissionMatchPatterns();
        let denied_patterns = context.deniedPermissionMatchPatterns();
        let permission_count = granted_permissions
            .count()
            .checked_add(denied_permissions.count())
            .ok_or(MacosGrantApplicationError::NativeStateLimitExceeded)?;
        let pattern_count = granted_patterns
            .count()
            .checked_add(denied_patterns.count())
            .ok_or(MacosGrantApplicationError::NativeStateLimitExceeded)?;
        if permission_count > MAX_NATIVE_PERMISSION_ENTRIES
            || pattern_count > MAX_NATIVE_PATTERN_ENTRIES
        {
            return Err(MacosGrantApplicationError::NativeStateLimitExceeded);
        }

        // Snapshot bounded keys before mutation. Status setters are required
        // to notify a loaded runtime that old capabilities were revoked; the
        // bulk empty dictionaries then canonicalize persisted readback.
        let granted_permission_keys = granted_permissions.allKeys();
        let denied_permission_keys = denied_permissions.allKeys();
        let granted_pattern_keys = granted_patterns.allKeys();
        let denied_pattern_keys = denied_patterns.allKeys();
        for permission in granted_permission_keys
            .iter()
            .chain(denied_permission_keys.iter())
        {
            context.setPermissionStatus_forPermission(
                WKWebExtensionContextPermissionStatus::Unknown,
                &permission,
            );
        }
        for pattern in granted_pattern_keys
            .iter()
            .chain(denied_pattern_keys.iter())
        {
            context.setPermissionStatus_forMatchPattern(
                WKWebExtensionContextPermissionStatus::Unknown,
                &pattern,
            );
        }
        context.setGrantedPermissions(&empty_permissions);
        context.setDeniedPermissions(&empty_permissions);
        context.setGrantedPermissionMatchPatterns(&empty_patterns);
        context.setDeniedPermissionMatchPatterns(&empty_patterns);
        context.setHasRequestedOptionalAccessToAllHosts(false);
        context.setHasAccessToPrivateData(false);
        Ok(())
    })
}

fn verify_applied(
    context: &WKWebExtensionContext,
    permissions: &[Retained<NSString>],
    patterns: &[Retained<WKWebExtensionMatchPattern>],
    private_data_access: bool,
) -> Result<(), MacosGrantApplicationError> {
    let exact = catch_native(|| unsafe {
        let granted_permissions = context.grantedPermissions();
        let denied_permissions = context.deniedPermissions();
        let granted_patterns = context.grantedPermissionMatchPatterns();
        let denied_patterns = context.deniedPermissionMatchPatterns();
        Ok(granted_permissions.count() == permissions.len()
            && denied_permissions.count() == 0
            && granted_patterns.count() == patterns.len()
            && denied_patterns.count() == 0
            && !context.hasRequestedOptionalAccessToAllHosts()
            && context.hasAccessToPrivateData() == private_data_access
            && permissions.iter().all(|permission| {
                granted_permissions.objectForKey(permission).is_some()
                    && context.permissionStatusForPermission(permission)
                        == WKWebExtensionContextPermissionStatus::GrantedExplicitly
            })
            && patterns.iter().all(|pattern| {
                granted_patterns.objectForKey(pattern).is_some()
                    && context.permissionStatusForMatchPattern(pattern)
                        == WKWebExtensionContextPermissionStatus::GrantedExplicitly
            }))
    })?;
    if exact {
        Ok(())
    } else {
        Err(MacosGrantApplicationError::AppliedReadbackMismatch)
    }
}

fn verify_empty(
    context: &WKWebExtensionContext,
    permissions: &[Retained<NSString>],
    patterns: &[Retained<WKWebExtensionMatchPattern>],
) -> Result<(), MacosGrantApplicationError> {
    let exact = catch_native(|| unsafe {
        Ok(context.grantedPermissions().count() == 0
            && context.deniedPermissions().count() == 0
            && context.grantedPermissionMatchPatterns().count() == 0
            && context.deniedPermissionMatchPatterns().count() == 0
            && !context.hasRequestedOptionalAccessToAllHosts()
            && !context.hasAccessToPrivateData()
            && permissions.iter().all(|permission| {
                !matches!(
                    context.permissionStatusForPermission(permission),
                    WKWebExtensionContextPermissionStatus::GrantedExplicitly
                        | WKWebExtensionContextPermissionStatus::GrantedImplicitly
                )
            })
            && patterns.iter().all(|pattern| {
                !matches!(
                    context.permissionStatusForMatchPattern(pattern),
                    WKWebExtensionContextPermissionStatus::GrantedExplicitly
                        | WKWebExtensionContextPermissionStatus::GrantedImplicitly
                )
            }))
    })?;
    if exact {
        Ok(())
    } else {
        Err(MacosGrantApplicationError::RollbackReadbackMismatch)
    }
}

fn catch_native<T>(
    operation: impl FnOnce() -> Result<T, MacosGrantApplicationError>,
) -> Result<T, MacosGrantApplicationError> {
    objc2::exception::catch(AssertUnwindSafe(operation))
        .map_err(|_| MacosGrantApplicationError::NativeException)?
}

fn resolve_permission(
    permission: MacosNativeApiPermission,
) -> Result<Retained<NSString>, MacosGrantApplicationError> {
    let symbol = match permission {
        MacosNativeApiPermission::ActiveTab => b"WKWebExtensionPermissionActiveTab\0".as_slice(),
        MacosNativeApiPermission::Alarms => b"WKWebExtensionPermissionAlarms\0".as_slice(),
        MacosNativeApiPermission::ClipboardWrite => {
            b"WKWebExtensionPermissionClipboardWrite\0".as_slice()
        }
        MacosNativeApiPermission::ContextMenus => {
            b"WKWebExtensionPermissionContextMenus\0".as_slice()
        }
        MacosNativeApiPermission::Cookies => b"WKWebExtensionPermissionCookies\0".as_slice(),
        MacosNativeApiPermission::DeclarativeNetRequest => {
            b"WKWebExtensionPermissionDeclarativeNetRequest\0".as_slice()
        }
        MacosNativeApiPermission::DeclarativeNetRequestFeedback => {
            b"WKWebExtensionPermissionDeclarativeNetRequestFeedback\0".as_slice()
        }
        MacosNativeApiPermission::DeclarativeNetRequestWithHostAccess => {
            b"WKWebExtensionPermissionDeclarativeNetRequestWithHostAccess\0".as_slice()
        }
        MacosNativeApiPermission::Menus => b"WKWebExtensionPermissionMenus\0".as_slice(),
        MacosNativeApiPermission::Scripting => b"WKWebExtensionPermissionScripting\0".as_slice(),
        MacosNativeApiPermission::Storage => b"WKWebExtensionPermissionStorage\0".as_slice(),
        MacosNativeApiPermission::Tabs => b"WKWebExtensionPermissionTabs\0".as_slice(),
        MacosNativeApiPermission::WebNavigation => {
            b"WKWebExtensionPermissionWebNavigation\0".as_slice()
        }
        MacosNativeApiPermission::WebRequest => b"WKWebExtensionPermissionWebRequest\0".as_slice(),
    };
    // Darwin defines RTLD_DEFAULT as `(void *)-2`; data-symbol lookup returns
    // a pointer to the exported Objective-C object pointer. Dynamic lookup is
    // mandatory because Zephium still admits macOS releases predating this API.
    let address = unsafe { dlsym((-2_isize) as *mut c_void, symbol.as_ptr().cast()) };
    let slot = NonNull::new(address.cast::<*const NSString>())
        .ok_or(MacosGrantApplicationError::MissingPermissionSymbol)?;
    let object = unsafe { slot.as_ptr().read() };
    let object = NonNull::new(object.cast_mut())
        .ok_or(MacosGrantApplicationError::MissingPermissionSymbol)?;
    unsafe { Retained::retain(object.as_ptr()) }
        .ok_or(MacosGrantApplicationError::MissingPermissionSymbol)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_compiled_permission_has_one_bounded_dynamic_symbol() {
        let permissions = [
            MacosNativeApiPermission::ActiveTab,
            MacosNativeApiPermission::Alarms,
            MacosNativeApiPermission::ClipboardWrite,
            MacosNativeApiPermission::ContextMenus,
            MacosNativeApiPermission::Cookies,
            MacosNativeApiPermission::DeclarativeNetRequest,
            MacosNativeApiPermission::DeclarativeNetRequestFeedback,
            MacosNativeApiPermission::DeclarativeNetRequestWithHostAccess,
            MacosNativeApiPermission::Menus,
            MacosNativeApiPermission::Scripting,
            MacosNativeApiPermission::Storage,
            MacosNativeApiPermission::Tabs,
            MacosNativeApiPermission::WebNavigation,
            MacosNativeApiPermission::WebRequest,
        ];
        assert_eq!(permissions.len(), 14);
        assert!(permissions.len() <= MAX_NATIVE_PERMISSION_ENTRIES);
        for permission in permissions {
            assert!(permission.as_str().len() <= 64);
            assert!(permission
                .as_str()
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric()));
        }
    }
}
