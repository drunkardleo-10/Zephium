//! Exact, bounded translation from effective Zephium grants to macOS shapes.
//!
//! WebKit's match-pattern semantics are not identical to Chrome's. This
//! compiler accepts only the intersection proven by the live macOS probe and
//! translates from Core's parsed components rather than reparsing canonical
//! strings. Core `Denied` rows mean only declaration-local absence, never an
//! explicit user denial. Plans therefore contain complete granted sets only
//! and mandate clearing both native denied sets during bulk replacement. The
//! resulting value is structural and non-authorizing.

use std::cmp::Ordering;
use std::error::Error;
use std::fmt;
use std::mem::size_of;
use std::net::Ipv4Addr;
use std::ops::Range;

use zephium_core::extensions::{
    ExtensionGrantBrowsingContext, ExtensionGrantDigest, ExtensionGrantRevision,
    ExtensionNativeGrantDecision, ExtensionNativeGrantRequirement, ExtensionNativeGrantSnapshot,
    ExtensionRuntimeInstance, MAX_EXTENSION_API_PERMISSIONS, MAX_EXTENSION_HOST_GRANTS,
};
use zephium_core::injection::{
    MatchPattern, MatchPatternComponents, MatchPatternHost, MatchPatternPort, MatchPatternScheme,
    MAX_MATCH_PATTERN_BYTES,
};

const MAX_MACOS_NATIVE_API_PERMISSIONS: usize = MAX_EXTENSION_API_PERMISSIONS;
const MAX_MACOS_NATIVE_GRANTED_HOST_PATTERNS: usize = MAX_EXTENSION_HOST_GRANTS * 2;
const MAX_MACOS_NATIVE_DENIED_SITE_PATTERNS: usize =
    zephium_core::extensions::MAX_EXTENSION_SITE_DENIALS_PER_PROFILE;
const MAX_MACOS_NATIVE_HOST_PATTERNS: usize =
    MAX_MACOS_NATIVE_GRANTED_HOST_PATTERNS + MAX_MACOS_NATIVE_DENIED_SITE_PATTERNS;
const MAX_MACOS_NATIVE_MATCH_PATTERN_BYTES: usize = MAX_MATCH_PATTERN_BYTES + 4;
const MAX_MACOS_NATIVE_PATTERN_ARENA_BYTES: usize =
    MAX_MACOS_NATIVE_HOST_PATTERNS * MAX_MACOS_NATIVE_MATCH_PATTERN_BYTES;
const RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES: usize = 2 * size_of::<usize>();

/// Conservative logical transient-heap ceiling for the pure compiler.
///
/// Host patterns remain typed borrowed keys until the final arena write, so
/// there is no attacker-controlled allocation per input string. This logical
/// charge covers the API vector and its possibly compacted boxed replacement,
/// typed-key vector, arena, span vector, and five modeled allocator headers;
/// it is deliberately not an assertion about allocator RSS behavior.
pub(super) const MACOS_NATIVE_GRANT_COMPILER_CONSERVATIVE_TRANSIENT_HEAP_CEILING_BYTES: usize =
    2 * MAX_MACOS_NATIVE_API_PERMISSIONS * size_of::<MacosNativeApiPermission>()
        + MAX_MACOS_NATIVE_HOST_PATTERNS * size_of::<WebPatternKey<'static>>()
        + MAX_MACOS_NATIVE_PATTERN_ARENA_BYTES
        + MAX_MACOS_NATIVE_HOST_PATTERNS * size_of::<PatternSpan>()
        + 7 * RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES;

/// Hard logical retained-memory ceiling for one compiled macOS grant plan.
pub(super) const MAX_MACOS_NATIVE_GRANT_PLAN_RETAINED_BYTES: usize =
    size_of::<MacosNativeGrantPlan>()
        + MAX_MACOS_NATIVE_API_PERMISSIONS * size_of::<MacosNativeApiPermission>()
        + MAX_MACOS_NATIVE_HOST_PATTERNS * size_of::<PatternSpan>()
        + MAX_MACOS_NATIVE_PATTERN_ARENA_BYTES
        + 4 * RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES;

const _: () = assert!(MAX_MACOS_NATIVE_PATTERN_ARENA_BYTES <= u32::MAX as usize);
const _: () = assert!(
    MACOS_NATIVE_GRANT_COMPILER_CONSERVATIVE_TRANSIENT_HEAP_CEILING_BYTES
        < 2 * MAX_MACOS_NATIVE_GRANT_PLAN_RETAINED_BYTES
);

/// A side-effect-free, move-only plan for one exact runtime grant generation.
///
/// This is neither operation authority nor native readback evidence. It must
/// remain joined to the reservation and authority that produced its source
/// snapshot. It deliberately implements neither `Clone` nor Serde traits.
#[must_use = "a native grant plan must remain joined to its runtime reservation"]
pub(super) struct MacosNativeGrantPlan {
    schema: MacosNativeGrantSchema,
    apply_mode: MacosNativeGrantApplyMode,
    runtime: ExtensionRuntimeInstance,
    grant_revision: ExtensionGrantRevision,
    grant_digest: ExtensionGrantDigest,
    granted_api_permissions: Box<[MacosNativeApiPermission]>,
    host_pattern_arena: Box<str>,
    host_pattern_spans: Box<[PatternSpan]>,
    denied_site_pattern_spans: Box<[PatternSpan]>,
    retained_bytes: usize,
}

#[cfg_attr(not(test), allow(dead_code))]
impl MacosNativeGrantPlan {
    pub(super) const fn schema(&self) -> MacosNativeGrantSchema {
        self.schema
    }

    pub(super) const fn apply_mode(&self) -> MacosNativeGrantApplyMode {
        self.apply_mode
    }

    pub(super) const fn runtime(&self) -> ExtensionRuntimeInstance {
        self.runtime
    }

    pub(super) const fn grant_revision(&self) -> ExtensionGrantRevision {
        self.grant_revision
    }

    pub(super) const fn grant_digest(&self) -> ExtensionGrantDigest {
        self.grant_digest
    }

    pub(super) fn granted_api_permissions(&self) -> &[MacosNativeApiPermission] {
        &self.granted_api_permissions
    }

    pub(super) fn granted_host_patterns(&self) -> impl ExactSizeIterator<Item = &str> + '_ {
        self.host_pattern_spans
            .iter()
            .map(|span| span.resolve(&self.host_pattern_arena))
    }

    pub(super) fn denied_site_patterns(&self) -> impl ExactSizeIterator<Item = &str> + '_ {
        self.denied_site_pattern_spans
            .iter()
            .map(|span| span.resolve(&self.host_pattern_arena))
    }

    /// Exact logical bytes retained by this compact representation.
    pub(super) const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

impl fmt::Debug for MacosNativeGrantPlan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MacosNativeGrantPlan")
            .field("schema", &self.schema)
            .field("apply_mode", &self.apply_mode)
            .field("runtime", &"<redacted>")
            .field("grant_generation", &"<redacted>")
            .field(
                "granted_api_permission_count",
                &self.granted_api_permissions.len(),
            )
            .field("host_pattern_count", &self.host_pattern_spans.len())
            .field(
                "denied_site_pattern_count",
                &self.denied_site_pattern_spans.len(),
            )
            .field("retained_bytes", &self.retained_bytes)
            .finish()
    }
}

/// Identity-free refusal from the pure macOS representability boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MacosNativeGrantPlanError {
    PrivateRuntimeUnsupported,
    FileAccessUnproven,
    RequiredApiGrantDenied,
    RequiredHostGrantDenied,
    ProhibitedApiPermission,
    UnsupportedApiPermission,
    Ipv6HostUnsupported,
    InvalidWebHost,
    ExactPortUnsupported,
    PathSemanticsUnsupported,
    ApiEntryLimitExceeded,
    HostEntryLimitExceeded,
    DeclaredCountMismatch,
    PatternTooLong,
    PatternArenaOverflow,
    RetainedBytesOverflow,
    RetainedBytesExceeded,
}

impl fmt::Display for MacosNativeGrantPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::PrivateRuntimeUnsupported => "private macOS extension runtime is unsupported",
            Self::FileAccessUnproven => "macOS extension file access has not passed its live gate",
            Self::RequiredApiGrantDenied => "a required extension API grant is denied",
            Self::RequiredHostGrantDenied => "a required extension host grant is denied",
            Self::ProhibitedApiPermission => "the extension API permission is prohibited",
            Self::UnsupportedApiPermission => {
                "the extension API permission is not representable on macOS"
            }
            Self::Ipv6HostUnsupported => "IPv6 extension host patterns are unsupported on macOS",
            Self::InvalidWebHost => "the extension web host shape is invalid",
            Self::ExactPortUnsupported => "exact extension host ports are unsupported on macOS",
            Self::PathSemanticsUnsupported => {
                "non-root-wildcard extension paths are unsupported on macOS"
            }
            Self::ApiEntryLimitExceeded => "the macOS API grant entry limit was exceeded",
            Self::HostEntryLimitExceeded => "the macOS host grant entry limit was exceeded",
            Self::DeclaredCountMismatch => "the native grant projection count is inconsistent",
            Self::PatternTooLong => "a translated macOS host pattern is too long",
            Self::PatternArenaOverflow => "the macOS host-pattern arena limit was exceeded",
            Self::RetainedBytesOverflow => "macOS native grant accounting overflowed",
            Self::RetainedBytesExceeded => {
                "the macOS native grant retained-byte limit was exceeded"
            }
        })
    }
}

impl Error for MacosNativeGrantPlanError {}

/// Compiles one structural grant snapshot without native work.
///
/// This remains private and disconnected until a reservation-level native
/// backend proof can be joined without admitting a compatibility reservation;
/// possession of the snapshot alone does not provide that proof.
#[allow(dead_code)] // Consumed by the feature-gated native adapter slice.
pub(super) fn compile_native_grant_plan(
    snapshot: &ExtensionNativeGrantSnapshot,
    backend: zephium_core::extensions::ExtensionRuntimeBackendTarget,
    publisher_native_messaging: bool,
) -> Result<MacosNativeGrantPlan, MacosNativeGrantPlanError> {
    let runtime = snapshot.runtime();
    if backend != zephium_core::extensions::ExtensionRuntimeBackendTarget::MacosNative {
        return Err(MacosNativeGrantPlanError::UnsupportedApiPermission);
    }
    let schema = if publisher_native_messaging {
        MacosNativeGrantSchema::WkWebExtensionPublisherNativeMessagingV1
    } else if snapshot.compatibility_target().as_str()
        == zephium_core::extensions::MACOS_NATIVE_BROKERED_COMPATIBILITY_TARGET
    {
        MacosNativeGrantSchema::WkWebExtensionBrokeredV1
    } else {
        MacosNativeGrantSchema::WkWebExtensionV1
    };
    compile_projection(CompilerInput {
        identity: PlanIdentity {
            schema,
            apply_mode:
                MacosNativeGrantApplyMode::ReplaceCompleteGrantedAndDeniedSetsVerifyReadback,
            runtime: runtime.instance(),
            grant_revision: snapshot.grant_revision(),
            grant_digest: snapshot.grant_digest(),
        },
        browsing_context: snapshot.browsing_context(),
        file_access_granted: snapshot.file_scheme_access_granted(),
        private_access_granted: snapshot.private_context_access_granted(),
        api_count: snapshot.api_grant_count(),
        api_grants: snapshot.api_grants().map(|grant| ApiGrantInput {
            name: grant.name().as_str(),
            requirement: grant.requirement(),
            decision: grant.decision(),
        }),
        host_count: snapshot.host_grant_count(),
        host_grants: snapshot.host_grants().map(|grant| HostGrantInput {
            pattern: grant.pattern(),
            requirement: grant.requirement(),
            decision: grant.decision(),
        }),
        denied_site_count: snapshot.denied_sites().len(),
        denied_sites: snapshot.denied_sites().map(|scope| scope.pattern()),
    })
}

#[derive(Clone, Copy)]
struct PlanIdentity {
    schema: MacosNativeGrantSchema,
    apply_mode: MacosNativeGrantApplyMode,
    runtime: ExtensionRuntimeInstance,
    grant_revision: ExtensionGrantRevision,
    grant_digest: ExtensionGrantDigest,
}

#[derive(Clone, Copy)]
struct ApiGrantInput<'a> {
    name: &'a str,
    requirement: ExtensionNativeGrantRequirement,
    decision: ExtensionNativeGrantDecision,
}

#[derive(Clone, Copy)]
struct HostGrantInput<'a> {
    pattern: &'a MatchPattern,
    requirement: ExtensionNativeGrantRequirement,
    decision: ExtensionNativeGrantDecision,
}

struct CompilerInput<A, H, D> {
    identity: PlanIdentity,
    browsing_context: ExtensionGrantBrowsingContext,
    file_access_granted: bool,
    private_access_granted: bool,
    api_count: usize,
    api_grants: A,
    host_count: usize,
    host_grants: H,
    denied_site_count: usize,
    denied_sites: D,
}

fn compile_projection<'a, A, H, D>(
    input: CompilerInput<A, H, D>,
) -> Result<MacosNativeGrantPlan, MacosNativeGrantPlanError>
where
    A: IntoIterator<Item = ApiGrantInput<'a>>,
    H: IntoIterator<Item = HostGrantInput<'a>>,
    D: IntoIterator<Item = &'a MatchPattern>,
{
    if input.browsing_context == ExtensionGrantBrowsingContext::Private
        || input.private_access_granted
    {
        return Err(MacosNativeGrantPlanError::PrivateRuntimeUnsupported);
    }
    if input.file_access_granted {
        return Err(MacosNativeGrantPlanError::FileAccessUnproven);
    }
    if input.api_count > MAX_MACOS_NATIVE_API_PERMISSIONS {
        return Err(MacosNativeGrantPlanError::ApiEntryLimitExceeded);
    }
    if input.host_count > MAX_EXTENSION_HOST_GRANTS {
        return Err(MacosNativeGrantPlanError::HostEntryLimitExceeded);
    }

    let mut granted_api_permissions = Vec::with_capacity(input.api_count);
    let mut seen_api = 0_usize;
    for grant in input.api_grants {
        seen_api = seen_api
            .checked_add(1)
            .ok_or(MacosNativeGrantPlanError::ApiEntryLimitExceeded)?;
        if seen_api > MAX_MACOS_NATIVE_API_PERMISSIONS {
            return Err(MacosNativeGrantPlanError::ApiEntryLimitExceeded);
        }
        if seen_api > input.api_count {
            return Err(MacosNativeGrantPlanError::DeclaredCountMismatch);
        }
        let disposition = input.identity.schema.permission_disposition(grant.name)?;
        if !grant.decision.is_granted()
            && grant.requirement == ExtensionNativeGrantRequirement::Required
        {
            return Err(MacosNativeGrantPlanError::RequiredApiGrantDenied);
        }
        // A Core denial is absence of effective authority, not evidence that
        // the user explicitly denied this permission in WebKit.
        match disposition {
            MacosNativeApiPermissionDisposition::Native(permission)
                if grant.decision.is_granted() =>
            {
                granted_api_permissions.push(permission);
            }
            MacosNativeApiPermissionDisposition::ProductProhibited
                if grant.decision.is_granted() =>
            {
                return Err(MacosNativeGrantPlanError::ProhibitedApiPermission);
            }
            MacosNativeApiPermissionDisposition::Native(_)
            | MacosNativeApiPermissionDisposition::NotInNativePermissionSet
            | MacosNativeApiPermissionDisposition::ProductProhibited => {}
        }
    }
    if seen_api != input.api_count {
        return Err(MacosNativeGrantPlanError::DeclaredCountMismatch);
    }
    granted_api_permissions.sort_unstable_by_key(|permission| permission.as_str());
    granted_api_permissions.dedup();

    let transient_host_capacity = input
        .host_count
        .checked_mul(2)
        .ok_or(MacosNativeGrantPlanError::HostEntryLimitExceeded)?;
    if transient_host_capacity > MAX_MACOS_NATIVE_GRANTED_HOST_PATTERNS {
        return Err(MacosNativeGrantPlanError::HostEntryLimitExceeded);
    }
    let mut host_grants = Vec::new();
    host_grants
        .try_reserve_exact(input.host_count)
        .map_err(|_| MacosNativeGrantPlanError::HostEntryLimitExceeded)?;
    let mut seen_host = 0_usize;
    for grant in input.host_grants {
        seen_host = seen_host
            .checked_add(1)
            .ok_or(MacosNativeGrantPlanError::HostEntryLimitExceeded)?;
        if seen_host > MAX_EXTENSION_HOST_GRANTS {
            return Err(MacosNativeGrantPlanError::HostEntryLimitExceeded);
        }
        if seen_host > input.host_count {
            return Err(MacosNativeGrantPlanError::DeclaredCountMismatch);
        }
        host_grants.push(grant);
    }
    if seen_host != input.host_count {
        return Err(MacosNativeGrantPlanError::DeclaredCountMismatch);
    }
    let grants_all_web = host_grants.iter().any(|grant| {
        grant.decision.is_granted()
            && matches!(grant.pattern.components(), MatchPatternComponents::AllUrls)
    });
    let mut granted_host_patterns: Vec<WebPatternKey<'a>> =
        Vec::with_capacity(transient_host_capacity);
    for grant in host_grants {
        // Static content-script routes are independently retained as required
        // authorities. When an effective `<all_urls>` grant is present, every
        // narrower HTTP/HTTPS route is already covered by the exact native
        // http/https wildcard pair. Do not widen or reject its path while
        // redundantly translating it into WebKit's origin-only permission set.
        if grants_all_web
            && grant.decision.is_granted()
            && !matches!(grant.pattern.components(), MatchPatternComponents::AllUrls)
        {
            continue;
        }
        translate_host_pattern(
            grant.pattern,
            grant.decision.is_granted(),
            &mut granted_host_patterns,
        )?;
        if !grant.decision.is_granted()
            && grant.requirement == ExtensionNativeGrantRequirement::Required
        {
            return Err(MacosNativeGrantPlanError::RequiredHostGrantDenied);
        }
        if granted_host_patterns.len() > MAX_MACOS_NATIVE_GRANTED_HOST_PATTERNS {
            return Err(MacosNativeGrantPlanError::HostEntryLimitExceeded);
        }
    }
    granted_host_patterns.sort_unstable();
    granted_host_patterns.dedup();

    if input.denied_site_count > MAX_MACOS_NATIVE_DENIED_SITE_PATTERNS {
        return Err(MacosNativeGrantPlanError::HostEntryLimitExceeded);
    }
    let mut denied_site_patterns = Vec::with_capacity(input.denied_site_count);
    let mut seen_denied_sites = 0_usize;
    for pattern in input.denied_sites {
        seen_denied_sites = seen_denied_sites
            .checked_add(1)
            .ok_or(MacosNativeGrantPlanError::HostEntryLimitExceeded)?;
        if seen_denied_sites > input.denied_site_count {
            return Err(MacosNativeGrantPlanError::DeclaredCountMismatch);
        }
        translate_host_pattern(pattern, true, &mut denied_site_patterns)?;
        if denied_site_patterns.len() > MAX_MACOS_NATIVE_DENIED_SITE_PATTERNS {
            return Err(MacosNativeGrantPlanError::HostEntryLimitExceeded);
        }
    }
    if seen_denied_sites != input.denied_site_count {
        return Err(MacosNativeGrantPlanError::DeclaredCountMismatch);
    }
    denied_site_patterns.sort_unstable();
    denied_site_patterns.dedup();
    if denied_site_patterns.len() != input.denied_site_count {
        return Err(MacosNativeGrantPlanError::DeclaredCountMismatch);
    }
    // Native granted/denied dictionaries must be disjoint. A profile policy
    // denial equal to one exact manifest grant dominates that native entry;
    // broader grants remain so WebKit can apply the proven narrow override.
    granted_host_patterns.retain(|granted| denied_site_patterns.binary_search(granted).is_err());

    compact_plan(
        input.identity,
        granted_api_permissions,
        granted_host_patterns,
        denied_site_patterns,
    )
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum MacosNativeApiPermission {
    ActiveTab,
    Alarms,
    ClipboardWrite,
    ContextMenus,
    Cookies,
    DeclarativeNetRequest,
    DeclarativeNetRequestFeedback,
    DeclarativeNetRequestWithHostAccess,
    Menus,
    NativeMessaging,
    Notifications,
    Scripting,
    Storage,
    Tabs,
    #[cfg_attr(
        not(feature = "native-web-extension-probes"),
        allow(
            dead_code,
            reason = "platform probes retain the native token while product schemas prohibit it"
        )
    )]
    UnlimitedStorage,
    WebNavigation,
    WebRequest,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MacosNativeApiPermissionDisposition {
    Native(MacosNativeApiPermission),
    /// WebKit accepts the manifest token but does not publish it in
    /// `requestedPermissions`. Compatibility remains an explicit manifest
    /// policy decision; there is no native permission-dictionary entry to
    /// apply or read back.
    NotInNativePermissionSet,
    /// The native runtime recognizes this permission, but Zephium does not
    /// expose the corresponding privileged product integration.
    ProductProhibited,
}

/// Exact policy vocabulary used to interpret one compiled native plan.
///
/// The explicit one-byte tag prevents a retained or cached plan from being
/// silently reinterpreted when a future WebKit capability matrix changes.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MacosNativeGrantSchema {
    WkWebExtensionV1 = 1,
    WkWebExtensionBrokeredV1 = 2,
    WkWebExtensionPublisherNativeMessagingV1 = 3,
}

/// Mandatory semantics for applying one complete compiled plan.
///
/// This is not a delta. A future adapter must replace both complete granted
/// collections, replace both native denied collections with empty sets, and
/// accept activation only after exact native readback matches all four sets.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MacosNativeGrantApplyMode {
    ReplaceCompleteGrantedAndDeniedSetsVerifyReadback = 1,
}

impl MacosNativeGrantSchema {
    fn permission_disposition(
        self,
        name: &str,
    ) -> Result<MacosNativeApiPermissionDisposition, MacosNativeGrantPlanError> {
        match self {
            Self::WkWebExtensionV1 => Self::wk_web_extension_v1_permission(name),
            Self::WkWebExtensionBrokeredV1 => Self::wk_web_extension_brokered_v1_permission(name),
            Self::WkWebExtensionPublisherNativeMessagingV1 => {
                Self::wk_web_extension_publisher_native_messaging_v1_permission(name)
            }
        }
    }

    fn wk_web_extension_publisher_native_messaging_v1_permission(
        name: &str,
    ) -> Result<MacosNativeApiPermissionDisposition, MacosNativeGrantPlanError> {
        match name {
            "nativeMessaging" => Ok(MacosNativeApiPermissionDisposition::Native(
                MacosNativeApiPermission::NativeMessaging,
            )),
            _ => Self::wk_web_extension_v1_permission(name),
        }
    }

    fn wk_web_extension_brokered_v1_permission(
        name: &str,
    ) -> Result<MacosNativeApiPermissionDisposition, MacosNativeGrantPlanError> {
        match name {
            "bookmarks" | "favicon" | "history" | "search" | "sessions" => {
                Ok(MacosNativeApiPermissionDisposition::NotInNativePermissionSet)
            }
            "nativeMessaging" => Ok(MacosNativeApiPermissionDisposition::Native(
                MacosNativeApiPermission::NativeMessaging,
            )),
            _ => Self::wk_web_extension_v1_permission(name),
        }
    }

    fn wk_web_extension_v1_permission(
        name: &str,
    ) -> Result<MacosNativeApiPermissionDisposition, MacosNativeGrantPlanError> {
        use MacosNativeApiPermissionDisposition::{
            Native, NotInNativePermissionSet, ProductProhibited,
        };
        match name {
            "activeTab" => Ok(Native(MacosNativeApiPermission::ActiveTab)),
            "alarms" => Ok(Native(MacosNativeApiPermission::Alarms)),
            "clipboardWrite" => Ok(Native(MacosNativeApiPermission::ClipboardWrite)),
            "contextMenus" => Ok(Native(MacosNativeApiPermission::ContextMenus)),
            "cookies" => Ok(Native(MacosNativeApiPermission::Cookies)),
            "declarativeNetRequest" => Ok(Native(MacosNativeApiPermission::DeclarativeNetRequest)),
            "declarativeNetRequestFeedback" => Ok(Native(
                MacosNativeApiPermission::DeclarativeNetRequestFeedback,
            )),
            "declarativeNetRequestWithHostAccess" => Ok(Native(
                MacosNativeApiPermission::DeclarativeNetRequestWithHostAccess,
            )),
            "menus" => Ok(Native(MacosNativeApiPermission::Menus)),
            "notifications" => Ok(Native(MacosNativeApiPermission::Notifications)),
            "scripting" => Ok(Native(MacosNativeApiPermission::Scripting)),
            "storage" => Ok(Native(MacosNativeApiPermission::Storage)),
            "tabs" => Ok(Native(MacosNativeApiPermission::Tabs)),
            // WebKit can report per-extension stored bytes, but exposes no
            // product quota setter. Granting this token removes its own local
            // storage quota and would let one extension grow outside every
            // Zephium resource bound. Keep the native enum for feature-only
            // platform probes; product grant schemas reject it until a
            // separately reviewed bounded storage design exists.
            "unlimitedStorage" => Ok(ProductProhibited),
            "webNavigation" => Ok(Native(MacosNativeApiPermission::WebNavigation)),
            "webRequest" => Ok(Native(MacosNativeApiPermission::WebRequest)),
            "clipboardRead"
            | "downloads"
            | "fontSettings"
            | "idle"
            | "management"
            | "offscreen"
            | "privacy"
            | "sidePanel"
            | "webRequestAuthProvider" => Ok(NotInNativePermissionSet),
            "history" | "nativeMessaging" => Ok(ProductProhibited),
            _ => Err(MacosNativeGrantPlanError::UnsupportedApiPermission),
        }
    }
}

/// Validates one dynamic API-permission cohort against the same product and
/// native representability vocabulary used by complete grant compilation.
/// This runs before user consent so a prohibited optional declaration can
/// never commit durably and fail only during the post-commit native rebind.
pub(super) fn validate_runtime_api_permission_request(
    permissions: &[zephium_core::extensions::ApiPermissionName],
) -> Result<(), MacosNativeGrantPlanError> {
    for permission in permissions {
        match MacosNativeGrantSchema::WkWebExtensionV1
            .permission_disposition(permission.as_str())?
        {
            MacosNativeApiPermissionDisposition::Native(_)
            | MacosNativeApiPermissionDisposition::NotInNativePermissionSet => {}
            MacosNativeApiPermissionDisposition::ProductProhibited => {
                return Err(MacosNativeGrantPlanError::ProhibitedApiPermission);
            }
        }
    }
    Ok(())
}

/// Compiles the response set for a dynamic host-permission callback using the
/// exact web-only translation used by complete native grant plans.
///
/// A file-only request is rejected instead of being persisted as an
/// ineffective host row and then accidentally made effective by returning the
/// raw WebKit request set. `<all_urls>` remains representable as its ordinary
/// HTTP/HTTPS subset; local-file authority stays behind the independent file
/// grant, which this request type intentionally cannot mutate.
pub(super) fn compile_runtime_host_permission_response(
    patterns: &[MatchPattern],
) -> Result<Box<[String]>, MacosNativeGrantPlanError> {
    let capacity = patterns
        .len()
        .checked_mul(2)
        .ok_or(MacosNativeGrantPlanError::HostEntryLimitExceeded)?;
    if capacity > MAX_MACOS_NATIVE_GRANTED_HOST_PATTERNS {
        return Err(MacosNativeGrantPlanError::HostEntryLimitExceeded);
    }
    let mut translated = Vec::with_capacity(capacity);
    for pattern in patterns {
        if matches!(
            pattern.components(),
            MatchPatternComponents::Standard {
                scheme: MatchPatternScheme::File,
                ..
            }
        ) {
            return Err(MacosNativeGrantPlanError::FileAccessUnproven);
        }
        translate_host_pattern(pattern, true, &mut translated)?;
    }
    translated.sort_unstable();
    translated.dedup();
    if translated.is_empty() && !patterns.is_empty() {
        return Err(MacosNativeGrantPlanError::FileAccessUnproven);
    }

    translated
        .into_iter()
        .map(|pattern| {
            let mut rendered = String::with_capacity(pattern.rendered_len()?);
            pattern.append_to(&mut rendered);
            Ok(rendered)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Vec::into_boxed_slice)
}

impl MacosNativeApiPermission {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::ActiveTab => "activeTab",
            Self::Alarms => "alarms",
            Self::ClipboardWrite => "clipboardWrite",
            Self::ContextMenus => "contextMenus",
            Self::Cookies => "cookies",
            Self::DeclarativeNetRequest => "declarativeNetRequest",
            Self::DeclarativeNetRequestFeedback => "declarativeNetRequestFeedback",
            Self::DeclarativeNetRequestWithHostAccess => "declarativeNetRequestWithHostAccess",
            Self::Menus => "menus",
            Self::NativeMessaging => "nativeMessaging",
            Self::Notifications => "notifications",
            Self::Scripting => "scripting",
            Self::Storage => "storage",
            Self::Tabs => "tabs",
            Self::UnlimitedStorage => "unlimitedStorage",
            Self::WebNavigation => "webNavigation",
            Self::WebRequest => "webRequest",
        }
    }
}

fn translate_host_pattern<'a>(
    pattern: &'a MatchPattern,
    is_granted: bool,
    output: &mut Vec<WebPatternKey<'a>>,
) -> Result<(), MacosNativeGrantPlanError> {
    match pattern.components() {
        MatchPatternComponents::AllUrls => {
            if is_granted {
                output.push(WebPatternKey {
                    scheme: WebScheme::Http,
                    host: WebHost::Any,
                });
                output.push(WebPatternKey {
                    scheme: WebScheme::Https,
                    host: WebHost::Any,
                });
            }
            Ok(())
        }
        MatchPatternComponents::Standard {
            scheme: MatchPatternScheme::File,
            ..
        } => Ok(()),
        MatchPatternComponents::Standard {
            scheme,
            host,
            port,
            path,
        } => {
            if port != MatchPatternPort::Any {
                return Err(MacosNativeGrantPlanError::ExactPortUnsupported);
            }
            if path.as_str() != "/*" {
                return Err(MacosNativeGrantPlanError::PathSemanticsUnsupported);
            }
            let host = host.ok_or(MacosNativeGrantPlanError::InvalidWebHost)?;
            let host = WebHost::try_from(host)?;
            if !is_granted {
                return Ok(());
            }
            match scheme {
                MatchPatternScheme::Http => output.push(WebPatternKey {
                    scheme: WebScheme::Http,
                    host,
                }),
                MatchPatternScheme::Https => output.push(WebPatternKey {
                    scheme: WebScheme::Https,
                    host,
                }),
                MatchPatternScheme::HttpAndHttps => {
                    output.push(WebPatternKey {
                        scheme: WebScheme::Http,
                        host,
                    });
                    output.push(WebPatternKey {
                        scheme: WebScheme::Https,
                        host,
                    });
                }
                MatchPatternScheme::File => unreachable!("file scheme handled above"),
            }
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum WebScheme {
    Http,
    Https,
}

impl WebScheme {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Https => "https",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum WebHost<'a> {
    Any,
    ExactDomain(&'a str),
    DomainAndSubdomains(&'a str),
    ExactIpv4(Ipv4Addr),
}

impl<'a> TryFrom<MatchPatternHost<'a>> for WebHost<'a> {
    type Error = MacosNativeGrantPlanError;

    fn try_from(host: MatchPatternHost<'a>) -> Result<Self, Self::Error> {
        match host {
            MatchPatternHost::Any => Ok(Self::Any),
            MatchPatternHost::ExactDomain(domain) => Ok(Self::ExactDomain(domain)),
            MatchPatternHost::DomainAndSubdomains(domain) => Ok(Self::DomainAndSubdomains(domain)),
            MatchPatternHost::ExactIpv4(address) => Ok(Self::ExactIpv4(address)),
            MatchPatternHost::ExactIpv6(_) => Err(MacosNativeGrantPlanError::Ipv6HostUnsupported),
        }
    }
}

impl WebHost<'_> {
    fn rendered_len(self) -> Result<usize, MacosNativeGrantPlanError> {
        match self {
            Self::Any => Ok(1),
            Self::ExactDomain(domain) => Ok(domain.len()),
            Self::DomainAndSubdomains(domain) => domain
                .len()
                .checked_add(2)
                .ok_or(MacosNativeGrantPlanError::PatternTooLong),
            Self::ExactIpv4(address) => Ok(ipv4_rendered_len(address)),
        }
    }

    fn append_to(self, output: &mut String) {
        match self {
            Self::Any => output.push('*'),
            Self::ExactDomain(domain) => output.push_str(domain),
            Self::DomainAndSubdomains(domain) => {
                output.push_str("*.");
                output.push_str(domain);
            }
            Self::ExactIpv4(address) => append_ipv4(output, address),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct WebPatternKey<'a> {
    scheme: WebScheme,
    host: WebHost<'a>,
}

impl<'a> WebPatternKey<'a> {
    fn rendered_len(self) -> Result<usize, MacosNativeGrantPlanError> {
        let host_bytes = self.host.rendered_len()?;
        let bytes = self
            .scheme
            .as_str()
            .len()
            .checked_add(3)
            .and_then(|bytes| host_bytes.checked_add(bytes))
            .and_then(|bytes| bytes.checked_add(2))
            .ok_or(MacosNativeGrantPlanError::PatternTooLong)?;
        if bytes > MAX_MACOS_NATIVE_MATCH_PATTERN_BYTES {
            return Err(MacosNativeGrantPlanError::PatternTooLong);
        }
        Ok(bytes)
    }

    fn append_to(self, output: &mut String) {
        output.push_str(self.scheme.as_str());
        output.push_str("://");
        self.host.append_to(output);
        output.push_str("/*");
    }

    fn rendered_bytes(self) -> RenderedPatternBytes<'a> {
        let (host_prefix, host_body) = match self.host {
            WebHost::Any => (b"*".as_slice(), RenderedSegment::Borrowed(b"")),
            WebHost::ExactDomain(domain) => {
                (b"".as_slice(), RenderedSegment::Borrowed(domain.as_bytes()))
            }
            WebHost::DomainAndSubdomains(domain) => (
                b"*.".as_slice(),
                RenderedSegment::Borrowed(domain.as_bytes()),
            ),
            WebHost::ExactIpv4(address) => {
                let (bytes, length) = render_ipv4_bytes(address);
                (b"".as_slice(), RenderedSegment::Inline { bytes, length })
            }
        };
        RenderedPatternBytes {
            segments: [
                RenderedSegment::Borrowed(self.scheme.as_str().as_bytes()),
                RenderedSegment::Borrowed(b"://"),
                RenderedSegment::Borrowed(host_prefix),
                host_body,
                RenderedSegment::Borrowed(b"/*"),
            ],
            segment_index: 0,
            byte_index: 0,
        }
    }
}

impl Ord for WebPatternKey<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.rendered_bytes().cmp(other.rendered_bytes())
    }
}

impl PartialEq for WebPatternKey<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.rendered_bytes().eq(other.rendered_bytes())
    }
}

impl Eq for WebPatternKey<'_> {}

impl PartialOrd for WebPatternKey<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

enum RenderedSegment<'a> {
    Borrowed(&'a [u8]),
    Inline { bytes: [u8; 15], length: usize },
}

impl RenderedSegment<'_> {
    fn get(&self, index: usize) -> Option<u8> {
        match self {
            Self::Borrowed(bytes) => bytes.get(index).copied(),
            Self::Inline { bytes, length } => (index < *length).then(|| bytes[index]),
        }
    }
}

struct RenderedPatternBytes<'a> {
    segments: [RenderedSegment<'a>; 5],
    segment_index: usize,
    byte_index: usize,
}

impl Iterator for RenderedPatternBytes<'_> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(segment) = self.segments.get(self.segment_index) {
            if let Some(byte) = segment.get(self.byte_index) {
                self.byte_index += 1;
                return Some(byte);
            }
            self.segment_index += 1;
            self.byte_index = 0;
        }
        None
    }
}

fn ipv4_rendered_len(address: Ipv4Addr) -> usize {
    address
        .octets()
        .into_iter()
        .map(decimal_octet_len)
        .sum::<usize>()
        + 3
}

const fn decimal_octet_len(value: u8) -> usize {
    if value >= 100 {
        3
    } else if value >= 10 {
        2
    } else {
        1
    }
}

fn append_ipv4(output: &mut String, address: Ipv4Addr) {
    for (index, octet) in address.octets().into_iter().enumerate() {
        if index != 0 {
            output.push('.');
        }
        append_decimal_octet(output, octet);
    }
}

fn render_ipv4_bytes(address: Ipv4Addr) -> ([u8; 15], usize) {
    let mut bytes = [0_u8; 15];
    let mut length = 0_usize;
    for (index, octet) in address.octets().into_iter().enumerate() {
        if index != 0 {
            bytes[length] = b'.';
            length += 1;
        }
        if octet >= 100 {
            bytes[length] = b'0' + octet / 100;
            length += 1;
            bytes[length] = b'0' + (octet / 10) % 10;
            length += 1;
        } else if octet >= 10 {
            bytes[length] = b'0' + octet / 10;
            length += 1;
        }
        bytes[length] = b'0' + octet % 10;
        length += 1;
    }
    (bytes, length)
}

fn append_decimal_octet(output: &mut String, value: u8) {
    if value >= 100 {
        output.push(char::from(b'0' + value / 100));
        output.push(char::from(b'0' + (value / 10) % 10));
    } else if value >= 10 {
        output.push(char::from(b'0' + value / 10));
    }
    output.push(char::from(b'0' + value % 10));
}

fn compact_plan(
    identity: PlanIdentity,
    granted_api_permissions: Vec<MacosNativeApiPermission>,
    granted_host_patterns: Vec<WebPatternKey<'_>>,
    denied_site_patterns: Vec<WebPatternKey<'_>>,
) -> Result<MacosNativeGrantPlan, MacosNativeGrantPlanError> {
    let arena_bytes = granted_host_patterns
        .iter()
        .chain(denied_site_patterns.iter())
        .try_fold(0_usize, |bytes, pattern| {
            bytes
                .checked_add(pattern.rendered_len()?)
                .ok_or(MacosNativeGrantPlanError::PatternArenaOverflow)
        })?;
    if arena_bytes > MAX_MACOS_NATIVE_PATTERN_ARENA_BYTES {
        return Err(MacosNativeGrantPlanError::PatternArenaOverflow);
    }

    let mut arena = String::with_capacity(arena_bytes);
    let mut spans = Vec::with_capacity(granted_host_patterns.len());
    for pattern in granted_host_patterns {
        let start = u32::try_from(arena.len())
            .map_err(|_| MacosNativeGrantPlanError::PatternArenaOverflow)?;
        let pattern_length = pattern.rendered_len()?;
        let length =
            u32::try_from(pattern_length).map_err(|_| MacosNativeGrantPlanError::PatternTooLong)?;
        pattern.append_to(&mut arena);
        debug_assert_eq!(arena.len(), start as usize + pattern_length);
        spans.push(PatternSpan { start, length });
    }
    let mut denied_spans = Vec::with_capacity(denied_site_patterns.len());
    for pattern in denied_site_patterns {
        let start = u32::try_from(arena.len())
            .map_err(|_| MacosNativeGrantPlanError::PatternArenaOverflow)?;
        let pattern_length = pattern.rendered_len()?;
        let length =
            u32::try_from(pattern_length).map_err(|_| MacosNativeGrantPlanError::PatternTooLong)?;
        pattern.append_to(&mut arena);
        debug_assert_eq!(arena.len(), start as usize + pattern_length);
        denied_spans.push(PatternSpan { start, length });
    }
    debug_assert_eq!(arena.len(), arena_bytes);

    let granted_api_permissions = granted_api_permissions.into_boxed_slice();
    let host_pattern_arena = arena.into_boxed_str();
    let host_pattern_spans = spans.into_boxed_slice();
    let denied_site_pattern_spans = denied_spans.into_boxed_slice();
    let retained_bytes = calculate_retained_bytes(
        granted_api_permissions.len(),
        host_pattern_arena.len(),
        host_pattern_spans.len(),
        denied_site_pattern_spans.len(),
    )?;
    if retained_bytes > MAX_MACOS_NATIVE_GRANT_PLAN_RETAINED_BYTES {
        return Err(MacosNativeGrantPlanError::RetainedBytesExceeded);
    }

    Ok(MacosNativeGrantPlan {
        schema: identity.schema,
        apply_mode: identity.apply_mode,
        runtime: identity.runtime,
        grant_revision: identity.grant_revision,
        grant_digest: identity.grant_digest,
        granted_api_permissions,
        host_pattern_arena,
        host_pattern_spans,
        denied_site_pattern_spans,
        retained_bytes,
    })
}

fn calculate_retained_bytes(
    api_count: usize,
    arena_bytes: usize,
    granted_span_count: usize,
    denied_span_count: usize,
) -> Result<usize, MacosNativeGrantPlanError> {
    let api_bytes = api_count
        .checked_mul(size_of::<MacosNativeApiPermission>())
        .ok_or(MacosNativeGrantPlanError::RetainedBytesOverflow)?;
    let granted_span_bytes = granted_span_count
        .checked_mul(size_of::<PatternSpan>())
        .ok_or(MacosNativeGrantPlanError::RetainedBytesOverflow)?;
    let denied_span_bytes = denied_span_count
        .checked_mul(size_of::<PatternSpan>())
        .ok_or(MacosNativeGrantPlanError::RetainedBytesOverflow)?;
    let api_allocation = retained_boxed_allocation_bytes(api_bytes)?;
    let arena_allocation = retained_boxed_allocation_bytes(arena_bytes)?;
    let granted_span_allocation = retained_boxed_allocation_bytes(granted_span_bytes)?;
    let denied_span_allocation = retained_boxed_allocation_bytes(denied_span_bytes)?;
    size_of::<MacosNativeGrantPlan>()
        .checked_add(api_allocation)
        .and_then(|bytes| bytes.checked_add(arena_allocation))
        .and_then(|bytes| bytes.checked_add(granted_span_allocation))
        .and_then(|bytes| bytes.checked_add(denied_span_allocation))
        .ok_or(MacosNativeGrantPlanError::RetainedBytesOverflow)
}

fn retained_boxed_allocation_bytes(
    payload_bytes: usize,
) -> Result<usize, MacosNativeGrantPlanError> {
    if payload_bytes == 0 {
        Ok(0)
    } else {
        RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES
            .checked_add(payload_bytes)
            .ok_or(MacosNativeGrantPlanError::RetainedBytesOverflow)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PatternSpan {
    start: u32,
    length: u32,
}

impl PatternSpan {
    fn resolve(self, arena: &str) -> &str {
        let start = self.start as usize;
        let end = start + self.length as usize;
        &arena[Range { start, end }]
    }
}

#[cfg(test)]
mod tests;
