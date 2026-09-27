use zephium_core::extensions::{
    ExtensionBackgroundEnvironment, ExtensionBackgroundWorkerType, ExtensionCompatibilityLevel,
    ExtensionCompatibilityTargetId, ExtensionContentScriptWorld, ExtensionManifestDeclaration,
};
use zephium_core::injection::{MatchOptions, MatchPatternComponents, MatchPatternScheme, MatchSet};
use zephium_extension_package::{
    AdmittedExtensionManifest, ExtensionManifestCompatibilityPolicy,
    ExtensionManifestCompatibilitySubject,
};

pub use zephium_core::extensions::ExtensionBetaRuntimeTarget as BetaRuntimeTarget;

/// Explicit source-policy limitation. The eventual permission/compatibility
/// consent must bind these exact limitations to the output being installed.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum BetaCompatibilityLimitation {
    /// Broad patterns will not receive access to local file documents.
    LocalFileAccessExcluded,
    /// This browser does not provide Chrome-account-backed extension sync.
    CloudStorageSyncUnavailable,
    /// WKWebExtension owns worker suspension/restart, not Chromium's lifecycle.
    PlatformBackgroundLifecycle,
    /// Native runtime may expose only generic fonts instead of Chrome's font list.
    FontEnumerationUnavailable,
    /// A Chrome version declaration is not a WebKit capability guarantee.
    UpstreamBrowserVersionNotApplicable,
    /// The original unlimitedStorage request is replaced by the native quota.
    BoundedStorageQuota,
    /// Glob-bearing scripts run in supported top-level documents only.
    MainDocumentContentScriptsOnly,
    /// Fragment-bearing URLs do not use the partially qualified glob relay.
    FragmentUrlContentScriptsUnavailable,
    /// Font-face-only stylesheets are withheld from glob-bearing groups.
    ContentScriptFontsUnavailable,
    /// This browser exposes no side panel API or surface.
    SidePanelUnavailable,
    /// Offscreen documents support only the on-demand LOCAL_STORAGE reason.
    OffscreenLocalStorageOnly,
    /// Original sandbox documents are inert; inline widgets cannot run.
    SandboxedPagesUnavailable,
    /// Clipboard reads, including auto-clear and SSH import, are unavailable.
    ClipboardReadUnavailable,
}

pub(super) struct CompiledBetaPolicy {
    runtime: BetaRuntimeTarget,
    target: ExtensionCompatibilityTargetId,
    external: bool,
    brokered: bool,
    capability_broker: bool,
    capabilities_v2: bool,
    identity: bool,
    main_document_globs: bool,
    adapted: bool,
    publisher: bool,
    bounded_storage: bool,
    output: bool,
}

impl CompiledBetaPolicy {
    pub(super) fn new(runtime: BetaRuntimeTarget) -> Self {
        Self {
            runtime,
            external: false,
            brokered: false,
            capability_broker: false,
            capabilities_v2: false,
            identity: false,
            main_document_globs: false,
            adapted: false,
            publisher: false,
            bounded_storage: false,
            output: false,
            target: ExtensionCompatibilityTargetId::parse_exact(runtime.target_id())
                .expect("compiled Beta target is valid"),
        }
    }

    pub(super) fn for_external(runtime: BetaRuntimeTarget) -> Self {
        Self {
            external: true,
            ..Self::new(runtime)
        }
    }

    pub(super) fn with_brokered(mut self, enabled: bool) -> Self {
        self.brokered = enabled && self.external && self.runtime == BetaRuntimeTarget::MacosNative;
        if self.brokered {
            self.target = ExtensionCompatibilityTargetId::parse_exact(
                zephium_core::extensions::MACOS_NATIVE_BROKERED_COMPATIBILITY_TARGET,
            )
            .expect("compiled target");
        }
        self
    }
    pub(super) fn with_history_v2(mut self, enabled: bool) -> Self {
        if enabled && self.brokered {
            self.target = ExtensionCompatibilityTargetId::parse_exact(
                zephium_core::extensions::LOCAL_MACOS_HISTORY_V2_COMPATIBILITY_TARGET,
            )
            .expect("compiled target");
        }
        self
    }
    pub(super) fn with_capability_broker(mut self, enabled: bool) -> Self {
        self.capability_broker =
            enabled && self.external && self.runtime == BetaRuntimeTarget::MacosNative;
        if self.capability_broker {
            self.target = ExtensionCompatibilityTargetId::parse_exact(
                zephium_core::extensions::LOCAL_MACOS_CAPABILITIES_V1_COMPATIBILITY_TARGET,
            )
            .expect("compiled target");
        }
        self
    }
    pub(super) fn with_capabilities_v2(mut self, enabled: bool) -> Self {
        self.capabilities_v2 =
            enabled && self.external && self.runtime == BetaRuntimeTarget::MacosNative;
        if self.capabilities_v2 {
            self.adapted = true;
            self.publisher = false;
            self.target = ExtensionCompatibilityTargetId::parse_exact(
                zephium_core::extensions::LOCAL_MACOS_CAPABILITIES_V2_COMPATIBILITY_TARGET,
            )
            .expect("compiled target");
        }
        self
    }
    pub(super) fn with_history_v3(mut self, enabled: bool) -> Self {
        if enabled && self.brokered {
            self.target = ExtensionCompatibilityTargetId::parse_exact(
                zephium_core::extensions::LOCAL_MACOS_HISTORY_V3_COMPATIBILITY_TARGET,
            )
            .expect("compiled target");
        }
        self
    }
    pub(super) fn for_output(mut self) -> Self {
        self.output = true;
        self
    }
    pub(super) fn with_bounded_storage(mut self, enabled: bool) -> Self {
        self.bounded_storage = enabled && self.external;
        if self.bounded_storage && !self.brokered && !self.capability_broker && !self.adapted {
            self.target = ExtensionCompatibilityTargetId::parse_exact(
                self.runtime.bounded_storage_target_id(),
            )
            .expect("compiled target");
        }
        self
    }
    pub(super) fn with_adapted(mut self, enabled: bool, publisher: bool) -> Self {
        self.adapted = enabled
            && self.external
            && !self.brokered
            && !self.capability_broker
            && self.runtime == BetaRuntimeTarget::MacosNative;
        self.publisher = self.adapted && publisher;
        if self.adapted {
            self.target = ExtensionCompatibilityTargetId::parse_exact(
                zephium_core::extensions::LOCAL_MACOS_ADAPTED_COMPATIBILITY_TARGET,
            )
            .expect("compiled target");
        }
        self
    }

    pub(super) fn with_identity(mut self, enabled: bool) -> Self {
        self.identity = enabled
            && self.external
            && self.runtime == BetaRuntimeTarget::MacosNative
            && !self.brokered
            && !self.capability_broker;
        if self.identity {
            self.adapted = true;
            self.publisher = false;
            self.target = ExtensionCompatibilityTargetId::parse_exact(
                zephium_core::extensions::LOCAL_MACOS_IDENTITY_V1_COMPATIBILITY_TARGET,
            )
            .expect("compiled target");
        }
        self
    }

    pub(super) fn with_main_document_globs(mut self, enabled: bool) -> Self {
        self.main_document_globs = enabled
            && self.external
            && self.runtime == BetaRuntimeTarget::MacosNative
            && !self.brokered
            && !self.capability_broker;
        if self.main_document_globs {
            self.target = ExtensionCompatibilityTargetId::parse_exact(
                zephium_core::extensions::LOCAL_MACOS_MAIN_DOCUMENT_GLOBS_V1_COMPATIBILITY_TARGET,
            )
            .expect("compiled target");
        }
        self
    }

    pub(super) fn limitations(
        &self,
        manifest: &AdmittedExtensionManifest,
    ) -> Vec<BetaCompatibilityLimitation> {
        let mut limitations = std::collections::BTreeSet::new();
        for row in manifest.descriptor().compatibility() {
            if row.level() != ExtensionCompatibilityLevel::Degraded {
                continue;
            }
            match row.declaration() {
                ExtensionManifestDeclaration::RequiredApiPermission(name)
                | ExtensionManifestDeclaration::OptionalApiPermission(name)
                    if name.as_str() == "unlimitedStorage" && self.bounded_storage =>
                {
                    limitations.insert(BetaCompatibilityLimitation::BoundedStorageQuota);
                }
                ExtensionManifestDeclaration::RequiredApiPermission(name)
                | ExtensionManifestDeclaration::OptionalApiPermission(name)
                    if name.as_str() == "storage" =>
                {
                    limitations.insert(BetaCompatibilityLimitation::CloudStorageSyncUnavailable);
                }
                ExtensionManifestDeclaration::Background => {
                    limitations.insert(BetaCompatibilityLimitation::PlatformBackgroundLifecycle);
                }
                ExtensionManifestDeclaration::RequiredApiPermission(name)
                | ExtensionManifestDeclaration::OptionalApiPermission(name)
                    if name.as_str() == "fontSettings" =>
                {
                    limitations.insert(BetaCompatibilityLimitation::FontEnumerationUnavailable);
                }
                ExtensionManifestDeclaration::RequiredApiPermission(name)
                | ExtensionManifestDeclaration::OptionalApiPermission(name)
                    if name.as_str() == "sidePanel" && self.main_document_globs =>
                {
                    limitations.insert(BetaCompatibilityLimitation::SidePanelUnavailable);
                }
                ExtensionManifestDeclaration::SidePanel { .. } if self.main_document_globs => {
                    limitations.insert(BetaCompatibilityLimitation::SidePanelUnavailable);
                }
                ExtensionManifestDeclaration::RequiredApiPermission(name)
                    if self.capabilities_v2 && name.as_str() == "clipboardRead" =>
                {
                    limitations.insert(BetaCompatibilityLimitation::ClipboardReadUnavailable);
                }
                ExtensionManifestDeclaration::RequiredApiPermission(name)
                    if self.capabilities_v2 && name.as_str() == "sidePanel" =>
                {
                    limitations.insert(BetaCompatibilityLimitation::SidePanelUnavailable);
                }
                ExtensionManifestDeclaration::Offscreen if self.capabilities_v2 => {
                    limitations.insert(BetaCompatibilityLimitation::OffscreenLocalStorageOnly);
                }
                ExtensionManifestDeclaration::Sandbox if self.capabilities_v2 => {
                    limitations.insert(BetaCompatibilityLimitation::SandboxedPagesUnavailable);
                }
                ExtensionManifestDeclaration::SidePanel { .. } if self.capabilities_v2 => {
                    limitations.insert(BetaCompatibilityLimitation::SidePanelUnavailable);
                }
                ExtensionManifestDeclaration::ContentScript { index, .. }
                    if self.main_document_globs =>
                {
                    if let Some(script) = manifest
                        .descriptor()
                        .declarations()
                        .execution()
                        .content_scripts()
                        .get(usize::from(*index))
                    {
                        if script.globs().is_present() {
                            limitations.insert(
                                BetaCompatibilityLimitation::MainDocumentContentScriptsOnly,
                            );
                            limitations.insert(
                                BetaCompatibilityLimitation::FragmentUrlContentScriptsUnavailable,
                            );
                            if script.css_entries() > 0 {
                                limitations.insert(
                                    BetaCompatibilityLimitation::ContentScriptFontsUnavailable,
                                );
                            }
                        }
                    }
                }
                ExtensionManifestDeclaration::MinimumChromiumVersion(_) => {
                    limitations
                        .insert(BetaCompatibilityLimitation::UpstreamBrowserVersionNotApplicable);
                }
                ExtensionManifestDeclaration::RequiredHostPermission(_)
                | ExtensionManifestDeclaration::OptionalHostPermission(_)
                | ExtensionManifestDeclaration::ContentScript { .. }
                | ExtensionManifestDeclaration::WebAccessibleResources { .. } => {
                    limitations.insert(BetaCompatibilityLimitation::LocalFileAccessExcluded);
                }
                _ => {}
            }
        }
        limitations.into_iter().collect()
    }
}

impl ExtensionManifestCompatibilityPolicy for CompiledBetaPolicy {
    fn target(&self) -> &ExtensionCompatibilityTargetId {
        &self.target
    }

    fn classify(
        &self,
        subject: ExtensionManifestCompatibilitySubject<'_>,
    ) -> Option<ExtensionCompatibilityLevel> {
        use ExtensionCompatibilityLevel::{Compatible, Degraded, Unsupported};
        use ExtensionManifestDeclaration as D;
        // This is the initial native MV3 source subset, not an API-parity
        // claim. Unknown required AND optional authority is explicitly denied.
        Some(match subject.declaration() {
            D::OptionalApiPermission(name)
                if self.capability_broker
                    && matches!(name.as_str(), "history" | "search" | "sessions") =>
            {
                Unsupported
            }
            D::OptionalApiPermission(name)
                if self.capabilities_v2
                    && matches!(
                        name.as_str(),
                        "offscreen" | "clipboardRead" | "sidePanel" | "nativeMessaging"
                    ) =>
            {
                Unsupported
            }
            D::RequiredApiPermission(name) | D::OptionalApiPermission(name) => {
                match name.as_str() {
                    "storage" => Degraded,
                    "cookies" | "clipboardWrite"
                        if self.external && self.runtime == BetaRuntimeTarget::MacosNative =>
                    {
                        Compatible
                    }
                    "declarativeNetRequest"
                    | "declarativeNetRequestWithHostAccess"
                    | "declarativeNetRequestFeedback"
                    | "webRequest"
                        if self.external && self.runtime == BetaRuntimeTarget::MacosNative =>
                    {
                        Degraded
                    }
                    "unlimitedStorage" if self.bounded_storage && !self.output => Degraded,
                    "tabs" | "scripting" | "activeTab" => Compatible,
                    "alarms" | "contextMenus" | "menus" if self.external => Compatible,
                    "fontSettings" if self.external => Degraded,
                    "bookmarks" | "history" | "sessions" | "notifications" | "favicon"
                    | "webNavigation" | "search"
                        if self.brokered =>
                    {
                        Degraded
                    }
                    "history" | "search" | "sessions" if self.capability_broker => Degraded,
                    "offscreen" if self.capabilities_v2 => Degraded,
                    "clipboardRead" | "sidePanel" if self.capabilities_v2 && !self.output => {
                        Degraded
                    }
                    "nativeMessaging" if self.capabilities_v2 && self.output => Degraded,
                    "identity" if self.identity => Degraded,
                    "sidePanel" if self.main_document_globs => Degraded,
                    "nativeMessaging" if self.brokered && self.output => Degraded,
                    "nativeMessaging" if self.capability_broker && self.output => Degraded,
                    "nativeMessaging" if self.identity && self.output => Degraded,
                    "nativeMessaging" if self.publisher => Degraded,
                    "cookies" if self.adapted => Compatible,
                    "declarativeNetRequest"
                    | "declarativeNetRequestWithHostAccess"
                    | "declarativeNetRequestFeedback"
                    | "webRequest"
                    | "webNavigation"
                    | "notifications"
                    | "privacy"
                    | "downloads"
                    | "idle"
                    | "management"
                    | "webRequestAuthProvider"
                        if self.adapted =>
                    {
                        Degraded
                    }
                    _ => Unsupported,
                }
            }
            D::OptionalHostPermission(pattern)
                if self.external && self.runtime == BetaRuntimeTarget::MacosNative =>
            {
                use zephium_core::injection::{MatchPattern, MatchPatternHost, MatchPatternPort};
                match MatchPattern::parse(pattern).ok()?.components() {
                    MatchPatternComponents::Standard {
                        port,
                        path,
                        host: name,
                        ..
                    } if port != MatchPatternPort::Any
                        || path.as_str() != "/*"
                        || matches!(name, Some(MatchPatternHost::ExactIpv6(_))) =>
                    {
                        Unsupported
                    }
                    _ => host(pattern),
                }
            }
            D::RequiredHostPermission(pattern) | D::OptionalHostPermission(pattern) => {
                host(pattern)
            }
            D::ContentScript { index, .. } => {
                let script = subject
                    .declarations()
                    .execution()
                    .content_scripts()
                    .get(usize::from(*index))?;
                if script.globs().is_present() && self.main_document_globs {
                    if script.world() != ExtensionContentScriptWorld::Isolated
                        || script.run_at()
                            != zephium_core::extensions::ExtensionContentScriptRunAt::DocumentIdle
                        || script.javascript_entries() == 0
                        || script.matches().options().match_origin_as_fallback
                        || !script
                            .matches()
                            .includes()
                            .iter()
                            .any(|pattern| pattern.components().includes_http_or_https())
                        || matches(script.matches(), true) == Unsupported
                    {
                        Unsupported
                    } else {
                        Degraded
                    }
                } else if (!self.external
                    && (script.world() != ExtensionContentScriptWorld::Isolated
                        || script.matches().options() != MatchOptions::default()))
                    || script.globs().is_present()
                {
                    Unsupported
                } else {
                    matches(
                        script.matches(),
                        (self.brokered || self.capability_broker || self.adapted) && !self.output,
                    )
                }
            }
            D::Background => {
                let background = subject.declarations().background()?;
                if self.external
                    && !self.output
                    && background.environment() == ExtensionBackgroundEnvironment::CrossBrowser
                    && ((self.runtime == BetaRuntimeTarget::MacosNative
                        && (self.adapted || self.brokered || self.capability_broker))
                        || self.runtime == BetaRuntimeTarget::WindowsNative)
                {
                    if self.runtime == BetaRuntimeTarget::WindowsNative {
                        Compatible
                    } else {
                        Degraded
                    }
                } else if self.adapted
                    && self.output
                    && background.environment() == ExtensionBackgroundEnvironment::Document
                {
                    Degraded
                } else if background.environment() != ExtensionBackgroundEnvironment::ServiceWorker
                    || !(background.worker_type() == ExtensionBackgroundWorkerType::Classic
                        || self.brokered
                        || self.capability_broker
                        || self.adapted
                        || (self.external && self.runtime == BetaRuntimeTarget::WindowsNative))
                {
                    Unsupported
                } else if self.runtime == BetaRuntimeTarget::MacosNative {
                    Degraded
                } else {
                    Compatible
                }
            }
            D::Action | D::OptionsPage { .. } => Compatible,
            D::Commands(_) if self.external => Compatible,
            D::MinimumChromiumVersion(_) if self.external => Degraded,
            D::ExtensionPagesCsp => {
                // The structural parser already rejects remote executable
                // sources. Wasm/eval adaptation needs a separate compiled profile.
                if self.adapted {
                    Degraded
                } else if subject
                    .resources()
                    .extension_pages_csp()
                    .contains("'wasm-unsafe-eval'")
                {
                    Unsupported
                } else {
                    Compatible
                }
            }
            D::WebAccessibleResources { index, .. } => {
                let resource = subject
                    .declarations()
                    .execution()
                    .web_accessible_resources()
                    .get(usize::from(*index))?;
                if resource.extension_id_count() != 0 {
                    Unsupported
                } else {
                    resource
                        .matches()
                        .map_or(Unsupported, |hosts| matches(hosts.matches(), false))
                }
            }
            D::NativeMessaging if self.brokered && self.output => Degraded,
            D::NativeMessaging if self.capability_broker && self.output => Degraded,
            D::NativeMessaging if self.capabilities_v2 && self.output => Degraded,
            D::NativeMessaging if self.identity && self.output => Degraded,
            D::NativeMessaging if self.publisher => Degraded,
            D::DeclarativeNetRequest(_) if self.adapted => Degraded,
            D::ManagedStorageSchema { .. }
                if self.external
                    && (self.adapted || self.brokered)
                    && subject
                        .declarations()
                        .required_api()
                        .contains_exact("storage") =>
            {
                Degraded
            }
            D::SidePanel { .. } if self.main_document_globs => Degraded,
            D::Offscreen | D::Sandbox | D::SidePanel { .. } if self.capabilities_v2 => Degraded,
            D::Offscreen
            | D::NativeMessaging
            | D::Override(_)
            | D::Sandbox
            | D::MinimumChromiumVersion(_)
            | D::Commands(_)
            | D::SidePanel { .. }
            | D::ManagedStorageSchema { .. }
            | D::DeclarativeNetRequest(_)
            | D::UnmodeledAuthority(_) => Unsupported,
        })
    }
}

fn host(pattern: &str) -> ExtensionCompatibilityLevel {
    use ExtensionCompatibilityLevel::{Compatible, Degraded, Unsupported};
    if pattern == "<all_urls>" {
        Degraded
    } else if pattern.starts_with("http://")
        || pattern.starts_with("https://")
        || pattern.starts_with("*://")
    {
        Compatible
    } else {
        Unsupported
    }
}

fn matches(set: &MatchSet, exclude_file: bool) -> ExtensionCompatibilityLevel {
    use ExtensionCompatibilityLevel::{Compatible, Degraded, Unsupported};
    let mut result = Compatible;
    for pattern in set.includes() {
        let level = match pattern.components() {
            MatchPatternComponents::AllUrls => Degraded,
            MatchPatternComponents::Standard {
                scheme: MatchPatternScheme::File,
                ..
            } => {
                if exclude_file {
                    Degraded
                } else {
                    Unsupported
                }
            }
            MatchPatternComponents::Standard { .. } => Compatible,
        };
        match level {
            Unsupported => return Unsupported,
            Degraded => result = Degraded,
            _ => {}
        }
    }
    result
}
