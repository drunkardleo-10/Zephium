//! Deterministic compact model encoding and token admission for semantic diffs.
//!
//! Diff content remains private until the same explicit tokenizer port used by
//! full observations admits it under a byte/token budget. Committed delivery
//! acknowledges the diff's exact current observation, making it eligible as
//! the next baseline without exposing internal stable keys.

use std::fmt;

use crate::semantic_diff::SemanticObservationFingerprint;
use crate::semantic_model::{
    checked_write, conservative_utf8_measurement, frame_trust_label, role_label, sensitivity_label,
    source_label, validate_semantic_token_measurement, write_operations, write_quoted,
    write_states, write_value, BoundedModelBuffer,
};
use crate::{
    SemanticDiff, SemanticDiffEntry, SemanticDiffEntryKind, SemanticDiffFrame, SemanticFrameJoin,
    SemanticModelDeliveryError, SemanticModelDeliverySettlement, SemanticModelEncodingBudget,
    SemanticModelEncodingError, SemanticNode, SemanticNodeChange, SemanticNodeChanges,
    SemanticObservationAcknowledgement, SemanticOperationClass, SemanticOperations,
    SemanticReferenceId, SemanticSensitivity, SemanticState, SemanticStates, SemanticTokenCounter,
    SemanticTokenMeasurement, SemanticTokenizerRevision,
};

/// Version of the compact semantic-diff model-input grammar.
pub const SEMANTIC_DIFF_MODEL_SCHEMA_VERSION: u16 = 3;

/// Content-free deterministic compact-diff metrics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticDiffEncodingStats {
    bytes: u32,
    lines: u16,
    frames: u8,
    entries: u16,
    reference_rebases: u16,
    secret_nodes: u16,
}

impl SemanticDiffEncodingStats {
    #[cfg(test)]
    pub(crate) const fn for_input_metrics_test(
        bytes: u32,
        lines: u16,
        frames: u8,
        entries: u16,
        reference_rebases: u16,
        secret_nodes: u16,
    ) -> Self {
        Self {
            bytes,
            lines,
            frames,
            entries,
            reference_rebases,
            secret_nodes,
        }
    }

    /// Encoded UTF-8 bytes.
    pub const fn bytes(self) -> u32 {
        self.bytes
    }

    /// Deterministic line count.
    pub const fn lines(self) -> u16 {
        self.lines
    }

    /// Distinct referenced frame count.
    pub const fn frames(self) -> u8 {
        self.frames
    }

    /// Semantic add/remove/change/move entry count.
    pub const fn entries(self) -> u16 {
        self.entries
    }

    /// Otherwise-unchanged reference rebase record count.
    pub const fn reference_rebases(self) -> u16 {
        self.reference_rebases
    }

    /// Current entry nodes classified secret after Rust-side redaction.
    pub const fn secret_nodes(self) -> u16 {
        self.secret_nodes
    }
}

/// Private compact diff bytes awaiting required token measurement.
pub struct SemanticEncodedDiff {
    content: String,
    budget: SemanticModelEncodingBudget,
    stats: SemanticDiffEncodingStats,
    current_fingerprint: SemanticObservationFingerprint,
    diff_guard: [u8; 32],
}

impl SemanticEncodedDiff {
    /// Content-free encoding statistics.
    pub const fn stats(&self) -> SemanticDiffEncodingStats {
        self.stats
    }

    /// Measures without exposing content outside the explicit tokenizer port.
    pub fn measure(
        &self,
        counter: &dyn SemanticTokenCounter,
        expected_revision: &SemanticTokenizerRevision,
    ) -> Result<SemanticTokenMeasurement, SemanticModelEncodingError> {
        let measurement = counter
            .count_tokens(&self.content)
            .map_err(SemanticModelEncodingError::TokenCounter)?;
        validate_semantic_token_measurement(&self.budget, &measurement, expected_revision)?;
        Ok(measurement)
    }

    /// Admits model-facing diff bytes only after the required count fits.
    pub fn admit(
        self,
        counter: &dyn SemanticTokenCounter,
        expected_revision: &SemanticTokenizerRevision,
    ) -> Result<SemanticDiffModelPayload, SemanticModelEncodingError> {
        let measurement = self.measure(counter, expected_revision)?;
        Ok(SemanticDiffModelPayload {
            content: self.content,
            stats: self.stats,
            measurement,
            current_fingerprint: self.current_fingerprint,
            diff_guard: self.diff_guard,
        })
    }

    /// Admits UTF-8 byte length as a conservative bound before provider counting.
    pub fn admit_conservative_utf8(
        self,
        revision: &SemanticTokenizerRevision,
    ) -> Result<SemanticDiffModelPayload, SemanticModelEncodingError> {
        let measurement = conservative_utf8_measurement(&self.content, revision)?;
        validate_semantic_token_measurement(&self.budget, &measurement, revision)?;
        Ok(SemanticDiffModelPayload {
            content: self.content,
            stats: self.stats,
            measurement,
            current_fingerprint: self.current_fingerprint,
            diff_guard: self.diff_guard,
        })
    }
}

impl fmt::Debug for SemanticEncodedDiff {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticEncodedDiff")
            .field("content", &"[redacted]")
            .field("budget", &self.budget)
            .field("stats", &self.stats)
            .field("current_fingerprint", &"[redacted]")
            .finish()
    }
}

/// Token-admitted compact semantic diff for the selected model adapter only.
pub struct SemanticDiffModelPayload {
    content: String,
    stats: SemanticDiffEncodingStats,
    measurement: SemanticTokenMeasurement,
    current_fingerprint: SemanticObservationFingerprint,
    diff_guard: [u8; 32],
}

/// Move-only proof retained beside one provider-bound diff request.
///
/// The provider adapter may retain compact content in its bounded transcript
/// and fixed request body while this content-free authority waits for exact
/// whole-input admission and transport commit. Refusal or cancellation drops
/// it without acknowledging the diff.
pub(crate) struct SemanticDiffDeliveryAuthority {
    measurement: SemanticTokenMeasurement,
    current_fingerprint: SemanticObservationFingerprint,
    diff_guard: [u8; 32],
}

impl SemanticDiffDeliveryAuthority {
    pub(crate) const fn token_measurement(&self) -> &SemanticTokenMeasurement {
        &self.measurement
    }

    pub(crate) fn matches_diff(&self, diff: &SemanticDiff) -> bool {
        self.diff_guard == diff.guard() && self.current_fingerprint == *diff.current_fingerprint()
    }

    pub(crate) fn commit(self) -> SemanticDiffDeliveryReceipt {
        SemanticDiffDeliveryReceipt {
            acknowledgement: SemanticObservationAcknowledgement::from_fingerprint(
                self.current_fingerprint,
            ),
            diff_guard: self.diff_guard,
        }
    }
}

impl fmt::Debug for SemanticDiffDeliveryAuthority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticDiffDeliveryAuthority")
            .field("measurement", &self.measurement)
            .field("current_fingerprint", &"[redacted]")
            .field("diff_guard", &"[redacted]")
            .finish()
    }
}

/// Opaque proof that one exact semantic diff reached committed model delivery.
///
/// This binds both the acknowledged baseline and the exact current observation.
/// It contains no page content and cannot authorize a model call by itself.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticDiffDeliveryReceipt {
    acknowledgement: SemanticObservationAcknowledgement,
    diff_guard: [u8; 32],
}

impl SemanticDiffDeliveryReceipt {
    /// Exact current observation acknowledgement for the next diff baseline.
    pub const fn acknowledgement(&self) -> &SemanticObservationAcknowledgement {
        &self.acknowledgement
    }

    /// Consumes the diff proof and returns its current observation baseline.
    pub fn into_acknowledgement(self) -> SemanticObservationAcknowledgement {
        self.acknowledgement
    }

    pub(crate) const fn guard(&self) -> [u8; 32] {
        self.diff_guard
    }
}

impl fmt::Debug for SemanticDiffDeliveryReceipt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticDiffDeliveryReceipt")
            .field("acknowledgement", &self.acknowledgement)
            .field("diff_guard", &"[redacted]")
            .finish()
    }
}

impl SemanticDiffModelPayload {
    /// Returns compact diff input to the already-selected model transport.
    pub fn as_str(&self) -> &str {
        &self.content
    }

    /// Content-free encoding statistics.
    pub const fn stats(&self) -> SemanticDiffEncodingStats {
        self.stats
    }

    /// Admitted bounded token measurement and tokenizer revision.
    pub const fn token_measurement(&self) -> &SemanticTokenMeasurement {
        &self.measurement
    }

    pub(crate) fn matches_diff(&self, diff: &SemanticDiff) -> bool {
        self.diff_guard == diff.guard() && self.current_fingerprint == *diff.current_fingerprint()
    }

    pub(crate) fn into_provider_parts(
        self,
    ) -> (
        String,
        SemanticDiffEncodingStats,
        SemanticDiffDeliveryAuthority,
    ) {
        (
            self.content,
            self.stats,
            SemanticDiffDeliveryAuthority {
                measurement: self.measurement,
                current_fingerprint: self.current_fingerprint,
                diff_guard: self.diff_guard,
            },
        )
    }

    /// Settles delivery while retaining proof of the exact baseline/current diff.
    pub fn settle_delivery_receipt(
        self,
        settlement: SemanticModelDeliverySettlement,
    ) -> Result<SemanticDiffDeliveryReceipt, SemanticModelDeliveryError> {
        match settlement {
            SemanticModelDeliverySettlement::Committed => {
                let (_, _, delivery) = self.into_provider_parts();
                Ok(delivery.commit())
            }
            SemanticModelDeliverySettlement::Refused => Err(SemanticModelDeliveryError::Refused),
            SemanticModelDeliverySettlement::Cancelled => {
                Err(SemanticModelDeliveryError::Cancelled)
            }
        }
    }

    /// Settles transport of this exact token-admitted diff.
    ///
    /// Only committed delivery acknowledges the exact current observation.
    pub fn settle_delivery(
        self,
        settlement: SemanticModelDeliverySettlement,
    ) -> Result<SemanticObservationAcknowledgement, SemanticModelDeliveryError> {
        match settlement {
            SemanticModelDeliverySettlement::Committed => Ok(
                SemanticObservationAcknowledgement::from_fingerprint(self.current_fingerprint),
            ),
            SemanticModelDeliverySettlement::Refused => Err(SemanticModelDeliveryError::Refused),
            SemanticModelDeliverySettlement::Cancelled => {
                Err(SemanticModelDeliveryError::Cancelled)
            }
        }
    }
}

impl fmt::Debug for SemanticDiffModelPayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticDiffModelPayload")
            .field("content", &"[redacted]")
            .field("stats", &self.stats)
            .field("measurement", &self.measurement)
            .field("current_fingerprint", &"[redacted]")
            .finish()
    }
}

/// Encodes one complete semantic delta into deterministic compact `ZDIFF3` lines.
pub fn encode_semantic_diff(
    diff: &SemanticDiff,
    budget: SemanticModelEncodingBudget,
) -> Result<SemanticEncodedDiff, SemanticModelEncodingError> {
    if diff.stats().entries() as usize != diff.entries().len()
        || diff.stats().reference_rebases() as usize != diff.reference_rebases().len()
    {
        return Err(SemanticModelEncodingError::Invariant);
    }
    let frames = diff.frames();
    let capacity = usize::try_from(budget.max_bytes().min(8 * 1024))
        .map_err(|_| SemanticModelEncodingError::Budget)?;
    let mut output = BoundedModelBuffer::new(capacity, budget.max_bytes());
    checked_write(
        &mut output,
        format_args!(
            "ZDIFF{} content=untrusted from_generation={} to_generation={} entries={} rebases={}\n",
            SEMANTIC_DIFF_MODEL_SCHEMA_VERSION,
            diff.previous_generation().get(),
            diff.current_generation().get(),
            diff.stats().entries(),
            diff.stats().reference_rebases(),
        ),
    )?;

    for (index, frame) in frames.iter().enumerate() {
        checked_write(&mut output, format_args!("F f{} origin=", index + 1))?;
        write_quoted(&mut output, frame.frame().origin().as_url().as_str())?;
        checked_write(
            &mut output,
            format_args!(
                " trust={} from_snapshot={} snapshot={}\n",
                frame_trust_label(frame.frame().trust()),
                frame.previous_snapshot().get(),
                frame.current_snapshot().get(),
            ),
        )?;
    }

    let mut secret_nodes = 0_u16;
    for entry in diff.entries() {
        let frame = frame_alias(frames, entry.frame())?;
        write_entry(&mut output, frame, entry)?;
        if entry
            .current_node()
            .is_some_and(|node| node.sensitivity() == SemanticSensitivity::Secret)
        {
            secret_nodes = secret_nodes
                .checked_add(1)
                .ok_or(SemanticModelEncodingError::Invariant)?;
        }
    }
    for rebase in diff.reference_rebases() {
        let frame = frame_alias(frames, rebase.frame())?;
        checked_write(
            &mut output,
            format_args!(
                "R f=f{} old={} ref={}\n",
                frame,
                rebase.previous_reference().model_token(),
                rebase.current_reference().model_token(),
            ),
        )?;
    }

    let content = output.finish();
    let lines = content.bytes().filter(|byte| *byte == b'\n').count();
    let stats = SemanticDiffEncodingStats {
        bytes: u32::try_from(content.len()).map_err(|_| SemanticModelEncodingError::Budget)?,
        lines: u16::try_from(lines).map_err(|_| SemanticModelEncodingError::Invariant)?,
        frames: u8::try_from(frames.len()).map_err(|_| SemanticModelEncodingError::Invariant)?,
        entries: diff.stats().entries(),
        reference_rebases: diff.stats().reference_rebases(),
        secret_nodes,
    };
    Ok(SemanticEncodedDiff {
        content,
        budget,
        stats,
        current_fingerprint: diff.current_fingerprint().clone(),
        diff_guard: diff.guard(),
    })
}

fn frame_alias(
    frames: &[SemanticDiffFrame],
    frame: &SemanticFrameJoin,
) -> Result<usize, SemanticModelEncodingError> {
    frames
        .iter()
        .position(|candidate| candidate.frame() == frame)
        .map(|index| index + 1)
        .ok_or(SemanticModelEncodingError::Invariant)
}

fn write_entry(
    output: &mut BoundedModelBuffer,
    frame: usize,
    entry: &SemanticDiffEntry,
) -> Result<(), SemanticModelEncodingError> {
    match entry.kind() {
        SemanticDiffEntryKind::Removed => {
            let previous = entry
                .previous_reference()
                .ok_or(SemanticModelEncodingError::Invariant)?;
            if entry.current_reference().is_some() || entry.current_node().is_some() {
                return Err(SemanticModelEncodingError::Invariant);
            }
            checked_write(
                output,
                format_args!(
                    "D remove f=f{} old={} r={}\n",
                    frame,
                    previous.model_token(),
                    role_label(entry.role()),
                ),
            )
        }
        SemanticDiffEntryKind::Added => {
            if entry.previous_reference().is_some()
                || !entry.changes().is_empty()
                || entry.movement().is_some()
            {
                return Err(SemanticModelEncodingError::Invariant);
            }
            let node = current_node(entry)?;
            write_current_prefix(output, "add", frame, entry)?;
            write_full_node(output, node)?;
            output.push("\n")
        }
        SemanticDiffEntryKind::Changed => {
            if entry.changes().is_empty() || entry.movement().is_some() {
                return Err(SemanticModelEncodingError::Invariant);
            }
            let node = current_node(entry)?;
            write_current_prefix(output, "change", frame, entry)?;
            write_change_labels(output, entry.changes())?;
            write_changed_fields(output, node, entry.changes())?;
            output.push("\n")
        }
        SemanticDiffEntryKind::Moved => {
            if !entry.changes().is_empty() || entry.movement().is_none() {
                return Err(SemanticModelEncodingError::Invariant);
            }
            current_node(entry)?;
            write_current_prefix(output, "move", frame, entry)?;
            write_movement(output, entry)?;
            output.push("\n")
        }
        SemanticDiffEntryKind::ChangedAndMoved => {
            if entry.changes().is_empty() || entry.movement().is_none() {
                return Err(SemanticModelEncodingError::Invariant);
            }
            let node = current_node(entry)?;
            write_current_prefix(output, "change_move", frame, entry)?;
            write_movement(output, entry)?;
            write_change_labels(output, entry.changes())?;
            write_changed_fields(output, node, entry.changes())?;
            output.push("\n")
        }
    }
}

fn current_node(entry: &SemanticDiffEntry) -> Result<&SemanticNode, SemanticModelEncodingError> {
    let node = entry
        .current_node()
        .ok_or(SemanticModelEncodingError::Invariant)?;
    if entry.current_reference() != Some(node.reference()) || entry.role() != node.role() {
        return Err(SemanticModelEncodingError::Invariant);
    }
    Ok(node)
}

fn write_current_prefix(
    output: &mut BoundedModelBuffer,
    kind: &str,
    frame: usize,
    entry: &SemanticDiffEntry,
) -> Result<(), SemanticModelEncodingError> {
    let current = entry
        .current_reference()
        .ok_or(SemanticModelEncodingError::Invariant)?;
    checked_write(output, format_args!("D {kind} f=f{frame}"))?;
    if let Some(previous) = entry.previous_reference() {
        checked_write(output, format_args!(" old={}", previous.model_token()))?;
    }
    checked_write(output, format_args!(" ref={} p=", current.model_token()))?;
    write_reference_or_root(output, entry.current_parent())?;
    let ordinal = entry
        .current_sibling_ordinal()
        .ok_or(SemanticModelEncodingError::Invariant)?;
    checked_write(
        output,
        format_args!(" i={} r={}", ordinal, role_label(entry.role())),
    )
}

fn write_movement(
    output: &mut BoundedModelBuffer,
    entry: &SemanticDiffEntry,
) -> Result<(), SemanticModelEncodingError> {
    let movement = entry
        .movement()
        .ok_or(SemanticModelEncodingError::Invariant)?;
    output.push(" from_p=")?;
    match movement.previous_parent() {
        Some(reference) => output.push(&reference.model_token())?,
        None => output.push("-")?,
    }
    checked_write(
        output,
        format_args!(" from_i={}", movement.previous_sibling_ordinal()),
    )
}

fn write_reference_or_root(
    output: &mut BoundedModelBuffer,
    reference: Option<SemanticReferenceId>,
) -> Result<(), SemanticModelEncodingError> {
    match reference {
        Some(reference) => output.push(&reference.model_token()),
        None => output.push("-"),
    }
}

fn write_full_node(
    output: &mut BoundedModelBuffer,
    node: &SemanticNode,
) -> Result<(), SemanticModelEncodingError> {
    checked_write(
        output,
        format_args!(
            " q={} src={}",
            sensitivity_label(node.sensitivity()),
            source_label(node.trust()),
        ),
    )?;
    if let Some(level) = node.heading_level() {
        checked_write(output, format_args!(" level={}", level.get()))?;
    }
    if let Some(kind) = node.landmark_kind() {
        checked_write(output, format_args!(" landmark={}", kind.label()))?;
    }
    if let Some(target) = node.link_destination() {
        output.push(" destination=")?;
        write_quoted(output, target.as_url().as_str())?;
    }
    write_states(output, node.states())?;
    write_operations(output, node.operations())?;
    if let Some(name) = node.name() {
        output.push(" name=")?;
        write_quoted(output, name.as_str())?;
    }
    if let Some(text) = node.text() {
        output.push(" text=")?;
        write_quoted(output, text.as_str())?;
    }
    if let Some(value) = node.value() {
        output.push(" value=")?;
        write_value(output, value)?;
    }
    Ok(())
}

fn write_change_labels(
    output: &mut BoundedModelBuffer,
    changes: SemanticNodeChanges,
) -> Result<(), SemanticModelEncodingError> {
    output.push(" changed=")?;
    let mut separator = "";
    for (change, label) in change_labels() {
        if changes.contains(change) {
            output.push(separator)?;
            output.push(label)?;
            separator = ",";
        }
    }
    if separator.is_empty() {
        return Err(SemanticModelEncodingError::Invariant);
    }
    Ok(())
}

fn write_changed_fields(
    output: &mut BoundedModelBuffer,
    node: &SemanticNode,
    changes: SemanticNodeChanges,
) -> Result<(), SemanticModelEncodingError> {
    if changes.contains(SemanticNodeChange::Name) {
        output.push(" name=")?;
        write_optional_text(output, node.name().map(|value| value.as_str()))?;
    }
    if changes.contains(SemanticNodeChange::LinkDestination) {
        output.push(" destination=")?;
        write_optional_text(
            output,
            node.link_destination()
                .map(|target| target.as_url().as_str()),
        )?;
    }
    if changes.contains(SemanticNodeChange::Text) {
        output.push(" text=")?;
        write_optional_text(output, node.text().map(|value| value.as_str()))?;
    }
    if changes.contains(SemanticNodeChange::Value) {
        output.push(" value=")?;
        match node.value() {
            Some(value) => write_value(output, value)?,
            None => output.push("-")?,
        }
    }
    if changes.contains(SemanticNodeChange::States) {
        write_state_inventory(output, node.states())?;
    }
    if changes.contains(SemanticNodeChange::Operations) {
        write_operation_inventory(output, node.operations())?;
    }
    if changes.contains(SemanticNodeChange::Sensitivity) {
        checked_write(
            output,
            format_args!(" q={}", sensitivity_label(node.sensitivity())),
        )?;
    }
    if changes.contains(SemanticNodeChange::Trust) {
        checked_write(output, format_args!(" src={}", source_label(node.trust())))?;
    }
    // Geometry remains in the authenticated semantic diff and fingerprint for
    // Rust-side freshness, hit-testing, and occlusion checks. Coordinates are
    // deliberately not projected into the model transcript, which acts only
    // through opaque references.
    if changes.contains(SemanticNodeChange::LandmarkKind) {
        checked_write(
            output,
            format_args!(
                " landmark={}",
                node.landmark_kind().map_or("-", |kind| kind.label())
            ),
        )?;
    }
    if changes.contains(SemanticNodeChange::HeadingLevel) {
        output.push(" level=")?;
        match node.heading_level() {
            Some(level) => checked_write(output, format_args!("{}", level.get()))?,
            None => output.push("-")?,
        }
    }
    Ok(())
}

fn write_optional_text(
    output: &mut BoundedModelBuffer,
    value: Option<&str>,
) -> Result<(), SemanticModelEncodingError> {
    match value {
        Some(value) => write_quoted(output, value),
        None => output.push("-"),
    }
}

fn write_state_inventory(
    output: &mut BoundedModelBuffer,
    states: SemanticStates,
) -> Result<(), SemanticModelEncodingError> {
    output.push(" states=")?;
    let values = [
        (SemanticState::Checked, "checked"),
        (SemanticState::Selected, "selected"),
        (SemanticState::Expanded, "expanded"),
        (SemanticState::Disabled, "disabled"),
        (SemanticState::Required, "required"),
        (SemanticState::Invalid, "invalid"),
        (SemanticState::Focused, "focused"),
    ];
    let mut separator = "";
    for (state, label) in values {
        if states.contains(state) {
            output.push(separator)?;
            output.push(label)?;
            separator = ",";
        }
    }
    if separator.is_empty() {
        output.push("-")?;
    }
    Ok(())
}

fn write_operation_inventory(
    output: &mut BoundedModelBuffer,
    operations: SemanticOperations,
) -> Result<(), SemanticModelEncodingError> {
    output.push(" ops=")?;
    let values = [
        (SemanticOperationClass::Click, "click"),
        (SemanticOperationClass::Fill, "fill"),
        (SemanticOperationClass::Select, "select"),
        (SemanticOperationClass::Press, "press"),
        (SemanticOperationClass::Scroll, "scroll"),
    ];
    let mut separator = "";
    for (operation, label) in values {
        if operations.contains(operation) {
            output.push(separator)?;
            output.push(label)?;
            separator = ",";
        }
    }
    if separator.is_empty() {
        output.push("-")?;
    }
    Ok(())
}

fn change_labels() -> [(SemanticNodeChange, &'static str); 11] {
    [
        (SemanticNodeChange::Name, "name"),
        (SemanticNodeChange::Text, "text"),
        (SemanticNodeChange::Value, "value"),
        (SemanticNodeChange::States, "states"),
        (SemanticNodeChange::Operations, "ops"),
        (SemanticNodeChange::Sensitivity, "sensitivity"),
        (SemanticNodeChange::Trust, "trust"),
        (SemanticNodeChange::Geometry, "geometry"),
        (SemanticNodeChange::HeadingLevel, "heading_level"),
        (SemanticNodeChange::LandmarkKind, "landmark_kind"),
        (SemanticNodeChange::LinkDestination, "link_destination"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        compute_semantic_diff, decode_semantic_snapshot, encode_semantic_observation,
        ContextCapabilities, ContextCapability, ContextId, ContextIdentity, ContextKind,
        ContextOperationId, ContextRegistry, ContextRunId, ContextSettlement, FrameGeneration,
        FrameId, SemanticDecodeContext, SemanticDiffBudget, SemanticDiffOutcome,
        SemanticFrameTrust, SemanticInvocationId, SemanticObservation,
        SemanticObservationAssembler, SemanticObservationBudget, SemanticObservationId,
        SemanticObservationRequest, SemanticOrigin, SemanticSnapshotGeneration,
        SemanticTokenCountQuality, SemanticTokenCountRequirement, SemanticTokenCounterError,
        SemanticTokenMeasurementError, SEMANTIC_WIRE_VERSION,
    };
    use serde_json::{json, Value};
    use zephium_core::ids::ProfileId;

    fn context() -> crate::ContextJoin {
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
        let operation = registry
            .begin_context(
                identity.id(),
                ContextOperationId::new(1).expect("operation"),
            )
            .expect("construct");
        registry
            .settle_construction(identity.id(), operation, ContextSettlement::Applied)
            .expect("settle");
        registry.join(identity.id()).expect("join")
    }

    fn observation(
        context: crate::ContextJoin,
        observation_id: u64,
        invocation: u64,
        snapshot_generation: u64,
        nodes: Value,
    ) -> SemanticObservation {
        observation_with_budget(
            context,
            observation_id,
            invocation,
            snapshot_generation,
            nodes,
            SemanticObservationBudget::try_new(64, 8192, 1).expect("budget"),
        )
    }

    fn observation_with_budget(
        context: crate::ContextJoin,
        observation_id: u64,
        invocation: u64,
        snapshot_generation: u64,
        nodes: Value,
        budget: SemanticObservationBudget,
    ) -> SemanticObservation {
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            FrameGeneration::INITIAL,
            SemanticOrigin::parse("https://encoded-private.example.test/path").expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        let generation = SemanticSnapshotGeneration::new(snapshot_generation).expect("generation");
        let wire = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": invocation,
            "g": snapshot_generation,
            "c": "complete",
            "n": nodes,
        }))
        .expect("wire");
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(invocation).expect("invocation"),
                frame,
                generation,
            ),
            &wire,
        )
        .expect("snapshot");
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(observation_id).expect("observation id"),
            context,
            budget,
        );
        SemanticObservationAssembler::new(request, snapshot)
            .expect("assembler")
            .finish()
            .expect("observation")
    }

    struct FixedCounter {
        revision: SemanticTokenizerRevision,
        tokens: u32,
        quality: SemanticTokenCountQuality,
    }

    impl SemanticTokenCounter for FixedCounter {
        fn count_tokens(
            &self,
            input: &str,
        ) -> Result<SemanticTokenMeasurement, SemanticTokenCounterError> {
            if input.is_empty() {
                return Err(SemanticTokenCounterError::InvalidResult);
            }
            SemanticTokenMeasurement::try_new(self.revision.clone(), self.tokens, self.quality)
                .map_err(|SemanticTokenMeasurementError::Invalid| {
                    SemanticTokenCounterError::InvalidResult
                })
        }
    }

    fn revision() -> SemanticTokenizerRevision {
        SemanticTokenizerRevision::try_new("test:exact:v1".to_owned()).expect("revision")
    }

    fn exact_counter(tokens: u32) -> FixedCounter {
        FixedCounter {
            revision: revision(),
            tokens,
            quality: SemanticTokenCountQuality::ExactLocal,
        }
    }

    fn acknowledge(observation: &SemanticObservation) -> SemanticObservationAcknowledgement {
        let revision = revision();
        encode_semantic_observation(
            observation,
            SemanticModelEncodingBudget::try_new(
                64 * 1024,
                1000,
                SemanticTokenCountRequirement::Exact,
            )
            .expect("budget"),
        )
        .expect("encode")
        .admit(&exact_counter(100), &revision)
        .expect("admit")
        .settle_delivery(SemanticModelDeliverySettlement::Committed)
        .expect("delivery")
    }

    fn changed_diff() -> Box<SemanticDiff> {
        let context = context();
        let previous = observation(
            context,
            1,
            11,
            21,
            json!([
                {"k": 9001, "r": "document"},
                {"k": 9002, "p": 0, "r": "heading", "l": 2, "n": "Private old heading"},
                {"k": 9003, "p": 0, "r": "button", "n": "Continue", "o": 1},
                {"k": 9004, "p": 0, "r": "paragraph", "t": "Removed private text"}
            ]),
        );
        let current = observation(
            context,
            2,
            12,
            22,
            json!([
                {"k": 9001, "r": "document"},
                {"k": 9003, "p": 0, "r": "button", "n": "Continue", "o": 1},
                {"k": 9002, "p": 0, "r": "heading", "l": 2, "n": "Private new heading"},
                {"k": 9005, "p": 0, "r": "paragraph", "t": "Added private text"}
            ]),
        );
        let acknowledgement = acknowledge(&previous);
        match compute_semantic_diff(
            &previous,
            &acknowledgement,
            &current,
            SemanticDiffBudget::ACTION,
        ) {
            SemanticDiffOutcome::Diff(diff) => diff,
            SemanticDiffOutcome::FreshSnapshot(reason) => {
                panic!("unexpected fresh snapshot: {reason:?}")
            }
        }
    }

    fn rebase_diff() -> Box<SemanticDiff> {
        let context = context();
        let previous = observation(
            context,
            10,
            31,
            41,
            json!([
                {"k": 1001, "r": "document"},
                {"k": 1002, "p": 0, "r": "button", "n": "First", "o": 1},
                {"k": 1003, "p": 0, "r": "button", "n": "Second", "o": 1}
            ]),
        );
        let current = observation(
            context,
            11,
            32,
            42,
            json!([
                {"k": 1001, "r": "document"},
                {"k": 1004, "p": 0, "r": "paragraph", "t": "Inserted private text"},
                {"k": 1002, "p": 0, "r": "button", "n": "First", "o": 1},
                {"k": 1003, "p": 0, "r": "button", "n": "Second", "o": 1}
            ]),
        );
        let acknowledgement = acknowledge(&previous);
        match compute_semantic_diff(
            &previous,
            &acknowledgement,
            &current,
            SemanticDiffBudget::ACTION,
        ) {
            SemanticDiffOutcome::Diff(diff) => diff,
            SemanticDiffOutcome::FreshSnapshot(reason) => {
                panic!("unexpected fresh snapshot: {reason:?}")
            }
        }
    }

    fn value_diff(value: &str) -> Box<SemanticDiff> {
        let context = context();
        let previous = observation(
            context,
            20,
            41,
            51,
            json!([
                {"k": 2001, "r": "document"},
                {"k": 2002, "p": 0, "r": "textbox", "n": "Long value",
                 "v": {"k": "text", "value": "old"}, "o": 11}
            ]),
        );
        let current = observation(
            context,
            21,
            42,
            52,
            json!([
                {"k": 2001, "r": "document"},
                {"k": 2002, "p": 0, "r": "textbox", "n": "Long value",
                 "v": {"k": "text", "value": value}, "o": 11}
            ]),
        );
        let acknowledgement = acknowledge(&previous);
        match compute_semantic_diff(
            &previous,
            &acknowledgement,
            &current,
            SemanticDiffBudget::ACTION,
        ) {
            SemanticDiffOutcome::Diff(diff) => diff,
            SemanticDiffOutcome::FreshSnapshot(reason) => {
                panic!("unexpected value fresh snapshot: {reason:?}")
            }
        }
    }

    #[test]
    fn oversized_public_link_capture_keeps_exact_bounded_prefix_and_truthful_evidence() {
        use crate::*;
        let context = context();
        let mut nodes = vec![json!({"k":9001,"r":"landmark","lm":"main"})];
        for index in 0..72 {
            nodes.push(
                json!({"k":9002+index,"p":0,"r":"link","n":format!("Result {index}"),
                "u":format!("https://public.example.test/{index}/{}", "a".repeat(170))}),
            );
        }
        let raw = observation_with_budget(
            context,
            1,
            11,
            21,
            Value::Array(nodes),
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        assert!(raw.total_text_bytes() <= 16 * 1024);
        let budget = SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE;
        assert!(matches!(
            encode_semantic_observation(&raw, budget),
            Err(SemanticModelEncodingError::OutputLimit)
        ));
        let projected = fit_semantic_observation_for_model(raw.clone(), budget).unwrap();
        assert!(projected.node_count() > 1 && projected.node_count() < raw.node_count());
        assert_eq!(projected.request(), raw.request());
        assert_eq!(projected.frames()[0].frame(), raw.frames()[0].frame());
        assert_eq!(
            projected.frames()[0].invocation(),
            raw.frames()[0].invocation()
        );
        assert_eq!(
            projected.frames()[0].generation(),
            raw.frames()[0].generation()
        );
        assert_eq!(
            projected.frames()[0].nodes(),
            &raw.frames()[0].nodes()[..usize::from(projected.node_count())]
        );
        assert_eq!(
            projected.frames()[0].completeness(),
            SemanticCompleteness::Truncated(SemanticTruncation::ModelProjectionLimit)
        );
        let payload = encode_semantic_observation(&projected, budget)
            .unwrap()
            .admit_conservative_utf8(&revision())
            .unwrap();
        assert!(payload.as_str().len() <= 16 * 1024);
        assert!(payload
            .as_str()
            .contains("complete=truncated_model_projection"));
        let ack = payload
            .settle_delivery(SemanticModelDeliverySettlement::Committed)
            .unwrap();
        assert!(ack.matches(&projected));
        assert!(
            !ack.matches(&raw),
            "full capture cannot borrow reduced delivery authority"
        );
        let removed = raw.frames()[0].nodes()[usize::from(projected.node_count())].reference();
        assert!(projected
            .resolve_node(removed, projected.frames()[0].frame())
            .is_err());
        let anchor = projected.frames()[0].nodes()[0].reference();
        assert!(projected
            .begin_expansion(
                SemanticObservationId::new(2).unwrap(),
                anchor,
                projected.frames()[0].frame(),
                SemanticExpansionKind::Region,
                SemanticObservationBudget::INITIAL_FILTERED
            )
            .is_ok());
        assert!(read_semantic_observation(
            &projected,
            SemanticReadAuthority::Acknowledged(&ack),
            SemanticCaptureInstant::from_millis(1),
            SemanticReadSensitivityLimit::PublicOnly,
            SemanticReadBudget::STANDARD
        )
        .is_ok());
        let larger = raw
            .model_prefix(usize::from(projected.node_count()) + 1)
            .unwrap();
        assert!(matches!(
            encode_semantic_observation(&larger, budget),
            Err(SemanticModelEncodingError::OutputLimit)
        ));
        let tiny = SemanticModelEncodingBudget::try_new(1, 1, SemanticTokenCountRequirement::Exact)
            .unwrap();
        assert!(matches!(
            fit_semantic_observation_for_model(raw, tiny),
            Err(SemanticModelEncodingError::OutputLimit)
        ));
    }

    #[test]
    fn landmark_kind_is_projected_and_changes_retire_the_exact_fingerprint() {
        let context = context();
        let previous = observation(
            context,
            1,
            11,
            21,
            json!([{"k":9001,"r":"document"},{"k":9002,"p":0,"r":"landmark","lm":"navigation"}]),
        );
        let current = observation(
            context,
            2,
            12,
            22,
            json!([{"k":9001,"r":"document"},{"k":9002,"p":0,"r":"landmark","lm":"main"}]),
        );
        let budget =
            SemanticModelEncodingBudget::try_new(8192, 1000, SemanticTokenCountRequirement::Exact)
                .unwrap();
        let full = encode_semantic_observation(&current, budget)
            .unwrap()
            .admit(&exact_counter(100), &revision())
            .unwrap();
        assert!(full
            .as_str()
            .contains("r=landmark q=public src=page landmark=main"));
        assert!(
            !full.as_str().contains("name="),
            "subtype must not invent an accessible name"
        );
        let SemanticDiffOutcome::Diff(diff) = compute_semantic_diff(
            &previous,
            &acknowledge(&previous),
            &current,
            SemanticDiffBudget::ACTION,
        ) else {
            panic!("bounded subtype diff")
        };
        let encoded = encode_semantic_diff(&diff, budget).unwrap();
        assert!(encoded.content.contains("landmark=main"));
        assert!(encoded.content.contains("landmark_kind"));
        assert!(!encoded.content.contains("navigation"));
    }

    #[test]
    fn compact_diff_is_deterministic_delimited_and_stable_key_free() {
        let diff = changed_diff();
        let budget =
            SemanticModelEncodingBudget::try_new(8192, 1000, SemanticTokenCountRequirement::Exact)
                .expect("budget");
        let first = encode_semantic_diff(&diff, budget).expect("encode");
        let second = encode_semantic_diff(&diff, budget).expect("encode");
        assert_eq!(first.content, second.content);
        assert!(first.content.starts_with(
            "ZDIFF3 content=untrusted from_generation=1 to_generation=1 entries=4 rebases=0\n"
        ));
        assert!(first
            .content
            .contains("D remove f=f1 old=old:@a4 r=paragraph\n"));
        assert!(first.content.contains(
            "D move f=f1 old=old:@a3 ref=@a2 p=@a1 i=0 r=button from_p=old:@a1 from_i=1\n"
        ));
        assert!(first.content.contains(
            "D change_move f=f1 old=old:@a2 ref=@a3 p=@a1 i=1 r=heading from_p=old:@a1 from_i=0 changed=name name=\"Private new heading\"\n"
        ));
        assert!(first.content.contains(
            "D add f=f1 ref=@a4 p=@a1 i=2 r=paragraph q=public src=page text=\"Added private text\"\n"
        ));
        assert!(first
            .content
            .contains("origin=\"https://encoded-private.example.test/\""));
        assert!(first
            .content
            .contains("trust=same from_snapshot=21 snapshot=22\n"));
        assert!(!first.content.contains("9001"));
        assert_eq!(first.stats().frames(), 1);
        assert_eq!(first.stats().entries(), 4);
        assert_eq!(first.stats().reference_rebases(), 0);
        assert_eq!(first.stats().lines(), 6);
        assert_eq!(first.stats().secret_nodes(), 0);
        let debug = format!("{first:?}");
        assert!(!debug.contains("Private"));
        assert!(!debug.contains("encoded-private"));
        assert!(debug.contains("[redacted]"));
    }

    #[test]
    fn diff_projects_only_the_shared_bounded_value_preview() {
        let value = "x".repeat(crate::MAX_SEMANTIC_VALUE_BYTES);
        let diff = value_diff(&value);
        let encoded = encode_semantic_diff(
            &diff,
            SemanticModelEncodingBudget::try_new(8192, 1000, SemanticTokenCountRequirement::Exact)
                .expect("budget"),
        )
        .expect("encode long value diff");
        let expected = format!(
            "value=\"{}\" source_bytes={} truncated=true",
            "x".repeat(crate::MAX_SEMANTIC_VALUE_PREVIEW_BYTES),
            crate::MAX_SEMANTIC_VALUE_BYTES
        );

        assert!(encoded.content.contains(&expected));
        assert!(!encoded.content.contains(&format!(
            "\"{}",
            "x".repeat(crate::MAX_SEMANTIC_VALUE_PREVIEW_BYTES + 1)
        )));
    }

    #[test]
    fn unchanged_reference_shifts_are_encoded_and_counted() {
        let diff = rebase_diff();
        let encoded = encode_semantic_diff(
            &diff,
            SemanticModelEncodingBudget::try_new(8192, 1000, SemanticTokenCountRequirement::Exact)
                .expect("budget"),
        )
        .expect("encode");
        assert!(encoded.content.contains("entries=1 rebases=2"));
        assert!(encoded.content.contains("R f=f1 old=old:@a2 ref=@a3\n"));
        assert!(encoded.content.contains("R f=f1 old=old:@a3 ref=@a4\n"));
        assert_eq!(encoded.stats().reference_rebases(), 2);
    }

    #[test]
    fn delivery_proof_binds_the_exact_baseline_and_current_pair() {
        let context = context();
        let actual_previous = observation(
            context,
            50,
            61,
            71,
            json!([
                {"k": 3001, "r": "document"},
                {"k": 3002, "p": 0, "r": "button", "n": "Private current action", "o": 1}
            ]),
        );
        let alternate_previous =
            observation(context, 50, 61, 71, json!([{"k": 3001, "r": "document"}]));
        let current = observation(
            context,
            51,
            62,
            72,
            json!([
                {"k": 3001, "r": "document"},
                {"k": 3002, "p": 0, "r": "button", "n": "Private current action", "o": 1}
            ]),
        );
        let exact_diff = match compute_semantic_diff(
            &actual_previous,
            &acknowledge(&actual_previous),
            &current,
            SemanticDiffBudget::ACTION,
        ) {
            SemanticDiffOutcome::Diff(diff) => diff,
            SemanticDiffOutcome::FreshSnapshot(reason) => {
                panic!("unexpected exact fresh snapshot: {reason:?}")
            }
        };
        let alternate_diff = match compute_semantic_diff(
            &alternate_previous,
            &acknowledge(&alternate_previous),
            &current,
            SemanticDiffBudget::ACTION,
        ) {
            SemanticDiffOutcome::Diff(diff) => diff,
            SemanticDiffOutcome::FreshSnapshot(reason) => {
                panic!("unexpected alternate fresh snapshot: {reason:?}")
            }
        };
        assert_ne!(exact_diff.guard(), alternate_diff.guard());

        let exact_payload =
            encode_semantic_diff(&exact_diff, SemanticModelEncodingBudget::ACTION_DIFF_EXACT)
                .expect("encode exact")
                .admit(&exact_counter(40), &revision())
                .expect("admit exact");
        assert!(exact_payload.matches_diff(&exact_diff));
        let alternate_payload = encode_semantic_diff(
            &alternate_diff,
            SemanticModelEncodingBudget::ACTION_DIFF_EXACT,
        )
        .expect("encode alternate")
        .admit(&exact_counter(40), &revision())
        .expect("admit alternate");
        assert!(!alternate_payload.matches_diff(&exact_diff));

        let (content, stats, delivery) = exact_payload.into_provider_parts();
        assert!(content.starts_with("ZDIFF3"));
        assert_eq!(
            usize::try_from(stats.bytes()).expect("bytes"),
            content.len()
        );
        let delivery_debug = format!("{delivery:?}");
        assert!(!delivery_debug.contains("Private current action"));
        assert!(delivery_debug.contains("[redacted]"));
        let receipt = delivery.commit();
        assert_eq!(receipt.guard(), exact_diff.guard());
        assert!(receipt.acknowledgement().matches(&current));
        let debug = format!("{exact_diff:?} {receipt:?}");
        assert!(!debug.contains("Private current action"));
        assert!(!debug.contains("encoded-private"));
        assert!(debug.contains("[redacted]"));
    }

    #[test]
    fn action_diff_requires_selected_quality_revision_and_two_hundred_token_cap() {
        assert_eq!(
            SemanticModelEncodingBudget::ACTION_DIFF_EXACT.max_tokens(),
            crate::ACTION_SEMANTIC_DIFF_TOKEN_TARGET
        );
        assert_eq!(crate::ACTION_SEMANTIC_DIFF_TOKEN_TARGET, 200);
        let diff = changed_diff();
        let expected = revision();

        let oversized = exact_counter(201);
        assert!(matches!(
            encode_semantic_diff(&diff, SemanticModelEncodingBudget::ACTION_DIFF_EXACT)
                .expect("encode")
                .measure(&oversized, &expected),
            Err(SemanticModelEncodingError::TokenLimit)
        ));

        let estimated = FixedCounter {
            revision: expected.clone(),
            tokens: 190,
            quality: SemanticTokenCountQuality::ProviderEstimate,
        };
        assert_eq!(
            encode_semantic_diff(&diff, SemanticModelEncodingBudget::ACTION_DIFF_EXACT)
                .expect("encode")
                .measure(&estimated, &expected),
            Err(SemanticModelEncodingError::TokenQuality)
        );
        let payload = encode_semantic_diff(
            &diff,
            SemanticModelEncodingBudget::ACTION_DIFF_PROVIDER_ESTIMATE,
        )
        .expect("encode")
        .admit(&estimated, &expected)
        .expect("admit");
        assert_eq!(payload.token_measurement().tokens(), 190);
        assert!(payload.as_str().starts_with("ZDIFF3"));
        let debug = format!("{payload:?}");
        assert!(!debug.contains("Private"));
        assert!(!debug.contains("encoded-private"));
        assert!(debug.contains("[redacted]"));
        let acknowledgement = payload
            .settle_delivery(SemanticModelDeliverySettlement::Committed)
            .expect("delivery");
        assert_eq!(acknowledgement.observation().get(), 2);

        let wrong_revision = FixedCounter {
            revision: SemanticTokenizerRevision::try_new("wrong:v1".to_owned()).expect("revision"),
            tokens: 100,
            quality: SemanticTokenCountQuality::ExactLocal,
        };
        assert!(matches!(
            encode_semantic_diff(&diff, SemanticModelEncodingBudget::ACTION_DIFF_EXACT)
                .expect("encode")
                .measure(&wrong_revision, &expected),
            Err(SemanticModelEncodingError::TokenizerRevisionMismatch)
        ));
    }

    #[test]
    fn action_diff_conservative_utf8_is_byte_exact_revision_bound_and_redacted() {
        let selected =
            SemanticTokenizerRevision::try_new("openai:responses-input-count:v1".to_owned())
                .expect("revision");
        let diff = value_diff("żółw");
        assert_eq!(
            SemanticModelEncodingBudget::ACTION_DIFF_CONSERVATIVE.max_tokens(),
            crate::ACTION_SEMANTIC_DIFF_TOKEN_TARGET
        );
        let conservative_budget = SemanticModelEncodingBudget::try_new(
            16 * 1024,
            1_000,
            SemanticTokenCountRequirement::ConservativeAllowed,
        )
        .expect("test conservative budget");
        let encoded =
            encode_semantic_diff(&diff, conservative_budget).expect("conservative diff encoding");
        let bytes = encoded.stats().bytes();
        assert!(encoded.content.len() > encoded.content.chars().count());
        let payload = encoded
            .admit_conservative_utf8(&selected)
            .expect("conservative diff admission");
        assert_eq!(payload.token_measurement().tokens(), bytes);
        assert_eq!(
            payload.token_measurement().quality(),
            SemanticTokenCountQuality::Conservative
        );
        assert_eq!(payload.token_measurement().revision(), &selected);
        let debug = format!("{payload:?}");
        assert!(!debug.contains("żółw"));
        assert!(!debug.contains("encoded-private"));
        assert!(debug.contains("[redacted]"));

        assert!(matches!(
            encode_semantic_diff(&diff, SemanticModelEncodingBudget::ACTION_DIFF_CONSERVATIVE)
                .expect("target encoding")
                .admit_conservative_utf8(&selected),
            Err(SemanticModelEncodingError::TokenLimit)
        ));

        let provider_exact_preflight = encode_semantic_diff(
            &diff,
            SemanticModelEncodingBudget::ACTION_DIFF_PROVIDER_EXACT_CONSERVATIVE,
        )
        .expect("provider-exact diff encoding")
        .admit_conservative_utf8(&selected)
        .expect("bounded provider-exact preflight");
        assert!(
            provider_exact_preflight.token_measurement().tokens()
                > crate::ACTION_SEMANTIC_DIFF_TOKEN_TARGET
        );
        assert!(
            provider_exact_preflight.token_measurement().tokens()
                <= crate::ACTION_DIFF_PROVIDER_EXACT_CONSERVATIVE_TOKEN_CEILING
        );

        assert!(matches!(
            encode_semantic_diff(&diff, SemanticModelEncodingBudget::ACTION_DIFF_EXACT)
                .expect("exact encoding")
                .admit_conservative_utf8(&selected),
            Err(SemanticModelEncodingError::TokenQuality)
        ));

        let too_small = SemanticModelEncodingBudget::try_new(
            bytes,
            bytes - 1,
            SemanticTokenCountRequirement::ConservativeAllowed,
        )
        .expect("tight budget");
        assert!(matches!(
            encode_semantic_diff(&diff, too_small)
                .expect("byte-fitting encoding")
                .admit_conservative_utf8(&selected),
            Err(SemanticModelEncodingError::TokenLimit)
        ));
    }

    #[test]
    fn byte_limit_refuses_without_returning_partial_diff_content() {
        assert!(matches!(
            encode_semantic_diff(
                &changed_diff(),
                SemanticModelEncodingBudget::try_new(
                    64,
                    200,
                    SemanticTokenCountRequirement::Exact,
                )
                .expect("budget"),
            ),
            Err(SemanticModelEncodingError::OutputLimit)
        ));
    }
}
