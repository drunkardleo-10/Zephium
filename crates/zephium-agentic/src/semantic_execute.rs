//! One-shot handoff from policy-dispatched semantic authority to a fixed backend.
//!
//! This functional core carries no selector, script, DOM object, native handle,
//! timer, worker, queue, retry, or backend-order policy. It splits one exact
//! policy-dispatched action into a retained authority half and a closed native
//! recipe, then rejoins the move-only native settlement before semantic waiting
//! or independent effect verification may begin.

use std::fmt;
use std::num::NonZeroU64;

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    AgentActiveEffect, AgentEffectId, ContextIdentity, SemanticActionAttemptId,
    SemanticActionFailure, SemanticActionKind, SemanticActionText, SemanticFrameJoin,
    SemanticInvocationId, SemanticPreparedAction, SemanticPressKey, SemanticRect, SemanticRole,
    SemanticScrollAmount, SemanticScrollDirection, SemanticSettleError, SemanticSettleInstant,
    SemanticSettleTracker, SemanticSnapshotGeneration,
};

/// Maximum time between final policy dispatch and one native backend terminal.
pub const MAX_SEMANTIC_ACTION_NATIVE_EXECUTION_MILLIS: u32 = 5_000;
/// Maximum admitted native viewport dimension used by visibility evidence.
pub const MAX_SEMANTIC_ACTION_VIEWPORT_DIMENSION: u32 = 32_768;

/// Monotonic shell clock used only by native action execution.
#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub struct SemanticActionExecutionInstant(u64);

impl SemanticActionExecutionInstant {
    /// Wraps one millisecond tick from the shell's process-local monotonic clock.
    pub const fn from_millis(value: u64) -> Self {
        Self(value)
    }

    /// Returns the raw tick only to the timer and metrics owner.
    pub const fn millis(self) -> u64 {
        self.0
    }

    const fn checked_add(self, millis: u32) -> Option<Self> {
        match self.0.checked_add(millis as u64) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

impl fmt::Debug for SemanticActionExecutionInstant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SemanticActionExecutionInstant([redacted])")
    }
}

/// Private stable target identity exposed only to a trusted native adapter.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SemanticActionNativeTargetId(NonZeroU64);

impl SemanticActionNativeTargetId {
    fn from_key(key: crate::semantic::SemanticNodeKey) -> Self {
        Self(key.into_nonzero())
    }

    /// Returns the process-local stable key for the fixed isolated/native adapter.
    ///
    /// This value is not durable DOM identity and must never enter a model,
    /// diagnostic, persistence record, selector, or page-world bridge.
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Debug for SemanticActionNativeTargetId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SemanticActionNativeTargetId([redacted])")
    }
}

/// Closed backend class reported by a trusted platform adapter.
///
/// This enum is diagnostic attribution, not backend selection authority. A
/// platform policy must choose only from separately qualified routes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticActionExecutionBackend {
    /// One fixed semantic recipe in Zephium's immutable isolated runtime.
    FixedSemanticRecipe,
    /// A supported engine-native input route scoped to the owned view.
    EngineNativeInput,
    /// An in-process accessibility action scoped to the owned view.
    InProcessAccessibility,
}

/// Closed native readiness proof sampled immediately before backend dispatch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticActionNativeReadiness {
    /// The exact target was connected, visible, and the unoccluded hit target.
    ExactVisibleUnoccludedTarget,
    /// The exact scroll target was connected and structurally revalidated.
    ExactConnectedScrollTarget,
}

/// Bounded current native viewport used to validate visibility evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticActionNativeViewport {
    width: u32,
    height: u32,
}

impl SemanticActionNativeViewport {
    /// Validates nonzero viewport dimensions under the fixed native ceiling.
    pub const fn try_new(
        width: u32,
        height: u32,
    ) -> Result<Self, SemanticActionNativeViewportError> {
        if width == 0
            || height == 0
            || width > MAX_SEMANTIC_ACTION_VIEWPORT_DIMENSION
            || height > MAX_SEMANTIC_ACTION_VIEWPORT_DIMENSION
        {
            return Err(SemanticActionNativeViewportError::Invalid);
        }
        Ok(Self { width, height })
    }

    /// Current viewport width in quantized CSS coordinates.
    pub const fn width(self) -> u32 {
        self.width
    }

    /// Current viewport height in quantized CSS coordinates.
    pub const fn height(self) -> u32 {
        self.height
    }
}

/// Refusal while constructing bounded viewport evidence.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticActionNativeViewportError {
    /// A dimension was zero or exceeded the fixed ceiling.
    #[error("semantic action native viewport is invalid")]
    Invalid,
}

#[derive(Clone, Eq, PartialEq)]
struct NativeCorrelation {
    effect: AgentEffectId,
    attempt: SemanticActionAttemptId,
    frame: SemanticFrameJoin,
    checkpoint_invocation: SemanticInvocationId,
    checkpoint_snapshot: SemanticSnapshotGeneration,
    target: SemanticActionNativeTargetId,
    requested_at: SemanticActionExecutionInstant,
    deadline: SemanticActionExecutionInstant,
    guard: [u8; 32],
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct SemanticActionCoordinatorKey {
    effect: AgentEffectId,
    attempt: SemanticActionAttemptId,
    context: ContextIdentity,
    guard: [u8; 32],
}

impl SemanticActionCoordinatorKey {
    pub(crate) const fn effect(self) -> AgentEffectId {
        self.effect
    }

    pub(crate) const fn attempt(self) -> SemanticActionAttemptId {
        self.attempt
    }

    pub(crate) const fn context(self) -> ContextIdentity {
        self.context
    }
}

impl NativeCorrelation {
    const fn coordinator_key(&self) -> SemanticActionCoordinatorKey {
        SemanticActionCoordinatorKey {
            effect: self.effect,
            attempt: self.attempt,
            context: self.frame.context().identity(),
            guard: self.guard,
        }
    }
}

#[derive(Eq, PartialEq)]
enum NativeRecipe {
    Click,
    Fill(SemanticActionText),
    Select(SemanticActionNativeTargetId),
    Press(SemanticPressKey),
    Scroll(SemanticScrollDirection, SemanticScrollAmount),
}

impl NativeRecipe {
    const fn kind(&self) -> SemanticActionKind {
        match self {
            Self::Click => SemanticActionKind::Click,
            Self::Fill(_) => SemanticActionKind::Fill,
            Self::Select(_) => SemanticActionKind::Select,
            Self::Press(_) => SemanticActionKind::Press,
            Self::Scroll(_, _) => SemanticActionKind::Scroll,
        }
    }
}

/// Retained policy authority waiting for one exact native terminal.
#[must_use]
pub(crate) struct SemanticActionExecutionPending {
    correlation: NativeCorrelation,
    active: AgentActiveEffect,
}

impl SemanticActionExecutionPending {
    /// Exact dispatched attempt retained by this one-shot join.
    #[cfg(test)]
    pub(crate) const fn attempt(&self) -> SemanticActionAttemptId {
        self.correlation.attempt
    }

    /// Native execution deadline derived before the request left the core.
    pub const fn deadline(&self) -> SemanticActionExecutionInstant {
        self.correlation.deadline
    }

    pub(crate) const fn coordinator_key(&self) -> SemanticActionCoordinatorKey {
        self.correlation.coordinator_key()
    }

    pub(crate) fn matches_native_settlement(
        &self,
        settlement: &SemanticActionNativeSettlement,
    ) -> bool {
        self.correlation == settlement.correlation
    }

    /// Rejoins a move-only native terminal and always returns policy authority.
    pub fn settle(
        self,
        current_frame: &SemanticFrameJoin,
        settlement: SemanticActionNativeSettlement,
    ) -> SemanticActionExecutionOutcome {
        let disposition = if self.correlation != settlement.correlation {
            SemanticActionExecutionDisposition::ContractViolation(
                SemanticActionExecutionContractError::RequestMismatch,
            )
        } else if current_frame != &self.correlation.frame {
            SemanticActionExecutionDisposition::Failed(SemanticActionFailure::StaleReference)
        } else {
            admit_native_outcome(
                settlement.outcome,
                self.correlation.requested_at,
                self.correlation.deadline,
            )
        };
        SemanticActionExecutionOutcome {
            active: self.active,
            disposition,
        }
    }

    /// Settles a synchronous shell/port refusal without fabricating native evidence.
    pub fn refuse(self, failure: SemanticActionFailure) -> SemanticActionExecutionOutcome {
        SemanticActionExecutionOutcome {
            active: self.active,
            disposition: SemanticActionExecutionDisposition::Failed(failure),
        }
    }
}

impl fmt::Debug for SemanticActionExecutionPending {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticActionExecutionPending")
            .field("effect", &self.correlation.effect)
            .field("attempt", &self.correlation.attempt)
            .field("frame", &self.correlation.frame)
            .field(
                "checkpoint_invocation",
                &self.correlation.checkpoint_invocation,
            )
            .field("checkpoint_snapshot", &self.correlation.checkpoint_snapshot)
            .field("target", &self.correlation.target)
            .field("deadline", &self.correlation.deadline)
            .field("guard", &"[redacted]")
            .finish()
    }
}

/// Non-cloneable closed recipe for one trusted native adapter.
#[must_use]
pub struct SemanticActionNativeRequest {
    correlation: NativeCorrelation,
    role: SemanticRole,
    expected_geometry: SemanticRect,
    recipe: NativeRecipe,
}

impl SemanticActionNativeRequest {
    /// Exact policy-dispatched action attempt.
    pub const fn attempt(&self) -> SemanticActionAttemptId {
        self.correlation.attempt
    }

    /// Exact current frame and document authority.
    pub const fn frame(&self) -> &SemanticFrameJoin {
        &self.correlation.frame
    }

    /// Exact semantic invocation checkpointed before policy dispatch.
    pub const fn checkpoint_invocation(&self) -> SemanticInvocationId {
        self.correlation.checkpoint_invocation
    }

    /// Exact semantic snapshot checkpointed before policy dispatch.
    pub const fn checkpoint_snapshot(&self) -> SemanticSnapshotGeneration {
        self.correlation.checkpoint_snapshot
    }

    /// Private target key to re-resolve inside the fixed adapter.
    pub const fn target(&self) -> SemanticActionNativeTargetId {
        self.correlation.target
    }

    /// Target role that the adapter must revalidate before executing.
    pub const fn target_role(&self) -> SemanticRole {
        self.role
    }

    /// Fresh semantic geometry used only as a native revalidation hint.
    pub const fn expected_geometry(&self) -> SemanticRect {
        self.expected_geometry
    }

    /// Closed action class.
    pub const fn kind(&self) -> SemanticActionKind {
        self.recipe.kind()
    }

    /// Exact bounded fill text for a fill recipe.
    pub const fn fill_text(&self) -> Option<&SemanticActionText> {
        match &self.recipe {
            NativeRecipe::Fill(value) => Some(value),
            _ => None,
        }
    }

    /// Private stable option target for a select recipe.
    pub const fn option(&self) -> Option<SemanticActionNativeTargetId> {
        match self.recipe {
            NativeRecipe::Select(option) => Some(option),
            _ => None,
        }
    }

    /// Fixed key for a press recipe.
    pub const fn press_key(&self) -> Option<SemanticPressKey> {
        match self.recipe {
            NativeRecipe::Press(key) => Some(key),
            _ => None,
        }
    }

    /// Fixed direction and magnitude for a scroll recipe.
    pub const fn scroll_recipe(&self) -> Option<(SemanticScrollDirection, SemanticScrollAmount)> {
        match self.recipe {
            NativeRecipe::Scroll(direction, amount) => Some((direction, amount)),
            _ => None,
        }
    }

    /// Native execution deadline including queue and backend work.
    pub const fn deadline(&self) -> SemanticActionExecutionInstant {
        self.correlation.deadline
    }

    /// Consumes the request into a claimed applied terminal.
    pub fn complete(
        self,
        backend: SemanticActionExecutionBackend,
        readiness: SemanticActionNativeReadiness,
        viewport: SemanticActionNativeViewport,
        actual_geometry: SemanticRect,
        revalidated_at: SemanticActionExecutionInstant,
        completed_at: SemanticActionExecutionInstant,
    ) -> SemanticActionNativeSettlement {
        SemanticActionNativeSettlement {
            correlation: self.correlation,
            outcome: NativeOutcome::Applied(NativeApplied {
                kind: self.recipe.kind(),
                backend,
                readiness,
                viewport,
                actual_geometry,
                revalidated_at,
                completed_at,
            }),
        }
    }

    /// Consumes the request into one closed native refusal.
    pub fn fail(
        self,
        failure: SemanticActionNativeFailure,
        completed_at: SemanticActionExecutionInstant,
    ) -> SemanticActionNativeSettlement {
        SemanticActionNativeSettlement {
            correlation: self.correlation,
            outcome: NativeOutcome::Failed {
                failure,
                completed_at,
            },
        }
    }
}

impl fmt::Debug for SemanticActionNativeRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticActionNativeRequest")
            .field("effect", &self.correlation.effect)
            .field("attempt", &self.correlation.attempt)
            .field("frame", &self.correlation.frame)
            .field(
                "checkpoint_invocation",
                &self.correlation.checkpoint_invocation,
            )
            .field("checkpoint_snapshot", &self.correlation.checkpoint_snapshot)
            .field("target", &self.correlation.target)
            .field("target_role", &self.role)
            .field("expected_geometry", &self.expected_geometry)
            .field("kind", &self.kind())
            .field("fill_bytes", &self.fill_text().map(SemanticActionText::len))
            .field("has_option", &self.option().is_some())
            .field("press_key", &self.press_key())
            .field("scroll_recipe", &self.scroll_recipe())
            .field("deadline", &self.correlation.deadline)
            .field("guard", &"[redacted]")
            .finish()
    }
}

/// Closed refusal emitted by a backend after native admission.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticActionNativeFailure {
    /// The exact reference or frame authority is stale.
    #[error("native semantic action reference is stale")]
    StaleReference,
    /// Stable target semantics or operation inventory changed.
    #[error("native semantic action target changed")]
    TargetChanged,
    /// The target became disabled.
    #[error("native semantic action target is disabled")]
    TargetDisabled,
    /// The target crossed a credential boundary.
    #[error("native semantic action target crossed a credential boundary")]
    CredentialBoundary,
    /// Visibility or exact hit testing found an occluding target.
    #[error("native semantic action target is occluded")]
    TargetOccluded,
    /// No qualified fixed backend supports the exact interaction.
    #[error("native semantic action interaction is unsupported")]
    UnsupportedInteraction,
    /// This interaction requires explicit human control.
    #[error("native semantic action requires human control")]
    NeedsHuman,
    /// The renderer disappeared before terminal settlement.
    #[error("native semantic action renderer was lost")]
    RendererLost,
    /// The one native execution deadline elapsed.
    #[error("native semantic action timed out")]
    TimedOut,
    /// Exact run/context cancellation preempted native execution.
    #[error("native semantic action was cancelled")]
    Cancelled,
    /// A fixed queue, payload, or platform resource ceiling was full.
    #[error("native semantic action resources are exhausted")]
    ResourceExhausted,
    /// The selected fixed backend terminally refused its request.
    #[error("native semantic action transport failed")]
    Transport,
    /// Process teardown sealed the native adapter.
    #[error("native semantic action adapter is shutting down")]
    Shutdown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeApplied {
    kind: SemanticActionKind,
    backend: SemanticActionExecutionBackend,
    readiness: SemanticActionNativeReadiness,
    viewport: SemanticActionNativeViewport,
    actual_geometry: SemanticRect,
    revalidated_at: SemanticActionExecutionInstant,
    completed_at: SemanticActionExecutionInstant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeOutcome {
    Applied(NativeApplied),
    Failed {
        failure: SemanticActionNativeFailure,
        completed_at: SemanticActionExecutionInstant,
    },
}

/// Move-only native terminal awaiting its retained policy half.
#[must_use]
pub struct SemanticActionNativeSettlement {
    correlation: NativeCorrelation,
    outcome: NativeOutcome,
}

impl fmt::Debug for SemanticActionNativeSettlement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticActionNativeSettlement")
            .field("effect", &self.correlation.effect)
            .field("attempt", &self.correlation.attempt)
            .field("frame", &self.correlation.frame)
            .field("target", &self.correlation.target)
            .field("outcome", &self.outcome)
            .field("guard", &"[redacted]")
            .finish()
    }
}

impl SemanticActionNativeSettlement {
    pub(crate) const fn coordinator_key(&self) -> SemanticActionCoordinatorKey {
        self.correlation.coordinator_key()
    }
}

/// Applied fixed-backend terminal from which bounded settlement may begin.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticActionExecutionApplied {
    backend: SemanticActionExecutionBackend,
    readiness: SemanticActionNativeReadiness,
    viewport: SemanticActionNativeViewport,
    actual_geometry: SemanticRect,
    revalidated_at: SemanticActionExecutionInstant,
    completed_at: SemanticActionExecutionInstant,
}

impl SemanticActionExecutionApplied {
    /// Qualified fixed backend that accepted the exact recipe.
    pub const fn backend(self) -> SemanticActionExecutionBackend {
        self.backend
    }

    /// Exact pre-dispatch readiness class.
    pub const fn readiness(self) -> SemanticActionNativeReadiness {
        self.readiness
    }

    /// Native viewport sampled with readiness.
    pub const fn viewport(self) -> SemanticActionNativeViewport {
        self.viewport
    }

    /// Fresh target geometry sampled with readiness.
    pub const fn actual_geometry(self) -> SemanticRect {
        self.actual_geometry
    }

    /// Monotonic time at which native preconditions were revalidated.
    pub const fn revalidated_at(self) -> SemanticActionExecutionInstant {
        self.revalidated_at
    }

    /// Monotonic fixed-backend completion time.
    pub const fn completed_at(self) -> SemanticActionExecutionInstant {
        self.completed_at
    }

    /// Completion time in the settlement core's same shell clock domain.
    pub const fn settle_started_at(self) -> SemanticSettleInstant {
        SemanticSettleInstant::from_millis(self.completed_at.millis())
    }

    #[cfg(test)]
    pub(crate) fn for_action_metrics_test(
        backend: SemanticActionExecutionBackend,
        revalidated_at: SemanticActionExecutionInstant,
        completed_at: SemanticActionExecutionInstant,
    ) -> Self {
        Self {
            backend,
            readiness: SemanticActionNativeReadiness::ExactVisibleUnoccludedTarget,
            viewport: SemanticActionNativeViewport::try_new(800, 600).expect("test viewport"),
            actual_geometry: SemanticRect::try_new(10, 10, 20, 20).expect("test geometry"),
            revalidated_at,
            completed_at,
        }
    }
}

/// Terminal result of rejoining retained policy authority and native work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticActionExecutionDisposition {
    /// One fixed backend applied the request; this is not effect proof.
    Applied(SemanticActionExecutionApplied),
    /// Expected typed action failure.
    Failed(SemanticActionFailure),
    /// Native/shell substitution or malformed evidence violated the contract.
    ContractViolation(SemanticActionExecutionContractError),
}

impl SemanticActionExecutionDisposition {
    /// Failure safe to charge through the policy ledger, if not applied.
    ///
    /// Contract violations map to `BackendRefused` while remaining separately
    /// visible to the fail-stop shell.
    pub const fn policy_failure(self) -> Option<SemanticActionFailure> {
        match self {
            Self::Applied(_) => None,
            Self::Failed(failure) => Some(failure),
            Self::ContractViolation(_) => Some(SemanticActionFailure::BackendRefused),
        }
    }
}

/// Native execution outcome that never loses its dispatched policy authority.
#[must_use]
pub struct SemanticActionExecutionOutcome {
    active: AgentActiveEffect,
    disposition: SemanticActionExecutionDisposition,
}

impl SemanticActionExecutionOutcome {
    /// Dispatched policy effect retained for exact verified/failed settlement.
    pub const fn active(&self) -> &AgentActiveEffect {
        &self.active
    }

    /// Applied, typed-failed, or contract-violation disposition.
    pub const fn disposition(&self) -> SemanticActionExecutionDisposition {
        self.disposition
    }

    /// Consumes the outcome into policy authority and content-free disposition.
    pub(crate) fn into_parts(self) -> (AgentActiveEffect, SemanticActionExecutionDisposition) {
        (self.active, self.disposition)
    }
}

impl fmt::Debug for SemanticActionExecutionOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticActionExecutionOutcome")
            .field("active", &"[redacted]")
            .field("disposition", &self.disposition)
            .finish()
    }
}

/// Exact post-execution authority and settlement state for one applied action.
///
/// This owner can be minted only by consuming a complete native execution
/// outcome. It prevents an applied timestamp/backend fact copied from another
/// attempt from starting settlement under unrelated policy authority.
#[must_use]
pub struct SemanticActionSettlementStart {
    key: SemanticActionCoordinatorKey,
    active: AgentActiveEffect,
    execution: SemanticActionExecutionApplied,
    tracker: SemanticSettleTracker,
}

impl SemanticActionSettlementStart {
    pub(crate) const fn coordinator_key(&self) -> SemanticActionCoordinatorKey {
        self.key
    }

    /// Exact dispatched policy authority retained through settlement.
    pub const fn active(&self) -> &AgentActiveEffect {
        &self.active
    }

    /// Content-free fixed-backend attribution and execution timing.
    pub const fn execution(&self) -> SemanticActionExecutionApplied {
        self.execution
    }

    /// Exact bounded settlement tracker.
    pub const fn tracker(&self) -> &SemanticSettleTracker {
        &self.tracker
    }

    pub(crate) const fn tracker_mut(&mut self) -> &mut SemanticSettleTracker {
        &mut self.tracker
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        AgentActiveEffect,
        SemanticActionExecutionApplied,
        SemanticSettleTracker,
    ) {
        (self.active, self.execution, self.tracker)
    }
}

impl fmt::Debug for SemanticActionSettlementStart {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticActionSettlementStart")
            .field("active", &"[redacted]")
            .field("execution", &self.execution)
            .field("tracker", &self.tracker)
            .finish()
    }
}

/// Failed native-to-settlement transition that returns policy authority.
#[must_use]
pub struct SemanticActionSettlementRefusal {
    active: Box<AgentActiveEffect>,
    error: SemanticActionSettlementStartError,
}

impl SemanticActionSettlementRefusal {
    /// Closed reason settlement did not begin.
    pub const fn error(&self) -> SemanticActionSettlementStartError {
        self.error
    }

    /// Closed action failure derived from this exact consuming refusal.
    pub const fn action_failure(&self) -> SemanticActionFailure {
        self.error.action_failure()
    }

    /// Recovers policy authority for one terminal charged settlement.
    pub(crate) fn into_parts(self) -> (AgentActiveEffect, SemanticActionSettlementStartError) {
        (*self.active, self.error)
    }
}

impl fmt::Debug for SemanticActionSettlementRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticActionSettlementRefusal")
            .field("active", &"[redacted]")
            .field("error", &self.error)
            .finish()
    }
}

/// Closed refusal before bounded post-action settlement begins.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticActionSettlementStartError {
    /// Supplied prepared action did not match retained policy authority.
    #[error("semantic action settlement authority mismatched")]
    AuthorityMismatch,
    /// Native execution ended with an expected typed action failure.
    #[error("semantic action execution failed before settlement: {0}")]
    ExecutionFailed(SemanticActionFailure),
    /// Native execution evidence violated its exact one-shot contract.
    #[error("semantic action execution contract failed before settlement: {0}")]
    ExecutionContract(SemanticActionExecutionContractError),
    /// The settlement tracker refused its clock/deadline contract.
    #[error("semantic action settlement could not start: {0}")]
    Settlement(SemanticSettleError),
}

impl SemanticActionSettlementStartError {
    /// Maps a failed native-to-settlement transition into the closed action taxonomy.
    pub const fn action_failure(self) -> SemanticActionFailure {
        match self {
            Self::ExecutionFailed(failure) => failure,
            Self::AuthorityMismatch | Self::ExecutionContract(_) | Self::Settlement(_) => {
                SemanticActionFailure::BackendRefused
            }
        }
    }
}

/// Consumes one exact native execution outcome into bounded settlement state.
///
/// Failed or malformed execution never creates a tracker and always returns
/// the still-dispatched policy authority. This function allocates no worker,
/// timer, queue, native object, or content buffer.
pub fn begin_semantic_action_settlement(
    outcome: SemanticActionExecutionOutcome,
    action: &SemanticPreparedAction,
) -> Result<SemanticActionSettlementStart, SemanticActionSettlementRefusal> {
    let (active, disposition) = outcome.into_parts();
    let error = if !active.matches_action(action) {
        SemanticActionSettlementStartError::AuthorityMismatch
    } else {
        match disposition {
            SemanticActionExecutionDisposition::Applied(applied) => {
                match SemanticSettleTracker::begin(
                    active.attempt(),
                    action,
                    applied.settle_started_at(),
                ) {
                    Ok(tracker) => {
                        let key = SemanticActionCoordinatorKey {
                            effect: active.id(),
                            attempt: active.attempt(),
                            context: action.frame().context().identity(),
                            guard: action.verification_guard(),
                        };
                        return Ok(SemanticActionSettlementStart {
                            key,
                            active,
                            execution: applied,
                            tracker,
                        });
                    }
                    Err(error) => SemanticActionSettlementStartError::Settlement(error),
                }
            }
            SemanticActionExecutionDisposition::Failed(failure) => {
                SemanticActionSettlementStartError::ExecutionFailed(failure)
            }
            SemanticActionExecutionDisposition::ContractViolation(error) => {
                SemanticActionSettlementStartError::ExecutionContract(error)
            }
        }
    };
    Err(SemanticActionSettlementRefusal {
        active: Box::new(active),
        error,
    })
}

/// Preparation failure that returns the still-dispatched policy authority.
#[must_use]
pub(crate) struct SemanticActionExecutionRefusal {
    active: Box<AgentActiveEffect>,
    error: SemanticActionExecutionPreparationError,
}

impl SemanticActionExecutionRefusal {
    /// Closed preparation refusal.
    #[cfg(test)]
    pub(crate) const fn error(&self) -> SemanticActionExecutionPreparationError {
        self.error
    }

    /// Recovers authority so policy can settle the dispatched effect as failed.
    pub(crate) fn into_parts(self) -> (AgentActiveEffect, SemanticActionExecutionPreparationError) {
        (*self.active, self.error)
    }
}

impl fmt::Debug for SemanticActionExecutionRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticActionExecutionRefusal")
            .field("active", &"[redacted]")
            .field("error", &self.error)
            .finish()
    }
}

/// Refusal before a request enters a native adapter.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticActionExecutionPreparationError {
    /// Policy-dispatched effect and prepared action do not share one guard.
    #[error("semantic action execution policy authority mismatched")]
    AuthorityMismatch,
    /// The exact pre-execution snapshot supplied no actionable geometry.
    #[error("semantic action execution geometry is absent or empty")]
    InvalidGeometry,
    /// The execution deadline overflowed the shell clock domain.
    #[error("semantic action execution deadline overflowed")]
    DeadlineOverflow,
    /// The prepared action's closed recipe was internally inconsistent.
    #[error("semantic action execution recipe is inconsistent")]
    Invariant,
}

impl SemanticActionExecutionPreparationError {
    /// Maps exact pre-native preparation failure into the closed action taxonomy.
    pub const fn action_failure(self) -> SemanticActionFailure {
        match self {
            Self::InvalidGeometry => SemanticActionFailure::TargetChanged,
            Self::AuthorityMismatch | Self::DeadlineOverflow | Self::Invariant => {
                SemanticActionFailure::BackendRefused
            }
        }
    }
}

/// Contract violation while admitting a claimed native terminal.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticActionExecutionContractError {
    /// Native terminal did not belong to the retained exact request.
    #[error("semantic action native request mismatched")]
    RequestMismatch,
    /// Native evidence time regressed before request admission.
    #[error("semantic action native clock regressed")]
    ClockRegression,
    /// A timeout was claimed before the request's absolute deadline.
    #[error("semantic action native timeout was premature")]
    PrematureTimeout,
    /// Native geometry contradicted its claimed visibility/readiness class.
    #[error("semantic action native geometry is invalid")]
    InvalidGeometry,
    /// Readiness evidence was incompatible with the fixed action class.
    #[error("semantic action native readiness is incompatible")]
    ReadinessMismatch,
}

/// Splits one policy-dispatched prepared action into retained/native halves.
///
/// The function clones at most one already-bounded 4 KiB fill value into the
/// native half. No work, task, timer, queue, or native object exists until a
/// caller explicitly dispatches the returned request.
pub(crate) fn prepare_semantic_action_execution(
    active: AgentActiveEffect,
    action: &SemanticPreparedAction,
    requested_at: SemanticActionExecutionInstant,
) -> Result<
    (SemanticActionExecutionPending, SemanticActionNativeRequest),
    SemanticActionExecutionRefusal,
> {
    if !active.matches_action(action) {
        return Err(SemanticActionExecutionRefusal {
            active: Box::new(active),
            error: SemanticActionExecutionPreparationError::AuthorityMismatch,
        });
    }
    let Some(expected_geometry) = action.target_geometry().filter(|rect| nonempty_rect(*rect))
    else {
        return Err(SemanticActionExecutionRefusal {
            active: Box::new(active),
            error: SemanticActionExecutionPreparationError::InvalidGeometry,
        });
    };
    let execution_millis = action
        .settle_budget()
        .millis()
        .min(MAX_SEMANTIC_ACTION_NATIVE_EXECUTION_MILLIS);
    let Some(deadline) = requested_at.checked_add(execution_millis) else {
        return Err(SemanticActionExecutionRefusal {
            active: Box::new(active),
            error: SemanticActionExecutionPreparationError::DeadlineOverflow,
        });
    };
    let recipe = match action.kind() {
        SemanticActionKind::Click => NativeRecipe::Click,
        SemanticActionKind::Fill => match action.fill_text().cloned() {
            Some(value) => NativeRecipe::Fill(value),
            None => {
                return Err(SemanticActionExecutionRefusal {
                    active: Box::new(active),
                    error: SemanticActionExecutionPreparationError::Invariant,
                });
            }
        },
        SemanticActionKind::Select => match action.option_key() {
            Some(option) => NativeRecipe::Select(SemanticActionNativeTargetId::from_key(option)),
            None => {
                return Err(SemanticActionExecutionRefusal {
                    active: Box::new(active),
                    error: SemanticActionExecutionPreparationError::Invariant,
                });
            }
        },
        SemanticActionKind::Press => match action.bound_action().press_key() {
            Some(key) => NativeRecipe::Press(key),
            None => {
                return Err(SemanticActionExecutionRefusal {
                    active: Box::new(active),
                    error: SemanticActionExecutionPreparationError::Invariant,
                });
            }
        },
        SemanticActionKind::Scroll => match action.scroll_recipe() {
            Some((direction, amount)) => NativeRecipe::Scroll(direction, amount),
            None => {
                return Err(SemanticActionExecutionRefusal {
                    active: Box::new(active),
                    error: SemanticActionExecutionPreparationError::Invariant,
                });
            }
        },
    };
    let target = SemanticActionNativeTargetId::from_key(action.target_key());
    let guard = execution_guard(
        &active,
        action,
        target,
        expected_geometry,
        requested_at,
        deadline,
    );
    let correlation = NativeCorrelation {
        effect: active.id(),
        attempt: active.attempt(),
        frame: action.frame().clone(),
        checkpoint_invocation: action.checkpoint_invocation(),
        checkpoint_snapshot: action.checkpoint_snapshot(),
        target,
        requested_at,
        deadline,
        guard,
    };
    Ok((
        SemanticActionExecutionPending {
            correlation: correlation.clone(),
            active,
        },
        SemanticActionNativeRequest {
            correlation,
            role: action.bound_action().target_role(),
            expected_geometry,
            recipe,
        },
    ))
}

fn execution_guard(
    active: &AgentActiveEffect,
    action: &SemanticPreparedAction,
    target: SemanticActionNativeTargetId,
    geometry: SemanticRect,
    requested_at: SemanticActionExecutionInstant,
    deadline: SemanticActionExecutionInstant,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"ZEPHIUM-SEMANTIC-NATIVE-EXECUTION-1\0");
    hasher.update(active.id().get().to_be_bytes());
    hasher.update(active.attempt().get().to_be_bytes());
    hasher.update(action.verification_guard());
    hasher.update(target.get().to_be_bytes());
    hasher.update(geometry.x().to_be_bytes());
    hasher.update(geometry.y().to_be_bytes());
    hasher.update(geometry.width().to_be_bytes());
    hasher.update(geometry.height().to_be_bytes());
    hasher.update(requested_at.millis().to_be_bytes());
    hasher.update(deadline.millis().to_be_bytes());
    hasher.finalize().into()
}

fn admit_native_outcome(
    outcome: NativeOutcome,
    requested_at: SemanticActionExecutionInstant,
    deadline: SemanticActionExecutionInstant,
) -> SemanticActionExecutionDisposition {
    match outcome {
        NativeOutcome::Failed {
            failure,
            completed_at,
        } => {
            if completed_at < requested_at {
                return SemanticActionExecutionDisposition::ContractViolation(
                    SemanticActionExecutionContractError::ClockRegression,
                );
            }
            if completed_at > deadline {
                return SemanticActionExecutionDisposition::Failed(SemanticActionFailure::Timeout);
            }
            if failure == SemanticActionNativeFailure::TimedOut && completed_at < deadline {
                return SemanticActionExecutionDisposition::ContractViolation(
                    SemanticActionExecutionContractError::PrematureTimeout,
                );
            }
            SemanticActionExecutionDisposition::Failed(map_native_failure(failure))
        }
        NativeOutcome::Applied(applied) => {
            if applied.revalidated_at < requested_at
                || applied.completed_at < applied.revalidated_at
            {
                return SemanticActionExecutionDisposition::ContractViolation(
                    SemanticActionExecutionContractError::ClockRegression,
                );
            }
            if applied.completed_at > deadline {
                return SemanticActionExecutionDisposition::Failed(SemanticActionFailure::Timeout);
            }
            if !nonempty_rect(applied.actual_geometry) {
                return SemanticActionExecutionDisposition::ContractViolation(
                    SemanticActionExecutionContractError::InvalidGeometry,
                );
            }
            let readiness_matches = match (applied.kind, applied.readiness) {
                (
                    SemanticActionKind::Click
                    | SemanticActionKind::Fill
                    | SemanticActionKind::Select
                    | SemanticActionKind::Press,
                    SemanticActionNativeReadiness::ExactVisibleUnoccludedTarget,
                ) => rect_intersects_viewport(applied.actual_geometry, applied.viewport),
                (
                    SemanticActionKind::Scroll,
                    SemanticActionNativeReadiness::ExactConnectedScrollTarget,
                ) => true,
                _ => {
                    return SemanticActionExecutionDisposition::ContractViolation(
                        SemanticActionExecutionContractError::ReadinessMismatch,
                    );
                }
            };
            if !readiness_matches {
                return SemanticActionExecutionDisposition::ContractViolation(
                    SemanticActionExecutionContractError::InvalidGeometry,
                );
            }
            SemanticActionExecutionDisposition::Applied(SemanticActionExecutionApplied {
                backend: applied.backend,
                readiness: applied.readiness,
                viewport: applied.viewport,
                actual_geometry: applied.actual_geometry,
                revalidated_at: applied.revalidated_at,
                completed_at: applied.completed_at,
            })
        }
    }
}

const fn nonempty_rect(rect: SemanticRect) -> bool {
    rect.width() > 0 && rect.height() > 0
}

fn rect_intersects_viewport(rect: SemanticRect, viewport: SemanticActionNativeViewport) -> bool {
    let left = i64::from(rect.x());
    let top = i64::from(rect.y());
    let right = left + i64::from(rect.width());
    let bottom = top + i64::from(rect.height());
    right > 0
        && bottom > 0
        && left < i64::from(viewport.width())
        && top < i64::from(viewport.height())
}

const fn map_native_failure(failure: SemanticActionNativeFailure) -> SemanticActionFailure {
    match failure {
        SemanticActionNativeFailure::StaleReference => SemanticActionFailure::StaleReference,
        SemanticActionNativeFailure::TargetChanged => SemanticActionFailure::TargetChanged,
        SemanticActionNativeFailure::TargetDisabled => SemanticActionFailure::TargetDisabled,
        SemanticActionNativeFailure::CredentialBoundary => {
            SemanticActionFailure::CredentialBoundary
        }
        SemanticActionNativeFailure::TargetOccluded => SemanticActionFailure::TargetOccluded,
        SemanticActionNativeFailure::UnsupportedInteraction => {
            SemanticActionFailure::UnsupportedInteraction
        }
        SemanticActionNativeFailure::NeedsHuman => SemanticActionFailure::NeedsHuman,
        SemanticActionNativeFailure::RendererLost => SemanticActionFailure::RendererLost,
        SemanticActionNativeFailure::TimedOut => SemanticActionFailure::Timeout,
        SemanticActionNativeFailure::Cancelled => SemanticActionFailure::Cancelled,
        SemanticActionNativeFailure::ResourceExhausted => SemanticActionFailure::ResourceExhausted,
        SemanticActionNativeFailure::Transport | SemanticActionNativeFailure::Shutdown => {
            SemanticActionFailure::BackendRefused
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        decode_semantic_snapshot, ContextCapabilities, ContextCapability, ContextId,
        ContextIdentity, ContextKind, ContextOperationId, ContextRegistry, ContextRunId,
        ContextSettlement, FrameGeneration, FrameId, SemanticActionBatch, SemanticActionBatchId,
        SemanticActionIntent, SemanticActionProposal, SemanticEffectClass, SemanticFrameTrust,
        SemanticObservation, SemanticObservationAssembler, SemanticObservationBudget,
        SemanticObservationId, SemanticOrigin, SemanticReferenceId, SemanticSettleBudget,
        SemanticState, SemanticVerification, SemanticWaitCondition, SEMANTIC_WIRE_VERSION,
    };
    use serde_json::json;
    use zephium_core::ids::ProfileId;

    fn observation(seed: u128, nodes: serde_json::Value) -> SemanticObservation {
        let identity = ContextIdentity::new(
            ContextId::from_raw(seed + 1),
            ContextRunId::from_raw(seed + 2),
            ProfileId::from(seed + 3),
            ContextKind::Owned,
        );
        let capabilities = ContextCapabilities::try_new(
            ContextKind::Owned,
            &[ContextCapability::Observe, ContextCapability::Act],
        )
        .expect("capabilities");
        let mut registry = ContextRegistry::new();
        registry.reserve(identity, capabilities).expect("reserve");
        let construction = registry
            .begin_context(
                identity.id(),
                ContextOperationId::new(1).expect("operation"),
            )
            .expect("construct");
        registry
            .settle_construction(identity.id(), construction, ContextSettlement::Applied)
            .expect("settle");
        let context = registry.join(identity.id()).expect("join");
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            FrameGeneration::INITIAL,
            SemanticOrigin::parse("https://native-execution-private.example.test/path")
                .expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        let bytes = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": 7,
            "g": 9,
            "c": "complete",
            "n": nodes,
        }))
        .expect("wire");
        let snapshot = decode_semantic_snapshot(
            crate::SemanticDecodeContext::new(
                SemanticInvocationId::new(7).expect("invocation"),
                frame,
                SemanticSnapshotGeneration::new(9).expect("generation"),
            ),
            &bytes,
        )
        .expect("snapshot");
        let request = crate::SemanticObservationRequest::initial(
            SemanticObservationId::new(1).expect("observation"),
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        SemanticObservationAssembler::new(request, snapshot)
            .expect("assembler")
            .finish()
            .expect("observation")
    }

    fn button_observation(seed: u128, geometry: bool) -> SemanticObservation {
        let button = if geometry {
            json!({
                "k": 2, "p": 0, "r": "button", "n": "Private action label", "o": 9,
                "b": {"x": 10, "y": 20, "w": 100, "h": 30}
            })
        } else {
            json!({
                "k": 2, "p": 0, "r": "button", "n": "Private action label", "o": 9
            })
        };
        observation(
            seed,
            json!([
                {"k": 1, "r": "document", "o": 16},
                button
            ]),
        )
    }

    fn proposal(
        intent: SemanticActionIntent,
        effect: SemanticEffectClass,
        verification: SemanticVerification,
    ) -> SemanticActionProposal {
        SemanticActionProposal::try_new(
            intent,
            effect,
            SemanticWaitCondition::Immediate,
            verification,
            SemanticSettleBudget::try_new(250).expect("settle budget"),
        )
        .expect("proposal")
    }

    fn prepared(
        observation: &SemanticObservation,
        batch: u64,
        proposal: SemanticActionProposal,
    ) -> SemanticPreparedAction {
        let action = SemanticActionBatch::bind(
            SemanticActionBatchId::new(batch).expect("batch"),
            observation,
            &[observation.frames()[0].frame().clone()],
            vec![proposal],
        )
        .expect("bind");
        action.actions()[0]
            .prepare(&observation.frames()[0])
            .expect("prepare")
    }

    fn click_action(observation: &SemanticObservation, batch: u64) -> SemanticPreparedAction {
        prepared(
            observation,
            batch,
            proposal(
                SemanticActionIntent::Click {
                    target: SemanticReferenceId::new(2).expect("target"),
                },
                SemanticEffectClass::Read,
                SemanticVerification::TargetState {
                    state: SemanticState::Focused,
                    present: true,
                },
            ),
        )
    }

    fn active(action: &SemanticPreparedAction, attempt: u64) -> AgentActiveEffect {
        AgentActiveEffect::for_execution_test(
            action,
            SemanticActionAttemptId::new(attempt).expect("attempt"),
        )
    }

    fn viewport() -> SemanticActionNativeViewport {
        SemanticActionNativeViewport::try_new(800, 600).expect("viewport")
    }

    fn rect(x: i32, y: i32, width: u32, height: u32) -> SemanticRect {
        SemanticRect::try_new(x, y, width, height).expect("geometry")
    }

    #[test]
    fn exact_applied_terminal_rejoins_policy_authority_before_settlement() {
        let observation = button_observation(10, true);
        let action = click_action(&observation, 1);
        let attempt = SemanticActionAttemptId::new(11).expect("attempt");
        let active = AgentActiveEffect::for_execution_test(&action, attempt);
        let (pending, native) = prepare_semantic_action_execution(
            active,
            &action,
            SemanticActionExecutionInstant::from_millis(1_000),
        )
        .expect("execution");
        assert_eq!(pending.attempt(), attempt);
        assert_eq!(pending.deadline().millis(), 1_250);
        assert_eq!(native.target().get(), 2);
        assert_eq!(native.kind(), SemanticActionKind::Click);
        assert_eq!(native.expected_geometry(), rect(10, 20, 100, 30));

        let settlement = native.complete(
            SemanticActionExecutionBackend::FixedSemanticRecipe,
            SemanticActionNativeReadiness::ExactVisibleUnoccludedTarget,
            viewport(),
            rect(12, 22, 100, 30),
            SemanticActionExecutionInstant::from_millis(1_010),
            SemanticActionExecutionInstant::from_millis(1_020),
        );
        let outcome = pending.settle(action.frame(), settlement);
        let start = begin_semantic_action_settlement(outcome, &action).expect("settlement start");
        assert_eq!(start.active().attempt(), attempt);
        let applied = start.execution();
        assert_eq!(
            applied.backend(),
            SemanticActionExecutionBackend::FixedSemanticRecipe
        );
        assert_eq!(applied.completed_at().millis(), 1_020);
        assert_eq!(applied.settle_started_at().millis(), 1_020);
        assert_eq!(start.tracker().deadline().millis(), 1_270);
        assert_eq!(
            start.tracker().status(),
            crate::SemanticSettleStatus::ReadyForVerification
        );
    }

    #[test]
    fn settlement_start_rejects_action_substitution_and_returns_policy_authority() {
        let observation = button_observation(15, true);
        let original = click_action(&observation, 15);
        let substituted = click_action(&observation, 16);
        let (pending, native) = prepare_semantic_action_execution(
            active(&original, 16),
            &original,
            SemanticActionExecutionInstant::from_millis(1_000),
        )
        .expect("execution");
        let outcome = pending.settle(
            original.frame(),
            native.complete(
                SemanticActionExecutionBackend::FixedSemanticRecipe,
                SemanticActionNativeReadiness::ExactVisibleUnoccludedTarget,
                viewport(),
                rect(10, 20, 100, 30),
                SemanticActionExecutionInstant::from_millis(1_010),
                SemanticActionExecutionInstant::from_millis(1_020),
            ),
        );
        let refusal = begin_semantic_action_settlement(outcome, &substituted)
            .expect_err("substituted action");
        assert_eq!(
            refusal.action_failure(),
            SemanticActionFailure::BackendRefused
        );
        let (returned, error) = refusal.into_parts();
        assert_eq!(returned.attempt().get(), 16);
        assert_eq!(error, SemanticActionSettlementStartError::AuthorityMismatch);
    }

    #[test]
    fn failed_or_overflowing_execution_never_starts_settlement() {
        let observation = button_observation(17, true);
        let action = click_action(&observation, 17);
        let (pending, native) = prepare_semantic_action_execution(
            active(&action, 17),
            &action,
            SemanticActionExecutionInstant::from_millis(1_000),
        )
        .expect("execution");
        let outcome = pending.settle(
            action.frame(),
            native.fail(
                SemanticActionNativeFailure::TargetOccluded,
                SemanticActionExecutionInstant::from_millis(1_010),
            ),
        );
        let refusal =
            begin_semantic_action_settlement(outcome, &action).expect_err("failed execution");
        assert_eq!(
            refusal.action_failure(),
            SemanticActionFailure::TargetOccluded
        );
        let (returned, error) = refusal.into_parts();
        assert_eq!(returned.attempt().get(), 17);
        assert_eq!(
            error,
            SemanticActionSettlementStartError::ExecutionFailed(
                SemanticActionFailure::TargetOccluded,
            )
        );

        let requested_at = SemanticActionExecutionInstant::from_millis(u64::MAX - 250);
        let (pending, native) =
            prepare_semantic_action_execution(active(&action, 18), &action, requested_at)
                .expect("late execution");
        let completed_at = SemanticActionExecutionInstant::from_millis(u64::MAX);
        let outcome = pending.settle(
            action.frame(),
            native.complete(
                SemanticActionExecutionBackend::FixedSemanticRecipe,
                SemanticActionNativeReadiness::ExactVisibleUnoccludedTarget,
                viewport(),
                rect(10, 20, 100, 30),
                requested_at,
                completed_at,
            ),
        );
        let refusal = begin_semantic_action_settlement(outcome, &action)
            .expect_err("settlement deadline overflow");
        let (returned, error) = refusal.into_parts();
        assert_eq!(returned.attempt().get(), 18);
        assert_eq!(
            error,
            SemanticActionSettlementStartError::Settlement(SemanticSettleError::DeadlineOverflow)
        );
    }

    #[test]
    fn execution_contract_violation_returns_authority_without_a_tracker() {
        let observation = button_observation(19, true);
        let action = click_action(&observation, 19);
        let (first_pending, first_native) = prepare_semantic_action_execution(
            active(&action, 19),
            &action,
            SemanticActionExecutionInstant::from_millis(1_000),
        )
        .expect("first execution");
        let (second_pending, second_native) = prepare_semantic_action_execution(
            active(&action, 20),
            &action,
            SemanticActionExecutionInstant::from_millis(1_000),
        )
        .expect("second execution");
        drop(first_native);

        let outcome = first_pending.settle(
            action.frame(),
            second_native.fail(
                SemanticActionNativeFailure::Transport,
                SemanticActionExecutionInstant::from_millis(1_010),
            ),
        );
        let refusal =
            begin_semantic_action_settlement(outcome, &action).expect_err("cross-request terminal");
        let (returned, error) = refusal.into_parts();
        assert_eq!(returned.attempt().get(), 19);
        assert_eq!(
            error,
            SemanticActionSettlementStartError::ExecutionContract(
                SemanticActionExecutionContractError::RequestMismatch
            )
        );
        assert_eq!(
            second_pending
                .refuse(SemanticActionFailure::Cancelled)
                .active()
                .attempt()
                .get(),
            20
        );
    }

    #[test]
    fn preparation_refusals_return_dispatched_policy_authority() {
        let without_geometry = button_observation(20, false);
        let action = click_action(&without_geometry, 2);
        let refusal = prepare_semantic_action_execution(
            active(&action, 21),
            &action,
            SemanticActionExecutionInstant::from_millis(1_000),
        )
        .expect_err("missing geometry");
        let (returned, error) = refusal.into_parts();
        assert_eq!(returned.attempt().get(), 21);
        assert_eq!(
            error,
            SemanticActionExecutionPreparationError::InvalidGeometry
        );

        let with_geometry = button_observation(30, true);
        let first = click_action(&with_geometry, 3);
        let second = click_action(&with_geometry, 4);
        let refusal = prepare_semantic_action_execution(
            active(&first, 22),
            &second,
            SemanticActionExecutionInstant::from_millis(1_000),
        )
        .expect_err("action substitution");
        assert_eq!(
            refusal.error(),
            SemanticActionExecutionPreparationError::AuthorityMismatch
        );

        let refusal = prepare_semantic_action_execution(
            refusal.into_parts().0,
            &first,
            SemanticActionExecutionInstant::from_millis(u64::MAX - 10),
        )
        .expect_err("deadline overflow");
        assert_eq!(
            refusal.error(),
            SemanticActionExecutionPreparationError::DeadlineOverflow
        );
    }

    #[test]
    fn cross_request_native_terminal_is_contract_violation_and_never_loses_authority() {
        let observation = button_observation(40, true);
        let action = click_action(&observation, 5);
        let (first_pending, first_native) = prepare_semantic_action_execution(
            active(&action, 31),
            &action,
            SemanticActionExecutionInstant::from_millis(1_000),
        )
        .expect("first");
        let (second_pending, second_native) = prepare_semantic_action_execution(
            active(&action, 32),
            &action,
            SemanticActionExecutionInstant::from_millis(1_000),
        )
        .expect("second");
        drop(first_native);
        let substituted = second_native.fail(
            SemanticActionNativeFailure::Transport,
            SemanticActionExecutionInstant::from_millis(1_010),
        );
        let outcome = first_pending.settle(action.frame(), substituted);
        assert_eq!(outcome.active().attempt().get(), 31);
        assert_eq!(
            outcome.disposition(),
            SemanticActionExecutionDisposition::ContractViolation(
                SemanticActionExecutionContractError::RequestMismatch
            )
        );
        assert_eq!(
            outcome.disposition().policy_failure(),
            Some(SemanticActionFailure::BackendRefused)
        );
        assert_eq!(
            second_pending
                .refuse(SemanticActionFailure::Cancelled)
                .active()
                .attempt()
                .get(),
            32
        );
    }

    #[test]
    fn time_readiness_geometry_and_context_claims_fail_closed() {
        let observation = button_observation(50, true);
        let action = click_action(&observation, 6);

        let (pending, native) = prepare_semantic_action_execution(
            active(&action, 41),
            &action,
            SemanticActionExecutionInstant::from_millis(1_000),
        )
        .expect("execution");
        let regressed = native.complete(
            SemanticActionExecutionBackend::FixedSemanticRecipe,
            SemanticActionNativeReadiness::ExactVisibleUnoccludedTarget,
            viewport(),
            rect(10, 20, 100, 30),
            SemanticActionExecutionInstant::from_millis(999),
            SemanticActionExecutionInstant::from_millis(1_010),
        );
        assert_eq!(
            pending.settle(action.frame(), regressed).disposition(),
            SemanticActionExecutionDisposition::ContractViolation(
                SemanticActionExecutionContractError::ClockRegression
            )
        );

        let (pending, native) = prepare_semantic_action_execution(
            active(&action, 42),
            &action,
            SemanticActionExecutionInstant::from_millis(1_000),
        )
        .expect("execution");
        let late = native.complete(
            SemanticActionExecutionBackend::FixedSemanticRecipe,
            SemanticActionNativeReadiness::ExactVisibleUnoccludedTarget,
            viewport(),
            rect(10, 20, 100, 30),
            SemanticActionExecutionInstant::from_millis(1_200),
            SemanticActionExecutionInstant::from_millis(1_251),
        );
        assert_eq!(
            pending.settle(action.frame(), late).disposition(),
            SemanticActionExecutionDisposition::Failed(SemanticActionFailure::Timeout)
        );

        let (pending, native) = prepare_semantic_action_execution(
            active(&action, 43),
            &action,
            SemanticActionExecutionInstant::from_millis(1_000),
        )
        .expect("execution");
        let offscreen = native.complete(
            SemanticActionExecutionBackend::FixedSemanticRecipe,
            SemanticActionNativeReadiness::ExactVisibleUnoccludedTarget,
            viewport(),
            rect(900, 20, 100, 30),
            SemanticActionExecutionInstant::from_millis(1_010),
            SemanticActionExecutionInstant::from_millis(1_020),
        );
        assert_eq!(
            pending.settle(action.frame(), offscreen).disposition(),
            SemanticActionExecutionDisposition::ContractViolation(
                SemanticActionExecutionContractError::InvalidGeometry
            )
        );

        let (pending, native) = prepare_semantic_action_execution(
            active(&action, 44),
            &action,
            SemanticActionExecutionInstant::from_millis(1_000),
        )
        .expect("execution");
        let wrong_readiness = native.complete(
            SemanticActionExecutionBackend::FixedSemanticRecipe,
            SemanticActionNativeReadiness::ExactConnectedScrollTarget,
            viewport(),
            rect(10, 20, 100, 30),
            SemanticActionExecutionInstant::from_millis(1_010),
            SemanticActionExecutionInstant::from_millis(1_020),
        );
        assert_eq!(
            pending
                .settle(action.frame(), wrong_readiness)
                .disposition(),
            SemanticActionExecutionDisposition::ContractViolation(
                SemanticActionExecutionContractError::ReadinessMismatch
            )
        );

        let same_context_wrong_frame = SemanticFrameJoin::try_new(
            action.frame().context(),
            FrameId::new(77).expect("child frame"),
            FrameGeneration::INITIAL,
            action.frame().origin().clone(),
            action.frame().trust(),
        )
        .expect("same-context child frame");
        let (pending, native) = prepare_semantic_action_execution(
            active(&action, 45),
            &action,
            SemanticActionExecutionInstant::from_millis(1_000),
        )
        .expect("execution");
        let settlement = native.fail(
            SemanticActionNativeFailure::TargetOccluded,
            SemanticActionExecutionInstant::from_millis(1_020),
        );
        assert_eq!(
            pending
                .settle(&same_context_wrong_frame, settlement)
                .disposition(),
            SemanticActionExecutionDisposition::Failed(SemanticActionFailure::StaleReference)
        );

        let foreign = button_observation(60, true);
        let (pending, native) = prepare_semantic_action_execution(
            active(&action, 46),
            &action,
            SemanticActionExecutionInstant::from_millis(1_000),
        )
        .expect("execution");
        let settlement = native.fail(
            SemanticActionNativeFailure::TargetOccluded,
            SemanticActionExecutionInstant::from_millis(1_020),
        );
        assert_eq!(
            pending
                .settle(foreign.frames()[0].frame(), settlement)
                .disposition(),
            SemanticActionExecutionDisposition::Failed(SemanticActionFailure::StaleReference)
        );
    }

    #[test]
    fn native_failures_remain_typed_and_timeout_cannot_be_claimed_early() {
        let observation = button_observation(70, true);
        let action = click_action(&observation, 7);
        let (pending, native) = prepare_semantic_action_execution(
            active(&action, 51),
            &action,
            SemanticActionExecutionInstant::from_millis(1_000),
        )
        .expect("execution");
        let occluded = native.fail(
            SemanticActionNativeFailure::TargetOccluded,
            SemanticActionExecutionInstant::from_millis(1_010),
        );
        assert_eq!(
            pending.settle(action.frame(), occluded).disposition(),
            SemanticActionExecutionDisposition::Failed(SemanticActionFailure::TargetOccluded)
        );

        let (pending, native) = prepare_semantic_action_execution(
            active(&action, 52),
            &action,
            SemanticActionExecutionInstant::from_millis(1_000),
        )
        .expect("execution");
        let premature = native.fail(
            SemanticActionNativeFailure::TimedOut,
            SemanticActionExecutionInstant::from_millis(1_249),
        );
        assert_eq!(
            pending.settle(action.frame(), premature).disposition(),
            SemanticActionExecutionDisposition::ContractViolation(
                SemanticActionExecutionContractError::PrematureTimeout
            )
        );

        let (pending, native) = prepare_semantic_action_execution(
            active(&action, 53),
            &action,
            SemanticActionExecutionInstant::from_millis(1_000),
        )
        .expect("execution");
        let timeout = native.fail(
            SemanticActionNativeFailure::TimedOut,
            SemanticActionExecutionInstant::from_millis(1_250),
        );
        assert_eq!(
            pending.settle(action.frame(), timeout).disposition(),
            SemanticActionExecutionDisposition::Failed(SemanticActionFailure::Timeout)
        );
    }

    #[test]
    fn every_fixed_recipe_is_closed_and_fill_diagnostics_are_redacted() {
        let observation = observation(
            80,
            json!([
                {"k": 1, "r": "document", "o": 16,
                 "b": {"x": 0, "y": 0, "w": 800, "h": 600}},
                {"k": 2, "p": 0, "r": "button", "n": "Private submit", "o": 9,
                 "b": {"x": 10, "y": 10, "w": 100, "h": 30}},
                {"k": 3, "p": 0, "r": "textbox", "n": "Private title",
                 "v": {"k": "text", "value": "old"}, "o": 11,
                 "b": {"x": 10, "y": 50, "w": 200, "h": 30}},
                {"k": 4, "p": 0, "r": "combobox", "n": "Private priority",
                 "v": {"k": "ordinal", "value": 0}, "o": 13,
                 "b": {"x": 10, "y": 90, "w": 200, "h": 30}},
                {"k": 5, "p": 3, "r": "option", "n": "Private high",
                 "v": {"k": "ordinal", "value": 1}, "o": 9,
                 "b": {"x": 10, "y": 120, "w": 200, "h": 30}}
            ]),
        );

        let click = click_action(&observation, 81);
        let fill = prepared(
            &observation,
            82,
            proposal(
                SemanticActionIntent::Fill {
                    target: SemanticReferenceId::new(3).expect("textbox"),
                    value: SemanticActionText::try_new("private replacement".to_owned())
                        .expect("text"),
                },
                SemanticEffectClass::LocalWrite,
                SemanticVerification::TargetValueMatchesInput,
            ),
        );
        let select = prepared(
            &observation,
            83,
            proposal(
                SemanticActionIntent::Select {
                    target: SemanticReferenceId::new(4).expect("combobox"),
                    option: SemanticReferenceId::new(5).expect("option"),
                },
                SemanticEffectClass::LocalWrite,
                SemanticVerification::TargetSelectionMatchesOption,
            ),
        );
        let press = prepared(
            &observation,
            84,
            proposal(
                SemanticActionIntent::Press {
                    target: SemanticReferenceId::new(3).expect("textbox"),
                    key: SemanticPressKey::Enter,
                },
                SemanticEffectClass::LocalWrite,
                SemanticVerification::TargetState {
                    state: SemanticState::Focused,
                    present: true,
                },
            ),
        );
        let scroll = prepared(
            &observation,
            85,
            proposal(
                SemanticActionIntent::Scroll {
                    target: SemanticReferenceId::new(1).expect("document"),
                    direction: SemanticScrollDirection::Down,
                    amount: SemanticScrollAmount::Page,
                },
                SemanticEffectClass::Read,
                SemanticVerification::ScrollPositionChanged,
            ),
        );

        let (_, click_native) = prepare_semantic_action_execution(
            active(&click, 61),
            &click,
            SemanticActionExecutionInstant::from_millis(1),
        )
        .expect("click");
        assert_eq!(click_native.kind(), SemanticActionKind::Click);

        let (_, fill_native) = prepare_semantic_action_execution(
            active(&fill, 62),
            &fill,
            SemanticActionExecutionInstant::from_millis(1),
        )
        .expect("fill");
        assert_eq!(
            fill_native.fill_text().map(SemanticActionText::as_str),
            Some("private replacement")
        );
        let debug = format!("{fill_native:?}");
        assert!(!debug.contains("private replacement"));
        assert!(!debug.contains("Private title"));
        assert!(!debug.contains("native-execution-private"));

        let (_, select_native) = prepare_semantic_action_execution(
            active(&select, 63),
            &select,
            SemanticActionExecutionInstant::from_millis(1),
        )
        .expect("select");
        assert_eq!(
            select_native
                .option()
                .map(SemanticActionNativeTargetId::get),
            Some(5)
        );

        let (_, press_native) = prepare_semantic_action_execution(
            active(&press, 64),
            &press,
            SemanticActionExecutionInstant::from_millis(1),
        )
        .expect("press");
        assert_eq!(press_native.press_key(), Some(SemanticPressKey::Enter));

        let (_, scroll_native) = prepare_semantic_action_execution(
            active(&scroll, 65),
            &scroll,
            SemanticActionExecutionInstant::from_millis(1),
        )
        .expect("scroll");
        assert_eq!(
            scroll_native.scroll_recipe(),
            Some((SemanticScrollDirection::Down, SemanticScrollAmount::Page))
        );
    }

    #[test]
    fn viewport_bounds_are_nonzero_and_fixed() {
        assert_eq!(
            SemanticActionNativeViewport::try_new(0, 1),
            Err(SemanticActionNativeViewportError::Invalid)
        );
        assert_eq!(
            SemanticActionNativeViewport::try_new(MAX_SEMANTIC_ACTION_VIEWPORT_DIMENSION + 1, 1,),
            Err(SemanticActionNativeViewportError::Invalid)
        );
        assert_eq!(viewport().width(), 800);
        assert_eq!(viewport().height(), 600);
    }
}
