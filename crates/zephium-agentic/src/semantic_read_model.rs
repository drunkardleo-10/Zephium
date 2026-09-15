//! Deterministic token-admitted model encoding for bounded semantic reads.
//!
//! Read content stays private until the selected tokenizer port admits the
//! exact `ZREAD3` bytes. Committed transport acknowledges only the exact read
//! projection; refused or cancelled transport consumes the payload without
//! creating authority.

use std::fmt;

use crate::semantic_model::{
    checked_write, frame_trust_label, role_label, sensitivity_label, source_label,
    validate_semantic_token_measurement, write_quoted, write_value_preview, BoundedModelBuffer,
};
use crate::{
    ContextJoin, SemanticCaptureInstant, SemanticFrameJoin, SemanticModelDeliveryError,
    SemanticModelDeliverySettlement, SemanticModelEncodingBudget, SemanticModelEncodingError,
    SemanticObservationGeneration, SemanticObservationId, SemanticReadContent, SemanticReadField,
    SemanticReadOmission, SemanticReadProvenance, SemanticReadResult, SemanticSensitivity,
    SemanticTokenCounter, SemanticTokenMeasurement, SemanticTokenizerRevision,
};

/// Version of the compact semantic-read model-input grammar.
pub const SEMANTIC_READ_MODEL_SCHEMA_VERSION: u16 = 3;

/// Content-free deterministic semantic-read encoding metrics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticReadEncodingStats {
    bytes: u32,
    lines: u16,
    frames: u8,
    items: u16,
    sensitive_items: u16,
    omitted_items: u16,
}

impl SemanticReadEncodingStats {
    #[cfg(test)]
    pub(crate) const fn for_input_metrics_test(
        bytes: u32,
        lines: u16,
        frames: u8,
        items: u16,
        sensitive_items: u16,
        omitted_items: u16,
    ) -> Self {
        Self {
            bytes,
            lines,
            frames,
            items,
            sensitive_items,
            omitted_items,
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

    /// Distinct source frames represented by retained values.
    pub const fn frames(self) -> u8 {
        self.frames
    }

    /// Retained readable value count.
    pub const fn items(self) -> u16 {
        self.items
    }

    /// Retained policy-admitted sensitive value count.
    pub const fn sensitive_items(self) -> u16 {
        self.sensitive_items
    }

    /// Truthfully omitted value count.
    pub const fn omitted_items(self) -> u16 {
        self.omitted_items
    }
}

/// Private compact read bytes awaiting required token measurement.
pub struct SemanticEncodedRead {
    content: String,
    budget: SemanticModelEncodingBudget,
    stats: SemanticReadEncodingStats,
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
    context: ContextJoin,
    observation_guard: [u8; 32],
    captured_at: SemanticCaptureInstant,
    read_guard: [u8; 32],
}

pub(crate) struct SemanticEncodedReadParts {
    pub(crate) content: String,
    pub(crate) budget: SemanticModelEncodingBudget,
    pub(crate) stats: SemanticReadEncodingStats,
    pub(crate) observation: SemanticObservationId,
    pub(crate) observation_generation: SemanticObservationGeneration,
    pub(crate) context: ContextJoin,
    pub(crate) observation_guard: [u8; 32],
    pub(crate) captured_at: SemanticCaptureInstant,
    pub(crate) read_guard: [u8; 32],
}

impl SemanticEncodedRead {
    /// Content-free encoding statistics.
    pub const fn stats(&self) -> SemanticReadEncodingStats {
        self.stats
    }

    /// Measures exact bytes through the selected tokenizer port without exposing them.
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

    /// Admits exact read bytes only after their token count fits.
    pub fn admit(
        self,
        counter: &dyn SemanticTokenCounter,
        expected_revision: &SemanticTokenizerRevision,
    ) -> Result<SemanticReadModelPayload, SemanticModelEncodingError> {
        let measurement = self.measure(counter, expected_revision)?;
        Ok(self.into_payload(measurement))
    }

    /// Uses a conservative UTF-8 bound for preflight only. Exact-budget reads
    /// still refuse it; provider-exact callers must count the whole immutable
    /// request before dispatching generation.
    pub fn admit_conservative_utf8(
        self,
        revision: &SemanticTokenizerRevision,
    ) -> Result<SemanticReadModelPayload, SemanticModelEncodingError> {
        let measurement =
            crate::semantic_model::conservative_utf8_measurement(&self.content, revision)?;
        validate_semantic_token_measurement(&self.budget, &measurement, revision)?;
        Ok(self.into_payload(measurement))
    }

    fn into_payload(self, measurement: SemanticTokenMeasurement) -> SemanticReadModelPayload {
        SemanticReadModelPayload {
            content: self.content,
            stats: self.stats,
            measurement,
            observation: self.observation,
            observation_generation: self.observation_generation,
            context: self.context,
            observation_guard: self.observation_guard,
            captured_at: self.captured_at,
            read_guard: self.read_guard,
        }
    }

    pub(crate) fn into_extraction_parts(self) -> SemanticEncodedReadParts {
        SemanticEncodedReadParts {
            content: self.content,
            budget: self.budget,
            stats: self.stats,
            observation: self.observation,
            observation_generation: self.observation_generation,
            context: self.context,
            observation_guard: self.observation_guard,
            captured_at: self.captured_at,
            read_guard: self.read_guard,
        }
    }
}

impl fmt::Debug for SemanticEncodedRead {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticEncodedRead")
            .field("content", &"[redacted]")
            .field("budget", &self.budget)
            .field("stats", &self.stats)
            .field("read_guard", &"[redacted]")
            .finish()
    }
}

/// Token-admitted compact semantic read for the selected model adapter only.
pub struct SemanticReadModelPayload {
    content: String,
    stats: SemanticReadEncodingStats,
    measurement: SemanticTokenMeasurement,
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
    context: ContextJoin,
    observation_guard: [u8; 32],
    captured_at: SemanticCaptureInstant,
    read_guard: [u8; 32],
}

pub(crate) struct SemanticReadDeliveryAuthority {
    measurement: SemanticTokenMeasurement,
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
    context: ContextJoin,
    observation_guard: [u8; 32],
    captured_at: SemanticCaptureInstant,
    items: u16,
    read_guard: [u8; 32],
}

impl SemanticReadDeliveryAuthority {
    pub(crate) const fn token_measurement(&self) -> &SemanticTokenMeasurement {
        &self.measurement
    }

    pub(crate) fn matches_read(&self, read: &SemanticReadResult<'_>) -> bool {
        self.observation == read.observation()
            && self.observation_generation == read.observation_generation()
            && self.context == read.context()
            && self.observation_guard == read.observation_guard()
            && self.captured_at == read.captured_at()
            && self.items == read.stats().items()
            && self.read_guard == read.guard()
    }

    pub(crate) fn commit(self) -> SemanticReadDeliveryReceipt {
        SemanticReadDeliveryReceipt {
            observation: self.observation,
            observation_generation: self.observation_generation,
            context: self.context,
            observation_guard: self.observation_guard,
            captured_at: self.captured_at,
            items: self.items,
            read_guard: self.read_guard,
        }
    }
}

impl SemanticReadModelPayload {
    /// Returns exact compact read input to the already-selected model transport.
    pub fn as_str(&self) -> &str {
        &self.content
    }

    /// Content-free encoding statistics.
    pub const fn stats(&self) -> SemanticReadEncodingStats {
        self.stats
    }

    /// Admitted bounded token measurement and tokenizer revision.
    pub const fn token_measurement(&self) -> &SemanticTokenMeasurement {
        &self.measurement
    }

    pub(crate) fn matches_read(&self, read: &SemanticReadResult<'_>) -> bool {
        self.observation == read.observation()
            && self.observation_generation == read.observation_generation()
            && self.context == read.context()
            && self.observation_guard == read.observation_guard()
            && self.captured_at == read.captured_at()
            && self.stats.items == read.stats().items()
            && self.read_guard == read.guard()
    }

    pub(crate) fn into_provider_parts(
        self,
    ) -> (
        String,
        SemanticReadEncodingStats,
        SemanticReadDeliveryAuthority,
    ) {
        (
            self.content,
            self.stats,
            SemanticReadDeliveryAuthority {
                measurement: self.measurement,
                observation: self.observation,
                observation_generation: self.observation_generation,
                context: self.context,
                observation_guard: self.observation_guard,
                captured_at: self.captured_at,
                items: self.stats.items,
                read_guard: self.read_guard,
            },
        )
    }

    /// Settles transport of this exact token-admitted read.
    ///
    /// Only committed delivery acknowledges this exact read projection.
    ///
    /// The receipt is deliberately not a full semantic-observation
    /// acknowledgement and cannot authorize diffs or progressive scopes.
    pub fn settle_delivery(
        self,
        settlement: SemanticModelDeliverySettlement,
    ) -> Result<SemanticReadDeliveryReceipt, SemanticModelDeliveryError> {
        match settlement {
            SemanticModelDeliverySettlement::Committed => Ok(SemanticReadDeliveryReceipt {
                observation: self.observation,
                observation_generation: self.observation_generation,
                context: self.context,
                observation_guard: self.observation_guard,
                captured_at: self.captured_at,
                items: self.stats.items,
                read_guard: self.read_guard,
            }),
            SemanticModelDeliverySettlement::Refused => Err(SemanticModelDeliveryError::Refused),
            SemanticModelDeliverySettlement::Cancelled => {
                Err(SemanticModelDeliveryError::Cancelled)
            }
        }
    }
}

impl fmt::Debug for SemanticReadModelPayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticReadModelPayload")
            .field("content", &"[redacted]")
            .field("stats", &self.stats)
            .field("measurement", &self.measurement)
            .field("read_guard", &"[redacted]")
            .finish()
    }
}

/// Opaque proof that one exact bounded read reached committed model delivery.
///
/// This receipt is not semantic observation, diff, action, or policy authority.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticReadDeliveryReceipt {
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
    context: ContextJoin,
    observation_guard: [u8; 32],
    captured_at: SemanticCaptureInstant,
    items: u16,
    read_guard: [u8; 32],
}

impl SemanticReadDeliveryReceipt {
    /// Source observation identity.
    pub const fn observation(&self) -> SemanticObservationId {
        self.observation
    }

    /// Source progressive observation generation.
    pub const fn observation_generation(&self) -> SemanticObservationGeneration {
        self.observation_generation
    }

    /// Exact source context/document/cancellation authority.
    pub const fn context(&self) -> ContextJoin {
        self.context
    }

    #[cfg(any(test, feature = "provider-transport"))]
    pub(crate) const fn observation_guard(&self) -> [u8; 32] {
        self.observation_guard
    }

    /// Trusted-shell capture time represented by the delivered read.
    pub const fn captured_at(&self) -> SemanticCaptureInstant {
        self.captured_at
    }

    /// Delivered readable primitive count.
    pub const fn items(&self) -> u16 {
        self.items
    }

    /// Reports whether this receipt was minted for the exact read projection.
    pub fn matches_read(&self, read: &SemanticReadResult<'_>) -> bool {
        self.observation == read.observation()
            && self.observation_generation == read.observation_generation()
            && self.context == read.context()
            && self.observation_guard == read.observation_guard()
            && self.captured_at == read.captured_at()
            && self.items == read.stats().items()
            && self.read_guard == read.guard()
    }

    pub(crate) const fn guard(&self) -> [u8; 32] {
        self.read_guard
    }
}

impl fmt::Debug for SemanticReadDeliveryReceipt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticReadDeliveryReceipt")
            .field("observation", &self.observation)
            .field("observation_generation", &self.observation_generation)
            .field("context", &self.context)
            .field("captured_at", &self.captured_at)
            .field("items", &self.items)
            .field("read_guard", &"[redacted]")
            .finish()
    }
}

/// Encodes one bounded semantic read into deterministic compact `ZREAD3` lines.
pub fn encode_semantic_read(
    read: &SemanticReadResult<'_>,
    budget: SemanticModelEncodingBudget,
) -> Result<SemanticEncodedRead, SemanticModelEncodingError> {
    validate_read(read)?;
    let frames = read_frames(read);
    let cross_document = frames.iter().any(|frame| frame.context() != read.context());
    let mut cohorts = Vec::new();
    if read.has_retained_evidence() {
        for fragment in read.fragments() {
            let source = fragment.provenance();
            if !cohorts.iter().any(|prior| same_capture(*prior, source)) {
                cohorts.push(source);
            }
        }
    }
    let capacity = usize::try_from(budget.max_bytes().min(8 * 1024))
        .map_err(|_| SemanticModelEncodingError::Budget)?;
    let mut output = BoundedModelBuffer::new(capacity, budget.max_bytes());
    checked_write(
        &mut output,
        format_args!(
            "ZREAD{} content=untrusted observation_generation={} captured_at_ms={} items={} omitted={} omissions=",
            SEMANTIC_READ_MODEL_SCHEMA_VERSION,
            read.observation_generation().get(),
            read.captured_at().millis(),
            read.stats().items(),
            read.stats().omitted_items(),
        ),
    )?;
    write_omissions(&mut output, read)?;
    if read.has_retained_evidence() {
        checked_write(
            &mut output,
            format_args!(" retained_history=true refs=historical_read_only provenance=cohorts_v1"),
        )?;
    }
    if read.includes_image_sources() {
        checked_write(&mut output, format_args!(" image_sources=true"))?;
    }
    if read.includes_link_destinations() {
        checked_write(&mut output, format_args!(" link_destinations=true"))?;
    }
    if read.source_roles() != crate::SemanticReadRoleSelection::ALL {
        checked_write(&mut output, format_args!(" selected_roles="))?;
        for (index, role) in read.source_roles().roles().enumerate() {
            if index > 0 {
                checked_write(&mut output, format_args!(","))?;
            }
            checked_write(&mut output, format_args!("{}", role_label(role)))?;
        }
    }
    checked_write(&mut output, format_args!("\n"))?;
    // Explicit columns and defaults remove repeated metadata, never evidence.
    // Nondefault provenance is written on the exact row or its capture cohort;
    // quoted values cannot introduce a row, declaration or override.
    checked_write(
        &mut output,
        format_args!("C columns=id,ref,field,role,value"),
    )?;
    if read.has_retained_evidence() {
        checked_write(
            &mut output,
            format_args!(
                " default_p={}",
                if cohorts.is_empty() { "none" } else { "p1" }
            ),
        )?;
    } else {
        checked_write(
            &mut output,
            format_args!(
                " default_f={}",
                if frames.is_empty() { "none" } else { "f1" }
            ),
        )?;
    }
    checked_write(
        &mut output,
        format_args!(" default_source=page default_sensitivity=public\n"),
    )?;

    for (index, frame) in frames.iter().enumerate() {
        let source = read
            .fragments()
            .iter()
            .find(|fragment| fragment.provenance().frame() == *frame)
            .ok_or(SemanticModelEncodingError::Invariant)?
            .provenance();
        checked_write(&mut output, format_args!("F f{} origin=", index + 1))?;
        write_quoted(&mut output, frame.origin().as_url().as_str())?;
        checked_write(
            &mut output,
            format_args!(" trust={}", frame_trust_label(frame.trust())),
        )?;
        if cross_document {
            checked_write(
                &mut output,
                format_args!(
                    " document_epoch={}",
                    frame.context().navigation_epoch().get()
                ),
            )?;
        }
        if !read.has_retained_evidence() {
            checked_write(
                &mut output,
                format_args!(
                    " invocation={} snapshot={}",
                    source.invocation().get(),
                    source.snapshot().get()
                ),
            )?;
        }
        checked_write(&mut output, format_args!("\n"))?;
    }

    for (index, source) in cohorts.iter().enumerate() {
        let frame = frames
            .iter()
            .position(|frame| *frame == source.frame())
            .ok_or(SemanticModelEncodingError::Invariant)?
            + 1;
        checked_write(&mut output, format_args!(
            "P p{} f=f{} historical_observation={} generation={} captured_at_ms={} invocation={} snapshot={}\n",
            index + 1, frame, source.observation().get(), source.observation_generation().get(),
            source.captured_at().millis(), source.invocation().get(), source.snapshot().get(),
        ))?;
    }

    for fragment in read.fragments() {
        let frame = frames
            .iter()
            .position(|candidate| *candidate == fragment.provenance().frame())
            .map(|index| index + 1)
            .ok_or(SemanticModelEncodingError::Invariant)?;
        checked_write(
            &mut output,
            format_args!(
                "R @r{} {} {} {} ",
                fragment.id().get(),
                fragment.provenance().reference().model_token(),
                field_label(fragment.field()),
                role_label(fragment.role()),
            ),
        )?;
        match fragment.content() {
            SemanticReadContent::Text(text) => write_quoted(&mut output, text.as_str())?,
            SemanticReadContent::ValuePreview(preview) => {
                write_value_preview(&mut output, preview)?;
            }
            SemanticReadContent::Boolean(value) => checked_write(
                &mut output,
                format_args!("{}", if value { "true" } else { "false" }),
            )?,
            SemanticReadContent::Ordinal(value) => {
                checked_write(&mut output, format_args!("{value}"))?;
            }
        }
        if frame != 1 && !read.has_retained_evidence() {
            checked_write(&mut output, format_args!(" f=f{frame}"))?;
        }
        if read.has_retained_evidence() {
            let cohort = cohorts
                .iter()
                .position(|source| same_capture(*source, fragment.provenance()))
                .ok_or(SemanticModelEncodingError::Invariant)?
                + 1;
            if cohort != 1 {
                checked_write(&mut output, format_args!(" p=p{cohort}"))?;
            }
        }
        if fragment.provenance().trust() != crate::SemanticTrust::UntrustedPage {
            checked_write(
                &mut output,
                format_args!(" source={}", source_label(fragment.provenance().trust())),
            )?;
        }
        if fragment.provenance().sensitivity() != SemanticSensitivity::Public {
            checked_write(
                &mut output,
                format_args!(
                    " sensitivity={}",
                    sensitivity_label(fragment.provenance().sensitivity())
                ),
            )?;
        }
        checked_write(&mut output, format_args!("\n"))?;
    }

    let content = output.finish();
    let lines = content.bytes().filter(|byte| *byte == b'\n').count();
    let stats = SemanticReadEncodingStats {
        bytes: u32::try_from(content.len()).map_err(|_| SemanticModelEncodingError::Budget)?,
        lines: u16::try_from(lines).map_err(|_| SemanticModelEncodingError::Invariant)?,
        frames: u8::try_from(frames.len()).map_err(|_| SemanticModelEncodingError::Invariant)?,
        items: read.stats().items(),
        sensitive_items: read.stats().sensitive_items(),
        omitted_items: read.stats().omitted_items(),
    };
    Ok(SemanticEncodedRead {
        content,
        budget,
        stats,
        observation: read.observation(),
        observation_generation: read.observation_generation(),
        context: read.context(),
        observation_guard: read.observation_guard(),
        captured_at: read.captured_at(),
        read_guard: read.guard(),
    })
}

// A cohort shares capture coordinates only. Reference, source class and
// sensitivity remain on each row, and values are never merged or deduplicated.
fn same_capture(a: SemanticReadProvenance<'_>, b: SemanticReadProvenance<'_>) -> bool {
    a.frame() == b.frame()
        && a.observation() == b.observation()
        && a.observation_generation() == b.observation_generation()
        && a.captured_at() == b.captured_at()
        && a.invocation() == b.invocation()
        && a.snapshot() == b.snapshot()
}

fn validate_read(read: &SemanticReadResult<'_>) -> Result<(), SemanticModelEncodingError> {
    if usize::from(read.stats().items()) != read.fragments().len() {
        return Err(SemanticModelEncodingError::Invariant);
    }
    for (index, fragment) in read.fragments().iter().enumerate() {
        let expected =
            u16::try_from(index + 1).map_err(|_| SemanticModelEncodingError::Invariant)?;
        let provenance = fragment.provenance();
        if fragment.id().get() != expected
            || ((provenance.context() != read.context()
                || provenance.observation() != read.observation()
                || provenance.observation_generation() != read.observation_generation()
                || provenance.captured_at() != read.captured_at())
                && read.source_acknowledgement(provenance).is_none())
            || provenance.sensitivity() == SemanticSensitivity::Secret
            || !read.source_roles().contains(fragment.role())
            || !field_matches_content(fragment.field(), fragment.content())
        {
            return Err(SemanticModelEncodingError::Invariant);
        }
    }
    Ok(())
}

fn read_frames<'a>(read: &'a SemanticReadResult<'a>) -> Vec<&'a SemanticFrameJoin> {
    let mut frames = Vec::new();
    for fragment in read.fragments() {
        let frame = fragment.provenance().frame();
        if !frames.contains(&frame) {
            frames.push(frame);
        }
    }
    frames
}

fn write_omissions(
    output: &mut BoundedModelBuffer,
    read: &SemanticReadResult<'_>,
) -> Result<(), SemanticModelEncodingError> {
    let mut wrote = false;
    for (omission, label) in [
        (SemanticReadOmission::SourceIncomplete, "source_incomplete"),
        (SemanticReadOmission::SensitivityLimit, "sensitivity_limit"),
        (SemanticReadOmission::Secret, "secret"),
        (SemanticReadOmission::ItemLimit, "item_limit"),
        (SemanticReadOmission::ByteLimit, "byte_limit"),
        (SemanticReadOmission::RoleSelection, "role_selection"),
        (
            SemanticReadOmission::ValuePreviewLimit,
            "value_preview_limit",
        ),
    ] {
        if read.omissions().contains(omission) {
            if wrote {
                checked_write(output, format_args!(","))?;
            }
            checked_write(output, format_args!("{label}"))?;
            wrote = true;
        }
    }
    if !wrote {
        checked_write(output, format_args!("none"))?;
    }
    Ok(())
}

const fn field_label(field: SemanticReadField) -> &'static str {
    match field {
        SemanticReadField::AccessibleName => "name",
        SemanticReadField::VisibleText => "text",
        SemanticReadField::TextValue => "text_value",
        SemanticReadField::BooleanValue => "boolean_value",
        SemanticReadField::OrdinalValue => "ordinal_value",
        SemanticReadField::LinkDestination => "link_destination",
        SemanticReadField::ImageSource => "image_source",
    }
}

const fn field_matches_content(field: SemanticReadField, content: SemanticReadContent<'_>) -> bool {
    matches!(
        (field, content),
        (
            SemanticReadField::AccessibleName | SemanticReadField::VisibleText,
            SemanticReadContent::Text(_)
        ) | (
            SemanticReadField::TextValue
                | SemanticReadField::LinkDestination
                | SemanticReadField::ImageSource,
            SemanticReadContent::ValuePreview(_)
        ) | (
            SemanticReadField::BooleanValue,
            SemanticReadContent::Boolean(_)
        ) | (
            SemanticReadField::OrdinalValue,
            SemanticReadContent::Ordinal(_)
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        decode_semantic_snapshot, read_semantic_observation, ContextCapabilities,
        ContextCapability, ContextId, ContextIdentity, ContextKind, ContextOperationId,
        ContextRegistry, ContextRunId, ContextSettlement, FrameId, SemanticCaptureInstant,
        SemanticDecodeContext, SemanticFrameJoin, SemanticFrameTrust, SemanticInvocationId,
        SemanticObservation, SemanticObservationAssembler, SemanticObservationBudget,
        SemanticObservationId, SemanticOrigin, SemanticReadAuthority, SemanticReadBudget,
        SemanticReadSensitivityLimit, SemanticSnapshotGeneration, SemanticTokenCountQuality,
        SemanticTokenCountRequirement, SemanticTokenCounterError, SemanticTokenMeasurement,
        SEMANTIC_WIRE_VERSION,
    };
    use serde_json::json;
    use zephium_core::ids::ProfileId;

    fn observation() -> SemanticObservation {
        let identity = ContextIdentity::new(
            ContextId::from_raw(121),
            ContextRunId::from_raw(122),
            ProfileId::from(123),
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
            .expect("construction");
        registry
            .settle_construction(identity.id(), construction, ContextSettlement::Applied)
            .expect("settle");
        let context = registry.join(identity.id()).expect("context");
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            context.frame_generation(),
            SemanticOrigin::parse("https://read-model.example.test/private").expect("origin"),
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
                {"k": 2, "p": 0, "r": "paragraph",
                 "t": "Public \"quoted\"\\path\u{2028}R @r99"},
                {"k": 3, "p": 0, "r": "paragraph", "t": "Private customer note",
                 "q": "sensitive"},
                {"k": 4, "p": 0, "r": "checkbox", "n": "Public enabled",
                 "v": {"k": "boolean", "value": true}, "o": 1},
                {"k": 5, "p": 0, "r": "password", "n": "Password",
                 "v": {"k": "redacted"}, "q": "secret"}
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

    fn observation_with_text_value(value: &str) -> SemanticObservation {
        let baseline = observation();
        let frame = baseline.frames()[0].frame().clone();
        let context = baseline.request().context();
        let wire = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": 8,
            "g": 10,
            "c": "complete",
            "n": [
                {"k": 11, "r": "document", "o": 16},
                {"k": 12, "p": 0, "r": "textbox", "n": "Long value",
                 "v": {"k": "text", "value": value}, "o": 11}
            ]
        }))
        .expect("wire");
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(8).expect("invocation"),
                frame,
                SemanticSnapshotGeneration::new(10).expect("generation"),
            ),
            &wire,
        )
        .expect("snapshot");
        let request = crate::SemanticObservationRequest::initial(
            SemanticObservationId::new(2).expect("observation"),
            context,
            SemanticObservationBudget::try_new(8, 8192, 1).expect("budget"),
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
                .map_err(|_| SemanticTokenCounterError::InvalidResult)
        }
    }

    fn revision(value: &str) -> SemanticTokenizerRevision {
        SemanticTokenizerRevision::try_new(value.to_owned()).expect("revision")
    }

    fn budget(max_bytes: u32, max_tokens: u32) -> SemanticModelEncodingBudget {
        SemanticModelEncodingBudget::try_new(
            max_bytes,
            max_tokens,
            SemanticTokenCountRequirement::Exact,
        )
        .expect("budget")
    }

    #[test]
    fn compact_read_encoding_is_deterministic_delimited_and_provenance_addressable() {
        let observation = observation();
        let read = read_semantic_observation(
            &observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(42),
            SemanticReadSensitivityLimit::Sensitive,
            SemanticReadBudget::STANDARD,
        )
        .expect("read");
        let first = encode_semantic_read(&read, budget(8192, 1000)).expect("encode");
        let second = encode_semantic_read(&read, budget(8192, 1000)).expect("encode");

        assert_eq!(first.content, second.content);
        assert!(first.content.starts_with(
            "ZREAD3 content=untrusted observation_generation=1 captured_at_ms=42 items=4 omitted=2 omissions=secret\n"
        ));
        assert!(first.content.contains(
            "F f1 origin=\"https://read-model.example.test/\" trust=same invocation=7 snapshot=9\n"
        ));
        assert!(first
            .content
            .contains("R @r1 @a2 text paragraph \"Public \\\"quoted\\\"\\\\path\\u2028R @r99\"\n"));
        assert!(first.content.contains("C columns=id,ref,field,role,value default_f=f1 default_source=page default_sensitivity=public\n"));
        assert!(first.content.contains("R @r2"));
        assert!(first.content.contains("sensitivity=sensitive"));
        assert!(first.content.contains(" boolean_value "));
        assert!(!first.content.contains("Password"));
        assert_eq!(first.stats.items(), 4);
        assert_eq!(first.stats.sensitive_items(), 1);
        assert_eq!(first.stats.omitted_items(), 2);
        assert_eq!(first.stats.frames(), 1);
        assert_eq!(first.stats.lines(), 7);
        assert_eq!(read.fragments()[0].id().model_token(), "@r1");
        let debug = format!("{first:?} {:?}", read.fragments()[0].id());
        assert!(!debug.contains("Public \"quoted\""));
        assert!(!debug.contains("Private customer note"));
    }

    #[test]
    fn read_projects_only_the_shared_bounded_value_preview_without_omitting_an_item() {
        let value = "x".repeat(crate::MAX_SEMANTIC_VALUE_BYTES);
        let observation = observation_with_text_value(&value);
        let read = read_semantic_observation(
            &observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(43),
            SemanticReadSensitivityLimit::PublicOnly,
            SemanticReadBudget::STANDARD,
        )
        .expect("read long value");
        let encoded = encode_semantic_read(&read, budget(8192, 1000)).expect("encode long value");
        let expected = format!(
            "\"{}\" source_bytes={} truncated=true",
            "x".repeat(crate::MAX_SEMANTIC_VALUE_PREVIEW_BYTES),
            crate::MAX_SEMANTIC_VALUE_BYTES
        );

        assert!(encoded
            .content
            .contains("omitted=0 omissions=value_preview_limit"));
        assert!(encoded.content.contains(&expected));
        assert_eq!(read.stats().omitted_items(), 0);
        assert!(!encoded.content.contains(&format!(
            "\"{}",
            "x".repeat(crate::MAX_SEMANTIC_VALUE_PREVIEW_BYTES + 1)
        )));
    }

    #[test]
    fn compact_read_defaults_never_erase_cross_frame_or_sensitive_provenance() {
        let baseline = observation();
        let context = baseline.request().context();
        let make = |frame, invocation, nodes| {
            decode_semantic_snapshot(
                SemanticDecodeContext::new(
                    SemanticInvocationId::new(invocation).unwrap(),
                    frame,
                    SemanticSnapshotGeneration::new(10).unwrap(),
                ),
                &serde_json::to_vec(&json!({"v":1,"i":invocation,"g":10,"c":"complete","n":nodes}))
                    .unwrap(),
            )
            .unwrap()
        };
        let main = make(
            baseline.frames()[0].frame().clone(),
            8,
            json!([
                {"k":1,"r":"document"}, {"k":2,"p":0,"r":"paragraph","t":"Parent evidence"},
                {"k":3,"p":0,"r":"frame_boundary"}
            ]),
        );
        let child_frame = SemanticFrameJoin::try_new(
            context,
            FrameId::new(2).unwrap(),
            context.frame_generation(),
            SemanticOrigin::parse("https://child-read.example.test/").unwrap(),
            SemanticFrameTrust::CrossOriginIsolated,
        )
        .unwrap();
        let child = make(
            child_frame.clone(),
            9,
            json!([
                {"k":1,"r":"paragraph","t":"Child evidence","q":"sensitive"}
            ]),
        );
        let mut assembler = SemanticObservationAssembler::new(
            crate::SemanticObservationRequest::initial(
                SemanticObservationId::new(2).unwrap(),
                context,
                SemanticObservationBudget::try_new(8, 8192, 2).unwrap(),
            ),
            main,
        )
        .unwrap();
        assembler
            .attach_frame(
                FrameId::MAIN,
                crate::SemanticReferenceId::new(3).unwrap(),
                child,
            )
            .unwrap();
        let observation = assembler.finish().unwrap();
        let read = read_semantic_observation(
            &observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(43),
            SemanticReadSensitivityLimit::Sensitive,
            SemanticReadBudget::STANDARD,
        )
        .unwrap();
        let encoded = encode_semantic_read(&read, budget(8192, 8192)).unwrap();
        assert_eq!(encoded.stats().frames(), 2);
        assert_eq!(read.fragments()[1].provenance().frame(), &child_frame);
        assert!(encoded
            .content
            .contains("F f2 origin=\"https://child-read.example.test/\" trust=cross_isolated"));
        assert!(encoded
            .content
            .contains("R @r1 @a2 text paragraph \"Parent evidence\"\n"));
        assert!(encoded
            .content
            .contains("R @r2 @a4 text paragraph \"Child evidence\" f=f2 sensitivity=sensitive\n"));
    }

    #[test]
    fn exact_token_admission_and_committed_delivery_gate_read_bytes() {
        let observation = observation();
        let read = read_semantic_observation(
            &observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(42),
            SemanticReadSensitivityLimit::Sensitive,
            SemanticReadBudget::STANDARD,
        )
        .expect("read");
        let selected = revision("model-tokenizer-v1");
        assert_eq!(
            encode_semantic_read(&read, budget(8192, 8192))
                .unwrap()
                .admit_conservative_utf8(&selected)
                .unwrap_err(),
            SemanticModelEncodingError::TokenQuality
        );
        let preflight = encode_semantic_read(
            &read,
            SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE,
        )
        .unwrap()
        .admit_conservative_utf8(&selected)
        .unwrap();
        assert_eq!(
            preflight.token_measurement().tokens() as usize,
            preflight.as_str().len()
        );
        let counter = FixedCounter {
            revision: selected.clone(),
            tokens: 50,
            quality: SemanticTokenCountQuality::ExactLocal,
        };
        let payload = encode_semantic_read(&read, budget(8192, 100))
            .expect("encode")
            .admit(&counter, &selected)
            .expect("admit");
        assert_eq!(payload.token_measurement().tokens(), 50);
        assert!(payload.as_str().starts_with("ZREAD3 content=untrusted"));
        let debug = format!("{payload:?}");
        assert!(!debug.contains("Private customer note"));
        let receipt = payload
            .settle_delivery(SemanticModelDeliverySettlement::Committed)
            .expect("commit");
        assert!(receipt.matches_read(&read));
        assert_eq!(receipt.observation().get(), 1);
        assert_eq!(receipt.observation_generation().get(), 1);
        assert_eq!(receipt.captured_at().millis(), 42);
        assert_eq!(receipt.items(), 4);
        assert!(!format!("{receipt:?}").contains("Private customer note"));
        let narrower = read_semantic_observation(
            &observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(42),
            SemanticReadSensitivityLimit::PublicOnly,
            SemanticReadBudget::STANDARD,
        )
        .expect("narrower read");
        assert!(!receipt.matches_read(&narrower));
        let later = read_semantic_observation(
            &observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(43),
            SemanticReadSensitivityLimit::Sensitive,
            SemanticReadBudget::STANDARD,
        )
        .expect("later read");
        assert!(!receipt.matches_read(&later));

        let refused = encode_semantic_read(&read, budget(8192, 100))
            .expect("encode")
            .admit(&counter, &selected)
            .expect("admit");
        assert_eq!(
            refused.settle_delivery(SemanticModelDeliverySettlement::Refused),
            Err(SemanticModelDeliveryError::Refused)
        );
    }

    #[test]
    fn read_encoding_enforces_byte_token_quality_revision_and_count_limits() {
        let observation = observation();
        let read = read_semantic_observation(
            &observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(42),
            SemanticReadSensitivityLimit::Sensitive,
            SemanticReadBudget::STANDARD,
        )
        .expect("read");
        assert_eq!(
            encode_semantic_read(&read, budget(32, 100)).expect_err("byte limit"),
            SemanticModelEncodingError::OutputLimit
        );

        let selected = revision("model-tokenizer-v1");
        let estimated = FixedCounter {
            revision: selected.clone(),
            tokens: 50,
            quality: SemanticTokenCountQuality::ProviderEstimate,
        };
        assert_eq!(
            encode_semantic_read(&read, budget(8192, 100))
                .expect("encode")
                .admit(&estimated, &selected)
                .expect_err("quality"),
            SemanticModelEncodingError::TokenQuality
        );
        let exact = FixedCounter {
            revision: revision("other-tokenizer-v1"),
            tokens: 50,
            quality: SemanticTokenCountQuality::ExactLocal,
        };
        assert_eq!(
            encode_semantic_read(&read, budget(8192, 100))
                .expect("encode")
                .admit(&exact, &selected)
                .expect_err("revision"),
            SemanticModelEncodingError::TokenizerRevisionMismatch
        );
        let oversized = FixedCounter {
            revision: selected.clone(),
            tokens: 101,
            quality: SemanticTokenCountQuality::ExactLocal,
        };
        assert_eq!(
            encode_semantic_read(&read, budget(8192, 100))
                .expect("encode")
                .admit(&oversized, &selected)
                .expect_err("tokens"),
            SemanticModelEncodingError::TokenLimit
        );
    }
}
