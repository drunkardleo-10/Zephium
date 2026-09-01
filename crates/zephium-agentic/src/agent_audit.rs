//! Bounded content-free semantic audit delivery.
//!
//! The ledger snapshots only exact supervisor-owned progress projections and
//! exposes typed batches to a trusted persistence adapter. It contains no
//! objective, prompt, model output, raw tool chatter, page content, origin,
//! selector, JavaScript, secret, native handle, path, or arbitrary error text.
//! It owns no I/O, task, timer, worker, channel, provider, page, or native view.

use std::collections::VecDeque;
use std::fmt;
use std::num::NonZeroU64;

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    AgentPolicyInstant, AgentProgressBlocker, AgentProgressOperation, AgentProgressResource,
    AgentProgressResult, AgentProgressState, AgentRunManifest, AgentRunManifestId,
    AgentRunSupervisor, AgentSemanticProgress, AgentSupervisorCancellationReason,
    AgentSupervisorExecutionOutcome, AgentSupervisorFailure, AgentSupervisorId,
    AgentSupervisorWait, ContextResourceDisposition, ContextTerminal, SemanticActionFailure,
    SemanticEffectClass, SemanticEffectProofKind,
};

/// Maximum undelivered semantic audit events retained by one run.
pub const MAX_PENDING_AGENT_AUDIT_EVENTS: usize = 64;
/// Maximum events transferred in one persistence delivery.
pub const MAX_AGENT_AUDIT_DELIVERY_EVENTS: usize = 16;
/// Exact byte width of the version-one durable semantic progress record.
pub const AGENT_AUDIT_RECORD_V1_BYTES: usize = 128;

/// Canonical fixed-width content-free semantic progress record.
///
/// This is the only event payload a durable adapter may retain. The encoding
/// contains a version byte, opaque identities, monotonic time, and closed enum
/// codes; unused tail bytes are zero. It cannot represent free-form content.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AgentAuditRecordV1([u8; AGENT_AUDIT_RECORD_V1_BYTES]);

impl AgentAuditRecordV1 {
    /// Exact immutable bytes for a trusted durable adapter.
    pub const fn as_bytes(&self) -> &[u8; AGENT_AUDIT_RECORD_V1_BYTES] {
        &self.0
    }
}

impl fmt::Debug for AgentAuditRecordV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AgentAuditRecordV1([redacted])")
    }
}

/// Strictly increasing identity for one semantic audit event.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AgentAuditEventId(NonZeroU64);

impl AgentAuditEventId {
    /// Constructs one nonzero shell-minted event identity.
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Numeric value for exact process-local ordering and persistence joins.
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Debug for AgentAuditEventId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AgentAuditEventId([redacted])")
    }
}

/// Strictly increasing identity for one exact audit delivery attempt.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AgentAuditDeliveryId(NonZeroU64);

impl AgentAuditDeliveryId {
    /// Constructs one nonzero shell-minted delivery identity.
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Numeric value for exact process-local correlation.
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Debug for AgentAuditDeliveryId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AgentAuditDeliveryId([redacted])")
    }
}

/// One content-free semantic progress snapshot awaiting durable append.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AgentAuditEvent {
    id: AgentAuditEventId,
    recorded_at: AgentPolicyInstant,
    progress: AgentSemanticProgress,
    record: AgentAuditRecordV1,
    guard: [u8; 32],
}

impl AgentAuditEvent {
    /// Exact one-shot event identity.
    pub const fn id(self) -> AgentAuditEventId {
        self.id
    }

    /// Trusted monotonic recording time within the manifest lifetime.
    pub const fn recorded_at(self) -> AgentPolicyInstant {
        self.recorded_at
    }

    /// Exact supervisor-owned semantic progress projection.
    pub const fn progress(self) -> AgentSemanticProgress {
        self.progress
    }

    /// Canonical content-free record accepted by the durable adapter.
    pub const fn persistence_record(self) -> AgentAuditRecordV1 {
        self.record
    }
}

impl fmt::Debug for AgentAuditEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentAuditEvent")
            .field("id", &self.id)
            .field("recorded_at", &self.recorded_at)
            .field("progress", &self.progress)
            .field("guard", &"[redacted]")
            .finish()
    }
}

/// Exact immutable proof identifying one in-flight delivery.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AgentAuditDeliveryProof {
    id: AgentAuditDeliveryId,
    first: AgentAuditEventId,
    last: AgentAuditEventId,
    events: u8,
    guard: [u8; 32],
}

impl AgentAuditDeliveryProof {
    /// Exact delivery attempt identity.
    pub const fn id(self) -> AgentAuditDeliveryId {
        self.id
    }

    /// First ordered event included in the batch.
    pub const fn first(self) -> AgentAuditEventId {
        self.first
    }

    /// Last ordered event included in the batch.
    pub const fn last(self) -> AgentAuditEventId {
        self.last
    }

    /// Exact number of ordered events included in the batch.
    pub const fn events(self) -> u8 {
        self.events
    }

    /// Builds an exact typed settlement for the trusted persistence adapter.
    pub const fn settle(self, outcome: AgentAuditDeliveryOutcome) -> AgentAuditDeliverySettlement {
        AgentAuditDeliverySettlement {
            proof: self,
            outcome,
        }
    }
}

impl fmt::Debug for AgentAuditDeliveryProof {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentAuditDeliveryProof")
            .field("id", &self.id)
            .field("first", &self.first)
            .field("last", &self.last)
            .field("events", &self.events)
            .field("guard", &"[redacted]")
            .finish()
    }
}

/// Non-cloneable bounded append request for the trusted audit adapter.
#[must_use]
pub struct AgentAuditDelivery {
    manifest: AgentRunManifestId,
    supervisor: AgentSupervisorId,
    proof: AgentAuditDeliveryProof,
    events: Vec<AgentAuditEvent>,
}

impl AgentAuditDelivery {
    /// Exact immutable manifest revision whose events are delivered.
    pub const fn manifest(&self) -> AgentRunManifestId {
        self.manifest
    }

    /// Exact mutable supervisor incarnation whose events are delivered.
    pub const fn supervisor(&self) -> AgentSupervisorId {
        self.supervisor
    }

    /// Exact proof the adapter must return on terminal settlement.
    pub const fn proof(&self) -> AgentAuditDeliveryProof {
        self.proof
    }

    /// Ordered bounded content-free events.
    pub fn events(&self) -> impl ExactSizeIterator<Item = AgentAuditEvent> + '_ {
        self.events.iter().copied()
    }
}

impl fmt::Debug for AgentAuditDelivery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentAuditDelivery")
            .field("manifest", &self.manifest)
            .field("supervisor", &self.supervisor)
            .field("proof", &self.proof)
            .field("events", &self.events.len())
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Closed failure returned by a trusted audit persistence adapter.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentAuditSinkFailure {
    /// Persistence was temporarily unavailable before commit.
    #[error("agent audit persistence is unavailable")]
    Unavailable,
    /// The persistence adapter's own bounded queue was full.
    #[error("agent audit persistence queue is full")]
    Capacity,
    /// Durable append failed before an exact commit acknowledgement.
    #[error("agent audit persistence append failed")]
    AppendFailed,
    /// Process shutdown refused a new append.
    #[error("agent audit persistence is shutting down")]
    Shutdown,
}

/// Terminal persistence outcome for one exact delivery attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentAuditDeliveryOutcome {
    /// Every event was durably committed in exact order.
    Committed,
    /// The adapter proved no event was committed.
    Refused(AgentAuditSinkFailure),
    /// Cancellation won before any event was committed.
    Cancelled,
}

/// Exact asynchronous settlement returned by the trusted audit adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentAuditDeliverySettlement {
    proof: AgentAuditDeliveryProof,
    outcome: AgentAuditDeliveryOutcome,
}

impl AgentAuditDeliverySettlement {
    /// Exact in-flight delivery proof.
    pub const fn proof(self) -> AgentAuditDeliveryProof {
        self.proof
    }

    /// Durable commit, proven pre-commit refusal, or cancellation.
    pub const fn outcome(self) -> AgentAuditDeliveryOutcome {
        self.outcome
    }
}

/// Immediate result of handing a delivery to a trusted persistence adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentAuditDispatch {
    /// The adapter accepted ownership and will settle asynchronously.
    Accepted(AgentAuditDeliveryProof),
    /// The adapter synchronously proved that no append began.
    Refused(AgentAuditDeliverySettlement),
}

/// One-shot shell callback for an exact asynchronous durable settlement.
pub type AgentAuditCompletion = Box<dyn FnOnce(AgentAuditDeliverySettlement) + Send + 'static>;

/// Closed trusted-shell port for durable content-free audit append.
pub trait AgentAuditPort: Send + Sync {
    /// Attempts to transactionally append one exact bounded event batch.
    ///
    /// The adapter must use the exact delivery identity and proof as an
    /// idempotency key. Re-observing the same pair may return the prior result;
    /// the same identity with any different proof must never append. A refused
    /// or cancelled settlement proves that none of the batch committed. An
    /// uncertain or partial durable outcome must remain unsettled for explicit
    /// reconciliation rather than being reported as refusal.
    /// An accepted delivery transfers both values to the adapter. It invokes
    /// `completion` exactly once only after a definite durable commit or a
    /// proven pre-commit refusal. A durability-ambiguous result intentionally
    /// produces no callback and must be reconciled by replaying the ledger's
    /// exact current delivery.
    fn append(
        &self,
        delivery: AgentAuditDelivery,
        completion: AgentAuditCompletion,
    ) -> AgentAuditDispatch;
}

/// Privacy-preserving bounded audit-ledger counts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentAuditLedgerStatus {
    pending: u8,
    in_flight: u8,
    committed: u64,
    shutdown_sealed: bool,
    fail_stopped: bool,
}

impl AgentAuditLedgerStatus {
    /// Undelivered events retained in memory, including any in-flight prefix.
    pub const fn pending(self) -> u8 {
        self.pending
    }

    /// Events in the one exact current delivery, or zero.
    pub const fn in_flight(self) -> u8 {
        self.in_flight
    }

    /// Events durably acknowledged during this ledger incarnation.
    pub const fn committed(self) -> u64 {
        self.committed
    }

    /// Whether no new events may be recorded during shutdown drain.
    pub const fn shutdown_sealed(self) -> bool {
        self.shutdown_sealed
    }

    /// Whether ambiguous identity/settlement sealed further mutation.
    pub const fn fail_stopped(self) -> bool {
        self.fail_stopped
    }
}

/// Closed audit-ledger refusal.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentAuditError {
    /// The ledger/supervisor/manifest join was not exact.
    #[error("agent audit authority mismatched")]
    Authority,
    /// The ledger must be created before the root starts executing.
    #[error("agent audit ledger must start with the queued root")]
    StartState,
    /// Event identity was reused or regressed.
    #[error("agent audit event identity replayed")]
    EventReplay,
    /// Trusted monotonic event time was outside the manifest or regressed.
    #[error("agent audit event time is invalid")]
    EventTime,
    /// The exact current projection was already recorded for this node.
    #[error("agent audit progress projection is unchanged")]
    DuplicateProgress,
    /// The undelivered event ceiling is full.
    #[error("agent audit pending event ceiling reached")]
    EventLimit,
    /// Bounded event/progress storage allocation failed.
    #[error("agent audit bounded storage is unavailable")]
    Capacity,
    /// Delivery size was zero or exceeded the hard batch ceiling.
    #[error("agent audit delivery size is invalid")]
    DeliverySize,
    /// Another exact delivery already owns the queue prefix.
    #[error("agent audit delivery is already pending")]
    DeliveryPending,
    /// Delivery identity was reused or regressed.
    #[error("agent audit delivery identity replayed")]
    DeliveryReplay,
    /// No exact delivery is currently in flight.
    #[error("agent audit delivery is missing")]
    DeliveryMissing,
    /// Delivery settlement identity was ambiguous; the ledger fail-stopped.
    #[error("agent audit delivery settlement mismatched")]
    DeliveryMismatch,
    /// New events are forbidden after shutdown sealing.
    #[error("agent audit ledger is sealed for shutdown")]
    ShutdownSealed,
    /// Ambiguous settlement terminally sealed the ledger.
    #[error("agent audit ledger is fail-stopped")]
    FailStopped,
    /// Internal bounded ordering/accounting became contradictory.
    #[error("agent audit ledger invariant failed")]
    Invariant,
}

#[derive(Clone, Copy)]
struct ProgressRow {
    node: crate::AgentPlanNodeId,
    progress: AgentSemanticProgress,
}

/// Bounded single-owner buffer in front of one durable audit adapter.
#[must_use]
pub struct AgentAuditLedger {
    manifest: AgentRunManifestId,
    manifest_guard: [u8; 32],
    starts_at: AgentPolicyInstant,
    expires_at: AgentPolicyInstant,
    supervisor: AgentSupervisorId,
    events: VecDeque<AgentAuditEvent>,
    progress: Vec<ProgressRow>,
    in_flight: Option<AgentAuditDeliveryProof>,
    last_event: Option<AgentAuditEventId>,
    last_delivery: Option<AgentAuditDeliveryId>,
    last_recorded_at: Option<AgentPolicyInstant>,
    committed: u64,
    shutdown_sealed: bool,
    fail_stopped: bool,
}

impl AgentAuditLedger {
    /// Creates an empty ledger joined before the exact supervisor root starts.
    pub fn try_new(
        manifest: &AgentRunManifest,
        supervisor: &AgentRunSupervisor,
    ) -> Result<Self, AgentAuditError> {
        if !supervisor.topology().matches_manifest(manifest) {
            return Err(AgentAuditError::Authority);
        }
        let status = supervisor.status();
        let root = supervisor.topology().root();
        if status.activated() != 1
            || status.live() != 1
            || status.queued() != 1
            || status.executing() != 0
            || status.contexts() != 0
            || supervisor
                .node_status(root)
                .is_none_or(|state| state != crate::AgentSupervisorNodeStatus::Queued)
        {
            return Err(AgentAuditError::StartState);
        }
        Ok(Self {
            manifest: manifest.id(),
            manifest_guard: manifest.guard(),
            starts_at: manifest.issued_at(),
            expires_at: manifest.expires_at(),
            supervisor: supervisor.id(),
            events: VecDeque::new(),
            progress: Vec::new(),
            in_flight: None,
            last_event: None,
            last_delivery: None,
            last_recorded_at: None,
            committed: 0,
            shutdown_sealed: false,
            fail_stopped: false,
        })
    }

    /// Records the exact current progress for one activated node.
    ///
    /// Capacity, identity, time, and duplicate checks all finish before the
    /// event ID is consumed or either bounded collection mutates.
    pub fn record_current(
        &mut self,
        supervisor: &AgentRunSupervisor,
        node: crate::AgentPlanNodeId,
        id: AgentAuditEventId,
        recorded_at: AgentPolicyInstant,
    ) -> Result<AgentAuditEvent, AgentAuditError> {
        self.validate()?;
        if self.fail_stopped {
            return Err(AgentAuditError::FailStopped);
        }
        if self.shutdown_sealed {
            return Err(AgentAuditError::ShutdownSealed);
        }
        if supervisor.id() != self.supervisor || supervisor.topology().manifest() != self.manifest {
            return Err(AgentAuditError::Authority);
        }
        let progress = supervisor
            .semantic_progress(node)
            .ok_or(AgentAuditError::Authority)?;
        if progress.manifest() != self.manifest || progress.supervisor() != self.supervisor {
            return Err(AgentAuditError::Authority);
        }
        if self.last_event.is_some_and(|last| id <= last) {
            return Err(AgentAuditError::EventReplay);
        }
        if recorded_at < self.starts_at
            || recorded_at > self.expires_at
            || self.last_recorded_at.is_some_and(|last| recorded_at < last)
        {
            return Err(AgentAuditError::EventTime);
        }
        if self.events.len() >= MAX_PENDING_AGENT_AUDIT_EVENTS {
            return Err(AgentAuditError::EventLimit);
        }
        let progress_index = self.progress.iter().position(|row| row.node == node);
        if progress_index.is_some_and(|index| self.progress[index].progress == progress) {
            return Err(AgentAuditError::DuplicateProgress);
        }
        self.events
            .try_reserve_exact(1)
            .map_err(|_| AgentAuditError::Capacity)?;
        if progress_index.is_none() {
            self.progress
                .try_reserve_exact(1)
                .map_err(|_| AgentAuditError::Capacity)?;
        }

        let record = event_record_v1(id, recorded_at, progress)?;
        let event = AgentAuditEvent {
            id,
            recorded_at,
            progress,
            record,
            guard: event_guard(self.manifest_guard, record),
        };
        self.events.push_back(event);
        match progress_index {
            Some(index) => self.progress[index].progress = progress,
            None => self.progress.push(ProgressRow { node, progress }),
        }
        self.last_event = Some(id);
        self.last_recorded_at = Some(recorded_at);
        self.validate()?;
        Ok(event)
    }

    /// Begins one exact bounded delivery without removing retained events.
    pub fn begin_delivery(
        &mut self,
        id: AgentAuditDeliveryId,
        max_events: usize,
    ) -> Result<AgentAuditDelivery, AgentAuditError> {
        self.validate()?;
        if self.fail_stopped {
            return Err(AgentAuditError::FailStopped);
        }
        if max_events == 0 || max_events > MAX_AGENT_AUDIT_DELIVERY_EVENTS {
            return Err(AgentAuditError::DeliverySize);
        }
        if self.in_flight.is_some() {
            return Err(AgentAuditError::DeliveryPending);
        }
        if self.last_delivery.is_some_and(|last| id <= last) {
            return Err(AgentAuditError::DeliveryReplay);
        }
        if self.events.is_empty() {
            return Err(AgentAuditError::DeliveryMissing);
        }
        let count = self.events.len().min(max_events);
        let mut events = Vec::new();
        events
            .try_reserve_exact(count)
            .map_err(|_| AgentAuditError::Capacity)?;
        events.extend(self.events.iter().take(count).copied());
        let proof = delivery_proof(self.manifest_guard, self.supervisor, id, &events)?;
        self.in_flight = Some(proof);
        self.last_delivery = Some(id);
        Ok(AgentAuditDelivery {
            manifest: self.manifest,
            supervisor: self.supervisor,
            proof,
            events,
        })
    }

    /// Rebuilds the exact idempotent delivery after a lost local handoff signal.
    pub fn current_delivery(&self) -> Result<Option<AgentAuditDelivery>, AgentAuditError> {
        self.validate()?;
        let Some(proof) = self.in_flight else {
            return Ok(None);
        };
        let count = usize::from(proof.events());
        let mut events = Vec::new();
        events
            .try_reserve_exact(count)
            .map_err(|_| AgentAuditError::Capacity)?;
        events.extend(self.events.iter().take(count).copied());
        if delivery_proof(self.manifest_guard, self.supervisor, proof.id(), &events)? != proof {
            return Err(AgentAuditError::Invariant);
        }
        Ok(Some(AgentAuditDelivery {
            manifest: self.manifest,
            supervisor: self.supervisor,
            proof,
            events,
        }))
    }

    /// Settles only the exact current delivery.
    ///
    /// Commit removes exactly its retained prefix. Proven refusal and
    /// cancellation clear the in-flight attempt but retain every event for a
    /// later explicitly identified retry.
    pub fn settle_delivery(
        &mut self,
        settlement: AgentAuditDeliverySettlement,
    ) -> Result<AgentAuditDeliveryOutcome, AgentAuditError> {
        self.validate()?;
        let Some(expected) = self.in_flight else {
            self.fail_stopped = true;
            return Err(AgentAuditError::DeliveryMissing);
        };
        if settlement.proof() != expected {
            self.fail_stopped = true;
            return Err(AgentAuditError::DeliveryMismatch);
        }
        match settlement.outcome() {
            AgentAuditDeliveryOutcome::Committed => {
                let committed = self
                    .committed
                    .checked_add(u64::from(expected.events()))
                    .ok_or(AgentAuditError::Invariant)?;
                for _ in 0..expected.events() {
                    self.events.pop_front().ok_or(AgentAuditError::Invariant)?;
                }
                self.committed = committed;
            }
            AgentAuditDeliveryOutcome::Refused(_) | AgentAuditDeliveryOutcome::Cancelled => {}
        }
        self.in_flight = None;
        self.validate()?;
        Ok(settlement.outcome())
    }

    /// Permanently rejects new records while preserving delivery/drain access.
    pub fn seal_for_shutdown(&mut self) -> Result<(), AgentAuditError> {
        self.validate()?;
        if self.fail_stopped {
            return Err(AgentAuditError::FailStopped);
        }
        self.shutdown_sealed = true;
        Ok(())
    }

    /// Exact bounded accounting without event or progress content.
    pub fn status(&self) -> AgentAuditLedgerStatus {
        AgentAuditLedgerStatus {
            pending: u8::try_from(self.events.len()).unwrap_or(u8::MAX),
            in_flight: self.in_flight.map_or(0, |proof| proof.events()),
            committed: self.committed,
            shutdown_sealed: self.shutdown_sealed,
            fail_stopped: self.fail_stopped,
        }
    }

    /// True only after shutdown seal and exact terminal delivery of all events.
    pub fn is_quiescent(&self) -> bool {
        self.shutdown_sealed && self.events.is_empty() && self.in_flight.is_none()
    }

    fn validate(&self) -> Result<(), AgentAuditError> {
        if self.events.len() > MAX_PENDING_AGENT_AUDIT_EVENTS
            || self.progress.len() > crate::MAX_AGENT_PLAN_NODES
        {
            return Err(AgentAuditError::Invariant);
        }
        if self
            .events
            .iter()
            .zip(self.events.iter().skip(1))
            .any(|(left, right)| left.id() >= right.id())
        {
            return Err(AgentAuditError::Invariant);
        }
        if self.progress.iter().enumerate().any(|(index, row)| {
            self.progress[index + 1..]
                .iter()
                .any(|next| next.node == row.node)
        }) {
            return Err(AgentAuditError::Invariant);
        }
        if let Some(proof) = self.in_flight {
            let count = usize::from(proof.events());
            if count == 0 || count > self.events.len() || count > MAX_AGENT_AUDIT_DELIVERY_EVENTS {
                return Err(AgentAuditError::Invariant);
            }
            let first = self.events.front().ok_or(AgentAuditError::Invariant)?;
            let last = self
                .events
                .get(count - 1)
                .ok_or(AgentAuditError::Invariant)?;
            if proof.first() != first.id() || proof.last() != last.id() {
                return Err(AgentAuditError::Invariant);
            }
        }
        Ok(())
    }
}

impl fmt::Debug for AgentAuditLedger {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentAuditLedger")
            .field("manifest", &self.manifest)
            .field("supervisor", &self.supervisor)
            .field("status", &self.status())
            .field("last_event", &self.last_event)
            .field("last_delivery", &self.last_delivery)
            .field("content", &"[redacted]")
            .finish()
    }
}

fn delivery_proof(
    manifest_guard: [u8; 32],
    supervisor: AgentSupervisorId,
    id: AgentAuditDeliveryId,
    events: &[AgentAuditEvent],
) -> Result<AgentAuditDeliveryProof, AgentAuditError> {
    let first = events.first().ok_or(AgentAuditError::DeliveryMissing)?.id();
    let last = events.last().ok_or(AgentAuditError::DeliveryMissing)?.id();
    let count = u8::try_from(events.len()).map_err(|_| AgentAuditError::DeliverySize)?;
    let mut hasher = Sha256::new();
    hasher.update(b"ZEPHIUM-AGENT-AUDIT-DELIVERY-1\0");
    hasher.update(manifest_guard);
    hasher.update(supervisor.get().to_be_bytes());
    hasher.update(id.get().to_be_bytes());
    hasher.update([count]);
    for event in events {
        hasher.update(event.guard);
    }
    Ok(AgentAuditDeliveryProof {
        id,
        first,
        last,
        events: count,
        guard: hasher.finalize().into(),
    })
}

fn event_record_v1(
    id: AgentAuditEventId,
    recorded_at: AgentPolicyInstant,
    progress: AgentSemanticProgress,
) -> Result<AgentAuditRecordV1, AgentAuditError> {
    let mut encoder = AuditRecordEncoder::new();
    encoder.update(id.get().to_be_bytes());
    encoder.update(recorded_at.millis().to_be_bytes());
    encoder.update(progress.manifest().bytes());
    encoder.update(progress.supervisor().get().to_be_bytes());
    encoder.update(progress.responsibility().bytes());
    encode_activity(&mut encoder, progress);
    encoder.finish()
}

fn event_guard(manifest_guard: [u8; 32], record: AgentAuditRecordV1) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"ZEPHIUM-AGENT-AUDIT-EVENT-1\0");
    hasher.update(manifest_guard);
    hasher.update(record.as_bytes());
    hasher.finalize().into()
}

struct AuditRecordEncoder {
    bytes: [u8; AGENT_AUDIT_RECORD_V1_BYTES],
    len: usize,
    overflowed: bool,
}

impl AuditRecordEncoder {
    fn new() -> Self {
        let mut bytes = [0_u8; AGENT_AUDIT_RECORD_V1_BYTES];
        bytes[0] = 1;
        Self {
            bytes,
            len: 1,
            overflowed: false,
        }
    }

    fn update(&mut self, bytes: impl AsRef<[u8]>) {
        let bytes = bytes.as_ref();
        let Some(end) = self.len.checked_add(bytes.len()) else {
            self.overflowed = true;
            return;
        };
        let Some(target) = self.bytes.get_mut(self.len..end) else {
            self.overflowed = true;
            return;
        };
        target.copy_from_slice(bytes);
        self.len = end;
    }

    fn finish(self) -> Result<AgentAuditRecordV1, AgentAuditError> {
        if self.overflowed {
            Err(AgentAuditError::Invariant)
        } else {
            Ok(AgentAuditRecordV1(self.bytes))
        }
    }
}

fn encode_activity(encoder: &mut AuditRecordEncoder, progress: AgentSemanticProgress) {
    let activity = progress.activity();
    match activity.operation() {
        AgentProgressOperation::Scheduling => encoder.update([1]),
        AgentProgressOperation::Planning => encoder.update([2]),
        AgentProgressOperation::Delegation => encoder.update([3]),
        AgentProgressOperation::Model => encoder.update([4]),
        AgentProgressOperation::Observation => encoder.update([5]),
        AgentProgressOperation::Read => encoder.update([6]),
        AgentProgressOperation::Context => encoder.update([7]),
        AgentProgressOperation::Effect(effect) => {
            encoder.update([8, effect_code(effect)]);
        }
        AgentProgressOperation::Verification => encoder.update([9]),
        AgentProgressOperation::Approval(effect) => {
            encoder.update([10, effect_code(effect)]);
        }
        AgentProgressOperation::Persistence => encoder.update([11]),
    }
    match activity.resource() {
        None => encoder.update([0]),
        Some(AgentProgressResource::Execution(value)) => {
            encoder.update([1]);
            encoder.update(value.get().to_be_bytes());
        }
        Some(AgentProgressResource::PlanNode(value)) => {
            encoder.update([2]);
            encoder.update(value.bytes());
        }
        Some(AgentProgressResource::ModelCall(value)) => {
            encoder.update([3]);
            encoder.update(value.get().to_be_bytes());
        }
        Some(AgentProgressResource::Context(value)) => {
            encoder.update([4]);
            encoder.update(value.bytes());
        }
        Some(AgentProgressResource::Effect(value)) => {
            encoder.update([5]);
            encoder.update(value.get().to_be_bytes());
        }
    }
    encoder.update([match progress.state() {
        AgentProgressState::Queued => 1,
        AgentProgressState::Active => 2,
        AgentProgressState::Waiting => 3,
        AgentProgressState::Succeeded => 4,
        AgentProgressState::Failed => 5,
        AgentProgressState::Cancelled => 6,
    }]);
    encode_result(encoder, progress.result());
    encode_blocker(encoder, progress.blocker());
}

fn encode_result(encoder: &mut AuditRecordEncoder, result: Option<AgentProgressResult>) {
    match result {
        None => encoder.update([0]),
        Some(AgentProgressResult::Supervisor(outcome)) => {
            encoder.update([1]);
            encode_supervisor_outcome(encoder, outcome);
        }
        Some(AgentProgressResult::Model(outcome)) => encoder.update([
            2,
            match outcome {
                crate::AgentModelCallSettlement::Completed => 1,
                crate::AgentModelCallSettlement::ProviderFailed => 2,
                crate::AgentModelCallSettlement::Cancelled => 3,
            },
        ]),
        Some(AgentProgressResult::Effect(outcome)) => {
            encoder.update([3]);
            match outcome {
                crate::AgentEffectSettlement::Verified(proof) => {
                    encoder.update([1, proof_code(proof)]);
                }
                crate::AgentEffectSettlement::Failed(failure) => {
                    encoder.update([2, action_failure_code(failure)]);
                }
            }
        }
        Some(AgentProgressResult::Context(outcome)) => {
            encoder.update([4]);
            match outcome {
                crate::AgentSupervisorContextReleaseOutcome::QueuedCancelled => {
                    encoder.update([1]);
                }
                crate::AgentSupervisorContextReleaseOutcome::Retired { terminal, resource } => {
                    encoder.update([2, context_terminal_code(terminal), resource_code(resource)]);
                }
            }
        }
    }
}

fn encode_supervisor_outcome(
    encoder: &mut AuditRecordEncoder,
    outcome: AgentSupervisorExecutionOutcome,
) {
    match outcome {
        AgentSupervisorExecutionOutcome::Waiting(wait) => {
            encoder.update([1, wait_code(wait)]);
        }
        AgentSupervisorExecutionOutcome::Succeeded => encoder.update([2]),
        AgentSupervisorExecutionOutcome::Failed(failure) => {
            encoder.update([3]);
            encode_supervisor_failure(encoder, failure);
        }
        AgentSupervisorExecutionOutcome::Cancelled(cancellation) => {
            encoder.update([4, cancellation_reason_code(cancellation.reason())]);
            encoder.update(cancellation.id().get().to_be_bytes());
        }
    }
}

fn encode_blocker(encoder: &mut AuditRecordEncoder, blocker: Option<AgentProgressBlocker>) {
    match blocker {
        None => encoder.update([0]),
        Some(AgentProgressBlocker::Scheduler(wait)) => encoder.update([1, wait_code(wait)]),
        Some(AgentProgressBlocker::NeedsHuman(reason)) => encoder.update([
            2,
            match reason {
                crate::AgentNeedsHumanReason::HumanControl => 1,
                crate::AgentNeedsHumanReason::CapabilityBoundary => 2,
                crate::AgentNeedsHumanReason::ScopeExpansion => 3,
                crate::AgentNeedsHumanReason::DataFlowApproval => 4,
                crate::AgentNeedsHumanReason::CrossOriginWrite => 5,
            },
        ]),
        Some(AgentProgressBlocker::Supervisor(failure)) => {
            encoder.update([3]);
            encode_supervisor_failure(encoder, failure);
        }
        Some(AgentProgressBlocker::Effect(failure)) => {
            encoder.update([4, action_failure_code(failure)]);
        }
        Some(AgentProgressBlocker::Provider) => encoder.update([5]),
        Some(AgentProgressBlocker::ModelCancelled) => encoder.update([6]),
        Some(AgentProgressBlocker::Cancellation(reason)) => {
            encoder.update([7, cancellation_reason_code(reason)]);
        }
        Some(AgentProgressBlocker::ContextCancelled) => encoder.update([8]),
    }
}

fn encode_supervisor_failure(encoder: &mut AuditRecordEncoder, failure: AgentSupervisorFailure) {
    match failure {
        AgentSupervisorFailure::BudgetExhausted => encoder.update([1]),
        AgentSupervisorFailure::ProviderFailed => encoder.update([2]),
        AgentSupervisorFailure::InvalidModelOutput => encoder.update([3]),
        AgentSupervisorFailure::PolicyDenied => encoder.update([4]),
        AgentSupervisorFailure::Action(failure) => {
            encoder.update([5, action_failure_code(failure)]);
        }
        AgentSupervisorFailure::ResourceExhausted => encoder.update([6]),
    }
}

const fn effect_code(effect: SemanticEffectClass) -> u8 {
    match effect {
        SemanticEffectClass::Read => 1,
        SemanticEffectClass::LocalWrite => 2,
        SemanticEffectClass::ExternalWrite => 3,
        SemanticEffectClass::Communication => 4,
        SemanticEffectClass::Purchase => 5,
        SemanticEffectClass::Destructive => 6,
        SemanticEffectClass::CapabilityBoundary => 7,
    }
}

const fn proof_code(proof: SemanticEffectProofKind) -> u8 {
    match proof {
        SemanticEffectProofKind::TargetState => 1,
        SemanticEffectProofKind::ExactTargetValue => 2,
        SemanticEffectProofKind::TargetValueChanged => 3,
        SemanticEffectProofKind::ExactSelection => 4,
        SemanticEffectProofKind::SelectionChanged => 5,
        SemanticEffectProofKind::Navigation => 6,
        SemanticEffectProofKind::Dialog => 7,
        SemanticEffectProofKind::Scroll => 8,
    }
}

const fn wait_code(wait: AgentSupervisorWait) -> u8 {
    match wait {
        AgentSupervisorWait::Descendants => 1,
        AgentSupervisorWait::Contexts => 2,
        AgentSupervisorWait::Yielded => 3,
    }
}

const fn cancellation_reason_code(reason: AgentSupervisorCancellationReason) -> u8 {
    match reason {
        AgentSupervisorCancellationReason::UserRequested => 1,
        AgentSupervisorCancellationReason::HumanTakeover => 2,
        AgentSupervisorCancellationReason::ParentTerminated => 3,
        AgentSupervisorCancellationReason::DeadlineExceeded => 4,
        AgentSupervisorCancellationReason::BudgetExhausted => 5,
        AgentSupervisorCancellationReason::PolicyRevoked => 6,
        AgentSupervisorCancellationReason::Shutdown => 7,
    }
}

const fn action_failure_code(failure: SemanticActionFailure) -> u8 {
    match failure {
        SemanticActionFailure::StaleReference => 1,
        SemanticActionFailure::TargetChanged => 2,
        SemanticActionFailure::TargetDisabled => 3,
        SemanticActionFailure::CredentialBoundary => 4,
        SemanticActionFailure::TargetOccluded => 5,
        SemanticActionFailure::UnsupportedInteraction => 6,
        SemanticActionFailure::BlockedOrigin => 7,
        SemanticActionFailure::LeaseViolation => 8,
        SemanticActionFailure::NeedsHuman => 9,
        SemanticActionFailure::HumanControlChanged => 10,
        SemanticActionFailure::NavigationReplaced => 11,
        SemanticActionFailure::RendererLost => 12,
        SemanticActionFailure::Timeout => 13,
        SemanticActionFailure::VerificationFailed => 14,
        SemanticActionFailure::Cancelled => 15,
        SemanticActionFailure::ResourceExhausted => 16,
        SemanticActionFailure::BackendRefused => 17,
    }
}

const fn context_terminal_code(terminal: ContextTerminal) -> u8 {
    match terminal {
        ContextTerminal::Closed => 1,
        ContextTerminal::Released => 2,
        ContextTerminal::Adopted => 3,
    }
}

const fn resource_code(resource: ContextResourceDisposition) -> u8 {
    match resource {
        ContextResourceDisposition::Destroyed => 1,
        ContextResourceDisposition::TransferredToBrowse => 2,
        ContextResourceDisposition::ExistingBrowseRetained => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AgentAccountScope, AgentDelegationSpec, AgentDelegationTopology, AgentEffectScope,
        AgentPlanNodeAuthority, AgentPlanNodeId, AgentPlanNodeScope, AgentProgressActivity,
        AgentRunBudget, AgentRunScope, ContextRunId, SemanticOrigin, SemanticSensitivity,
    };
    use zephium_core::ids::ProfileId;

    fn manifest(id: u128) -> AgentRunManifest {
        let profile = ProfileId::from(1);
        let origin = SemanticOrigin::parse("https://audit.example.test/private?token=hidden")
            .expect("origin");
        let effects =
            AgentEffectScope::try_new(&[SemanticEffectClass::Read]).expect("effect scope");
        AgentRunManifest::try_new(
            AgentRunManifestId::from_raw(id),
            ContextRunId::from_raw(2),
            AgentRunScope::try_new(
                vec![profile],
                vec![AgentAccountScope::Anonymous],
                vec![origin.clone()],
                SemanticSensitivity::Public,
                effects,
                Vec::new(),
            )
            .expect("run scope"),
            AgentRunBudget::try_new(100, 1_000, 1_000, 1).expect("run budget"),
            AgentPolicyInstant::from_millis(100),
            AgentPolicyInstant::from_millis(10_000),
            vec![AgentPlanNodeScope::new(
                AgentPlanNodeId::from_raw(1),
                AgentPlanNodeAuthority::try_new(
                    vec![profile],
                    vec![AgentAccountScope::Anonymous],
                    vec![origin],
                    SemanticSensitivity::Public,
                    effects,
                )
                .expect("node authority"),
                AgentRunBudget::try_new(100, 1_000, 1_000, 1).expect("node budget"),
                AgentPolicyInstant::from_millis(9_000),
            )],
        )
        .expect("manifest")
    }

    fn make_supervisor(manifest: &AgentRunManifest, id: u64) -> AgentRunSupervisor {
        let topology = AgentDelegationTopology::try_new(
            manifest,
            vec![AgentDelegationSpec::new(AgentPlanNodeId::from_raw(1), None)],
        )
        .expect("topology");
        AgentRunSupervisor::new(AgentSupervisorId::new(id).expect("supervisor"), topology)
    }

    fn event(value: u64) -> AgentAuditEventId {
        AgentAuditEventId::new(value).expect("event")
    }

    fn delivery_id(value: u64) -> AgentAuditDeliveryId {
        AgentAuditDeliveryId::new(value).expect("delivery")
    }

    fn attempt(value: u64) -> crate::AgentSupervisorAttemptId {
        crate::AgentSupervisorAttemptId::new(value).expect("attempt")
    }

    #[test]
    fn ledger_joins_the_queued_root_and_records_only_exact_changed_progress() {
        let manifest = manifest(1);
        let mut supervisor = make_supervisor(&manifest, 1);
        let root = AgentPlanNodeId::from_raw(1);
        let mut ledger = AgentAuditLedger::try_new(&manifest, &supervisor).expect("ledger");
        let queued = ledger
            .record_current(
                &supervisor,
                root,
                event(1),
                AgentPolicyInstant::from_millis(100),
            )
            .expect("queued event");
        assert_eq!(
            queued.progress(),
            supervisor.semantic_progress(root).expect("progress")
        );
        let record = queued.persistence_record();
        assert_eq!(record.as_bytes().len(), AGENT_AUDIT_RECORD_V1_BYTES);
        assert_eq!(record.as_bytes()[0], 1);
        assert_eq!(&record.as_bytes()[1..9], &1_u64.to_be_bytes());
        assert_eq!(&record.as_bytes()[9..17], &100_u64.to_be_bytes());
        assert_eq!(&record.as_bytes()[17..33], &manifest.id().bytes());
        assert_eq!(&record.as_bytes()[33..41], &1_u64.to_be_bytes());
        assert_eq!(&record.as_bytes()[41..57], &root.bytes());
        assert_eq!(&record.as_bytes()[57..62], &[1, 0, 1, 0, 0]);
        assert!(record.as_bytes()[62..].iter().all(|byte| *byte == 0));
        assert_eq!(format!("{record:?}"), "AgentAuditRecordV1([redacted])");
        assert_eq!(ledger.status().pending(), 1);
        assert_eq!(
            ledger
                .record_current(
                    &supervisor,
                    root,
                    event(2),
                    AgentPolicyInstant::from_millis(100),
                )
                .expect_err("unchanged projection"),
            AgentAuditError::DuplicateProgress
        );

        let execution = supervisor.start(root, attempt(1)).expect("start root");
        ledger
            .record_current(
                &supervisor,
                root,
                event(2),
                AgentPolicyInstant::from_millis(101),
            )
            .expect("active event");
        assert_eq!(
            ledger
                .record_current(
                    &supervisor,
                    root,
                    event(2),
                    AgentPolicyInstant::from_millis(101),
                )
                .expect_err("event replay"),
            AgentAuditError::EventReplay
        );
        supervisor
            .record_progress_activity(
                &execution,
                AgentProgressActivity::try_new(AgentProgressOperation::Planning, None)
                    .expect("planning"),
            )
            .expect("planning progress");
        assert_eq!(
            ledger
                .record_current(
                    &supervisor,
                    root,
                    event(3),
                    AgentPolicyInstant::from_millis(99),
                )
                .expect_err("time regression"),
            AgentAuditError::EventTime
        );

        let foreign = make_supervisor(&manifest, 2);
        assert_eq!(
            ledger
                .record_current(
                    &foreign,
                    root,
                    event(3),
                    AgentPolicyInstant::from_millis(102),
                )
                .expect_err("foreign supervisor"),
            AgentAuditError::Authority
        );
        assert_eq!(ledger.status().pending(), 2);
        let debug = format!("{ledger:?} {queued:?}");
        assert!(debug.contains("[redacted]"));
        assert!(!debug.contains("audit.example.test"));
        assert!(!debug.contains("hidden"));

        let started = make_supervisor(&manifest, 3);
        let mut started = started;
        let _execution = started.start(root, attempt(1)).expect("start root");
        assert_eq!(
            AgentAuditLedger::try_new(&manifest, &started).expect_err("late ledger"),
            AgentAuditError::StartState
        );
    }

    #[test]
    fn delivery_is_bounded_recoverable_and_removes_only_exact_committed_prefixes() {
        let manifest = manifest(2);
        let mut supervisor = make_supervisor(&manifest, 10);
        let root = AgentPlanNodeId::from_raw(1);
        let mut ledger = AgentAuditLedger::try_new(&manifest, &supervisor).expect("ledger");
        ledger
            .record_current(
                &supervisor,
                root,
                event(1),
                AgentPolicyInstant::from_millis(100),
            )
            .expect("queued");
        let execution = supervisor.start(root, attempt(1)).expect("start");
        ledger
            .record_current(
                &supervisor,
                root,
                event(2),
                AgentPolicyInstant::from_millis(101),
            )
            .expect("active");
        supervisor
            .record_progress_activity(
                &execution,
                AgentProgressActivity::try_new(AgentProgressOperation::Planning, None)
                    .expect("planning"),
            )
            .expect("planning progress");
        ledger
            .record_current(
                &supervisor,
                root,
                event(3),
                AgentPolicyInstant::from_millis(102),
            )
            .expect("planning");

        let first = ledger
            .begin_delivery(delivery_id(1), 2)
            .expect("first delivery");
        let first_proof = first.proof();
        assert_eq!(first.events().len(), 2);
        assert_eq!(first_proof.first(), event(1));
        assert_eq!(first_proof.last(), event(2));
        supervisor
            .wait(execution, AgentSupervisorWait::Yielded)
            .expect("yield while prefix is in flight");
        ledger
            .record_current(
                &supervisor,
                root,
                event(4),
                AgentPolicyInstant::from_millis(103),
            )
            .expect("append behind in-flight prefix");
        assert_eq!(
            ledger
                .begin_delivery(delivery_id(2), 1)
                .expect_err("single flight"),
            AgentAuditError::DeliveryPending
        );
        let recovered = ledger
            .current_delivery()
            .expect("reconcile")
            .expect("current delivery");
        assert_eq!(recovered.proof(), first_proof);
        assert_eq!(
            ledger
                .settle_delivery(first_proof.settle(AgentAuditDeliveryOutcome::Refused(
                    AgentAuditSinkFailure::Unavailable,
                )))
                .expect("refusal"),
            AgentAuditDeliveryOutcome::Refused(AgentAuditSinkFailure::Unavailable)
        );
        assert_eq!(ledger.status().pending(), 4);
        assert_eq!(ledger.status().in_flight(), 0);

        let retry = ledger.begin_delivery(delivery_id(2), 2).expect("retry");
        assert_eq!(
            ledger
                .settle_delivery(retry.proof().settle(AgentAuditDeliveryOutcome::Committed))
                .expect("commit prefix"),
            AgentAuditDeliveryOutcome::Committed
        );
        assert_eq!(ledger.status().pending(), 2);
        assert_eq!(ledger.status().committed(), 2);

        let cancelled = ledger
            .begin_delivery(delivery_id(3), 16)
            .expect("cancel delivery");
        ledger
            .settle_delivery(
                cancelled
                    .proof()
                    .settle(AgentAuditDeliveryOutcome::Cancelled),
            )
            .expect("cancelled");
        assert_eq!(ledger.status().pending(), 2);
        let final_delivery = ledger
            .begin_delivery(delivery_id(4), 16)
            .expect("final delivery");
        ledger
            .settle_delivery(
                final_delivery
                    .proof()
                    .settle(AgentAuditDeliveryOutcome::Committed),
            )
            .expect("final commit");
        ledger.seal_for_shutdown().expect("shutdown seal");
        assert!(ledger.is_quiescent());
        assert_eq!(ledger.status().committed(), 4);
        assert_eq!(
            ledger
                .record_current(
                    &supervisor,
                    root,
                    event(5),
                    AgentPolicyInstant::from_millis(104),
                )
                .expect_err("shutdown record"),
            AgentAuditError::ShutdownSealed
        );
    }

    #[test]
    fn mismatched_delivery_fail_stops_without_losing_the_retained_prefix() {
        let manifest = manifest(3);
        let supervisor = make_supervisor(&manifest, 20);
        let root = AgentPlanNodeId::from_raw(1);
        let mut ledger = AgentAuditLedger::try_new(&manifest, &supervisor).expect("ledger");
        ledger
            .record_current(
                &supervisor,
                root,
                event(1),
                AgentPolicyInstant::from_millis(100),
            )
            .expect("queued");
        let delivery = ledger.begin_delivery(delivery_id(1), 1).expect("delivery");
        let exact = delivery.proof();
        let mut wrong = exact;
        wrong.guard[0] ^= 1;
        assert_eq!(
            ledger
                .settle_delivery(wrong.settle(AgentAuditDeliveryOutcome::Committed))
                .expect_err("ambiguous settlement"),
            AgentAuditError::DeliveryMismatch
        );
        assert!(ledger.status().fail_stopped());
        assert_eq!(ledger.status().pending(), 1);
        assert_eq!(ledger.status().in_flight(), 1);
        assert_eq!(
            ledger
                .begin_delivery(delivery_id(2), 1)
                .expect_err("fail stopped"),
            AgentAuditError::FailStopped
        );
        ledger
            .settle_delivery(exact.settle(AgentAuditDeliveryOutcome::Committed))
            .expect("exact late acknowledgement can drain");
        assert_eq!(ledger.status().pending(), 0);
        assert_eq!(ledger.status().committed(), 1);
        assert!(ledger.status().fail_stopped());
    }

    #[test]
    fn pending_event_ceiling_backpressures_without_consuming_identity() {
        let manifest = manifest(4);
        let mut supervisor = make_supervisor(&manifest, 30);
        let root = AgentPlanNodeId::from_raw(1);
        let mut ledger = AgentAuditLedger::try_new(&manifest, &supervisor).expect("ledger");
        ledger
            .record_current(
                &supervisor,
                root,
                event(1),
                AgentPolicyInstant::from_millis(100),
            )
            .expect("queued");
        let mut next_event = 2_u64;
        let mut next_attempt = 1_u64;
        for _ in 0..31 {
            let execution = supervisor
                .start(root, attempt(next_attempt))
                .expect("start");
            next_attempt += 1;
            ledger
                .record_current(
                    &supervisor,
                    root,
                    event(next_event),
                    AgentPolicyInstant::from_millis(100),
                )
                .expect("active");
            next_event += 1;
            supervisor
                .wait(execution, AgentSupervisorWait::Yielded)
                .expect("yield");
            ledger
                .record_current(
                    &supervisor,
                    root,
                    event(next_event),
                    AgentPolicyInstant::from_millis(100),
                )
                .expect("waiting");
            next_event += 1;
        }
        let execution = supervisor
            .start(root, attempt(next_attempt))
            .expect("final start");
        ledger
            .record_current(
                &supervisor,
                root,
                event(next_event),
                AgentPolicyInstant::from_millis(100),
            )
            .expect("sixty-fourth event");
        next_event += 1;
        assert_eq!(
            ledger.status().pending(),
            MAX_PENDING_AGENT_AUDIT_EVENTS as u8
        );
        supervisor
            .record_progress_activity(
                &execution,
                AgentProgressActivity::try_new(AgentProgressOperation::Planning, None)
                    .expect("planning"),
            )
            .expect("planning progress");
        assert_eq!(
            ledger
                .record_current(
                    &supervisor,
                    root,
                    event(next_event),
                    AgentPolicyInstant::from_millis(100),
                )
                .expect_err("backpressure"),
            AgentAuditError::EventLimit
        );

        let batch = ledger
            .begin_delivery(delivery_id(1), MAX_AGENT_AUDIT_DELIVERY_EVENTS)
            .expect("bounded drain");
        ledger
            .settle_delivery(batch.proof().settle(AgentAuditDeliveryOutcome::Committed))
            .expect("commit drain");
        ledger
            .record_current(
                &supervisor,
                root,
                event(next_event),
                AgentPolicyInstant::from_millis(100),
            )
            .expect("capacity refusal did not consume event id");
        assert_eq!(ledger.status().pending(), 49);
    }

    #[test]
    fn port_receives_only_the_closed_delivery_contract() {
        struct RecordingPort;

        impl AgentAuditPort for RecordingPort {
            fn append(
                &self,
                delivery: AgentAuditDelivery,
                completion: AgentAuditCompletion,
            ) -> AgentAuditDispatch {
                assert_eq!(delivery.events().len(), 1);
                completion(
                    delivery
                        .proof()
                        .settle(AgentAuditDeliveryOutcome::Committed),
                );
                AgentAuditDispatch::Accepted(delivery.proof())
            }
        }

        let manifest = manifest(5);
        let supervisor = make_supervisor(&manifest, 40);
        let root = AgentPlanNodeId::from_raw(1);
        let mut ledger = AgentAuditLedger::try_new(&manifest, &supervisor).expect("ledger");
        ledger
            .record_current(
                &supervisor,
                root,
                event(1),
                AgentPolicyInstant::from_millis(100),
            )
            .expect("queued");
        let delivery = ledger.begin_delivery(delivery_id(1), 1).expect("delivery");
        let (settled_tx, settled_rx) = std::sync::mpsc::sync_channel(1);
        let AgentAuditDispatch::Accepted(proof) = RecordingPort.append(
            delivery,
            Box::new(move |settlement| settled_tx.send(settlement).expect("settlement receiver")),
        ) else {
            panic!("port accepted the fixed batch");
        };
        let settlement = settled_rx.recv().expect("settlement");
        assert_eq!(settlement.proof(), proof);
        ledger.settle_delivery(settlement).expect("commit");
        assert_eq!(ledger.status().pending(), 0);
    }
}
