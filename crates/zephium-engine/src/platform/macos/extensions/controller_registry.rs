//! Bounded ownership of profile-scoped WebKit extension controllers.
//!
//! Construction remains explicit and lifecycle-authorized: startup and view
//! construction never allocate a controller. The native runtime adapter may
//! prepare one only after the Store has durably entered the macOS namespace,
//! and must supply that exact namespace scope on every preparation or borrow.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::panic::AssertUnwindSafe;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, Sel};
use objc2::sel;
use objc2::MainThreadOnly;
use objc2_app_kit::NSView;
use objc2_foundation::MainThreadMarker;
use objc2_foundation::{NSClassFromString, NSProcessInfo, NSString, NSUUID};
use objc2_web_kit::WKWebExtensionControllerConfiguration;
use objc2_web_kit::{
    WKWebExtensionContext, WKWebExtensionController, WKWebView, WKWebViewConfiguration,
    WKWebsiteDataStore,
};
use zephium_core::extensions::{
    ExtensionActionRejection, ExtensionActionRequest, ExtensionActionRequestId,
    ExtensionBrowserSurface, ExtensionNativeNamespaceScope,
};
use zephium_core::ids::{ItemId, ProfileId};
use zephium_extension_runtime_api::ExtensionRuntimeMacosControllerAbsenceAudit;

use super::browser_request_broker::{BrowserRequestPool, BrowserRequestSettlementOutcome};
#[cfg(feature = "native-web-extension-probes")]
use super::browser_surface::BrowserSurfaceDiagnostics;
#[cfg(feature = "native-web-extension-probes")]
use super::browser_surface::ProbeBrowserSurfaceIdentity;
use super::browser_surface::{BrowserSurfaceError, MacosExtensionBrowserSurfaceHost};
use super::erasure::{
    ControllerErasureTicket, ControllerErasureWitness, PersistentControllerErasure,
    ProfileControllerErasure,
};
use super::runtime_grant_broker::{RuntimeGrantRequestPool, RuntimeGrantSettlementOutcome};

const MAX_PERSISTENT_CONTROLLERS: usize = zephium_core::session::MAX_SESSION_PROFILES;
const _: () = assert!(MAX_PERSISTENT_CONTROLLERS == 64);

pub(crate) struct ControllerActionTab {
    tab: super::browser_surface::NativeExtensionTab,
    resident: bool,
}

impl ControllerActionTab {
    pub(crate) fn into_parts(self) -> (super::browser_surface::NativeExtensionTab, bool) {
        (self.tab, self.resident)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ControllerRegistryError {
    CapacityExceeded,
    Sealed,
    IntegrityFailed,
    MainThreadRequired,
    InvalidRuntimeVersion,
    IncompleteRuntime,
    NativeException,
    NonPersistentStore,
    StoreIdentifierMismatch,
    NonPersistentController,
    ControllerIdentifierMismatch,
    ControllerStoreMismatch,
    ControllerAlias,
    StoreAlias,
    UnexpectedLoadedContext,
    UnexpectedLoadedExtension,
    EntryChanged,
    ViewStoreMismatch,
    ViewControllerMismatch,
    UnexpectedViewController,
    BrowserSurfaceProfileMismatch,
    BrowserSurfaceStale,
    BrowserSurfacePrivacyChanged,
    BrowserSurfaceIntegrity,
    ControllerDelegateMismatch,
    ErasureInFlight,
    ErasureGenerationExhausted,
    NamespaceScopeRequired,
    NamespaceReopenUnavailable,
    UnsupportedNamespaceScope,
}

impl fmt::Display for ControllerRegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::CapacityExceeded => "macOS extension-controller profile capacity was exceeded",
            Self::Sealed => "macOS extension-controller registry is sealed",
            Self::IntegrityFailed => "macOS extension-controller registry integrity has failed",
            Self::MainThreadRequired => {
                "macOS extension-controller work requires the application main thread"
            }
            Self::InvalidRuntimeVersion => "macOS returned an invalid operating-system version",
            Self::IncompleteRuntime => "macOS is missing a required public WebKit extension API",
            Self::NativeException => {
                "WebKit raised an exception at the extension-controller boundary"
            }
            Self::NonPersistentStore => {
                "a durable extension profile received a non-persistent website data store"
            }
            Self::StoreIdentifierMismatch => {
                "a durable extension profile received the wrong website data-store identifier"
            }
            Self::NonPersistentController => {
                "a durable extension profile received a non-persistent controller"
            }
            Self::ControllerIdentifierMismatch => {
                "a durable extension profile received the wrong controller identifier"
            }
            Self::ControllerStoreMismatch => {
                "an extension controller did not retain its exact profile website data store"
            }
            Self::ControllerAlias => "two extension profiles shared one native controller",
            Self::StoreAlias => "two extension profiles shared one native website data store",
            Self::UnexpectedLoadedContext => {
                "a quiescent extension controller unexpectedly contains a loaded context"
            }
            Self::UnexpectedLoadedExtension => {
                "a quiescent extension controller unexpectedly contains a loaded extension"
            }
            Self::EntryChanged => {
                "the prepared extension-controller entry changed during view construction"
            }
            Self::ViewStoreMismatch => {
                "WKWebView did not retain the prepared profile website data store"
            }
            Self::ViewControllerMismatch => {
                "WKWebView did not retain the prepared profile extension controller"
            }
            Self::UnexpectedViewController => {
                "a view without prepared extension authority received a controller"
            }
            Self::BrowserSurfaceProfileMismatch => {
                "an extension browser surface crossed profile ownership"
            }
            Self::BrowserSurfaceStale => {
                "a stale extension browser-surface generation reached the native adapter"
            }
            Self::BrowserSurfacePrivacyChanged => {
                "an extension browser window changed privacy class in place"
            }
            Self::BrowserSurfaceIntegrity => {
                "the native extension browser-surface graph lost integrity"
            }
            Self::ControllerDelegateMismatch => {
                "an extension controller did not retain its exact browser-surface delegate"
            }
            Self::ErasureInFlight => "macOS extension-controller erasure is already in flight",
            Self::ErasureGenerationExhausted => {
                "macOS extension-controller erasure generation was exhausted"
            }
            Self::NamespaceScopeRequired => {
                "a retained macOS extension controller has no durable namespace scope"
            }
            Self::NamespaceReopenUnavailable => {
                "the durable macOS extension namespace requires restart cleanup that is not implemented"
            }
            Self::UnsupportedNamespaceScope => {
                "the extension namespace scope is not supported by this macOS erasure adapter"
            }
        })
    }
}

impl Error for ControllerRegistryError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SlotAdmission {
    Existing,
    Vacant,
}

struct RegistrySlots<T> {
    entries: HashMap<ProfileId, T>,
    sealed: bool,
    integrity_failed: bool,
}

impl<T> RegistrySlots<T> {
    fn new() -> Self {
        // `HashMap::new` owns no allocation. Controller capacity is consumed
        // only by the explicit probe preparation seam below.
        Self {
            entries: HashMap::new(),
            sealed: false,
            integrity_failed: false,
        }
    }

    fn admission(&self, profile: ProfileId) -> Result<SlotAdmission, ControllerRegistryError> {
        if self.sealed {
            return Err(ControllerRegistryError::Sealed);
        }
        if self.integrity_failed {
            return Err(ControllerRegistryError::IntegrityFailed);
        }
        if self.entries.contains_key(&profile) {
            return Ok(SlotAdmission::Existing);
        }
        if self.entries.len() >= MAX_PERSISTENT_CONTROLLERS {
            return Err(ControllerRegistryError::CapacityExceeded);
        }
        Ok(SlotAdmission::Vacant)
    }

    fn insert_vacant(
        &mut self,
        profile: ProfileId,
        value: T,
    ) -> Result<(), ControllerRegistryError> {
        if self.admission(profile)? != SlotAdmission::Vacant {
            self.integrity_failed = true;
            return Err(ControllerRegistryError::IntegrityFailed);
        }
        match self.entries.entry(profile) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(value);
            }
            std::collections::hash_map::Entry::Occupied(_) => {
                // Never overwrite/drop an exact native owner, even if a
                // future refactor violates the pre-admission contract.
                self.integrity_failed = true;
                return Err(ControllerRegistryError::IntegrityFailed);
            }
        }
        Ok(())
    }

    fn poison(&mut self) {
        self.integrity_failed = true;
    }

    fn seal(&mut self) {
        self.sealed = true;
    }

    fn release_all(&mut self) {
        self.entries.clear();
    }

    fn is_quiescent(&self) -> bool {
        self.sealed && self.entries.is_empty() && !self.integrity_failed
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeAvailability {
    Unprobed,
    Unavailable,
    Supported,
    Broken,
}

pub(super) struct PersistentControllerEntry {
    pub(super) profile: ProfileId,
    pub(super) store: Retained<WKWebsiteDataStore>,
    pub(super) controller: Retained<WKWebExtensionController>,
    browser_surface: MacosExtensionBrowserSurfaceHost,
}

enum PersistentControllerSlot {
    #[allow(dead_code)] // Constructed only by the cfg-gated native probe seam.
    Prepared(PersistentControllerEntry),
    Erasing(Rc<ControllerErasureWitness>),
}

/// Proof retained across Wry construction and consumed by exact post-build
/// readback. Its native fields are intentionally private and it is not Clone.
pub(crate) struct PreparedControllerAttachment {
    profile: ProfileId,
    store: Retained<WKWebsiteDataStore>,
    controller: Retained<WKWebExtensionController>,
}

/// A fresh per-view configuration plus the exact profile objects it must
/// expose after Wry returns the constructed view.
pub(crate) struct PreparedDurableViewConfiguration {
    configuration: Retained<WKWebViewConfiguration>,
    proof: PreparedControllerAttachment,
}

impl PreparedDurableViewConfiguration {
    pub(crate) fn into_parts(
        self,
    ) -> (
        Retained<WKWebViewConfiguration>,
        PreparedControllerAttachment,
    ) {
        (self.configuration, self.proof)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ControllerPreparation {
    RuntimeUnavailable,
    Prepared,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ControllerNamespaceRecoveryAudit {
    RuntimeUnavailable,
    OwnersPresent,
    Absent(ExtensionRuntimeMacosControllerAbsenceAudit),
}

/// UI-thread-owned registry for persistent controller namespaces.
///
/// `new` performs no runtime lookup and allocates neither Rust nor native
/// storage. Entries are inserted only by the authority-bound native runtime
/// activation path; view construction can borrow but never create a namespace.
pub(crate) struct PersistentControllerRegistry {
    slots: RegistrySlots<PersistentControllerSlot>,
    runtime: RuntimeAvailability,
    next_erasure_generation: u64,
    browser_request_sink: Option<crate::EngineEventIngressSink>,
    browser_request_pool: Rc<BrowserRequestPool>,
    runtime_grant_pool: Rc<RuntimeGrantRequestPool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ControllerErasureSettlement {
    Settled,
    Stale,
    IntegrityFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ControllerSurfaceApplication {
    ControllerUnprepared,
    Applied,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ControllerBrowserRequestSettlement {
    ControllerUnprepared,
    Settled,
    Stale,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ControllerRuntimeGrantSettlement {
    ControllerUnprepared,
    Settled,
    Stale,
}

impl PersistentControllerRegistry {
    pub(crate) fn new() -> Self {
        Self {
            slots: RegistrySlots::new(),
            runtime: RuntimeAvailability::Unprobed,
            next_erasure_generation: 0,
            browser_request_sink: None,
            browser_request_pool: Rc::new(BrowserRequestPool::new()),
            runtime_grant_pool: Rc::new(RuntimeGrantRequestPool::new()),
        }
    }

    pub(crate) fn with_browser_request_sink(sink: crate::EngineEventIngressSink) -> Self {
        Self {
            browser_request_sink: Some(sink),
            ..Self::new()
        }
    }

    /// Return a fresh custom configuration only for an exact entry previously
    /// admitted by the native-runtime seam. A missing entry is deliberately
    /// not a request to allocate one.
    pub(crate) fn configuration_for_durable_profile(
        &mut self,
        profile: ProfileId,
    ) -> Result<Option<PreparedDurableViewConfiguration>, ControllerRegistryError> {
        self.slots.admission(profile)?;
        let Some(slot) = self.slots.entries.get(&profile) else {
            return Ok(None);
        };
        let PersistentControllerSlot::Prepared(entry) = slot else {
            return Err(ControllerRegistryError::ErasureInFlight);
        };
        let prepared = catch_native(|| {
            validate_entry_identity(entry)?;
            let mtm = MainThreadMarker::new().ok_or(ControllerRegistryError::MainThreadRequired)?;
            // Every Wry view gets a fresh configuration/user-content
            // controller. Wry is allowed to install per-view scripts and
            // handlers without mutating a sibling view's registrations.
            let configuration = unsafe { WKWebViewConfiguration::new(mtm) };
            unsafe {
                configuration.setWebsiteDataStore(&entry.store);
                configuration.setWebExtensionController(Some(&entry.controller));
            }
            validate_view_configuration(&configuration, &entry.store, &entry.controller)?;
            Ok(PreparedDurableViewConfiguration {
                configuration,
                proof: PreparedControllerAttachment {
                    profile,
                    store: entry.store.clone(),
                    controller: entry.controller.clone(),
                },
            })
        });
        if prepared.is_err() {
            self.slots.poison();
        }
        prepared.map(Some)
    }

    /// Reconciles one newer Shell-owned logical surface with an already
    /// authorized controller. An absent controller is an inert no-op; this
    /// path never allocates a native namespace as a side effect.
    pub(crate) fn apply_browser_surface(
        &mut self,
        surface: &ExtensionBrowserSurface,
        mut webview_for: impl FnMut(ItemId) -> Option<Retained<WKWebView>>,
    ) -> Result<ControllerSurfaceApplication, ControllerRegistryError> {
        self.slots.admission(surface.profile())?;
        let Some(slot) = self.slots.entries.get_mut(&surface.profile()) else {
            return Ok(ControllerSurfaceApplication::ControllerUnprepared);
        };
        let PersistentControllerSlot::Prepared(entry) = slot else {
            return Err(ControllerRegistryError::ErasureInFlight);
        };
        let result = catch_native(|| {
            validate_entry_identity(entry)?;
            entry
                .browser_surface
                .apply(&entry.controller, surface, &mut webview_for)
                .map_err(map_browser_surface_error)?;
            validate_entry_identity(entry)
        });
        if result.is_err() {
            self.slots.poison();
            return result.map(|()| ControllerSurfaceApplication::Applied);
        }
        Ok(ControllerSurfaceApplication::Applied)
    }

    /// Resolves an existing logical tab for action projection without asking
    /// for, retaining, or creating its content webview.
    pub(crate) fn action_tab(
        &mut self,
        profile: ProfileId,
        generation: zephium_core::extensions::ExtensionBrowserSurfaceGeneration,
        tab: ItemId,
    ) -> Result<Option<ControllerActionTab>, ControllerRegistryError> {
        self.slots.admission(profile)?;
        let Some(PersistentControllerSlot::Prepared(entry)) = self.slots.entries.get(&profile)
        else {
            return Ok(None);
        };
        let result = catch_native(|| {
            validate_entry_identity(entry)?;
            entry
                .browser_surface
                .action_tab(generation, tab)
                .map_err(map_browser_surface_error)
        });
        if result
            .as_ref()
            .is_err_and(|error| *error != ControllerRegistryError::BrowserSurfaceStale)
        {
            self.slots.poison();
        }
        result.map(|tab| tab.map(|(tab, resident)| ControllerActionTab { tab, resident }))
    }

    /// Installs the one exact popup callback expectation after the host has
    /// joined runtime, surface, tab and resource authority.
    pub(crate) fn begin_action_popup(
        &mut self,
        profile: ProfileId,
        request: ExtensionActionRequest,
        owner: super::native_runtime::MacosNativeActionPopupOwner,
        tab: super::browser_surface::NativeExtensionTab,
        parent: Retained<NSView>,
        lease: crate::host::NativeResourceLease,
    ) -> Result<Result<(), ExtensionActionRejection>, ControllerRegistryError> {
        self.slots.admission(profile)?;
        let Some(PersistentControllerSlot::Prepared(entry)) = self.slots.entries.get(&profile)
        else {
            return Ok(Err(ExtensionActionRejection::RuntimeUnavailable));
        };
        validate_entry_identity(entry)?;
        if !std::ptr::eq(
            Retained::as_ptr(&entry.controller),
            owner.controller_identity(),
        ) {
            return Ok(Err(ExtensionActionRejection::NativeAdmissionFailed));
        }
        let context = owner.into_context();
        Ok(entry.browser_surface.begin_action_popup(
            request,
            entry.controller.clone(),
            context,
            tab,
            parent,
            lease,
        ))
    }

    pub(crate) fn cancel_action_popup(
        &mut self,
        profile: ProfileId,
        request: ExtensionActionRequestId,
        reason: ExtensionActionRejection,
    ) -> bool {
        let Some(PersistentControllerSlot::Prepared(entry)) = self.slots.entries.get(&profile)
        else {
            return false;
        };
        entry.browser_surface.cancel_action_popup(request, reason)
    }

    pub(crate) fn timeout_action_popup(
        &mut self,
        profile: ProfileId,
        request: ExtensionActionRequestId,
    ) -> bool {
        let Some(PersistentControllerSlot::Prepared(entry)) = self.slots.entries.get(&profile)
        else {
            return false;
        };
        entry.browser_surface.timeout_action_popup(request)
    }

    pub(crate) fn cancel_action_popup_context(
        &mut self,
        profile: ProfileId,
        context: *const WKWebExtensionContext,
        reason: ExtensionActionRejection,
    ) {
        let Some(PersistentControllerSlot::Prepared(entry)) = self.slots.entries.get(&profile)
        else {
            return;
        };
        entry
            .browser_surface
            .cancel_action_popup_context(context, reason);
    }

    #[cfg(feature = "native-web-extension-probes")]
    pub(crate) fn probe_browser_surface_identity(
        &mut self,
        profile: ProfileId,
        window: zephium_core::ids::WindowId,
        tab: ItemId,
    ) -> Result<Option<ProbeBrowserSurfaceIdentity>, ControllerRegistryError> {
        self.slots.admission(profile)?;
        let Some(PersistentControllerSlot::Prepared(entry)) = self.slots.entries.get(&profile)
        else {
            return Ok(None);
        };
        validate_entry_identity(entry)?;
        Ok(entry.browser_surface.probe_identity(window, tab))
    }

    #[cfg(feature = "native-web-extension-probes")]
    pub(crate) fn probe_browser_surface_lifecycle_drops(
        &mut self,
        profile: ProfileId,
    ) -> Result<Option<Arc<std::sync::atomic::AtomicUsize>>, ControllerRegistryError> {
        self.slots.admission(profile)?;
        let Some(PersistentControllerSlot::Prepared(entry)) = self.slots.entries.get(&profile)
        else {
            return Ok(None);
        };
        validate_entry_identity(entry)?;
        Ok(Some(entry.browser_surface.probe_lifecycle_drops()))
    }

    #[cfg(feature = "native-web-extension-probes")]
    pub(crate) fn probe_browser_surface_diagnostics(
        &mut self,
        profile: ProfileId,
    ) -> Result<Option<BrowserSurfaceDiagnostics>, ControllerRegistryError> {
        self.slots.admission(profile)?;
        let Some(PersistentControllerSlot::Prepared(entry)) = self.slots.entries.get(&profile)
        else {
            return Ok(None);
        };
        validate_entry_identity(entry)?;
        Ok(Some(entry.browser_surface.diagnostics()))
    }

    /// Updates only the physical-view binding of one already-projected tab.
    /// A missing controller or tab is intentionally inert: surface and view
    /// publication may arrive in either FIFO order on the native event loop.
    pub(crate) fn bind_browser_surface_view(
        &mut self,
        profile: ProfileId,
        id: ItemId,
        webview: Option<&Retained<WKWebView>>,
    ) -> Result<(), ControllerRegistryError> {
        self.slots.admission(profile)?;
        let Some(slot) = self.slots.entries.get(&profile) else {
            return Ok(());
        };
        let PersistentControllerSlot::Prepared(entry) = slot else {
            return Err(ControllerRegistryError::ErasureInFlight);
        };
        let result = catch_native(|| {
            validate_entry_identity(entry)?;
            entry.browser_surface.bind_webview(id, webview);
            Ok(())
        });
        if result.is_err() {
            self.slots.poison();
        }
        result
    }

    pub(crate) fn settle_browser_request(
        &mut self,
        profile: ProfileId,
        request: zephium_core::extensions::ExtensionBrowserRequestId,
        settlement: zephium_core::extensions::ExtensionBrowserRequestSettlement,
    ) -> Result<ControllerBrowserRequestSettlement, ControllerRegistryError> {
        self.slots.admission(profile)?;
        let Some(slot) = self.slots.entries.get(&profile) else {
            return Ok(ControllerBrowserRequestSettlement::ControllerUnprepared);
        };
        let PersistentControllerSlot::Prepared(entry) = slot else {
            return Ok(ControllerBrowserRequestSettlement::Stale);
        };
        validate_entry_identity(entry)?;
        match entry.browser_surface.settle_request(request, settlement) {
            BrowserRequestSettlementOutcome::Settled => {
                Ok(ControllerBrowserRequestSettlement::Settled)
            }
            BrowserRequestSettlementOutcome::Stale => Ok(ControllerBrowserRequestSettlement::Stale),
            BrowserRequestSettlementOutcome::IntegrityFailed => {
                self.slots.poison();
                Err(ControllerRegistryError::BrowserSurfaceIntegrity)
            }
        }
    }

    pub(crate) fn timeout_browser_request(
        &mut self,
        profile: ProfileId,
        request: zephium_core::extensions::ExtensionBrowserRequestId,
    ) -> bool {
        let Some(PersistentControllerSlot::Prepared(entry)) = self.slots.entries.get(&profile)
        else {
            return false;
        };
        entry.browser_surface.timeout_request(request)
    }

    pub(crate) fn runtime_grant_context_identity(
        &mut self,
        profile: ProfileId,
        request: zephium_core::ports::extensions::ExtensionRuntimeGrantRequestId,
    ) -> Result<Option<*const WKWebExtensionContext>, ControllerRegistryError> {
        self.slots.admission(profile)?;
        let Some(PersistentControllerSlot::Prepared(entry)) = self.slots.entries.get(&profile)
        else {
            return Ok(None);
        };
        validate_entry_identity(entry)?;
        Ok(entry
            .browser_surface
            .runtime_grant_context_identity(request))
    }

    pub(crate) fn finalize_runtime_grant_request(
        &mut self,
        profile: ProfileId,
        request: zephium_core::ports::extensions::ExtensionRuntimeGrantRequestId,
        subject: Option<(
            zephium_core::extensions::ExtensionRuntimeInstance,
            zephium_core::extensions::ExtensionNativeOwnershipKey,
            String,
        )>,
    ) -> Result<bool, ControllerRegistryError> {
        self.slots.admission(profile)?;
        let Some(PersistentControllerSlot::Prepared(entry)) = self.slots.entries.get(&profile)
        else {
            return Ok(false);
        };
        validate_entry_identity(entry)?;
        Ok(entry
            .browser_surface
            .finalize_runtime_grant_request(request, subject))
    }

    pub(crate) fn settle_runtime_grant_request(
        &mut self,
        runtime: zephium_core::extensions::ExtensionRuntimeInstance,
        request: zephium_core::ports::extensions::ExtensionRuntimeGrantRequestId,
        settlement: zephium_core::ports::extensions::ExtensionRuntimeGrantPromptSettlement,
    ) -> Result<ControllerRuntimeGrantSettlement, ControllerRegistryError> {
        let profile = runtime.profile();
        self.slots.admission(profile)?;
        let Some(PersistentControllerSlot::Prepared(entry)) = self.slots.entries.get(&profile)
        else {
            return Ok(ControllerRuntimeGrantSettlement::ControllerUnprepared);
        };
        validate_entry_identity(entry)?;
        match entry
            .browser_surface
            .settle_runtime_grant_request(runtime, request, settlement)
        {
            RuntimeGrantSettlementOutcome::Settled => Ok(ControllerRuntimeGrantSettlement::Settled),
            RuntimeGrantSettlementOutcome::Stale => Ok(ControllerRuntimeGrantSettlement::Stale),
            RuntimeGrantSettlementOutcome::IntegrityFailed => {
                self.slots.poison();
                Err(ControllerRegistryError::BrowserSurfaceIntegrity)
            }
        }
    }

    pub(crate) fn timeout_runtime_grant_request(
        &mut self,
        profile: ProfileId,
        request: zephium_core::ports::extensions::ExtensionRuntimeGrantRequestId,
    ) -> bool {
        let Some(PersistentControllerSlot::Prepared(entry)) = self.slots.entries.get(&profile)
        else {
            return false;
        };
        entry.browser_surface.timeout_runtime_grant_request(request)
    }

    pub(crate) fn cancel_runtime_grant_context(
        &mut self,
        profile: ProfileId,
        context: *const WKWebExtensionContext,
    ) {
        if let Some(PersistentControllerSlot::Prepared(entry)) = self.slots.entries.get(&profile) {
            entry.browser_surface.cancel_runtime_grant_context(context);
        }
    }

    /// Clears an authorized profile's delegate graph after all of its views
    /// and runtime contexts have retired. An absent controller is inert; no
    /// native namespace is created by cleanup.
    pub(crate) fn clear_browser_surface(
        &mut self,
        profile: ProfileId,
    ) -> Result<(), ControllerRegistryError> {
        self.slots.admission(profile)?;
        let Some(slot) = self.slots.entries.get_mut(&profile) else {
            return Ok(());
        };
        let PersistentControllerSlot::Prepared(entry) = slot else {
            return Err(ControllerRegistryError::ErasureInFlight);
        };
        let result = catch_native(|| {
            validate_entry_identity(entry)?;
            if unsafe { entry.controller.extensionContexts() }.count() != 0
                || unsafe { entry.controller.extensions() }.count() != 0
            {
                return Err(ControllerRegistryError::UnexpectedLoadedContext);
            }
            entry.browser_surface.clear(&entry.controller);
            validate_entry_identity(entry)
        });
        if result.is_err() {
            self.slots.poison();
        }
        result
    }

    /// Verify the configuration Wry actually used. On a supported runtime,
    /// an unprepared durable or private view must expose no controller.
    pub(crate) fn attest_built_view(
        &mut self,
        view: &wry::WebView,
        proof: Option<PreparedControllerAttachment>,
    ) -> Result<(), ControllerRegistryError> {
        if self.slots.sealed {
            return Err(ControllerRegistryError::Sealed);
        }
        if self.slots.integrity_failed {
            return Err(ControllerRegistryError::IntegrityFailed);
        }
        if proof.is_none() && self.runtime != RuntimeAvailability::Supported {
            // Calling a 15.4 selector is forbidden on the admitted 14.8.x
            // product floor. No entry can exist until the supported-runtime
            // probe has positively completed.
            return Ok(());
        }

        let result = catch_native(|| {
            let wk = super::super::native::webkit(view);
            let configuration = unsafe { wk.configuration() };
            match proof {
                Some(proof) => {
                    let Some(PersistentControllerSlot::Prepared(entry)) =
                        self.slots.entries.get(&proof.profile)
                    else {
                        return Err(ControllerRegistryError::EntryChanged);
                    };
                    if Retained::as_ptr(&entry.store) != Retained::as_ptr(&proof.store)
                        || Retained::as_ptr(&entry.controller)
                            != Retained::as_ptr(&proof.controller)
                    {
                        return Err(ControllerRegistryError::EntryChanged);
                    }
                    validate_entry_identity(entry)?;
                    validate_view_configuration(&configuration, &proof.store, &proof.controller)
                }
                None => {
                    if unsafe { configuration.webExtensionController() }.is_some() {
                        Err(ControllerRegistryError::UnexpectedViewController)
                    } else {
                        Ok(())
                    }
                }
            }
        });
        if result.is_err() {
            self.slots.poison();
        }
        result
    }

    /// Transfer an exact, already-prepared controller only after the caller
    /// has fenced extension runtimes and dropped every profile view.
    ///
    /// Scope and process state must agree exactly. `None` is admitted only
    /// with no entry; `MacosControllerV1` is admitted only with a retained
    /// entry. A durable V1 scope after restart fails closed until deterministic
    /// reopen is implemented, while a retained entry without the Store marker
    /// is an equally terminal composition mismatch.
    pub(crate) fn begin_profile_erasure(
        &mut self,
        profile: ProfileId,
        attempt: Arc<AtomicBool>,
        namespace_scope: Option<ExtensionNativeNamespaceScope>,
    ) -> Result<ProfileControllerErasure, ControllerRegistryError> {
        self.slots.admission(profile)?;
        let slot = self.slots.entries.get(&profile);
        match (namespace_scope, slot) {
            (None, None) => return Ok(ProfileControllerErasure::NamespaceNotRequired),
            (None, Some(_)) => return Err(ControllerRegistryError::NamespaceScopeRequired),
            (Some(ExtensionNativeNamespaceScope::MacosControllerV1), None) => {
                // V1 is deterministic, but this slice deliberately has no
                // durable reopen authority yet. Never turn a process restart
                // into fabricated absence or create a namespace for an
                // ordinary marker-free profile.
                return Err(ControllerRegistryError::NamespaceReopenUnavailable);
            }
            (Some(ExtensionNativeNamespaceScope::MacosControllerV1), Some(_)) => {}
            (Some(_), _) => return Err(ControllerRegistryError::UnsupportedNamespaceScope),
        };
        let slot = slot.ok_or(ControllerRegistryError::EntryChanged)?;

        match slot {
            PersistentControllerSlot::Prepared(entry) => {
                if let Err(error) = catch_native(|| validate_quiescent_entry(entry)) {
                    self.slots.poison();
                    return Err(error);
                }
            }
            PersistentControllerSlot::Erasing(witness) => {
                if !witness.controller_release_is_proven() || witness.attempt_is_active() {
                    return Err(ControllerRegistryError::ErasureInFlight);
                }
            }
        }

        let generation = self.mint_erasure_generation()?;
        let slot = self
            .slots
            .entries
            .remove(&profile)
            .ok_or(ControllerRegistryError::EntryChanged)?;
        match slot {
            PersistentControllerSlot::Prepared(entry) => {
                let witness = Rc::new(ControllerErasureWitness::new(
                    profile,
                    generation,
                    attempt,
                    Some(entry),
                ));
                self.slots
                    .entries
                    .insert(profile, PersistentControllerSlot::Erasing(witness.clone()));
                Ok(ProfileControllerErasure::Pending(
                    PersistentControllerErasure::new(witness),
                ))
            }
            PersistentControllerSlot::Erasing(previous) => {
                debug_assert!(previous.controller_release_is_proven());
                debug_assert!(!previous.attempt_is_active());
                let witness = Rc::new(ControllerErasureWitness::new(
                    profile, generation, attempt, None,
                ));
                witness.mark_controller_released_for_retry();
                let ticket = witness.ticket();
                self.slots
                    .entries
                    .insert(profile, PersistentControllerSlot::Erasing(witness));
                Ok(ProfileControllerErasure::ControllerAlreadyReleased(ticket))
            }
        }
    }

    fn mint_erasure_generation(&mut self) -> Result<u64, ControllerRegistryError> {
        let Some(generation) = self.next_erasure_generation.checked_add(1) else {
            self.slots.poison();
            return Err(ControllerRegistryError::ErasureGenerationExhausted);
        };
        self.next_erasure_generation = generation;
        Ok(generation)
    }

    pub(crate) fn settle_verified_profile_erasure(
        &mut self,
        profile: ProfileId,
        ticket: ControllerErasureTicket,
        attempt: &Arc<AtomicBool>,
    ) -> ControllerErasureSettlement {
        let Some(slot) = self.slots.entries.get(&profile) else {
            return ControllerErasureSettlement::Stale;
        };
        let PersistentControllerSlot::Erasing(witness) = slot else {
            self.slots.poison();
            return ControllerErasureSettlement::IntegrityFailed;
        };
        if witness.generation() != ticket.generation() {
            return ControllerErasureSettlement::Stale;
        }
        if ticket.profile() != profile
            || !witness.matches_attempt(attempt)
            || attempt.load(Ordering::Acquire)
            || !witness.controller_release_is_proven()
            || witness.retains_native_owner()
        {
            self.slots.poison();
            return ControllerErasureSettlement::IntegrityFailed;
        }
        self.slots.entries.remove(&profile);
        ControllerErasureSettlement::Settled
    }

    pub(crate) fn seal(&mut self) {
        self.slots.seal();
    }

    /// Called only after every Wry view and warm spare has been dropped.
    /// A loaded context makes the clean-shutdown proof permanently false,
    /// while native roots are still released rather than leaked.
    pub(crate) fn release_all_after_views(&mut self) -> bool {
        if !self.slots.sealed {
            self.slots.poison();
        }
        let entries_are_inert = self.slots.entries.values().all(|slot| match slot {
            PersistentControllerSlot::Prepared(entry) => {
                catch_native(|| validate_quiescent_entry(entry)).is_ok()
            }
            PersistentControllerSlot::Erasing(_) => false,
        });
        if !entries_are_inert {
            self.slots.poison();
        } else {
            for slot in self.slots.entries.values_mut() {
                let PersistentControllerSlot::Prepared(entry) = slot else {
                    self.slots.poison();
                    break;
                };
                if catch_native(|| {
                    entry.browser_surface.clear(&entry.controller);
                    validate_entry_identity(entry)
                })
                .is_err()
                {
                    self.slots.poison();
                    break;
                }
            }
        }
        self.slots.release_all();
        self.slots.is_quiescent()
    }

    /// Prepares one persistent controller only after durable native ownership
    /// has established the exact namespace obligation.
    ///
    /// Replays return the existing controller. They never replace a native
    /// object, and ordinary view construction cannot call this seam.
    pub(crate) fn prepare_for_native_runtime(
        &mut self,
        profile: ProfileId,
        namespace_scope: ExtensionNativeNamespaceScope,
    ) -> Result<ControllerPreparation, ControllerRegistryError> {
        if namespace_scope != ExtensionNativeNamespaceScope::MacosControllerV1 {
            return Err(ControllerRegistryError::UnsupportedNamespaceScope);
        }
        match self.ensure_runtime_available()? {
            ControllerPreparation::RuntimeUnavailable => {
                return Ok(ControllerPreparation::RuntimeUnavailable)
            }
            ControllerPreparation::Prepared => {}
        }
        match self.slots.admission(profile)? {
            SlotAdmission::Existing => {
                let result = catch_native(|| {
                    let Some(PersistentControllerSlot::Prepared(entry)) =
                        self.slots.entries.get(&profile)
                    else {
                        return Err(ControllerRegistryError::ErasureInFlight);
                    };
                    validate_entry_identity(entry)
                });
                if result.is_err() {
                    self.slots.poison();
                    return result.map(|()| ControllerPreparation::Prepared);
                }
                return Ok(ControllerPreparation::Prepared);
            }
            SlotAdmission::Vacant => {}
        }

        let sink = self.browser_request_sink.clone();
        let request_pool = self.browser_request_pool.clone();
        let runtime_grant_pool = self.runtime_grant_pool.clone();
        let candidate =
            match catch_native(|| create_entry(profile, sink, request_pool, runtime_grant_pool)) {
                Ok(candidate) => candidate,
                Err(error) => {
                    self.slots.poison();
                    return Err(error);
                }
            };
        let aliased = self.slots.entries.values().any(|other| match other {
            PersistentControllerSlot::Prepared(other) => {
                Retained::as_ptr(&other.controller) == Retained::as_ptr(&candidate.controller)
            }
            PersistentControllerSlot::Erasing(_) => false,
        });
        if aliased {
            self.slots.poison();
            return Err(ControllerRegistryError::ControllerAlias);
        }
        let store_aliased = self.slots.entries.values().any(|other| match other {
            PersistentControllerSlot::Prepared(other) => {
                Retained::as_ptr(&other.store) == Retained::as_ptr(&candidate.store)
            }
            PersistentControllerSlot::Erasing(_) => false,
        });
        if store_aliased {
            self.slots.poison();
            return Err(ControllerRegistryError::StoreAlias);
        }
        if let Err(error) = self
            .slots
            .insert_vacant(profile, PersistentControllerSlot::Prepared(candidate))
        {
            self.slots.poison();
            return Err(error);
        }
        Ok(ControllerPreparation::Prepared)
    }

    /// Reopens and audits the exact deterministic profile namespace without
    /// loading an extension package.
    ///
    /// This is the crash-recovery boundary: an empty controller inventory can
    /// prove runtime-owner absence even though persistent extension storage is
    /// intentionally retained for a later authorized activation. A nonempty
    /// inventory is reported as possible ownership, not treated as corruption
    /// and never unloaded by identity guesswork.
    pub(crate) fn audit_native_runtime_recovery(
        &mut self,
        profile: ProfileId,
        namespace_scope: ExtensionNativeNamespaceScope,
    ) -> Result<ControllerNamespaceRecoveryAudit, ControllerRegistryError> {
        match self.prepare_for_native_runtime(profile, namespace_scope)? {
            ControllerPreparation::RuntimeUnavailable => {
                return Ok(ControllerNamespaceRecoveryAudit::RuntimeUnavailable)
            }
            ControllerPreparation::Prepared => {}
        }
        let result = catch_native(|| {
            let Some(PersistentControllerSlot::Prepared(entry)) = self.slots.entries.get(&profile)
            else {
                return Err(ControllerRegistryError::EntryChanged);
            };
            validate_entry_identity(entry)?;
            let contexts_empty = unsafe { entry.controller.extensionContexts() }.count() == 0;
            let extensions_empty = unsafe { entry.controller.extensions() }.count() == 0;
            if !contexts_empty || !extensions_empty {
                return Ok(ControllerNamespaceRecoveryAudit::OwnersPresent);
            }
            let audit = ExtensionRuntimeMacosControllerAbsenceAudit::try_from_observations(
                true,
                true,
                true,
                contexts_empty,
                extensions_empty,
            )
            .ok_or(ControllerRegistryError::IntegrityFailed)?;
            Ok(ControllerNamespaceRecoveryAudit::Absent(audit))
        });
        if result.is_err() {
            self.slots.poison();
        }
        result
    }

    /// Borrows an already-prepared exact controller without allocating a
    /// namespace as a side effect.
    pub(crate) fn controller_for_native_runtime(
        &mut self,
        profile: ProfileId,
        namespace_scope: ExtensionNativeNamespaceScope,
    ) -> Result<Retained<WKWebExtensionController>, ControllerRegistryError> {
        if namespace_scope != ExtensionNativeNamespaceScope::MacosControllerV1 {
            return Err(ControllerRegistryError::UnsupportedNamespaceScope);
        }
        self.slots.admission(profile)?;
        let result = catch_native(|| {
            let Some(PersistentControllerSlot::Prepared(entry)) = self.slots.entries.get(&profile)
            else {
                return Err(ControllerRegistryError::EntryChanged);
            };
            validate_entry_identity(entry)?;
            Ok(entry.controller.clone())
        });
        if result.is_err() {
            self.slots.poison();
        }
        result
    }

    #[cfg(any(test, feature = "native-web-extension-probes"))]
    #[allow(dead_code)] // Product-path probes consume this through feature-gated modules.
    pub(crate) fn prepare_for_native_probe(
        &mut self,
        profile: ProfileId,
    ) -> Result<ControllerPreparation, ControllerRegistryError> {
        self.prepare_for_native_runtime(profile, ExtensionNativeNamespaceScope::MacosControllerV1)
    }

    fn ensure_runtime_available(
        &mut self,
    ) -> Result<ControllerPreparation, ControllerRegistryError> {
        match self.runtime {
            RuntimeAvailability::Supported => return Ok(ControllerPreparation::Prepared),
            RuntimeAvailability::Unavailable => {
                return Ok(ControllerPreparation::RuntimeUnavailable)
            }
            RuntimeAvailability::Broken => return Err(ControllerRegistryError::IntegrityFailed),
            RuntimeAvailability::Unprobed => {}
        }
        match discover_runtime() {
            Ok(RuntimeAvailability::Unavailable) => {
                self.runtime = RuntimeAvailability::Unavailable;
                Ok(ControllerPreparation::RuntimeUnavailable)
            }
            Ok(RuntimeAvailability::Supported) => {
                self.runtime = RuntimeAvailability::Supported;
                Ok(ControllerPreparation::Prepared)
            }
            Ok(RuntimeAvailability::Unprobed | RuntimeAvailability::Broken) => {
                self.runtime = RuntimeAvailability::Broken;
                self.slots.poison();
                Err(ControllerRegistryError::IntegrityFailed)
            }
            Err(error) => {
                self.runtime = RuntimeAvailability::Broken;
                self.slots.poison();
                Err(error)
            }
        }
    }
}

pub(super) fn catch_native<T>(
    operation: impl FnOnce() -> Result<T, ControllerRegistryError>,
) -> Result<T, ControllerRegistryError> {
    objc2::exception::catch(AssertUnwindSafe(operation))
        .map_err(|_| ControllerRegistryError::NativeException)?
}

fn map_browser_surface_error(error: BrowserSurfaceError) -> ControllerRegistryError {
    match error {
        BrowserSurfaceError::MainThreadRequired => ControllerRegistryError::MainThreadRequired,
        BrowserSurfaceError::ProfileMismatch => {
            ControllerRegistryError::BrowserSurfaceProfileMismatch
        }
        BrowserSurfaceError::StaleGeneration => ControllerRegistryError::BrowserSurfaceStale,
        BrowserSurfaceError::PrivacyClassChanged => {
            ControllerRegistryError::BrowserSurfacePrivacyChanged
        }
        BrowserSurfaceError::IntegrityFailed => ControllerRegistryError::BrowserSurfaceIntegrity,
    }
}

fn validate_store(
    store: &WKWebsiteDataStore,
    profile: ProfileId,
) -> Result<(), ControllerRegistryError> {
    if !unsafe { store.isPersistent() } {
        return Err(ControllerRegistryError::NonPersistentStore);
    }
    let identifier =
        unsafe { store.identifier() }.ok_or(ControllerRegistryError::StoreIdentifierMismatch)?;
    if identifier.as_bytes() != profile.bytes() {
        return Err(ControllerRegistryError::StoreIdentifierMismatch);
    }
    Ok(())
}

pub(super) fn validate_entry_identity(
    entry: &PersistentControllerEntry,
) -> Result<(), ControllerRegistryError> {
    validate_store(&entry.store, entry.profile)?;
    let configuration = unsafe { entry.controller.configuration() };
    if !unsafe { configuration.isPersistent() } {
        return Err(ControllerRegistryError::NonPersistentController);
    }
    let identifier = unsafe { configuration.identifier() }
        .ok_or(ControllerRegistryError::ControllerIdentifierMismatch)?;
    if identifier.as_bytes() != entry.profile.bytes() {
        return Err(ControllerRegistryError::ControllerIdentifierMismatch);
    }
    let default_store = unsafe { configuration.defaultWebsiteDataStore() }
        .ok_or(ControllerRegistryError::ControllerStoreMismatch)?;
    if Retained::as_ptr(&default_store) != Retained::as_ptr(&entry.store) {
        return Err(ControllerRegistryError::ControllerStoreMismatch);
    }
    validate_store(&default_store, entry.profile)?;
    if !entry.browser_surface.is_attached(&entry.controller) {
        return Err(ControllerRegistryError::ControllerDelegateMismatch);
    }
    let basis = unsafe { configuration.webViewConfiguration() };
    let basis_store = unsafe { basis.websiteDataStore() };
    if Retained::as_ptr(&basis_store) != Retained::as_ptr(&entry.store) {
        return Err(ControllerRegistryError::ControllerStoreMismatch);
    }
    validate_store(&basis_store, entry.profile)?;
    Ok(())
}

pub(super) fn validate_quiescent_entry(
    entry: &PersistentControllerEntry,
) -> Result<(), ControllerRegistryError> {
    validate_entry_identity(entry)?;
    if unsafe { entry.controller.extensionContexts() }.count() != 0 {
        return Err(ControllerRegistryError::UnexpectedLoadedContext);
    }
    if unsafe { entry.controller.extensions() }.count() != 0 {
        return Err(ControllerRegistryError::UnexpectedLoadedExtension);
    }
    Ok(())
}

fn validate_view_configuration(
    configuration: &WKWebViewConfiguration,
    store: &WKWebsiteDataStore,
    controller: &WKWebExtensionController,
) -> Result<(), ControllerRegistryError> {
    let actual_store = unsafe { configuration.websiteDataStore() };
    if !std::ptr::eq(Retained::as_ptr(&actual_store), store) {
        return Err(ControllerRegistryError::ViewStoreMismatch);
    }
    let actual_controller = unsafe { configuration.webExtensionController() }
        .ok_or(ControllerRegistryError::ViewControllerMismatch)?;
    if !std::ptr::eq(Retained::as_ptr(&actual_controller), controller) {
        return Err(ControllerRegistryError::ViewControllerMismatch);
    }
    Ok(())
}

fn create_entry(
    profile: ProfileId,
    browser_request_sink: Option<crate::EngineEventIngressSink>,
    browser_request_pool: Rc<BrowserRequestPool>,
    runtime_grant_pool: Rc<RuntimeGrantRequestPool>,
) -> Result<PersistentControllerEntry, ControllerRegistryError> {
    let mtm = MainThreadMarker::new().ok_or(ControllerRegistryError::MainThreadRequired)?;
    let identifier = NSUUID::from_bytes(profile.bytes());
    let store = unsafe { WKWebsiteDataStore::dataStoreForIdentifier(&identifier, mtm) };
    validate_store(&store, profile)?;

    let basis = unsafe { WKWebViewConfiguration::new(mtm) };
    unsafe { basis.setWebsiteDataStore(&store) };
    let controller_configuration = unsafe {
        WKWebExtensionControllerConfiguration::configurationWithIdentifier(&identifier, mtm)
    };
    unsafe {
        controller_configuration.setDefaultWebsiteDataStore(Some(&store));
        controller_configuration.setWebViewConfiguration(Some(&basis));
    }
    let controller = unsafe {
        WKWebExtensionController::initWithConfiguration(
            WKWebExtensionController::alloc(mtm),
            &controller_configuration,
        )
    };
    let browser_surface = MacosExtensionBrowserSurfaceHost::new(
        profile,
        browser_request_sink,
        browser_request_pool,
        runtime_grant_pool,
    )
    .map_err(map_browser_surface_error)?;
    browser_surface.attach(&controller);
    let entry = PersistentControllerEntry {
        profile,
        store,
        controller,
        browser_surface,
    };
    validate_quiescent_entry(&entry)?;
    Ok(entry)
}

fn discover_runtime() -> Result<RuntimeAvailability, ControllerRegistryError> {
    let version = NSProcessInfo::processInfo().operatingSystemVersion();
    if version.majorVersion < 0 || version.minorVersion < 0 || version.patchVersion < 0 {
        return Err(ControllerRegistryError::InvalidRuntimeVersion);
    }
    if !runtime_supports_controllers(version.majorVersion, version.minorVersion) {
        return Ok(RuntimeAvailability::Unavailable);
    }

    let controller = required_class("WKWebExtensionController")?;
    let configuration = required_class("WKWebExtensionControllerConfiguration")?;
    required_class("WKWebExtensionContext")?;
    let data_record = required_class("WKWebExtensionDataRecord")?;
    let webview_configuration = required_class("WKWebViewConfiguration")?;
    let website_data_store = required_class("WKWebsiteDataStore")?;

    require_instance_selectors(
        controller,
        &[
            sel!(initWithConfiguration:),
            sel!(configuration),
            sel!(extensions),
            sel!(extensionContexts),
            sel!(fetchDataRecordsOfTypes:completionHandler:),
            sel!(removeDataOfTypes:fromDataRecords:completionHandler:),
            sel!(delegate),
            sel!(setDelegate:),
            sel!(didOpenWindow:),
            sel!(didCloseWindow:),
            sel!(didFocusWindow:),
            sel!(didOpenTab:),
            sel!(didCloseTab:windowIsClosing:),
            sel!(didActivateTab:previousActiveTab:),
            sel!(didMoveTab:fromIndex:inWindow:),
        ],
    )?;
    require_class_selectors(controller, &[sel!(allExtensionDataTypes)])?;
    require_class_selectors(configuration, &[sel!(configurationWithIdentifier:)])?;
    require_instance_selectors(
        configuration,
        &[
            sel!(isPersistent),
            sel!(identifier),
            sel!(defaultWebsiteDataStore),
            sel!(setDefaultWebsiteDataStore:),
            sel!(webViewConfiguration),
            sel!(setWebViewConfiguration:),
        ],
    )?;
    require_instance_selectors(
        webview_configuration,
        &[
            sel!(websiteDataStore),
            sel!(setWebsiteDataStore:),
            sel!(webExtensionController),
            sel!(setWebExtensionController:),
        ],
    )?;
    require_class_selectors(website_data_store, &[sel!(dataStoreForIdentifier:)])?;
    require_instance_selectors(website_data_store, &[sel!(isPersistent), sel!(identifier)])?;
    require_instance_selectors(
        data_record,
        &[
            sel!(uniqueIdentifier),
            sel!(containedDataTypes),
            sel!(errors),
            sel!(sizeInBytesOfTypes:),
        ],
    )?;
    Ok(RuntimeAvailability::Supported)
}

fn runtime_supports_controllers(major: isize, minor: isize) -> bool {
    major > 15 || (major == 15 && minor >= 4)
}

fn required_class(name: &str) -> Result<&'static AnyClass, ControllerRegistryError> {
    NSClassFromString(&NSString::from_str(name)).ok_or(ControllerRegistryError::IncompleteRuntime)
}

fn require_instance_selectors(
    class: &AnyClass,
    selectors: &[Sel],
) -> Result<(), ControllerRegistryError> {
    if selectors
        .iter()
        .all(|selector| class.responds_to(*selector))
    {
        Ok(())
    } else {
        Err(ControllerRegistryError::IncompleteRuntime)
    }
}

fn require_class_selectors(
    class: &AnyClass,
    selectors: &[Sel],
) -> Result<(), ControllerRegistryError> {
    require_instance_selectors(class.metaclass(), selectors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_install_is_allocation_free_and_dormant() {
        let registry = PersistentControllerRegistry::new();
        assert_eq!(registry.slots.entries.capacity(), 0);
        assert!(registry.slots.entries.is_empty());
        assert_eq!(registry.runtime, RuntimeAvailability::Unprobed);
    }

    #[test]
    fn slots_admit_exactly_sixty_four_profiles_without_eviction() {
        let mut slots = RegistrySlots::new();
        for value in 1..=MAX_PERSISTENT_CONTROLLERS {
            let profile = ProfileId::from(value as u128);
            assert_eq!(slots.admission(profile), Ok(SlotAdmission::Vacant));
            slots.insert_vacant(profile, value).unwrap();
        }
        let first = ProfileId::from(1);
        assert_eq!(slots.admission(first), Ok(SlotAdmission::Existing));
        assert_eq!(slots.entries.get(&first), Some(&1));
        assert_eq!(
            slots.admission(ProfileId::from(10_000)),
            Err(ControllerRegistryError::CapacityExceeded)
        );
        assert_eq!(slots.entries.len(), MAX_PERSISTENT_CONTROLLERS);
        assert_eq!(slots.entries.get(&first), Some(&1));
    }

    #[test]
    fn poison_and_seal_are_sticky_and_never_evict_owned_entries() {
        let profile = ProfileId::from(7);
        let mut poisoned = RegistrySlots::new();
        poisoned.insert_vacant(profile, 1_u8).unwrap();
        poisoned.poison();
        assert_eq!(
            poisoned.admission(profile),
            Err(ControllerRegistryError::IntegrityFailed)
        );
        assert_eq!(poisoned.entries.get(&profile), Some(&1));
        poisoned.release_all();
        assert!(!poisoned.is_quiescent());

        let mut sealed = RegistrySlots::new();
        sealed.insert_vacant(profile, 1_u8).unwrap();
        sealed.seal();
        assert_eq!(
            sealed.admission(profile),
            Err(ControllerRegistryError::Sealed)
        );
        assert_eq!(sealed.entries.get(&profile), Some(&1));
        sealed.release_all();
        assert!(sealed.is_quiescent());
    }

    #[test]
    fn supported_runtime_boundary_is_exactly_macos_fifteen_four() {
        assert!(!runtime_supports_controllers(14, 8));
        assert!(!runtime_supports_controllers(15, 3));
        assert!(runtime_supports_controllers(15, 4));
        assert!(runtime_supports_controllers(15, 7));
        assert!(runtime_supports_controllers(16, 0));
        assert!(runtime_supports_controllers(26, 0));
    }

    #[test]
    fn absent_process_local_entry_does_not_create_an_erasure_namespace() {
        let profile = ProfileId::from(41);
        let mut registry = PersistentControllerRegistry::new();
        let attempt = Arc::new(AtomicBool::new(true));
        assert!(matches!(
            registry.begin_profile_erasure(profile, attempt, None),
            Ok(ProfileControllerErasure::NamespaceNotRequired)
        ));
        assert!(registry.slots.entries.is_empty());
        assert_eq!(registry.next_erasure_generation, 0);
    }

    #[test]
    fn native_controller_borrow_never_creates_a_missing_namespace() {
        let profile = ProfileId::from(42);
        let mut registry = PersistentControllerRegistry::new();
        assert!(matches!(
            registry.controller_for_native_runtime(
                profile,
                ExtensionNativeNamespaceScope::MacosControllerV1,
            ),
            Err(ControllerRegistryError::EntryChanged)
        ));
        assert!(registry.slots.entries.is_empty());
        assert_eq!(registry.runtime, RuntimeAvailability::Unprobed);
    }

    #[test]
    fn erasure_generation_exhaustion_is_sticky_and_cannot_wrap() {
        let mut registry = PersistentControllerRegistry::new();
        registry.next_erasure_generation = u64::MAX;
        assert_eq!(
            registry.mint_erasure_generation(),
            Err(ControllerRegistryError::ErasureGenerationExhausted)
        );
        assert!(registry.slots.integrity_failed);
        assert_eq!(registry.next_erasure_generation, u64::MAX);
    }

    #[test]
    fn exact_released_generation_settles_once_and_duplicate_is_stale() {
        let profile = ProfileId::from(43);
        let attempt = Arc::new(AtomicBool::new(false));
        let witness = Rc::new(ControllerErasureWitness::new(
            profile,
            7,
            attempt.clone(),
            None,
        ));
        witness.mark_controller_released_for_retry();
        let ticket = witness.ticket();
        let mut registry = PersistentControllerRegistry::new();
        registry
            .slots
            .entries
            .insert(profile, PersistentControllerSlot::Erasing(witness));

        assert_eq!(
            registry.settle_verified_profile_erasure(profile, ticket, &attempt),
            ControllerErasureSettlement::Settled
        );
        assert_eq!(
            registry.settle_verified_profile_erasure(profile, ticket, &attempt),
            ControllerErasureSettlement::Stale
        );
        assert!(!registry.slots.integrity_failed);
        assert!(registry.slots.entries.is_empty());
    }

    #[test]
    fn stale_generation_cannot_retire_a_newer_released_attempt() {
        let profile = ProfileId::from(44);
        let attempt = Arc::new(AtomicBool::new(false));
        let witness = Rc::new(ControllerErasureWitness::new(
            profile,
            9,
            attempt.clone(),
            None,
        ));
        witness.mark_controller_released_for_retry();
        let mut registry = PersistentControllerRegistry::new();
        registry
            .slots
            .entries
            .insert(profile, PersistentControllerSlot::Erasing(witness));

        assert_eq!(
            registry.settle_verified_profile_erasure(
                profile,
                ControllerErasureTicket {
                    profile,
                    generation: 8,
                },
                &attempt,
            ),
            ControllerErasureSettlement::Stale
        );
        assert!(!registry.slots.integrity_failed);
        assert!(registry.slots.entries.contains_key(&profile));
    }

    #[test]
    fn website_store_retry_rebinds_attempt_without_recreating_controller() {
        let profile = ProfileId::from(45);
        let previous_attempt = Arc::new(AtomicBool::new(false));
        let witness = Rc::new(ControllerErasureWitness::new(
            profile,
            1,
            previous_attempt,
            None,
        ));
        witness.mark_controller_released_for_retry();
        let mut registry = PersistentControllerRegistry::new();
        registry.next_erasure_generation = 1;
        registry
            .slots
            .entries
            .insert(profile, PersistentControllerSlot::Erasing(witness));

        let retry_attempt = Arc::new(AtomicBool::new(true));
        let ticket = match registry
            .begin_profile_erasure(
                profile,
                retry_attempt.clone(),
                Some(ExtensionNativeNamespaceScope::MacosControllerV1),
            )
            .unwrap()
        {
            ProfileControllerErasure::ControllerAlreadyReleased(ticket) => ticket,
            _ => panic!("released controller unexpectedly restarted physical erasure"),
        };
        assert_eq!(ticket.generation(), 2);
        let Some(PersistentControllerSlot::Erasing(witness)) = registry.slots.entries.get(&profile)
        else {
            panic!("retry witness disappeared");
        };
        assert!(witness.matches_attempt(&retry_attempt));
        assert!(witness.controller_release_is_proven());
        assert!(!witness.retains_native_owner());
    }

    #[test]
    fn active_controller_erasure_refuses_retry_without_replacing_debt() {
        let profile = ProfileId::from(46);
        let current_attempt = Arc::new(AtomicBool::new(true));
        let witness = Rc::new(ControllerErasureWitness::new(
            profile,
            1,
            current_attempt,
            None,
        ));
        let retained = witness.clone();
        let mut registry = PersistentControllerRegistry::new();
        registry
            .slots
            .entries
            .insert(profile, PersistentControllerSlot::Erasing(witness));

        assert!(matches!(
            registry.begin_profile_erasure(
                profile,
                Arc::new(AtomicBool::new(true)),
                Some(ExtensionNativeNamespaceScope::MacosControllerV1),
            ),
            Err(ControllerRegistryError::ErasureInFlight)
        ));
        let Some(PersistentControllerSlot::Erasing(actual)) = registry.slots.entries.get(&profile)
        else {
            panic!("active debt disappeared");
        };
        assert!(Rc::ptr_eq(actual, &retained));
        assert_eq!(registry.next_erasure_generation, 0);
    }

    #[test]
    fn shutdown_never_claims_clean_with_controller_erasure_debt() {
        let profile = ProfileId::from(47);
        let witness = Rc::new(ControllerErasureWitness::new(
            profile,
            1,
            Arc::new(AtomicBool::new(true)),
            None,
        ));
        let mut registry = PersistentControllerRegistry::new();
        registry
            .slots
            .entries
            .insert(profile, PersistentControllerSlot::Erasing(witness));
        registry.seal();
        assert!(!registry.release_all_after_views());
        assert!(registry.slots.entries.is_empty());
        assert!(registry.slots.integrity_failed);
    }

    #[test]
    fn durable_v1_scope_without_retained_entry_refuses_to_reopen_or_create() {
        let profile = ProfileId::from(48);
        let mut registry = PersistentControllerRegistry::new();
        assert!(matches!(
            registry.begin_profile_erasure(
                profile,
                Arc::new(AtomicBool::new(true)),
                Some(ExtensionNativeNamespaceScope::MacosControllerV1),
            ),
            Err(ControllerRegistryError::NamespaceReopenUnavailable)
        ));
        assert!(registry.slots.entries.is_empty());
        assert_eq!(registry.next_erasure_generation, 0);
        assert!(!registry.slots.integrity_failed);
    }

    #[test]
    fn retained_controller_without_durable_scope_is_a_marker_mismatch() {
        let profile = ProfileId::from(49);
        let witness = Rc::new(ControllerErasureWitness::new(
            profile,
            1,
            Arc::new(AtomicBool::new(false)),
            None,
        ));
        witness.mark_controller_released_for_retry();
        let retained = witness.clone();
        let mut registry = PersistentControllerRegistry::new();
        registry
            .slots
            .entries
            .insert(profile, PersistentControllerSlot::Erasing(witness));

        assert!(matches!(
            registry.begin_profile_erasure(profile, Arc::new(AtomicBool::new(true)), None,),
            Err(ControllerRegistryError::NamespaceScopeRequired)
        ));
        let Some(PersistentControllerSlot::Erasing(actual)) = registry.slots.entries.get(&profile)
        else {
            panic!("marker mismatch dropped the retained debt");
        };
        assert!(Rc::ptr_eq(actual, &retained));
        assert_eq!(registry.next_erasure_generation, 0);
        assert!(!registry.slots.integrity_failed);
    }
}
