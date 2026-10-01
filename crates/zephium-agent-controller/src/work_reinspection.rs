//! One explicit read of independently opened state after an uncertain write.
//! Original effect accounting and original resource quarantine remain unchanged.
use std::fmt;
use std::sync::Arc;
use zephium_agentic::*;

const MAX_READ_MILLIS: u64 = 30_000;

#[cfg(all(test, feature = "probe-harness"))]
#[path = "work_reinspection_tests.rs"]
pub(crate) mod tests;

/// Trusted, exact document and logical field selected for a separately approved
/// read. Construct this from user/task authority before capturing state, never
/// from a model proposal or matching page value. It authorizes no native work.
pub struct AgentWorkEffectReadTarget {
    document: ContextNavigationTarget,
    name: Option<SemanticActionText>,
}
impl AgentWorkEffectReadTarget {
    /// Selects an exact destination and accessible name. An absent name requires
    /// exactly one control with the original action's role in a complete read.
    pub fn try_new(
        document: ContextNavigationTarget,
        name: Option<String>,
    ) -> Result<Self, AgentWorkEffectReinspectionError> {
        if name
            .as_ref()
            .is_some_and(|name| name.is_empty() || name.len() > MAX_SEMANTIC_NAME_BYTES)
        {
            return Err(AgentWorkEffectReinspectionError::Target);
        }
        Ok(Self {
            document,
            name: name
                .map(SemanticActionText::try_new)
                .transpose()
                .map_err(|_| AgentWorkEffectReinspectionError::Target)?,
        })
    }
}
impl fmt::Debug for AgentWorkEffectReadTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AgentWorkEffectReadTarget([redacted])")
    }
}

/// Closed result of independent current-state comparison. Neither variant
/// asserts that the original command caused this state or proves remote saving.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentWorkEffectObservedState {
    /// The separately captured complete value equals the original fill input.
    ExpectedValueObserved,
    /// The uniquely identified field contains another complete value.
    DifferentValueObserved,
}

/// One move-only independently captured state record. No method turns this into
/// a successful original effect receipt, restores refs, or grants continuation.
#[must_use]
pub struct AgentWorkEffectReobservation {
    pub(crate) owner: Arc<()>,
    receipt: AgentEffectReceipt,
    state: AgentWorkEffectObservedState,
    frame: SemanticFrameJoin,
    invocation: SemanticInvocationId,
    snapshot: SemanticSnapshotGeneration,
    observed_at: AgentPolicyInstant,
}
impl AgentWorkEffectReobservation {
    /// Original failed receipt, preserved verbatim with its failure classification.
    pub const fn original_effect(&self) -> AgentEffectReceipt {
        self.receipt
    }
    /// Current-state comparison; never a native execution or save proof.
    pub const fn state(&self) -> AgentWorkEffectObservedState {
        self.state
    }
    /// Exact independently opened context that supplied this record.
    pub const fn frame(&self) -> &SemanticFrameJoin {
        &self.frame
    }
    /// Exact new read invocation, without exposing page text or old references.
    pub const fn invocation(&self) -> SemanticInvocationId {
        self.invocation
    }
    /// Snapshot lineage of the independently captured document.
    pub const fn snapshot(&self) -> SemanticSnapshotGeneration {
        self.snapshot
    }
    /// Original trusted-shell time of the accounted observation.
    pub const fn observed_at(&self) -> AgentPolicyInstant {
        self.observed_at
    }
}
impl fmt::Debug for AgentWorkEffectReobservation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentWorkEffectReobservation")
            .field("original_effect", &self.receipt)
            .field("state", &self.state)
            .field("observed_at", &self.observed_at)
            .finish_non_exhaustive()
    }
}

/// Explicit, single-read reconciliation owner. Its native read must use the
/// ordinary retained port and an independently acquired resource. This object
/// retains no old observation, model transcript, native handle or write recipe.
#[must_use]
pub struct AgentWorkEffectReinspection {
    owner: Arc<()>,
    receipt: AgentEffectReceipt,
    binding: WorkBrowserReadBinding,
    correlation: SemanticRuntimeCorrelation,
    target: AgentWorkEffectReadTarget,
    expected: SemanticActionText,
    role: SemanticRole,
    source_account: AgentContextAccountBinding,
    issued_at: AgentPolicyInstant,
    deadline: AgentPolicyInstant,
}
impl AgentWorkEffectReinspection {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare(
        owner: Arc<()>,
        receipt: AgentEffectReceipt,
        action: &SemanticPreparedAction,
        source_account: AgentContextAccountBinding,
        resources: &mut WorkBrowserResources,
        lease: &WorkBrowserExecutionLease,
        target: AgentWorkEffectReadTarget,
        now: AgentPolicyInstant,
    ) -> Result<(Self, WorkBrowserObservationRequest), AgentWorkEffectReinspectionError> {
        let binding = resources
            .read_binding(lease, now)
            .map_err(AgentWorkEffectReinspectionError::Resource)?;
        let source = action.frame().context();
        let current = binding.frame().context();
        if action.kind() != SemanticActionKind::Fill
            || action.verification() != SemanticVerification::TargetValueMatchesInput
            || !matches!(
                action.bound_action().target_role(),
                SemanticRole::Textbox | SemanticRole::Searchbox | SemanticRole::Combobox
            )
            || source_account.context() != source
            || current.identity().profile() != source.identity().profile()
            || current.identity().owner() != source.identity().owner()
            || current.identity().id() == source.identity().id()
            || binding.frame().origin() != action.frame().origin()
            || binding.document() != &target.document
            || binding.current_requested_document() != &target.document
            || now < source_account.observed_at()
        {
            return Err(AgentWorkEffectReinspectionError::Binding);
        }
        let expected = action
            .fill_text()
            .filter(|value| value.len() <= MAX_SEMANTIC_VALUE_PREVIEW_BYTES)
            .ok_or(AgentWorkEffectReinspectionError::Target)?
            .clone();
        let deadline = AgentPolicyInstant::from_millis(
            now.millis()
                .checked_add(MAX_READ_MILLIS)
                .ok_or(AgentWorkEffectReinspectionError::Deadline)?
                .min(lease.deadline().millis()),
        );
        if now >= deadline {
            return Err(AgentWorkEffectReinspectionError::Deadline);
        }
        let request = resources
            .observe_initial(lease, now)
            .map_err(AgentWorkEffectReinspectionError::Resource)?;
        let correlation = request.invocation().correlation();
        Ok((
            Self {
                owner,
                receipt,
                binding,
                correlation,
                target,
                expected,
                role: action.bound_action().target_role(),
                source_account,
                issued_at: now,
                deadline,
            },
            request,
        ))
    }

    /// Exact admitted read binding; descriptive only, never a replacement lease.
    pub const fn binding(&self) -> &WorkBrowserReadBinding {
        &self.binding
    }

    /// Rejects foreign callbacks without consuming either original owner. This
    /// check must precede routing a callback to `finish`.
    pub fn accepts_completion(&self, completion: &WorkBrowserObservationCompletion) -> bool {
        completion.matches(self.binding.lease(), &self.correlation)
    }

    /// Accounts one native read and compares independently observed current
    /// state. The caller must source fresh account facts from its trusted account
    /// adapter. Returning a record grants no further native/model work.
    pub fn finish(
        self,
        resources: &mut WorkBrowserResources,
        completion: WorkBrowserObservationCompletion,
        account: AgentContextAccountBinding,
        now: AgentPolicyInstant,
    ) -> Result<AgentWorkEffectReobservation, AgentWorkEffectReinspectionRefusal> {
        let registry = resources.phase(self.binding.lease().resource());
        if !self.accepts_completion(&completion) || registry.is_err() {
            return Err(AgentWorkEffectReinspectionRefusal {
                error: registry
                    .err()
                    .map(AgentWorkEffectReinspectionError::Resource)
                    .unwrap_or(AgentWorkEffectReinspectionError::Correlation),
                unmatched: Some((Box::new(self), Box::new(completion))),
            });
        }
        self.finish_exact(resources, completion, account, now)
            .map_err(|error| AgentWorkEffectReinspectionRefusal {
                error,
                unmatched: None,
            })
    }

    fn finish_exact(
        self,
        resources: &mut WorkBrowserResources,
        completion: WorkBrowserObservationCompletion,
        account: AgentContextAccountBinding,
        now: AgentPolicyInstant,
    ) -> Result<AgentWorkEffectReobservation, AgentWorkEffectReinspectionError> {
        // Always account an exact callback, even when its evidence is too late
        // or the independent account sample cannot authorize its use.
        let event = resources
            .settle_observation(completion, now)
            .map_err(AgentWorkEffectReinspectionError::Resource)?;
        let WorkBrowserObservationEvent::Snapshot(snapshot) = event else {
            return Err(AgentWorkEffectReinspectionError::Observation);
        };
        let current = resources
            .read_binding(self.binding.lease(), now)
            .map_err(AgentWorkEffectReinspectionError::Resource)?;
        if now < self.issued_at || now >= self.deadline {
            return Err(AgentWorkEffectReinspectionError::Deadline);
        }
        if current.frame() != self.binding.frame()
            || current.document() != &self.target.document
            || current.current_requested_document() != &self.target.document
            || snapshot.frame() != self.binding.frame()
            || snapshot.invocation() != self.correlation.invocation()
            || snapshot.generation() != self.correlation.snapshot_generation()
        {
            return Err(AgentWorkEffectReinspectionError::Binding);
        }
        if account.context() != snapshot.frame().context()
            || account.account() != self.source_account.account()
            || account.attestation() == self.source_account.attestation()
            || account.observed_at() < self.issued_at
            || account.observed_at() > now
            || now.millis() - account.observed_at().millis()
                > MAX_AGENT_ACCOUNT_ATTESTATION_AGE_MILLIS
        {
            return Err(AgentWorkEffectReinspectionError::Account);
        }
        let state = compare(
            &snapshot,
            self.role,
            self.target.name.as_ref(),
            &self.expected,
        )?;
        Ok(AgentWorkEffectReobservation {
            owner: self.owner,
            receipt: self.receipt,
            state,
            frame: snapshot.frame().clone(),
            invocation: snapshot.invocation(),
            snapshot: snapshot.generation(),
            observed_at: now,
        })
    }
}

/// Foreign callback routing is lossless. Exact callbacks are accounted even
/// when their evidence is refused; they cannot be replayed as another read.
#[must_use]
pub struct AgentWorkEffectReinspectionRefusal {
    error: AgentWorkEffectReinspectionError,
    unmatched: Option<(
        Box<AgentWorkEffectReinspection>,
        Box<WorkBrowserObservationCompletion>,
    )>,
}
impl AgentWorkEffectReinspectionRefusal {
    /// Closed rejection reason, without page content.
    pub const fn error(&self) -> AgentWorkEffectReinspectionError {
        self.error
    }
    /// Returns both unchanged owners only for a foreign callback. This is
    /// callback rerouting, not permission to issue another observation.
    pub fn into_unmatched(
        self,
    ) -> Option<(
        AgentWorkEffectReinspection,
        WorkBrowserObservationCompletion,
    )> {
        self.unmatched
            .map(|(ticket, completion)| (*ticket, *completion))
    }
}
impl fmt::Debug for AgentWorkEffectReinspectionRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentWorkEffectReinspectionRefusal")
            .field("error", &self.error)
            .field("unmatched", &self.unmatched.is_some())
            .finish()
    }
}
impl fmt::Debug for AgentWorkEffectReinspection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentWorkEffectReinspection")
            .field("receipt", &self.receipt)
            .field("issued_at", &self.issued_at)
            .field("deadline", &self.deadline)
            .finish_non_exhaustive()
    }
}

fn compare(
    snapshot: &SemanticSnapshot,
    role: SemanticRole,
    name: Option<&SemanticActionText>,
    expected: &SemanticActionText,
) -> Result<AgentWorkEffectObservedState, AgentWorkEffectReinspectionError> {
    if snapshot.completeness() != SemanticCompleteness::Complete {
        return Err(AgentWorkEffectReinspectionError::Observation);
    }
    // Identify before comparing values: a matching value cannot select its own
    // target, and duplicate controls cannot be resolved by guessing.
    let mut matches = snapshot.nodes().iter().filter(|node| {
        node.role() == role
            && name.is_none_or(|name| {
                node.name()
                    .is_some_and(|actual| actual.as_str() == name.as_str())
            })
    });
    let node = matches
        .next()
        .ok_or(AgentWorkEffectReinspectionError::Target)?;
    if matches.next().is_some() || node.sensitivity() == SemanticSensitivity::Secret {
        return Err(AgentWorkEffectReinspectionError::Target);
    }
    let Some(SemanticValueSummary::Text(value)) = node.value() else {
        return Err(AgentWorkEffectReinspectionError::Observation);
    };
    let value = value.preview();
    if value.truncated() {
        return Err(AgentWorkEffectReinspectionError::Observation);
    }
    Ok(if value.text() == expected.as_str() {
        AgentWorkEffectObservedState::ExpectedValueObserved
    } else {
        AgentWorkEffectObservedState::DifferentValueObserved
    })
}

/// Closed read/evidence refusal. No variant permits retry or action execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentWorkEffectReinspectionError {
    /// Original action has no exact, drained uncertain application to inspect.
    Unavailable,
    /// One read was already issued or recorded for this original owner.
    AlreadyIssued,
    /// Resource, profile, run, document or observation lineage differs.
    Binding,
    /// Original registry refused the read or callback accounting.
    Resource(WorkBrowserResourceError),
    /// Callback or evidence belongs to another exact read/action owner.
    Correlation,
    /// Original bounded read interval expired or the clock regressed.
    Deadline,
    /// Fresh independent account evidence is absent or mismatched.
    Account,
    /// Trusted field identity is missing, ambiguous, secret or unsupported.
    Target,
    /// Native observation is refused, incomplete, redacted or truncated.
    Observation,
}
