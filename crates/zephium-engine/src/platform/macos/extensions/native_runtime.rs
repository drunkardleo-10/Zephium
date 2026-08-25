//! Construction and exact teardown of one native macOS extension.
//!
//! This is the narrow native seam between an authenticated package-root lease,
//! one complete Core grant snapshot, the profile-owned controller, and the
//! host lifecycle registry. Product entry is fenced behind startup cleanup and
//! hydration so every owner it creates remains recoverable.

// Probe-only helpers remain compiled beside the production lifecycle.

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
#[cfg(feature = "native-web-extension-probes")]
pub(crate) enum MacosNativeRuntimeStartFailure {
    RejectedBeforeNative(MacosNativeRuntimeFailure),
    OwnershipUncertain(MacosNativeRuntimeFailure),
}

#[cfg(feature = "native-web-extension-probes")]
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

#[cfg(feature = "native-web-extension-probes")]
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

/// Exact same-process settlement of one retained native owner.
///
/// The owner remains captive in the host lifecycle slot while this audit
/// executes. `Absent` is returned only after the same unload, grant-clear, and
/// controller-membership proof used by retirement; `Owned` requires exact
/// pointer and identifier readback from the retained context.
#[must_use = "same-process reconciliation must settle the host lifecycle ticket"]
pub(crate) enum MacosNativeRuntimeReconciliation {
    Owned,
    Absent(ExtensionRuntimeMacosAbsenceAudit),
    StillUncertain(MacosNativeRuntimeFailure),
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
    action_projection: Option<Box<ActionProjectionCache>>,
}

struct ActionProjectionCache {
    next_revision: Option<u64>,
    last: Option<zephium_core::extensions::ExtensionActionState>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MacosNativeActionFailure {
    OwnerInvalid,
    ActionUnavailable,
    ContextMismatch,
    InvalidProjection,
    RevisionExhausted,
    StaleAction,
    ActionDisabled,
    PopupRequired,
    NativeException,
}

pub(crate) struct MacosNativeActionPopupOwner {
    context: Retained<WKWebExtensionContext>,
    controller: *const WKWebExtensionController,
}

impl MacosNativeActionPopupOwner {
    pub(super) fn controller_identity(&self) -> *const WKWebExtensionController {
        self.controller
    }

    pub(super) fn into_context(self) -> Retained<WKWebExtensionContext> {
        self.context
    }
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

    /// Retains the exact loaded context for a popup callback reservation. The
    /// retained object is identity only; it grants neither controller nor tab
    /// authority and is revalidated again by the delegate.
    pub(crate) fn action_popup_owner(
        &self,
    ) -> Result<MacosNativeActionPopupOwner, MacosNativeActionFailure> {
        validate_loaded_owner_membership(self)
            .map_err(|_| MacosNativeActionFailure::OwnerInvalid)?;
        Ok(MacosNativeActionPopupOwner {
            context: self.context.clone(),
            controller: Retained::as_ptr(&self.controller),
        })
    }

    pub(crate) fn action_popup_context_identity(&self) -> *const WKWebExtensionContext {
        Retained::as_ptr(&self.context)
    }

    /// Comparison-only identity used to join a spontaneous delegate callback
    /// to the exact published runtime that already owns this context.
    pub(crate) fn runtime_grant_context_identity(&self) -> *const WKWebExtensionContext {
        Retained::as_ptr(&self.context)
    }

    /// Bounded authenticated display label for browser-owned consent UI.
    /// The package was parsed from the held content-addressed root before this
    /// owner existed; this value remains display-only and grants no authority.
    pub(crate) fn runtime_grant_display_name(&self) -> Option<String> {
        let name = unsafe { self.extension.displayName() }?.to_string();
        (!name.is_empty()
            && name.len()
                <= zephium_core::ports::extensions::MAX_EXTENSION_MANAGEMENT_DISPLAY_TEXT_BYTES
            && !name.chars().any(char::is_control))
        .then_some(name)
    }

    pub(crate) fn retire(mut self) -> MacosNativeRuntimeRetirement {
        match self.prove_absence() {
            Ok(audit) => MacosNativeRuntimeRetirement::Absent(audit),
            Err(failure) => MacosNativeRuntimeRetirement::Retained {
                failure,
                owner: self,
            },
        }
    }

    /// Audits the exact retained owner without transferring or duplicating its
    /// native identity. A loaded, pointer-attested context is owned even when
    /// its extension reports runtime errors; those errors affect usability,
    /// not whether WebKit still owns the context.
    pub(crate) fn reconcile(&mut self) -> MacosNativeRuntimeReconciliation {
        if validate_loaded_owner_membership(self).is_ok() {
            return MacosNativeRuntimeReconciliation::Owned;
        }
        match self.prove_absence() {
            Ok(audit) => MacosNativeRuntimeReconciliation::Absent(audit),
            Err(failure) => MacosNativeRuntimeReconciliation::StillUncertain(failure),
        }
    }

    /// Reads the effective action for one exact already-published logical tab.
    /// No popup webview or content renderer is requested by this path.
    pub(crate) fn action_state_for_tab(
        &mut self,
        runtime: zephium_core::extensions::ExtensionRuntimeInstance,
        tab_id: zephium_core::ids::ItemId,
        tab: &objc2::runtime::ProtocolObject<dyn objc2_web_kit::WKWebExtensionTab>,
    ) -> Result<zephium_core::extensions::ExtensionActionState, MacosNativeActionFailure> {
        use objc2_foundation::NSUTF8StringEncoding;
        use zephium_core::extensions::{
            ExtensionActionRevision, ExtensionActionScope, ExtensionActionState,
            MAX_EXTENSION_ACTION_BADGE_BYTES, MAX_EXTENSION_ACTION_LABEL_BYTES,
        };

        validate_loaded_owner_membership(self)
            .map_err(|_| MacosNativeActionFailure::OwnerInvalid)?;
        let action = unsafe { self.context.actionForTab(Some(tab)) }
            .ok_or(MacosNativeActionFailure::ActionUnavailable)?;
        let action_context = unsafe { action.webExtensionContext() }
            .ok_or(MacosNativeActionFailure::ContextMismatch)?;
        let associated_tab =
            unsafe { action.associatedTab() }.ok_or(MacosNativeActionFailure::ContextMismatch)?;
        if !std::ptr::eq(&*action_context, &*self.context) || &*associated_tab != tab {
            return Err(MacosNativeActionFailure::ContextMismatch);
        }

        let label = unsafe { action.label() };
        let badge = unsafe { action.badgeText() };
        if label.lengthOfBytesUsingEncoding(NSUTF8StringEncoding) > MAX_EXTENSION_ACTION_LABEL_BYTES
            || badge.lengthOfBytesUsingEncoding(NSUTF8StringEncoding)
                > MAX_EXTENSION_ACTION_BADGE_BYTES
        {
            return Err(MacosNativeActionFailure::InvalidProjection);
        }
        let projection = self.action_projection.get_or_insert_with(|| {
            Box::new(ActionProjectionCache {
                next_revision: Some(1),
                last: None,
            })
        });
        // An exhausted counter must still permit an unchanged read. Use the
        // cached revision as a comparison-only candidate, then reject only if
        // WebKit actually presents a new value that cannot be numbered.
        let (revision, revision_exhausted) = match projection.next_revision {
            Some(next) => (
                ExtensionActionRevision::new(next)
                    .ok_or(MacosNativeActionFailure::RevisionExhausted)?,
                false,
            ),
            None => (
                projection
                    .last
                    .as_ref()
                    .map(zephium_core::extensions::ExtensionActionState::revision)
                    .ok_or(MacosNativeActionFailure::RevisionExhausted)?,
                true,
            ),
        };
        let icon = super::action_icon::rasterize_action_icon(&action);
        let state = objc2::rc::autoreleasepool(|pool| {
            let label = unsafe { label.to_str(pool) };
            let badge = unsafe { badge.to_str(pool) };
            ExtensionActionState::new(
                runtime,
                ExtensionActionScope::Tab(tab_id),
                revision,
                label,
                badge,
                icon,
                unsafe { action.isEnabled() },
                unsafe { action.presentsPopup() },
                unsafe { action.hasUnreadBadgeText() },
            )
        })
        .map_err(|_| MacosNativeActionFailure::InvalidProjection)?;
        if let Some(cached) = projection
            .last
            .as_ref()
            .filter(|cached| cached.same_presentation(&state))
        {
            return Ok(cached.clone());
        }
        if revision_exhausted {
            return Err(MacosNativeActionFailure::RevisionExhausted);
        }
        projection.next_revision = revision.get().checked_add(1);
        projection.last = Some(state.clone());
        Ok(state)
    }

    /// Performs only an action that still exactly matches the Shell-visible
    /// revision and does not require popup presentation. Popup actions are
    /// deliberately refused until the separately-budgeted delegate broker has
    /// retained its completion and resource lease.
    pub(crate) fn perform_non_popup_action_for_tab(
        &mut self,
        runtime: zephium_core::extensions::ExtensionRuntimeInstance,
        tab_id: zephium_core::ids::ItemId,
        tab: &objc2::runtime::ProtocolObject<dyn objc2_web_kit::WKWebExtensionTab>,
        expected_revision: zephium_core::extensions::ExtensionActionRevision,
    ) -> Result<(), MacosNativeActionFailure> {
        let state = self.action_state_for_tab(runtime, tab_id, tab)?;
        if state.revision() != expected_revision {
            return Err(MacosNativeActionFailure::StaleAction);
        }
        if !state.is_enabled() {
            return Err(MacosNativeActionFailure::ActionDisabled);
        }
        if state.presents_popup() {
            return Err(MacosNativeActionFailure::PopupRequired);
        }
        objc2::exception::catch(AssertUnwindSafe(|| unsafe {
            self.context.performActionForTab(Some(tab));
        }))
        .map_err(|_| MacosNativeActionFailure::NativeException)
    }

    /// Enters WebKit only after the host has retained the popup resource lease
    /// and installed the exact delegate callback expectation.
    pub(crate) fn validate_popup_action_for_tab(
        &mut self,
        runtime: zephium_core::extensions::ExtensionRuntimeInstance,
        tab_id: zephium_core::ids::ItemId,
        tab: &objc2::runtime::ProtocolObject<dyn objc2_web_kit::WKWebExtensionTab>,
        expected_revision: zephium_core::extensions::ExtensionActionRevision,
    ) -> Result<(), MacosNativeActionFailure> {
        let state = self.action_state_for_tab(runtime, tab_id, tab)?;
        if state.revision() != expected_revision {
            return Err(MacosNativeActionFailure::StaleAction);
        }
        if !state.is_enabled() {
            return Err(MacosNativeActionFailure::ActionDisabled);
        }
        if !state.presents_popup() {
            return Err(MacosNativeActionFailure::StaleAction);
        }
        Ok(())
    }

    fn prove_absence(
        &mut self,
    ) -> Result<ExtensionRuntimeMacosAbsenceAudit, MacosNativeRuntimeFailure> {
        let initially_loaded = context_is_loaded(&self.context)?;
        let unload_failed = initially_loaded
            && catch_native(|| unsafe {
                self.controller
                    .unloadExtensionContext_error(&self.context)
                    .map_err(|_| MacosNativeRuntimeFailure::ControllerUnloadFailed)
            })
            .is_err();
        let remains_loaded = context_is_loaded(&self.context)?;
        if remains_loaded {
            return Err(if unload_failed {
                MacosNativeRuntimeFailure::ControllerUnloadFailed
            } else {
                MacosNativeRuntimeFailure::AbsenceReadbackMismatch
            });
        }

        let grant_audit = match clear_all_grants_and_verify(&self.context) {
            Ok(audit) => audit,
            Err(error) => return Err(MacosNativeRuntimeFailure::GrantCleanup(error)),
        };
        self.applied_grants = None;

        exact_absence_audit(self, grant_audit)
            .ok_or(MacosNativeRuntimeFailure::AbsenceReadbackMismatch)
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

/// Completes every fallible pre-entry check without asking WebKit to parse or
/// retain an extension.
pub(crate) fn prepare_native_runtime_activation(
    native_root: &mut ExtensionRuntimeNativeRootLease,
    grants: &ExtensionNativeGrantSnapshot,
    backend: zephium_core::extensions::ExtensionRuntimeBackendTarget,
    expected_owner_id: ExtensionRuntimeNativeOwnerId,
    controller: Retained<WKWebExtensionController>,
) -> Result<PreparedMacosNativeRuntimeActivation, MacosNativeRuntimeFailure> {
    let mtm = admit_runtime()?;
    let grants =
        compile_native_grant_plan(grants, backend).map_err(MacosNativeRuntimeFailure::GrantPlan)?;
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
        report_product_probe_manifest_errors(&extension);
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
        action_projection: None,
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

#[cfg(feature = "native-extension-product-probes")]
fn report_product_probe_manifest_errors(extension: &WKWebExtension) {
    // This feature is linked only into the sealed, debug-only product probe;
    // its manifest bytes are checked-in authenticated fixture data. Never
    // enable this raw framework diagnostic for user-installed packages.
    let errors = unsafe { extension.errors() };
    for index in 0..errors.count() {
        let error = errors.objectAtIndex(index);
        eprintln!(
            "extension-product-probe-manifest-error: domain={}; code={}; description={}",
            error.domain(),
            error.code(),
            error.localizedDescription(),
        );
    }
}

#[cfg(not(feature = "native-extension-product-probes"))]
fn report_product_probe_manifest_errors(_extension: &WKWebExtension) {}

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

fn validate_loaded_owner_membership(
    owner: &MacosNativeRuntimeOwner,
) -> Result<(), MacosNativeRuntimeFailure> {
    catch_native(|| unsafe {
        let identifier = owner.context.uniqueIdentifier();
        let expected = native_context_identity(owner.owner_id)?;
        let base_url_matches = owner
            .context
            .baseURL()
            .absoluteString()
            .is_some_and(|actual| actual.isEqualToString(&expected.base_url_string));
        let context_controller = owner.context.webExtensionController();
        let looked_up = owner
            .controller
            .extensionContextForExtension(&owner.extension);
        let exact = owner.context.isLoaded()
            && identifier.isEqualToString(&expected.identifier)
            && base_url_matches
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
        Ok(())
    })
}

fn validate_loaded_owner(owner: &MacosNativeRuntimeOwner) -> Result<(), MacosNativeRuntimeFailure> {
    validate_loaded_owner_membership(owner)?;
    catch_native(|| unsafe {
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
    let identity = native_context_identity(owner_id)?;
    let inspectable = cfg!(feature = "native-extension-lab-diagnostics");
    catch_native(|| unsafe {
        context.setBaseURL(&identity.base_url);
        context.setUniqueIdentifier(&identity.identifier);
        context.setInspectable(inspectable);
        let base_url_matches = context
            .baseURL()
            .absoluteString()
            .is_some_and(|actual| actual.isEqualToString(&identity.base_url_string));
        if base_url_matches
            && context
                .uniqueIdentifier()
                .isEqualToString(&identity.identifier)
            && context.isInspectable() == inspectable
            && !context.isLoaded()
            && context.webExtensionController().is_none()
        {
            Ok(())
        } else {
            Err(MacosNativeRuntimeFailure::IdentityReadbackMismatch)
        }
    })
}

struct NativeContextIdentity {
    identifier: Retained<NSString>,
    base_url_string: Retained<NSString>,
    base_url: Retained<NSURL>,
}

fn native_context_identity(
    owner_id: ExtensionRuntimeNativeOwnerId,
) -> Result<NativeContextIdentity, MacosNativeRuntimeFailure> {
    let bytes = owner_id.encoded_bytes();
    let identifier_text = std::str::from_utf8(&bytes)
        .map_err(|_| MacosNativeRuntimeFailure::IdentityReadbackMismatch)?;
    let identifier = NSString::from_str(identifier_text);
    let base_url_string = NSString::from_str(&format!("webkit-extension://{identifier_text}/"));
    let base_url = NSURL::URLWithString(&base_url_string)
        .ok_or(MacosNativeRuntimeFailure::IdentityReadbackMismatch)?;
    Ok(NativeContextIdentity {
        identifier,
        base_url_string,
        base_url,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_context_origin_is_stable_exact_and_owner_distinct() {
        let first = ExtensionRuntimeNativeOwnerId::from_encoded_bytes([b'a'; 32]).unwrap();
        let second = ExtensionRuntimeNativeOwnerId::from_encoded_bytes([b'b'; 32]).unwrap();
        let first_identity = native_context_identity(first).unwrap();
        let repeated_identity = native_context_identity(first).unwrap();
        let second_identity = native_context_identity(second).unwrap();

        assert_eq!(first_identity.identifier.to_string(), "a".repeat(32));
        assert_eq!(first_identity.identifier, repeated_identity.identifier);
        assert_eq!(
            first_identity.base_url_string,
            repeated_identity.base_url_string
        );
        assert_eq!(
            first_identity.base_url_string.to_string(),
            format!("webkit-extension://{}/", "a".repeat(32))
        );
        assert_eq!(
            first_identity.base_url.absoluteString().unwrap(),
            repeated_identity.base_url.absoluteString().unwrap()
        );
        assert_ne!(
            first_identity.base_url_string,
            second_identity.base_url_string
        );
    }
}
