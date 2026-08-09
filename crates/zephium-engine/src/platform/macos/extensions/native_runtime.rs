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
    /// Activation failed without constructing an exact owner and without a
    /// complete native absence audit. The host must retain cleanup debt: the
    /// parse request crossed the native boundary, so this outcome cannot mint
    /// never-entered absence.
    RejectedWithoutAbsenceProof(MacosNativeRuntimeFailure),
    /// Activation failed after constructing an exact owner, and exact teardown
    /// proved that all grants were cleared and the owner was unloaded from its
    /// controller. These observations are sufficient for the reservation-bound
    /// host issuer to mint macOS absence evidence.
    RejectedAfterCleanup {
        failure: MacosNativeRuntimeFailure,
        owner_id: ExtensionRuntimeNativeOwnerId,
        audit: ExtensionRuntimeMacosAbsenceAudit,
    },
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

/// Fully validated, main-thread-only input for the one native entry call.
///
/// Package-root access, grant compilation, runtime availability, and URL
/// readback all finish before this value is constructed. Once it exists, the
/// host can atomically mark its lifecycle ticket as native-entered and call
/// [`begin_prepared_native_runtime_activation`] without conflating a clean
/// pre-entry refusal with an exception raised while entering WebKit.
#[must_use = "prepared native activation must be entered or discarded before native entry"]
pub(crate) struct PreparedMacosNativeRuntimeActivation {
    resource_url: Retained<NSURL>,
    grants: MacosNativeGrantPlan,
    expected_owner_id: ExtensionRuntimeNativeOwnerId,
    controller: Retained<WKWebExtensionController>,
    mtm: MainThreadMarker,
}

/// Copy-only exact identity retained while the move-only owner is inside a
/// native teardown call.
///
/// Pointer values are compared but never dereferenced. They remain valid as
/// identity anchors because a failed teardown must return the same retained
/// owner before the call settles; successful teardown returns no owner.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct MacosNativeRuntimeOwnerIdentity {
    owner_id: ExtensionRuntimeNativeOwnerId,
    context: *const WKWebExtensionContext,
    controller: *const WKWebExtensionController,
}

impl fmt::Debug for MacosNativeRuntimeOwnerIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MacosNativeRuntimeOwnerIdentity")
            .field("owner_id", &"<redacted>")
            .field("context", &"<native>")
            .field("controller", &"<native>")
            .finish()
    }
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
    pub(crate) const fn owner_id(&self) -> ExtensionRuntimeNativeOwnerId {
        self.owner_id
    }

    pub(crate) fn is_exactly(&self, other: &Self) -> bool {
        self.identity() == other.identity()
    }

    pub(crate) fn identity(&self) -> MacosNativeRuntimeOwnerIdentity {
        MacosNativeRuntimeOwnerIdentity {
            owner_id: self.owner_id,
            context: Retained::as_ptr(&self.context),
            controller: Retained::as_ptr(&self.controller),
        }
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
    let prepared =
        prepare_native_runtime_activation(native_root, grants, expected_owner_id, controller)
            .map_err(MacosNativeRuntimeStartFailure::RejectedBeforeNative)?;
    begin_prepared_native_runtime_activation(prepared, completion)
        .map_err(MacosNativeRuntimeStartFailure::OwnershipUncertain)
}

/// Completes every fallible pre-entry check without asking WebKit to parse or
/// retain an extension.
pub(crate) fn prepare_native_runtime_activation(
    native_root: &mut ExtensionRuntimeNativeRootLease,
    grants: &ExtensionNativeGrantSnapshot,
    expected_owner_id: ExtensionRuntimeNativeOwnerId,
    controller: Retained<WKWebExtensionController>,
) -> Result<PreparedMacosNativeRuntimeActivation, MacosNativeRuntimeFailure> {
    let mtm = admit_runtime()?;
    let grants = compile_native_grant_plan(grants).map_err(MacosNativeRuntimeFailure::GrantPlan)?;
    let resource_url = verified_resource_url(native_root)?;
    Ok(PreparedMacosNativeRuntimeActivation {
        resource_url,
        grants,
        expected_owner_id,
        controller,
        mtm,
    })
}

/// Enters WebKit with an already-validated activation plan.
///
/// Every returned error is ownership-uncertain: the Objective-C call was
/// attempted and may have copied its callback before raising an exception.
pub(crate) fn begin_prepared_native_runtime_activation(
    prepared: PreparedMacosNativeRuntimeActivation,
    completion: impl FnOnce(MacosNativeRuntimeActivation) + 'static,
) -> Result<(), MacosNativeRuntimeFailure> {
    let PreparedMacosNativeRuntimeActivation {
        resource_url,
        grants,
        expected_owner_id,
        controller,
        mtm,
    } = prepared;
    begin_with_grants(
        resource_url,
        NativeGrantSource::Compiled(grants),
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
    .map_err(MacosNativeRuntimeStartFailure::OwnershipUncertain)
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
) -> Result<(), MacosNativeRuntimeFailure> {
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
        return MacosNativeRuntimeActivation::RejectedWithoutAbsenceProof(
            MacosNativeRuntimeFailure::ExtensionParseFailed,
        );
    };
    if !error.is_null() {
        return MacosNativeRuntimeActivation::RejectedWithoutAbsenceProof(
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
            None => MacosNativeRuntimeActivation::RejectedWithoutAbsenceProof(failure),
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
    let owner_id = owner.owner_id();
    match owner.retire() {
        MacosNativeRuntimeRetirement::Absent(audit) => {
            MacosNativeRuntimeActivation::RejectedAfterCleanup {
                failure,
                owner_id,
                audit,
            }
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
