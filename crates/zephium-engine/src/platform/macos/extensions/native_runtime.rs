//! Dormant construction and exact teardown of one native macOS extension.
//!
//! The product factory does not call this module yet. It is the narrow native
//! seam between an authenticated package-root lease, one complete Core grant
//! snapshot, the profile-owned controller, and the host lifecycle registry.
//! Keeping this seam dormant until restart reconciliation is wired prevents a
//! release build from creating ownership it cannot subsequently recover.

// The production entry point and exact-owner comparison remain intentionally
// dormant until host activation, retirement, and restart reconciliation are
// enabled as one lifecycle slice. The native probe consumes the rest.
#![allow(dead_code)]

use std::cell::RefCell;
use std::error::Error;
use std::ffi::{CStr, CString};
use std::fmt;
use std::os::unix::ffi::OsStrExt;
use std::panic::AssertUnwindSafe;
use std::ptr::NonNull;
use std::rc::Rc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_foundation::{MainThreadMarker, NSError, NSProcessInfo, NSString, NSURL};
use objc2_web_kit::{WKWebExtension, WKWebExtensionContext, WKWebExtensionController};
use zephium_core::extensions::ExtensionNativeGrantSnapshot;
use zephium_extension_runtime_api::{
    ExtensionPackageAccessError, ExtensionRuntimeMacosAbsenceAudit, ExtensionRuntimeNativeOwnerId,
    ExtensionRuntimeNativeRootLease, ExtensionRuntimeVisitorError,
};

use super::grant_application::{
    apply_compiled_grants, clear_all_grants_and_verify, AppliedMacosGrantSet,
    ClearedMacosGrantAudit, MacosGrantApplicationError,
};
use super::grants::{compile_native_grant_plan, MacosNativeGrantPlan, MacosNativeGrantPlanError};
#[cfg(feature = "native-web-extension-probes")]
use super::{grant_application::apply_probe_grants, MacosNativeApiPermission};

const MINIMUM_MACOS_MAJOR: isize = 15;
const MINIMUM_MACOS_MINOR: isize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MacosNativeRuntimeFailure {
    UnsupportedRuntime,
    MainThreadRequired,
    PackageRootAccess(ExtensionPackageAccessError),
    PackageRootRejected(ExtensionRuntimeVisitorError),
    GrantPlan(MacosNativeGrantPlanError),
    ExtensionParseFailed,
    ExtensionManifestInvalid,
    NativeException,
    ContextConstructionFailed,
    IdentityReadbackMismatch,
    GrantApplication(MacosGrantApplicationError),
    ControllerLoadFailed,
    LoadedOwnerReadbackMismatch,
    ContextRuntimeError,
    ControllerUnloadFailed,
    GrantCleanup(MacosGrantApplicationError),
    AbsenceReadbackMismatch,
}

impl fmt::Display for MacosNativeRuntimeFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedRuntime => {
                formatter.write_str("the macOS native extension runtime is unavailable")
            }
            Self::MainThreadRequired => {
                formatter.write_str("macOS native extension work requires the main thread")
            }
            Self::PackageRootAccess(error) => {
                write!(formatter, "package root access failed: {error}")
            }
            Self::PackageRootRejected(error) => {
                write!(
                    formatter,
                    "package root was rejected by the native adapter: {error}"
                )
            }
            Self::GrantPlan(error) => write!(formatter, "native grant plan was rejected: {error}"),
            Self::ExtensionParseFailed => {
                formatter.write_str("WebKit could not parse the extension package")
            }
            Self::ExtensionManifestInvalid => {
                formatter.write_str("WebKit rejected the extension manifest")
            }
            Self::NativeException => {
                formatter.write_str("WebKit raised an exception at the native extension boundary")
            }
            Self::ContextConstructionFailed => {
                formatter.write_str("WebKit constructed an invalid extension context")
            }
            Self::IdentityReadbackMismatch => {
                formatter.write_str("WebKit did not retain the exact extension owner identity")
            }
            Self::GrantApplication(error) => {
                write!(
                    formatter,
                    "native extension grants could not be applied: {error}"
                )
            }
            Self::ControllerLoadFailed => {
                formatter.write_str("WebKit could not load the extension context")
            }
            Self::LoadedOwnerReadbackMismatch => {
                formatter.write_str("loaded extension ownership failed exact native readback")
            }
            Self::ContextRuntimeError => {
                formatter.write_str("the loaded extension context reported a runtime error")
            }
            Self::ControllerUnloadFailed => {
                formatter.write_str("WebKit could not unload the extension context")
            }
            Self::GrantCleanup(error) => {
                write!(
                    formatter,
                    "native extension grants could not be cleared: {error}"
                )
            }
            Self::AbsenceReadbackMismatch => {
                formatter.write_str("native extension absence failed exact readback")
            }
        }
    }
}

impl Error for MacosNativeRuntimeFailure {}

/// Typed synchronous refusal from the activation start boundary.
///
/// `RejectedBeforeNative` proves WebKit was never asked to parse the package.
/// `OwnershipUncertain` means the Objective-C call raised after entry; WebKit
/// may have copied the callback or begun work, so lifecycle code must not mint
/// never-entered absence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MacosNativeRuntimeStartFailure {
    RejectedBeforeNative(MacosNativeRuntimeFailure),
    OwnershipUncertain(MacosNativeRuntimeFailure),
}

impl fmt::Display for MacosNativeRuntimeStartFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RejectedBeforeNative(failure) => {
                write!(
                    formatter,
                    "native activation was rejected before entry: {failure}"
                )
            }
            Self::OwnershipUncertain(failure) => {
                write!(
                    formatter,
                    "native activation start is ownership-uncertain: {failure}"
                )
            }
        }
    }
}

impl Error for MacosNativeRuntimeStartFailure {}

/// Settlement of an activation attempt after WebKit has accepted the async
/// parse request.
#[must_use = "native activation settlement owns any possible WebKit owner"]
pub(crate) enum MacosNativeRuntimeActivation {
    Rejected(MacosNativeRuntimeFailure),
    Activated(MacosNativeRuntimeOwner),
    OwnershipUncertain {
        failure: MacosNativeRuntimeFailure,
        owner: MacosNativeRuntimeOwner,
    },
}

/// Settlement of exact owner teardown.
#[must_use = "a retained native owner must remain quarantined until absence is proven"]
pub(crate) enum MacosNativeRuntimeRetirement {
    Absent(ExtensionRuntimeMacosAbsenceAudit),
    Retained {
        failure: MacosNativeRuntimeFailure,
        owner: MacosNativeRuntimeOwner,
    },
}

/// Move-only native objects for one exact catalog-authenticated extension ID.
///
/// This object performs no work in `Drop`: passive destruction cannot prove
/// WebKit absence. Lifecycle code must consume it through `retire` or retain it
/// as cleanup debt.
#[must_use = "the native owner must be retired or retained as cleanup debt"]
pub(crate) struct MacosNativeRuntimeOwner {
    owner_id: ExtensionRuntimeNativeOwnerId,
    _resource_url: Retained<NSURL>,
    extension: Retained<WKWebExtension>,
    context: Retained<WKWebExtensionContext>,
    controller: Retained<WKWebExtensionController>,
    applied_grants: Option<AppliedMacosGrantSet>,
}

impl MacosNativeRuntimeOwner {
    pub(super) const fn owner_id(&self) -> ExtensionRuntimeNativeOwnerId {
        self.owner_id
    }

    pub(super) fn is_exactly(&self, other: &Self) -> bool {
        self.owner_id == other.owner_id
            && Retained::as_ptr(&self.context) == Retained::as_ptr(&other.context)
            && Retained::as_ptr(&self.controller) == Retained::as_ptr(&other.controller)
    }

    pub(crate) fn retire(mut self) -> MacosNativeRuntimeRetirement {
        let initially_loaded = match context_is_loaded(&self.context) {
            Ok(loaded) => loaded,
            Err(failure) => {
                return MacosNativeRuntimeRetirement::Retained {
                    failure,
                    owner: self,
                };
            }
        };
        let unload_failed = initially_loaded
            && catch_native(|| unsafe {
                self.controller
                    .unloadExtensionContext_error(&self.context)
                    .map_err(|_| MacosNativeRuntimeFailure::ControllerUnloadFailed)
            })
            .is_err();
        let remains_loaded = match context_is_loaded(&self.context) {
            Ok(loaded) => loaded,
            Err(failure) => {
                return MacosNativeRuntimeRetirement::Retained {
                    failure,
                    owner: self,
                };
            }
        };
        if remains_loaded {
            return MacosNativeRuntimeRetirement::Retained {
                failure: if unload_failed {
                    MacosNativeRuntimeFailure::ControllerUnloadFailed
                } else {
                    MacosNativeRuntimeFailure::AbsenceReadbackMismatch
                },
                owner: self,
            };
        }

        let grant_audit = match clear_all_grants_and_verify(&self.context) {
            Ok(audit) => audit,
            Err(error) => {
                return MacosNativeRuntimeRetirement::Retained {
                    failure: MacosNativeRuntimeFailure::GrantCleanup(error),
                    owner: self,
                };
            }
        };
        self.applied_grants = None;

        match exact_absence_audit(&self, grant_audit) {
            Some(audit) => MacosNativeRuntimeRetirement::Absent(audit),
            None => MacosNativeRuntimeRetirement::Retained {
                failure: MacosNativeRuntimeFailure::AbsenceReadbackMismatch,
                owner: self,
            },
        }
    }
}

impl fmt::Debug for MacosNativeRuntimeOwner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MacosNativeRuntimeOwner")
            .field("owner_id", &"<redacted>")
            .field("resource_url", &"<redacted>")
            .field("extension", &"<native>")
            .field("context", &"<native>")
            .field("controller", &"<native>")
            .field("has_applied_grants", &self.applied_grants.is_some())
            .finish()
    }
}

/// Begins asynchronous native construction from an exact authenticated root.
///
/// The callback is invoked at most once. `Ok(())` means WebKit owns the copied
/// completion block and the caller must keep its lifecycle ticket pending until
/// the callback settles it. The typed start failure distinguishes a clean
/// pre-entry refusal from an Objective-C exception after native entry.
pub(super) fn begin_native_runtime_activation(
    native_root: &mut ExtensionRuntimeNativeRootLease,
    grants: &ExtensionNativeGrantSnapshot,
    expected_owner_id: ExtensionRuntimeNativeOwnerId,
    controller: Retained<WKWebExtensionController>,
    completion: impl FnOnce(MacosNativeRuntimeActivation) + 'static,
) -> Result<(), MacosNativeRuntimeStartFailure> {
    let mtm = admit_runtime().map_err(MacosNativeRuntimeStartFailure::RejectedBeforeNative)?;
    let grant_plan = compile_native_grant_plan(grants)
        .map_err(MacosNativeRuntimeFailure::GrantPlan)
        .map_err(MacosNativeRuntimeStartFailure::RejectedBeforeNative)?;
    let resource_url = verified_resource_url(native_root)
        .map_err(MacosNativeRuntimeStartFailure::RejectedBeforeNative)?;
    begin_with_grants(
        resource_url,
        NativeGrantSource::Compiled(grant_plan),
        expected_owner_id,
        controller,
        completion,
        mtm,
    )
}

#[cfg(feature = "native-web-extension-probes")]
pub(crate) fn begin_probe_native_runtime_activation(
    path: &std::path::Path,
    permissions: Box<[MacosNativeApiPermission]>,
    patterns: Box<[&'static str]>,
    private_data_access: bool,
    expected_owner_id: ExtensionRuntimeNativeOwnerId,
    controller: Retained<WKWebExtensionController>,
    completion: impl FnOnce(MacosNativeRuntimeActivation) + 'static,
) -> Result<(), MacosNativeRuntimeStartFailure> {
    let mtm = admit_runtime().map_err(MacosNativeRuntimeStartFailure::RejectedBeforeNative)?;
    let resource_url = resource_url_from_path(path)
        .map_err(MacosNativeRuntimeFailure::PackageRootRejected)
        .map_err(MacosNativeRuntimeStartFailure::RejectedBeforeNative)?;
    begin_with_grants(
        resource_url,
        NativeGrantSource::Probe {
            permissions,
            patterns,
            private_data_access,
        },
        expected_owner_id,
        controller,
        completion,
        mtm,
    )
}

enum NativeGrantSource {
    Compiled(MacosNativeGrantPlan),
    #[cfg(feature = "native-web-extension-probes")]
    Probe {
        permissions: Box<[MacosNativeApiPermission]>,
        patterns: Box<[&'static str]>,
        private_data_access: bool,
    },
}

impl NativeGrantSource {
    fn apply(
        &self,
        context: &WKWebExtensionContext,
    ) -> Result<AppliedMacosGrantSet, MacosGrantApplicationError> {
        match self {
            Self::Compiled(plan) => apply_compiled_grants(context, plan),
            #[cfg(feature = "native-web-extension-probes")]
            Self::Probe {
                permissions,
                patterns,
                private_data_access,
            } => apply_probe_grants(context, permissions, patterns, *private_data_access),
        }
    }
}

fn begin_with_grants(
    resource_url: Retained<NSURL>,
    grants: NativeGrantSource,
    expected_owner_id: ExtensionRuntimeNativeOwnerId,
    controller: Retained<WKWebExtensionController>,
    completion: impl FnOnce(MacosNativeRuntimeActivation) + 'static,
    mtm: MainThreadMarker,
) -> Result<(), MacosNativeRuntimeStartFailure> {
    let completion = Rc::new(RefCell::new(Some(completion)));
    let callback_completion = Rc::clone(&completion);
    let callback_url = resource_url.clone();
    let callback = RcBlock::new(move |extension: *mut WKWebExtension, error: *mut NSError| {
        // Claim the one terminal callback before creating any native context.
        // A duplicate native invocation is therefore a side-effect-free no-op,
        // rather than a silently dropped second owner.
        let completion = callback_completion.borrow_mut().take();
        let Some(completion) = completion else {
            return;
        };
        let outcome = settle_parse_callback(
            extension,
            error,
            callback_url.clone(),
            &grants,
            expected_owner_id,
            controller.clone(),
        );
        completion(outcome);
    });
    catch_native(|| unsafe {
        WKWebExtension::extensionWithResourceBaseURL_completionHandler(
            &resource_url,
            &callback,
            mtm,
        );
        Ok(())
    })
    .map_err(MacosNativeRuntimeStartFailure::OwnershipUncertain)
}

fn settle_parse_callback(
    extension: *mut WKWebExtension,
    error: *mut NSError,
    resource_url: Retained<NSURL>,
    grants: &NativeGrantSource,
    expected_owner_id: ExtensionRuntimeNativeOwnerId,
    controller: Retained<WKWebExtensionController>,
) -> MacosNativeRuntimeActivation {
    let Some(extension) = (unsafe { Retained::retain(extension) }) else {
        return MacosNativeRuntimeActivation::Rejected(
            MacosNativeRuntimeFailure::ExtensionParseFailed,
        );
    };
    if !error.is_null() {
        return MacosNativeRuntimeActivation::Rejected(
            MacosNativeRuntimeFailure::ExtensionParseFailed,
        );
    }
    match construct_loaded_owner(
        resource_url,
        extension,
        grants,
        expected_owner_id,
        controller,
    ) {
        Ok(owner) => MacosNativeRuntimeActivation::Activated(owner),
        Err((failure, owner)) => match owner {
            Some(owner) => classify_failed_owner(failure, owner),
            None => MacosNativeRuntimeActivation::Rejected(failure),
        },
    }
}

fn construct_loaded_owner(
    resource_url: Retained<NSURL>,
    extension: Retained<WKWebExtension>,
    grants: &NativeGrantSource,
    expected_owner_id: ExtensionRuntimeNativeOwnerId,
    controller: Retained<WKWebExtensionController>,
) -> Result<MacosNativeRuntimeOwner, (MacosNativeRuntimeFailure, Option<MacosNativeRuntimeOwner>)> {
    let extension_valid = catch_native(|| unsafe {
        Ok(extension.errors().count() == 0 && extension.manifestVersion() == 3.0)
    });
    if extension_valid != Ok(true) {
        return Err((
            extension_valid
                .err()
                .unwrap_or(MacosNativeRuntimeFailure::ExtensionManifestInvalid),
            None,
        ));
    }

    let context = match catch_native(|| unsafe {
        let context = WKWebExtensionContext::contextForExtension(&extension);
        if context.isLoaded()
            || Retained::as_ptr(&context.webExtension()) != Retained::as_ptr(&extension)
        {
            return Err(MacosNativeRuntimeFailure::ContextConstructionFailed);
        }
        Ok(context)
    }) {
        Ok(context) => context,
        Err(failure) => return Err((failure, None)),
    };
    let mut owner = MacosNativeRuntimeOwner {
        owner_id: expected_owner_id,
        _resource_url: resource_url,
        extension,
        context,
        controller,
        applied_grants: None,
    };

    if let Err(failure) = set_and_verify_identity(&owner.context, expected_owner_id) {
        return Err((failure, Some(owner)));
    }
    match grants.apply(&owner.context) {
        Ok(applied) => owner.applied_grants = Some(applied),
        Err(error) => {
            return Err((
                MacosNativeRuntimeFailure::GrantApplication(error),
                Some(owner),
            ));
        }
    }
    if catch_native(|| unsafe {
        owner
            .controller
            .loadExtensionContext_error(&owner.context)
            .map_err(|_| MacosNativeRuntimeFailure::ControllerLoadFailed)
    })
    .is_err()
    {
        return Err((MacosNativeRuntimeFailure::ControllerLoadFailed, Some(owner)));
    }
    match validate_loaded_owner(&owner) {
        Ok(()) => Ok(owner),
        Err(failure) => Err((failure, Some(owner))),
    }
}

fn classify_failed_owner(
    failure: MacosNativeRuntimeFailure,
    owner: MacosNativeRuntimeOwner,
) -> MacosNativeRuntimeActivation {
    match owner.retire() {
        MacosNativeRuntimeRetirement::Absent(_audit) => {
            MacosNativeRuntimeActivation::Rejected(failure)
        }
        MacosNativeRuntimeRetirement::Retained {
            failure: _cleanup_failure,
            owner,
        } => MacosNativeRuntimeActivation::OwnershipUncertain { failure, owner },
    }
}

fn validate_loaded_owner(owner: &MacosNativeRuntimeOwner) -> Result<(), MacosNativeRuntimeFailure> {
    catch_native(|| unsafe {
        let identifier = owner.context.uniqueIdentifier();
        let expected_bytes = owner.owner_id.encoded_bytes();
        let expected = NSString::from_str(
            std::str::from_utf8(&expected_bytes)
                .map_err(|_| MacosNativeRuntimeFailure::IdentityReadbackMismatch)?,
        );
        let context_controller = owner.context.webExtensionController();
        let looked_up = owner
            .controller
            .extensionContextForExtension(&owner.extension);
        let exact = owner.context.isLoaded()
            && identifier.isEqualToString(&expected)
            && owner
                .controller
                .extensionContexts()
                .containsObject(&owner.context)
            && owner
                .controller
                .extensions()
                .containsObject(&owner.extension)
            && context_controller.as_ref().is_some_and(|actual| {
                Retained::as_ptr(actual) == Retained::as_ptr(&owner.controller)
            })
            && looked_up
                .as_ref()
                .is_some_and(|actual| Retained::as_ptr(actual) == Retained::as_ptr(&owner.context));
        if !exact {
            return Err(MacosNativeRuntimeFailure::LoadedOwnerReadbackMismatch);
        }
        if owner.context.errors().count() != 0 {
            return Err(MacosNativeRuntimeFailure::ContextRuntimeError);
        }
        Ok(())
    })
}

fn exact_absence_audit(
    owner: &MacosNativeRuntimeOwner,
    _grant_audit: ClearedMacosGrantAudit,
) -> Option<ExtensionRuntimeMacosAbsenceAudit> {
    let absent = catch_native(|| unsafe {
        let controller_absent = !owner
            .controller
            .extensionContexts()
            .containsObject(&owner.context)
            && !owner
                .controller
                .extensions()
                .containsObject(&owner.extension)
            && owner
                .controller
                .extensionContextForExtension(&owner.extension)
                .is_none()
            && owner.context.webExtensionController().is_none();
        Ok((!owner.context.isLoaded(), controller_absent))
    })
    .ok()?;
    ExtensionRuntimeMacosAbsenceAudit::try_from_observations(
        true, true, true, true, false, false, true, true, absent.0, absent.1,
    )
}

fn set_and_verify_identity(
    context: &WKWebExtensionContext,
    owner_id: ExtensionRuntimeNativeOwnerId,
) -> Result<(), MacosNativeRuntimeFailure> {
    let bytes = owner_id.encoded_bytes();
    let identifier = std::str::from_utf8(&bytes)
        .map_err(|_| MacosNativeRuntimeFailure::IdentityReadbackMismatch)?;
    let identifier = NSString::from_str(identifier);
    catch_native(|| unsafe {
        context.setUniqueIdentifier(&identifier);
        context.setInspectable(false);
        if context.uniqueIdentifier().isEqualToString(&identifier)
            && !context.isInspectable()
            && !context.isLoaded()
            && context.webExtensionController().is_none()
        {
            Ok(())
        } else {
            Err(MacosNativeRuntimeFailure::IdentityReadbackMismatch)
        }
    })
}

fn context_is_loaded(context: &WKWebExtensionContext) -> Result<bool, MacosNativeRuntimeFailure> {
    catch_native(|| unsafe { Ok(context.isLoaded()) })
}

fn verified_resource_url(
    native_root: &mut ExtensionRuntimeNativeRootLease,
) -> Result<Retained<NSURL>, MacosNativeRuntimeFailure> {
    let mut resource_url = None;
    let visited = native_root.with_verified_path(&mut |root: &std::path::Path| {
        resource_url = Some(resource_url_from_path(root)?);
        Ok(())
    });
    match visited {
        Ok(Ok(())) => resource_url.ok_or(MacosNativeRuntimeFailure::PackageRootRejected(
            ExtensionRuntimeVisitorError::InvalidData,
        )),
        Ok(Err(error)) => Err(MacosNativeRuntimeFailure::PackageRootRejected(error)),
        Err(error) => Err(MacosNativeRuntimeFailure::PackageRootAccess(error)),
    }
}

fn resource_url_from_path(
    root: &std::path::Path,
) -> Result<Retained<NSURL>, ExtensionRuntimeVisitorError> {
    let path = CString::new(root.as_os_str().as_bytes())
        .map_err(|_| ExtensionRuntimeVisitorError::InvalidData)?;
    let pointer =
        NonNull::new(path.as_ptr().cast_mut()).ok_or(ExtensionRuntimeVisitorError::InvalidData)?;
    let url = unsafe {
        NSURL::fileURLWithFileSystemRepresentation_isDirectory_relativeToURL(pointer, true, None)
    };
    let exact = url.isFileURL()
        && url.hasDirectoryPath()
        && unsafe { CStr::from_ptr(url.fileSystemRepresentation().as_ptr()) }.to_bytes()
            == root.as_os_str().as_bytes();
    if exact {
        Ok(url)
    } else {
        Err(ExtensionRuntimeVisitorError::InvalidData)
    }
}

fn admit_runtime() -> Result<MainThreadMarker, MacosNativeRuntimeFailure> {
    let mtm = MainThreadMarker::new().ok_or(MacosNativeRuntimeFailure::MainThreadRequired)?;
    let version = NSProcessInfo::processInfo().operatingSystemVersion();
    if version.majorVersion < 0 || version.minorVersion < 0 || version.patchVersion < 0 {
        return Err(MacosNativeRuntimeFailure::UnsupportedRuntime);
    }
    if version.majorVersion > MINIMUM_MACOS_MAJOR
        || (version.majorVersion == MINIMUM_MACOS_MAJOR
            && version.minorVersion >= MINIMUM_MACOS_MINOR)
    {
        Ok(mtm)
    } else {
        Err(MacosNativeRuntimeFailure::UnsupportedRuntime)
    }
}

fn catch_native<T>(
    operation: impl FnOnce() -> Result<T, MacosNativeRuntimeFailure>,
) -> Result<T, MacosNativeRuntimeFailure> {
    objc2::exception::catch(AssertUnwindSafe(operation))
        .map_err(|_| MacosNativeRuntimeFailure::NativeException)?
}
