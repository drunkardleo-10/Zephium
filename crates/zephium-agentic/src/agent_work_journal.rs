//! Content-free durable Work facts. None of these values is execution authority.
//!
//! Restart never recreates a manifest, task predicate, credential, native port,
//! observation, approval permit or opaque reference. Approved review requires
//! an entirely new product admission and fresh trusted observation.

use std::fmt;

#[cfg(test)]
#[path = "agent_work_journal_tests.rs"]
mod tests;

use crate::{
    AgentBrowserHumanReason, AgentNativeShutdownProof, AgentNeedsHumanTransition, AgentRunManifest,
    AgentRunPolicySettlement, AgentRunProgressOutcome, AgentSupervisorFailure,
    SemanticObservationId,
};

/// Fixed version-one record width, including reserved zero bytes.
pub const AGENT_WORK_RECORD_BYTES: usize = 96;
/// Retention is explicit; capacity exhaustion never evicts recovery obligations.
pub const MAX_DURABLE_AGENT_WORK_RUNS: usize = 1024;

/// Unpredictable identity of one exclusively fenced Store owner incarnation.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AgentWorkIncarnation([u8; 16]);

impl AgentWorkIncarnation {
    /// Mints an identity; only a successful durable claim makes it current.
    pub fn generate() -> Self {
        Self(ulid::Ulid::new().0.to_be_bytes())
    }
    /// Fixed identity bytes, never a credential or executable capability.
    pub const fn bytes(self) -> [u8; 16] {
        self.0
    }
}

impl fmt::Debug for AgentWorkIncarnation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AgentWorkIncarnation([redacted])")
    }
}

/// Closed durable disposition. Terminal means immutable, not necessarily clean.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum AgentWorkDisposition {
    /// Persisted before runtime activation.
    Admitted = 1,
    /// Runtime activation may have occurred; never safe to replay.
    Running = 2,
    /// Predispatch policy refusal requires explicit human review.
    NeedsApproval = 3,
    /// Process-local owners retain unresolved obligations.
    RecoveryRequired = 4,
    /// Prior process ended without immutable terminal persistence.
    Interrupted = 5,
    /// Original execution and lifecycle proved clean task completion.
    Succeeded = 6,
    /// Review accepted, but only fresh independent admission may execute.
    FreshAdmissionRequired = 7,
    /// Review rejected; any recorded debt remains unresolved.
    Rejected = 8,
    /// Explicit fail-closed classification; never evidence of clean drain.
    FailedClosed = 9,
    /// Task failed, but original execution and lifecycle owners proved drain.
    Failed = 10,
    /// Task was cancelled, with original execution and lifecycle owners drained.
    Cancelled = 11,
    /// The run cleanly relinquished all execution authority after requesting
    /// human intervention. A fresh admission is required to continue.
    WaitingForHuman = 12,
}

impl AgentWorkDisposition {
    const fn is_review_classification(self) -> bool {
        matches!(
            self,
            Self::NeedsApproval
                | Self::FreshAdmissionRequired
                | Self::Rejected
                | Self::FailedClosed
        )
    }
    /// Immutable records cannot be reopened or rewritten, including on restart.
    pub const fn is_terminal(self) -> bool {
        (self as u8) >= 6
    }
    fn decode(value: u8) -> Option<Self> {
        Some(match value {
            1 => Self::Admitted,
            2 => Self::Running,
            3 => Self::NeedsApproval,
            4 => Self::RecoveryRequired,
            5 => Self::Interrupted,
            6 => Self::Succeeded,
            7 => Self::FreshAdmissionRequired,
            8 => Self::Rejected,
            9 => Self::FailedClosed,
            10 => Self::Failed,
            11 => Self::Cancelled,
            12 => Self::WaitingForHuman,
            _ => return None,
        })
    }
}

/// Conservatively recorded obligations, not independent settlement evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentWorkDebt(u8);

impl AgentWorkDebt {
    /// No durable claim about which resources might have been dispatched.
    pub const UNKNOWN: Self = Self(63);
    /// Original proof-bearing closure covered all execution owners.
    pub const NONE: Self = Self(0);
    /// Closed six-bit native/provider/policy/audit/runtime/context mask.
    pub const fn bits(self) -> u8 {
        self.0
    }
}

/// Content-free durable handoff metadata. The run key plus exact bounded
/// observation ordinal supplies correlation without persisting page content or
/// revivable browser authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentWorkHumanHandoff {
    reason: AgentBrowserHumanReason,
    observation: u32,
}

impl AgentWorkHumanHandoff {
    /// Binds the closed reason to the exact run-local observation request. Work
    /// budgets keep this ordinal within `u32`; overflow fails before terminal
    /// ownership is consumed.
    pub fn try_new(
        reason: AgentBrowserHumanReason,
        observation: SemanticObservationId,
    ) -> Result<Self, AgentWorkJournalError> {
        Ok(Self {
            reason,
            observation: observation
                .get()
                .try_into()
                .map_err(|_| AgentWorkJournalError::Capacity)?,
        })
    }

    /// Closed reason suitable for product restoration.
    pub const fn reason(self) -> AgentBrowserHumanReason {
        self.reason
    }

    /// Exact run-local observation request correlation.
    pub const fn observation(self) -> u32 {
        self.observation
    }

    const fn reason_byte(self) -> u8 {
        match self.reason {
            AgentBrowserHumanReason::SignIn => 1,
            AgentBrowserHumanReason::Permission => 2,
            AgentBrowserHumanReason::UnsupportedInteraction => 3,
            AgentBrowserHumanReason::Verification => 4,
            AgentBrowserHumanReason::UserDecision => 5,
            AgentBrowserHumanReason::SensitiveEffect => 6,
            AgentBrowserHumanReason::HumanChallenge => 7,
        }
    }

    fn decode(reason: u8, observation: u32) -> Option<Self> {
        let reason = match reason {
            1 => AgentBrowserHumanReason::SignIn,
            2 => AgentBrowserHumanReason::Permission,
            3 => AgentBrowserHumanReason::UnsupportedInteraction,
            4 => AgentBrowserHumanReason::Verification,
            5 => AgentBrowserHumanReason::UserDecision,
            6 => AgentBrowserHumanReason::SensitiveEffect,
            7 => AgentBrowserHumanReason::HumanChallenge,
            _ => return None,
        };
        (observation != 0).then_some(Self {
            reason,
            observation,
        })
    }
}

/// One fixed-width, content-free persistence fact. Debug omits identities.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AgentWorkRecord([u8; AGENT_WORK_RECORD_BYTES]);

impl AgentWorkRecord {
    /// Builds initial admission from an actual trusted manifest, never model text.
    pub fn admitted(manifest: &AgentRunManifest, owner: AgentWorkIncarnation) -> Self {
        let mut bytes = [0; AGENT_WORK_RECORD_BYTES];
        bytes[0] = 1;
        bytes[1] = AgentWorkDisposition::Admitted as u8;
        bytes[2] = AgentWorkDebt::UNKNOWN.bits();
        bytes[8..16].copy_from_slice(&1_u64.to_be_bytes());
        bytes[16..32].copy_from_slice(&owner.bytes());
        bytes[32..48].copy_from_slice(&manifest.id().bytes());
        bytes[48..64].copy_from_slice(&manifest.run().bytes());
        bytes[64..96].copy_from_slice(&manifest.guard());
        Self(bytes)
    }
    /// Validates persisted facts only. Decoding never reconstructs authority.
    pub fn decode(bytes: [u8; AGENT_WORK_RECORD_BYTES]) -> Option<Self> {
        let record = Self(bytes);
        let disposition = AgentWorkDisposition::decode(bytes[1])?;
        let handoff = AgentWorkHumanHandoff::decode(
            bytes[3],
            u32::from_be_bytes(bytes[4..8].try_into().ok()?),
        );
        if bytes[0] != 1
            || bytes[2] > 63
            || (disposition == AgentWorkDisposition::WaitingForHuman) != handoff.is_some()
            || (disposition != AgentWorkDisposition::WaitingForHuman && bytes[3..8] != [0; 5])
            || record.revision() == 0
            || bytes[16..32] == [0; 16]
            || (!disposition.is_review_classification()
                && matches!(
                    disposition,
                    AgentWorkDisposition::Succeeded
                        | AgentWorkDisposition::Failed
                        | AgentWorkDisposition::Cancelled
                        | AgentWorkDisposition::WaitingForHuman
                ) != (bytes[2] == 0))
        {
            return None;
        }
        Some(record)
    }
    /// Stable bytes suitable only for the content-free Store protocol.
    pub const fn as_bytes(&self) -> &[u8; AGENT_WORK_RECORD_BYTES] {
        &self.0
    }
    /// Exact manifest/run key, independent of process incarnation.
    pub fn key(self) -> [u8; 32] {
        let mut key = [0; 32];
        key.copy_from_slice(&self.0[32..64]);
        key
    }
    /// Current owning process fence.
    pub fn incarnation(self) -> AgentWorkIncarnation {
        let mut id = [0; 16];
        id.copy_from_slice(&self.0[16..32]);
        AgentWorkIncarnation(id)
    }
    /// Monotonic exact compare-and-set revision.
    pub fn revision(self) -> u64 {
        let mut revision = [0; 8];
        revision.copy_from_slice(&self.0[8..16]);
        u64::from_be_bytes(revision)
    }
    /// Current closed durable classification.
    pub fn disposition(self) -> AgentWorkDisposition {
        // Construction and decoding validate the closed discriminant.
        AgentWorkDisposition::decode(self.0[1]).unwrap_or(AgentWorkDisposition::FailedClosed)
    }
    /// Conservative debt retained even when human review closes the record.
    pub const fn debt(self) -> AgentWorkDebt {
        AgentWorkDebt(self.0[2])
    }
    /// Durable handoff reason and exact run-local observation correlation.
    /// This is descriptive state only and cannot recreate browser authority.
    pub fn human_handoff(self) -> Option<AgentWorkHumanHandoff> {
        (self.disposition() == AgentWorkDisposition::WaitingForHuman).then(|| {
            AgentWorkHumanHandoff::decode(
                self.0[3],
                u32::from_be_bytes(self.0[4..8].try_into().expect("fixed record slice")),
            )
            .expect("validated durable handoff")
        })
    }
    /// Constructs a non-success transition. Approval acceptance never executes.
    pub fn transition(self, next: AgentWorkDisposition) -> Result<Self, AgentWorkJournalError> {
        use AgentWorkDisposition::*;
        let allowed = matches!(
            (self.disposition(), next),
            (Admitted, Running | RecoveryRequired | FailedClosed)
                | (Running, NeedsApproval | RecoveryRequired | FailedClosed)
                | (
                    NeedsApproval | Interrupted,
                    FreshAdmissionRequired | Rejected | FailedClosed
                )
                | (RecoveryRequired, FailedClosed)
        );
        if !allowed {
            return Err(AgentWorkJournalError::Transition);
        }
        self.next(next, self.incarnation(), self.debt())
    }
    /// Records success only from original policy/audit and native closure proofs.
    /// The application must also own the matching clean runtime lifecycle result.
    pub fn completed(
        self,
        policy: AgentRunPolicySettlement,
        _native: &AgentNativeShutdownProof,
    ) -> Result<Self, AgentWorkJournalError> {
        if self.disposition() != AgentWorkDisposition::Running
            || self.0[32..48] != policy.closure().manifest().bytes()
            || self.0[64..96] != policy.closure().manifest_guard()
            || policy.closure().outcome() != AgentRunProgressOutcome::Succeeded
        {
            return Err(AgentWorkJournalError::Transition);
        }
        self.next(
            AgentWorkDisposition::Succeeded,
            self.incarnation(),
            AgentWorkDebt::NONE,
        )
    }
    /// Records unsuccessful closure only from the original policy/audit and
    /// native proofs. A failure classification alone cannot clear debt. The
    /// application must also own the matching clean runtime lifecycle result.
    pub fn closed_unsuccessfully(
        self,
        policy: AgentRunPolicySettlement,
        _native: &AgentNativeShutdownProof,
    ) -> Result<Self, AgentWorkJournalError> {
        if self.disposition() != AgentWorkDisposition::Running
            || self.0[32..48] != policy.closure().manifest().bytes()
            || self.0[64..96] != policy.closure().manifest_guard()
        {
            return Err(AgentWorkJournalError::Transition);
        }
        let disposition = match policy.closure().outcome() {
            AgentRunProgressOutcome::Failed(_) => AgentWorkDisposition::Failed,
            AgentRunProgressOutcome::Cancelled(_) => AgentWorkDisposition::Cancelled,
            AgentRunProgressOutcome::Succeeded => return Err(AgentWorkJournalError::Transition),
        };
        self.next(disposition, self.incarnation(), AgentWorkDebt::NONE)
    }
    /// Records a clean, non-executable human handoff from the original
    /// policy/audit and native closure proofs. The internal supervisor failure
    /// is a fail-closed execution accounting fact, not the product outcome.
    pub fn waiting_for_human(
        self,
        policy: AgentRunPolicySettlement,
        _native: &AgentNativeShutdownProof,
        handoff: AgentWorkHumanHandoff,
    ) -> Result<Self, AgentWorkJournalError> {
        self.waiting_for_human_policy(policy, handoff)
    }

    fn waiting_for_human_policy(
        self,
        policy: AgentRunPolicySettlement,
        handoff: AgentWorkHumanHandoff,
    ) -> Result<Self, AgentWorkJournalError> {
        let closure = policy.closure();
        if self.disposition() != AgentWorkDisposition::Running
            || self.0[32..48] != closure.manifest().bytes()
            || self.0[64..96] != closure.manifest_guard()
            || closure.outcome()
                != AgentRunProgressOutcome::Failed(AgentSupervisorFailure::PolicyDenied)
        {
            return Err(AgentWorkJournalError::Transition);
        }
        let mut next = self.next(
            AgentWorkDisposition::WaitingForHuman,
            self.incarnation(),
            AgentWorkDebt::NONE,
        )?;
        next.0[3] = handoff.reason_byte();
        next.0[4..8].copy_from_slice(&handoff.observation().to_be_bytes());
        Ok(next)
    }
    /// Records an unexecuted policy refusal only after original failed policy,
    /// audit and native closure. The application must additionally own the
    /// exact clean runtime join. Review is still required; no proposal or
    /// authorization survives as executable authority in this record.
    pub fn needs_approval_closed(
        self,
        policy: AgentRunPolicySettlement,
        _native: &AgentNativeShutdownProof,
        review: AgentNeedsHumanTransition,
    ) -> Result<Self, AgentWorkJournalError> {
        let closure = policy.closure();
        if self.disposition() != AgentWorkDisposition::Running
            || self.0[32..48] != closure.manifest().bytes()
            || self.0[64..96] != closure.manifest_guard()
            || self.0[48..64] != review.context().identity().owner().bytes()
            || !review.matches_manifest_revision(closure.manifest(), closure.manifest_guard())
            || closure.outcome()
                != AgentRunProgressOutcome::Failed(AgentSupervisorFailure::PolicyDenied)
        {
            return Err(AgentWorkJournalError::Transition);
        }
        self.next(
            AgentWorkDisposition::NeedsApproval,
            self.incarnation(),
            AgentWorkDebt::NONE,
        )
    }
    /// Store-only restart classification after acquiring the exclusive process
    /// lock. Terminal facts remain byte-for-byte unchanged.
    pub fn interrupted(self, owner: AgentWorkIncarnation) -> Result<Self, AgentWorkJournalError> {
        if self.disposition().is_terminal() || owner == self.incarnation() {
            return Ok(self);
        }
        self.next(
            AgentWorkDisposition::Interrupted,
            owner,
            AgentWorkDebt::UNKNOWN,
        )
    }
    fn next(
        self,
        disposition: AgentWorkDisposition,
        owner: AgentWorkIncarnation,
        debt: AgentWorkDebt,
    ) -> Result<Self, AgentWorkJournalError> {
        let revision = self
            .revision()
            .checked_add(1)
            .ok_or(AgentWorkJournalError::Capacity)?;
        let mut next = self;
        next.0[1] = disposition as u8;
        next.0[2] = debt.bits();
        next.0[8..16].copy_from_slice(&revision.to_be_bytes());
        next.0[16..32].copy_from_slice(&owner.bytes());
        Ok(next)
    }
    /// Independently checks the persisted CAS successor grammar. Restart uses
    /// a separate exclusively fenced transaction, never this admission lane.
    pub fn is_successor_of(self, previous: Self) -> bool {
        if self.key() != previous.key()
            || self.0[64..96] != previous.0[64..96]
            || self.incarnation() != previous.incarnation()
            || previous.revision().checked_add(1) != Some(self.revision())
            || previous.disposition().is_terminal()
        {
            return false;
        }
        if matches!(
            self.disposition(),
            AgentWorkDisposition::Succeeded
                | AgentWorkDisposition::Failed
                | AgentWorkDisposition::Cancelled
                | AgentWorkDisposition::WaitingForHuman
        ) {
            return previous.disposition() == AgentWorkDisposition::Running
                && self.debt() == AgentWorkDebt::NONE;
        }
        if self.disposition() == AgentWorkDisposition::NeedsApproval
            && self.debt() == AgentWorkDebt::NONE
        {
            return previous.disposition() == AgentWorkDisposition::Running;
        }
        previous.transition(self.disposition()) == Ok(self)
    }
}

impl fmt::Debug for AgentWorkRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentWorkRecord")
            .field("revision", &self.revision())
            .field("disposition", &self.disposition())
            .field("debt", &self.debt())
            .finish_non_exhaustive()
    }
}

/// Closed persistence/refusal classifications; no storage error text escapes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentWorkJournalError {
    /// Store or exclusive process lock is unavailable.
    Unavailable,
    /// The process incarnation is not the current exclusive owner.
    Fenced,
    /// Expected durable identity/revision no longer matches.
    Conflict,
    /// Transition would reopen terminal state or replay a decision.
    Transition,
    /// Bounded durable storage, revision or mailbox capacity is exhausted.
    Capacity,
    /// Durable data is malformed or a write outcome cannot be determined.
    Uncertain,
    /// The Store actor has sealed shutdown admission.
    Shutdown,
}

/// Bounded durable commands. Exact mutation retransmission reconciles storage
/// uncertainty only; it never replays provider or native execution.
#[derive(Clone, Copy, Debug)]
pub enum AgentWorkJournalRequest {
    /// Acquire exclusive process ownership and classify prior incomplete runs.
    Claim,
    /// Read one exact fact after an uncertain acknowledgement.
    Read {
        /// Current process fence.
        owner: AgentWorkIncarnation,
        /// Exact manifest/run key.
        key: [u8; 32],
    },
    /// Admit or update one exact record. None is initial admission only.
    CompareAndSet(AgentWorkJournalMutation),
}

/// Exact mutation intent. Decoded records cannot mint successful terminal writes.
#[derive(Clone, Copy)]
pub struct AgentWorkJournalMutation {
    expected: Option<AgentWorkRecord>,
    next: AgentWorkRecord,
    result_profile: Option<zephium_core::ids::ProfileId>,
}

impl fmt::Debug for AgentWorkJournalMutation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentWorkJournalMutation")
            .field("next", &self.next)
            .field("requires_result", &self.result_profile.is_some())
            .finish_non_exhaustive()
    }
}

impl AgentWorkJournalMutation {
    /// Creates a durable pre-execution admission from the trusted manifest.
    pub fn admit(manifest: &AgentRunManifest, owner: AgentWorkIncarnation) -> Self {
        Self {
            expected: None,
            next: AgentWorkRecord::admitted(manifest, owner),
            result_profile: None,
        }
    }
    /// Requires atomic durable result publication for this exact registered
    /// profile. Store persists this intent before any execution starts.
    pub fn admit_with_result(
        manifest: &AgentRunManifest,
        owner: AgentWorkIncarnation,
        profile: zephium_core::ids::ProfileId,
    ) -> Result<Self, AgentWorkJournalError> {
        if !manifest
            .plan_nodes()
            .iter()
            .any(|node| node.profiles().contains(&profile))
        {
            return Err(AgentWorkJournalError::Transition);
        }
        Ok(Self {
            result_profile: Some(profile),
            ..Self::admit(manifest, owner)
        })
    }
    /// Explicit durable-result destination, present only on original admission.
    pub const fn result_profile(self) -> Option<zephium_core::ids::ProfileId> {
        self.result_profile
    }
    /// Creates a non-success transition, including one-use human review.
    pub fn transition(
        previous: AgentWorkRecord,
        disposition: AgentWorkDisposition,
    ) -> Result<Self, AgentWorkJournalError> {
        Ok(Self {
            expected: Some(previous),
            next: previous.transition(disposition)?,
            result_profile: None,
        })
    }
    /// The original clean runtime lifecycle owner supplies its native proof;
    /// policy settlement independently binds the exact successful manifest.
    pub fn completed(
        previous: AgentWorkRecord,
        policy: AgentRunPolicySettlement,
        native: &AgentNativeShutdownProof,
    ) -> Result<Self, AgentWorkJournalError> {
        Ok(Self {
            expected: Some(previous),
            next: previous.completed(policy, native)?,
            result_profile: None,
        })
    }
    /// Persists failed/cancelled drain, never task success, from the original
    /// joined policy/audit and native proofs plus the application's clean join.
    pub fn closed_unsuccessfully(
        previous: AgentWorkRecord,
        policy: AgentRunPolicySettlement,
        native: &AgentNativeShutdownProof,
    ) -> Result<Self, AgentWorkJournalError> {
        Ok(Self {
            expected: Some(previous),
            next: previous.closed_unsuccessfully(policy, native)?,
            result_profile: None,
        })
    }
    /// Persists a clean model-requested handoff only from the original joined
    /// policy/audit and native shutdown proof.
    pub fn waiting_for_human(
        previous: AgentWorkRecord,
        policy: AgentRunPolicySettlement,
        native: &AgentNativeShutdownProof,
        handoff: AgentWorkHumanHandoff,
    ) -> Result<Self, AgentWorkJournalError> {
        Ok(Self {
            expected: Some(previous),
            next: previous.waiting_for_human(policy, native, handoff)?,
            result_profile: None,
        })
    }
    /// Closes only the run's retained-resource execution lease, not the resource
    /// or browser lifetime. The application must independently join the exact
    /// scoped worker drain before persisting this mutation. Decoded records,
    /// lease coordinates and a wake cannot substitute for native delivery.
    pub fn closed_retained(
        previous: AgentWorkRecord,
        policy: AgentRunPolicySettlement,
        delivery: &crate::WorkBrowserLeaseDeliveryProof,
    ) -> Result<Self, AgentWorkJournalError> {
        if previous.disposition() != AgentWorkDisposition::Running
            || previous.0[32..48] != policy.closure().manifest().bytes()
            || previous.0[48..64] != delivery.lease().run().bytes()
            || previous.0[64..96] != policy.closure().manifest_guard()
        {
            return Err(AgentWorkJournalError::Transition);
        }
        let disposition = match policy.closure().outcome() {
            AgentRunProgressOutcome::Succeeded => AgentWorkDisposition::Succeeded,
            AgentRunProgressOutcome::Failed(_) => AgentWorkDisposition::Failed,
            AgentRunProgressOutcome::Cancelled(_) => AgentWorkDisposition::Cancelled,
        };
        Ok(Self {
            expected: Some(previous),
            next: previous.next(disposition, previous.incarnation(), AgentWorkDebt::NONE)?,
            result_profile: None,
        })
    }
    /// Persists a clean retained-page handoff. The delivery proves the old run
    /// lease drained; it does not grant a successor lease or resume token.
    pub fn waiting_for_human_retained(
        previous: AgentWorkRecord,
        policy: AgentRunPolicySettlement,
        delivery: &crate::WorkBrowserLeaseDeliveryProof,
        handoff: AgentWorkHumanHandoff,
    ) -> Result<Self, AgentWorkJournalError> {
        if previous.disposition() != AgentWorkDisposition::Running
            || previous.0[32..48] != policy.closure().manifest().bytes()
            || previous.0[48..64] != delivery.lease().run().bytes()
            || previous.0[64..96] != policy.closure().manifest_guard()
        {
            return Err(AgentWorkJournalError::Transition);
        }
        Ok(Self {
            expected: Some(previous),
            next: previous.waiting_for_human_policy(policy, handoff)?,
            result_profile: None,
        })
    }
    /// Preserves an exact policy-derived review request with original clean
    /// closure proofs. A generic transition or decoded fact cannot clear debt.
    pub fn needs_approval_closed(
        previous: AgentWorkRecord,
        policy: AgentRunPolicySettlement,
        native: &AgentNativeShutdownProof,
        review: AgentNeedsHumanTransition,
    ) -> Result<Self, AgentWorkJournalError> {
        Ok(Self {
            expected: Some(previous),
            next: previous.needs_approval_closed(policy, native, review)?,
            result_profile: None,
        })
    }
    /// Exact expected bytes for adapter-side CAS, absent only on admission.
    pub const fn expected(self) -> Option<AgentWorkRecord> {
        self.expected
    }
    /// Exact acknowledged durable fact; never executable authority.
    pub const fn next(self) -> AgentWorkRecord {
        self.next
    }
}

/// Content-free durable acknowledgement; never executable authority.
#[derive(Debug)]
pub enum AgentWorkJournalReply {
    /// Bounded complete inventory after atomically claiming this incarnation.
    Claimed {
        /// Fresh Store-minted incarnation, never supplied or restored by callers.
        owner: AgentWorkIncarnation,
        /// Complete bounded inventory, containing facts but no authority.
        records: Vec<AgentWorkRecord>,
    },
    /// Exact durable fact, or absence on a reconciliation read.
    Record(Option<AgentWorkRecord>),
}

/// Exactly one callback for an accepted Store operation, including uncertainty.
pub type AgentWorkJournalCompletion =
    Box<dyn FnOnce(Result<AgentWorkJournalReply, AgentWorkJournalError>) + Send>;

/// Dormant content-free application persistence port, not a native authority.
pub trait AgentWorkJournalPort: Send + Sync {
    /// Separate private result lane, never a content-bearing journal/audit row.
    /// The same exact Store allocation owns both lanes. Default adapters refuse.
    fn artifact(
        &self,
        _request: crate::AgentWorkArtifactRequest,
        _completion: crate::AgentWorkArtifactCompletion,
    ) -> Result<(), AgentWorkJournalError> {
        Err(AgentWorkJournalError::Unavailable)
    }
    /// Nonblocking bounded admission. Refusal does not invoke the callback.
    fn dispatch(
        &self,
        request: AgentWorkJournalRequest,
        completion: AgentWorkJournalCompletion,
    ) -> Result<(), AgentWorkJournalError>;
}
