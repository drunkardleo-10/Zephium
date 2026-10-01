//! Immutable public evidence carried between Work consumers, never execution.
//!
//! This foundation collects already-validated model mappings across retained
//! documents/resources. It does not publish artifacts, dispatch providers,
//! merge claims, certify truth, restore refs or grant a parent data-flow policy.
//! The caller must independently authorize every later disclosure/persistence.

use crate::*;
use sha2::{Digest, Sha256};
use std::{fmt, sync::Arc};
use thiserror::Error;

/// Maximum distinct extraction contributions in one bounded evidence set.
pub const MAX_WORK_EVIDENCE_ENTRIES: u16 = 16;
/// Aggregate retained strings and source primitives, excluding fixed metadata.
/// Metadata is independently bounded by entry and existing extraction ceilings.
pub const MAX_WORK_EVIDENCE_CONTENT_BYTES: u32 = 1024 * 1024;

/// An immutable evidence identity, not a browser reference or publication proof.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct WorkEvidenceEntryId([u8; 16]);
impl WorkEvidenceEntryId {
    /// Stable data-only identity for an explicit consumer.
    pub const fn bytes(self) -> [u8; 16] {
        self.0
    }
}
impl fmt::Debug for WorkEvidenceEntryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WorkEvidenceEntryId([redacted])")
    }
}

/// Source binding proves provenance, not semantic entailment or user acceptance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkEvidenceReview {
    /// Every mapping still requires independent review for the intended use.
    NeedsReview,
}

/// A mapping plus its exact historical retained-resource/document attribution.
/// No live lease, registry, native callback or provider transcript is retained.
pub struct WorkEvidenceEntry {
    id: WorkEvidenceEntryId,
    resource: WorkBrowserResourceIdentity,
    run: ContextRunId,
    storage: ContextProfileStorageClass,
    document: String,
    result: Arc<SemanticOwnedExtractionResult>,
    content_bytes: u32,
}
impl WorkEvidenceEntry {
    /// Joins only an admitted result and an exact historical read binding.
    /// This is not a liveness check; evidence may outlive native teardown.
    /// The caller retains its original result on every admission failure.
    pub fn try_new(
        binding: &WorkBrowserReadBinding,
        result: &Arc<SemanticOwnedExtractionResult>,
    ) -> Result<Arc<Self>, WorkEvidenceError> {
        let sources = result.evidence_sources();
        if sources.is_empty() || result.stats().sensitive_source_edges() != 0 {
            return Err(WorkEvidenceError::Source);
        }
        let resource = binding.lease().resource().identity();
        if sources.iter().any(|source| {
            source.sensitivity != SemanticSensitivity::Public
                || &source.frame != binding.frame()
                || source.frame.context().identity().profile() != resource.profile()
                || source.frame.context().identity().owner() != binding.lease().run()
        }) {
            return Err(WorkEvidenceError::Source);
        }
        let document = binding.document().as_url().as_str();
        if document.len() > MAX_AGENT_BROWSER_NAVIGATION_URL_BYTES
            || crate::semantic_wire::looks_like_secret_value(document)
        {
            return Err(WorkEvidenceError::Source);
        }
        let mut bytes = document
            .len()
            .checked_add(result.stats().text_bytes() as usize)
            .ok_or(WorkEvidenceError::Capacity)?;
        for field in result.fields() {
            bytes = bytes
                .checked_add(field.name().len())
                .ok_or(WorkEvidenceError::Capacity)?;
        }
        for source in sources {
            let content = match &source.content {
                SemanticOwnedReadContent::Text(text) => text.len(),
                SemanticOwnedReadContent::ValuePreview { text, .. } => text.len(),
                SemanticOwnedReadContent::Boolean(_) => 1,
                SemanticOwnedReadContent::Ordinal(_) => 2,
            };
            // Each owned source currently retains its own canonical origin.
            bytes = bytes
                .checked_add(content)
                .and_then(|n| n.checked_add(source.frame.origin().as_url().as_str().len()))
                .ok_or(WorkEvidenceError::Capacity)?;
        }
        let content_bytes = u32::try_from(bytes).map_err(|_| WorkEvidenceError::Capacity)?;
        if content_bytes > MAX_WORK_EVIDENCE_CONTENT_BYTES {
            return Err(WorkEvidenceError::Capacity);
        }
        Ok(Arc::new(Self {
            id: WorkEvidenceEntryId(ulid::Ulid::new().0.to_be_bytes()),
            resource,
            run: binding.lease().run(),
            storage: binding.storage(),
            document: document.to_owned(),
            result: Arc::clone(result),
            content_bytes,
        }))
    }
    /// Data-only contribution identity. Read-local fragment IDs remain local.
    pub const fn id(&self) -> WorkEvidenceEntryId {
        self.id
    }
    /// Historical resource identity, without its native incarnation authority.
    pub const fn resource(&self) -> WorkBrowserResourceIdentity {
        self.resource
    }
    /// Historical producing run, without its execution lease.
    pub const fn run(&self) -> ContextRunId {
        self.run
    }
    /// Historical profile storage class. Retaining evidence never upgrades
    /// ephemeral browsing to persistence permission.
    pub const fn storage(&self) -> ContextProfileStorageClass {
        self.storage
    }
    /// Exact native-finalized historical document, not a navigation permission.
    pub fn document(&self) -> &str {
        &self.document
    }
    /// Original immutable mapping with guarded result-local source resolution.
    /// Cite through this entry and its result, never a set-global `@r` ordinal.
    pub fn extraction(&self) -> &SemanticOwnedExtractionResult {
        &self.result
    }
    /// Structural source admission never upgrades a mapping to verified truth.
    pub const fn review(&self) -> WorkEvidenceReview {
        WorkEvidenceReview::NeedsReview
    }
    /// Charged retained content. Shared imports are charged conservatively again.
    pub const fn content_bytes(&self) -> u32 {
        self.content_bytes
    }
}
impl fmt::Debug for WorkEvidenceEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkEvidenceEntry")
            .field("content_bytes", &self.content_bytes)
            .field("review", &self.review())
            .finish_non_exhaustive()
    }
}

/// Caller-selected bounds under fixed process ceilings; no implicit eviction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkEvidenceBudget {
    entries: u16,
    content_bytes: u32,
}
impl WorkEvidenceBudget {
    /// Validates nonzero limits. Budget exhaustion preserves all prior entries.
    pub const fn try_new(entries: u16, content_bytes: u32) -> Result<Self, WorkEvidenceError> {
        if entries == 0
            || entries > MAX_WORK_EVIDENCE_ENTRIES
            || content_bytes == 0
            || content_bytes > MAX_WORK_EVIDENCE_CONTENT_BYTES
        {
            Err(WorkEvidenceError::Capacity)
        } else {
            Ok(Self {
                entries,
                content_bytes,
            })
        }
    }
}

/// Content-free handle for parent-node output joins. It proves neither durable
/// publication, factual correctness nor authority to read/disclose the body.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct WorkEvidenceDescriptor {
    id: [u8; 16],
    work: WorkId,
    profile: AgentWorkProfileId,
    run: ContextRunId,
    digest: [u8; 32],
    entries: u16,
    content_bytes: u32,
}
impl WorkEvidenceDescriptor {
    /// Immutable set identity; never a task or supervisor identity.
    pub const fn id(self) -> [u8; 16] {
        self.id
    }
    /// Existing owning Work.
    pub const fn work(self) -> WorkId {
        self.work
    }
    /// Exact source profile; consumers must independently authorize access.
    pub const fn profile(self) -> AgentWorkProfileId {
        self.profile
    }
    /// Exact producing run, joined from every original retained read lease.
    /// The orchestration owner independently joins its manifest/node/attempt.
    pub const fn run(self) -> ContextRunId {
        self.run
    }
    /// Integrity identity over ordered evidence and ownership, not authenticity
    /// or publication proof. Diagnostics must not print this digest.
    pub const fn digest(self) -> [u8; 32] {
        self.digest
    }
    /// Contribution count, not unique sources or agreement count.
    pub const fn entries(self) -> u16 {
        self.entries
    }
    /// Conservatively charged retained content bytes.
    pub const fn content_bytes(self) -> u32 {
        self.content_bytes
    }
}
impl fmt::Debug for WorkEvidenceDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkEvidenceDescriptor")
            .field("entries", &self.entries)
            .field("content_bytes", &self.content_bytes)
            .finish_non_exhaustive()
    }
}

/// Bounded collection owner before a one-way immutable handoff.
pub struct WorkEvidenceBuilder {
    work: WorkId,
    profile: AgentWorkProfileId,
    budget: WorkEvidenceBudget,
    run: Option<ContextRunId>,
    entries: Vec<Arc<WorkEvidenceEntry>>,
    content_bytes: u32,
}
impl WorkEvidenceBuilder {
    /// Uses existing Work/profile identity; allocates no native or worker state.
    pub const fn new(
        work: WorkId,
        profile: AgentWorkProfileId,
        budget: WorkEvidenceBudget,
    ) -> Self {
        Self {
            work,
            profile,
            budget,
            run: None,
            entries: Vec::new(),
            content_bytes: 0,
        }
    }
    /// Shares one immutable contribution. Failure leaves builder and caller
    /// unchanged. Duplicate contributions cannot masquerade as corroboration.
    pub fn insert(&mut self, entry: &Arc<WorkEvidenceEntry>) -> Result<(), WorkEvidenceError> {
        if entry.resource.work() != self.work
            || entry.resource.profile() != self.profile
            || self.run.is_some_and(|run| run != entry.run)
        {
            return Err(WorkEvidenceError::Scope);
        }
        if self.entries.iter().any(|prior| {
            prior.id == entry.id
                || (prior.resource == entry.resource
                    && prior.result.evidence_guard() == entry.result.evidence_guard())
        }) {
            return Err(WorkEvidenceError::Duplicate);
        }
        let bytes = self
            .content_bytes
            .checked_add(entry.content_bytes)
            .ok_or(WorkEvidenceError::Capacity)?;
        if self.entries.len() >= usize::from(self.budget.entries)
            || bytes > self.budget.content_bytes
        {
            return Err(WorkEvidenceError::Capacity);
        }
        self.entries
            .try_reserve(1)
            .map_err(|_| WorkEvidenceError::Capacity)?;
        self.entries.push(Arc::clone(entry));
        self.content_bytes = bytes;
        self.run = Some(entry.run);
        Ok(())
    }
    /// Freezes an ordered collection. An empty set never pretends to be evidence.
    pub fn finish(self) -> Result<WorkEvidenceSet, WorkEvidenceError> {
        if self.entries.is_empty() {
            return Err(WorkEvidenceError::Empty);
        }
        let run = self.run.ok_or(WorkEvidenceError::Empty)?;
        let mut digest = Sha256::new();
        digest.update(b"zephium.work-evidence.v1\0");
        digest.update(self.work.bytes());
        digest.update(self.profile.to_string().as_bytes());
        digest.update(run.bytes());
        digest.update(self.content_bytes.to_be_bytes());
        digest.update((self.entries.len() as u16).to_be_bytes());
        for entry in &self.entries {
            digest.update(entry.id.bytes());
            digest.update(entry.resource.resource().bytes());
            digest.update(entry.resource.context().bytes());
            digest.update([match entry.storage {
                ContextProfileStorageClass::Durable => 1,
                ContextProfileStorageClass::Ephemeral => 2,
            }]);
            digest.update((entry.document.len() as u64).to_be_bytes());
            digest.update(entry.document.as_bytes());
            digest.update(entry.result.evidence_guard());
        }
        Ok(WorkEvidenceSet {
            descriptor: WorkEvidenceDescriptor {
                id: ulid::Ulid::new().0.to_be_bytes(),
                work: self.work,
                profile: self.profile,
                run,
                digest: digest.finalize().into(),
                entries: self.entries.len() as u16,
                content_bytes: self.content_bytes,
            },
            entries: self.entries.into(),
        })
    }
}
impl fmt::Debug for WorkEvidenceBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkEvidenceBuilder")
            .field("entries", &self.entries.len())
            .field("content_bytes", &self.content_bytes)
            .finish_non_exhaustive()
    }
}

/// Immutable ordered evidence, not a synthesized answer or durable artifact.
/// Parent/child consumers share allocations without copying page transcripts.
#[derive(Clone)]
pub struct WorkEvidenceSet {
    descriptor: WorkEvidenceDescriptor,
    entries: Arc<[Arc<WorkEvidenceEntry>]>,
}
impl WorkEvidenceSet {
    /// Compact metadata for an independent parent-node/output join.
    pub const fn descriptor(&self) -> WorkEvidenceDescriptor {
        self.descriptor
    }
    /// Explicit private-data access. Entry-local citation guards remain intact.
    pub fn entries(&self) -> &[Arc<WorkEvidenceEntry>] {
        &self.entries
    }
    /// Resolves only an exact contribution identity in this set.
    pub fn entry(&self, id: WorkEvidenceEntryId) -> Option<&WorkEvidenceEntry> {
        self.entries
            .iter()
            .find(|entry| entry.id == id)
            .map(Arc::as_ref)
    }
}
impl fmt::Debug for WorkEvidenceSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkEvidenceSet")
            .field("descriptor", &self.descriptor)
            .finish()
    }
}

/// Content-free admission failures; none discard earlier collected evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum WorkEvidenceError {
    /// Work or profile differs from the exact source ownership.
    #[error("evidence ownership mismatch")]
    Scope,
    /// Result sources do not match the exact public retained document binding.
    #[error("evidence source mismatch")]
    Source,
    /// One fixed content, entry or allocation bound was exhausted.
    #[error("evidence capacity exhausted")]
    Capacity,
    /// The same immutable contribution is already present.
    #[error("duplicate evidence contribution")]
    Duplicate,
    /// No source-carrying contribution exists.
    #[error("empty evidence collection")]
    Empty,
}

#[cfg(test)]
#[path = "agent_work_evidence_tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) fn descriptor_for_test(
    work: WorkId,
    profile: AgentWorkProfileId,
    run: ContextRunId,
) -> WorkEvidenceDescriptor {
    tests::descriptor(work, profile, run)
}
