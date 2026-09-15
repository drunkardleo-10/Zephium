//! Run-local public evidence; owns safe primitives, never snapshots or actions.
use super::*;
use std::sync::Arc;

const MAX_CAPTURES: usize = 8;

fn merge_omitted_stats(target: &mut SemanticReadStats, source: SemanticReadStats) {
    target.omitted_items = target.omitted_items.saturating_add(source.omitted_items);
    target.withheld_sensitive_nodes = target
        .withheld_sensitive_nodes
        .saturating_add(source.withheld_sensitive_nodes);
    target.secret_nodes = target.secret_nodes.saturating_add(source.secret_nodes);
    target.redacted_values = target
        .redacted_values
        .saturating_add(source.redacted_values);
    target.incomplete_frames = target
        .incomplete_frames
        .saturating_add(source.incomplete_frames);
}

enum Content {
    Text(SemanticText),
    Preview {
        text: String,
        source_bytes: usize,
        truncated: bool,
    },
    Boolean(bool),
    Ordinal(u16),
}

impl Content {
    fn borrow(&self) -> SemanticReadContent<'_> {
        match self {
            Self::Text(text) => SemanticReadContent::Text(text),
            Self::Preview {
                text,
                source_bytes,
                truncated,
            } => SemanticReadContent::ValuePreview(SemanticValuePreview::retained(
                text,
                *source_bytes,
                *truncated,
            )),
            Self::Boolean(value) => SemanticReadContent::Boolean(*value),
            Self::Ordinal(value) => SemanticReadContent::Ordinal(*value),
        }
    }
    fn copy(content: SemanticReadContent<'_>) -> Self {
        match content {
            SemanticReadContent::Text(text) => Self::Text(text.clone()),
            SemanticReadContent::ValuePreview(value) => Self::Preview {
                text: value.text().to_owned(),
                source_bytes: value.source_bytes(),
                truncated: value.truncated(),
            },
            SemanticReadContent::Boolean(value) => Self::Boolean(value),
            SemanticReadContent::Ordinal(value) => Self::Ordinal(value),
        }
    }
}

struct Fragment {
    node_key: crate::semantic::SemanticNodeKey,
    field: SemanticReadField,
    role: SemanticRole,
    content: Content,
    frame: Arc<SemanticFrameJoin>,
    invocation: SemanticInvocationId,
    snapshot: SemanticSnapshotGeneration,
    reference: SemanticReferenceId,
    trust: SemanticTrust,
}

struct Capture {
    acknowledgement: SemanticObservationAcknowledgement,
    captured_at: SemanticCaptureInstant,
    fragments: Vec<Fragment>,
    omissions: SemanticReadOmissions,
    stats: SemanticReadStats,
}

impl Capture {
    fn cost_bucket(&self) -> u32 {
        u32::from(self.stats.items)
            .div_ceil(16)
            .max(self.stats.content_bytes.div_ceil(4096))
    }

    fn remove_duplicates(&mut self, preferred: &Self) {
        self.fragments.retain(|fragment| {
            let duplicate = preferred.fragments.iter().any(|other| {
                fragment.node_key == other.node_key
                    && fragment.field == other.field
                    && fragment.role == other.role
                    && fragment.frame == other.frame
                    && fragment.trust == other.trust
                    && fragment.content.borrow() == other.content.borrow()
            });
            if duplicate {
                self.stats.items -= 1;
                self.stats.public_items -= 1;
                self.stats.content_bytes -= fragment.content.borrow().retained_bytes();
            }
            !duplicate
        });
    }
}

/// Bounded safe evidence from acknowledged observations. Crossing documents
/// requires an explicit committed navigation receipt; retained references
/// remain historical and can only contribute to terminal extraction.
///
/// Holds at most eight captures and STANDARD's 128 fragments / 32 KiB content.
/// Under pressure, large captures yield to focused reads; age breaks ties.
/// Omissions remain explicit. Empty reads never displace useful evidence. This object has no
/// browser operation, codec, persistence, or provider-disclosure authority.
#[derive(Default)]
pub struct SemanticRetainedReadEvidence {
    link_destinations: Option<bool>,
    context: Option<ContextJoin>,
    roles: Option<SemanticReadRoleSelection>,
    captures: Vec<Capture>,
    dropped_stats: SemanticReadStats,
    dropped_omissions: u8,
    last_seen: Option<SemanticObservationAcknowledgement>,
    last_empty: Option<SemanticReadStats>,
}

impl SemanticRetainedReadEvidence {
    /// Releases all private data at a navigation, task or account boundary.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Advances the read owner along one accounted native navigation. This
    /// does not make old references actionable or authorize model disclosure;
    /// extraction policy independently rejoins every historical source to its
    /// original committed taint and the same lease's navigation receipts.
    pub fn advance_after_navigation(
        &mut self,
        receipt: crate::AgentNavigationReceipt,
    ) -> Result<(), SemanticReadError> {
        if self.context != Some(receipt.source())
            || receipt.settlement() != crate::AgentNavigationSettlement::Committed
            || !crate::agent_policy::is_document_successor(
                receipt.source(),
                receipt.operation().context(),
            )
        {
            return Err(SemanticReadError::AuthorityMismatch);
        }
        self.context = Some(receipt.operation().context());
        Ok(())
    }

    /// Safe primitive bytes retained, excluding bounded structural metadata.
    pub fn retained_bytes(&self) -> u32 {
        self.captures
            .iter()
            .map(|capture| capture.stats.content_bytes)
            .sum()
    }

    /// Public source fragment count retained across captures.
    pub fn retained_items(&self) -> u16 {
        self.captures
            .iter()
            .map(|capture| capture.stats.items)
            .sum()
    }

    /// Admits only an exact acknowledged public read. Repeated admission is
    /// idempotent; scope/account/document substitution refuses before mutation.
    pub fn retain(
        &mut self,
        read: &SemanticReadResult<'_>,
        acknowledgement: &SemanticObservationAcknowledgement,
    ) -> Result<(), SemanticReadError> {
        if !read.matches_acknowledgement(acknowledgement)
            || read.has_retained_evidence()
            || self
                .context
                .is_some_and(|context| context != read.context())
            || self.roles.is_some_and(|roles| roles != read.source_roles())
            || self
                .link_destinations
                .is_some_and(|links| links != read.includes_link_destinations())
            || read
                .fragments()
                .iter()
                .any(|fragment| fragment.provenance().sensitivity() != SemanticSensitivity::Public)
            || read.stats.items > SemanticReadBudget::STANDARD.max_items()
            || read.stats.content_bytes > SemanticReadBudget::STANDARD.max_bytes()
        {
            return Err(SemanticReadError::AuthorityMismatch);
        }
        if self.last_seen.as_ref() == Some(acknowledgement)
            || self
                .captures
                .iter()
                .any(|capture| capture.acknowledgement == *acknowledgement)
        {
            return Ok(());
        }
        self.context = Some(read.context());
        self.roles = Some(read.source_roles());
        self.link_destinations = Some(read.includes_link_destinations());
        if let Some(stats) = self.last_empty.take() {
            merge_omitted_stats(&mut self.dropped_stats, stats);
        }
        self.last_seen = Some(acknowledgement.clone());
        self.dropped_omissions |= read.omissions.bits();
        if read.fragments.is_empty() {
            self.last_empty = Some(read.stats);
            return Ok(());
        }
        let mut frames: Vec<Arc<SemanticFrameJoin>> = Vec::new();
        let mut capture = Capture {
            acknowledgement: acknowledgement.clone(),
            captured_at: read.captured_at(),
            fragments: read
                .fragments()
                .iter()
                .map(|fragment| {
                    let source = fragment.provenance();
                    let frame = match frames.iter().find(|frame| frame.as_ref() == source.frame()) {
                        Some(frame) => frame.clone(),
                        None => {
                            let frame = Arc::new(source.frame().clone());
                            frames.push(frame.clone());
                            frame
                        }
                    };
                    Fragment {
                        node_key: source.node_key,
                        field: fragment.field(),
                        role: fragment.role(),
                        content: Content::copy(fragment.content()),
                        frame,
                        invocation: source.invocation(),
                        snapshot: source.snapshot(),
                        reference: source.reference(),
                        trust: source.trust(),
                    }
                })
                .collect(),
            omissions: read.omissions(),
            stats: read.stats(),
        };
        for prior in &mut self.captures {
            // Keep duplicates in the capture least likely to be evicted; newest wins ties.
            if prior.cost_bucket() < capture.cost_bucket() {
                capture.remove_duplicates(prior);
            } else {
                prior.remove_duplicates(&capture);
            }
        }
        self.captures.retain(|prior| {
            if prior.fragments.is_empty() {
                merge_omitted_stats(&mut self.dropped_stats, prior.stats);
                self.dropped_omissions |= prior.omissions.bits();
                false
            } else {
                true
            }
        });
        if capture.fragments.is_empty() {
            merge_omitted_stats(&mut self.dropped_stats, capture.stats);
            self.dropped_omissions |= capture.omissions.bits();
        } else {
            self.captures.push(capture);
        }
        while self.captures.len() > MAX_CAPTURES
            || self.retained_items() > SemanticReadBudget::STANDARD.max_items()
            || self.retained_bytes() > SemanticReadBudget::STANDARD.max_bytes()
        {
            let byte_pressure = self.retained_bytes() > SemanticReadBudget::STANDARD.max_bytes();
            let item_pressure = self.retained_items() > SemanticReadBudget::STANDARD.max_items();
            let index = if byte_pressure || item_pressure {
                self.captures
                    .iter()
                    .enumerate()
                    .max_by_key(|(index, capture)| {
                        (capture.cost_bucket(), std::cmp::Reverse(*index))
                    })
                    .map(|(index, _)| index)
                    .expect("nonempty over-budget captures")
            } else {
                0
            };
            let removed = self.captures.remove(index);
            merge_omitted_stats(&mut self.dropped_stats, removed.stats);
            self.dropped_stats.omitted_items = self
                .dropped_stats
                .omitted_items
                .saturating_add(removed.stats.items);
            self.dropped_omissions |= SemanticReadOmission::ItemLimit.bit();
            if byte_pressure {
                self.dropped_omissions |= SemanticReadOmission::ByteLimit.bit();
            }
        }
        Ok(())
    }

    /// Builds a terminal mapping read under the *current* acknowledged baseline.
    /// Historical sources preserve capture coordinates and are separately joined
    /// to committed policy taint. Result-local @r identities are reassigned;
    /// historical @a identities remain non-actionable provenance only.
    pub fn merge_for_extraction<'a>(
        &'a self,
        mut current: SemanticReadResult<'a>,
    ) -> Result<SemanticReadResult<'a>, SemanticReadError> {
        if current.has_retained_evidence()
            || current.subtree.is_some()
            || self
                .context
                .is_some_and(|context| context != current.context())
            || self
                .roles
                .is_some_and(|roles| roles != current.source_roles())
            || self
                .link_destinations
                .is_some_and(|links| links != current.includes_link_destinations())
        {
            return Err(SemanticReadError::AuthorityMismatch);
        }
        current.omissions.0 |= self.dropped_omissions;
        merge_omitted_stats(&mut current.stats, self.dropped_stats);
        if self
            .last_seen
            .as_ref()
            .is_some_and(|ack| !current.matches_acknowledgement(ack))
        {
            if let Some(stats) = self.last_empty {
                merge_omitted_stats(&mut current.stats, stats);
            }
        }
        // A broad current snapshot must not crowd every earlier detail out of
        // terminal evidence. Reserve at most half for independently cited history.
        let history = self
            .captures
            .iter()
            .filter(|capture| !current.matches_acknowledgement(&capture.acknowledgement));
        let (history_items, history_bytes) =
            history.fold((0_u16, 0_u32), |(items, bytes), capture| {
                (
                    items + capture.stats.items,
                    bytes + capture.stats.content_bytes,
                )
            });
        let budget = SemanticReadBudget::STANDARD;
        let item_limit = budget.max_items() - history_items.min(budget.max_items() / 2);
        let byte_limit = budget.max_bytes() - history_bytes.min(budget.max_bytes() / 2);
        while current.stats.items > item_limit || current.stats.content_bytes > byte_limit {
            current
                .omissions
                .insert(if current.stats.items > item_limit {
                    SemanticReadOmission::ItemLimit
                } else {
                    SemanticReadOmission::ByteLimit
                });
            let removed = current.fragments.pop().expect("nonempty over-budget read");
            current.stats.items -= 1;
            current.stats.content_bytes -= removed.content.retained_bytes();
            match removed.provenance.sensitivity {
                SemanticSensitivity::Public => current.stats.public_items -= 1,
                SemanticSensitivity::Sensitive => current.stats.sensitive_items -= 1,
                SemanticSensitivity::Secret => return Err(SemanticReadError::AuthorityMismatch),
            }
            current.stats.omitted_items = current.stats.omitted_items.saturating_add(1);
        }
        for capture in self.captures.iter().rev() {
            if current.matches_acknowledgement(&capture.acknowledgement) {
                continue;
            }
            current.historical.push(capture.acknowledgement.clone());
            current.omissions.0 |= capture.omissions.0;
            merge_omitted_stats(&mut current.stats, capture.stats);
            for source in &capture.fragments {
                let content = source.content.borrow();
                let bytes = content.retained_bytes();
                if current.stats.items >= SemanticReadBudget::STANDARD.max_items()
                    || current.stats.content_bytes + bytes
                        > SemanticReadBudget::STANDARD.max_bytes()
                {
                    current.omissions.insert(
                        if current.stats.items >= SemanticReadBudget::STANDARD.max_items() {
                            SemanticReadOmission::ItemLimit
                        } else {
                            SemanticReadOmission::ByteLimit
                        },
                    );
                    current.stats.omitted_items = current.stats.omitted_items.saturating_add(1);
                    continue;
                }
                current.stats.items += 1;
                current.stats.public_items += 1;
                current.stats.content_bytes += bytes;
                current.fragments.push(SemanticReadFragment {
                    id: SemanticReadFragmentId::new(current.stats.items)
                        .expect("bounded nonzero fragment"),
                    field: source.field,
                    role: source.role,
                    content,
                    provenance: SemanticReadProvenance {
                        node_key: source.node_key,
                        observation: capture.acknowledgement.observation(),
                        observation_generation: capture.acknowledgement.generation(),
                        captured_at: capture.captured_at,
                        frame: &source.frame,
                        invocation: source.invocation,
                        snapshot: source.snapshot,
                        reference: source.reference,
                        sensitivity: SemanticSensitivity::Public,
                        trust: source.trust,
                    },
                });
            }
        }
        current.guard = read_guard(
            &current.observation_fingerprint,
            current.captured_at,
            &current.fragments,
            current.omissions,
            current.stats,
            current.roles,
            current.link_destinations,
        );
        let mut hasher = Sha256::new();
        hasher.update(b"ZEPHIUM-RETAINED-READ-1\0");
        hasher.update(current.guard);
        for acknowledgement in &current.historical {
            hasher.update(acknowledgement.guard());
        }
        current.guard = hasher.finalize().into();
        Ok(current)
    }
}

impl fmt::Debug for SemanticRetainedReadEvidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SemanticRetainedReadEvidence")
            .field("captures", &self.captures.len())
            .field("items", &self.retained_items())
            .field("bytes", &self.retained_bytes())
            .finish_non_exhaustive()
    }
}
