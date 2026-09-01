//! Bounded pre-policy semantic action proposals and exact snapshot binding.
//!
//! This module deliberately stops before policy authorization or native input.
//! It turns a small closed model-facing vocabulary into content-redacted guards
//! over one exact observation and current frame cohort. A later policy layer
//! must mint execution authority; these values alone cannot cause an effect.

use std::fmt;
use std::num::{NonZeroU32, NonZeroU64};

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::semantic::SemanticNodeKey;
use crate::semantic_wire::looks_like_secret_value;
use crate::{
    ContextJoin, FrameId, SemanticFrameJoin, SemanticObservation, SemanticObservationGeneration,
    SemanticObservationId, SemanticOperationClass, SemanticOperations, SemanticRect,
    SemanticReferenceError, SemanticReferenceId, SemanticRole, SemanticSensitivity,
    SemanticSnapshot, SemanticSnapshotGeneration, SemanticState, SemanticStates, SemanticTrust,
    MAX_SEMANTIC_FRAMES,
};

/// Maximum sequential actions proposed in one model turn.
pub const MAX_SEMANTIC_ACTIONS_PER_BATCH: usize = 8;
/// Maximum UTF-8 bytes in one fixed text-replacement action.
pub const MAX_SEMANTIC_ACTION_TEXT_BYTES: usize = 4 * 1024;
/// Maximum aggregate UTF-8 bytes carried by one action batch.
pub const MAX_SEMANTIC_ACTION_BATCH_TEXT_BYTES: usize = 16 * 1024;
/// Maximum settle budget for one action.
pub const MAX_SEMANTIC_ACTION_SETTLE_MILLIS: u32 = 30_000;
/// Maximum aggregate settle budget for one batch.
pub const MAX_SEMANTIC_ACTION_BATCH_SETTLE_MILLIS: u32 = 60_000;
/// Maximum quiet interval in a mutation-quiet wait.
pub const MAX_SEMANTIC_MUTATION_QUIET_MILLIS: u32 = 1_000;

/// Nonzero shell-minted identity for one exact bounded action batch.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SemanticActionBatchId(NonZeroU64);

impl SemanticActionBatchId {
    /// Constructs a nonzero process-local action-batch identity.
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the process-local correlation value to trusted orchestration.
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Debug for SemanticActionBatchId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SemanticActionBatchId([redacted])")
    }
}

/// Exact bounded text replacement carried only to a fixed fill recipe after policy.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticActionText(String);

impl SemanticActionText {
    /// Applies hard character/size bounds and refuses recognized secret forms.
    ///
    /// This heuristic refusal is defense in depth, not data classification.
    /// Deterministic provenance/source-to-sink policy remains mandatory before
    /// execution. Empty text is a valid clear operation.
    pub fn try_new(value: String) -> Result<Self, SemanticActionTextError> {
        if value.len() > MAX_SEMANTIC_ACTION_TEXT_BYTES {
            return Err(SemanticActionTextError::Limit);
        }
        if value.chars().any(invalid_action_character) {
            return Err(SemanticActionTextError::InvalidCharacter);
        }
        if looks_like_secret_value(&value) {
            return Err(SemanticActionTextError::Secret);
        }
        Ok(Self(value))
    }

    /// Returns exact replacement text only to trusted policy/native adapters.
    ///
    /// This value is plain text, never HTML, a selector, JavaScript, a property
    /// path, or a native handle. It must not be logged or traced.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Exact UTF-8 byte count used for aggregate batch admission.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether this action intentionally clears the target.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for SemanticActionText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticActionText")
            .field("bytes", &self.0.len())
            .field("content", &"[redacted]")
            .finish()
    }
}

fn invalid_action_character(character: char) -> bool {
    (character.is_control() && !matches!(character, '\t' | '\n' | '\r'))
        || matches!(
            character,
            '\u{00ad}'
                | '\u{061c}'
                | '\u{180e}'
                | '\u{200b}'..='\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{206f}'
                | '\u{feff}'
                | '\u{fff9}'..='\u{fffb}'
                | '\u{e0001}'
                | '\u{e0020}'..='\u{e007f}'
        )
}

/// Refusal while constructing fixed fill text.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticActionTextError {
    /// Text exceeded the fixed per-action byte ceiling.
    #[error("semantic action text exceeds its byte ceiling")]
    Limit,
    /// Text contained a control or invisible directional formatting character.
    #[error("semantic action text contains a forbidden character")]
    InvalidCharacter,
    /// Text matched a credential, token, authorization, or private-key form.
    #[error("semantic action text resembles a secret")]
    Secret,
}

/// Closed model-proposable semantic operation class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticActionKind {
    /// Activate one observed control.
    Click,
    /// Replace text in one observed editable control.
    Fill,
    /// Select one observed option within an observed select-like control.
    Select,
    /// Dispatch one fixed key recipe to an observed control.
    Press,
    /// Scroll one observed document or region through a fixed recipe.
    Scroll,
}

impl SemanticActionKind {
    /// Snapshot operation capability required by this action class.
    pub const fn operation(self) -> SemanticOperationClass {
        match self {
            Self::Click => SemanticOperationClass::Click,
            Self::Fill => SemanticOperationClass::Fill,
            Self::Select => SemanticOperationClass::Select,
            Self::Press => SemanticOperationClass::Press,
            Self::Scroll => SemanticOperationClass::Scroll,
        }
    }
}

/// Fixed key recipes; arbitrary key strings and chords are impossible.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticPressKey {
    /// Enter/Return.
    Enter,
    /// Escape.
    Escape,
    /// Space.
    Space,
    /// Tab in the ordinary forward direction.
    Tab,
    /// Up arrow.
    ArrowUp,
    /// Down arrow.
    ArrowDown,
    /// Left arrow.
    ArrowLeft,
    /// Right arrow.
    ArrowRight,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// Backspace.
    Backspace,
    /// Forward delete.
    Delete,
}

/// Closed scroll direction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticScrollDirection {
    /// Move toward smaller vertical coordinates.
    Up,
    /// Move toward larger vertical coordinates.
    Down,
    /// Move toward smaller horizontal coordinates.
    Left,
    /// Move toward larger horizontal coordinates.
    Right,
}

/// Closed scroll magnitude; arbitrary pixel deltas are impossible.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticScrollAmount {
    /// One engine-defined line step.
    Line,
    /// Half of the current visible extent.
    HalfPage,
    /// One current visible extent.
    Page,
    /// Minimum movement needed to expose the target region.
    IntoView,
}

/// Small model-facing intent containing only opaque references and fixed data.
#[derive(Clone, Eq, PartialEq)]
pub enum SemanticActionIntent {
    /// Activate one observed target.
    Click {
        /// Opaque current target reference.
        target: SemanticReferenceId,
    },
    /// Replace the target's safe text.
    Fill {
        /// Opaque current target reference.
        target: SemanticReferenceId,
        /// Bounded plain replacement text.
        value: SemanticActionText,
    },
    /// Select an observed option within an observed select-like target.
    Select {
        /// Opaque combobox/listbox reference.
        target: SemanticReferenceId,
        /// Opaque option reference from the same frame and snapshot.
        option: SemanticReferenceId,
    },
    /// Dispatch one fixed key recipe.
    Press {
        /// Opaque current target reference.
        target: SemanticReferenceId,
        /// Fixed key recipe.
        key: SemanticPressKey,
    },
    /// Scroll one observed region using fixed direction and magnitude.
    Scroll {
        /// Opaque current scroll-region reference.
        target: SemanticReferenceId,
        /// Fixed direction.
        direction: SemanticScrollDirection,
        /// Fixed magnitude.
        amount: SemanticScrollAmount,
    },
}

impl SemanticActionIntent {
    /// Closed action class.
    pub const fn kind(&self) -> SemanticActionKind {
        match self {
            Self::Click { .. } => SemanticActionKind::Click,
            Self::Fill { .. } => SemanticActionKind::Fill,
            Self::Select { .. } => SemanticActionKind::Select,
            Self::Press { .. } => SemanticActionKind::Press,
            Self::Scroll { .. } => SemanticActionKind::Scroll,
        }
    }

    /// Primary target reference.
    pub const fn target(&self) -> SemanticReferenceId {
        match self {
            Self::Click { target }
            | Self::Fill { target, .. }
            | Self::Select { target, .. }
            | Self::Press { target, .. }
            | Self::Scroll { target, .. } => *target,
        }
    }
}

impl fmt::Debug for SemanticActionIntent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut value = formatter.debug_struct("SemanticActionIntent");
        value
            .field("kind", &self.kind())
            .field("target", &self.target());
        match self {
            Self::Fill { value: text, .. } => {
                value.field("text", text);
            }
            Self::Select { option, .. } => {
                value.field("option", option);
            }
            Self::Press { key, .. } => {
                value.field("key", key);
            }
            Self::Scroll {
                direction, amount, ..
            } => {
                value.field("direction", direction).field("amount", amount);
            }
            Self::Click { .. } => {}
        }
        value.finish()
    }
}

/// Declared effect boundary. This declaration is untrusted until policy admits it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticEffectClass {
    /// Read-only page exploration with no durable write.
    Read,
    /// Reversible local page/form state not yet committed externally.
    LocalWrite,
    /// A durable write to an external service.
    ExternalWrite,
    /// A message, comment, email, or other communication.
    Communication,
    /// A purchase, paid subscription, or financial commitment.
    Purchase,
    /// A deletion or other destructive effect.
    Destructive,
    /// Upload, download, clipboard, popup, permission, credential, or OS boundary.
    CapabilityBoundary,
}

impl SemanticEffectClass {
    const fn requires_single_action(self) -> bool {
        matches!(
            self,
            Self::ExternalWrite
                | Self::Communication
                | Self::Purchase
                | Self::Destructive
                | Self::CapabilityBoundary
        )
    }
}

/// Whether a browser dialog must appear or disappear.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticDialogState {
    /// A dialog is present.
    Present,
    /// A previously present dialog is absent.
    Absent,
}

/// Bounded quiet interval used only by a typed settle condition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticMutationQuietPeriod(NonZeroU32);

impl SemanticMutationQuietPeriod {
    /// Validates a nonzero interval no longer than one second.
    pub const fn try_new(millis: u32) -> Result<Self, SemanticActionContractError> {
        match NonZeroU32::new(millis) {
            Some(value) if millis <= MAX_SEMANTIC_MUTATION_QUIET_MILLIS => Ok(Self(value)),
            _ => Err(SemanticActionContractError::QuietPeriod),
        }
    }

    /// Exact interval in milliseconds.
    pub const fn millis(self) -> u32 {
        self.0.get()
    }
}

/// One typed settle condition under the action's absolute deadline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticWaitCondition {
    /// Observe immediately after backend completion without an idle heuristic.
    Immediate,
    /// Wait for one exact native navigation commitment.
    NavigationCommitted,
    /// Wait for the current document to reach the adapter's fixed ready state.
    DocumentReady,
    /// Wait for the primary target to gain or lose one allowlisted state.
    TargetState {
        /// Allowlisted state.
        state: SemanticState,
        /// Required presence.
        present: bool,
    },
    /// Wait for the native-attested committed URL to change.
    UrlChanged,
    /// Wait for bounded semantic title state to change.
    TitleChanged,
    /// Wait for a browser/page dialog boundary.
    Dialog(SemanticDialogState),
    /// Wait for a semantic projection change.
    SemanticChange,
    /// Wait for bounded mutation quiet, never network idle.
    MutationQuiet(SemanticMutationQuietPeriod),
    /// Wait for an independently sampled scroll-position change.
    ScrollPositionChanged,
}

/// Independent effect proof required after the settle condition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticVerification {
    /// Primary target gained or lost one allowlisted state.
    TargetState {
        /// Allowlisted state.
        state: SemanticState,
        /// Required presence.
        present: bool,
    },
    /// Safe target value exactly matches the bounded fill input.
    TargetValueMatchesInput,
    /// Safe target value changed from its pre-action value after a fixed key recipe.
    TargetValueChanged,
    /// Target selection exactly matches the bound option.
    TargetSelectionMatchesOption,
    /// Target selection changed after a fixed key recipe.
    TargetSelectionChanged,
    /// One exact navigation committed under the current context authority.
    NavigationCommitted,
    /// Dialog state matches the declared state.
    Dialog(SemanticDialogState),
    /// Scroll position changed without claiming unrelated page effects.
    ScrollPositionChanged,
}

/// Bounded per-action settle budget; execution converts it to one absolute deadline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticSettleBudget(NonZeroU32);

impl SemanticSettleBudget {
    /// Validates a nonzero per-action budget at or below thirty seconds.
    pub const fn try_new(millis: u32) -> Result<Self, SemanticActionContractError> {
        match NonZeroU32::new(millis) {
            Some(value) if millis <= MAX_SEMANTIC_ACTION_SETTLE_MILLIS => Ok(Self(value)),
            _ => Err(SemanticActionContractError::SettleBudget),
        }
    }

    /// Exact relative budget used to derive an executor-owned absolute deadline.
    pub const fn millis(self) -> u32 {
        self.0.get()
    }
}

/// One model proposal. It is not policy or execution authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticActionProposal {
    intent: SemanticActionIntent,
    effect: SemanticEffectClass,
    wait: SemanticWaitCondition,
    verification: SemanticVerification,
    settle_budget: SemanticSettleBudget,
}

impl SemanticActionProposal {
    /// Validates operation, settle, and verification compatibility.
    pub fn try_new(
        intent: SemanticActionIntent,
        effect: SemanticEffectClass,
        wait: SemanticWaitCondition,
        verification: SemanticVerification,
        settle_budget: SemanticSettleBudget,
    ) -> Result<Self, SemanticActionContractError> {
        if !verification_matches(intent.kind(), verification) || !wait_matches(wait, verification) {
            return Err(SemanticActionContractError::OutcomeContract);
        }
        Ok(Self {
            intent,
            effect,
            wait,
            verification,
            settle_budget,
        })
    }

    /// Proposed fixed action intent.
    pub const fn intent(&self) -> &SemanticActionIntent {
        &self.intent
    }

    /// Untrusted declared effect boundary.
    pub const fn effect(&self) -> SemanticEffectClass {
        self.effect
    }

    /// Typed settle condition.
    pub const fn wait(&self) -> SemanticWaitCondition {
        self.wait
    }

    /// Required independent verification.
    pub const fn verification(&self) -> SemanticVerification {
        self.verification
    }

    /// Per-action time budget.
    pub const fn settle_budget(&self) -> SemanticSettleBudget {
        self.settle_budget
    }
}

fn verification_matches(kind: SemanticActionKind, verification: SemanticVerification) -> bool {
    match kind {
        SemanticActionKind::Click => matches!(
            verification,
            SemanticVerification::TargetState { .. }
                | SemanticVerification::NavigationCommitted
                | SemanticVerification::Dialog(_)
        ),
        SemanticActionKind::Fill => verification == SemanticVerification::TargetValueMatchesInput,
        SemanticActionKind::Select => {
            verification == SemanticVerification::TargetSelectionMatchesOption
        }
        SemanticActionKind::Press => matches!(
            verification,
            SemanticVerification::TargetState { .. }
                | SemanticVerification::TargetValueChanged
                | SemanticVerification::TargetSelectionChanged
                | SemanticVerification::NavigationCommitted
                | SemanticVerification::Dialog(_)
        ),
        SemanticActionKind::Scroll => verification == SemanticVerification::ScrollPositionChanged,
    }
}

fn wait_matches(wait: SemanticWaitCondition, verification: SemanticVerification) -> bool {
    match verification {
        SemanticVerification::NavigationCommitted => matches!(
            wait,
            SemanticWaitCondition::NavigationCommitted
                | SemanticWaitCondition::DocumentReady
                | SemanticWaitCondition::UrlChanged
        ),
        SemanticVerification::Dialog(state) => wait == SemanticWaitCondition::Dialog(state),
        SemanticVerification::TargetState { state, present } => {
            matches!(
                wait,
                SemanticWaitCondition::Immediate
                    | SemanticWaitCondition::SemanticChange
                    | SemanticWaitCondition::MutationQuiet(_)
            ) || wait == SemanticWaitCondition::TargetState { state, present }
        }
        SemanticVerification::ScrollPositionChanged => matches!(
            wait,
            SemanticWaitCondition::Immediate | SemanticWaitCondition::ScrollPositionChanged
        ),
        SemanticVerification::TargetValueMatchesInput
        | SemanticVerification::TargetValueChanged
        | SemanticVerification::TargetSelectionChanged
        | SemanticVerification::TargetSelectionMatchesOption => matches!(
            wait,
            SemanticWaitCondition::Immediate
                | SemanticWaitCondition::SemanticChange
                | SemanticWaitCondition::MutationQuiet(_)
                | SemanticWaitCondition::TargetState { .. }
        ),
    }
}

/// Refusal while constructing a proposed action outcome contract.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticActionContractError {
    /// Mutation-quiet interval was zero or over the hard bound.
    #[error("semantic mutation quiet period is invalid")]
    QuietPeriod,
    /// Per-action settle budget was zero or over the hard bound.
    #[error("semantic action settle budget is invalid")]
    SettleBudget,
    /// Action kind, wait, and independent verification disagree.
    #[error("semantic action outcome contract is incompatible")]
    OutcomeContract,
}

#[derive(Clone, Eq, PartialEq)]
struct BoundNode {
    reference: SemanticReferenceId,
    node_key: SemanticNodeKey,
    frame: SemanticFrameJoin,
    snapshot: SemanticSnapshotGeneration,
    role: SemanticRole,
    value: Option<crate::SemanticValueSummary>,
    states: SemanticStates,
    operations: SemanticOperations,
    sensitivity: SemanticSensitivity,
    trust: SemanticTrust,
    geometry: Option<SemanticRect>,
    structural_digest: [u8; 32],
}

impl fmt::Debug for BoundNode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BoundNode")
            .field("reference", &self.reference)
            .field("node_key", &self.node_key)
            .field("frame", &self.frame)
            .field("snapshot", &self.snapshot)
            .field("role", &self.role)
            .field("has_value", &self.value.is_some())
            .field("states", &self.states)
            .field("operations", &self.operations)
            .field("sensitivity", &self.sensitivity)
            .field("trust", &self.trust)
            .field("geometry", &self.geometry)
            .field("structural_digest", &"[redacted]")
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum BoundActionIntent {
    Click {
        target: BoundNode,
    },
    Fill {
        target: BoundNode,
        value: SemanticActionText,
    },
    Select {
        target: BoundNode,
        option: Box<BoundNode>,
    },
    Press {
        target: BoundNode,
        key: SemanticPressKey,
    },
    Scroll {
        target: BoundNode,
        direction: SemanticScrollDirection,
        amount: SemanticScrollAmount,
    },
}

impl BoundActionIntent {
    const fn kind(&self) -> SemanticActionKind {
        match self {
            Self::Click { .. } => SemanticActionKind::Click,
            Self::Fill { .. } => SemanticActionKind::Fill,
            Self::Select { .. } => SemanticActionKind::Select,
            Self::Press { .. } => SemanticActionKind::Press,
            Self::Scroll { .. } => SemanticActionKind::Scroll,
        }
    }

    const fn target(&self) -> &BoundNode {
        match self {
            Self::Click { target }
            | Self::Fill { target, .. }
            | Self::Select { target, .. }
            | Self::Press { target, .. }
            | Self::Scroll { target, .. } => target,
        }
    }
}

/// One observation-bound action. It still carries no policy execution permit.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticBoundAction {
    ordinal: u8,
    intent: BoundActionIntent,
    effect: SemanticEffectClass,
    wait: SemanticWaitCondition,
    verification: SemanticVerification,
    settle_budget: SemanticSettleBudget,
}

impl SemanticBoundAction {
    pub(crate) const fn target_key(&self) -> SemanticNodeKey {
        self.intent.target().node_key
    }

    pub(crate) const fn target_value(&self) -> Option<&crate::SemanticValueSummary> {
        self.intent.target().value.as_ref()
    }

    /// One-based position in the exact batch.
    pub const fn ordinal(&self) -> u8 {
        self.ordinal
    }

    /// Closed action class.
    pub const fn kind(&self) -> SemanticActionKind {
        self.intent.kind()
    }

    /// Original opaque target reference.
    pub const fn target_reference(&self) -> SemanticReferenceId {
        self.intent.target().reference
    }

    /// Bound exact action frame.
    pub const fn frame(&self) -> &SemanticFrameJoin {
        &self.intent.target().frame
    }

    /// Exact source snapshot generation.
    pub const fn snapshot_generation(&self) -> SemanticSnapshotGeneration {
        self.intent.target().snapshot
    }

    /// Target role observed when the proposal was bound.
    pub const fn target_role(&self) -> SemanticRole {
        self.intent.target().role
    }

    /// Target states observed when the proposal was bound.
    pub const fn target_states(&self) -> SemanticStates {
        self.intent.target().states
    }

    /// Geometry presence/value observed for later native revalidation.
    pub const fn target_geometry(&self) -> Option<SemanticRect> {
        self.intent.target().geometry
    }

    /// Declared effect boundary, still requiring policy admission.
    pub const fn effect(&self) -> SemanticEffectClass {
        self.effect
    }

    /// Typed settle condition.
    pub const fn wait(&self) -> SemanticWaitCondition {
        self.wait
    }

    /// Independent post-settle verification requirement.
    pub const fn verification(&self) -> SemanticVerification {
        self.verification
    }

    /// Per-action deadline budget.
    pub const fn settle_budget(&self) -> SemanticSettleBudget {
        self.settle_budget
    }

    /// Bounded fill text when this is a fill action.
    pub const fn fill_text(&self) -> Option<&SemanticActionText> {
        match &self.intent {
            BoundActionIntent::Fill { value, .. } => Some(value),
            _ => None,
        }
    }

    /// Exact option reference when this is a select action.
    pub const fn option_reference(&self) -> Option<SemanticReferenceId> {
        match &self.intent {
            BoundActionIntent::Select { option, .. } => Some(option.reference),
            _ => None,
        }
    }

    /// Fixed key recipe when this is a press action.
    pub const fn press_key(&self) -> Option<SemanticPressKey> {
        match &self.intent {
            BoundActionIntent::Press { key, .. } => Some(*key),
            _ => None,
        }
    }

    /// Fixed direction and magnitude when this is a scroll action.
    pub const fn scroll_recipe(&self) -> Option<(SemanticScrollDirection, SemanticScrollAmount)> {
        match &self.intent {
            BoundActionIntent::Scroll {
                direction, amount, ..
            } => Some((*direction, *amount)),
            _ => None,
        }
    }

    /// Revalidates stable identity and structural semantics in a fresh snapshot.
    ///
    /// This does not prove visibility, occlusion, policy, or backend success;
    /// those remain mandatory later pipeline stages.
    pub fn revalidate(
        &self,
        current: &SemanticSnapshot,
    ) -> Result<(), SemanticActionRevalidationError> {
        let (target_index, _) = revalidate_node(
            self.intent.target(),
            current,
            Some(self.kind().operation()),
            matches!(self.intent, BoundActionIntent::Fill { .. }),
        )?;
        if let BoundActionIntent::Select { option, .. } = &self.intent {
            let (option_index, option_node) = revalidate_node(option, current, None, false)
                .map_err(|_| SemanticActionRevalidationError::SelectionTarget)?;
            if option_node.role() != SemanticRole::Option
                || !is_descendant(current, option_index, target_index)
            {
                return Err(SemanticActionRevalidationError::SelectionTarget);
            }
        }
        Ok(())
    }

    pub(crate) fn verification_target<'a>(
        &self,
        current: &'a SemanticSnapshot,
    ) -> Result<(usize, &'a crate::SemanticNode), SemanticActionRevalidationError> {
        verification_node(self.intent.target(), current)
    }

    pub(crate) fn verification_option<'a>(
        &self,
        current: &'a SemanticSnapshot,
        target_index: usize,
    ) -> Result<(usize, &'a crate::SemanticNode), SemanticActionRevalidationError> {
        let BoundActionIntent::Select { option, .. } = &self.intent else {
            return Err(SemanticActionRevalidationError::SelectionTarget);
        };
        let (option_index, option_node) = verification_node(option, current)?;
        if option_node.role() != SemanticRole::Option
            || !is_descendant(current, option_index, target_index)
        {
            return Err(SemanticActionRevalidationError::SelectionTarget);
        }
        Ok((option_index, option_node))
    }

    pub(crate) fn verification_guard(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(b"ZEPHIUM-SEMANTIC-VERIFICATION-GUARD-1\0");
        hash_frame(&mut hasher, self.frame());
        hasher.update([self.ordinal]);
        hasher.update(self.target_key().get().to_be_bytes());
        hasher.update(self.snapshot_generation().get().to_be_bytes());
        hash_effect(&mut hasher, self.effect);
        hash_wait(&mut hasher, self.wait);
        hash_verification(&mut hasher, self.verification);
        hasher.update(self.settle_budget.millis().to_be_bytes());
        match &self.intent {
            BoundActionIntent::Click { .. } => hasher.update([1]),
            BoundActionIntent::Fill { value, .. } => {
                hasher.update([2]);
                hasher.update((value.len() as u64).to_be_bytes());
                hasher.update(value.as_str().as_bytes());
            }
            BoundActionIntent::Select { option, .. } => {
                hasher.update([3]);
                hasher.update(option.node_key.get().to_be_bytes());
            }
            BoundActionIntent::Press { key, .. } => {
                hasher.update([4, press_key_code(*key)]);
            }
            BoundActionIntent::Scroll {
                direction, amount, ..
            } => {
                hasher.update([
                    5,
                    scroll_direction_code(*direction),
                    scroll_amount_code(*amount),
                ]);
            }
        }
        hasher.finalize().into()
    }
}

impl fmt::Debug for SemanticBoundAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticBoundAction")
            .field("ordinal", &self.ordinal)
            .field("kind", &self.kind())
            .field("target", self.intent.target())
            .field("effect", &self.effect)
            .field("wait", &self.wait)
            .field("verification", &self.verification)
            .field("settle_budget", &self.settle_budget)
            .field("fill_bytes", &self.fill_text().map(SemanticActionText::len))
            .finish()
    }
}

/// Complete bounded action proposal bound to one exact current observation.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticActionBatch {
    id: SemanticActionBatchId,
    context: ContextJoin,
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
    effect: SemanticEffectClass,
    actions: Vec<SemanticBoundAction>,
    settle_millis: u32,
    text_bytes: usize,
}

impl SemanticActionBatch {
    /// Resolves all opaque references against an exact current frame cohort.
    ///
    /// Successful binding is a pre-policy guard only. It does not authorize or
    /// execute input. The current frame cohort must come from trusted native
    /// lifecycle state and match the observation exactly.
    pub fn bind(
        id: SemanticActionBatchId,
        observation: &SemanticObservation,
        current_frames: &[SemanticFrameJoin],
        proposals: Vec<SemanticActionProposal>,
    ) -> Result<Self, SemanticActionBindingError> {
        if proposals.is_empty() {
            return Err(SemanticActionBindingError::Empty);
        }
        if proposals.len() > MAX_SEMANTIC_ACTIONS_PER_BATCH {
            return Err(SemanticActionBindingError::ActionLimit);
        }
        validate_current_frames(observation, current_frames)?;

        let effect = proposals[0].effect;
        if proposals.iter().any(|proposal| proposal.effect != effect) {
            return Err(SemanticActionBindingError::MixedEffectBoundary);
        }
        if effect.requires_single_action() && proposals.len() != 1 {
            return Err(SemanticActionBindingError::EffectBatchLimit);
        }

        let mut actions = Vec::with_capacity(proposals.len());
        let mut settle_millis = 0_u32;
        let mut text_bytes = 0_usize;
        for (index, proposal) in proposals.into_iter().enumerate() {
            settle_millis = settle_millis
                .checked_add(proposal.settle_budget.millis())
                .ok_or(SemanticActionBindingError::SettleLimit)?;
            if settle_millis > MAX_SEMANTIC_ACTION_BATCH_SETTLE_MILLIS {
                return Err(SemanticActionBindingError::SettleLimit);
            }
            let ordinal =
                u8::try_from(index + 1).map_err(|_| SemanticActionBindingError::ActionLimit)?;
            let intent = bind_intent(
                observation,
                current_frames,
                proposal.intent,
                &mut text_bytes,
            )?;
            validate_bound_verification(&intent, proposal.verification)?;
            validate_verification_baseline(&intent, proposal.verification)?;
            actions.push(SemanticBoundAction {
                ordinal,
                intent,
                effect: proposal.effect,
                wait: proposal.wait,
                verification: proposal.verification,
                settle_budget: proposal.settle_budget,
            });
        }

        Ok(Self {
            id,
            context: observation.request().context(),
            observation: observation.request().id(),
            observation_generation: observation.request().generation(),
            effect,
            actions,
            settle_millis,
            text_bytes,
        })
    }

    /// Exact batch identity.
    pub const fn id(&self) -> SemanticActionBatchId {
        self.id
    }

    /// Exact context/document/cancellation authority represented at binding.
    pub const fn context(&self) -> ContextJoin {
        self.context
    }

    /// Observation request whose opaque references were consumed.
    pub const fn observation(&self) -> SemanticObservationId {
        self.observation
    }

    /// Progressive observation generation whose references were consumed.
    pub const fn observation_generation(&self) -> SemanticObservationGeneration {
        self.observation_generation
    }

    /// Homogeneous effect boundary for the complete batch.
    pub const fn effect(&self) -> SemanticEffectClass {
        self.effect
    }

    /// Sequential bound actions.
    pub fn actions(&self) -> &[SemanticBoundAction] {
        &self.actions
    }

    /// Aggregate relative budget from which execution derives one absolute deadline.
    pub const fn settle_millis(&self) -> u32 {
        self.settle_millis
    }

    /// Aggregate exact fill-text bytes.
    pub const fn text_bytes(&self) -> usize {
        self.text_bytes
    }
}

fn validate_bound_verification(
    intent: &BoundActionIntent,
    verification: SemanticVerification,
) -> Result<(), SemanticActionBindingError> {
    let compatible = match (intent, verification) {
        (BoundActionIntent::Press { target, .. }, SemanticVerification::TargetValueChanged) => {
            matches!(
                target.role,
                SemanticRole::Textbox
                    | SemanticRole::Searchbox
                    | SemanticRole::Spinbutton
                    | SemanticRole::Slider
            )
        }
        (BoundActionIntent::Press { target, .. }, SemanticVerification::TargetSelectionChanged) => {
            matches!(
                target.role,
                SemanticRole::Combobox
                    | SemanticRole::Listbox
                    | SemanticRole::Option
                    | SemanticRole::Radio
                    | SemanticRole::Tab
                    | SemanticRole::MenuItem
            )
        }
        (BoundActionIntent::Press { target, key }, SemanticVerification::NavigationCommitted) => {
            matches!(target.role, SemanticRole::Link | SemanticRole::Button)
                && matches!(key, SemanticPressKey::Enter | SemanticPressKey::Space)
        }
        (BoundActionIntent::Press { key, .. }, SemanticVerification::Dialog(_)) => matches!(
            key,
            SemanticPressKey::Enter | SemanticPressKey::Escape | SemanticPressKey::Space
        ),
        _ => true,
    };
    if compatible {
        Ok(())
    } else {
        Err(SemanticActionBindingError::OutcomeContract)
    }
}

fn validate_verification_baseline(
    intent: &BoundActionIntent,
    verification: SemanticVerification,
) -> Result<(), SemanticActionBindingError> {
    let already_satisfied = match verification {
        SemanticVerification::TargetState { state, present } => {
            intent.target().states.contains(state) == present
        }
        SemanticVerification::TargetValueMatchesInput => match intent {
            BoundActionIntent::Fill { target, value } => match target.value.as_ref() {
                Some(crate::SemanticValueSummary::Text(current)) => {
                    current.as_str() == value.as_str()
                }
                None => value.is_empty(),
                Some(
                    crate::SemanticValueSummary::Redacted
                    | crate::SemanticValueSummary::Boolean(_)
                    | crate::SemanticValueSummary::Ordinal(_),
                ) => false,
            },
            _ => false,
        },
        SemanticVerification::TargetSelectionMatchesOption => match intent {
            BoundActionIntent::Select { option, .. } => {
                option.states.contains(SemanticState::Selected)
            }
            _ => false,
        },
        SemanticVerification::TargetValueChanged
        | SemanticVerification::TargetSelectionChanged
        | SemanticVerification::NavigationCommitted
        | SemanticVerification::Dialog(_)
        | SemanticVerification::ScrollPositionChanged => false,
    };
    if already_satisfied {
        Err(SemanticActionBindingError::OutcomeAlreadySatisfied)
    } else {
        Ok(())
    }
}

impl fmt::Debug for SemanticActionBatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticActionBatch")
            .field("id", &self.id)
            .field("context", &self.context)
            .field("observation", &self.observation)
            .field("observation_generation", &self.observation_generation)
            .field("effect", &self.effect)
            .field("action_count", &self.actions.len())
            .field("settle_millis", &self.settle_millis)
            .field("text_bytes", &self.text_bytes)
            .finish()
    }
}

fn validate_current_frames(
    observation: &SemanticObservation,
    current_frames: &[SemanticFrameJoin],
) -> Result<(), SemanticActionBindingError> {
    if current_frames.is_empty()
        || current_frames.len() > MAX_SEMANTIC_FRAMES
        || current_frames.len() != observation.frames().len()
    {
        return Err(SemanticActionBindingError::CurrentFrameCohort);
    }
    let context = observation.request().context();
    let mut ids = Vec::<FrameId>::with_capacity(current_frames.len());
    for frame in current_frames {
        if frame.context() != context || ids.contains(&frame.frame()) {
            return Err(SemanticActionBindingError::CurrentFrameCohort);
        }
        ids.push(frame.frame());
    }
    if !ids.contains(&FrameId::MAIN) {
        return Err(SemanticActionBindingError::CurrentFrameCohort);
    }
    for snapshot in observation.frames() {
        current_frame(current_frames, snapshot.frame())?;
    }
    Ok(())
}

fn bind_intent(
    observation: &SemanticObservation,
    current_frames: &[SemanticFrameJoin],
    intent: SemanticActionIntent,
    text_bytes: &mut usize,
) -> Result<BoundActionIntent, SemanticActionBindingError> {
    match intent {
        SemanticActionIntent::Click { target } => Ok(BoundActionIntent::Click {
            target: bind_target(
                observation,
                current_frames,
                target,
                SemanticOperationClass::Click,
            )?,
        }),
        SemanticActionIntent::Fill { target, value } => {
            let target = bind_target(
                observation,
                current_frames,
                target,
                SemanticOperationClass::Fill,
            )?;
            if target.role == SemanticRole::Password
                || target.sensitivity == SemanticSensitivity::Secret
            {
                return Err(SemanticActionBindingError::CredentialBoundary);
            }
            *text_bytes = text_bytes
                .checked_add(value.len())
                .ok_or(SemanticActionBindingError::TextLimit)?;
            if *text_bytes > MAX_SEMANTIC_ACTION_BATCH_TEXT_BYTES {
                return Err(SemanticActionBindingError::TextLimit);
            }
            Ok(BoundActionIntent::Fill { target, value })
        }
        SemanticActionIntent::Select { target, option } => {
            let target_node = bind_target(
                observation,
                current_frames,
                target,
                SemanticOperationClass::Select,
            )?;
            if !matches!(
                target_node.role,
                SemanticRole::Combobox | SemanticRole::Listbox
            ) {
                return Err(SemanticActionBindingError::SelectionTarget);
            }
            let (option_frame, option_index, option_node) = locate_node(observation, option)?;
            let current_frame = current_frame(current_frames, option_frame.frame())?;
            observation
                .resolve_node(option, current_frame)
                .map_err(SemanticActionBindingError::Reference)?;
            let (target_frame, target_index, _) = locate_node(observation, target)?;
            if option_frame.frame() != target_frame.frame()
                || option_node.role() != SemanticRole::Option
                || !is_descendant(option_frame, option_index, target_index)
            {
                return Err(SemanticActionBindingError::SelectionTarget);
            }
            let option_node = bind_node(option_frame, option_node);
            Ok(BoundActionIntent::Select {
                target: target_node,
                option: Box::new(option_node),
            })
        }
        SemanticActionIntent::Press { target, key } => Ok(BoundActionIntent::Press {
            target: bind_target(
                observation,
                current_frames,
                target,
                SemanticOperationClass::Press,
            )?,
            key,
        }),
        SemanticActionIntent::Scroll {
            target,
            direction,
            amount,
        } => Ok(BoundActionIntent::Scroll {
            target: bind_target(
                observation,
                current_frames,
                target,
                SemanticOperationClass::Scroll,
            )?,
            direction,
            amount,
        }),
    }
}

fn bind_target(
    observation: &SemanticObservation,
    current_frames: &[SemanticFrameJoin],
    reference: SemanticReferenceId,
    operation: SemanticOperationClass,
) -> Result<BoundNode, SemanticActionBindingError> {
    let (snapshot, _, node) = locate_node(observation, reference)?;
    let current = current_frame(current_frames, snapshot.frame())?;
    observation
        .resolve(reference, current, operation)
        .map_err(SemanticActionBindingError::Reference)?;
    Ok(bind_node(snapshot, node))
}

fn current_frame<'a>(
    current_frames: &'a [SemanticFrameJoin],
    expected: &SemanticFrameJoin,
) -> Result<&'a SemanticFrameJoin, SemanticActionBindingError> {
    let Some(current) = current_frames
        .iter()
        .find(|frame| frame.frame() == expected.frame())
    else {
        return Err(SemanticActionBindingError::CurrentFrameMissing);
    };
    if current != expected {
        return Err(SemanticActionBindingError::Reference(
            SemanticReferenceError::Stale,
        ));
    }
    Ok(current)
}

fn locate_node(
    observation: &SemanticObservation,
    reference: SemanticReferenceId,
) -> Result<(&SemanticSnapshot, usize, &crate::SemanticNode), SemanticActionBindingError> {
    for snapshot in observation.frames() {
        if let Some((index, node)) = snapshot
            .nodes()
            .iter()
            .enumerate()
            .find(|(_, node)| node.reference() == reference)
        {
            return Ok((snapshot, index, node));
        }
    }
    Err(SemanticActionBindingError::Reference(
        SemanticReferenceError::Unknown,
    ))
}

fn is_descendant(snapshot: &SemanticSnapshot, mut child: usize, ancestor: usize) -> bool {
    while let Some(parent) = snapshot.nodes()[child].parent().map(usize::from) {
        if parent == ancestor {
            return true;
        }
        child = parent;
    }
    false
}

fn bind_node(snapshot: &SemanticSnapshot, node: &crate::SemanticNode) -> BoundNode {
    let parent_key = node
        .parent()
        .and_then(|parent| snapshot.nodes().get(usize::from(parent)))
        .map(crate::SemanticNode::key);
    BoundNode {
        reference: node.reference(),
        node_key: node.key(),
        frame: snapshot.frame().clone(),
        snapshot: snapshot.generation(),
        role: node.role(),
        value: node.value().cloned(),
        states: node.states(),
        operations: node.operations(),
        sensitivity: node.sensitivity(),
        trust: node.trust(),
        geometry: node.geometry(),
        structural_digest: structural_digest(node, parent_key.map(|key| key.get())),
    }
}

fn verification_node<'a>(
    bound: &BoundNode,
    current: &'a SemanticSnapshot,
) -> Result<(usize, &'a crate::SemanticNode), SemanticActionRevalidationError> {
    if current.frame() != &bound.frame || bound.snapshot.next() != Some(current.generation()) {
        return Err(SemanticActionRevalidationError::StaleAuthority);
    }
    let Some((index, node)) = current
        .nodes()
        .iter()
        .enumerate()
        .find(|(_, node)| node.key() == bound.node_key)
    else {
        return Err(SemanticActionRevalidationError::TargetMissing);
    };
    if node.role() != bound.role || node.trust() != bound.trust {
        return Err(SemanticActionRevalidationError::TargetChanged);
    }
    if node.role() == SemanticRole::Password
        || node.sensitivity() == SemanticSensitivity::Secret
        || node.sensitivity() != bound.sensitivity
    {
        return Err(SemanticActionRevalidationError::CredentialBoundary);
    }
    Ok((index, node))
}

fn structural_digest(node: &crate::SemanticNode, parent_key: Option<u64>) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"ZEPHIUM-SEMANTIC-ACTION-GUARD-1\0");
    hasher.update([role_code(node.role())]);
    hasher.update([node.heading_level().map_or(0, |level| level.get())]);
    hasher.update([node.operations().bits()]);
    hasher.update([sensitivity_code(node.sensitivity())]);
    hasher.update([trust_code(node.trust())]);
    hasher.update(parent_key.unwrap_or(0).to_be_bytes());
    match node.name() {
        Some(name) => {
            hasher.update([1]);
            hasher.update((name.len() as u64).to_be_bytes());
            hasher.update(name.as_str().as_bytes());
        }
        None => hasher.update([0]),
    }
    hasher.finalize().into()
}

fn hash_frame(hasher: &mut Sha256, frame: &SemanticFrameJoin) {
    let context = frame.context();
    let identity = context.identity();
    hasher.update(identity.id().bytes());
    hasher.update(identity.owner().bytes());
    hasher.update(identity.profile().bytes());
    hasher.update([match identity.kind() {
        crate::ContextKind::Owned => 1,
        crate::ContextKind::BorrowedTab => 2,
        crate::ContextKind::HumanSignInHandoff => 3,
    }]);
    hasher.update(context.context_generation().get().to_be_bytes());
    hasher.update(context.navigation_epoch().get().to_be_bytes());
    hasher.update(context.frame().get().to_be_bytes());
    hasher.update(context.frame_generation().get().to_be_bytes());
    hasher.update(context.cancellation_generation().get().to_be_bytes());
    hasher.update(frame.frame().get().to_be_bytes());
    hasher.update(frame.frame_generation().get().to_be_bytes());
    hasher.update(frame.origin().as_url().as_str().as_bytes());
    hasher.update([match frame.trust() {
        crate::SemanticFrameTrust::SameOrigin => 1,
        crate::SemanticFrameTrust::CrossOriginIsolated => 2,
        crate::SemanticFrameTrust::Unsupported => 3,
    }]);
}

fn hash_effect(hasher: &mut Sha256, effect: SemanticEffectClass) {
    hasher.update([match effect {
        SemanticEffectClass::Read => 1,
        SemanticEffectClass::LocalWrite => 2,
        SemanticEffectClass::ExternalWrite => 3,
        SemanticEffectClass::Communication => 4,
        SemanticEffectClass::Purchase => 5,
        SemanticEffectClass::Destructive => 6,
        SemanticEffectClass::CapabilityBoundary => 7,
    }]);
}

fn hash_wait(hasher: &mut Sha256, wait: SemanticWaitCondition) {
    match wait {
        SemanticWaitCondition::Immediate => hasher.update([1]),
        SemanticWaitCondition::NavigationCommitted => hasher.update([2]),
        SemanticWaitCondition::DocumentReady => hasher.update([3]),
        SemanticWaitCondition::TargetState { state, present } => {
            hasher.update([4, state_code(state), u8::from(present)]);
        }
        SemanticWaitCondition::UrlChanged => hasher.update([5]),
        SemanticWaitCondition::TitleChanged => hasher.update([6]),
        SemanticWaitCondition::Dialog(state) => {
            hasher.update([7, dialog_state_code(state)]);
        }
        SemanticWaitCondition::SemanticChange => hasher.update([8]),
        SemanticWaitCondition::MutationQuiet(quiet) => {
            hasher.update([9]);
            hasher.update(quiet.millis().to_be_bytes());
        }
        SemanticWaitCondition::ScrollPositionChanged => hasher.update([10]),
    }
}

fn hash_verification(hasher: &mut Sha256, verification: SemanticVerification) {
    match verification {
        SemanticVerification::TargetState { state, present } => {
            hasher.update([1, state_code(state), u8::from(present)]);
        }
        SemanticVerification::TargetValueMatchesInput => hasher.update([2]),
        SemanticVerification::TargetValueChanged => hasher.update([3]),
        SemanticVerification::TargetSelectionMatchesOption => hasher.update([4]),
        SemanticVerification::TargetSelectionChanged => hasher.update([5]),
        SemanticVerification::NavigationCommitted => hasher.update([6]),
        SemanticVerification::Dialog(state) => {
            hasher.update([7, dialog_state_code(state)]);
        }
        SemanticVerification::ScrollPositionChanged => hasher.update([8]),
    }
}

const fn state_code(state: SemanticState) -> u8 {
    match state {
        SemanticState::Checked => 1,
        SemanticState::Selected => 2,
        SemanticState::Expanded => 3,
        SemanticState::Disabled => 4,
        SemanticState::Required => 5,
        SemanticState::Invalid => 6,
        SemanticState::Focused => 7,
    }
}

const fn dialog_state_code(state: SemanticDialogState) -> u8 {
    match state {
        SemanticDialogState::Present => 1,
        SemanticDialogState::Absent => 2,
    }
}

const fn press_key_code(key: SemanticPressKey) -> u8 {
    match key {
        SemanticPressKey::Enter => 1,
        SemanticPressKey::Escape => 2,
        SemanticPressKey::Space => 3,
        SemanticPressKey::Tab => 4,
        SemanticPressKey::ArrowUp => 5,
        SemanticPressKey::ArrowDown => 6,
        SemanticPressKey::ArrowLeft => 7,
        SemanticPressKey::ArrowRight => 8,
        SemanticPressKey::Home => 9,
        SemanticPressKey::End => 10,
        SemanticPressKey::PageUp => 11,
        SemanticPressKey::PageDown => 12,
        SemanticPressKey::Backspace => 13,
        SemanticPressKey::Delete => 14,
    }
}

const fn scroll_direction_code(direction: SemanticScrollDirection) -> u8 {
    match direction {
        SemanticScrollDirection::Up => 1,
        SemanticScrollDirection::Down => 2,
        SemanticScrollDirection::Left => 3,
        SemanticScrollDirection::Right => 4,
    }
}

const fn scroll_amount_code(amount: SemanticScrollAmount) -> u8 {
    match amount {
        SemanticScrollAmount::Line => 1,
        SemanticScrollAmount::HalfPage => 2,
        SemanticScrollAmount::Page => 3,
        SemanticScrollAmount::IntoView => 4,
    }
}

fn revalidate_node<'a>(
    bound: &BoundNode,
    current: &'a SemanticSnapshot,
    operation: Option<SemanticOperationClass>,
    deny_credential: bool,
) -> Result<(usize, &'a crate::SemanticNode), SemanticActionRevalidationError> {
    if current.frame() != &bound.frame || current.generation() < bound.snapshot {
        return Err(SemanticActionRevalidationError::StaleAuthority);
    }
    let Some((index, node)) = current
        .nodes()
        .iter()
        .enumerate()
        .find(|(_, node)| node.key() == bound.node_key)
    else {
        return Err(SemanticActionRevalidationError::TargetMissing);
    };
    if node.states().contains(SemanticState::Disabled) {
        return Err(SemanticActionRevalidationError::TargetDisabled);
    }
    if !stable_action_states_match(bound.states, node.states()) {
        return Err(SemanticActionRevalidationError::TargetChanged);
    }
    if operation.is_some_and(|operation| !node.operations().contains(operation)) {
        return Err(SemanticActionRevalidationError::OperationDenied);
    }
    if deny_credential
        && (node.role() == SemanticRole::Password
            || node.sensitivity() == SemanticSensitivity::Secret)
    {
        return Err(SemanticActionRevalidationError::CredentialBoundary);
    }
    let parent_key = node
        .parent()
        .and_then(|parent| current.nodes().get(usize::from(parent)))
        .map(crate::SemanticNode::key)
        .map(SemanticNodeKey::get);
    if structural_digest(node, parent_key) != bound.structural_digest {
        return Err(SemanticActionRevalidationError::TargetChanged);
    }
    Ok((index, node))
}

fn stable_action_states_match(previous: SemanticStates, current: SemanticStates) -> bool {
    [
        SemanticState::Checked,
        SemanticState::Selected,
        SemanticState::Expanded,
        SemanticState::Required,
        SemanticState::Invalid,
    ]
    .into_iter()
    .all(|state| previous.contains(state) == current.contains(state))
}

const fn sensitivity_code(value: SemanticSensitivity) -> u8 {
    match value {
        SemanticSensitivity::Public => 1,
        SemanticSensitivity::Sensitive => 2,
        SemanticSensitivity::Secret => 3,
    }
}

const fn trust_code(value: SemanticTrust) -> u8 {
    match value {
        SemanticTrust::UntrustedPage => 1,
        SemanticTrust::BrowserDerived => 2,
    }
}

const fn role_code(role: SemanticRole) -> u8 {
    match role {
        SemanticRole::Group => 1,
        SemanticRole::Document => 2,
        SemanticRole::Landmark => 3,
        SemanticRole::Heading => 4,
        SemanticRole::Paragraph => 5,
        SemanticRole::Link => 6,
        SemanticRole::Button => 7,
        SemanticRole::Textbox => 8,
        SemanticRole::Password => 9,
        SemanticRole::Searchbox => 10,
        SemanticRole::Checkbox => 11,
        SemanticRole::Radio => 12,
        SemanticRole::Combobox => 13,
        SemanticRole::Listbox => 14,
        SemanticRole::Option => 15,
        SemanticRole::Spinbutton => 16,
        SemanticRole::Slider => 17,
        SemanticRole::Tab => 18,
        SemanticRole::MenuItem => 19,
        SemanticRole::Dialog => 20,
        SemanticRole::List => 21,
        SemanticRole::ListItem => 22,
        SemanticRole::Table => 23,
        SemanticRole::Row => 24,
        SemanticRole::CellHeader => 25,
        SemanticRole::Cell => 26,
        SemanticRole::Image => 27,
        SemanticRole::Progress => 28,
        SemanticRole::Status => 29,
        SemanticRole::FrameBoundary => 30,
    }
}

/// Refusal while binding pre-policy proposals to exact observation authority.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticActionBindingError {
    /// A batch must contain at least one action.
    #[error("semantic action batch is empty")]
    Empty,
    /// Action count exceeded the fixed batch ceiling.
    #[error("semantic action batch exceeds its action ceiling")]
    ActionLimit,
    /// Trusted current-frame input was empty, duplicate, oversized, or cross-context.
    #[error("semantic action current-frame cohort is invalid")]
    CurrentFrameCohort,
    /// Exact current authority for an observed action frame was absent.
    #[error("semantic action current frame is missing")]
    CurrentFrameMissing,
    /// An opaque reference was unknown, stale, or not operation-authorized.
    #[error("semantic action reference binding failed")]
    Reference(SemanticReferenceError),
    /// Password/secret filling requires a separate credential capability.
    #[error("semantic action crosses a credential capability boundary")]
    CredentialBoundary,
    /// Select target/option role, frame, snapshot, or ancestry was incompatible.
    #[error("semantic selection target is invalid")]
    SelectionTarget,
    /// The exact declared postcondition already held before any effect.
    #[error("semantic action outcome is already satisfied")]
    OutcomeAlreadySatisfied,
    /// Bound target role or fixed key cannot establish the declared postcondition.
    #[error("semantic action bound outcome contract is incompatible")]
    OutcomeContract,
    /// Aggregate replacement text exceeded the fixed batch ceiling.
    #[error("semantic action batch exceeds its text ceiling")]
    TextLimit,
    /// Aggregate settle budgets exceeded the fixed batch ceiling.
    #[error("semantic action batch exceeds its settle ceiling")]
    SettleLimit,
    /// One batch attempted to cross between declared effect classes.
    #[error("semantic action batch crosses an effect boundary")]
    MixedEffectBoundary,
    /// Externally consequential effect classes require one verified action at a time.
    #[error("semantic effect class permits only one action per batch")]
    EffectBatchLimit,
}

/// Typed structural refusal before backend visibility/occlusion revalidation.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticActionRevalidationError {
    /// Context, frame document, or snapshot generation moved backward/replaced.
    #[error("semantic action authority is stale")]
    StaleAuthority,
    /// Stable target identity no longer exists in the current bounded snapshot.
    #[error("semantic action target is missing")]
    TargetMissing,
    /// Stable target role, state, parent, name, trust, sensitivity, or operations changed.
    #[error("semantic action target changed materially")]
    TargetChanged,
    /// Current semantics no longer allow the proposed operation class.
    #[error("semantic action operation is no longer allowed")]
    OperationDenied,
    /// Current target is disabled.
    #[error("semantic action target is disabled")]
    TargetDisabled,
    /// Fill target became password/secret and requires a credential capability.
    #[error("semantic action target crossed a credential boundary")]
    CredentialBoundary,
    /// Bound option disappeared, changed, or no longer descends from the target.
    #[error("semantic action selection target changed")]
    SelectionTarget,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        decode_semantic_snapshot, ContextCapabilities, ContextCapability, ContextId,
        ContextIdentity, ContextKind, ContextOperationId, ContextRegistry, ContextRunId,
        ContextSettlement, FrameGeneration, SemanticDecodeContext, SemanticFrameTrust,
        SemanticInvocationId, SemanticObservationAssembler, SemanticObservationBudget,
        SemanticOrigin, SEMANTIC_WIRE_VERSION,
    };
    use serde_json::json;
    use zephium_core::ids::ProfileId;

    fn observation() -> SemanticObservation {
        let identity = ContextIdentity::new(
            ContextId::from_raw(1),
            ContextRunId::from_raw(2),
            ProfileId::from(3),
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
            SemanticOrigin::parse("https://action-private.example.test/path").expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        let bytes = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": 7,
            "g": 9,
            "c": "complete",
            "n": [
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "button", "n": "Save private draft", "o": 9,
                 "b": {"x": 10, "y": 10, "w": 100, "h": 30}},
                {"k": 3, "p": 0, "r": "textbox", "n": "Title", "v": {"k": "text", "value": "old"}, "o": 11,
                 "b": {"x": 10, "y": 50, "w": 200, "h": 30}},
                {"k": 4, "p": 0, "r": "password", "n": "Password", "v": {"k": "redacted"}, "o": 11},
                {"k": 5, "p": 0, "r": "combobox", "n": "Priority", "v": {"k": "ordinal", "value": 0}, "o": 13},
                {"k": 6, "p": 4, "r": "option", "n": "High", "v": {"k": "ordinal", "value": 1}, "o": 9},
                {"k": 7, "p": 0, "r": "option", "n": "Detached option", "v": {"k": "ordinal", "value": 2}, "o": 9}
            ]
        }))
        .expect("wire");
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
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

    fn frames(observation: &SemanticObservation) -> Vec<SemanticFrameJoin> {
        observation
            .frames()
            .iter()
            .map(|snapshot| snapshot.frame().clone())
            .collect()
    }

    fn current_snapshot(
        observation: &SemanticObservation,
        invocation: u64,
        generation: u64,
        nodes: serde_json::Value,
    ) -> SemanticSnapshot {
        let bytes = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": invocation,
            "g": generation,
            "c": "complete",
            "n": nodes,
        }))
        .expect("wire");
        decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(invocation).expect("invocation"),
                observation.frames()[0].frame().clone(),
                SemanticSnapshotGeneration::new(generation).expect("generation"),
            ),
            &bytes,
        )
        .expect("current snapshot")
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
            SemanticSettleBudget::try_new(250).expect("budget"),
        )
        .expect("proposal")
    }

    #[test]
    fn binds_all_fixed_action_shapes_to_exact_observation_authority() {
        let observation = observation();
        let current = frames(&observation);

        let click = SemanticActionBatch::bind(
            SemanticActionBatchId::new(1).expect("batch"),
            &observation,
            &current,
            vec![proposal(
                SemanticActionIntent::Click {
                    target: SemanticReferenceId::new(2).expect("reference"),
                },
                SemanticEffectClass::Read,
                SemanticVerification::TargetState {
                    state: SemanticState::Focused,
                    present: true,
                },
            )],
        )
        .expect("click");
        assert_eq!(click.actions()[0].kind(), SemanticActionKind::Click);
        assert_eq!(click.actions()[0].target_role(), SemanticRole::Button);
        assert!(click.actions()[0].target_geometry().is_some());

        let fill_and_press = SemanticActionBatch::bind(
            SemanticActionBatchId::new(2).expect("batch"),
            &observation,
            &current,
            vec![
                proposal(
                    SemanticActionIntent::Fill {
                        target: SemanticReferenceId::new(3).expect("reference"),
                        value: SemanticActionText::try_new("new private title".to_owned())
                            .expect("text"),
                    },
                    SemanticEffectClass::LocalWrite,
                    SemanticVerification::TargetValueMatchesInput,
                ),
                proposal(
                    SemanticActionIntent::Press {
                        target: SemanticReferenceId::new(3).expect("reference"),
                        key: SemanticPressKey::Enter,
                    },
                    SemanticEffectClass::LocalWrite,
                    SemanticVerification::TargetState {
                        state: SemanticState::Focused,
                        present: true,
                    },
                ),
            ],
        )
        .expect("fill and press");
        assert_eq!(fill_and_press.actions().len(), 2);
        assert_eq!(fill_and_press.actions()[1].ordinal(), 2);
        assert_eq!(fill_and_press.text_bytes(), "new private title".len());
        assert_eq!(fill_and_press.settle_millis(), 500);

        let select = SemanticActionBatch::bind(
            SemanticActionBatchId::new(3).expect("batch"),
            &observation,
            &current,
            vec![proposal(
                SemanticActionIntent::Select {
                    target: SemanticReferenceId::new(5).expect("target"),
                    option: SemanticReferenceId::new(6).expect("option"),
                },
                SemanticEffectClass::LocalWrite,
                SemanticVerification::TargetSelectionMatchesOption,
            )],
        )
        .expect("select");
        assert_eq!(
            select.actions()[0].option_reference(),
            SemanticReferenceId::new(6)
        );

        let scroll = SemanticActionBatch::bind(
            SemanticActionBatchId::new(4).expect("batch"),
            &observation,
            &current,
            vec![proposal(
                SemanticActionIntent::Scroll {
                    target: SemanticReferenceId::new(1).expect("document"),
                    direction: SemanticScrollDirection::Down,
                    amount: SemanticScrollAmount::Page,
                },
                SemanticEffectClass::Read,
                SemanticVerification::ScrollPositionChanged,
            )],
        )
        .expect("scroll");
        assert_eq!(scroll.actions()[0].kind(), SemanticActionKind::Scroll);
    }

    #[test]
    fn text_and_debug_are_bounded_secret_safe_and_content_redacted() {
        assert_eq!(
            SemanticActionText::try_new("sk-super-secret-value".to_owned()),
            Err(SemanticActionTextError::Secret)
        );
        assert_eq!(
            SemanticActionText::try_new("unsafe\u{202e}text".to_owned()),
            Err(SemanticActionTextError::InvalidCharacter)
        );
        assert_eq!(
            SemanticActionText::try_new("x".repeat(MAX_SEMANTIC_ACTION_TEXT_BYTES + 1)),
            Err(SemanticActionTextError::Limit)
        );
        assert!(SemanticActionText::try_new(String::new())
            .expect("clear")
            .is_empty());

        let observation = observation();
        let batch = SemanticActionBatch::bind(
            SemanticActionBatchId::new(9).expect("batch"),
            &observation,
            &frames(&observation),
            vec![proposal(
                SemanticActionIntent::Fill {
                    target: SemanticReferenceId::new(3).expect("reference"),
                    value: SemanticActionText::try_new("private replacement".to_owned())
                        .expect("text"),
                },
                SemanticEffectClass::LocalWrite,
                SemanticVerification::TargetValueMatchesInput,
            )],
        )
        .expect("batch");
        let debug = format!("{batch:?} {:?}", batch.actions());
        assert!(!debug.contains("private replacement"));
        assert!(!debug.contains("Save private draft"));
        assert!(!debug.contains("action-private"));
        assert!(!debug.contains("5da29"));
        assert!(debug.contains("text_bytes"));
    }

    #[test]
    fn credentials_wrong_operations_and_invalid_selection_fail_closed() {
        let observation = observation();
        let current = frames(&observation);
        assert_eq!(
            SemanticActionBatch::bind(
                SemanticActionBatchId::new(10).expect("batch"),
                &observation,
                &current,
                vec![proposal(
                    SemanticActionIntent::Fill {
                        target: SemanticReferenceId::new(4).expect("password"),
                        value: SemanticActionText::try_new("ordinary-value".to_owned())
                            .expect("text"),
                    },
                    SemanticEffectClass::LocalWrite,
                    SemanticVerification::TargetValueMatchesInput,
                )],
            ),
            Err(SemanticActionBindingError::CredentialBoundary)
        );
        assert!(matches!(
            SemanticActionBatch::bind(
                SemanticActionBatchId::new(11).expect("batch"),
                &observation,
                &current,
                vec![proposal(
                    SemanticActionIntent::Fill {
                        target: SemanticReferenceId::new(2).expect("button"),
                        value: SemanticActionText::try_new("x".to_owned()).expect("text"),
                    },
                    SemanticEffectClass::LocalWrite,
                    SemanticVerification::TargetValueMatchesInput,
                )],
            ),
            Err(SemanticActionBindingError::Reference(
                SemanticReferenceError::OperationDenied
            ))
        ));
        assert_eq!(
            SemanticActionBatch::bind(
                SemanticActionBatchId::new(12).expect("batch"),
                &observation,
                &current,
                vec![proposal(
                    SemanticActionIntent::Select {
                        target: SemanticReferenceId::new(5).expect("target"),
                        option: SemanticReferenceId::new(7).expect("option"),
                    },
                    SemanticEffectClass::LocalWrite,
                    SemanticVerification::TargetSelectionMatchesOption,
                )],
            ),
            Err(SemanticActionBindingError::SelectionTarget)
        );
    }

    #[test]
    fn batch_effect_time_count_and_current_frame_bounds_are_enforced() {
        let observation = observation();
        let current = frames(&observation);
        let click = || {
            proposal(
                SemanticActionIntent::Click {
                    target: SemanticReferenceId::new(2).expect("button"),
                },
                SemanticEffectClass::Read,
                SemanticVerification::TargetState {
                    state: SemanticState::Focused,
                    present: true,
                },
            )
        };
        assert_eq!(
            SemanticActionBatch::bind(
                SemanticActionBatchId::new(20).expect("batch"),
                &observation,
                &current,
                Vec::new(),
            ),
            Err(SemanticActionBindingError::Empty)
        );
        assert_eq!(
            SemanticActionBatch::bind(
                SemanticActionBatchId::new(21).expect("batch"),
                &observation,
                &current,
                (0..=MAX_SEMANTIC_ACTIONS_PER_BATCH)
                    .map(|_| click())
                    .collect(),
            ),
            Err(SemanticActionBindingError::ActionLimit)
        );
        assert_eq!(
            SemanticActionBatch::bind(
                SemanticActionBatchId::new(22).expect("batch"),
                &observation,
                &[],
                vec![click()],
            ),
            Err(SemanticActionBindingError::CurrentFrameCohort)
        );
        assert_eq!(
            SemanticActionBatch::bind(
                SemanticActionBatchId::new(25).expect("batch"),
                &observation,
                &current,
                vec![
                    click(),
                    proposal(
                        SemanticActionIntent::Click {
                            target: SemanticReferenceId::new(2).expect("button"),
                        },
                        SemanticEffectClass::LocalWrite,
                        SemanticVerification::TargetState {
                            state: SemanticState::Focused,
                            present: true
                        },
                    ),
                ],
            ),
            Err(SemanticActionBindingError::MixedEffectBoundary)
        );

        let full_text = || {
            proposal(
                SemanticActionIntent::Fill {
                    target: SemanticReferenceId::new(3).expect("textbox"),
                    value: SemanticActionText::try_new("x".repeat(MAX_SEMANTIC_ACTION_TEXT_BYTES))
                        .expect("text"),
                },
                SemanticEffectClass::LocalWrite,
                SemanticVerification::TargetValueMatchesInput,
            )
        };
        assert_eq!(
            SemanticActionBatch::bind(
                SemanticActionBatchId::new(26).expect("batch"),
                &observation,
                &current,
                (0..5).map(|_| full_text()).collect(),
            ),
            Err(SemanticActionBindingError::TextLimit)
        );

        let external = |id| {
            proposal(
                SemanticActionIntent::Click { target: id },
                SemanticEffectClass::ExternalWrite,
                SemanticVerification::TargetState {
                    state: SemanticState::Focused,
                    present: true,
                },
            )
        };
        assert_eq!(
            SemanticActionBatch::bind(
                SemanticActionBatchId::new(23).expect("batch"),
                &observation,
                &current,
                vec![
                    external(SemanticReferenceId::new(2).expect("button")),
                    external(SemanticReferenceId::new(2).expect("button")),
                ],
            ),
            Err(SemanticActionBindingError::EffectBatchLimit)
        );

        let slow = || {
            SemanticActionProposal::try_new(
                SemanticActionIntent::Click {
                    target: SemanticReferenceId::new(2).expect("button"),
                },
                SemanticEffectClass::Read,
                SemanticWaitCondition::Immediate,
                SemanticVerification::TargetState {
                    state: SemanticState::Focused,
                    present: true,
                },
                SemanticSettleBudget::try_new(10_000).expect("budget"),
            )
            .expect("proposal")
        };
        assert_eq!(
            SemanticActionBatch::bind(
                SemanticActionBatchId::new(24).expect("batch"),
                &observation,
                &current,
                (0..7).map(|_| slow()).collect(),
            ),
            Err(SemanticActionBindingError::SettleLimit)
        );
    }

    #[test]
    fn outcome_contracts_reject_blind_or_incompatible_settlement() {
        assert_eq!(
            SemanticMutationQuietPeriod::try_new(0),
            Err(SemanticActionContractError::QuietPeriod)
        );
        assert_eq!(
            SemanticSettleBudget::try_new(MAX_SEMANTIC_ACTION_SETTLE_MILLIS + 1),
            Err(SemanticActionContractError::SettleBudget)
        );
        assert_eq!(
            SemanticActionProposal::try_new(
                SemanticActionIntent::Scroll {
                    target: SemanticReferenceId::new(1).expect("document"),
                    direction: SemanticScrollDirection::Down,
                    amount: SemanticScrollAmount::Page,
                },
                SemanticEffectClass::Read,
                SemanticWaitCondition::SemanticChange,
                SemanticVerification::TargetState {
                    state: SemanticState::Focused,
                    present: true
                },
                SemanticSettleBudget::try_new(100).expect("budget"),
            ),
            Err(SemanticActionContractError::OutcomeContract)
        );
        let observation = observation();
        assert_eq!(
            SemanticActionBatch::bind(
                SemanticActionBatchId::new(29).expect("batch"),
                &observation,
                &frames(&observation),
                vec![proposal(
                    SemanticActionIntent::Press {
                        target: SemanticReferenceId::new(3).expect("textbox"),
                        key: SemanticPressKey::ArrowDown,
                    },
                    SemanticEffectClass::LocalWrite,
                    SemanticVerification::TargetSelectionChanged,
                )],
            ),
            Err(SemanticActionBindingError::OutcomeContract)
        );
        assert_eq!(
            SemanticActionProposal::try_new(
                SemanticActionIntent::Click {
                    target: SemanticReferenceId::new(2).expect("button"),
                },
                SemanticEffectClass::Read,
                SemanticWaitCondition::Immediate,
                SemanticVerification::NavigationCommitted,
                SemanticSettleBudget::try_new(100).expect("budget"),
            ),
            Err(SemanticActionContractError::OutcomeContract)
        );
    }

    #[test]
    fn revalidation_follows_stable_identity_and_ignores_safe_mutable_fields() {
        let observation = observation();
        let batch = SemanticActionBatch::bind(
            SemanticActionBatchId::new(30).expect("batch"),
            &observation,
            &frames(&observation),
            vec![proposal(
                SemanticActionIntent::Click {
                    target: SemanticReferenceId::new(2).expect("button"),
                },
                SemanticEffectClass::Read,
                SemanticVerification::TargetState {
                    state: SemanticState::Focused,
                    present: true,
                },
            )],
        )
        .expect("batch");
        let action = &batch.actions()[0];

        let reordered = current_snapshot(
            &observation,
            8,
            10,
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 99, "p": 0, "r": "paragraph", "n": "Inserted sibling"},
                {"k": 2, "p": 0, "r": "button", "n": "Save private draft", "s": 64, "o": 9,
                 "b": {"x": 40, "y": 30, "w": 120, "h": 40}}
            ]),
        );
        assert_eq!(action.revalidate(&reordered), Ok(()));

        let renamed = current_snapshot(
            &observation,
            9,
            10,
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "button", "n": "Publish private draft", "o": 9}
            ]),
        );
        assert_eq!(
            action.revalidate(&renamed),
            Err(SemanticActionRevalidationError::TargetChanged)
        );

        let state_changed = current_snapshot(
            &observation,
            16,
            10,
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "button", "n": "Save private draft", "s": 4, "o": 9}
            ]),
        );
        assert_eq!(
            action.revalidate(&state_changed),
            Err(SemanticActionRevalidationError::TargetChanged)
        );

        let missing = current_snapshot(
            &observation,
            10,
            10,
            json!([{"k": 1, "r": "document", "o": 16}]),
        );
        assert_eq!(
            action.revalidate(&missing),
            Err(SemanticActionRevalidationError::TargetMissing)
        );

        let operation_removed = current_snapshot(
            &observation,
            11,
            10,
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "button", "n": "Save private draft"}
            ]),
        );
        assert_eq!(
            action.revalidate(&operation_removed),
            Err(SemanticActionRevalidationError::OperationDenied)
        );

        let disabled = current_snapshot(
            &observation,
            12,
            10,
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "button", "n": "Save private draft", "s": 8}
            ]),
        );
        assert_eq!(
            action.revalidate(&disabled),
            Err(SemanticActionRevalidationError::TargetDisabled)
        );

        let older = current_snapshot(
            &observation,
            13,
            8,
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "button", "n": "Save private draft", "o": 9}
            ]),
        );
        assert_eq!(
            action.revalidate(&older),
            Err(SemanticActionRevalidationError::StaleAuthority)
        );
    }

    #[test]
    fn revalidation_refuses_credential_escalation_and_option_drift() {
        let observation = observation();
        let fill = SemanticActionBatch::bind(
            SemanticActionBatchId::new(31).expect("batch"),
            &observation,
            &frames(&observation),
            vec![proposal(
                SemanticActionIntent::Fill {
                    target: SemanticReferenceId::new(3).expect("textbox"),
                    value: SemanticActionText::try_new("ordinary-value".to_owned()).expect("text"),
                },
                SemanticEffectClass::LocalWrite,
                SemanticVerification::TargetValueMatchesInput,
            )],
        )
        .expect("fill");
        let credential = current_snapshot(
            &observation,
            14,
            10,
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 3, "p": 0, "r": "textbox", "n": "Title",
                 "v": {"k": "redacted"}, "o": 11, "q": "secret"}
            ]),
        );
        assert_eq!(
            fill.actions()[0].revalidate(&credential),
            Err(SemanticActionRevalidationError::CredentialBoundary)
        );

        let select = SemanticActionBatch::bind(
            SemanticActionBatchId::new(32).expect("batch"),
            &observation,
            &frames(&observation),
            vec![proposal(
                SemanticActionIntent::Select {
                    target: SemanticReferenceId::new(5).expect("combobox"),
                    option: SemanticReferenceId::new(6).expect("option"),
                },
                SemanticEffectClass::LocalWrite,
                SemanticVerification::TargetSelectionMatchesOption,
            )],
        )
        .expect("select");
        let detached = current_snapshot(
            &observation,
            15,
            10,
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 5, "p": 0, "r": "combobox", "n": "Priority",
                 "v": {"k": "ordinal", "value": 0}, "o": 13},
                {"k": 6, "p": 0, "r": "option", "n": "High",
                 "v": {"k": "ordinal", "value": 1}, "o": 9}
            ]),
        );
        assert_eq!(
            select.actions()[0].revalidate(&detached),
            Err(SemanticActionRevalidationError::SelectionTarget)
        );
    }
}
