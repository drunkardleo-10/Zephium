//! Native-runtime grant projections and retained structural snapshots.
//!
//! This module translates one already-authorized runtime snapshot into the
//! complete declaration-by-declaration grant state needed by a native adapter.
//! The borrowed projection performs no allocation. Its owned transport form
//! shallow-retains immutable Arc owners without cloning manifest strings or
//! compiled match patterns. Both forms are structural only: neither can
//! authorize an extension operation when separated from the move-only runtime
//! operation authority that created them.

use std::fmt;
use std::iter::FusedIterator;
use std::mem::size_of;
use std::sync::Arc;

use crate::injection::MatchPattern;

use super::{
    ApiPermissionName, ExtensionApiGrantDecision, ExtensionCompatibilityTargetId,
    ExtensionGrantAuthority, ExtensionGrantBrowsingContext, ExtensionGrantDigest,
    ExtensionGrantRevision, ExtensionManifestDeclarations, ExtensionManifestDescriptor,
    ExtensionProfilePolicy, ExtensionProfilePolicyDigest, ExtensionProfilePolicyRevision,
    ExtensionRuntimeFingerprint, ExtensionSiteAccessScope, ExtensionUrlScopeDecision,
    MAX_EXTENSION_CONTENT_SCRIPT_DECLARATIONS, MAX_EXTENSION_HOST_GRANTS,
};

const REQUIRED_HOST_SOURCE_INDEX: usize = 0;
const CONTENT_SCRIPT_HOST_SOURCE_START: usize = 1;
const OPTIONAL_HOST_SOURCE_INDEX: usize = MAX_EXTENSION_CONTENT_SCRIPT_DECLARATIONS + 1;
const HOST_SOURCE_COUNT: usize = MAX_EXTENSION_CONTENT_SCRIPT_DECLARATIONS + 2;
const RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES: usize = 2 * size_of::<usize>();
const RETAINED_ARC_COUNTER_BYTES: usize = 2 * size_of::<usize>();

/// Whether one projected declaration is required or optional in the admitted
/// manifest.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ExtensionNativeGrantRequirement {
    Required,
    Optional,
}

/// Exact grant-row decision for one declared authority.
///
/// `Denied` means only that this exact manifest declaration is absent from the
/// effective grant row. It is not a URL deny-list entry: a separately granted
/// broader host pattern may still authorize the same URL. Native adapters must
/// consume the complete projection atomically, apply the independent
/// file/private gates, reject platform shapes they cannot represent exactly,
/// and verify native readback. In particular, they must not blindly translate
/// denied host declarations into a platform-native denied-pattern collection.
/// Denial reasons remain an internal grant-policy detail.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExtensionNativeGrantDecision {
    Granted,
    Denied,
}

impl ExtensionNativeGrantDecision {
    const fn from_api(decision: ExtensionApiGrantDecision) -> Self {
        match decision {
            ExtensionApiGrantDecision::Granted => Self::Granted,
            ExtensionApiGrantDecision::Denied(_) => Self::Denied,
        }
    }

    const fn from_host(decision: ExtensionUrlScopeDecision) -> Self {
        match decision {
            ExtensionUrlScopeDecision::InScope => Self::Granted,
            ExtensionUrlScopeDecision::OutOfScope(_) => Self::Denied,
        }
    }

    pub const fn is_granted(self) -> bool {
        matches!(self, Self::Granted)
    }
}

/// One exact declared API permission and its effective native decision.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ExtensionNativeApiGrant<'a> {
    name: &'a ApiPermissionName,
    requirement: ExtensionNativeGrantRequirement,
    decision: ExtensionNativeGrantDecision,
}

impl fmt::Debug for ExtensionNativeApiGrant<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionNativeApiGrant")
            .field("name", &"<redacted>")
            .field("requirement", &self.requirement)
            .field("decision", &self.decision)
            .finish()
    }
}

impl<'a> ExtensionNativeApiGrant<'a> {
    pub const fn name(self) -> &'a ApiPermissionName {
        self.name
    }

    pub const fn requirement(self) -> ExtensionNativeGrantRequirement {
        self.requirement
    }

    pub const fn decision(self) -> ExtensionNativeGrantDecision {
        self.decision
    }
}

/// One exact canonical host match pattern and its effective native decision.
///
/// Required host authority is the deduplicated union of explicit required
/// host permissions and content-script include patterns. File-scheme access
/// is intentionally projected as an independent flag on
/// [`ExtensionNativeGrantProjection`], so a granted `<all_urls>` declaration
/// does not silently grant `file:` access.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ExtensionNativeHostGrant<'a> {
    pattern: &'a MatchPattern,
    requirement: ExtensionNativeGrantRequirement,
    decision: ExtensionNativeGrantDecision,
}

impl fmt::Debug for ExtensionNativeHostGrant<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionNativeHostGrant")
            .field("pattern", &"<redacted>")
            .field("requirement", &self.requirement)
            .field("decision", &self.decision)
            .finish()
    }
}

impl<'a> ExtensionNativeHostGrant<'a> {
    pub const fn pattern(self) -> &'a MatchPattern {
        self.pattern
    }

    pub const fn requirement(self) -> ExtensionNativeGrantRequirement {
        self.requirement
    }

    pub const fn decision(self) -> ExtensionNativeGrantDecision {
        self.decision
    }
}

/// Borrowed, allocation-free projection of one exact runtime's native grants.
///
/// The only public construction path is
/// [`super::ExtensionRuntimeOperationAuthority::native_grant_projection`],
/// which verifies the caller's complete runtime fingerprint. This value is
/// structural and non-authorizing: adapters must retain the originating
/// operation authority and join it with authenticated package/native
/// ownership before doing privileged work.
///
/// The projection deliberately cannot be cloned:
///
/// ```compile_fail
/// use zephium_core::extensions::ExtensionNativeGrantProjection;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ExtensionNativeGrantProjection<'static>>();
/// ```
///
/// It cannot cross a serialization boundary:
///
/// ```compile_fail
/// use zephium_core::extensions::ExtensionNativeGrantProjection;
/// fn requires_serialize<T: serde::Serialize>() {}
/// requires_serialize::<ExtensionNativeGrantProjection<'static>>();
/// ```
#[must_use = "native grant projection must be consumed with its operation authority"]
pub struct ExtensionNativeGrantProjection<'a> {
    runtime: &'a ExtensionRuntimeFingerprint,
    manifest: &'a Arc<ExtensionManifestDescriptor>,
    grants: &'a Arc<ExtensionGrantAuthority>,
    profile_policy: &'a Arc<ExtensionProfilePolicy>,
}

impl<'a> ExtensionNativeGrantProjection<'a> {
    pub(super) fn new(
        runtime: &'a ExtensionRuntimeFingerprint,
        manifest: &'a Arc<ExtensionManifestDescriptor>,
        grants: &'a Arc<ExtensionGrantAuthority>,
        profile_policy: &'a Arc<ExtensionProfilePolicy>,
    ) -> Self {
        debug_assert_eq!(runtime.package(), manifest.package());
        debug_assert_eq!(runtime.package(), grants.package());
        debug_assert_eq!(runtime.grant_revision(), grants.revision());
        debug_assert_eq!(runtime.grant_digest(), grants.digest());
        debug_assert_eq!(runtime.profile_policy_revision(), profile_policy.revision());
        debug_assert_eq!(runtime.profile_policy_digest(), profile_policy.digest());
        Self {
            runtime,
            manifest,
            grants,
            profile_policy,
        }
    }

    /// Complete exact runtime generation whose grants are projected.
    ///
    /// This fingerprint remains non-authorizing identity when separated from
    /// the originating operation authority.
    pub const fn runtime(&self) -> &'a ExtensionRuntimeFingerprint {
        self.runtime
    }

    /// Exact grant-row revision bound into the runtime fingerprint.
    pub const fn grant_revision(&self) -> ExtensionGrantRevision {
        self.runtime.grant_revision()
    }

    /// Exact complete grant digest bound into the runtime fingerprint.
    pub const fn grant_digest(&self) -> ExtensionGrantDigest {
        self.runtime.grant_digest()
    }

    /// Browsing partition for which every projected decision is effective.
    pub const fn browsing_context(&self) -> ExtensionGrantBrowsingContext {
        self.runtime.browsing_context()
    }

    pub fn profile_policy_revision(&self) -> ExtensionProfilePolicyRevision {
        self.profile_policy.revision()
    }

    pub fn profile_policy_digest(&self) -> ExtensionProfilePolicyDigest {
        self.profile_policy.digest()
    }

    pub fn denied_sites(&self) -> impl ExactSizeIterator<Item = &ExtensionSiteAccessScope> {
        self.profile_policy.denied_sites().iter()
    }

    /// Canonically ordered complete API declaration stream.
    ///
    /// Required and optional declarations are merged by exact permission name.
    /// Optional denials remain explicit. The iterator borrows the admitted
    /// manifest and grant authority and allocates nothing.
    pub fn api_grants(&self) -> ExtensionNativeApiGrantIter<'a> {
        let declarations = self.manifest.declarations();
        ExtensionNativeApiGrantIter {
            required: declarations.required_api().names(),
            optional: declarations.optional_api().names(),
            required_index: 0,
            optional_index: 0,
            manifest: self.manifest,
            grants: self.grants,
            context: self.runtime.browsing_context(),
        }
    }

    /// Exact number of declared API permissions in [`Self::api_grants`].
    pub fn api_grant_count(&self) -> usize {
        let declarations = self.manifest.declarations();
        declarations
            .required_api()
            .len()
            .saturating_add(declarations.optional_api().len())
    }

    /// Canonically ordered complete host declaration stream.
    ///
    /// A fixed cursor is retained for each bounded manifest source: explicit
    /// required hosts, every content-script include set, and explicit optional
    /// hosts. This avoids cloning match patterns or allocating a temporary
    /// union while still yielding a globally sorted, deduplicated stream.
    pub fn host_grants(&self) -> ExtensionNativeHostGrantIter<'a> {
        ExtensionNativeHostGrantIter {
            declarations: self.manifest.declarations(),
            manifest: self.manifest,
            grants: self.grants,
            context: self.runtime.browsing_context(),
            cursors: [0; HOST_SOURCE_COUNT],
            last: None,
            yielded: 0,
        }
    }

    /// Exact number of canonical host authorities in [`Self::host_grants`].
    ///
    /// Counting performs the same bounded allocation-free merge as iteration.
    pub fn host_grant_count(&self) -> usize {
        self.host_grants().count()
    }

    /// Whether `file:` is effectively enabled for this exact runtime context.
    ///
    /// This remains independent from host patterns: `<all_urls>` can be
    /// granted while this flag is false. In a private context, both the file
    /// and private grants would be required.
    pub fn file_scheme_access_granted(&self) -> bool {
        self.grants
            .effective_file_access(self.runtime.browsing_context())
    }

    /// Whether this exact runtime is effectively admitted to private context.
    ///
    /// This deliberately does not expose the durable raw private toggle.
    /// Private runtime eligibility is currently unsupported, so a valid
    /// operation authority projects `false` even if a future-intent bit was
    /// persisted on the grant row.
    pub fn private_context_access_granted(&self) -> bool {
        self.grants
            .effective_private_access(self.runtime.browsing_context())
    }

    /// Exact exclusive inline size of this borrowed projection.
    ///
    /// The manifest, grants, and runtime are borrowed and intentionally are
    /// not double-counted. Iterators are transient stack values and allocate
    /// no heap storage.
    pub const fn retained_bytes(&self) -> usize {
        size_of::<Self>()
    }

    /// Retains this exact structural grant snapshot beyond the authority
    /// borrow without cloning manifest strings or compiled matchers.
    ///
    /// The returned value shallow-clones the immutable admitted manifest and
    /// grant owners. It remains non-authorizing and must stay joined to the
    /// originating operation authority, authenticated package access, and
    /// native ownership reservation before a platform capability is changed.
    pub fn into_owned_snapshot(self) -> ExtensionNativeGrantSnapshot {
        ExtensionNativeGrantSnapshot {
            runtime: self.runtime.clone(),
            manifest: Arc::clone(self.manifest),
            grants: Arc::clone(self.grants),
            profile_policy: Arc::clone(self.profile_policy),
        }
    }
}

impl fmt::Debug for ExtensionNativeGrantProjection<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionNativeGrantProjection")
            .field("runtime", &"<redacted>")
            .field("authority", &"<redacted>")
            .finish()
    }
}

/// Owned, bounded, non-authorizing native grant transport snapshot.
///
/// This value keeps shallow references to the exact immutable Store-admitted
/// manifest and grant authority plus their complete runtime fingerprint. It
/// exists so a trusted host can retain grant structure across a side-effect-
/// free bind and apply it later on the native UI thread without deep-copying
/// patterns. It cannot mint operation witnesses or prove package/native
/// ownership.
///
/// The snapshot deliberately does not implement `Clone` or serialization:
///
/// ```compile_fail
/// use zephium_core::extensions::ExtensionNativeGrantSnapshot;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ExtensionNativeGrantSnapshot>();
/// ```
///
/// ```compile_fail
/// use zephium_core::extensions::ExtensionNativeGrantSnapshot;
/// fn requires_serialize<T: serde::Serialize>() {}
/// requires_serialize::<ExtensionNativeGrantSnapshot>();
/// ```
///
/// ```compile_fail
/// use serde::de::DeserializeOwned;
/// use zephium_core::extensions::ExtensionNativeGrantSnapshot;
/// fn requires_deserialize<T: DeserializeOwned>() {}
/// requires_deserialize::<ExtensionNativeGrantSnapshot>();
/// ```
#[must_use = "native grant snapshot must remain joined to runtime authority"]
pub struct ExtensionNativeGrantSnapshot {
    runtime: ExtensionRuntimeFingerprint,
    manifest: Arc<ExtensionManifestDescriptor>,
    grants: Arc<ExtensionGrantAuthority>,
    profile_policy: Arc<ExtensionProfilePolicy>,
}

impl ExtensionNativeGrantSnapshot {
    /// Complete exact runtime generation whose grants are retained.
    pub const fn runtime(&self) -> &ExtensionRuntimeFingerprint {
        &self.runtime
    }

    /// Exact grant-row revision bound into the runtime fingerprint.
    pub const fn grant_revision(&self) -> ExtensionGrantRevision {
        self.runtime.grant_revision()
    }

    /// Exact complete grant digest bound into the runtime fingerprint.
    pub const fn grant_digest(&self) -> ExtensionGrantDigest {
        self.runtime.grant_digest()
    }

    /// Browsing partition for which every retained decision is effective.
    pub const fn browsing_context(&self) -> ExtensionGrantBrowsingContext {
        self.runtime.browsing_context()
    }

    pub fn profile_policy_revision(&self) -> ExtensionProfilePolicyRevision {
        self.profile_policy.revision()
    }

    pub fn profile_policy_digest(&self) -> ExtensionProfilePolicyDigest {
        self.profile_policy.digest()
    }

    pub fn denied_sites(&self) -> impl ExactSizeIterator<Item = &ExtensionSiteAccessScope> {
        self.profile_policy.denied_sites().iter()
    }

    /// Exact reviewed compatibility profile bound into the admitted manifest.
    pub fn compatibility_target(&self) -> &ExtensionCompatibilityTargetId {
        self.manifest.compatibility_target()
    }

    fn projection(&self) -> ExtensionNativeGrantProjection<'_> {
        ExtensionNativeGrantProjection::new(
            &self.runtime,
            &self.manifest,
            &self.grants,
            &self.profile_policy,
        )
    }

    /// Canonically ordered complete API declaration stream.
    pub fn api_grants(&self) -> ExtensionNativeApiGrantIter<'_> {
        self.projection().api_grants()
    }

    /// Exact number of declared API permissions in [`Self::api_grants`].
    pub fn api_grant_count(&self) -> usize {
        self.projection().api_grant_count()
    }

    /// Canonically ordered complete host declaration stream.
    pub fn host_grants(&self) -> ExtensionNativeHostGrantIter<'_> {
        self.projection().host_grants()
    }

    /// Exact number of canonical host authorities in [`Self::host_grants`].
    pub fn host_grant_count(&self) -> usize {
        self.projection().host_grant_count()
    }

    /// Whether `file:` is effectively enabled for this exact runtime context.
    pub fn file_scheme_access_granted(&self) -> bool {
        self.projection().file_scheme_access_granted()
    }

    /// Whether this exact runtime is effectively admitted to private context.
    pub fn private_context_access_granted(&self) -> bool {
        self.projection().private_context_access_granted()
    }

    /// Exact storage newly retained beside the originating operation authority.
    ///
    /// The immutable manifest and grant allocations are excluded because the
    /// authority already charges both in full. A trusted host may use this
    /// smaller charge only while its control state continuously charges that
    /// exact authority (or an equivalent quarantine charge) and drops this
    /// snapshot no later than the matching authority state. Standalone owners
    /// must use [`Self::retained_bytes`] instead.
    pub const fn operation_authority_companion_retained_bytes(&self) -> usize {
        size_of::<Self>()
    }

    /// Conservative logical inline-plus-shared-payload charge.
    ///
    /// The immutable Arc payloads are counted in full so this snapshot remains
    /// safely bounded even if it outlives every other reference.
    pub fn retained_bytes(&self) -> usize {
        size_of::<Self>()
            .saturating_add(self.manifest.retained_bytes())
            .saturating_add(self.grants.retained_bytes())
            .saturating_add(self.profile_policy.retained_bytes())
            .saturating_add(3 * RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES)
            .saturating_add(3 * RETAINED_ARC_COUNTER_BYTES)
    }
}

impl fmt::Debug for ExtensionNativeGrantSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionNativeGrantSnapshot")
            .field("runtime", &"<redacted>")
            .field("authority", &"<redacted>")
            .finish()
    }
}

/// Allocation-free canonical iterator over all declared API permissions.
#[derive(Clone)]
pub struct ExtensionNativeApiGrantIter<'a> {
    required: &'a [ApiPermissionName],
    optional: &'a [ApiPermissionName],
    required_index: usize,
    optional_index: usize,
    manifest: &'a ExtensionManifestDescriptor,
    grants: &'a ExtensionGrantAuthority,
    context: ExtensionGrantBrowsingContext,
}

impl<'a> Iterator for ExtensionNativeApiGrantIter<'a> {
    type Item = ExtensionNativeApiGrant<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let required = self.required.get(self.required_index);
        let optional = self.optional.get(self.optional_index);
        let (name, requirement) = match (required, optional) {
            (Some(required), Some(optional)) if required < optional => {
                self.required_index += 1;
                (required, ExtensionNativeGrantRequirement::Required)
            }
            (Some(required), Some(optional)) if optional < required => {
                self.optional_index += 1;
                (optional, ExtensionNativeGrantRequirement::Optional)
            }
            (Some(required), Some(_)) => {
                // The admitted manifest rejects cross-set duplicates. Advancing
                // both is a deterministic fail-closed fallback if that
                // invariant is ever violated internally.
                self.required_index += 1;
                self.optional_index += 1;
                (required, ExtensionNativeGrantRequirement::Required)
            }
            (Some(required), None) => {
                self.required_index += 1;
                (required, ExtensionNativeGrantRequirement::Required)
            }
            (None, Some(optional)) => {
                self.optional_index += 1;
                (optional, ExtensionNativeGrantRequirement::Optional)
            }
            (None, None) => return None,
        };
        Some(ExtensionNativeApiGrant {
            name,
            requirement,
            decision: ExtensionNativeGrantDecision::from_api(self.grants.decide_api(
                self.manifest,
                name,
                self.context,
            )),
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self
            .required
            .len()
            .saturating_sub(self.required_index)
            .saturating_add(self.optional.len().saturating_sub(self.optional_index));
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for ExtensionNativeApiGrantIter<'_> {}
impl FusedIterator for ExtensionNativeApiGrantIter<'_> {}

/// Allocation-free canonical iterator over every declared host authority.
#[derive(Clone)]
pub struct ExtensionNativeHostGrantIter<'a> {
    declarations: &'a ExtensionManifestDeclarations,
    manifest: &'a ExtensionManifestDescriptor,
    grants: &'a ExtensionGrantAuthority,
    context: ExtensionGrantBrowsingContext,
    cursors: [usize; HOST_SOURCE_COUNT],
    last: Option<&'a str>,
    yielded: usize,
}

impl<'a> Iterator for ExtensionNativeHostGrantIter<'a> {
    type Item = ExtensionNativeHostGrant<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut next: Option<(&'a MatchPattern, ExtensionNativeGrantRequirement)> = None;
        for source in 0..HOST_SOURCE_COUNT {
            let patterns = host_source_patterns(self.declarations, source);
            let cursor = &mut self.cursors[source];
            while patterns
                .get(*cursor)
                .is_some_and(|pattern| self.last.is_some_and(|last| pattern.as_str() <= last))
            {
                *cursor += 1;
            }
            let Some(candidate) = patterns.get(*cursor) else {
                continue;
            };
            let requirement = if source == OPTIONAL_HOST_SOURCE_INDEX {
                ExtensionNativeGrantRequirement::Optional
            } else {
                ExtensionNativeGrantRequirement::Required
            };
            match next {
                Some((current, current_requirement))
                    if current.as_str() < candidate.as_str()
                        || (current.as_str() == candidate.as_str()
                            && current_requirement
                                == ExtensionNativeGrantRequirement::Required) => {}
                _ => next = Some((candidate, requirement)),
            }
        }

        let (pattern, requirement) = next?;
        self.last = Some(pattern.as_str());
        self.yielded = self.yielded.saturating_add(1);
        Some(ExtensionNativeHostGrant {
            pattern,
            requirement,
            decision: ExtensionNativeGrantDecision::from_host(self.grants.decide_declared_host(
                self.manifest,
                pattern,
                self.context,
            )),
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (
            0,
            Some(MAX_EXTENSION_HOST_GRANTS.saturating_sub(self.yielded)),
        )
    }
}

impl FusedIterator for ExtensionNativeHostGrantIter<'_> {}

fn host_source_patterns(
    declarations: &ExtensionManifestDeclarations,
    source: usize,
) -> &[MatchPattern] {
    match source {
        REQUIRED_HOST_SOURCE_INDEX => declarations
            .required_hosts()
            .map_or(&[], super::ExtensionHostPermissionSet::patterns),
        CONTENT_SCRIPT_HOST_SOURCE_START..=MAX_EXTENSION_CONTENT_SCRIPT_DECLARATIONS => {
            declarations
                .execution()
                .content_scripts()
                .get(source - CONTENT_SCRIPT_HOST_SOURCE_START)
                .map_or(&[], |script| script.matches().includes())
        }
        OPTIONAL_HOST_SOURCE_INDEX => declarations
            .optional_hosts()
            .map_or(&[], super::ExtensionHostPermissionSet::patterns),
        _ => &[],
    }
}
