//! Bounded one-way Windows cookie-transfer contracts.
//!
//! Cookie values remain inside the trusted native adapter. The shell submits
//! only canonical origins and exact context/profile authority; native results
//! return counts and closed failure classes. No reverse synchronization,
//! arbitrary storage bridge, cookie value, header, or profile path is
//! representable here.

use std::collections::BTreeMap;
use std::fmt;
use std::num::NonZeroU64;

use thiserror::Error;
use url::Url;

use crate::{
    ContextCapabilities, ContextCapability, ContextConstructionProof, ContextId, ContextJoin,
    ContextKind, ContextNavigationTarget, ContextProfileLease,
};

/// Maximum canonical origins in one native cookie transfer.
pub const MAX_COOKIE_TRANSFER_ORIGINS: usize = 8;
/// Maximum unique native-exposed cookies in one transfer.
pub const MAX_COOKIES_PER_TRANSFER: usize = 256;
/// Maximum UTF-8 bytes across native-exposed cookie fields in one transfer.
pub const MAX_COOKIE_TRANSFER_BYTES: usize = 512 * 1024;
/// Maximum UTF-8 bytes across fields of one native-exposed cookie.
pub const MAX_COOKIE_BYTES: usize = 16 * 1024;
/// Maximum native lifetime of one cookie transfer, including cleanup proof.
///
/// The Windows adapter reserves the final ten seconds for profile-wide
/// cleanup. This ceiling bounds retained cookie managers and callback owners;
/// physical qualification may lower it but cannot widen it at runtime.
pub const MAX_COOKIE_TRANSFER_MILLIS: u64 = 30_000;
/// Maximum cookie transfers concurrently retained by the process.
pub const MAX_PENDING_COOKIE_TRANSFERS: usize = 2;

/// Refusal while constructing or coordinating a cookie-transfer value.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ContextCookieTransferError {
    /// A scope must contain at least one canonical web origin.
    #[error("cookie transfer origin scope is empty")]
    EmptyScope,
    /// The fixed origin ceiling was exceeded.
    #[error("cookie transfer origin ceiling exceeded")]
    OriginLimit,
    /// The same canonical origin appeared more than once.
    #[error("cookie transfer origin scope contains a duplicate")]
    DuplicateOrigin,
    /// The target does not have an HTTP or HTTPS tuple origin.
    #[error("cookie transfer origin is invalid")]
    InvalidOrigin,
    /// Endpoint context kind is incompatible with this transfer direction.
    #[error("cookie transfer context kind is incompatible")]
    ContextKind,
    /// Endpoints are owned by different runs.
    #[error("cookie transfer owner does not match")]
    OwnerMismatch,
    /// Endpoints select different browser profiles.
    #[error("cookie transfer profile does not match")]
    ProfileMismatch,
    /// Source and destination resolve to the same context identity.
    #[error("cookie transfer endpoints must be distinct")]
    SameContext,
    /// A required negotiated native capability is absent.
    #[error("cookie transfer capability is unavailable")]
    CapabilityUnavailable,
    /// Construction did not prove the required Windows profile binding.
    #[error("cookie transfer storage proof is incompatible")]
    ConstructionProof,
    /// A retained profile lease does not match its endpoint exactly.
    #[error("cookie transfer profile lease does not match")]
    ProfileLease,
    /// Returned counts exceed a fixed cookie or payload ceiling.
    #[error("cookie transfer result ceiling exceeded")]
    ResultLimit,
    /// Returned counts contradict each other or the requested scope.
    #[error("cookie transfer result is contradictory")]
    ResultInvariant,
    /// The process-local request/deadline interval was empty or too large.
    #[error("cookie transfer deadline is invalid")]
    Deadline,
    /// Process shutdown permanently sealed new transfers.
    #[error("cookie transfers are sealed for shutdown")]
    ShutdownSealed,
    /// The fixed concurrent transfer ceiling is full.
    #[error("concurrent cookie transfer ceiling exceeded")]
    TransferLimit,
    /// The process-local transfer identity is already active.
    #[error("cookie transfer identity is already active")]
    DuplicateTransfer,
    /// Another active transfer owns the destination automation profile.
    ///
    /// WebView2 cookie managers are profile-scoped, so distinct destination
    /// contexts on one profile are the same physical mutation target.
    #[error("cookie transfer destination profile is busy")]
    DestinationBusy,
    /// No active transfer has the exact identity.
    #[error("cookie transfer was not found")]
    NotFound,
    /// Settlement does not match the exact admitted request.
    #[error("cookie transfer settlement is stale")]
    StaleSettlement,
    /// Internal bounded accounting became contradictory.
    #[error("cookie transfer accounting invariant failed")]
    Invariant,
}

/// Process-local monotonic time used only to bound native cookie work.
#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub struct ContextCookieTransferInstant(u64);

impl ContextCookieTransferInstant {
    /// Wraps a monotonic millisecond tick from one process-local clock domain.
    pub const fn from_millis(value: u64) -> Self {
        Self(value)
    }

    /// Returns the process-local tick to the trusted runtime adapter.
    pub const fn millis(self) -> u64 {
        self.0
    }
}

impl fmt::Debug for ContextCookieTransferInstant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ContextCookieTransferInstant([redacted])")
    }
}

/// Exact bounded lifetime of one native cookie-transfer attempt.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ContextCookieTransferWindow {
    requested_at: ContextCookieTransferInstant,
    deadline: ContextCookieTransferInstant,
}

impl ContextCookieTransferWindow {
    /// Validates one nonempty interval under the process hard ceiling.
    pub const fn try_new(
        requested_at: ContextCookieTransferInstant,
        deadline: ContextCookieTransferInstant,
    ) -> Result<Self, ContextCookieTransferError> {
        let Some(duration) = deadline.millis().checked_sub(requested_at.millis()) else {
            return Err(ContextCookieTransferError::Deadline);
        };
        if duration == 0 || duration > MAX_COOKIE_TRANSFER_MILLIS {
            return Err(ContextCookieTransferError::Deadline);
        }
        Ok(Self {
            requested_at,
            deadline,
        })
    }

    /// Exact trusted-shell request time.
    pub const fn requested_at(self) -> ContextCookieTransferInstant {
        self.requested_at
    }

    /// Sole absolute completion deadline in the same clock domain.
    pub const fn deadline(self) -> ContextCookieTransferInstant {
        self.deadline
    }

    /// Validated native lifetime in milliseconds.
    pub const fn duration_millis(self) -> u64 {
        self.deadline.millis() - self.requested_at.millis()
    }
}

impl fmt::Debug for ContextCookieTransferWindow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextCookieTransferWindow")
            .field("requested_at", &self.requested_at)
            .field("deadline", &self.deadline)
            .finish()
    }
}

/// Canonical HTTP(S) origin used only by the trusted native cookie adapter.
#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct ContextCookieOrigin(Url);

impl ContextCookieOrigin {
    /// Derives a canonical origin from one already-valid navigation target.
    pub fn from_target(
        target: &ContextNavigationTarget,
    ) -> Result<Self, ContextCookieTransferError> {
        Self::try_new(target.as_url().clone())
    }

    /// Parses a canonicalizable HTTP(S) origin or URL.
    pub fn parse(value: &str) -> Result<Self, ContextCookieTransferError> {
        let url = Url::parse(value).map_err(|_| ContextCookieTransferError::InvalidOrigin)?;
        Self::try_new(url)
    }

    /// Returns the canonical origin URL to the trusted native adapter.
    pub const fn as_url(&self) -> &Url {
        &self.0
    }

    fn try_new(mut url: Url) -> Result<Self, ContextCookieTransferError> {
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || !zephium_core::navigation::is_allowed(&url)
        {
            return Err(ContextCookieTransferError::InvalidOrigin);
        }
        url.set_path("/");
        url.set_query(None);
        url.set_fragment(None);
        Ok(Self(url))
    }
}

impl fmt::Debug for ContextCookieOrigin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ContextCookieOrigin([redacted])")
    }
}

/// Nonempty deduplicated bounded cohort of canonical cookie origins.
#[derive(Clone, Eq, PartialEq)]
pub struct ContextCookieScope {
    origins: Vec<ContextCookieOrigin>,
}

impl ContextCookieScope {
    /// Validates one complete origin cohort without adding implied origins.
    pub fn try_new(origins: Vec<ContextCookieOrigin>) -> Result<Self, ContextCookieTransferError> {
        if origins.is_empty() {
            return Err(ContextCookieTransferError::EmptyScope);
        }
        if origins.len() > MAX_COOKIE_TRANSFER_ORIGINS {
            return Err(ContextCookieTransferError::OriginLimit);
        }
        for (index, origin) in origins.iter().enumerate() {
            if origins[index + 1..].contains(origin) {
                return Err(ContextCookieTransferError::DuplicateOrigin);
            }
        }
        Ok(Self { origins })
    }

    /// Exact canonical origins in caller-specified order.
    pub fn origins(&self) -> &[ContextCookieOrigin] {
        &self.origins
    }

    /// Number of canonical origins in this cohort.
    pub fn len(&self) -> usize {
        self.origins.len()
    }

    /// Reports whether the cohort is empty.
    pub fn is_empty(&self) -> bool {
        self.origins.is_empty()
    }
}

impl fmt::Debug for ContextCookieScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextCookieScope")
            .field("origin_count", &self.origins.len())
            .finish()
    }
}

/// Nonzero process-local identity of one exact cookie transfer.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ContextCookieTransferId(NonZeroU64);

impl ContextCookieTransferId {
    /// Constructs a shell-minted nonzero transfer identity.
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the process-local correlation value.
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Debug for ContextCookieTransferId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ContextCookieTransferId([redacted])")
    }
}

/// Supported one-way cookie-transfer direction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextCookieTransferDirection {
    /// Selected normal Windows profile to its stable clean automation subprofile.
    SelectedProfileToOwned,
    /// Temporary human handoff to the stable clean automation subprofile.
    HumanHandoffToOwned,
}

/// Exact bounded request whose cookie values remain entirely native.
#[derive(Clone, Eq, PartialEq)]
pub struct ContextCookieTransferRequest {
    id: ContextCookieTransferId,
    window: ContextCookieTransferWindow,
    direction: ContextCookieTransferDirection,
    source: Option<ContextJoin>,
    source_profile_lease: Option<ContextProfileLease>,
    destination: ContextJoin,
    destination_profile_lease: ContextProfileLease,
    scope: ContextCookieScope,
}

impl ContextCookieTransferRequest {
    /// Builds an initial selected-profile-to-automation-subprofile import.
    pub fn selected_profile_to_owned(
        id: ContextCookieTransferId,
        window: ContextCookieTransferWindow,
        destination: ContextJoin,
        destination_capabilities: ContextCapabilities,
        destination_profile_lease: ContextProfileLease,
        destination_proof: ContextConstructionProof,
        scope: ContextCookieScope,
    ) -> Result<Self, ContextCookieTransferError> {
        validate_owned_destination(
            destination,
            destination_capabilities,
            destination_profile_lease,
            destination_proof,
        )?;
        Ok(Self {
            id,
            window,
            direction: ContextCookieTransferDirection::SelectedProfileToOwned,
            source: None,
            source_profile_lease: None,
            destination,
            destination_profile_lease,
            scope,
        })
    }

    /// Builds a post-authentication handoff-to-automation-subprofile import.
    #[allow(clippy::too_many_arguments)]
    pub fn human_handoff_to_owned(
        id: ContextCookieTransferId,
        window: ContextCookieTransferWindow,
        source: ContextJoin,
        source_capabilities: ContextCapabilities,
        source_profile_lease: ContextProfileLease,
        source_proof: ContextConstructionProof,
        destination: ContextJoin,
        destination_capabilities: ContextCapabilities,
        destination_profile_lease: ContextProfileLease,
        destination_proof: ContextConstructionProof,
        scope: ContextCookieScope,
    ) -> Result<Self, ContextCookieTransferError> {
        if source.identity().kind() != ContextKind::HumanSignInHandoff {
            return Err(ContextCookieTransferError::ContextKind);
        }
        if source_capabilities.kind() != ContextKind::HumanSignInHandoff
            || !source_capabilities.contains(ContextCapability::ExportCookies)
        {
            return Err(ContextCookieTransferError::CapabilityUnavailable);
        }
        if source_profile_lease.identity() != source.identity() {
            return Err(ContextCookieTransferError::ProfileLease);
        }
        if source_proof != ContextConstructionProof::WindowsHumanSignInHandoffNormalExtensions {
            return Err(ContextCookieTransferError::ConstructionProof);
        }
        validate_owned_destination(
            destination,
            destination_capabilities,
            destination_profile_lease,
            destination_proof,
        )?;
        if source.identity().id() == destination.identity().id() {
            return Err(ContextCookieTransferError::SameContext);
        }
        if source.identity().owner() != destination.identity().owner() {
            return Err(ContextCookieTransferError::OwnerMismatch);
        }
        if source.identity().profile() != destination.identity().profile() {
            return Err(ContextCookieTransferError::ProfileMismatch);
        }
        Ok(Self {
            id,
            window,
            direction: ContextCookieTransferDirection::HumanHandoffToOwned,
            source: Some(source),
            source_profile_lease: Some(source_profile_lease),
            destination,
            destination_profile_lease,
            scope,
        })
    }

    /// Exact transfer correlation identity.
    pub const fn id(&self) -> ContextCookieTransferId {
        self.id
    }

    /// Exact bounded native execution window.
    pub const fn window(&self) -> ContextCookieTransferWindow {
        self.window
    }

    /// Closed one-way transfer direction.
    pub const fn direction(&self) -> ContextCookieTransferDirection {
        self.direction
    }

    /// Exact handoff source, absent for selected-profile import.
    pub const fn source(&self) -> Option<ContextJoin> {
        self.source
    }

    /// Exact retained source lease, absent for selected-profile import.
    pub const fn source_profile_lease(&self) -> Option<ContextProfileLease> {
        self.source_profile_lease
    }

    /// Exact clean owned destination.
    pub const fn destination(&self) -> ContextJoin {
        self.destination
    }

    /// Exact retained destination profile lease.
    pub const fn destination_profile_lease(&self) -> ContextProfileLease {
        self.destination_profile_lease
    }

    /// Complete bounded canonical origin scope.
    pub const fn scope(&self) -> &ContextCookieScope {
        &self.scope
    }

    fn contains_context(&self, context: ContextId) -> bool {
        self.destination.identity().id() == context
            || self
                .source
                .is_some_and(|source| source.identity().id() == context)
    }
}

impl fmt::Debug for ContextCookieTransferRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextCookieTransferRequest")
            .field("id", &self.id)
            .field("window", &self.window)
            .field("direction", &self.direction)
            .field("source", &self.source)
            .field("source_profile_lease", &self.source_profile_lease)
            .field("destination", &self.destination)
            .field("destination_profile_lease", &self.destination_profile_lease)
            .field("scope", &self.scope)
            .finish()
    }
}

/// Closed native failure class for one cookie-transfer phase.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ContextCookieTransferFailure {
    /// Source profile/context was unavailable or stale.
    #[error("cookie transfer source is unavailable")]
    SourceUnavailable,
    /// Destination context was unavailable or stale.
    #[error("cookie transfer destination is unavailable")]
    DestinationUnavailable,
    /// Native cookie enumeration failed before application.
    #[error("cookie enumeration failed")]
    EnumerationFailed,
    /// Native data exceeded a fixed cookie, field, or payload ceiling.
    #[error("cookie transfer native data exceeded its ceiling")]
    LimitExceeded,
    /// A native-exposed cookie could not be represented exactly.
    #[error("cookie transfer encountered an invalid native cookie")]
    InvalidCookie,
    /// Destination rejected at least one cookie application.
    #[error("cookie application failed")]
    ApplicationFailed,
    /// The bounded native deadline elapsed.
    #[error("cookie transfer timed out")]
    TimedOut,
    /// Cancellation terminated the native transfer.
    #[error("cookie transfer was cancelled")]
    Cancelled,
    /// Process teardown permanently sealed the native adapter.
    #[error("cookie transfer adapter is shutting down")]
    Shutdown,
}

/// Privacy-preserving native cookie accounting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContextCookieTransferCounts {
    /// Canonical origins completely enumerated.
    pub origins_completed: u8,
    /// Unique native-exposed cookies observed after cross-origin deduplication.
    pub cookies_observed: u16,
    /// Unique cookies applied to the clean destination.
    pub cookies_applied: u16,
    /// Observed cookies carrying the native HttpOnly flag.
    pub http_only_observed: u16,
    /// HttpOnly cookies applied with that flag preserved.
    pub http_only_applied: u16,
    /// UTF-8 bytes across validated native-exposed cookie fields.
    pub payload_bytes: u32,
}

/// Validated cookie-transfer counts without names, values, domains, or paths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContextCookieTransferStats(ContextCookieTransferCounts);

impl ContextCookieTransferStats {
    /// Validates native counts against fixed transfer ceilings.
    pub fn try_new(
        counts: ContextCookieTransferCounts,
    ) -> Result<Self, ContextCookieTransferError> {
        if usize::from(counts.origins_completed) > MAX_COOKIE_TRANSFER_ORIGINS
            || usize::from(counts.cookies_observed) > MAX_COOKIES_PER_TRANSFER
            || usize::from(counts.cookies_applied) > MAX_COOKIES_PER_TRANSFER
            || usize::try_from(counts.payload_bytes)
                .map_or(true, |bytes| bytes > MAX_COOKIE_TRANSFER_BYTES)
        {
            return Err(ContextCookieTransferError::ResultLimit);
        }
        if counts.cookies_applied > counts.cookies_observed
            || counts.http_only_observed > counts.cookies_observed
            || counts.http_only_applied > counts.http_only_observed
            || counts.http_only_applied > counts.cookies_applied
        {
            return Err(ContextCookieTransferError::ResultInvariant);
        }
        Ok(Self(counts))
    }

    /// Complete validated native count cohort.
    pub const fn counts(self) -> ContextCookieTransferCounts {
        self.0
    }
}

/// Terminal native cookie-transfer outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextCookieTransferOutcome {
    /// Every enumerated cookie was applied with its exposed attributes preserved.
    Applied(ContextCookieTransferStats),
    /// Native work failed before any destination mutation.
    Refused(ContextCookieTransferFailure),
    /// At least one destination cookie may have applied before terminal failure.
    /// The destination must be destroyed/recreated rather than navigated.
    Partial {
        /// Terminal native failure class.
        failure: ContextCookieTransferFailure,
        /// Counts observed at the failure boundary.
        stats: ContextCookieTransferStats,
    },
}

/// Exact asynchronous settlement of one admitted cookie transfer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextCookieTransferSettlement {
    request: ContextCookieTransferRequest,
    outcome: ContextCookieTransferOutcome,
}

impl ContextCookieTransferSettlement {
    /// Validates success/partial semantics against the exact requested scope.
    pub fn try_new(
        request: ContextCookieTransferRequest,
        outcome: ContextCookieTransferOutcome,
    ) -> Result<Self, ContextCookieTransferError> {
        let counts = match outcome {
            ContextCookieTransferOutcome::Applied(stats) => {
                let counts = stats.counts();
                if usize::from(counts.origins_completed) != request.scope.len()
                    || counts.cookies_applied != counts.cookies_observed
                    || counts.http_only_applied != counts.http_only_observed
                {
                    return Err(ContextCookieTransferError::ResultInvariant);
                }
                counts
            }
            ContextCookieTransferOutcome::Refused(_) => {
                return Ok(Self { request, outcome });
            }
            ContextCookieTransferOutcome::Partial { stats, .. } => {
                let counts = stats.counts();
                if counts.cookies_applied == 0
                    || usize::from(counts.origins_completed) > request.scope.len()
                {
                    return Err(ContextCookieTransferError::ResultInvariant);
                }
                counts
            }
        };
        if usize::from(counts.origins_completed) > request.scope.len() {
            return Err(ContextCookieTransferError::ResultInvariant);
        }
        Ok(Self { request, outcome })
    }

    /// Exact admitted request echoed by the native adapter.
    pub const fn request(&self) -> &ContextCookieTransferRequest {
        &self.request
    }

    /// Validated terminal native outcome.
    pub const fn outcome(&self) -> ContextCookieTransferOutcome {
        self.outcome
    }
}

/// Privacy-preserving concurrent transfer counts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContextCookieTransferRegistryStatus {
    pending: u8,
    shutdown_sealed: bool,
}

impl ContextCookieTransferRegistryStatus {
    /// Exact number of admitted transfers awaiting terminal settlement.
    pub const fn pending(self) -> u8 {
        self.pending
    }

    /// Whether shutdown permanently rejected new transfers.
    pub const fn shutdown_sealed(self) -> bool {
        self.shutdown_sealed
    }
}

/// Bounded single-owner registry for cookie-transfer correlation and shutdown.
#[derive(Default)]
pub struct ContextCookieTransferRegistry {
    pending: BTreeMap<ContextCookieTransferId, ContextCookieTransferRequest>,
    shutdown_sealed: bool,
}

impl ContextCookieTransferRegistry {
    /// Creates an empty registry with no worker, timer, native object, or I/O.
    pub const fn new() -> Self {
        Self {
            pending: BTreeMap::new(),
            shutdown_sealed: false,
        }
    }

    /// Admits one transfer without eviction or implicit retry.
    pub fn admit(
        &mut self,
        request: ContextCookieTransferRequest,
    ) -> Result<(), ContextCookieTransferError> {
        self.validate()?;
        if self.shutdown_sealed {
            return Err(ContextCookieTransferError::ShutdownSealed);
        }
        if self.pending.contains_key(&request.id) {
            return Err(ContextCookieTransferError::DuplicateTransfer);
        }
        if self.pending.values().any(|active| {
            active.destination.identity().profile() == request.destination.identity().profile()
        }) {
            return Err(ContextCookieTransferError::DestinationBusy);
        }
        if self.pending.len() >= MAX_PENDING_COOKIE_TRANSFERS {
            return Err(ContextCookieTransferError::TransferLimit);
        }
        self.pending.insert(request.id, request);
        self.validate()
    }

    /// Settles and removes only the exact admitted request.
    pub fn settle(
        &mut self,
        settlement: ContextCookieTransferSettlement,
    ) -> Result<ContextCookieTransferOutcome, ContextCookieTransferError> {
        self.validate()?;
        let id = settlement.request.id;
        let retained = self
            .pending
            .get(&id)
            .ok_or(ContextCookieTransferError::NotFound)?;
        if retained != &settlement.request {
            return Err(ContextCookieTransferError::StaleSettlement);
        }
        self.pending
            .remove(&id)
            .ok_or(ContextCookieTransferError::Invariant)?;
        self.validate()?;
        Ok(settlement.outcome)
    }

    /// Returns exact pending requests joined to one context in stable id order.
    pub fn transfers_for_context(&self, context: ContextId) -> Vec<ContextCookieTransferRequest> {
        self.pending
            .values()
            .filter(|request| request.contains_context(context))
            .cloned()
            .collect()
    }

    /// Permanently rejects admissions and returns the exact cancellation cohort.
    pub fn seal_for_shutdown(
        &mut self,
    ) -> Result<Vec<ContextCookieTransferRequest>, ContextCookieTransferError> {
        self.validate()?;
        self.shutdown_sealed = true;
        let pending = self.pending.values().cloned().collect();
        self.validate()?;
        Ok(pending)
    }

    /// Returns bounded counts without origins, profiles, or context identities.
    pub fn status(&self) -> ContextCookieTransferRegistryStatus {
        ContextCookieTransferRegistryStatus {
            pending: u8::try_from(self.pending.len()).unwrap_or(u8::MAX),
            shutdown_sealed: self.shutdown_sealed,
        }
    }

    /// True only after shutdown seal and exact terminal settlement of all transfers.
    pub fn is_quiescent(&self) -> bool {
        self.shutdown_sealed && self.pending.is_empty()
    }

    fn validate(&self) -> Result<(), ContextCookieTransferError> {
        if self.pending.len() > MAX_PENDING_COOKIE_TRANSFERS {
            return Err(ContextCookieTransferError::TransferLimit);
        }
        for (id, request) in &self.pending {
            if *id != request.id {
                return Err(ContextCookieTransferError::Invariant);
            }
        }
        for (index, request) in self.pending.values().enumerate() {
            if self.pending.values().skip(index + 1).any(|candidate| {
                candidate.destination.identity().profile()
                    == request.destination.identity().profile()
            }) {
                return Err(ContextCookieTransferError::Invariant);
            }
        }
        Ok(())
    }
}

fn validate_owned_destination(
    destination: ContextJoin,
    capabilities: ContextCapabilities,
    profile_lease: ContextProfileLease,
    proof: ContextConstructionProof,
) -> Result<(), ContextCookieTransferError> {
    if destination.identity().kind() != ContextKind::Owned {
        return Err(ContextCookieTransferError::ContextKind);
    }
    if capabilities.kind() != ContextKind::Owned
        || !capabilities.contains(ContextCapability::ImportCookies)
    {
        return Err(ContextCookieTransferError::CapabilityUnavailable);
    }
    if profile_lease.identity() != destination.identity() {
        return Err(ContextCookieTransferError::ProfileLease);
    }
    if proof != ContextConstructionProof::WindowsOwnedAutomationSubprofileEmptyInventory {
        return Err(ContextCookieTransferError::ConstructionProof);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ContextId, ContextIdentity, ContextOperationId, ContextProfileLeaseId,
        ContextProfileLeasePurpose, ContextProfileLeaseRegistry, ContextProfileStorageClass,
        ContextRegistry, ContextRunId, ContextSettlement,
    };
    use zephium_core::ids::ProfileId;

    const OWNED_CAPABILITIES: [ContextCapability; 2] = [
        ContextCapability::Navigate,
        ContextCapability::ImportCookies,
    ];
    const HANDOFF_CAPABILITIES: [ContextCapability; 2] = [
        ContextCapability::HumanControl,
        ContextCapability::ExportCookies,
    ];

    fn capabilities(kind: ContextKind) -> ContextCapabilities {
        let values = match kind {
            ContextKind::Owned => &OWNED_CAPABILITIES[..],
            ContextKind::HumanSignInHandoff => &HANDOFF_CAPABILITIES[..],
            ContextKind::BorrowedTab => &[][..],
        };
        ContextCapabilities::try_new(kind, values).expect("capabilities")
    }

    fn ready_join(context: u128, owner: u128, profile: u128, kind: ContextKind) -> ContextJoin {
        let identity = ContextIdentity::new(
            ContextId::from_raw(context),
            ContextRunId::from_raw(owner),
            ProfileId::from(profile),
            kind,
        );
        let mut registry = ContextRegistry::new();
        registry
            .reserve(identity, capabilities(kind))
            .expect("reserve");
        let construction = registry
            .begin_context(
                identity.id(),
                ContextOperationId::new(1).expect("operation"),
            )
            .expect("begin");
        registry
            .settle_construction(identity.id(), construction, ContextSettlement::Applied)
            .expect("settle");
        registry.join(identity.id()).expect("join")
    }

    fn profile_lease(join: ContextJoin, value: u64) -> ContextProfileLease {
        let purpose = match join.identity().kind() {
            ContextKind::Owned => ContextProfileLeasePurpose::Owned,
            ContextKind::BorrowedTab => ContextProfileLeasePurpose::BorrowedTab,
            ContextKind::HumanSignInHandoff => ContextProfileLeasePurpose::HumanSignInHandoff,
        };
        ContextProfileLeaseRegistry::new()
            .acquire(
                ContextProfileLeaseId::new(value).expect("lease id"),
                join.identity(),
                ContextProfileStorageClass::Durable,
                purpose,
            )
            .expect("lease")
    }

    fn scope() -> ContextCookieScope {
        ContextCookieScope::try_new(vec![ContextCookieOrigin::parse(
            "https://example.test/login?secret=value",
        )
        .expect("origin")])
        .expect("scope")
    }

    fn window() -> ContextCookieTransferWindow {
        ContextCookieTransferWindow::try_new(
            ContextCookieTransferInstant::from_millis(1_000),
            ContextCookieTransferInstant::from_millis(11_000),
        )
        .expect("window")
    }

    fn selected_request_for_profile(
        value: u64,
        context: u128,
        profile: u128,
    ) -> ContextCookieTransferRequest {
        let destination = ready_join(context, 50, profile, ContextKind::Owned);
        ContextCookieTransferRequest::selected_profile_to_owned(
            ContextCookieTransferId::new(value).expect("transfer"),
            window(),
            destination,
            capabilities(ContextKind::Owned),
            profile_lease(destination, value),
            ContextConstructionProof::WindowsOwnedAutomationSubprofileEmptyInventory,
            scope(),
        )
        .expect("request")
    }

    fn selected_request(value: u64, context: u128) -> ContextCookieTransferRequest {
        selected_request_for_profile(value, context, 60)
    }

    fn stats(origins: u8, observed: u16, applied: u16) -> ContextCookieTransferStats {
        ContextCookieTransferStats::try_new(ContextCookieTransferCounts {
            origins_completed: origins,
            cookies_observed: observed,
            cookies_applied: applied,
            http_only_observed: observed.min(1),
            http_only_applied: applied.min(1),
            payload_bytes: u32::from(observed) * 128,
        })
        .expect("stats")
    }

    #[test]
    fn origins_are_canonical_deduplicated_bounded_and_debug_redacted() {
        let first = ContextCookieOrigin::parse("https://example.test/a?q=secret").expect("origin");
        let same = ContextCookieOrigin::parse("https://example.test/b").expect("same origin");
        assert_eq!(first, same);
        assert_eq!(first.as_url().as_str(), "https://example.test/");
        assert_eq!(format!("{first:?}"), "ContextCookieOrigin([redacted])");
        assert_eq!(
            ContextCookieScope::try_new(vec![first, same]),
            Err(ContextCookieTransferError::DuplicateOrigin)
        );
        assert_eq!(
            ContextCookieOrigin::parse("about:blank"),
            Err(ContextCookieTransferError::InvalidOrigin)
        );
        assert_eq!(
            ContextCookieOrigin::parse("https://user:secret@example.test/"),
            Err(ContextCookieTransferError::InvalidOrigin)
        );
    }

    #[test]
    fn selected_profile_import_requires_clean_windows_subprofile_proof() {
        let destination = ready_join(1, 50, 60, ContextKind::Owned);
        assert_eq!(
            ContextCookieTransferRequest::selected_profile_to_owned(
                ContextCookieTransferId::new(1).expect("transfer"),
                window(),
                destination,
                capabilities(ContextKind::Owned),
                profile_lease(destination, 1),
                ContextConstructionProof::WindowsOwnedSelectedProfileEmptyInventory,
                scope(),
            ),
            Err(ContextCookieTransferError::ConstructionProof)
        );
    }

    #[test]
    fn handoff_requires_same_owner_profile_and_exact_capabilities() {
        let source = ready_join(1, 50, 60, ContextKind::HumanSignInHandoff);
        let destination = ready_join(2, 50, 61, ContextKind::Owned);
        assert_eq!(
            ContextCookieTransferRequest::human_handoff_to_owned(
                ContextCookieTransferId::new(1).expect("transfer"),
                window(),
                source,
                capabilities(ContextKind::HumanSignInHandoff),
                profile_lease(source, 1),
                ContextConstructionProof::WindowsHumanSignInHandoffNormalExtensions,
                destination,
                capabilities(ContextKind::Owned),
                profile_lease(destination, 2),
                ContextConstructionProof::WindowsOwnedAutomationSubprofileEmptyInventory,
                scope(),
            ),
            Err(ContextCookieTransferError::ProfileMismatch)
        );
    }

    #[test]
    fn native_window_is_nonempty_bounded_exact_and_redacted() {
        let requested_at = ContextCookieTransferInstant::from_millis(50);
        let deadline = ContextCookieTransferInstant::from_millis(50 + MAX_COOKIE_TRANSFER_MILLIS);
        let window =
            ContextCookieTransferWindow::try_new(requested_at, deadline).expect("maximum window");
        assert_eq!(window.requested_at(), requested_at);
        assert_eq!(window.deadline(), deadline);
        assert_eq!(window.duration_millis(), MAX_COOKIE_TRANSFER_MILLIS);
        assert_eq!(
            format!("{window:?}"),
            "ContextCookieTransferWindow { requested_at: ContextCookieTransferInstant([redacted]), deadline: ContextCookieTransferInstant([redacted]) }"
        );
        assert_eq!(
            ContextCookieTransferWindow::try_new(requested_at, requested_at),
            Err(ContextCookieTransferError::Deadline)
        );
        assert_eq!(
            ContextCookieTransferWindow::try_new(
                requested_at,
                ContextCookieTransferInstant::from_millis(
                    requested_at.millis() + MAX_COOKIE_TRANSFER_MILLIS + 1,
                ),
            ),
            Err(ContextCookieTransferError::Deadline)
        );
        assert_eq!(
            ContextCookieTransferWindow::try_new(
                requested_at,
                ContextCookieTransferInstant::from_millis(requested_at.millis() - 1),
            ),
            Err(ContextCookieTransferError::Deadline)
        );
    }

    #[test]
    fn settlement_rejoins_the_exact_native_window() {
        let request = selected_request(1, 1);
        let mut substituted = request.clone();
        substituted.window = ContextCookieTransferWindow::try_new(
            ContextCookieTransferInstant::from_millis(2_000),
            ContextCookieTransferInstant::from_millis(12_000),
        )
        .expect("substituted window");
        let mut registry = ContextCookieTransferRegistry::new();
        registry.admit(request.clone()).expect("admit");
        let stale = ContextCookieTransferSettlement::try_new(
            substituted,
            ContextCookieTransferOutcome::Refused(ContextCookieTransferFailure::Cancelled),
        )
        .expect("stale settlement");
        assert_eq!(
            registry.settle(stale),
            Err(ContextCookieTransferError::StaleSettlement)
        );
        assert_eq!(registry.status().pending(), 1);
        let exact = ContextCookieTransferSettlement::try_new(
            request.clone(),
            ContextCookieTransferOutcome::Refused(ContextCookieTransferFailure::Cancelled),
        )
        .expect("exact settlement");
        assert_eq!(
            registry.settle(exact),
            Ok(ContextCookieTransferOutcome::Refused(
                ContextCookieTransferFailure::Cancelled
            ))
        );
        assert_eq!(request.window(), window());
    }

    #[test]
    fn applied_and_partial_outcomes_are_mechanically_distinct() {
        let request = selected_request(1, 1);
        assert!(ContextCookieTransferSettlement::try_new(
            request.clone(),
            ContextCookieTransferOutcome::Applied(stats(1, 3, 3)),
        )
        .is_ok());
        assert_eq!(
            ContextCookieTransferSettlement::try_new(
                request.clone(),
                ContextCookieTransferOutcome::Applied(stats(1, 3, 2)),
            ),
            Err(ContextCookieTransferError::ResultInvariant)
        );
        assert_eq!(
            ContextCookieTransferSettlement::try_new(
                request,
                ContextCookieTransferOutcome::Partial {
                    failure: ContextCookieTransferFailure::ApplicationFailed,
                    stats: stats(0, 3, 0),
                },
            ),
            Err(ContextCookieTransferError::ResultInvariant)
        );
    }

    #[test]
    fn native_counts_reject_payload_and_http_only_contradictions() {
        assert_eq!(
            ContextCookieTransferStats::try_new(ContextCookieTransferCounts {
                origins_completed: 1,
                cookies_observed: 1,
                cookies_applied: 1,
                http_only_observed: 0,
                http_only_applied: 1,
                payload_bytes: 10,
            }),
            Err(ContextCookieTransferError::ResultInvariant)
        );
        assert_eq!(
            ContextCookieTransferStats::try_new(ContextCookieTransferCounts {
                origins_completed: 1,
                cookies_observed: 1,
                cookies_applied: 1,
                http_only_observed: 1,
                http_only_applied: 1,
                payload_bytes: u32::try_from(MAX_COOKIE_TRANSFER_BYTES + 1)
                    .expect("bounded constant"),
            }),
            Err(ContextCookieTransferError::ResultLimit)
        );
    }

    #[test]
    fn registry_bounds_destination_profiles_and_exact_settlement_without_eviction() {
        let mut registry = ContextCookieTransferRegistry::new();
        let first = selected_request(1, 1);
        registry.admit(first.clone()).expect("first");
        assert_eq!(
            registry.admit(first.clone()),
            Err(ContextCookieTransferError::DuplicateTransfer)
        );
        let same_destination = ContextCookieTransferRequest {
            id: ContextCookieTransferId::new(2).expect("transfer"),
            ..first.clone()
        };
        assert_eq!(
            registry.admit(same_destination),
            Err(ContextCookieTransferError::DestinationBusy)
        );
        assert_eq!(
            registry.admit(selected_request(2, 2)),
            Err(ContextCookieTransferError::DestinationBusy)
        );
        registry
            .admit(selected_request_for_profile(2, 2, 61))
            .expect("second destination profile");
        assert_eq!(
            registry.admit(selected_request_for_profile(3, 3, 62)),
            Err(ContextCookieTransferError::TransferLimit)
        );
        let settlement = ContextCookieTransferSettlement::try_new(
            first,
            ContextCookieTransferOutcome::Applied(stats(1, 2, 2)),
        )
        .expect("settlement");
        assert_eq!(
            registry.settle(settlement),
            Ok(ContextCookieTransferOutcome::Applied(stats(1, 2, 2)))
        );
        assert_eq!(registry.status().pending(), 1);
    }

    #[test]
    fn shutdown_seal_retains_pending_transfers_until_terminal_callbacks() {
        let mut registry = ContextCookieTransferRegistry::new();
        let request = selected_request(1, 1);
        registry.admit(request.clone()).expect("admit");
        assert_eq!(
            registry.seal_for_shutdown().expect("seal"),
            vec![request.clone()]
        );
        assert!(!registry.is_quiescent());
        assert_eq!(
            registry.admit(selected_request(2, 2)),
            Err(ContextCookieTransferError::ShutdownSealed)
        );
        let settlement = ContextCookieTransferSettlement::try_new(
            request,
            ContextCookieTransferOutcome::Refused(ContextCookieTransferFailure::Cancelled),
        )
        .expect("settlement");
        registry.settle(settlement).expect("settle");
        assert!(registry.is_quiescent());
    }

    #[test]
    fn debug_output_never_contains_origins_profiles_or_transfer_ids() {
        let request = selected_request(9001, 1);
        let debug = format!("{request:?}");
        assert!(debug.contains("[redacted]"));
        assert!(!debug.contains("example.test"));
        assert!(!debug.contains("9001"));
        assert!(!debug.contains("ProfileId"));
    }
}
