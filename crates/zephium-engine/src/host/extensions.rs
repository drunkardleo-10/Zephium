//! Engine-local transient extension document authority.
//!
//! Durable manifest and grant decisions live above the native engine. This
//! module owns only the short-lived join with one exact native runtime and one
//! exact committed tab document. Neither a public presentation id nor a URL
//! alone is authority: every row retains the physical view's event permit and
//! navigation tracker identities.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use zephium_core::extensions::{
    ExtensionActiveTabGrantWitness, ExtensionDocumentAuthorityWitness, ExtensionDocumentPurpose,
    ExtensionGrantDenial, ExtensionRuntimeFingerprint, ExtensionRuntimeInstance,
    ExtensionUrlScopeDecision, MAX_EXTENSION_INSTALLS_PER_PROFILE,
};
use zephium_core::ids::{ItemId, ProfileId};

use crate::navigation_epoch::{
    DocumentOperationGeneration, NavigationActivity, NavigationEpoch, NavigationEpochTracker,
};

use super::permits::EventPermit;
use super::resources::NativeResourceClass;
use super::EngineHost;

/// Every native tab can carry at most one origin grant per installed
/// extension. This is a hard process bound, not an eviction policy: evicting
/// an older authority row would silently change security behavior.
const MAX_ACTIVE_TAB_AUTHORITIES: usize =
    NativeResourceClass::Tab.limit() * MAX_EXTENSION_INSTALLS_PER_PROFILE;
/// Durable state permits this many distinct runtime identities process-wide.
/// The native resource ledger remains the tighter bound for webview-backed
/// owners; this cap prevents a future non-view native context from escaping
/// the profile/install ceilings.
#[cfg(test)]
const MAX_NATIVE_EXTENSION_RUNTIME_OWNERS: usize =
    zephium_core::session::MAX_SESSION_PROFILES * MAX_EXTENSION_INSTALLS_PER_PROFILE;
const MAX_PENDING_DOCUMENT_PERMITS: usize = MAX_ACTIVE_TAB_AUTHORITIES;
const MAX_PENDING_DOCUMENT_PERMITS_PER_AUTHORITY: usize = 4;
const DOCUMENT_PERMIT_TTL: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct DocumentPermitId(u64);

#[derive(Clone, Eq, PartialEq)]
struct DocumentOrigin {
    scheme: WebScheme,
    host: Box<str>,
    port: u16,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum WebScheme {
    Http,
    Https,
}

impl DocumentOrigin {
    fn from_url(url: &str) -> Option<Self> {
        let parsed = url::Url::parse(url).ok()?;
        // `activeTab` is not an escape hatch around the independent file-URL
        // grant, nor around privileged/opaque browser schemes. Add a scheme
        // only when its full product permission model is joined here.
        let scheme = match parsed.scheme() {
            "http" => WebScheme::Http,
            "https" => WebScheme::Https,
            _ => return None,
        };
        Some(Self {
            scheme,
            host: parsed.host_str()?.into(),
            port: parsed.port_or_known_default()?,
        })
    }
}

struct ActiveTabAuthority {
    runtime: ExtensionRuntimeFingerprint,
    event_permit: EventPermit,
    navigation: NavigationEpochTracker,
    epoch: NavigationEpoch,
    url: Arc<str>,
    origin: DocumentOrigin,
}

/// Borrowed exact document identity assembled only from an EngineHost view.
/// Tests construct the same shape from an event permit and tracker harness;
/// no presentation token or caller-provided URL can substitute for it.
struct NativeCommittedDocument<'a> {
    item: ItemId,
    profile: ProfileId,
    event_permit: &'a EventPermit,
    navigation: &'a NavigationEpochTracker,
    epoch: NavigationEpoch,
    url: &'a str,
}

struct OwnedHostDocument {
    item: ItemId,
    profile: ProfileId,
    event_permit: EventPermit,
    navigation: NavigationEpochTracker,
    epoch: NavigationEpoch,
    url: String,
}

impl OwnedHostDocument {
    fn borrowed(&self) -> NativeCommittedDocument<'_> {
        NativeCommittedDocument {
            item: self.item,
            profile: self.profile,
            event_permit: &self.event_permit,
            navigation: &self.navigation,
            epoch: self.epoch,
            url: &self.url,
        }
    }
}

impl ActiveTabAuthority {
    fn matches_native_document(
        &self,
        event_permit: &EventPermit,
        navigation: &NavigationEpochTracker,
        epoch: NavigationEpoch,
        url: &str,
    ) -> bool {
        self.event_permit.same_generation(event_permit)
            && self.navigation.same_generation(navigation)
            && self.event_permit.active_token().is_some()
            && self.navigation.matches_committed_snapshot(epoch, url)
    }
}

/// Actual native runtime ownership must be retained beside every toolbar
/// invocation. There is deliberately no production constructor yet: the
/// native extension adapters do not exist, so production cannot populate the
/// owner map or mint `activeTab` authority. A platform adapter must add a
/// concrete, payload-carrying RAII variant here before exposing ingress. The
/// enum is intentionally uninhabited in production today.
enum NativeExtensionRuntimeOwner {
    /// Full-fingerprint owner used by engine-only authority tests.
    #[cfg(test)]
    ExactFingerprint(Box<ExtensionRuntimeFingerprint>),
    /// Deliberately incomplete low-level registry harness. Retention succeeds,
    /// but every witness-bearing ingress must reject it.
    #[cfg(test)]
    InstanceOnlyHarness(ExtensionRuntimeInstance),
}

impl NativeExtensionRuntimeOwner {
    #[cfg(test)]
    fn into_registry_entry(self) -> (ExtensionRuntimeInstance, Self) {
        match self {
            Self::ExactFingerprint(runtime) => {
                let instance = runtime.instance();
                (instance, Self::ExactFingerprint(runtime))
            }
            Self::InstanceOnlyHarness(runtime) => (runtime, Self::InstanceOnlyHarness(runtime)),
        }
    }

    #[cfg(test)]
    fn authenticates(&self, runtime: &ExtensionRuntimeFingerprint) -> bool {
        match self {
            Self::ExactFingerprint(retained) => retained.as_ref() == runtime,
            Self::InstanceOnlyHarness(_) => false,
        }
    }

    #[cfg(not(test))]
    fn authenticates(&self, _runtime: &ExtensionRuntimeFingerprint) -> bool {
        false
    }

    #[cfg(test)]
    fn exact_fingerprint_harness(runtime: ExtensionRuntimeFingerprint) -> Self {
        Self::ExactFingerprint(Box::new(runtime))
    }

    #[cfg(test)]
    const fn instance_only_harness(runtime: ExtensionRuntimeInstance) -> Self {
        Self::InstanceOnlyHarness(runtime)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DocumentAuthoritySource {
    DurableHost,
    ActiveTab,
}

struct PendingDocumentPermit {
    runtime: ExtensionRuntimeFingerprint,
    witness: ExtensionDocumentAuthorityWitness,
    authority_source: DocumentAuthoritySource,
    item: ItemId,
    event_permit: EventPermit,
    navigation: NavigationEpochTracker,
    epoch: NavigationEpoch,
    activity: NavigationActivity,
    operation_generation: DocumentOperationGeneration,
    url: Arc<str>,
    purpose: ExtensionDocumentPurpose,
    expires_at: Instant,
}

/// One-shot reference to an engine-retained document operation.
///
/// This is intentionally non-`Clone`. Redemption consumes the matching
/// engine row before validating it, so a second presentation of the same
/// object is denied even when the first presentation was stale or expired.
struct ExtensionDocumentPermit {
    id: DocumentPermitId,
    runtime: ExtensionRuntimeFingerprint,
    authority_source: DocumentAuthoritySource,
    item: ItemId,
    epoch: NavigationEpoch,
    activity: NavigationActivity,
    operation_generation: DocumentOperationGeneration,
    url: Arc<str>,
    purpose: ExtensionDocumentPurpose,
    expires_at: Instant,
}

impl fmt::Debug for ExtensionDocumentPermit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionDocumentPermit")
            .field("id", &self.id)
            .field("runtime", &"<redacted>")
            .field("item", &self.item)
            .field("epoch", &self.epoch)
            .field("activity", &self.activity)
            .field("operation_generation", &"<redacted>")
            .field("purpose", &self.purpose)
            .field("authority_source", &self.authority_source)
            .field("url", &"<redacted>")
            .field("expires_at", &"<monotonic-deadline>")
            .finish()
    }
}

/// Internal, non-transferable result of consuming a one-shot permit. The
/// EngineHost wrapper keeps this value on its stack and revalidates it before
/// and after the native call; no port or public API can receive it.
struct RedeemedDocumentGuard {
    runtime: ExtensionRuntimeFingerprint,
    witness: ExtensionDocumentAuthorityWitness,
    authority_source: DocumentAuthoritySource,
    item: ItemId,
    event_permit: EventPermit,
    navigation: NavigationEpochTracker,
    epoch: NavigationEpoch,
    activity: NavigationActivity,
    operation_generation: DocumentOperationGeneration,
    url: Arc<str>,
    purpose: ExtensionDocumentPurpose,
}

impl RedeemedDocumentGuard {
    fn matches_document(
        &self,
        profile: ProfileId,
        event_permit: &EventPermit,
        navigation: &NavigationEpochTracker,
    ) -> bool {
        self.runtime.instance().profile() == profile
            && self.witness.matches(&self.runtime, self.purpose)
            && self.event_permit.same_generation(event_permit)
            && self.navigation.same_generation(navigation)
            && self.event_permit.active_token().is_some()
            && self.navigation.matches_activity(self.activity)
            && self.operation_generation.is_active()
            && self
                .navigation
                .document_operation_snapshot(self.epoch, &self.url)
                .is_some_and(|current| current.same_generation(&self.operation_generation))
    }
}

/// Keep the native call between two validations of the same redeemed guard.
///
/// This helper deliberately owns the ordering rather than leaving it to each
/// platform operation. A native call can pump callbacks, so successful work
/// is not allowed to escape unless the exact runtime owner and document are
/// still authoritative after the call returns.
fn with_revalidated_native_operation<R, V, F>(
    guard: &RedeemedDocumentGuard,
    expected_purpose: ExtensionDocumentPurpose,
    mut validate: V,
    operation: F,
) -> Result<R, ExtensionAuthorityDenial>
where
    V: FnMut(&RedeemedDocumentGuard) -> bool,
    F: FnOnce() -> Result<R, ExtensionAuthorityDenial>,
{
    if guard.purpose != expected_purpose || !validate(guard) {
        return Err(ExtensionAuthorityDenial::NativeDocumentMismatch);
    }
    let result = operation()?;
    if !validate(guard) {
        return Err(ExtensionAuthorityDenial::NativeDocumentMismatch);
    }
    Ok(result)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExtensionAuthorityDenial {
    #[cfg(test)]
    RuntimeOwnerCapacity,
    RuntimeOwnerMissing,
    RuntimeFingerprintMismatch,
    #[cfg(test)]
    RuntimeOwnerAlreadyRetained,
    ActiveTabCapacity,
    PendingPermitCapacity,
    PendingPermitPerAuthorityCapacity,
    PermitIdentityExhausted,
    WrongProfile,
    UnsupportedInvocation,
    InvocationWitnessMismatch,
    PurposeWitnessMismatch,
    DocumentUrlOutOfScope,
    UnsupportedDocumentOrigin,
    DocumentNotPresented,
    NativeDocumentMismatch,
    ActiveTabAuthorityMissing,
    PermitMissingOrReplayed,
    PermitMismatch,
    PermitExpired,
    NativeOperationFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ToolbarActiveTabGrant {
    Granted,
    NotApplicable,
    CapacityExceeded,
    Invalid,
}

#[derive(Default)]
pub(super) struct ExtensionDocumentAuthority {
    runtime_owners: BTreeMap<ExtensionRuntimeInstance, NativeExtensionRuntimeOwner>,
    active_tabs: BTreeMap<(ExtensionRuntimeInstance, ItemId), ActiveTabAuthority>,
    pending_permits: BTreeMap<DocumentPermitId, PendingDocumentPermit>,
    // Read by native navigation callbacks solely to skip an otherwise empty
    // host-queue mutation. It is never authority; false negatives are
    // prevented by publishing true before the retained row is installed and
    // clearing only after the map is observed empty on the host thread.
    pending_presence: Arc<AtomicBool>,
    next_permit_id: u64,
    permit_ids_exhausted: bool,
}

impl ExtensionDocumentAuthority {
    pub(super) fn pending_presence(&self) -> Arc<AtomicBool> {
        self.pending_presence.clone()
    }

    fn active_tab_matches_host_document(
        &self,
        runtime: &ExtensionRuntimeFingerprint,
        document: &NativeCommittedDocument<'_>,
    ) -> bool {
        let instance = runtime.instance();
        instance.profile() == document.profile
            && self
                .active_tabs
                .get(&(instance, document.item))
                .is_some_and(|authority| {
                    authority.runtime == *runtime
                        && authority.epoch == document.epoch
                        && &*authority.url == document.url
                        && authority.matches_native_document(
                            document.event_permit,
                            document.navigation,
                            document.epoch,
                            document.url,
                        )
                })
    }

    /// Retires all prior generations for one install before retaining its new
    /// exact native owner. The owner is moved into this map and cannot be
    /// represented by the freely copyable runtime identity alone.
    #[cfg(test)]
    #[allow(dead_code)] // The first native adapter will become the sole caller.
    fn retain_runtime_owner(
        &mut self,
        owner: NativeExtensionRuntimeOwner,
    ) -> Result<(), ExtensionAuthorityDenial> {
        let (runtime, owner) = owner.into_registry_entry();
        if self.runtime_owners.contains_key(&runtime) {
            return Err(ExtensionAuthorityDenial::RuntimeOwnerAlreadyRetained);
        }

        let superseded: Vec<_> = self
            .runtime_owners
            .keys()
            .copied()
            .filter(|retained| {
                retained.profile() == runtime.profile()
                    && retained.install_id() == runtime.install_id()
            })
            .collect();
        for retained in superseded {
            self.retire_runtime(retained);
        }

        if self.runtime_owners.len() >= MAX_NATIVE_EXTENSION_RUNTIME_OWNERS {
            return Err(ExtensionAuthorityDenial::RuntimeOwnerCapacity);
        }
        self.runtime_owners.insert(runtime, owner);
        Ok(())
    }

    #[cfg(not(test))]
    #[allow(dead_code)] // Uninhabited until the runtime service/native join exists.
    fn retain_runtime_owner(
        &mut self,
        owner: NativeExtensionRuntimeOwner,
    ) -> Result<(), ExtensionAuthorityDenial> {
        match owner {}
    }

    fn require_runtime_owner(
        &self,
        runtime: &ExtensionRuntimeFingerprint,
    ) -> Result<(), ExtensionAuthorityDenial> {
        match self.runtime_owners.get(&runtime.instance()) {
            None => Err(ExtensionAuthorityDenial::RuntimeOwnerMissing),
            Some(owner) if owner.authenticates(runtime) => Ok(()),
            Some(_) => Err(ExtensionAuthorityDenial::RuntimeFingerprintMismatch),
        }
    }

    /// Browser chrome is the sole caller of this user-gesture boundary. It
    /// still cannot mint authority unless the exact runtime's native owner is
    /// retained and the exact physical tab generation has a committed web
    /// origin.
    fn grant_active_tab_from_witness_document(
        &mut self,
        witness: ExtensionActiveTabGrantWitness,
        document: NativeCommittedDocument<'_>,
    ) -> Result<(), ExtensionAuthorityDenial> {
        let runtime = witness.runtime().clone();
        let invocation = witness.invocation();
        if !witness.matches(&runtime, invocation) {
            return Err(ExtensionAuthorityDenial::InvocationWitnessMismatch);
        }
        if invocation.transient_grant_api_name() != "activeTab" {
            return Err(ExtensionAuthorityDenial::UnsupportedInvocation);
        }
        self.require_runtime_owner(&runtime)?;
        let instance = runtime.instance();
        if instance.profile() != document.profile {
            return Err(ExtensionAuthorityDenial::WrongProfile);
        }
        if document.event_permit.active_token().is_none()
            || !document
                .navigation
                .matches_committed_snapshot(document.epoch, document.url)
        {
            return Err(ExtensionAuthorityDenial::NativeDocumentMismatch);
        }
        let Some(origin) = DocumentOrigin::from_url(document.url) else {
            return Err(ExtensionAuthorityDenial::UnsupportedDocumentOrigin);
        };
        let key = (instance, document.item);
        if !self.active_tabs.contains_key(&key)
            && self.active_tabs.len() >= MAX_ACTIVE_TAB_AUTHORITIES
        {
            return Err(ExtensionAuthorityDenial::ActiveTabCapacity);
        }

        self.revoke_pending_for_key(key);
        self.active_tabs.insert(
            key,
            ActiveTabAuthority {
                runtime,
                event_permit: document.event_permit.clone(),
                navigation: document.navigation.clone(),
                epoch: document.epoch,
                url: Arc::from(document.url),
                origin,
            },
        );
        Ok(())
    }

    /// A provisional start invalidates every already-issued one-shot permit,
    /// but deliberately preserves the origin grant. If the load fails before
    /// commit, `activeTab` remains valid for the restored document while the
    /// pre-navigation operation can never become valid again.
    pub(super) fn on_navigation_started(&mut self, item: ItemId) {
        self.revoke_pending_for_item(item);
    }

    /// Reconcile a trusted identity-bearing committed snapshot. Same-origin
    /// navigation retains the user gesture while rebinding it to the new
    /// epoch and exact URL. Cross-origin navigation or physical-generation
    /// replacement revokes the authority row.
    pub(super) fn on_committed_document(
        &mut self,
        item: ItemId,
        event_permit: &EventPermit,
        navigation: &NavigationEpochTracker,
        epoch: NavigationEpoch,
        url: &str,
    ) {
        // The ordinary browsing hot path has no extension grant rows. Avoid
        // URL parsing/allocation entirely until at least one exact item row
        // could be affected.
        if self.active_tabs.is_empty()
            || !self
                .active_tabs
                .keys()
                .any(|(_, row_item)| *row_item == item)
        {
            return;
        }
        let current_origin = DocumentOrigin::from_url(url);
        let mut document_changed = false;
        self.active_tabs.retain(|(_, row_item), authority| {
            if *row_item != item {
                return true;
            }
            let keep = authority.event_permit.same_generation(event_permit)
                && authority.navigation.same_generation(navigation)
                && authority.event_permit.active_token().is_some()
                && navigation.matches_committed_snapshot(epoch, url)
                && current_origin
                    .as_ref()
                    .is_some_and(|origin| *origin == authority.origin);
            if keep {
                document_changed |= authority.epoch != epoch || &*authority.url != url;
                authority.epoch = epoch;
                authority.url = Arc::from(url);
            } else {
                document_changed = true;
            }
            keep
        });
        if document_changed {
            self.revoke_pending_for_item(item);
        }
    }

    fn issue_document_permit_from_host_document(
        &mut self,
        document: NativeCommittedDocument<'_>,
        witness: ExtensionDocumentAuthorityWitness,
        now: Instant,
    ) -> Result<ExtensionDocumentPermit, ExtensionAuthorityDenial> {
        self.prune_expired(now);
        let runtime = witness.runtime().clone();
        let instance = runtime.instance();
        let purpose = witness.purpose();
        if !witness.matches(&runtime, purpose) {
            return Err(ExtensionAuthorityDenial::PurposeWitnessMismatch);
        }
        self.require_runtime_owner(&runtime)?;
        if instance.profile() != document.profile {
            return Err(ExtensionAuthorityDenial::WrongProfile);
        }
        if document.event_permit.active_token().is_none()
            || !document
                .navigation
                .matches_committed_snapshot(document.epoch, document.url)
        {
            return Err(ExtensionAuthorityDenial::NativeDocumentMismatch);
        }
        let authority_source = self.document_authority_source(&witness, &document)?;
        let activity = document
            .navigation
            .activity_snapshot()
            .ok_or(ExtensionAuthorityDenial::NativeDocumentMismatch)?;
        let operation_generation = document
            .navigation
            .document_operation_snapshot(document.epoch, document.url)
            .ok_or(ExtensionAuthorityDenial::NativeDocumentMismatch)?;
        let item = document.item;
        let epoch = document.epoch;
        let url: Arc<str> = Arc::from(document.url);
        if self.pending_permits.len() >= MAX_PENDING_DOCUMENT_PERMITS {
            return Err(ExtensionAuthorityDenial::PendingPermitCapacity);
        }
        let for_authority = self
            .pending_permits
            .values()
            .filter(|pending| pending.runtime.instance() == instance && pending.item == item)
            .count();
        if for_authority >= MAX_PENDING_DOCUMENT_PERMITS_PER_AUTHORITY {
            return Err(ExtensionAuthorityDenial::PendingPermitPerAuthorityCapacity);
        }
        let Some(id) = self.allocate_permit_id() else {
            return Err(ExtensionAuthorityDenial::PermitIdentityExhausted);
        };
        let Some(expires_at) = now.checked_add(DOCUMENT_PERMIT_TTL) else {
            return Err(ExtensionAuthorityDenial::PermitExpired);
        };
        let permit = ExtensionDocumentPermit {
            id,
            runtime: runtime.clone(),
            authority_source,
            item,
            epoch,
            activity,
            operation_generation: operation_generation.clone(),
            url: url.clone(),
            purpose,
            expires_at,
        };
        // A transient false positive only schedules an empty invalidation; a
        // false negative could leave a permit replayable after provisional
        // failure, so publish presence before making the row reachable.
        self.pending_presence.store(true, Ordering::Release);
        self.pending_permits.insert(
            id,
            PendingDocumentPermit {
                runtime,
                witness,
                authority_source,
                item,
                event_permit: document.event_permit.clone(),
                navigation: document.navigation.clone(),
                epoch,
                activity,
                operation_generation,
                url,
                purpose,
                expires_at,
            },
        );
        Ok(permit)
    }

    fn document_authority_source(
        &self,
        witness: &ExtensionDocumentAuthorityWitness,
        document: &NativeCommittedDocument<'_>,
    ) -> Result<DocumentAuthoritySource, ExtensionAuthorityDenial> {
        let url = url::Url::parse(document.url)
            .map_err(|_| ExtensionAuthorityDenial::NativeDocumentMismatch)?;
        match witness.decide_engine_document_url_scope(&url) {
            ExtensionUrlScopeDecision::InScope => Ok(DocumentAuthoritySource::DurableHost),
            ExtensionUrlScopeDecision::OutOfScope(ExtensionGrantDenial::UrlNotGranted)
                if DocumentOrigin::from_url(document.url).is_some() =>
            {
                if self.active_tab_matches_host_document(witness.runtime(), document) {
                    Ok(DocumentAuthoritySource::ActiveTab)
                } else {
                    Err(ExtensionAuthorityDenial::ActiveTabAuthorityMissing)
                }
            }
            ExtensionUrlScopeDecision::OutOfScope(_) => {
                Err(ExtensionAuthorityDenial::DocumentUrlOutOfScope)
            }
        }
    }

    #[cfg(test)]
    fn redeem_document_permit_at(
        &mut self,
        permit: &ExtensionDocumentPermit,
        now: Instant,
    ) -> Result<RedeemedDocumentGuard, ExtensionAuthorityDenial> {
        self.redeem_document_permit_after_host_validation(permit, now)
    }

    fn redeem_document_permit_after_host_validation(
        &mut self,
        permit: &ExtensionDocumentPermit,
        now: Instant,
    ) -> Result<RedeemedDocumentGuard, ExtensionAuthorityDenial> {
        // Remove first. Expired, malformed, and stale presentations are still
        // terminal consumption attempts and can never be replayed later.
        let Some(pending) = self.pending_permits.remove(&permit.id) else {
            return Err(ExtensionAuthorityDenial::PermitMissingOrReplayed);
        };
        self.refresh_pending_presence();
        if pending.runtime != permit.runtime
            || pending.authority_source != permit.authority_source
            || pending.item != permit.item
            || pending.epoch != permit.epoch
            || pending.activity != permit.activity
            || !pending
                .operation_generation
                .same_generation(&permit.operation_generation)
            || pending.url != permit.url
            || pending.purpose != permit.purpose
            || pending.expires_at != permit.expires_at
            || !pending.witness.matches(&pending.runtime, pending.purpose)
        {
            return Err(ExtensionAuthorityDenial::PermitMismatch);
        }
        if now >= pending.expires_at {
            return Err(ExtensionAuthorityDenial::PermitExpired);
        }
        self.require_runtime_owner(&pending.runtime)?;
        if pending.event_permit.active_token().is_none()
            || !pending.navigation.matches_activity(pending.activity)
            || !pending.operation_generation.is_active()
            || !pending
                .navigation
                .matches_committed_snapshot(pending.epoch, &pending.url)
        {
            return Err(ExtensionAuthorityDenial::NativeDocumentMismatch);
        }
        let document = NativeCommittedDocument {
            item: pending.item,
            profile: pending.runtime.instance().profile(),
            event_permit: &pending.event_permit,
            navigation: &pending.navigation,
            epoch: pending.epoch,
            url: &pending.url,
        };
        let source = self.document_authority_source(&pending.witness, &document)?;
        if source != pending.authority_source {
            return Err(ExtensionAuthorityDenial::PermitMismatch);
        }
        Ok(RedeemedDocumentGuard {
            runtime: pending.runtime,
            witness: pending.witness,
            authority_source: pending.authority_source,
            item: pending.item,
            event_permit: pending.event_permit,
            navigation: pending.navigation,
            epoch: pending.epoch,
            activity: pending.activity,
            operation_generation: pending.operation_generation,
            url: pending.url,
            purpose: pending.purpose,
        })
    }

    #[allow(dead_code)] // Runtime coordinator hook; no runtime exists yet.
    pub(super) fn retire_runtime(&mut self, runtime: ExtensionRuntimeInstance) {
        self.active_tabs
            .retain(|(row_runtime, _), _| *row_runtime != runtime);
        self.pending_permits
            .retain(|_, pending| pending.runtime.instance() != runtime);
        self.refresh_pending_presence();
        // Drop the actual native owner only after no document authority can
        // name it or survive its teardown.
        self.runtime_owners.remove(&runtime);
    }

    pub(super) fn revoke_item(&mut self, item: ItemId) {
        self.active_tabs
            .retain(|(_, row_item), _| *row_item != item);
        self.revoke_pending_for_item(item);
    }

    pub(super) fn revoke_profile(&mut self, profile: ProfileId) {
        self.active_tabs
            .retain(|(runtime, _), _| runtime.profile() != profile);
        self.pending_permits
            .retain(|_, pending| pending.runtime.instance().profile() != profile);
        self.refresh_pending_presence();
        self.runtime_owners
            .retain(|runtime, _| runtime.profile() != profile);
    }

    pub(super) fn revoke_all(&mut self) {
        self.active_tabs.clear();
        self.pending_permits.clear();
        self.refresh_pending_presence();
        self.runtime_owners.clear();
    }

    fn allocate_permit_id(&mut self) -> Option<DocumentPermitId> {
        if self.permit_ids_exhausted {
            return None;
        }
        let id = self.next_permit_id.max(1);
        let Some(next) = id.checked_add(1) else {
            self.permit_ids_exhausted = true;
            self.pending_permits.clear();
            self.refresh_pending_presence();
            return None;
        };
        self.next_permit_id = next;
        Some(DocumentPermitId(id))
    }

    fn prune_expired(&mut self, now: Instant) {
        self.pending_permits
            .retain(|_, pending| now < pending.expires_at);
        self.refresh_pending_presence();
    }

    fn revoke_pending_for_key(&mut self, key: (ExtensionRuntimeInstance, ItemId)) {
        self.pending_permits
            .retain(|_, pending| (pending.runtime.instance(), pending.item) != key);
        self.refresh_pending_presence();
    }

    fn revoke_pending_for_item(&mut self, item: ItemId) {
        self.pending_permits
            .retain(|_, pending| pending.item != item);
        self.refresh_pending_presence();
    }

    fn refresh_pending_presence(&self) {
        self.pending_presence
            .store(!self.pending_permits.is_empty(), Ordering::Release);
    }
}

impl EngineHost {
    fn exact_presented_extension_document(
        &self,
        item: ItemId,
        expected_profile: ProfileId,
    ) -> Result<OwnedHostDocument, ExtensionAuthorityDenial> {
        let profile = self
            .partitions
            .get(&item)
            .map(|partition| partition.profile())
            .ok_or(ExtensionAuthorityDenial::NativeDocumentMismatch)?;
        if profile != expected_profile {
            return Err(ExtensionAuthorityDenial::WrongProfile);
        }
        let view = self
            .views
            .get(&item)
            .ok_or(ExtensionAuthorityDenial::NativeDocumentMismatch)?;
        if !view.presentable {
            return Err(ExtensionAuthorityDenial::DocumentNotPresented);
        }
        let (epoch, url) = view
            .navigation
            .committed_snapshot()
            .ok_or(ExtensionAuthorityDenial::NativeDocumentMismatch)?;
        if view.event_permit.active_token().is_none()
            || self
                .navigation_snapshots
                .get(&item)
                .and_then(|snapshot| snapshot.url.as_deref())
                != Some(url.as_str())
        {
            return Err(ExtensionAuthorityDenial::NativeDocumentMismatch);
        }
        Ok(OwnedHostDocument {
            item,
            profile,
            event_permit: view.event_permit.clone(),
            navigation: view.navigation.clone(),
            epoch,
            url,
        })
    }

    /// The only future production ingress for minting activeTab scope. All
    /// document fields are derived from EngineHost-owned maps; the service can
    /// supply only an opaque witness bound to the same runtime/invocation.
    fn grant_active_tab_from_user_invocation(
        &mut self,
        item: ItemId,
        witness: ExtensionActiveTabGrantWitness,
    ) -> Result<(), ExtensionAuthorityDenial> {
        let profile = witness.runtime_instance().profile();
        let document = self.exact_presented_extension_document(item, profile)?;
        self.extension_document_authority
            .grant_active_tab_from_witness_document(witness, document.borrowed())
    }

    /// Joins an operation-authority witness with the exact currently
    /// presented host document. A restricted or not-yet-presented page simply
    /// receives no transient scope; it does not suppress the independent
    /// action click event.
    pub(super) fn grant_toolbar_active_tab(
        &mut self,
        item: ItemId,
        witness: ExtensionActiveTabGrantWitness,
    ) -> ToolbarActiveTabGrant {
        match self.grant_active_tab_from_user_invocation(item, witness) {
            Ok(()) => ToolbarActiveTabGrant::Granted,
            Err(
                ExtensionAuthorityDenial::DocumentNotPresented
                | ExtensionAuthorityDenial::NativeDocumentMismatch
                | ExtensionAuthorityDenial::UnsupportedDocumentOrigin,
            ) => ToolbarActiveTabGrant::NotApplicable,
            Err(ExtensionAuthorityDenial::ActiveTabCapacity) => {
                ToolbarActiveTabGrant::CapacityExceeded
            }
            Err(_) => ToolbarActiveTabGrant::Invalid,
        }
    }

    /// Issues a permit only after joining the purpose witness with the exact
    /// presented EngineHost document and either durable host scope or a valid
    /// exact-document activeTab fallback.
    #[allow(dead_code)]
    fn issue_extension_document_permit(
        &mut self,
        item: ItemId,
        witness: ExtensionDocumentAuthorityWitness,
    ) -> Result<ExtensionDocumentPermit, ExtensionAuthorityDenial> {
        let runtime = witness.runtime().clone();
        let purpose = witness.purpose();
        if !witness.matches(&runtime, purpose) {
            return Err(ExtensionAuthorityDenial::PurposeWitnessMismatch);
        }
        self.extension_document_authority
            .require_runtime_owner(&runtime)?;
        let document =
            self.exact_presented_extension_document(item, runtime.instance().profile())?;
        self.extension_document_authority
            .issue_document_permit_from_host_document(document.borrowed(), witness, Instant::now())
    }

    fn redeemed_guard_matches_host(&self, guard: &RedeemedDocumentGuard) -> bool {
        let Ok(document) =
            self.exact_presented_extension_document(guard.item, guard.runtime.instance().profile())
        else {
            return false;
        };
        self.extension_document_authority
            .require_runtime_owner(&guard.runtime)
            .is_ok()
            && document.epoch == guard.epoch
            && document.url.as_str() == &*guard.url
            && guard.matches_document(
                document.profile,
                &document.event_permit,
                &document.navigation,
            )
            && match guard.authority_source {
                DocumentAuthoritySource::DurableHost => {
                    url::Url::parse(&guard.url).ok().is_some_and(|url| {
                        guard.witness.decide_engine_document_url_scope(&url)
                            == ExtensionUrlScopeDecision::InScope
                    })
                }
                DocumentAuthoritySource::ActiveTab => {
                    self.extension_document_authority
                        .active_tab_matches_host_document(&guard.runtime, &document.borrowed())
                        && DocumentOrigin::from_url(&guard.url).is_some()
                        && url::Url::parse(&guard.url).ok().is_some_and(|url| {
                            guard.witness.decide_engine_document_url_scope(&url)
                                == ExtensionUrlScopeDecision::OutOfScope(
                                    ExtensionGrantDenial::UrlNotGranted,
                                )
                        })
                }
            }
    }

    /// Consume and execute inside one exact host validation boundary. A
    /// native call may pump callbacks, so the same physical view, committed
    /// document, operation generation, URL, and purpose are checked again
    /// before any result can escape.
    #[allow(dead_code)]
    fn with_redeemed_extension_document_operation<R, F>(
        &mut self,
        permit: &ExtensionDocumentPermit,
        operation: F,
    ) -> Result<R, ExtensionAuthorityDenial>
    where
        F: FnOnce(&wry::WebView) -> Result<R, ()>,
    {
        let guard = self
            .extension_document_authority
            .redeem_document_permit_after_host_validation(permit, Instant::now())?;
        let host: &EngineHost = self;
        with_revalidated_native_operation(
            &guard,
            permit.purpose,
            |guard| host.redeemed_guard_matches_host(guard),
            || {
                host.views
                    .get(&guard.item)
                    .ok_or(ExtensionAuthorityDenial::NativeDocumentMismatch)
                    .and_then(|view| {
                        operation(&view.view)
                            .map_err(|()| ExtensionAuthorityDenial::NativeOperationFailed)
                    })
            },
        )
    }

    /// Apply a provisional-navigation revocation only to the exact physical
    /// view generation and navigation epoch carried by the native callback.
    pub(super) fn invalidate_extension_document_permits_for_navigation(
        &mut self,
        item: ItemId,
        source_permit: &EventPermit,
        source_navigation: &NavigationEpochTracker,
    ) {
        let exact = self.views.get(&item).is_some_and(|view| {
            view.event_permit.same_generation(source_permit)
                && view.navigation.same_generation(source_navigation)
        });
        if exact {
            self.extension_document_authority
                .on_navigation_started(item);
        }
    }

    /// Explicit runtime retirement hook for disable, update, crash, and
    /// reconciliation replacement. No current production caller can activate
    /// a runtime; this is ready for the native-owner coordinator that will.
    #[allow(dead_code)]
    pub(crate) fn retire_extension_runtime(&mut self, runtime: ExtensionRuntimeInstance) {
        self.extension_document_authority.retire_runtime(runtime);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::sync::atomic::AtomicBool;

    use wry::{NavigationEvent, NavigationEventPhase, NavigationId};
    use zephium_core::extensions::{
        ApiPermissionName, ExtensionApiPermissionSet, ExtensionArchiveDigest, ExtensionAuthorityId,
        ExtensionCatalogGenerationRole, ExtensionCatalogSetDigest,
        ExtensionCompatibilityClassification, ExtensionCompatibilityLevel,
        ExtensionCompatibilityTargetId, ExtensionContentSecurityPolicyDeclaration,
        ExtensionGrantAuthority, ExtensionGrantBrowsingContext, ExtensionGrantCohort,
        ExtensionGrantManifestBinding, ExtensionGrantManifestBindings, ExtensionHostPermissionSet,
        ExtensionInstall, ExtensionInstallCatalog, ExtensionInstallCatalogRevision,
        ExtensionInstallRevision, ExtensionManifestDeclarations, ExtensionManifestDescriptor,
        ExtensionManifestDigest, ExtensionManifestExecutionSurfaces,
        ExtensionManifestResourceDigest, ExtensionNativeOwnershipJournal,
        ExtensionNativeOwnershipJournalMutation, ExtensionNativeOwnershipKey,
        ExtensionNativeOwnershipPreparation, ExtensionPackageIdentity, ExtensionPackageKey,
        ExtensionPackagePayloadIdentity, ExtensionPackagePinAcquisitionBinding,
        ExtensionPackagePinHeldBinding, ExtensionPackageRevision, ExtensionRuntimeBackendTarget,
        ExtensionRuntimeGeneration, ExtensionRuntimeOperationAuthority, ExtensionTreeDigest,
        ExtensionUserInvocationKind,
    };
    use zephium_core::ids::ExtensionInstallId;
    use zephium_core::injection::{MatchOptions, MatchPattern, MatchSet};

    use super::*;

    struct TestDocument {
        token: Arc<AtomicBool>,
        permit: EventPermit,
        navigation: NavigationEpochTracker,
        epoch: NavigationEpoch,
        url: String,
    }

    impl TestDocument {
        fn committed(native_id: u64, url: &str) -> Self {
            let token = Arc::new(AtomicBool::new(true));
            let permit = EventPermit::bound(&token);
            let navigation = NavigationEpochTracker::new();
            let epoch = commit(&navigation, native_id, url);
            let (_, url) = navigation
                .committed_snapshot()
                .expect("test navigation committed a canonical URL");
            Self {
                token,
                permit,
                navigation,
                epoch,
                url,
            }
        }

        fn navigate_and_commit(&mut self, native_id: u64, url: &str) {
            self.epoch = commit(&self.navigation, native_id, url);
            self.url = self
                .navigation
                .committed_snapshot()
                .expect("test navigation committed a canonical URL")
                .1;
        }
    }

    fn event(id: u64, phase: NavigationEventPhase, url: &str) -> NavigationEvent {
        NavigationEvent {
            id: NavigationId::from_raw(id),
            phase,
            url: url.to_owned(),
        }
    }

    fn commit(tracker: &NavigationEpochTracker, native_id: u64, url: &str) -> NavigationEpoch {
        let epoch = tracker.begin(url).expect("allowed URL starts navigation");
        assert!(matches!(
            tracker.observe_navigation(&event(native_id, NavigationEventPhase::Started, url)),
            Some(crate::navigation_epoch::NavigationTransition::Started(observed)) if observed == epoch
        ));
        assert!(matches!(
            tracker.observe_navigation(&event(native_id, NavigationEventPhase::Committed, url)),
            Some(crate::navigation_epoch::NavigationTransition::Committed(observed)) if observed == epoch
        ));
        epoch
    }

    struct TestRuntime {
        _held_pin: ExtensionPackagePinHeldBinding,
        operation_authority: ExtensionRuntimeOperationAuthority,
        fingerprint: ExtensionRuntimeFingerprint,
    }

    impl TestRuntime {
        fn instance(&self) -> ExtensionRuntimeInstance {
            self.fingerprint.instance()
        }

        fn profile(&self) -> ProfileId {
            self.instance().profile()
        }

        fn active_tab_witness(&self) -> ExtensionActiveTabGrantWitness {
            self.operation_authority
                .mint_active_tab_grant_witness(
                    &self.fingerprint,
                    ExtensionUserInvocationKind::ToolbarAction,
                )
                .unwrap()
        }

        fn document_witness(
            &self,
            purpose: ExtensionDocumentPurpose,
        ) -> ExtensionDocumentAuthorityWitness {
            self.operation_authority
                .mint_document_authority_witness(&self.fingerprint, purpose)
                .unwrap()
        }
    }

    fn runtime(profile: u128, install: u128, generation: u64) -> TestRuntime {
        test_runtime(profile, install, generation, false, false)
    }

    fn durable_runtime(profile: u128, install: u128, generation: u64) -> TestRuntime {
        test_runtime(profile, install, generation, true, false)
    }

    fn test_runtime(
        profile: u128,
        install: u128,
        generation: u64,
        grant_hosts: bool,
        file_access: bool,
    ) -> TestRuntime {
        let profile = ProfileId::from(profile);
        let install_id = ExtensionInstallId::from(install);
        let package = ExtensionPackageIdentity::new(
            ExtensionAuthorityId::from_bytes([1; 32]),
            ExtensionPackageKey::from_bytes([2; 32]),
            ExtensionPackageRevision::INITIAL,
            ExtensionPackagePayloadIdentity::acquired_zip(
                3,
                ExtensionArchiveDigest::from_bytes([3; 32]),
            )
            .unwrap(),
            ExtensionManifestDigest::from_bytes([4; 32]),
            ExtensionTreeDigest::from_bytes([5; 32]),
        );
        let api = |names: &[&str]| {
            ExtensionApiPermissionSet::new(
                names
                    .iter()
                    .map(|name| ApiPermissionName::parse_exact(name).unwrap())
                    .collect(),
            )
            .unwrap()
        };
        let hosts = ExtensionHostPermissionSet::new(
            MatchSet::parse(
                ["<all_urls>"],
                std::iter::empty::<&str>(),
                MatchOptions::default(),
            )
            .unwrap(),
        )
        .unwrap();
        let declarations = ExtensionManifestDeclarations::new(
            api(&[]),
            api(&["activeTab", "scripting"]),
            None,
            Some(hosts),
            None,
            None,
            Vec::new(),
            ExtensionManifestExecutionSurfaces::new(
                Vec::new(),
                ExtensionContentSecurityPolicyDeclaration::new(
                    ExtensionManifestResourceDigest::from_bytes([6; 32]),
                ),
                None,
                Vec::new(),
            )
            .unwrap(),
            Vec::new(),
        )
        .unwrap();
        let compatibility = declarations
            .declaration_keys()
            .into_iter()
            .map(|declaration| {
                ExtensionCompatibilityClassification::new(
                    declaration,
                    ExtensionCompatibilityLevel::Compatible,
                )
            })
            .collect();
        let manifest = Arc::new(
            ExtensionManifestDescriptor::new(
                package,
                3,
                declarations,
                ExtensionCompatibilityTargetId::parse_exact("test.engine.authority.v1").unwrap(),
                compatibility,
            )
            .unwrap(),
        );
        let install = ExtensionInstall::from_persisted(
            install_id,
            ExtensionInstallRevision::new(7).unwrap(),
            manifest.package().clone(),
            true,
        );
        let catalog = ExtensionInstallCatalog::from_persisted(
            ExtensionInstallCatalogRevision::new(9).unwrap(),
            Some(install_id),
            vec![install.clone()],
        )
        .unwrap();
        let bindings =
            ExtensionGrantManifestBindings::new(vec![ExtensionGrantManifestBinding::new(
                install_id,
                Arc::clone(&manifest),
            )])
            .unwrap();
        let authority = ExtensionGrantAuthority::initialize(
            &install,
            ["activeTab", "scripting"]
                .into_iter()
                .map(|name| ApiPermissionName::parse_exact(name).unwrap())
                .collect(),
            if grant_hosts {
                vec![MatchPattern::parse("<all_urls>").unwrap()]
            } else {
                Vec::new()
            },
            file_access,
            false,
            &manifest,
        )
        .unwrap();
        let cohort =
            ExtensionGrantCohort::from_persisted(profile, catalog, bindings, vec![authority])
                .unwrap();
        let eligibility = cohort
            .runtime_eligibility(install_id, ExtensionGrantBrowsingContext::Regular)
            .unwrap();
        let generation =
            ExtensionRuntimeGeneration::new(generation).expect("nonzero test generation");
        let preparation = ExtensionNativeOwnershipPreparation::new(
            ExtensionNativeOwnershipKey::new(
                profile,
                install_id,
                ExtensionGrantBrowsingContext::Regular,
            ),
            eligibility.package().clone(),
            ExtensionCatalogSetDigest::from_bytes([7; 32]),
            ExtensionCatalogGenerationRole::Active,
            eligibility.catalog_revision(),
            eligibility.install_revision(),
            eligibility.grant_revision(),
            eligibility.grant_digest(),
            ExtensionRuntimeBackendTarget::LinuxCompatibility,
        );
        let journal = ExtensionNativeOwnershipJournal::empty();
        let journal_revision = journal.revision();
        let applied = journal
            .apply(
                journal_revision,
                ExtensionNativeOwnershipJournalMutation::begin(preparation),
            )
            .expect("test ownership journal enters NativeAbsentPreparing");
        let acquisition = ExtensionPackagePinAcquisitionBinding::mint(
            applied.entry().expect("begin retains one exact entry"),
            eligibility,
        )
        .expect("test eligibility matches the exact acquisition row");
        let (held_pin, operation_authority) = acquisition
            .into_runtime_parts(generation)
            .into_held_binding_and_operation_authority();
        let fingerprint = operation_authority.fingerprint().clone();
        TestRuntime {
            _held_pin: held_pin,
            operation_authority,
            fingerprint,
        }
    }

    fn retain_runtime(authority: &mut ExtensionDocumentAuthority, runtime: &TestRuntime) {
        authority
            .retain_runtime_owner(NativeExtensionRuntimeOwner::exact_fingerprint_harness(
                runtime.fingerprint.clone(),
            ))
            .expect("test runtime owner retained");
    }

    fn grant(
        authority: &mut ExtensionDocumentAuthority,
        runtime: &TestRuntime,
        item: ItemId,
        document: &TestDocument,
    ) -> Result<(), ExtensionAuthorityDenial> {
        authority.grant_active_tab_from_witness_document(
            runtime.active_tab_witness(),
            NativeCommittedDocument {
                item,
                profile: runtime.profile(),
                event_permit: &document.permit,
                navigation: &document.navigation,
                epoch: document.epoch,
                url: &document.url,
            },
        )
    }

    fn issue(
        authority: &mut ExtensionDocumentAuthority,
        runtime: &TestRuntime,
        item: ItemId,
        document: &TestDocument,
        purpose: ExtensionDocumentPurpose,
        now: Instant,
    ) -> Result<ExtensionDocumentPermit, ExtensionAuthorityDenial> {
        authority.issue_document_permit_from_host_document(
            NativeCommittedDocument {
                item,
                profile: runtime.profile(),
                event_permit: &document.permit,
                navigation: &document.navigation,
                epoch: document.epoch,
                url: &document.url,
            },
            runtime.document_witness(purpose),
            now,
        )
    }

    #[test]
    fn toolbar_invocation_requires_the_exact_retained_native_runtime_and_profile() {
        let mut authority = ExtensionDocumentAuthority::default();
        let runtime = runtime(1, 10, 1);
        let document = TestDocument::committed(1, "https://example.test/page");
        let item = ItemId::from(1);

        assert_eq!(
            grant(&mut authority, &runtime, item, &document),
            Err(ExtensionAuthorityDenial::RuntimeOwnerMissing)
        );
        retain_runtime(&mut authority, &runtime);
        assert_eq!(
            authority.grant_active_tab_from_witness_document(
                runtime.active_tab_witness(),
                NativeCommittedDocument {
                    item,
                    profile: ProfileId::from(2),
                    event_permit: &document.permit,
                    navigation: &document.navigation,
                    epoch: document.epoch,
                    url: &document.url,
                },
            ),
            Err(ExtensionAuthorityDenial::WrongProfile)
        );
        assert_eq!(grant(&mut authority, &runtime, item, &document), Ok(()));
    }

    #[test]
    fn toolbar_invocation_rejects_opaque_file_and_dead_documents() {
        let mut authority = ExtensionDocumentAuthority::default();
        let runtime = runtime(1, 10, 1);
        retain_runtime(&mut authority, &runtime);
        let item = ItemId::from(1);

        for url in [
            "file:///private/data.txt",
            "about:blank",
            "data:text/plain,hello",
        ] {
            assert!(DocumentOrigin::from_url(url).is_none());
        }
        let opaque = TestDocument::committed(3, "about:blank");
        assert_eq!(
            grant(&mut authority, &runtime, item, &opaque),
            Err(ExtensionAuthorityDenial::UnsupportedDocumentOrigin)
        );

        let dead = TestDocument::committed(4, "https://example.test/");
        dead.permit.revoke();
        assert_eq!(
            grant(&mut authority, &runtime, item, &dead),
            Err(ExtensionAuthorityDenial::NativeDocumentMismatch)
        );
    }

    #[test]
    fn same_origin_commits_rebind_but_cross_origin_commits_revoke() {
        let mut authority = ExtensionDocumentAuthority::default();
        let runtime = runtime(1, 10, 1);
        let item = ItemId::from(1);
        let mut document = TestDocument::committed(1, "https://example.test/start");
        retain_runtime(&mut authority, &runtime);
        grant(&mut authority, &runtime, item, &document).unwrap();

        document.navigate_and_commit(2, "https://example.test:443/next");
        authority.on_committed_document(
            item,
            &document.permit,
            &document.navigation,
            document.epoch,
            &document.url,
        );
        let row = authority
            .active_tabs
            .get(&(runtime.instance(), item))
            .expect("same default-port origin retained");
        assert_eq!(row.epoch, document.epoch);
        assert_eq!(&*row.url, document.url);

        document.navigate_and_commit(3, "https://other.test/");
        authority.on_committed_document(
            item,
            &document.permit,
            &document.navigation,
            document.epoch,
            &document.url,
        );
        assert!(!authority
            .active_tabs
            .contains_key(&(runtime.instance(), item)));
    }

    #[test]
    fn physical_view_replacement_revokes_even_when_url_and_item_match() {
        let mut authority = ExtensionDocumentAuthority::default();
        let runtime = runtime(1, 10, 1);
        let item = ItemId::from(1);
        let old = TestDocument::committed(1, "https://example.test/");
        retain_runtime(&mut authority, &runtime);
        grant(&mut authority, &runtime, item, &old).unwrap();

        let replacement = TestDocument::committed(2, "https://example.test/");
        authority.on_committed_document(
            item,
            &replacement.permit,
            &replacement.navigation,
            replacement.epoch,
            &replacement.url,
        );
        assert!(!authority
            .active_tabs
            .contains_key(&(runtime.instance(), item)));
    }

    #[test]
    fn provisional_navigation_consumes_pending_permits_but_preserves_origin_grant() {
        let mut authority = ExtensionDocumentAuthority::default();
        let runtime = runtime(1, 10, 1);
        let item = ItemId::from(1);
        let document = TestDocument::committed(1, "https://example.test/");
        let now = Instant::now();
        retain_runtime(&mut authority, &runtime);
        grant(&mut authority, &runtime, item, &document).unwrap();
        let pending_presence = authority.pending_presence();
        let stale = issue(
            &mut authority,
            &runtime,
            item,
            &document,
            ExtensionDocumentPurpose::ExecuteScript,
            now,
        )
        .unwrap();
        assert!(pending_presence.load(Ordering::Acquire));

        authority.on_navigation_started(item);
        assert!(!pending_presence.load(Ordering::Acquire));
        assert!(authority
            .active_tabs
            .contains_key(&(runtime.instance(), item)));
        assert!(matches!(
            authority.redeem_document_permit_at(&stale, now),
            Err(ExtensionAuthorityDenial::PermitMissingOrReplayed)
        ));
        assert!(issue(
            &mut authority,
            &runtime,
            item,
            &document,
            ExtensionDocumentPurpose::ExecuteScript,
            now,
        )
        .is_ok());
    }

    #[test]
    fn restored_document_cannot_revalidate_a_permit_before_queued_invalidation() {
        let mut authority = ExtensionDocumentAuthority::default();
        let runtime = runtime(1, 10, 1);
        let item = ItemId::from(1);
        let document = TestDocument::committed(1, "https://example.test/original");
        let now = Instant::now();
        retain_runtime(&mut authority, &runtime);
        grant(&mut authority, &runtime, item, &document).unwrap();
        let stale = issue(
            &mut authority,
            &runtime,
            item,
            &document,
            ExtensionDocumentPurpose::ExecuteScript,
            now,
        )
        .unwrap();

        let attempted = document
            .navigation
            .begin("https://example.test/provisional")
            .unwrap();
        assert!(matches!(
            document.navigation.observe_navigation(&event(
                2,
                NavigationEventPhase::Started,
                "https://example.test/provisional",
            )),
            Some(crate::navigation_epoch::NavigationTransition::Started(epoch)) if epoch == attempted
        ));
        assert!(matches!(
            issue(
                &mut authority,
                &runtime,
                item,
                &document,
                ExtensionDocumentPurpose::ExecuteScript,
                now,
            ),
            Err(ExtensionAuthorityDenial::NativeDocumentMismatch)
        ));
        assert!(authority
            .active_tabs
            .contains_key(&(runtime.instance(), item)));
        assert!(matches!(
            document.navigation.observe_navigation(&event(
                2,
                NavigationEventPhase::Failed,
                "https://example.test/provisional",
            )),
            Some(crate::navigation_epoch::NavigationTransition::Failed {
                failed,
                restored: Some(restored),
                ..
            }) if failed == attempted && restored == document.epoch
        ));
        assert_eq!(
            document.navigation.current_committed(),
            Some(document.epoch)
        );

        // Simulate an operation task that was already queued ahead of the
        // Started callback's host invalidation. The synchronously revoked
        // operation generation (and never-restored activity witness) still
        // rejects it, while remove-before-validate kills replay.
        assert!(matches!(
            authority.redeem_document_permit_at(&stale, now),
            Err(ExtensionAuthorityDenial::NativeDocumentMismatch)
        ));
        assert!(matches!(
            authority.redeem_document_permit_at(&stale, now),
            Err(ExtensionAuthorityDenial::PermitMissingOrReplayed)
        ));
        assert!(authority
            .active_tabs
            .contains_key(&(runtime.instance(), item)));
        assert!(issue(
            &mut authority,
            &runtime,
            item,
            &document,
            ExtensionDocumentPurpose::ExecuteScript,
            now,
        )
        .is_ok());
    }

    #[test]
    fn redeemed_guard_fails_postcheck_when_native_work_pumps_navigation() {
        let mut authority = ExtensionDocumentAuthority::default();
        let runtime = runtime(1, 10, 1);
        let item = ItemId::from(1);
        let document = TestDocument::committed(1, "https://example.test/");
        let now = Instant::now();
        retain_runtime(&mut authority, &runtime);
        grant(&mut authority, &runtime, item, &document).unwrap();
        let permit = issue(
            &mut authority,
            &runtime,
            item,
            &document,
            ExtensionDocumentPurpose::ExecuteScript,
            now,
        )
        .unwrap();
        let guard = authority.redeem_document_permit_at(&permit, now).unwrap();
        assert!(guard.matches_document(runtime.profile(), &document.permit, &document.navigation,));

        // `begin` is the synchronous tracker boundary used by programmatic
        // navigation; native Started performs the same terminal revocation.
        document
            .navigation
            .begin("https://example.test/navigation-pumped-by-native-call")
            .unwrap();
        assert!(!guard.matches_document(runtime.profile(), &document.permit, &document.navigation,));
    }

    #[test]
    fn core_ingress_witnesses_are_exact_runtime_invocation_and_purpose_bound() {
        let first = runtime(1, 10, 1);
        let second = runtime(1, 10, 2);
        let invocation = ExtensionUserInvocationKind::ToolbarAction;
        let grant = first.active_tab_witness();
        assert!(grant.matches(&first.fingerprint, invocation));
        assert!(!grant.matches(&second.fingerprint, invocation));

        let purpose = first.document_witness(ExtensionDocumentPurpose::InsertCss);
        assert!(purpose.matches(&first.fingerprint, ExtensionDocumentPurpose::InsertCss));
        assert!(!purpose.matches(&first.fingerprint, ExtensionDocumentPurpose::RemoveCss));
        assert!(!purpose.matches(&second.fingerprint, ExtensionDocumentPurpose::InsertCss));
    }

    #[test]
    fn instance_only_owner_and_same_instance_fingerprint_drift_fail_closed() {
        let mut authority = ExtensionDocumentAuthority::default();
        let retained = runtime(1, 10, 1);
        let different_fingerprint = durable_runtime(1, 10, 1);
        let item = ItemId::from(1);
        let document = TestDocument::committed(1, "https://example.test/");

        authority
            .retain_runtime_owner(NativeExtensionRuntimeOwner::instance_only_harness(
                retained.instance(),
            ))
            .unwrap();
        assert_eq!(
            authority.grant_active_tab_from_witness_document(
                retained.active_tab_witness(),
                NativeCommittedDocument {
                    item,
                    profile: retained.profile(),
                    event_permit: &document.permit,
                    navigation: &document.navigation,
                    epoch: document.epoch,
                    url: &document.url,
                },
            ),
            Err(ExtensionAuthorityDenial::RuntimeFingerprintMismatch)
        );
        authority.runtime_owners.clear();
        retain_runtime(&mut authority, &retained);
        assert_eq!(
            grant(&mut authority, &different_fingerprint, item, &document,),
            Err(ExtensionAuthorityDenial::RuntimeFingerprintMismatch)
        );
    }

    #[test]
    fn durable_host_scope_issues_and_redeems_without_an_active_tab_row() {
        let mut authority = ExtensionDocumentAuthority::default();
        let runtime = durable_runtime(1, 10, 1);
        let item = ItemId::from(1);
        let document = TestDocument::committed(1, "https://example.test/durable");
        let now = Instant::now();
        retain_runtime(&mut authority, &runtime);

        let permit = issue(
            &mut authority,
            &runtime,
            item,
            &document,
            ExtensionDocumentPurpose::ExecuteScript,
            now,
        )
        .unwrap();
        assert_eq!(
            permit.authority_source,
            DocumentAuthoritySource::DurableHost
        );
        assert!(authority.active_tabs.is_empty());
        let guard = authority.redeem_document_permit_at(&permit, now).unwrap();
        assert_eq!(guard.authority_source, DocumentAuthoritySource::DurableHost);
        assert!(authority.active_tabs.is_empty());
    }

    #[test]
    fn active_tab_is_only_the_exact_http_url_not_granted_fallback() {
        let mut authority = ExtensionDocumentAuthority::default();
        let runtime = runtime(1, 10, 1);
        let item = ItemId::from(1);
        let web = TestDocument::committed(1, "https://example.test/fallback");
        let now = Instant::now();
        retain_runtime(&mut authority, &runtime);

        assert!(matches!(
            issue(
                &mut authority,
                &runtime,
                item,
                &web,
                ExtensionDocumentPurpose::ExecuteScript,
                now,
            ),
            Err(ExtensionAuthorityDenial::ActiveTabAuthorityMissing)
        ));
        grant(&mut authority, &runtime, item, &web).unwrap();
        let fallback = issue(
            &mut authority,
            &runtime,
            item,
            &web,
            ExtensionDocumentPurpose::ExecuteScript,
            now,
        )
        .unwrap();
        assert_eq!(
            fallback.authority_source,
            DocumentAuthoritySource::ActiveTab
        );

        let blank = TestDocument::committed(2, "about:blank");
        assert!(matches!(
            issue(
                &mut authority,
                &runtime,
                item,
                &blank,
                ExtensionDocumentPurpose::ExecuteScript,
                now,
            ),
            Err(ExtensionAuthorityDenial::DocumentUrlOutOfScope)
        ));
    }

    #[test]
    fn exact_fingerprint_is_rechecked_at_redeem_and_after_redemption() {
        let mut authority = ExtensionDocumentAuthority::default();
        let runtime = runtime(1, 10, 1);
        let drifted = durable_runtime(1, 10, 1);
        let item = ItemId::from(1);
        let document = TestDocument::committed(1, "https://example.test/");
        let now = Instant::now();
        retain_runtime(&mut authority, &runtime);
        grant(&mut authority, &runtime, item, &document).unwrap();
        let permit = issue(
            &mut authority,
            &runtime,
            item,
            &document,
            ExtensionDocumentPurpose::ExecuteScript,
            now,
        )
        .unwrap();
        authority.runtime_owners.insert(
            runtime.instance(),
            NativeExtensionRuntimeOwner::exact_fingerprint_harness(drifted.fingerprint.clone()),
        );
        assert!(matches!(
            authority.redeem_document_permit_at(&permit, now),
            Err(ExtensionAuthorityDenial::RuntimeFingerprintMismatch)
        ));

        authority.runtime_owners.insert(
            runtime.instance(),
            NativeExtensionRuntimeOwner::exact_fingerprint_harness(runtime.fingerprint.clone()),
        );
        let permit = issue(
            &mut authority,
            &runtime,
            item,
            &document,
            ExtensionDocumentPurpose::ExecuteScript,
            now,
        )
        .unwrap();
        let guard = authority.redeem_document_permit_at(&permit, now).unwrap();
        authority.runtime_owners.insert(
            runtime.instance(),
            NativeExtensionRuntimeOwner::exact_fingerprint_harness(drifted.fingerprint),
        );
        assert_eq!(
            authority.require_runtime_owner(&guard.runtime),
            Err(ExtensionAuthorityDenial::RuntimeFingerprintMismatch)
        );
    }

    #[test]
    fn native_operation_boundaries_contain_owner_fingerprint_drift() {
        let mut authority = ExtensionDocumentAuthority::default();
        let runtime = runtime(1, 10, 1);
        let drifted = durable_runtime(1, 10, 1);
        let item = ItemId::from(1);
        let document = TestDocument::committed(1, "https://example.test/");
        let now = Instant::now();
        retain_runtime(&mut authority, &runtime);
        grant(&mut authority, &runtime, item, &document).unwrap();

        let before_native = issue(
            &mut authority,
            &runtime,
            item,
            &document,
            ExtensionDocumentPurpose::ExecuteScript,
            now,
        )
        .unwrap();
        let before_guard = authority
            .redeem_document_permit_at(&before_native, now)
            .unwrap();
        let authority = RefCell::new(authority);
        authority.borrow_mut().runtime_owners.insert(
            runtime.instance(),
            NativeExtensionRuntimeOwner::exact_fingerprint_harness(drifted.fingerprint.clone()),
        );
        let native_called = Cell::new(false);
        assert_eq!(
            with_revalidated_native_operation(
                &before_guard,
                ExtensionDocumentPurpose::ExecuteScript,
                |guard| {
                    authority
                        .borrow()
                        .require_runtime_owner(&guard.runtime)
                        .is_ok()
                        && guard.matches_document(
                            runtime.profile(),
                            &document.permit,
                            &document.navigation,
                        )
                },
                || {
                    native_called.set(true);
                    Ok(())
                },
            ),
            Err(ExtensionAuthorityDenial::NativeDocumentMismatch)
        );
        assert!(
            !native_called.get(),
            "pre-native drift must skip native work"
        );

        authority.borrow_mut().runtime_owners.insert(
            runtime.instance(),
            NativeExtensionRuntimeOwner::exact_fingerprint_harness(runtime.fingerprint.clone()),
        );
        let after_native = issue(
            &mut authority.borrow_mut(),
            &runtime,
            item,
            &document,
            ExtensionDocumentPurpose::ExecuteScript,
            now,
        )
        .unwrap();
        let after_guard = authority
            .borrow_mut()
            .redeem_document_permit_at(&after_native, now)
            .unwrap();
        let native_called = Cell::new(false);
        assert_eq!(
            with_revalidated_native_operation(
                &after_guard,
                ExtensionDocumentPurpose::ExecuteScript,
                |guard| {
                    authority
                        .borrow()
                        .require_runtime_owner(&guard.runtime)
                        .is_ok()
                        && guard.matches_document(
                            runtime.profile(),
                            &document.permit,
                            &document.navigation,
                        )
                },
                || {
                    native_called.set(true);
                    authority.borrow_mut().runtime_owners.insert(
                        runtime.instance(),
                        NativeExtensionRuntimeOwner::exact_fingerprint_harness(
                            drifted.fingerprint.clone(),
                        ),
                    );
                    Ok(())
                },
            ),
            Err(ExtensionAuthorityDenial::NativeDocumentMismatch)
        );
        assert!(
            native_called.get(),
            "post-native drift occurs during native work"
        );
    }

    #[test]
    fn document_permits_are_exact_one_shot_and_expiring() {
        let mut authority = ExtensionDocumentAuthority::default();
        let runtime = runtime(1, 10, 1);
        let item = ItemId::from(1);
        let document = TestDocument::committed(1, "https://example.test/private?secret=1");
        let now = Instant::now();
        retain_runtime(&mut authority, &runtime);
        grant(&mut authority, &runtime, item, &document).unwrap();

        let permit = issue(
            &mut authority,
            &runtime,
            item,
            &document,
            ExtensionDocumentPurpose::InsertCss,
            now,
        )
        .unwrap();
        assert!(authority.redeem_document_permit_at(&permit, now).is_ok());
        assert!(matches!(
            authority.redeem_document_permit_at(&permit, now),
            Err(ExtensionAuthorityDenial::PermitMissingOrReplayed)
        ));

        let expired = issue(
            &mut authority,
            &runtime,
            item,
            &document,
            ExtensionDocumentPurpose::RemoveCss,
            now,
        )
        .unwrap();
        assert!(matches!(
            authority.redeem_document_permit_at(&expired, now + DOCUMENT_PERMIT_TTL),
            Err(ExtensionAuthorityDenial::PermitExpired)
        ));
        assert!(matches!(
            authority.redeem_document_permit_at(&expired, now),
            Err(ExtensionAuthorityDenial::PermitMissingOrReplayed)
        ));
    }

    #[test]
    fn document_permit_debug_redacts_url_and_deadline() {
        let mut authority = ExtensionDocumentAuthority::default();
        let runtime = runtime(1, 10, 1);
        let item = ItemId::from(1);
        let document = TestDocument::committed(1, "https://example.test/private?secret=1");
        retain_runtime(&mut authority, &runtime);
        grant(&mut authority, &runtime, item, &document).unwrap();
        let permit = issue(
            &mut authority,
            &runtime,
            item,
            &document,
            ExtensionDocumentPurpose::ExecuteScript,
            Instant::now(),
        )
        .unwrap();

        let debug = format!("{permit:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("ExtensionRuntimeFingerprint"));
        assert!(!debug.contains("example.test"));
        assert!(!debug.contains("secret=1"));
    }

    #[test]
    fn runtime_update_profile_retirement_close_and_shutdown_revoke_exact_scopes() {
        let mut authority = ExtensionDocumentAuthority::default();
        let first = runtime(1, 10, 1);
        let replacement = runtime(1, 10, 2);
        let sibling = runtime(2, 20, 1);
        let first_item = ItemId::from(1);
        let sibling_item = ItemId::from(2);
        let first_document = TestDocument::committed(1, "https://first.test/");
        let sibling_document = TestDocument::committed(2, "https://sibling.test/");
        retain_runtime(&mut authority, &first);
        retain_runtime(&mut authority, &sibling);
        grant(&mut authority, &first, first_item, &first_document).unwrap();
        grant(&mut authority, &sibling, sibling_item, &sibling_document).unwrap();

        retain_runtime(&mut authority, &replacement);
        assert!(!authority.runtime_owners.contains_key(&first.instance()));
        assert!(!authority
            .active_tabs
            .contains_key(&(first.instance(), first_item)));
        assert!(authority
            .runtime_owners
            .contains_key(&replacement.instance()));
        assert!(authority.runtime_owners.contains_key(&sibling.instance()));

        authority.revoke_item(sibling_item);
        assert!(!authority
            .active_tabs
            .contains_key(&(sibling.instance(), sibling_item)));
        authority.revoke_profile(ProfileId::from(1));
        assert!(!authority
            .runtime_owners
            .contains_key(&replacement.instance()));
        assert!(authority.runtime_owners.contains_key(&sibling.instance()));
        authority.revoke_all();
        assert!(authority.runtime_owners.is_empty());
        assert!(authority.active_tabs.is_empty());
        assert!(authority.pending_permits.is_empty());
    }

    #[test]
    fn pending_permit_cap_is_per_authority_and_never_evicts() {
        let mut authority = ExtensionDocumentAuthority::default();
        let runtime = runtime(1, 10, 1);
        let item = ItemId::from(1);
        let document = TestDocument::committed(1, "https://example.test/");
        let now = Instant::now();
        retain_runtime(&mut authority, &runtime);
        grant(&mut authority, &runtime, item, &document).unwrap();

        let mut permits = Vec::new();
        for _ in 0..MAX_PENDING_DOCUMENT_PERMITS_PER_AUTHORITY {
            permits.push(
                issue(
                    &mut authority,
                    &runtime,
                    item,
                    &document,
                    ExtensionDocumentPurpose::ExecuteScript,
                    now,
                )
                .unwrap(),
            );
        }
        assert!(matches!(
            issue(
                &mut authority,
                &runtime,
                item,
                &document,
                ExtensionDocumentPurpose::ExecuteScript,
                now,
            ),
            Err(ExtensionAuthorityDenial::PendingPermitPerAuthorityCapacity)
        ));
        assert_eq!(authority.pending_permits.len(), permits.len());
    }

    #[test]
    fn same_document_url_change_rebinds_origin_grant_and_consumes_old_permit() {
        let mut authority = ExtensionDocumentAuthority::default();
        let runtime = runtime(1, 10, 1);
        let item = ItemId::from(1);
        let mut document = TestDocument::committed(1, "https://example.test/start");
        let now = Instant::now();
        retain_runtime(&mut authority, &runtime);
        grant(&mut authority, &runtime, item, &document).unwrap();
        let old = issue(
            &mut authority,
            &runtime,
            item,
            &document,
            ExtensionDocumentPurpose::ExecuteScript,
            now,
        )
        .unwrap();

        assert!(document
            .navigation
            .observe_source(document.epoch, "https://example.test/replaced?history=1"));
        document.url = document.navigation.committed_snapshot().unwrap().1;
        authority.on_committed_document(
            item,
            &document.permit,
            &document.navigation,
            document.epoch,
            &document.url,
        );

        let retained = authority
            .active_tabs
            .get(&(runtime.instance(), item))
            .unwrap();
        assert_eq!(&*retained.url, document.url);
        assert!(matches!(
            authority.redeem_document_permit_at(&old, now),
            Err(ExtensionAuthorityDenial::PermitMissingOrReplayed)
        ));
    }

    #[test]
    fn malformed_permit_attempt_is_consumed_before_validation() {
        let mut authority = ExtensionDocumentAuthority::default();
        let runtime = runtime(1, 10, 1);
        let item = ItemId::from(1);
        let document = TestDocument::committed(1, "https://example.test/");
        let now = Instant::now();
        retain_runtime(&mut authority, &runtime);
        grant(&mut authority, &runtime, item, &document).unwrap();
        let mut malformed = issue(
            &mut authority,
            &runtime,
            item,
            &document,
            ExtensionDocumentPurpose::ExecuteScript,
            now,
        )
        .unwrap();
        malformed.url = Arc::from("https://example.test/counterfeit");

        assert!(matches!(
            authority.redeem_document_permit_at(&malformed, now),
            Err(ExtensionAuthorityDenial::PermitMismatch)
        ));
        assert!(matches!(
            authority.redeem_document_permit_at(&malformed, now),
            Err(ExtensionAuthorityDenial::PermitMissingOrReplayed)
        ));
    }

    #[test]
    fn active_tab_and_pending_global_caps_refuse_without_eviction() {
        let mut authority = ExtensionDocumentAuthority::default();
        let document = TestDocument::committed(1, "https://example.test/");
        let runtimes: Vec<_> = (0..MAX_EXTENSION_INSTALLS_PER_PROFILE)
            .map(|index| runtime(1, index as u128 + 1, 1))
            .collect();
        for runtime in &runtimes {
            retain_runtime(&mut authority, runtime);
        }
        for item_index in 0..NativeResourceClass::Tab.limit() {
            let item = ItemId::from(item_index as u128 + 1);
            for runtime in &runtimes {
                grant(&mut authority, runtime, item, &document).unwrap();
            }
        }
        assert_eq!(authority.active_tabs.len(), MAX_ACTIVE_TAB_AUTHORITIES);
        let overflow_runtime = runtime(1, 99, 1);
        retain_runtime(&mut authority, &overflow_runtime);
        assert_eq!(
            grant(
                &mut authority,
                &overflow_runtime,
                ItemId::from(1),
                &document,
            ),
            Err(ExtensionAuthorityDenial::ActiveTabCapacity)
        );
        assert!(authority
            .active_tabs
            .contains_key(&(runtimes[0].instance(), ItemId::from(1))));

        let mut pending_authority = ExtensionDocumentAuthority::default();
        for runtime in &runtimes {
            retain_runtime(&mut pending_authority, runtime);
        }
        let now = Instant::now();
        let authority_count =
            MAX_PENDING_DOCUMENT_PERMITS / MAX_PENDING_DOCUMENT_PERMITS_PER_AUTHORITY;
        for index in 0..authority_count {
            let runtime = &runtimes[index % runtimes.len()];
            let item = ItemId::from((index / runtimes.len()) as u128 + 1);
            grant(&mut pending_authority, runtime, item, &document).unwrap();
            for _ in 0..MAX_PENDING_DOCUMENT_PERMITS_PER_AUTHORITY {
                issue(
                    &mut pending_authority,
                    runtime,
                    item,
                    &document,
                    ExtensionDocumentPurpose::ExecuteScript,
                    now,
                )
                .unwrap();
            }
        }
        assert_eq!(
            pending_authority.pending_permits.len(),
            MAX_PENDING_DOCUMENT_PERMITS
        );
        let extra_runtime = &runtimes[0];
        let extra_item = ItemId::from((authority_count / runtimes.len()) as u128 + 1);
        grant(&mut pending_authority, extra_runtime, extra_item, &document).unwrap();
        assert!(matches!(
            issue(
                &mut pending_authority,
                extra_runtime,
                extra_item,
                &document,
                ExtensionDocumentPurpose::ExecuteScript,
                now,
            ),
            Err(ExtensionAuthorityDenial::PendingPermitCapacity)
        ));
        assert_eq!(
            pending_authority.pending_permits.len(),
            MAX_PENDING_DOCUMENT_PERMITS
        );
    }

    #[test]
    fn runtime_owner_cap_is_exact_and_does_not_evict() {
        let mut authority = ExtensionDocumentAuthority::default();
        let mut first = None;
        for profile in 0..zephium_core::session::MAX_SESSION_PROFILES {
            for install in 0..MAX_EXTENSION_INSTALLS_PER_PROFILE {
                let runtime = runtime(profile as u128 + 1, install as u128 + 1, 1);
                first.get_or_insert(runtime.instance());
                retain_runtime(&mut authority, &runtime);
            }
        }
        assert_eq!(
            authority.runtime_owners.len(),
            MAX_NATIVE_EXTENSION_RUNTIME_OWNERS
        );
        assert_eq!(
            authority.retain_runtime_owner(NativeExtensionRuntimeOwner::exact_fingerprint_harness(
                runtime(9_999, 9_999, 1).fingerprint,
            )),
            Err(ExtensionAuthorityDenial::RuntimeOwnerCapacity)
        );
        assert!(authority.runtime_owners.contains_key(&first.unwrap()));
    }

    #[test]
    fn permit_counter_exhaustion_is_terminal_and_clears_outstanding_rows() {
        let mut authority = ExtensionDocumentAuthority::default();
        let runtime = runtime(1, 10, 1);
        let item = ItemId::from(1);
        let document = TestDocument::committed(1, "https://example.test/");
        retain_runtime(&mut authority, &runtime);
        grant(&mut authority, &runtime, item, &document).unwrap();
        authority.next_permit_id = u64::MAX - 1;
        let now = Instant::now();
        let last = issue(
            &mut authority,
            &runtime,
            item,
            &document,
            ExtensionDocumentPurpose::ExecuteScript,
            now,
        )
        .unwrap();
        assert_eq!(last.id, DocumentPermitId(u64::MAX - 1));
        assert!(matches!(
            issue(
                &mut authority,
                &runtime,
                item,
                &document,
                ExtensionDocumentPurpose::ExecuteScript,
                now,
            ),
            Err(ExtensionAuthorityDenial::PermitIdentityExhausted)
        ));
        assert!(authority.pending_permits.is_empty());
        assert!(matches!(
            issue(
                &mut authority,
                &runtime,
                item,
                &document,
                ExtensionDocumentPurpose::ExecuteScript,
                now,
            ),
            Err(ExtensionAuthorityDenial::PermitIdentityExhausted)
        ));
    }

    #[test]
    fn test_documents_keep_their_event_generation_alive() {
        let document = TestDocument::committed(1, "https://example.test/");
        assert!(document.permit.matches_token(&document.token));
    }

    #[test]
    fn production_wiring_revokes_at_every_existing_host_lifecycle_boundary() {
        let lifecycle = include_str!("lifecycle.rs");
        let close = lifecycle
            .split_once("pub(crate) fn close(&mut self, id: ItemId)")
            .unwrap()
            .1
            .split_once("#[cfg(not(target_os = \"windows\"))]")
            .unwrap()
            .0;
        assert!(
            close
                .find("extension_document_authority.revoke_item(id)")
                .unwrap()
                < close.find("self.views.remove(&id)").unwrap()
        );
        let shutdown = lifecycle
            .split_once("fn shutdown_common(&mut self)")
            .unwrap()
            .1;
        assert!(
            shutdown
                .find("extension_document_authority.revoke_all()")
                .unwrap()
                < shutdown.find("self.close(id)").unwrap()
        );

        let profiles = include_str!("profiles.rs");
        assert!(profiles.matches("extension_document_authority").count() >= 2);
        let construction = include_str!("construction.rs");
        assert!(construction.contains("NavigationTransition::Started(epoch)"));
        assert!(construction.contains("queue_navigation_authority_invalidation"));
        let navigation = include_str!("navigation.rs");
        assert!(navigation.contains("extension_document_authority.on_committed_document"));

        let production = include_str!("extensions.rs")
            .split_once("#[cfg(test)]\nmod tests")
            .unwrap()
            .0;
        assert!(!production.contains("NavigationPresentationId"));
        assert!(!production.contains("evaluate_script"));
        assert!(!production.contains("execute_script"));
        assert!(!production.contains("enum ActiveTabGrantWitness"));
        assert!(!production.contains("enum DocumentPurposeWitness"));
        assert!(!production.contains("grant_active_tab_from_host_document"));
        assert!(production.contains("witness: ExtensionActiveTabGrantWitness"));
        assert!(production.contains("witness: ExtensionDocumentAuthorityWitness"));
    }
}
