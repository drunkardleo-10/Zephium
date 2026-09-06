//! Pure identity and lifecycle core for agent-owned browser contexts.
//!
//! This module contains no browser engine, native object, page data, URL,
//! profile path, timer, worker, or background task. Native adapters receive
//! exact joins and return exact settlements; the application shell remains the
//! sole owner of mutation and side effects.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;
use ulid::Ulid;
use zephium_core::ids::ProfileId;

const MAX_CONTEXT_CAPABILITIES: usize = 11;

macro_rules! durable_id {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(Ulid);

        impl $name {
            /// Mints a new identity at a trusted shell edge.
            pub fn generate() -> Self {
                Self(Ulid::new())
            }

            /// Parses one canonical ULID and rejects every other spelling.
            pub fn parse(value: &str) -> Option<Self> {
                let parsed = Ulid::from_string(value).ok()?;
                (parsed.to_string() == value).then_some(Self(parsed))
            }

            /// Returns stable big-endian identity bytes for durable joins.
            pub const fn bytes(self) -> [u8; 16] {
                self.0 .0.to_be_bytes()
            }

            #[cfg(test)]
            pub(crate) const fn from_raw(value: u128) -> Self {
                Self(Ulid(value))
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(concat!(stringify!($name), "([redacted])"))
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.0.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(&value)
                    .ok_or_else(|| serde::de::Error::custom(concat!("invalid ", stringify!($name))))
            }
        }
    };
}
pub(crate) use durable_id;

durable_id!(
    ContextId,
    "Durable opaque identity of one agent-browser context."
);
durable_id!(
    ContextRunId,
    "Durable opaque identity of the exact run that owns a context."
);

macro_rules! nonzero_generation {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(u64);

        impl $name {
            /// First legal generation.
            pub const INITIAL: Self = Self(1);

            /// Constructs a nonzero generation.
            pub const fn new(value: u64) -> Option<Self> {
                if value == 0 {
                    None
                } else {
                    Some(Self(value))
                }
            }

            /// Returns the process-local numeric value for exact correlation.
            pub const fn get(self) -> u64 {
                self.0
            }

            /// Advances without wrapping.
            pub const fn next(self) -> Option<Self> {
                match self.0.checked_add(1) {
                    Some(value) => Some(Self(value)),
                    None => None,
                }
            }
        }
    };
}

nonzero_generation!(
    ContextGeneration,
    "Non-reusable incarnation of one logical context's native and agent authority."
);
nonzero_generation!(
    NavigationEpoch,
    "Monotonic identity of the current main-frame navigation epoch."
);
nonzero_generation!(
    FrameGeneration,
    "Monotonic generation of one frame's current document."
);
nonzero_generation!(
    RunCancellationGeneration,
    "Monotonic generation of the run-cancellation authority joined to a context."
);
nonzero_generation!(
    ContextOperationId,
    "Nonzero shell-minted correlation identity for one context operation."
);

/// Opaque frame identity used only for exact native-to-domain joins.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FrameId(u64);

impl FrameId {
    /// Reserved identity for the context's main frame.
    pub const MAIN: Self = Self(1);

    /// Constructs a nonzero frame identity.
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 {
            None
        } else {
            Some(Self(value))
        }
    }

    /// Returns the process-local numeric value for native correlation.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// How a context participates in ordinary Browse identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ContextKind {
    /// Run-owned context absent from tab/session and extension inventory.
    Owned,
    /// Existing normal tab held under an explicit revocable agent lease.
    BorrowedTab,
    /// Temporary normal human-controlled sign-in handoff context.
    HumanSignInHandoff,
}

/// Closed native capabilities negotiated for one exact context.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ContextCapability {
    /// Commit browser navigation.
    Navigate,
    /// Produce bounded semantic observations.
    Observe,
    /// Execute fixed semantic actions.
    Act,
    /// Show or hide the exact native page surface.
    Present,
    /// Suspend and resume the native page.
    Suspend,
    /// Reconstruct a native page after renderer loss.
    Recover,
    /// Transfer input exclusively to and from a person.
    HumanControl,
    /// Explicitly adopt an owned context into ordinary Browse.
    Adopt,
    /// Release a borrowed or handoff context without adopting it.
    Release,
    /// Import a bounded origin cookie cohort through a platform API.
    ImportCookies,
    /// Export a bounded origin cookie cohort through a platform API.
    ExportCookies,
}

impl ContextCapability {
    const fn bit(self) -> u16 {
        1 << (self as u16)
    }

    const fn allowed_for(self, kind: ContextKind) -> bool {
        match kind {
            ContextKind::Owned => matches!(
                self,
                Self::Navigate
                    | Self::Observe
                    | Self::Act
                    | Self::Present
                    | Self::Suspend
                    | Self::Recover
                    | Self::HumanControl
                    | Self::Adopt
                    | Self::ImportCookies
            ),
            ContextKind::BorrowedTab => matches!(
                self,
                Self::Navigate
                    | Self::Observe
                    | Self::Act
                    | Self::Present
                    | Self::Suspend
                    | Self::Recover
                    | Self::HumanControl
                    | Self::Release
            ),
            ContextKind::HumanSignInHandoff => matches!(
                self,
                Self::Navigate
                    | Self::Present
                    | Self::Recover
                    | Self::HumanControl
                    | Self::Release
                    | Self::ExportCookies
            ),
        }
    }
}

/// Failure to construct a truthful, kind-scoped capability set.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ContextCapabilityError {
    /// The same capability appeared more than once.
    #[error("duplicate context capability")]
    Duplicate,
    /// The capability is forbidden for this context kind.
    #[error("context capability is incompatible with context kind")]
    IncompatibleKind,
    /// The fixed capability ceiling was exceeded.
    #[error("context capability ceiling exceeded")]
    Limit,
}

/// Compact, non-authorizing capability inventory for one context.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ContextCapabilities {
    bits: u16,
    kind: ContextKind,
}

impl ContextCapabilities {
    /// Validates a complete capability inventory for `kind`.
    pub fn try_new(
        kind: ContextKind,
        capabilities: &[ContextCapability],
    ) -> Result<Self, ContextCapabilityError> {
        if capabilities.len() > MAX_CONTEXT_CAPABILITIES {
            return Err(ContextCapabilityError::Limit);
        }
        let mut bits = 0_u16;
        for capability in capabilities {
            if !capability.allowed_for(kind) {
                return Err(ContextCapabilityError::IncompatibleKind);
            }
            let bit = capability.bit();
            if bits & bit != 0 {
                return Err(ContextCapabilityError::Duplicate);
            }
            bits |= bit;
        }
        Ok(Self { bits, kind })
    }

    /// Reports whether this inventory contains `capability`.
    pub const fn contains(self, capability: ContextCapability) -> bool {
        self.bits & capability.bit() != 0
    }

    /// Returns the number of represented capabilities.
    pub const fn len(self) -> u32 {
        self.bits.count_ones()
    }

    /// Reports whether no capability is present.
    pub const fn is_empty(self) -> bool {
        self.bits == 0
    }

    /// Context kind against which this complete inventory was validated.
    pub const fn kind(self) -> ContextKind {
        self.kind
    }
}

impl fmt::Debug for ContextCapabilities {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextCapabilities")
            .field("kind", &self.kind)
            .field("count", &self.len())
            .finish()
    }
}

/// Immutable identity bound to one context for its complete lifetime.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ContextIdentity {
    id: ContextId,
    owner: ContextRunId,
    profile: ProfileId,
    kind: ContextKind,
}

impl ContextIdentity {
    /// Binds a context to one run, selected profile, and identity kind.
    pub const fn new(
        id: ContextId,
        owner: ContextRunId,
        profile: ProfileId,
        kind: ContextKind,
    ) -> Self {
        Self {
            id,
            owner,
            profile,
            kind,
        }
    }

    /// Durable context identity.
    pub const fn id(self) -> ContextId {
        self.id
    }

    /// Exact owning run.
    pub const fn owner(self) -> ContextRunId {
        self.owner
    }

    /// Exact selected browser profile.
    pub const fn profile(self) -> ProfileId {
        self.profile
    }

    /// Context identity kind.
    pub const fn kind(self) -> ContextKind {
        self.kind
    }
}

impl fmt::Debug for ContextIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextIdentity")
            .field("id", &self.id)
            .field("owner", &self.owner)
            .field("profile", &"[redacted]")
            .field("kind", &self.kind)
            .finish()
    }
}

/// Exact context/document/cancellation coordinates required by every result.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ContextJoin {
    identity: ContextIdentity,
    context_generation: ContextGeneration,
    navigation_epoch: NavigationEpoch,
    frame: FrameId,
    frame_generation: FrameGeneration,
    cancellation_generation: RunCancellationGeneration,
}

impl ContextJoin {
    /// Immutable context identity.
    pub const fn identity(self) -> ContextIdentity {
        self.identity
    }

    /// Exact context incarnation.
    pub const fn context_generation(self) -> ContextGeneration {
        self.context_generation
    }

    /// Exact main-frame navigation epoch.
    pub const fn navigation_epoch(self) -> NavigationEpoch {
        self.navigation_epoch
    }

    /// Exact frame identity.
    pub const fn frame(self) -> FrameId {
        self.frame
    }

    /// Exact frame document generation.
    pub const fn frame_generation(self) -> FrameGeneration {
        self.frame_generation
    }

    /// Exact run-cancellation generation.
    pub const fn cancellation_generation(self) -> RunCancellationGeneration {
        self.cancellation_generation
    }
}

impl fmt::Debug for ContextJoin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextJoin")
            .field("identity", &self.identity)
            .field("context_generation", &self.context_generation)
            .field("navigation_epoch", &self.navigation_epoch)
            .field("frame", &self.frame)
            .field("frame_generation", &self.frame_generation)
            .field("cancellation_generation", &self.cancellation_generation)
            .finish()
    }
}

/// Lifecycle operation whose native result must settle exactly once.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ContextOperationKind {
    /// Construct the initial native context.
    Construct,
    /// Commit one exact navigation.
    Navigate,
    /// Suspend the native context.
    Suspend,
    /// Resume a suspended native context.
    Resume,
    /// Recover after renderer loss.
    Recover,
    /// Present the exact native page for inspection.
    Show,
    /// Hide the exact native page.
    Hide,
    /// Transfer exclusive input to a person.
    BeginHumanControl,
    /// Return exclusive input to the agent.
    EndHumanControl,
    /// Close and destroy an owned context.
    Close,
    /// Adopt an owned context into ordinary Browse.
    Adopt,
    /// Release a borrowed or handoff context.
    Release,
}

/// Operation correlation plus every required context join coordinate.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ContextOperationJoin {
    operation: ContextOperationId,
    kind: ContextOperationKind,
    context: ContextJoin,
}

impl ContextOperationJoin {
    /// Shell-minted operation correlation identity.
    pub const fn operation(self) -> ContextOperationId {
        self.operation
    }

    /// Closed operation class.
    pub const fn kind(self) -> ContextOperationKind {
        self.kind
    }

    /// Complete context join captured when the operation began.
    pub const fn context(self) -> ContextJoin {
        self.context
    }
}

impl fmt::Debug for ContextOperationJoin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextOperationJoin")
            .field("operation", &self.operation)
            .field("kind", &self.kind)
            .field("context", &self.context)
            .finish()
    }
}

/// Coarse native lifecycle without conflating ownership or visibility.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextLifecycle {
    /// Initial native construction is in flight.
    Constructing,
    /// Native context exists and may be observed or operated.
    Ready,
    /// A suspend request is in flight.
    Suspending,
    /// Native context is retained in a suspended state.
    Suspended,
    /// A resume request is in flight.
    Resuming,
    /// The renderer is absent and recovery is queued or intentionally deferred.
    RendererLost,
    /// Renderer-loss recovery is in flight.
    Recovering,
    /// Native destruction is in flight.
    Closing,
    /// Adoption into Browse is in flight.
    Adopting,
    /// Borrowed/handoff release is in flight.
    Releasing,
    /// Automation authority is sealed; only teardown may proceed.
    Faulted,
    /// Native ownership settled in one terminal disposition.
    Terminal(ContextTerminal),
}

/// Terminal disposition of a context identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextTerminal {
    /// Owned native context was closed without becoming a tab.
    Closed,
    /// Borrowed or handoff context returned to its prior human owner.
    Released,
    /// Owned context was explicitly adopted into ordinary Browse.
    Adopted,
}

/// Presentation state, intentionally independent from ownership and control.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextVisibility {
    /// No interactive native page surface is presented.
    Hidden,
    /// The exact native page is presented for inspection or human control.
    Visible,
}

/// Exclusive input owner for the context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextControl {
    /// Agent operations may proceed if the observation is fresh.
    Agent,
    /// A person exclusively owns page input; agent operations are paused.
    Human,
}

/// Whether the current document has been freshly observed for agent use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextFreshness {
    /// A complete observation is required before reference-based action.
    ObservationRequired,
    /// The shell acknowledged an observation against the exact current join.
    Observed,
}

/// Privacy-preserving read model for scheduling and resource accounting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContextStatus {
    lifecycle: ContextLifecycle,
    visibility: ContextVisibility,
    control: ContextControl,
    freshness: ContextFreshness,
    native_view_resident: bool,
    run_cancelled: bool,
    pending_operation: Option<ContextOperationKind>,
}

impl ContextStatus {
    /// Current native lifecycle.
    pub const fn lifecycle(self) -> ContextLifecycle {
        self.lifecycle
    }

    /// Current presentation state.
    pub const fn visibility(self) -> ContextVisibility {
        self.visibility
    }

    /// Current exclusive input owner.
    pub const fn control(self) -> ContextControl {
        self.control
    }

    /// Current observation freshness.
    pub const fn freshness(self) -> ContextFreshness {
        self.freshness
    }

    /// Whether a native view is still owned and counted.
    pub const fn native_view_resident(self) -> bool {
        self.native_view_resident
    }

    /// Whether cancellation is sticky for this run/context binding.
    pub const fn run_cancelled(self) -> bool {
        self.run_cancelled
    }

    /// Current exact in-flight lifecycle operation class, if any.
    pub const fn pending_operation(self) -> Option<ContextOperationKind> {
        self.pending_operation
    }

    /// Returns true only when reference-based agent work may safely proceed.
    pub const fn can_automate(self) -> bool {
        matches!(self.lifecycle, ContextLifecycle::Ready)
            && matches!(self.control, ContextControl::Agent)
            && matches!(self.freshness, ContextFreshness::Observed)
            && !self.run_cancelled
            && self.pending_operation.is_none()
    }
}

/// Exact current context authority joined atomically to its automation status.
///
/// This is a privacy-preserving registry projection, not an action permit.
/// Keeping the join and status together prevents a status sampled from one
/// document or cancellation generation from authorizing another.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContextAutomationState {
    context: ContextJoin,
    status: ContextStatus,
}

impl ContextAutomationState {
    pub(crate) const fn new(context: ContextJoin, status: ContextStatus) -> Self {
        Self { context, status }
    }

    /// Exact current context/document/cancellation authority.
    pub const fn context(self) -> ContextJoin {
        self.context
    }

    /// Atomically sampled lifecycle/control/freshness projection.
    pub const fn status(self) -> ContextStatus {
        self.status
    }

    /// Whether this exact sampled authority may attempt agent input.
    pub const fn can_automate(self) -> bool {
        self.status.can_automate()
    }
}

/// Closed native settlement without page or platform error text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextSettlement {
    /// The exact requested native transition completed.
    Applied,
    /// The exact requested native transition terminated without applying.
    Refused,
}

/// Typed refusal from the pure context lifecycle core.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ContextTransitionError {
    /// Native/profile/document/cancellation identity no longer matches.
    #[error("stale context join")]
    StaleJoin,
    /// An operation result did not match the sole in-flight operation.
    #[error("context operation correlation mismatch")]
    OperationMismatch,
    /// Another lifecycle operation already owns the context.
    #[error("context lifecycle operation already in flight")]
    OperationInFlight,
    /// The current lifecycle cannot perform the requested transition.
    #[error("invalid context lifecycle transition")]
    InvalidLifecycle,
    /// The context kind cannot perform the requested transition.
    #[error("context kind does not support this transition")]
    InvalidKind,
    /// The negotiated native capability is absent.
    #[error("context capability unavailable")]
    CapabilityUnavailable,
    /// A context must be hidden before suspension.
    #[error("context must be hidden")]
    MustBeHidden,
    /// A person currently owns input and agent mutation is forbidden.
    #[error("human control is active")]
    HumanControlActive,
    /// Adoption is allowed only under explicit human control.
    #[error("human control is required")]
    HumanControlRequired,
    /// A sticky run cancellation already revoked automation.
    #[error("context run was cancelled")]
    RunCancelled,
    /// A non-wrapping identity generation was exhausted.
    #[error("context generation exhausted")]
    GenerationExhausted,
    /// The context has already reached a terminal disposition.
    #[error("context is terminal")]
    Terminal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingOperation {
    id: ContextOperationId,
    kind: ContextOperationKind,
}

/// Pure aggregate enforcing context identity, lifecycle, and stale-result joins.
pub struct ContextRecord {
    identity: ContextIdentity,
    capabilities: ContextCapabilities,
    context_generation: ContextGeneration,
    navigation_epoch: NavigationEpoch,
    frame_generation: FrameGeneration,
    cancellation_generation: RunCancellationGeneration,
    lifecycle: ContextLifecycle,
    visibility: ContextVisibility,
    control: ContextControl,
    freshness: ContextFreshness,
    native_view_resident: bool,
    run_cancelled: bool,
    pending: Option<PendingOperation>,
    pending_navigation: Option<ContextOperationId>,
    recovery_target: Option<(ContextControl, ContextVisibility)>,
}

impl ContextRecord {
    /// Starts one hidden context with one exact construction operation.
    pub fn begin(
        identity: ContextIdentity,
        capabilities: ContextCapabilities,
        operation: ContextOperationId,
    ) -> Result<(Self, ContextOperationJoin), ContextTransitionError> {
        if capabilities.kind() != identity.kind() {
            return Err(ContextTransitionError::InvalidKind);
        }
        let record = Self {
            identity,
            capabilities,
            context_generation: ContextGeneration::INITIAL,
            navigation_epoch: NavigationEpoch::INITIAL,
            frame_generation: FrameGeneration::INITIAL,
            cancellation_generation: RunCancellationGeneration::INITIAL,
            lifecycle: ContextLifecycle::Constructing,
            visibility: ContextVisibility::Hidden,
            control: ContextControl::Agent,
            freshness: ContextFreshness::ObservationRequired,
            native_view_resident: false,
            run_cancelled: false,
            pending: Some(PendingOperation {
                id: operation,
                kind: ContextOperationKind::Construct,
            }),
            pending_navigation: None,
            recovery_target: None,
        };
        let join = record.operation_join(operation, ContextOperationKind::Construct);
        Ok((record, join))
    }

    /// Immutable identity for this context.
    pub const fn identity(&self) -> ContextIdentity {
        self.identity
    }

    /// Negotiated, non-authorizing capability inventory.
    pub const fn capabilities(&self) -> ContextCapabilities {
        self.capabilities
    }

    /// Complete current join for an exact asynchronous operation.
    pub const fn join(&self) -> ContextJoin {
        ContextJoin {
            identity: self.identity,
            context_generation: self.context_generation,
            navigation_epoch: self.navigation_epoch,
            frame: FrameId::MAIN,
            frame_generation: self.frame_generation,
            cancellation_generation: self.cancellation_generation,
        }
    }

    /// Compact status without page data, URLs, native handles, or paths.
    pub const fn status(&self) -> ContextStatus {
        ContextStatus {
            lifecycle: self.lifecycle,
            visibility: self.visibility,
            control: self.control,
            freshness: self.freshness,
            native_view_resident: self.native_view_resident,
            run_cancelled: self.run_cancelled,
            pending_operation: match self.pending {
                Some(pending) => Some(pending.kind),
                None if self.pending_navigation.is_some() => Some(ContextOperationKind::Navigate),
                None => None,
            },
        }
    }

    /// Settles the exact initial native construction.
    pub fn settle_construction(
        &mut self,
        join: ContextOperationJoin,
        settlement: ContextSettlement,
    ) -> Result<(), ContextTransitionError> {
        self.require_pending(join, ContextOperationKind::Construct)?;
        if self.lifecycle != ContextLifecycle::Constructing {
            return Err(ContextTransitionError::InvalidLifecycle);
        }
        self.pending = None;
        match settlement {
            ContextSettlement::Applied => {
                self.lifecycle = ContextLifecycle::Ready;
                self.native_view_resident = true;
            }
            ContextSettlement::Refused => self.fault(),
        }
        Ok(())
    }

    /// Marks one complete semantic observation current for the exact join.
    pub fn acknowledge_observation(
        &mut self,
        join: ContextJoin,
    ) -> Result<(), ContextTransitionError> {
        self.require_join(join)?;
        self.require_ready_agent()?;
        self.require_capability(ContextCapability::Observe)?;
        if self.pending_navigation.is_some() {
            return Err(ContextTransitionError::OperationInFlight);
        }
        self.freshness = ContextFreshness::Observed;
        Ok(())
    }

    /// Starts one navigation and invalidates every prior document reference.
    pub fn begin_navigation(
        &mut self,
        operation: ContextOperationId,
    ) -> Result<ContextOperationJoin, ContextTransitionError> {
        self.require_ready_agent()?;
        self.require_capability(ContextCapability::Navigate)?;
        self.require_no_operation()?;
        self.advance(false, true, true, false)?;
        self.freshness = ContextFreshness::ObservationRequired;
        self.pending_navigation = Some(operation);
        Ok(self.operation_join(operation, ContextOperationKind::Navigate))
    }

    /// Settles one exact shell-requested navigation.
    pub fn settle_navigation(
        &mut self,
        join: ContextOperationJoin,
        _settlement: ContextSettlement,
    ) -> Result<(), ContextTransitionError> {
        self.require_join(join.context)?;
        if join.kind != ContextOperationKind::Navigate
            || self.pending_navigation != Some(join.operation)
        {
            return Err(ContextTransitionError::OperationMismatch);
        }
        self.pending_navigation = None;
        self.freshness = ContextFreshness::ObservationRequired;
        Ok(())
    }

    /// Records a page-initiated or human navigation against the prior exact join.
    pub fn observe_navigation_replacement(
        &mut self,
        prior: ContextJoin,
    ) -> Result<ContextJoin, ContextTransitionError> {
        self.require_join(prior)?;
        if !matches!(self.lifecycle, ContextLifecycle::Ready) {
            return Err(ContextTransitionError::InvalidLifecycle);
        }
        self.advance(false, true, true, false)?;
        self.pending_navigation = None;
        self.freshness = ContextFreshness::ObservationRequired;
        Ok(self.join())
    }

    /// Starts presentation for inspection without changing ownership.
    pub fn begin_show(
        &mut self,
        operation: ContextOperationId,
    ) -> Result<ContextOperationJoin, ContextTransitionError> {
        self.require_ready_agent()?;
        self.require_capability(ContextCapability::Present)?;
        self.require_no_operation()?;
        self.pending = Some(PendingOperation {
            id: operation,
            kind: ContextOperationKind::Show,
        });
        Ok(self.operation_join(operation, ContextOperationKind::Show))
    }

    /// Settles exact presentation for inspection.
    pub fn settle_show(
        &mut self,
        join: ContextOperationJoin,
        settlement: ContextSettlement,
    ) -> Result<(), ContextTransitionError> {
        self.require_pending(join, ContextOperationKind::Show)?;
        if self.lifecycle != ContextLifecycle::Ready || self.control != ContextControl::Agent {
            return Err(ContextTransitionError::InvalidLifecycle);
        }
        self.pending = None;
        if settlement == ContextSettlement::Applied {
            self.visibility = ContextVisibility::Visible;
        }
        Ok(())
    }

    /// Starts hiding the exact native page without changing ownership.
    pub fn begin_hide(
        &mut self,
        operation: ContextOperationId,
    ) -> Result<ContextOperationJoin, ContextTransitionError> {
        self.require_ready_agent()?;
        self.require_capability(ContextCapability::Present)?;
        self.require_no_operation()?;
        self.pending = Some(PendingOperation {
            id: operation,
            kind: ContextOperationKind::Hide,
        });
        Ok(self.operation_join(operation, ContextOperationKind::Hide))
    }

    /// Settles exact native hiding.
    pub fn settle_hide(
        &mut self,
        join: ContextOperationJoin,
        settlement: ContextSettlement,
    ) -> Result<(), ContextTransitionError> {
        self.require_pending(join, ContextOperationKind::Hide)?;
        if self.lifecycle != ContextLifecycle::Ready || self.control != ContextControl::Agent {
            return Err(ContextTransitionError::InvalidLifecycle);
        }
        self.pending = None;
        if settlement == ContextSettlement::Applied {
            self.visibility = ContextVisibility::Hidden;
        }
        Ok(())
    }

    /// Revokes agent input and transfers exclusive visible control to a person.
    pub fn begin_human_control(
        &mut self,
        operation: ContextOperationId,
    ) -> Result<ContextOperationJoin, ContextTransitionError> {
        self.require_ready_agent()?;
        self.require_capability(ContextCapability::HumanControl)?;
        // Human input wins over queued navigation and presentation work.
        // Advancing every join coordinate makes their callbacks stale.
        self.pending = None;
        self.pending_navigation = None;
        self.advance(true, true, true, true)?;
        self.control = ContextControl::Human;
        self.freshness = ContextFreshness::ObservationRequired;
        self.pending = Some(PendingOperation {
            id: operation,
            kind: ContextOperationKind::BeginHumanControl,
        });
        Ok(self.operation_join(operation, ContextOperationKind::BeginHumanControl))
    }

    /// Settles native presentation for human control without restoring agent input on refusal.
    pub fn settle_begin_human_control(
        &mut self,
        join: ContextOperationJoin,
        settlement: ContextSettlement,
    ) -> Result<(), ContextTransitionError> {
        self.require_pending(join, ContextOperationKind::BeginHumanControl)?;
        if self.lifecycle != ContextLifecycle::Ready || self.control != ContextControl::Human {
            return Err(ContextTransitionError::InvalidLifecycle);
        }
        self.pending = None;
        if settlement == ContextSettlement::Applied {
            self.visibility = ContextVisibility::Visible;
        }
        Ok(())
    }

    /// Starts return of human input to the agent.
    pub fn begin_end_human_control(
        &mut self,
        operation: ContextOperationId,
    ) -> Result<ContextOperationJoin, ContextTransitionError> {
        self.require_nonterminal()?;
        if self.lifecycle != ContextLifecycle::Ready || self.control != ContextControl::Human {
            return Err(ContextTransitionError::InvalidLifecycle);
        }
        self.require_no_operation()?;
        self.advance(true, true, true, true)?;
        self.freshness = ContextFreshness::ObservationRequired;
        self.pending = Some(PendingOperation {
            id: operation,
            kind: ContextOperationKind::EndHumanControl,
        });
        Ok(self.operation_join(operation, ContextOperationKind::EndHumanControl))
    }

    /// Settles return from human control and requires a complete fresh observation.
    pub fn settle_end_human_control(
        &mut self,
        join: ContextOperationJoin,
        settlement: ContextSettlement,
    ) -> Result<(), ContextTransitionError> {
        self.require_pending(join, ContextOperationKind::EndHumanControl)?;
        if self.lifecycle != ContextLifecycle::Ready || self.control != ContextControl::Human {
            return Err(ContextTransitionError::InvalidLifecycle);
        }
        self.pending = None;
        if settlement == ContextSettlement::Applied {
            self.control = ContextControl::Agent;
        }
        self.freshness = ContextFreshness::ObservationRequired;
        Ok(())
    }

    /// Starts suspension after proving the context is hidden and agent-owned.
    pub fn begin_suspend(
        &mut self,
        operation: ContextOperationId,
    ) -> Result<ContextOperationJoin, ContextTransitionError> {
        self.require_ready_agent()?;
        self.require_capability(ContextCapability::Suspend)?;
        if self.visibility != ContextVisibility::Hidden {
            return Err(ContextTransitionError::MustBeHidden);
        }
        self.require_no_operation()?;
        self.advance(true, true, true, true)?;
        self.lifecycle = ContextLifecycle::Suspending;
        self.freshness = ContextFreshness::ObservationRequired;
        self.pending = Some(PendingOperation {
            id: operation,
            kind: ContextOperationKind::Suspend,
        });
        Ok(self.operation_join(operation, ContextOperationKind::Suspend))
    }

    /// Settles the exact suspend request.
    pub fn settle_suspend(
        &mut self,
        join: ContextOperationJoin,
        settlement: ContextSettlement,
    ) -> Result<(), ContextTransitionError> {
        self.require_pending(join, ContextOperationKind::Suspend)?;
        if self.lifecycle != ContextLifecycle::Suspending {
            return Err(ContextTransitionError::InvalidLifecycle);
        }
        self.pending = None;
        self.lifecycle = match settlement {
            ContextSettlement::Applied => ContextLifecycle::Suspended,
            ContextSettlement::Refused => ContextLifecycle::Ready,
        };
        Ok(())
    }

    /// Starts native resume for one suspended context.
    pub fn begin_resume(
        &mut self,
        operation: ContextOperationId,
    ) -> Result<ContextOperationJoin, ContextTransitionError> {
        self.require_nonterminal()?;
        if self.run_cancelled {
            return Err(ContextTransitionError::RunCancelled);
        }
        self.require_capability(ContextCapability::Suspend)?;
        if self.lifecycle != ContextLifecycle::Suspended {
            return Err(ContextTransitionError::InvalidLifecycle);
        }
        self.require_no_operation()?;
        self.advance(true, true, true, true)?;
        self.lifecycle = ContextLifecycle::Resuming;
        self.freshness = ContextFreshness::ObservationRequired;
        self.pending = Some(PendingOperation {
            id: operation,
            kind: ContextOperationKind::Resume,
        });
        Ok(self.operation_join(operation, ContextOperationKind::Resume))
    }

    /// Settles the exact resume request.
    pub fn settle_resume(
        &mut self,
        join: ContextOperationJoin,
        settlement: ContextSettlement,
    ) -> Result<(), ContextTransitionError> {
        self.require_pending(join, ContextOperationKind::Resume)?;
        if self.lifecycle != ContextLifecycle::Resuming {
            return Err(ContextTransitionError::InvalidLifecycle);
        }
        self.pending = None;
        self.lifecycle = match settlement {
            ContextSettlement::Applied => ContextLifecycle::Ready,
            ContextSettlement::Refused => ContextLifecycle::Suspended,
        };
        Ok(())
    }

    /// Invalidates authority after exact renderer loss without starting work.
    pub fn renderer_lost(
        &mut self,
        prior: ContextJoin,
    ) -> Result<ContextJoin, ContextTransitionError> {
        self.require_join(prior)?;
        self.require_nonterminal()?;
        if matches!(
            self.lifecycle,
            ContextLifecycle::Closing
                | ContextLifecycle::Adopting
                | ContextLifecycle::Releasing
                | ContextLifecycle::RendererLost
                | ContextLifecycle::Faulted
        ) {
            return Err(ContextTransitionError::InvalidLifecycle);
        }
        let recovery_target = self
            .recovery_target
            .unwrap_or((self.control, self.visibility));
        self.advance(true, true, true, true)?;
        self.lifecycle = ContextLifecycle::RendererLost;
        self.visibility = ContextVisibility::Hidden;
        self.freshness = ContextFreshness::ObservationRequired;
        self.native_view_resident = false;
        self.pending_navigation = None;
        self.pending = None;
        self.recovery_target = Some(recovery_target);
        Ok(self.join())
    }

    /// Starts exact renderer recovery after scheduler/resource admission.
    pub fn begin_recovery(
        &mut self,
        operation: ContextOperationId,
    ) -> Result<ContextOperationJoin, ContextTransitionError> {
        self.require_nonterminal()?;
        if self.run_cancelled {
            return Err(ContextTransitionError::RunCancelled);
        }
        self.require_capability(ContextCapability::Recover)?;
        if self.lifecycle != ContextLifecycle::RendererLost || self.recovery_target.is_none() {
            return Err(ContextTransitionError::InvalidLifecycle);
        }
        self.require_no_operation()?;
        self.advance(true, true, true, true)?;
        self.lifecycle = ContextLifecycle::Recovering;
        self.pending = Some(PendingOperation {
            id: operation,
            kind: ContextOperationKind::Recover,
        });
        Ok(self.operation_join(operation, ContextOperationKind::Recover))
    }

    /// Settles exact renderer recovery.
    pub fn settle_recovery(
        &mut self,
        join: ContextOperationJoin,
        settlement: ContextSettlement,
    ) -> Result<(), ContextTransitionError> {
        self.require_pending(join, ContextOperationKind::Recover)?;
        if self.lifecycle != ContextLifecycle::Recovering {
            return Err(ContextTransitionError::InvalidLifecycle);
        }
        self.pending = None;
        let Some(recovery_target) = self.recovery_target.take() else {
            self.fault();
            return Err(ContextTransitionError::OperationMismatch);
        };
        match settlement {
            ContextSettlement::Applied => {
                self.lifecycle = ContextLifecycle::Ready;
                self.native_view_resident = true;
                self.control = recovery_target.0;
                self.visibility = recovery_target.1;
            }
            ContextSettlement::Refused => self.fault(),
        }
        Ok(())
    }

    /// Sticky cancellation that invalidates every outstanding agent join.
    pub fn cancel_run(
        &mut self,
        prior: ContextJoin,
    ) -> Result<ContextJoin, ContextTransitionError> {
        self.require_join(prior)?;
        self.require_nonterminal()?;
        if self.run_cancelled {
            return Err(ContextTransitionError::RunCancelled);
        }
        let operation_was_in_flight = self.pending.is_some();
        self.advance(true, true, true, true)?;
        self.run_cancelled = true;
        self.freshness = ContextFreshness::ObservationRequired;
        self.pending_navigation = None;
        if operation_was_in_flight {
            self.fault();
        }
        Ok(self.join())
    }

    /// Starts unconditional native teardown for any nonterminal context.
    pub fn begin_close(
        &mut self,
        operation: ContextOperationId,
    ) -> Result<ContextOperationJoin, ContextTransitionError> {
        self.require_nonterminal()?;
        if self.identity.kind() != ContextKind::Owned {
            return Err(ContextTransitionError::InvalidKind);
        }
        if self.pending.is_some() && self.lifecycle != ContextLifecycle::Faulted {
            return Err(ContextTransitionError::OperationInFlight);
        }
        if self.lifecycle != ContextLifecycle::Faulted {
            self.advance(true, true, true, true)?;
        }
        self.lifecycle = ContextLifecycle::Closing;
        self.visibility = ContextVisibility::Hidden;
        self.control = ContextControl::Agent;
        self.freshness = ContextFreshness::ObservationRequired;
        self.pending_navigation = None;
        self.recovery_target = None;
        self.pending = Some(PendingOperation {
            id: operation,
            kind: ContextOperationKind::Close,
        });
        Ok(self.operation_join(operation, ContextOperationKind::Close))
    }

    /// Settles native teardown; refusal remains faulted and resident.
    pub fn settle_close(
        &mut self,
        join: ContextOperationJoin,
        settlement: ContextSettlement,
    ) -> Result<(), ContextTransitionError> {
        self.require_pending(join, ContextOperationKind::Close)?;
        if self.lifecycle != ContextLifecycle::Closing {
            return Err(ContextTransitionError::InvalidLifecycle);
        }
        self.pending = None;
        match settlement {
            ContextSettlement::Applied => {
                self.lifecycle = ContextLifecycle::Terminal(ContextTerminal::Closed);
                self.native_view_resident = false;
            }
            ContextSettlement::Refused => self.fault(),
        }
        Ok(())
    }

    /// Starts explicit user-authorized adoption of an owned context.
    pub fn begin_adoption(
        &mut self,
        operation: ContextOperationId,
    ) -> Result<ContextOperationJoin, ContextTransitionError> {
        self.require_nonterminal()?;
        if self.identity.kind() != ContextKind::Owned {
            return Err(ContextTransitionError::InvalidKind);
        }
        if self.run_cancelled {
            return Err(ContextTransitionError::RunCancelled);
        }
        self.require_capability(ContextCapability::Adopt)?;
        if self.lifecycle != ContextLifecycle::Ready || self.control != ContextControl::Human {
            return Err(ContextTransitionError::HumanControlRequired);
        }
        self.require_no_operation()?;
        self.advance(true, true, true, true)?;
        self.lifecycle = ContextLifecycle::Adopting;
        self.freshness = ContextFreshness::ObservationRequired;
        self.pending = Some(PendingOperation {
            id: operation,
            kind: ContextOperationKind::Adopt,
        });
        Ok(self.operation_join(operation, ContextOperationKind::Adopt))
    }

    /// Settles adoption; refusal returns to explicit human control.
    pub fn settle_adoption(
        &mut self,
        join: ContextOperationJoin,
        settlement: ContextSettlement,
    ) -> Result<(), ContextTransitionError> {
        self.require_pending(join, ContextOperationKind::Adopt)?;
        if self.lifecycle != ContextLifecycle::Adopting {
            return Err(ContextTransitionError::InvalidLifecycle);
        }
        self.pending = None;
        match settlement {
            ContextSettlement::Applied => {
                self.lifecycle = ContextLifecycle::Terminal(ContextTerminal::Adopted);
                self.native_view_resident = false;
            }
            ContextSettlement::Refused => {
                self.lifecycle = ContextLifecycle::Ready;
                self.control = ContextControl::Human;
                self.visibility = ContextVisibility::Visible;
            }
        }
        Ok(())
    }

    /// Starts release of a borrowed-tab or sign-in-handoff context.
    pub fn begin_release(
        &mut self,
        operation: ContextOperationId,
    ) -> Result<ContextOperationJoin, ContextTransitionError> {
        self.require_nonterminal()?;
        if !matches!(
            self.identity.kind(),
            ContextKind::BorrowedTab | ContextKind::HumanSignInHandoff
        ) {
            return Err(ContextTransitionError::InvalidKind);
        }
        self.require_capability(ContextCapability::Release)?;
        if !matches!(
            self.lifecycle,
            ContextLifecycle::Ready
                | ContextLifecycle::Suspended
                | ContextLifecycle::RendererLost
                | ContextLifecycle::Faulted
        ) {
            return Err(ContextTransitionError::InvalidLifecycle);
        }
        self.require_no_operation()?;
        if self.lifecycle != ContextLifecycle::Faulted {
            self.advance(true, true, true, true)?;
        }
        self.lifecycle = ContextLifecycle::Releasing;
        self.freshness = ContextFreshness::ObservationRequired;
        self.recovery_target = None;
        self.pending = Some(PendingOperation {
            id: operation,
            kind: ContextOperationKind::Release,
        });
        Ok(self.operation_join(operation, ContextOperationKind::Release))
    }

    /// Settles borrowed/handoff release; refusal seals automation for teardown.
    pub fn settle_release(
        &mut self,
        join: ContextOperationJoin,
        settlement: ContextSettlement,
    ) -> Result<(), ContextTransitionError> {
        self.require_pending(join, ContextOperationKind::Release)?;
        if self.lifecycle != ContextLifecycle::Releasing {
            return Err(ContextTransitionError::InvalidLifecycle);
        }
        self.pending = None;
        match settlement {
            ContextSettlement::Applied => {
                self.lifecycle = ContextLifecycle::Terminal(ContextTerminal::Released);
                self.native_view_resident = false;
            }
            ContextSettlement::Refused => self.fault(),
        }
        Ok(())
    }

    fn require_join(&self, join: ContextJoin) -> Result<(), ContextTransitionError> {
        if join == self.join() {
            Ok(())
        } else {
            Err(ContextTransitionError::StaleJoin)
        }
    }

    fn require_pending(
        &self,
        join: ContextOperationJoin,
        expected: ContextOperationKind,
    ) -> Result<(), ContextTransitionError> {
        self.require_join(join.context)?;
        if join.kind != expected
            || self.pending
                != Some(PendingOperation {
                    id: join.operation,
                    kind: expected,
                })
        {
            return Err(ContextTransitionError::OperationMismatch);
        }
        Ok(())
    }

    fn require_no_operation(&self) -> Result<(), ContextTransitionError> {
        if self.pending.is_some() || self.pending_navigation.is_some() {
            Err(ContextTransitionError::OperationInFlight)
        } else {
            Ok(())
        }
    }

    fn require_nonterminal(&self) -> Result<(), ContextTransitionError> {
        if matches!(self.lifecycle, ContextLifecycle::Terminal(_)) {
            Err(ContextTransitionError::Terminal)
        } else {
            Ok(())
        }
    }

    fn require_ready_agent(&self) -> Result<(), ContextTransitionError> {
        self.require_nonterminal()?;
        if self.run_cancelled {
            return Err(ContextTransitionError::RunCancelled);
        }
        if self.control == ContextControl::Human {
            return Err(ContextTransitionError::HumanControlActive);
        }
        if self.lifecycle != ContextLifecycle::Ready {
            return Err(ContextTransitionError::InvalidLifecycle);
        }
        Ok(())
    }

    fn require_capability(
        &self,
        capability: ContextCapability,
    ) -> Result<(), ContextTransitionError> {
        if self.capabilities.contains(capability) {
            Ok(())
        } else {
            Err(ContextTransitionError::CapabilityUnavailable)
        }
    }

    fn operation_join(
        &self,
        operation: ContextOperationId,
        kind: ContextOperationKind,
    ) -> ContextOperationJoin {
        ContextOperationJoin {
            operation,
            kind,
            context: self.join(),
        }
    }

    fn advance(
        &mut self,
        context: bool,
        navigation: bool,
        frame: bool,
        cancellation: bool,
    ) -> Result<(), ContextTransitionError> {
        let next_context = if context {
            self.context_generation.next()
        } else {
            Some(self.context_generation)
        };
        let next_navigation = if navigation {
            self.navigation_epoch.next()
        } else {
            Some(self.navigation_epoch)
        };
        let next_frame = if frame {
            self.frame_generation.next()
        } else {
            Some(self.frame_generation)
        };
        let next_cancellation = if cancellation {
            self.cancellation_generation.next()
        } else {
            Some(self.cancellation_generation)
        };
        let (Some(next_context), Some(next_navigation), Some(next_frame), Some(next_cancellation)) =
            (next_context, next_navigation, next_frame, next_cancellation)
        else {
            self.fault();
            return Err(ContextTransitionError::GenerationExhausted);
        };
        self.context_generation = next_context;
        self.navigation_epoch = next_navigation;
        self.frame_generation = next_frame;
        self.cancellation_generation = next_cancellation;
        Ok(())
    }

    fn fault(&mut self) {
        self.lifecycle = ContextLifecycle::Faulted;
        self.visibility = ContextVisibility::Hidden;
        self.freshness = ContextFreshness::ObservationRequired;
        self.pending = None;
        self.pending_navigation = None;
        self.recovery_target = None;
    }
}

impl fmt::Debug for ContextRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextRecord")
            .field("identity", &self.identity)
            .field("capabilities", &self.capabilities)
            .field("context_generation", &self.context_generation)
            .field("navigation_epoch", &self.navigation_epoch)
            .field("frame_generation", &self.frame_generation)
            .field("cancellation_generation", &self.cancellation_generation)
            .field("status", &self.status())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNED_CAPABILITIES: [ContextCapability; 9] = [
        ContextCapability::Navigate,
        ContextCapability::Observe,
        ContextCapability::Act,
        ContextCapability::Present,
        ContextCapability::Suspend,
        ContextCapability::Recover,
        ContextCapability::HumanControl,
        ContextCapability::Adopt,
        ContextCapability::ImportCookies,
    ];

    fn profile() -> ProfileId {
        ProfileId::from(3)
    }

    fn identity(kind: ContextKind) -> ContextIdentity {
        ContextIdentity::new(
            ContextId::from_raw(1),
            ContextRunId::from_raw(2),
            profile(),
            kind,
        )
    }

    fn operation(value: u64) -> ContextOperationId {
        ContextOperationId::new(value).expect("nonzero operation")
    }

    fn owned() -> (ContextRecord, ContextOperationJoin) {
        ContextRecord::begin(
            identity(ContextKind::Owned),
            ContextCapabilities::try_new(ContextKind::Owned, &OWNED_CAPABILITIES)
                .expect("capabilities"),
            operation(1),
        )
        .expect("context")
    }

    fn ready_owned() -> ContextRecord {
        let (mut record, join) = owned();
        record
            .settle_construction(join, ContextSettlement::Applied)
            .expect("construct");
        record
    }

    fn show(record: &mut ContextRecord, value: u64) {
        let join = record.begin_show(operation(value)).expect("show");
        record
            .settle_show(join, ContextSettlement::Applied)
            .expect("shown");
    }

    fn hide(record: &mut ContextRecord, value: u64) {
        let join = record.begin_hide(operation(value)).expect("hide");
        record
            .settle_hide(join, ContextSettlement::Applied)
            .expect("hidden");
    }

    fn begin_human(record: &mut ContextRecord, value: u64) -> ContextJoin {
        let join = record
            .begin_human_control(operation(value))
            .expect("human control");
        record
            .settle_begin_human_control(join, ContextSettlement::Applied)
            .expect("human control presented");
        join.context()
    }

    fn end_human(record: &mut ContextRecord, value: u64) -> ContextJoin {
        let join = record
            .begin_end_human_control(operation(value))
            .expect("end human control");
        record
            .settle_end_human_control(join, ContextSettlement::Applied)
            .expect("agent control restored");
        join.context()
    }

    #[test]
    fn durable_ids_are_canonical_and_debug_redacted() {
        let id = ContextId::from_raw(7);
        let encoded = serde_json::to_string(&id).expect("encode");
        let decoded: ContextId = serde_json::from_str(&encoded).expect("decode");
        assert_eq!(decoded, id);
        assert_eq!(format!("{id:?}"), "ContextId([redacted])");
        assert!(ContextId::parse("0000000000000000000000000o").is_none());
    }

    #[test]
    fn capability_inventory_is_kind_scoped_and_duplicate_closed() {
        assert_eq!(
            ContextCapabilities::try_new(
                ContextKind::Owned,
                &[ContextCapability::Observe, ContextCapability::Observe]
            ),
            Err(ContextCapabilityError::Duplicate)
        );
        assert_eq!(
            ContextCapabilities::try_new(ContextKind::Owned, &[ContextCapability::Release]),
            Err(ContextCapabilityError::IncompatibleKind)
        );
        assert!(ContextCapabilities::try_new(
            ContextKind::HumanSignInHandoff,
            &[
                ContextCapability::Navigate,
                ContextCapability::Present,
                ContextCapability::HumanControl,
                ContextCapability::ExportCookies,
                ContextCapability::Release,
            ]
        )
        .is_ok());
    }

    #[test]
    fn construction_observation_and_navigation_are_exactly_joined() {
        let (mut record, construction) = owned();
        assert!(!record.status().can_automate());
        record
            .settle_construction(construction, ContextSettlement::Applied)
            .expect("construction");
        let initial = record.join();
        record
            .acknowledge_observation(initial)
            .expect("observation");
        assert!(record.status().can_automate());

        let navigation = record.begin_navigation(operation(2)).expect("navigation");
        assert_ne!(
            navigation.context().navigation_epoch(),
            initial.navigation_epoch()
        );
        assert_ne!(
            navigation.context().frame_generation(),
            initial.frame_generation()
        );
        assert!(!record.status().can_automate());
        assert_eq!(
            record.settle_navigation(
                ContextOperationJoin {
                    operation: operation(3),
                    ..navigation
                },
                ContextSettlement::Applied,
            ),
            Err(ContextTransitionError::OperationMismatch)
        );
        record
            .settle_navigation(navigation, ContextSettlement::Applied)
            .expect("navigation settlement");
        assert_eq!(
            record.acknowledge_observation(initial),
            Err(ContextTransitionError::StaleJoin)
        );
        record
            .acknowledge_observation(record.join())
            .expect("fresh observation");
        assert!(record.status().can_automate());
    }

    #[test]
    fn visibility_never_changes_identity_or_input_owner() {
        let mut record = ready_owned();
        let identity = record.identity();
        let generation = record.join().context_generation();
        show(&mut record, 2);
        assert_eq!(record.identity(), identity);
        assert_eq!(record.join().context_generation(), generation);
        assert_eq!(record.status().control(), ContextControl::Agent);
        hide(&mut record, 3);
        assert_eq!(record.status().visibility(), ContextVisibility::Hidden);
    }

    #[test]
    fn presentation_changes_only_after_exact_native_settlement() {
        let mut record = ready_owned();
        let show_operation = record.begin_show(operation(2)).expect("show");
        assert_eq!(record.status().visibility(), ContextVisibility::Hidden);
        assert_eq!(
            record.status().pending_operation(),
            Some(ContextOperationKind::Show)
        );
        record
            .settle_show(show_operation, ContextSettlement::Refused)
            .expect("show refused");
        assert_eq!(record.status().visibility(), ContextVisibility::Hidden);

        show(&mut record, 3);
        let hide = record.begin_hide(operation(4)).expect("hide");
        assert_eq!(record.status().visibility(), ContextVisibility::Visible);
        record
            .settle_hide(hide, ContextSettlement::Refused)
            .expect("hide refused");
        assert_eq!(record.status().visibility(), ContextVisibility::Visible);
    }

    #[test]
    fn native_refusal_never_returns_input_from_a_person() {
        let mut record = ready_owned();
        let human = record
            .begin_human_control(operation(2))
            .expect("begin human control");
        record
            .settle_begin_human_control(human, ContextSettlement::Refused)
            .expect("presentation refused");
        assert_eq!(record.status().control(), ContextControl::Human);
        assert_eq!(record.status().visibility(), ContextVisibility::Hidden);

        let agent = record
            .begin_end_human_control(operation(3))
            .expect("begin agent return");
        record
            .settle_end_human_control(agent, ContextSettlement::Refused)
            .expect("agent return refused");
        assert_eq!(record.status().control(), ContextControl::Human);
        assert_eq!(
            record.status().freshness(),
            ContextFreshness::ObservationRequired
        );
        assert!(!record.status().can_automate());
    }

    #[test]
    fn human_takeover_preempts_pending_presentation() {
        let mut record = ready_owned();
        let show = record.begin_show(operation(2)).expect("show");
        let human = record
            .begin_human_control(operation(3))
            .expect("human control");
        assert_eq!(record.status().control(), ContextControl::Human);
        assert_eq!(
            record.settle_show(show, ContextSettlement::Applied),
            Err(ContextTransitionError::StaleJoin)
        );
        record
            .settle_begin_human_control(human, ContextSettlement::Applied)
            .expect("human control presented");
        assert_eq!(record.status().visibility(), ContextVisibility::Visible);
    }

    #[test]
    fn human_takeover_revokes_old_results_and_requires_reobservation() {
        let mut record = ready_owned();
        record
            .acknowledge_observation(record.join())
            .expect("observation");
        let old = record.join();
        let pending_navigation = record
            .begin_navigation(operation(2))
            .expect("pending navigation");
        let human_operation = record
            .begin_human_control(operation(3))
            .expect("human control");
        let human = human_operation.context();
        assert_ne!(old.context_generation(), human.context_generation());
        assert_ne!(
            old.cancellation_generation(),
            human.cancellation_generation()
        );
        assert_eq!(record.status().control(), ContextControl::Human);
        assert_eq!(
            record.acknowledge_observation(old),
            Err(ContextTransitionError::StaleJoin)
        );
        assert_eq!(
            record.settle_navigation(pending_navigation, ContextSettlement::Applied),
            Err(ContextTransitionError::StaleJoin)
        );
        assert_eq!(
            record.begin_hide(operation(4)),
            Err(ContextTransitionError::HumanControlActive)
        );
        record
            .settle_begin_human_control(human_operation, ContextSettlement::Applied)
            .expect("human control presented");
        let returned = end_human(&mut record, 5);
        assert_ne!(returned.context_generation(), human.context_generation());
        assert!(!record.status().can_automate());
        record
            .acknowledge_observation(returned)
            .expect("new observation");
        assert!(record.status().can_automate());
    }

    #[test]
    fn suspend_resume_preserve_native_ownership_but_invalidate_references() {
        let mut record = ready_owned();
        let old = record.join();
        show(&mut record, 2);
        assert_eq!(
            record.begin_suspend(operation(2)),
            Err(ContextTransitionError::MustBeHidden)
        );
        hide(&mut record, 3);
        let suspend = record.begin_suspend(operation(2)).expect("suspend");
        record
            .settle_suspend(suspend, ContextSettlement::Applied)
            .expect("suspended");
        assert_eq!(record.status().lifecycle(), ContextLifecycle::Suspended);
        assert!(record.status().native_view_resident());
        let resume = record.begin_resume(operation(3)).expect("resume");
        record
            .settle_resume(resume, ContextSettlement::Applied)
            .expect("resumed");
        assert_eq!(record.status().lifecycle(), ContextLifecycle::Ready);
        assert_eq!(
            record.acknowledge_observation(old),
            Err(ContextTransitionError::StaleJoin)
        );
    }

    #[test]
    fn renderer_loss_requires_exact_recovery_before_new_observation() {
        let mut record = ready_owned();
        let old = record.join();
        let lost = record.renderer_lost(old).expect("renderer loss");
        assert!(!record.status().native_view_resident());
        assert_eq!(record.status().lifecycle(), ContextLifecycle::RendererLost);
        assert_eq!(record.join(), lost);
        let recovery = record.begin_recovery(operation(2)).expect("recovery");
        assert_eq!(record.status().lifecycle(), ContextLifecycle::Recovering);
        assert_eq!(
            record.settle_recovery(
                ContextOperationJoin {
                    operation: operation(3),
                    ..recovery
                },
                ContextSettlement::Applied,
            ),
            Err(ContextTransitionError::OperationMismatch)
        );
        record
            .settle_recovery(recovery, ContextSettlement::Applied)
            .expect("recovery");
        assert!(record.status().native_view_resident());
        assert!(!record.status().can_automate());
    }

    #[test]
    fn renderer_recovery_preserves_existing_human_control() {
        let mut record = ready_owned();
        let human = begin_human(&mut record, 2);
        record.renderer_lost(human).expect("renderer loss");
        assert_eq!(record.status().control(), ContextControl::Human);
        let recovery = record.begin_recovery(operation(2)).expect("recovery");
        record
            .settle_recovery(recovery, ContextSettlement::Applied)
            .expect("recovery");
        assert_eq!(record.status().control(), ContextControl::Human);
        assert_eq!(record.status().visibility(), ContextVisibility::Visible);
        assert!(!record.status().can_automate());
    }

    #[test]
    fn run_cancellation_is_sticky_and_invalidates_pending_navigation() {
        let mut record = ready_owned();
        let navigation = record.begin_navigation(operation(2)).expect("navigation");
        let cancelled = record
            .cancel_run(navigation.context())
            .expect("cancellation");
        assert!(record.status().run_cancelled());
        assert_eq!(
            record.settle_navigation(navigation, ContextSettlement::Applied),
            Err(ContextTransitionError::StaleJoin)
        );
        assert_eq!(
            record.begin_navigation(operation(3)),
            Err(ContextTransitionError::RunCancelled)
        );
        assert_eq!(record.join(), cancelled);
        let close = record.begin_close(operation(4)).expect("cleanup");
        record
            .settle_close(close, ContextSettlement::Applied)
            .expect("closed");
        assert_eq!(
            record.status().lifecycle(),
            ContextLifecycle::Terminal(ContextTerminal::Closed)
        );
    }

    #[test]
    fn cancellation_seals_inflight_native_transition_for_cleanup() {
        let mut record = ready_owned();
        let suspend = record.begin_suspend(operation(2)).expect("suspend");
        let cancelled = record.cancel_run(suspend.context()).expect("cancel");
        assert_eq!(record.status().lifecycle(), ContextLifecycle::Faulted);
        assert_eq!(record.join(), cancelled);
        assert_eq!(
            record.settle_suspend(suspend, ContextSettlement::Applied),
            Err(ContextTransitionError::StaleJoin)
        );
        let close = record.begin_close(operation(3)).expect("cleanup");
        record
            .settle_close(close, ContextSettlement::Applied)
            .expect("close");
    }

    #[test]
    fn cancellation_never_takes_input_from_a_person() {
        let mut record = ready_owned();
        let human = begin_human(&mut record, 2);
        record.cancel_run(human).expect("cancel");
        assert_eq!(record.status().control(), ContextControl::Human);
        assert_eq!(record.status().visibility(), ContextVisibility::Visible);
        assert!(record.status().run_cancelled());
        assert!(!record.status().can_automate());
    }

    #[test]
    fn owned_adoption_and_borrowed_release_are_mutually_exclusive() {
        let mut owned = ready_owned();
        assert_eq!(
            owned.begin_adoption(operation(2)),
            Err(ContextTransitionError::HumanControlRequired)
        );
        begin_human(&mut owned, 2);
        let adoption = owned.begin_adoption(operation(3)).expect("adoption");
        owned
            .settle_adoption(adoption, ContextSettlement::Applied)
            .expect("adopted");
        assert_eq!(
            owned.status().lifecycle(),
            ContextLifecycle::Terminal(ContextTerminal::Adopted)
        );
        assert_eq!(
            owned.begin_close(operation(3)),
            Err(ContextTransitionError::Terminal)
        );

        let borrowed_capabilities = ContextCapabilities::try_new(
            ContextKind::BorrowedTab,
            &[
                ContextCapability::Observe,
                ContextCapability::Present,
                ContextCapability::HumanControl,
                ContextCapability::Release,
            ],
        )
        .expect("borrowed capabilities");
        let (mut borrowed, construction) = ContextRecord::begin(
            identity(ContextKind::BorrowedTab),
            borrowed_capabilities,
            operation(10),
        )
        .expect("borrowed");
        borrowed
            .settle_construction(construction, ContextSettlement::Applied)
            .expect("borrowed ready");
        assert_eq!(
            borrowed.begin_close(operation(11)),
            Err(ContextTransitionError::InvalidKind)
        );
        assert_eq!(
            borrowed.begin_adoption(operation(11)),
            Err(ContextTransitionError::InvalidKind)
        );
        let release = borrowed.begin_release(operation(11)).expect("release");
        borrowed
            .settle_release(release, ContextSettlement::Applied)
            .expect("released");
        assert_eq!(
            borrowed.status().lifecycle(),
            ContextLifecycle::Terminal(ContextTerminal::Released)
        );
    }

    #[test]
    fn generation_exhaustion_seals_automation_but_still_allows_close() {
        let mut record = ready_owned();
        record.context_generation = ContextGeneration::new(u64::MAX).expect("max");
        assert_eq!(
            record.begin_human_control(operation(2)),
            Err(ContextTransitionError::GenerationExhausted)
        );
        assert_eq!(record.status().lifecycle(), ContextLifecycle::Faulted);
        let close = record.begin_close(operation(2)).expect("fault cleanup");
        record
            .settle_close(close, ContextSettlement::Applied)
            .expect("closed");
        assert_eq!(
            record.status().lifecycle(),
            ContextLifecycle::Terminal(ContextTerminal::Closed)
        );
    }
}
