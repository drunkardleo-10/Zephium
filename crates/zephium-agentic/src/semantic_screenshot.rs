//! One-shot bounded viewport screenshots over exact browser authority.
//!
//! A screenshot request is bound to an exact semantic observation that already
//! reached committed model delivery, then split into a non-cloneable pending
//! verifier and native request. The native adapter must capture only the web
//! content viewport after proving the exact current document has reached its
//! platform content-available edge. Returned PNG bytes are bounded before
//! admission, CRC-checked, restricted to fixed 8-bit RGB/RGBA, and compacted in
//! place to remove all ancillary metadata. No pixel decoder, file, timer,
//! worker, view, or thread is created by this functional core.

use std::fmt;
use std::num::NonZeroU64;

use crc32fast::Hasher as Crc32;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::semantic_diff::SemanticObservationFingerprint;
use crate::{
    ContextJoin, FrameId, SemanticCaptureInstant, SemanticCompleteness,
    SemanticFrameBoundaryStatus, SemanticObservation, SemanticObservationAcknowledgement,
    SemanticObservationGeneration, SemanticObservationId, SemanticSensitivity,
    SemanticSnapshotGeneration, SemanticValueSummary,
};

/// Maximum physical pixel width accepted from a native viewport capture.
pub const MAX_SEMANTIC_SCREENSHOT_WIDTH: u16 = 2048;
/// Maximum physical pixel height accepted from a native viewport capture.
pub const MAX_SEMANTIC_SCREENSHOT_HEIGHT: u16 = 2048;
/// Maximum aggregate physical pixels in one admitted screenshot.
pub const MAX_SEMANTIC_SCREENSHOT_PIXELS: u32 = 2_097_152;
/// Maximum native PNG bytes accepted before structural validation.
pub const MAX_SEMANTIC_SCREENSHOT_PNG_BYTES: u32 = 9 * 1024 * 1024;
/// Maximum PNG chunks scanned in one native result.
pub const MAX_SEMANTIC_SCREENSHOT_PNG_CHUNKS: u16 = 1024;
/// Maximum request-to-completion interval on the process monotonic clock.
pub const MAX_SEMANTIC_SCREENSHOT_CAPTURE_MILLIS: u64 = 5_000;
/// Maximum native screenshot captures executing or awaiting settlement.
pub const MAX_PENDING_SEMANTIC_SCREENSHOTS: usize = 2;

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const PNG_MINIMUM_STRUCTURAL_BYTES: u32 = 57;
const PNG_IHDR: [u8; 4] = *b"IHDR";
const PNG_IDAT: [u8; 4] = *b"IDAT";
const PNG_IEND: [u8; 4] = *b"IEND";
const PNG_SRGB: [u8; 4] = *b"sRGB";
const PNG_GAMA: [u8; 4] = *b"gAMA";
const PNG_CHRM: [u8; 4] = *b"cHRM";

/// Nonzero identity for one screenshot attempt.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SemanticScreenshotRequestId(NonZeroU64);

impl SemanticScreenshotRequestId {
    /// Constructs a nonzero screenshot request identity.
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the process-local request ordinal to native coordination code.
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Debug for SemanticScreenshotRequestId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SemanticScreenshotRequestId([redacted])")
    }
}

/// Closed screenshot scope supported symmetrically by both initial platforms.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticScreenshotScope {
    /// Exact current web-content viewport, excluding native browser chrome.
    Viewport,
}

/// Hard caller-selected ceilings for one screenshot attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticScreenshotBudget {
    max_width: u16,
    max_height: u16,
    max_pixels: u32,
    max_png_bytes: u32,
}

impl SemanticScreenshotBudget {
    /// Conservative default for one model-facing viewport image.
    pub const STANDARD: Self = Self {
        max_width: 1280,
        max_height: 800,
        max_pixels: 1_024_000,
        max_png_bytes: 5 * 1024 * 1024,
    };

    /// Validates nonzero ceilings under all process-wide hard limits.
    pub const fn try_new(
        max_width: u16,
        max_height: u16,
        max_pixels: u32,
        max_png_bytes: u32,
    ) -> Result<Self, SemanticScreenshotBudgetError> {
        if max_width == 0
            || max_width > MAX_SEMANTIC_SCREENSHOT_WIDTH
            || max_height == 0
            || max_height > MAX_SEMANTIC_SCREENSHOT_HEIGHT
            || max_pixels == 0
            || max_pixels > MAX_SEMANTIC_SCREENSHOT_PIXELS
            || max_png_bytes < PNG_MINIMUM_STRUCTURAL_BYTES
            || max_png_bytes > MAX_SEMANTIC_SCREENSHOT_PNG_BYTES
        {
            return Err(SemanticScreenshotBudgetError::Invalid);
        }
        let dimension_pixels = (max_width as u32) * (max_height as u32);
        if max_pixels > dimension_pixels {
            return Err(SemanticScreenshotBudgetError::Invalid);
        }
        Ok(Self {
            max_width,
            max_height,
            max_pixels,
            max_png_bytes,
        })
    }

    /// Maximum physical pixel width.
    pub const fn max_width(self) -> u16 {
        self.max_width
    }

    /// Maximum physical pixel height.
    pub const fn max_height(self) -> u16 {
        self.max_height
    }

    /// Maximum aggregate physical pixels.
    pub const fn max_pixels(self) -> u32 {
        self.max_pixels
    }

    /// Maximum native encoded PNG bytes, including removable metadata.
    pub const fn max_png_bytes(self) -> u32 {
        self.max_png_bytes
    }
}

/// Refusal to construct an invalid screenshot budget.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticScreenshotBudgetError {
    /// A ceiling was zero, internally contradictory, or above its hard limit.
    #[error("semantic screenshot budget is invalid")]
    Invalid,
}

/// Refusal to bind a screenshot request to semantic/model authority.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticScreenshotRequestError {
    /// The exact source observation did not reach committed model delivery.
    #[error("semantic screenshot source observation was not delivered")]
    ObservationNotDelivered,
    /// The source observation was not rooted at the main web-content frame.
    #[error("semantic screenshot source context is unsupported")]
    UnsupportedContext,
    /// The bounded viewport/frame scope was not completely observable.
    #[error("semantic screenshot source observation is incomplete")]
    IncompleteObservation,
    /// The source observation contains known secret or redacted semantics.
    #[error("semantic screenshot source observation contains secret content")]
    SecretContent,
    /// The monotonic interval was empty, reversed, or above the hard deadline.
    #[error("semantic screenshot deadline is invalid")]
    Deadline,
}

/// Pre-policy screenshot request bound to an exact acknowledged observation.
///
/// This request is not a policy permit. The future supervisor must still
/// authorize visual sensitive-data disclosure, run/profile/origin scope, cost,
/// and human-control state before dispatching the native half.
#[must_use]
pub struct SemanticScreenshotRequest {
    id: SemanticScreenshotRequestId,
    scope: SemanticScreenshotScope,
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
    snapshot_generation: SemanticSnapshotGeneration,
    context: ContextJoin,
    requested_at: SemanticCaptureInstant,
    deadline: SemanticCaptureInstant,
    budget: SemanticScreenshotBudget,
    source_guard: [u8; 32],
    guard: [u8; 32],
}

impl SemanticScreenshotRequest {
    /// Request identity.
    pub const fn id(&self) -> SemanticScreenshotRequestId {
        self.id
    }

    /// Closed viewport-only v1 capture scope.
    pub const fn scope(&self) -> SemanticScreenshotScope {
        self.scope
    }

    /// Exact acknowledged observation from which the request was issued.
    pub const fn observation(&self) -> SemanticObservationId {
        self.observation
    }

    /// Exact progressive generation of the acknowledged observation.
    pub const fn observation_generation(&self) -> SemanticObservationGeneration {
        self.observation_generation
    }

    /// Exact main-frame semantic snapshot that authorized visual capture.
    pub const fn snapshot_generation(&self) -> SemanticSnapshotGeneration {
        self.snapshot_generation
    }

    /// Exact context/document/cancellation authority.
    pub const fn context(&self) -> ContextJoin {
        self.context
    }

    /// Trusted-shell request time.
    pub const fn requested_at(&self) -> SemanticCaptureInstant {
        self.requested_at
    }

    /// Sole absolute completion deadline.
    pub const fn deadline(&self) -> SemanticCaptureInstant {
        self.deadline
    }

    /// Hard native and admitted-image ceilings.
    pub const fn budget(&self) -> SemanticScreenshotBudget {
        self.budget
    }

    fn begin_native(self) -> (SemanticScreenshotPending, SemanticScreenshotNativeRequest) {
        let pending = SemanticScreenshotPending {
            id: self.id,
            scope: self.scope,
            observation: self.observation,
            observation_generation: self.observation_generation,
            snapshot_generation: self.snapshot_generation,
            context: self.context,
            requested_at: self.requested_at,
            deadline: self.deadline,
            budget: self.budget,
            source_guard: self.source_guard,
            guard: self.guard,
        };
        let native = SemanticScreenshotNativeRequest {
            id: self.id,
            scope: self.scope,
            context: self.context,
            requested_at: self.requested_at,
            deadline: self.deadline,
            budget: self.budget,
            snapshot_generation: self.snapshot_generation,
            guard: self.guard,
        };
        (pending, native)
    }
}

impl fmt::Debug for SemanticScreenshotRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticScreenshotRequest")
            .field("id", &self.id)
            .field("scope", &self.scope)
            .field("observation", &self.observation)
            .field("observation_generation", &self.observation_generation)
            .field("snapshot_generation", &self.snapshot_generation)
            .field("context", &self.context)
            .field("requested_at", &self.requested_at)
            .field("deadline", &self.deadline)
            .field("budget", &self.budget)
            .field("source_guard", &"[redacted]")
            .field("guard", &"[redacted]")
            .finish()
    }
}

/// Binds one viewport screenshot to exact committed semantic-model delivery.
pub fn prepare_semantic_screenshot(
    id: SemanticScreenshotRequestId,
    observation: &SemanticObservation,
    acknowledgement: &SemanticObservationAcknowledgement,
    requested_at: SemanticCaptureInstant,
    deadline: SemanticCaptureInstant,
    budget: SemanticScreenshotBudget,
) -> Result<SemanticScreenshotRequest, SemanticScreenshotRequestError> {
    if !acknowledgement.matches(observation) {
        return Err(SemanticScreenshotRequestError::ObservationNotDelivered);
    }
    let context = observation.request().context();
    if context.frame() != FrameId::MAIN {
        return Err(SemanticScreenshotRequestError::UnsupportedContext);
    }
    if observation
        .frames()
        .iter()
        .any(|frame| frame.completeness() != SemanticCompleteness::Complete)
        || observation.frame_boundaries().iter().any(|boundary| {
            !matches!(
                boundary.status(),
                SemanticFrameBoundaryStatus::Observed { .. }
            )
        })
    {
        return Err(SemanticScreenshotRequestError::IncompleteObservation);
    }
    if observation.frames().iter().any(|frame| {
        frame.nodes().iter().any(|node| {
            node.sensitivity() == SemanticSensitivity::Secret
                || matches!(node.value(), Some(SemanticValueSummary::Redacted))
        })
    }) {
        return Err(SemanticScreenshotRequestError::SecretContent);
    }
    let Some(duration) = deadline.millis().checked_sub(requested_at.millis()) else {
        return Err(SemanticScreenshotRequestError::Deadline);
    };
    if duration == 0 || duration > MAX_SEMANTIC_SCREENSHOT_CAPTURE_MILLIS {
        return Err(SemanticScreenshotRequestError::Deadline);
    }
    let fingerprint = SemanticObservationFingerprint::from_observation(observation);
    let snapshot_generation = observation
        .frames()
        .first()
        .ok_or(SemanticScreenshotRequestError::IncompleteObservation)?
        .generation();
    let scope = SemanticScreenshotScope::Viewport;
    let guard = screenshot_request_guard(
        id,
        scope,
        fingerprint.digest(),
        requested_at,
        deadline,
        budget,
    );
    Ok(SemanticScreenshotRequest {
        id,
        scope,
        observation: observation.request().id(),
        observation_generation: observation.request().generation(),
        snapshot_generation,
        context,
        requested_at,
        deadline,
        budget,
        source_guard: fingerprint.digest(),
        guard,
    })
}

fn screenshot_request_guard(
    id: SemanticScreenshotRequestId,
    scope: SemanticScreenshotScope,
    observation_digest: [u8; 32],
    requested_at: SemanticCaptureInstant,
    deadline: SemanticCaptureInstant,
    budget: SemanticScreenshotBudget,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"zephium.semantic-screenshot-request.v1\0");
    hasher.update(id.get().to_be_bytes());
    hasher.update([match scope {
        SemanticScreenshotScope::Viewport => 0,
    }]);
    hasher.update(observation_digest);
    hasher.update(requested_at.millis().to_be_bytes());
    hasher.update(deadline.millis().to_be_bytes());
    hasher.update(budget.max_width().to_be_bytes());
    hasher.update(budget.max_height().to_be_bytes());
    hasher.update(budget.max_pixels().to_be_bytes());
    hasher.update(budget.max_png_bytes().to_be_bytes());
    hasher.finalize().into()
}

/// Non-cloneable pending verifier retained outside the native executor.
#[must_use]
pub struct SemanticScreenshotPending {
    id: SemanticScreenshotRequestId,
    scope: SemanticScreenshotScope,
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
    snapshot_generation: SemanticSnapshotGeneration,
    context: ContextJoin,
    requested_at: SemanticCaptureInstant,
    deadline: SemanticCaptureInstant,
    budget: SemanticScreenshotBudget,
    source_guard: [u8; 32],
    guard: [u8; 32],
}

impl SemanticScreenshotPending {
    /// Exact pending request identity for bounded coordinator accounting.
    pub const fn id(&self) -> SemanticScreenshotRequestId {
        self.id
    }
}

impl fmt::Debug for SemanticScreenshotPending {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticScreenshotPending")
            .field("id", &self.id)
            .field("scope", &self.scope)
            .field("observation", &self.observation)
            .field("observation_generation", &self.observation_generation)
            .field("snapshot_generation", &self.snapshot_generation)
            .field("context", &self.context)
            .field("deadline", &self.deadline)
            .field("budget", &self.budget)
            .field("source_guard", &"[redacted]")
            .field("guard", &"[redacted]")
            .finish()
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct PendingScreenshotEntry {
    id: SemanticScreenshotRequestId,
    context: ContextJoin,
    guard: [u8; 32],
}

/// Content-free bounded screenshot coordinator status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticScreenshotCoordinatorStatus {
    pending: u8,
}

impl SemanticScreenshotCoordinatorStatus {
    /// Native captures executing or awaiting exact settlement.
    pub const fn pending(self) -> u8 {
        self.pending
    }
}

/// Zero-idle coordinator enforcing global and per-context capture concurrency.
///
/// Every native outcome must terminally settle or cancel its reservation. An
/// admission failure also removes the reservation, so it cannot become implicit
/// retry authority.
#[derive(Default)]
pub struct SemanticScreenshotCoordinator {
    pending: Vec<PendingScreenshotEntry>,
}

impl SemanticScreenshotCoordinator {
    /// Constructs an empty coordinator without allocating.
    pub const fn new() -> Self {
        Self {
            pending: Vec::new(),
        }
    }

    /// Reserves one bounded slot and splits a request into pending/native halves.
    pub fn begin(
        &mut self,
        request: SemanticScreenshotRequest,
    ) -> Result<
        (SemanticScreenshotPending, SemanticScreenshotNativeRequest),
        SemanticScreenshotCoordinatorError,
    > {
        if self.pending.iter().any(|entry| entry.id == request.id) {
            return Err(SemanticScreenshotCoordinatorError::DuplicateRequest);
        }
        if self
            .pending
            .iter()
            .any(|entry| entry.context == request.context)
        {
            return Err(SemanticScreenshotCoordinatorError::ContextBusy);
        }
        if self.pending.len() >= MAX_PENDING_SEMANTIC_SCREENSHOTS {
            return Err(SemanticScreenshotCoordinatorError::Capacity);
        }
        self.pending.push(PendingScreenshotEntry {
            id: request.id,
            context: request.context,
            guard: request.guard,
        });
        Ok(request.begin_native())
    }

    /// Terminally admits one exact capture and releases its reservation.
    pub fn admit(
        &mut self,
        pending: SemanticScreenshotPending,
        current_context: ContextJoin,
        capture: SemanticScreenshotNativeCapture,
    ) -> Result<SemanticScreenshot, SemanticScreenshotCoordinatorError> {
        let Some(index) = self.pending.iter().position(|entry| entry.id == pending.id) else {
            return Err(SemanticScreenshotCoordinatorError::UnknownRequest);
        };
        let entry = self.pending[index];
        if entry.context != pending.context || entry.guard != pending.guard {
            return Err(SemanticScreenshotCoordinatorError::RequestMismatch);
        }
        self.pending.remove(index);
        admit_semantic_screenshot(pending, current_context, capture)
            .map_err(SemanticScreenshotCoordinatorError::Admission)
    }

    /// Terminally cancels one exact reservation after native cancellation/refusal.
    pub fn cancel(
        &mut self,
        pending: SemanticScreenshotPending,
    ) -> Result<(), SemanticScreenshotCoordinatorError> {
        let Some(index) = self.pending.iter().position(|entry| entry.id == pending.id) else {
            return Err(SemanticScreenshotCoordinatorError::UnknownRequest);
        };
        if self.pending[index].context != pending.context
            || self.pending[index].guard != pending.guard
        {
            return Err(SemanticScreenshotCoordinatorError::RequestMismatch);
        }
        self.pending.remove(index);
        Ok(())
    }

    /// Content-free current capacity accounting.
    pub fn status(&self) -> SemanticScreenshotCoordinatorStatus {
        SemanticScreenshotCoordinatorStatus {
            pending: u8::try_from(self.pending.len()).unwrap_or(u8::MAX),
        }
    }
}

impl fmt::Debug for SemanticScreenshotCoordinator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticScreenshotCoordinator")
            .field("status", &self.status())
            .finish()
    }
}

/// Closed screenshot coordinator refusal.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticScreenshotCoordinatorError {
    /// Two pending requests reused one process-local identity.
    #[error("semantic screenshot request identity is already pending")]
    DuplicateRequest,
    /// The same exact context already has a native capture pending.
    #[error("semantic screenshot context already has a pending capture")]
    ContextBusy,
    /// Global native screenshot concurrency is exhausted.
    #[error("semantic screenshot pending capacity is exhausted")]
    Capacity,
    /// No pending reservation matched the settlement identity.
    #[error("semantic screenshot request is not pending")]
    UnknownRequest,
    /// Pending settlement authority did not match its reservation.
    #[error("semantic screenshot pending request mismatch")]
    RequestMismatch,
    /// Exact native-result admission failed terminally.
    #[error(transparent)]
    Admission(SemanticScreenshotError),
}

/// Non-cloneable exact native viewport capture request.
///
/// The platform stream implementation must refuse writes above
/// `budget().max_png_bytes()` so an oversized encoder cannot allocate first and
/// fail only after returning to Rust.
#[must_use]
pub struct SemanticScreenshotNativeRequest {
    id: SemanticScreenshotRequestId,
    scope: SemanticScreenshotScope,
    context: ContextJoin,
    requested_at: SemanticCaptureInstant,
    deadline: SemanticCaptureInstant,
    budget: SemanticScreenshotBudget,
    snapshot_generation: SemanticSnapshotGeneration,
    guard: [u8; 32],
}

impl SemanticScreenshotNativeRequest {
    /// Exact native request identity.
    pub const fn id(&self) -> SemanticScreenshotRequestId {
        self.id
    }

    /// Exact current web-content scope.
    pub const fn scope(&self) -> SemanticScreenshotScope {
        self.scope
    }

    /// Exact context/document/cancellation join to revalidate before capture.
    pub const fn context(&self) -> ContextJoin {
        self.context
    }

    /// Trusted-shell clock anchor used to map native elapsed time.
    pub const fn requested_at(&self) -> SemanticCaptureInstant {
        self.requested_at
    }

    /// Sole absolute native completion deadline.
    pub const fn deadline(&self) -> SemanticCaptureInstant {
        self.deadline
    }

    /// Native dimension, pixel, and bounded-stream ceilings.
    pub const fn budget(&self) -> SemanticScreenshotBudget {
        self.budget
    }

    /// Exact main-frame semantic snapshot to revalidate before native capture.
    pub const fn snapshot_generation(&self) -> SemanticSnapshotGeneration {
        self.snapshot_generation
    }

    /// Consumes the one-shot native job into a claimed completed capture.
    ///
    /// `paint` must come from platform state for this exact document, not from
    /// the page or the encoded image. Admission still rechecks all immutable
    /// authority, times, dimensions, PNG structure, and resource bounds.
    pub fn complete(
        self,
        paint: SemanticScreenshotPaintEvidence,
        started_at: SemanticCaptureInstant,
        completed_at: SemanticCaptureInstant,
        width: u32,
        height: u32,
        png: Vec<u8>,
    ) -> SemanticScreenshotNativeCapture {
        SemanticScreenshotNativeCapture {
            id: self.id,
            scope: self.scope,
            context: self.context,
            requested_at: self.requested_at,
            deadline: self.deadline,
            budget: self.budget,
            snapshot_generation: self.snapshot_generation,
            guard: self.guard,
            paint,
            started_at,
            completed_at,
            width,
            height,
            png,
        }
    }
}

impl fmt::Debug for SemanticScreenshotNativeRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticScreenshotNativeRequest")
            .field("id", &self.id)
            .field("scope", &self.scope)
            .field("context", &self.context)
            .field("requested_at", &self.requested_at)
            .field("deadline", &self.deadline)
            .field("budget", &self.budget)
            .field("snapshot_generation", &self.snapshot_generation)
            .field("guard", &"[redacted]")
            .finish()
    }
}

/// Native attestation that capture cannot return the prior navigation's paint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticScreenshotPaintEvidence {
    /// The exact joined document reached its platform content-available edge.
    ExactDocumentContentAvailable,
}

/// Closed native screenshot refusal emitted after successful port admission.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticScreenshotNativeFailure {
    /// The pinned platform cannot implement the required capture invariants.
    #[error("native semantic screenshot is unsupported")]
    Unsupported,
    /// A bounded queue, encoder, image, or native capture slot is full.
    #[error("native semantic screenshot resources are exhausted")]
    ResourceExhausted,
    /// Cancellation or context retirement terminated the exact attempt.
    #[error("native semantic screenshot was cancelled")]
    Cancelled,
    /// The sole native capture deadline elapsed.
    #[error("native semantic screenshot timed out")]
    TimedOut,
    /// Context, document, or semantic snapshot authority no longer matches.
    #[error("native semantic screenshot authority is stale")]
    Stale,
    /// The exact document has not reached a capturable content edge.
    #[error("native semantic screenshot document is not ready")]
    NotReady,
    /// The exact native renderer disappeared before settlement.
    #[error("native semantic screenshot renderer was lost")]
    RendererLost,
    /// The platform capture or bounded encoder refused the operation.
    #[error("native semantic screenshot transport failed")]
    Transport,
    /// Process teardown permanently sealed the native adapter.
    #[error("native semantic screenshot adapter is shutting down")]
    Shutdown,
}

/// Claimed native result awaiting functional-core admission.
#[must_use]
pub struct SemanticScreenshotNativeCapture {
    id: SemanticScreenshotRequestId,
    scope: SemanticScreenshotScope,
    context: ContextJoin,
    requested_at: SemanticCaptureInstant,
    deadline: SemanticCaptureInstant,
    budget: SemanticScreenshotBudget,
    snapshot_generation: SemanticSnapshotGeneration,
    guard: [u8; 32],
    paint: SemanticScreenshotPaintEvidence,
    started_at: SemanticCaptureInstant,
    completed_at: SemanticCaptureInstant,
    width: u32,
    height: u32,
    png: Vec<u8>,
}

impl fmt::Debug for SemanticScreenshotNativeCapture {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticScreenshotNativeCapture")
            .field("id", &self.id)
            .field("scope", &self.scope)
            .field("context", &self.context)
            .field("requested_at", &self.requested_at)
            .field("deadline", &self.deadline)
            .field("budget", &self.budget)
            .field("snapshot_generation", &self.snapshot_generation)
            .field("paint", &self.paint)
            .field("started_at", &self.started_at)
            .field("completed_at", &self.completed_at)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("png_bytes", &self.png.len())
            .field("png", &"[redacted]")
            .field("guard", &"[redacted]")
            .finish()
    }
}

/// Fixed admitted PNG pixel layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticScreenshotPixelLayout {
    /// Eight-bit red, green, and blue channels.
    Rgb8,
    /// Eight-bit red, green, blue, and alpha channels.
    Rgba8,
}

/// Trust class for admitted screenshot pixels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticScreenshotTrust {
    /// Browser-rendered pixels from hostile page content under native authority.
    BrowserRenderedPage,
}

/// Content-free screenshot validation and compaction metrics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticScreenshotStats {
    width: u16,
    height: u16,
    pixels: u32,
    native_png_bytes: u32,
    canonical_png_bytes: u32,
    native_chunks: u16,
    retained_chunks: u16,
    dropped_ancillary_chunks: u16,
    dropped_ancillary_bytes: u32,
    layout: SemanticScreenshotPixelLayout,
}

impl SemanticScreenshotStats {
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn for_input_metrics_test(
        width: u16,
        height: u16,
        pixels: u32,
        native_png_bytes: u32,
        canonical_png_bytes: u32,
        native_chunks: u16,
        retained_chunks: u16,
        dropped_ancillary_chunks: u16,
        dropped_ancillary_bytes: u32,
        layout: SemanticScreenshotPixelLayout,
    ) -> Self {
        Self {
            width,
            height,
            pixels,
            native_png_bytes,
            canonical_png_bytes,
            native_chunks,
            retained_chunks,
            dropped_ancillary_chunks,
            dropped_ancillary_bytes,
            layout,
        }
    }

    /// Physical pixel width.
    pub const fn width(self) -> u16 {
        self.width
    }

    /// Physical pixel height.
    pub const fn height(self) -> u16 {
        self.height
    }

    /// Aggregate physical pixels.
    pub const fn pixels(self) -> u32 {
        self.pixels
    }

    /// Native PNG bytes before safe ancillary metadata removal.
    pub const fn native_png_bytes(self) -> u32 {
        self.native_png_bytes
    }

    /// Canonical retained PNG bytes.
    pub const fn canonical_png_bytes(self) -> u32 {
        self.canonical_png_bytes
    }

    /// Native chunks scanned and CRC-checked.
    pub const fn native_chunks(self) -> u16 {
        self.native_chunks
    }

    /// Structural/image chunks and fixed bounded color declarations retained.
    pub const fn retained_chunks(self) -> u16 {
        self.retained_chunks
    }

    /// Ancillary metadata chunks removed in place.
    pub const fn dropped_ancillary_chunks(self) -> u16 {
        self.dropped_ancillary_chunks
    }

    /// Ancillary metadata bytes removed in place, including chunk framing.
    pub const fn dropped_ancillary_bytes(self) -> u32 {
        self.dropped_ancillary_bytes
    }

    /// Fixed decoded pixel layout declared by the canonical IHDR.
    pub const fn layout(self) -> SemanticScreenshotPixelLayout {
        self.layout
    }
}

/// Admitted sensitive viewport PNG for an explicitly selected model adapter.
///
/// Page pixels remain hostile and may contain sensitive visual content. This
/// value grants no policy, persistence, logging, or provider authority; those
/// layers must consume it only after their own exact permits and budgets.
#[must_use]
pub struct SemanticScreenshot {
    id: SemanticScreenshotRequestId,
    scope: SemanticScreenshotScope,
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
    context: ContextJoin,
    captured_at: SemanticCaptureInstant,
    stats: SemanticScreenshotStats,
    source_guard: [u8; 32],
    png: Vec<u8>,
}

impl SemanticScreenshot {
    /// Exact screenshot attempt identity.
    pub const fn id(&self) -> SemanticScreenshotRequestId {
        self.id
    }

    /// Exact admitted screenshot scope.
    pub const fn scope(&self) -> SemanticScreenshotScope {
        self.scope
    }

    /// Acknowledged observation from which capture was requested.
    pub const fn observation(&self) -> SemanticObservationId {
        self.observation
    }

    /// Progressive generation of the acknowledged source observation.
    pub const fn observation_generation(&self) -> SemanticObservationGeneration {
        self.observation_generation
    }

    /// Exact context/document/cancellation authority at completion.
    pub const fn context(&self) -> ContextJoin {
        self.context
    }

    /// Trusted-shell completion time; pixels were captured no later than this.
    pub const fn captured_at(&self) -> SemanticCaptureInstant {
        self.captured_at
    }

    /// Screenshots are always conservatively sensitive, never public.
    pub const fn sensitivity(&self) -> SemanticSensitivity {
        SemanticSensitivity::Sensitive
    }

    /// Browser-rendered hostile-page pixel trust class.
    pub const fn trust(&self) -> SemanticScreenshotTrust {
        SemanticScreenshotTrust::BrowserRenderedPage
    }

    /// Content-free dimensions, encoding, and metadata-removal metrics.
    pub const fn stats(&self) -> SemanticScreenshotStats {
        self.stats
    }

    /// Exact canonical PNG bytes for an already-authorized model adapter.
    pub fn as_png(&self) -> &[u8] {
        &self.png
    }

    /// Moves exact canonical PNG bytes into an already-authorized transport.
    pub fn into_png(self) -> Vec<u8> {
        self.png
    }

    pub(crate) fn into_provider_parts(
        self,
    ) -> (
        Vec<u8>,
        SemanticScreenshotStats,
        SemanticScreenshotDeliveryAuthority,
    ) {
        let guard = screenshot_delivery_guard(&self);
        let authority = SemanticScreenshotDeliveryAuthority {
            id: self.id,
            scope: self.scope,
            observation: self.observation,
            observation_generation: self.observation_generation,
            context: self.context,
            captured_at: self.captured_at,
            source_guard: self.source_guard,
            guard,
        };
        (self.png, self.stats, authority)
    }
}

impl fmt::Debug for SemanticScreenshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticScreenshot")
            .field("id", &self.id)
            .field("scope", &self.scope)
            .field("observation", &self.observation)
            .field("observation_generation", &self.observation_generation)
            .field("context", &self.context)
            .field("captured_at", &self.captured_at)
            .field("sensitivity", &SemanticSensitivity::Sensitive)
            .field("trust", &SemanticScreenshotTrust::BrowserRenderedPage)
            .field("stats", &self.stats)
            .field("source_guard", &"[redacted]")
            .field("png", &"[redacted]")
            .finish()
    }
}

/// Content-free exact screenshot proof retained until provider commitment.
///
/// The authority is crate-private because only the fixed provider codec may
/// hold image disclosure authority before transport commitment.
#[must_use]
pub(crate) struct SemanticScreenshotDeliveryAuthority {
    id: SemanticScreenshotRequestId,
    scope: SemanticScreenshotScope,
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
    context: ContextJoin,
    captured_at: SemanticCaptureInstant,
    source_guard: [u8; 32],
    guard: [u8; 32],
}

impl SemanticScreenshotDeliveryAuthority {
    pub(crate) fn matches_observation(&self, observation: &SemanticObservation) -> bool {
        self.observation == observation.request().id()
            && self.observation_generation == observation.request().generation()
            && self.context == observation.request().context()
            && self.source_guard
                == SemanticObservationFingerprint::from_observation(observation).digest()
    }

    pub(crate) const fn context(&self) -> ContextJoin {
        self.context
    }

    pub(crate) const fn guard(&self) -> [u8; 32] {
        self.guard
    }

    pub(crate) fn commit(self) -> SemanticScreenshotDeliveryReceipt {
        SemanticScreenshotDeliveryReceipt {
            id: self.id,
            scope: self.scope,
            observation: self.observation,
            observation_generation: self.observation_generation,
            context: self.context,
            captured_at: self.captured_at,
            guard: self.guard,
        }
    }
}

impl fmt::Debug for SemanticScreenshotDeliveryAuthority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticScreenshotDeliveryAuthority")
            .field("id", &self.id)
            .field("scope", &self.scope)
            .field("observation", &self.observation)
            .field("observation_generation", &self.observation_generation)
            .field("context", &self.context)
            .field("captured_at", &self.captured_at)
            .field("source_guard", &"[redacted]")
            .field("guard", &"[redacted]")
            .finish()
    }
}

/// Content-free proof that one exact canonical screenshot reached a provider.
///
/// This receipt cannot recreate image bytes, grant browser authority, or seed
/// semantic references. It exists only for exact policy taint commitment and
/// later audit correlation.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticScreenshotDeliveryReceipt {
    id: SemanticScreenshotRequestId,
    scope: SemanticScreenshotScope,
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
    context: ContextJoin,
    captured_at: SemanticCaptureInstant,
    guard: [u8; 32],
}

impl SemanticScreenshotDeliveryReceipt {
    /// Exact screenshot request delivered.
    pub const fn id(&self) -> SemanticScreenshotRequestId {
        self.id
    }

    /// Fixed admitted visual scope.
    pub const fn scope(&self) -> SemanticScreenshotScope {
        self.scope
    }

    /// Exact semantic observation that authorized capture.
    pub const fn observation(&self) -> SemanticObservationId {
        self.observation
    }

    /// Exact progressive source generation.
    pub const fn observation_generation(&self) -> SemanticObservationGeneration {
        self.observation_generation
    }

    /// Exact context/document/cancellation authority at capture.
    pub const fn context(&self) -> ContextJoin {
        self.context
    }

    /// Trusted-shell capture completion time.
    pub const fn captured_at(&self) -> SemanticCaptureInstant {
        self.captured_at
    }

    pub(crate) const fn guard(&self) -> [u8; 32] {
        self.guard
    }
}

impl fmt::Debug for SemanticScreenshotDeliveryReceipt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticScreenshotDeliveryReceipt")
            .field("id", &self.id)
            .field("scope", &self.scope)
            .field("observation", &self.observation)
            .field("observation_generation", &self.observation_generation)
            .field("context", &self.context)
            .field("captured_at", &self.captured_at)
            .field("guard", &"[redacted]")
            .finish()
    }
}

fn screenshot_delivery_guard(screenshot: &SemanticScreenshot) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"zephium.semantic-screenshot-delivery.v1\0");
    hasher.update(screenshot.id.get().to_be_bytes());
    hasher.update([match screenshot.scope {
        SemanticScreenshotScope::Viewport => 0,
    }]);
    hasher.update(screenshot.observation.get().to_be_bytes());
    hasher.update(screenshot.observation_generation.get().to_be_bytes());
    hash_context(&mut hasher, screenshot.context);
    hasher.update(screenshot.captured_at.millis().to_be_bytes());
    hasher.update(screenshot.source_guard);
    let stats = screenshot.stats;
    hasher.update(stats.width().to_be_bytes());
    hasher.update(stats.height().to_be_bytes());
    hasher.update(stats.pixels().to_be_bytes());
    hasher.update(stats.canonical_png_bytes().to_be_bytes());
    hasher.update(&screenshot.png);
    hasher.finalize().into()
}

fn hash_context(hasher: &mut Sha256, context: ContextJoin) {
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
}

#[cfg(test)]
pub(crate) fn admitted_test_screenshot(
    observation: &SemanticObservation,
    acknowledgement: &SemanticObservationAcknowledgement,
    id: u64,
    pixel: u8,
) -> SemanticScreenshot {
    let request = prepare_semantic_screenshot(
        SemanticScreenshotRequestId::new(id).expect("test screenshot id"),
        observation,
        acknowledgement,
        SemanticCaptureInstant::from_millis(1_000),
        SemanticCaptureInstant::from_millis(2_000),
        SemanticScreenshotBudget::STANDARD,
    )
    .expect("test screenshot request");
    let context = request.context();
    let mut coordinator = SemanticScreenshotCoordinator::new();
    let (pending, native) = coordinator.begin(request).expect("test screenshot begin");
    let capture = native.complete(
        SemanticScreenshotPaintEvidence::ExactDocumentContentAvailable,
        SemanticCaptureInstant::from_millis(1_100),
        SemanticCaptureInstant::from_millis(1_200),
        1,
        1,
        test_rgba_png(pixel),
    );
    coordinator
        .admit(pending, context, capture)
        .expect("test screenshot admission")
}

#[cfg(test)]
fn test_rgba_png(pixel: u8) -> Vec<u8> {
    fn append_chunk(png: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) {
        png.extend_from_slice(
            &u32::try_from(data.len())
                .expect("test PNG chunk length")
                .to_be_bytes(),
        );
        png.extend_from_slice(&kind);
        png.extend_from_slice(data);
        let mut crc = Crc32::new();
        crc.update(&kind);
        crc.update(data);
        png.extend_from_slice(&crc.finalize().to_be_bytes());
    }

    let mut png = PNG_SIGNATURE.to_vec();
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&1_u32.to_be_bytes());
    header.extend_from_slice(&1_u32.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    append_chunk(&mut png, PNG_IHDR, &header);

    // One final RFC 1951 stored block containing one RGBA scanline, wrapped
    // in zlib with the matching Adler-32 checksum.
    let scanline = [0, pixel, pixel, pixel, 0xff];
    let length = u16::try_from(scanline.len()).expect("test scanline length");
    let mut image = vec![0x78, 0x01, 0x01];
    image.extend_from_slice(&length.to_le_bytes());
    image.extend_from_slice(&(!length).to_le_bytes());
    image.extend_from_slice(&scanline);
    let mut a = 1_u32;
    let mut b = 0_u32;
    for byte in scanline {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    image.extend_from_slice(&((b << 16) | a).to_be_bytes());
    append_chunk(&mut png, PNG_IDAT, &image);
    append_chunk(&mut png, PNG_IEND, &[]);
    png
}

/// Closed refusal from native screenshot result admission.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticScreenshotError {
    /// Pending and native halves came from different screenshot requests.
    #[error("semantic screenshot request binding mismatch")]
    RequestMismatch,
    /// Current context/document/cancellation authority no longer matches.
    #[error("semantic screenshot context is stale")]
    StaleContext,
    /// Native capture began or completed outside the sole request interval.
    #[error("semantic screenshot capture timing is invalid")]
    Timing,
    /// Declared dimensions were zero, inconsistent, or above a hard ceiling.
    #[error("semantic screenshot dimensions are invalid")]
    Dimensions,
    /// Encoded bytes or aggregate pixels exceeded the selected budget.
    #[error("semantic screenshot image ceiling exceeded")]
    ImageLimit,
    /// PNG signature, chunk framing, order, or terminal shape was invalid.
    #[error("semantic screenshot PNG structure is malformed")]
    MalformedPng,
    /// A PNG chunk checksum was invalid.
    #[error("semantic screenshot PNG checksum is invalid")]
    PngChecksum,
    /// PNG pixel format was not fixed non-interlaced 8-bit RGB or RGBA.
    #[error("semantic screenshot PNG pixel format is unsupported")]
    UnsupportedPng,
    /// PNG chunk count exceeded the fixed parser ceiling.
    #[error("semantic screenshot PNG chunk ceiling exceeded")]
    ChunkLimit,
    /// An internal bounded count or in-place compaction invariant failed.
    #[error("semantic screenshot internal invariant failed")]
    Invariant,
}

/// Consumes exact pending/native halves into one bounded canonical screenshot.
fn admit_semantic_screenshot(
    pending: SemanticScreenshotPending,
    current_context: ContextJoin,
    capture: SemanticScreenshotNativeCapture,
) -> Result<SemanticScreenshot, SemanticScreenshotError> {
    if pending.guard != capture.guard
        || pending.id != capture.id
        || pending.scope != capture.scope
        || pending.context != capture.context
        || pending.requested_at != capture.requested_at
        || pending.deadline != capture.deadline
        || pending.budget != capture.budget
        || pending.snapshot_generation != capture.snapshot_generation
    {
        return Err(SemanticScreenshotError::RequestMismatch);
    }
    if current_context != pending.context {
        return Err(SemanticScreenshotError::StaleContext);
    }
    if capture.started_at < pending.requested_at
        || capture.completed_at < capture.started_at
        || capture.completed_at > pending.deadline
    {
        return Err(SemanticScreenshotError::Timing);
    }
    match capture.paint {
        SemanticScreenshotPaintEvidence::ExactDocumentContentAvailable => {}
    }
    let (png, stats) = admit_png(capture.png, capture.width, capture.height, pending.budget)?;
    Ok(SemanticScreenshot {
        id: pending.id,
        scope: pending.scope,
        observation: pending.observation,
        observation_generation: pending.observation_generation,
        context: pending.context,
        captured_at: capture.completed_at,
        stats,
        source_guard: pending.source_guard,
        png,
    })
}

fn admit_png(
    mut png: Vec<u8>,
    declared_width: u32,
    declared_height: u32,
    budget: SemanticScreenshotBudget,
) -> Result<(Vec<u8>, SemanticScreenshotStats), SemanticScreenshotError> {
    let native_png_bytes =
        u32::try_from(png.len()).map_err(|_| SemanticScreenshotError::ImageLimit)?;
    if native_png_bytes > budget.max_png_bytes()
        || native_png_bytes > MAX_SEMANTIC_SCREENSHOT_PNG_BYTES
    {
        return Err(SemanticScreenshotError::ImageLimit);
    }
    if png.len()
        < usize::try_from(PNG_MINIMUM_STRUCTURAL_BYTES)
            .map_err(|_| SemanticScreenshotError::Invariant)?
        || png.get(..PNG_SIGNATURE.len()) != Some(PNG_SIGNATURE.as_slice())
    {
        return Err(SemanticScreenshotError::MalformedPng);
    }

    let mut read = PNG_SIGNATURE.len();
    let mut write = PNG_SIGNATURE.len();
    let mut chunk_count = 0_u16;
    let mut retained_chunks = 0_u16;
    let mut dropped_chunks = 0_u16;
    let mut dropped_bytes = 0_u32;
    let mut seen_header = false;
    let mut seen_data = false;
    let mut data_finished = false;
    let mut seen_end = false;
    let mut seen_srgb = false;
    let mut seen_gama = false;
    let mut seen_chrm = false;
    let mut total_data_bytes = 0_u32;
    let mut dimensions = None;
    let mut layout = None;

    while read < png.len() {
        chunk_count = chunk_count
            .checked_add(1)
            .ok_or(SemanticScreenshotError::ChunkLimit)?;
        if chunk_count > MAX_SEMANTIC_SCREENSHOT_PNG_CHUNKS {
            return Err(SemanticScreenshotError::ChunkLimit);
        }
        let header_end = read
            .checked_add(8)
            .ok_or(SemanticScreenshotError::MalformedPng)?;
        let header = png
            .get(read..header_end)
            .ok_or(SemanticScreenshotError::MalformedPng)?;
        let length = u32::from_be_bytes(
            header[..4]
                .try_into()
                .map_err(|_| SemanticScreenshotError::MalformedPng)?,
        );
        let kind: [u8; 4] = header[4..8]
            .try_into()
            .map_err(|_| SemanticScreenshotError::MalformedPng)?;
        if !kind.iter().all(u8::is_ascii_alphabetic) || kind[2].is_ascii_lowercase() {
            return Err(SemanticScreenshotError::MalformedPng);
        }
        let data_start = header_end;
        let data_end = data_start
            .checked_add(
                usize::try_from(length).map_err(|_| SemanticScreenshotError::MalformedPng)?,
            )
            .ok_or(SemanticScreenshotError::MalformedPng)?;
        let chunk_end = data_end
            .checked_add(4)
            .ok_or(SemanticScreenshotError::MalformedPng)?;
        let chunk = png
            .get(read..chunk_end)
            .ok_or(SemanticScreenshotError::MalformedPng)?;
        let expected_crc = u32::from_be_bytes(
            chunk[chunk.len() - 4..]
                .try_into()
                .map_err(|_| SemanticScreenshotError::MalformedPng)?,
        );
        let mut crc = Crc32::new();
        crc.update(&kind);
        crc.update(
            png.get(data_start..data_end)
                .ok_or(SemanticScreenshotError::MalformedPng)?,
        );
        if crc.finalize() != expected_crc {
            return Err(SemanticScreenshotError::PngChecksum);
        }

        let retain = match kind {
            PNG_IHDR => {
                if seen_header || read != PNG_SIGNATURE.len() || length != 13 {
                    return Err(SemanticScreenshotError::MalformedPng);
                }
                let data = png
                    .get(data_start..data_end)
                    .ok_or(SemanticScreenshotError::MalformedPng)?;
                let width = u32::from_be_bytes(
                    data[..4]
                        .try_into()
                        .map_err(|_| SemanticScreenshotError::MalformedPng)?,
                );
                let height = u32::from_be_bytes(
                    data[4..8]
                        .try_into()
                        .map_err(|_| SemanticScreenshotError::MalformedPng)?,
                );
                if data[8] != 8 || data[10] != 0 || data[11] != 0 || data[12] != 0 {
                    return Err(SemanticScreenshotError::UnsupportedPng);
                }
                let pixel_layout = match data[9] {
                    2 => SemanticScreenshotPixelLayout::Rgb8,
                    6 => SemanticScreenshotPixelLayout::Rgba8,
                    _ => return Err(SemanticScreenshotError::UnsupportedPng),
                };
                validate_dimensions(width, height, declared_width, declared_height, budget)?;
                dimensions = Some((width, height));
                layout = Some(pixel_layout);
                seen_header = true;
                true
            }
            PNG_IDAT => {
                if !seen_header || data_finished {
                    return Err(SemanticScreenshotError::MalformedPng);
                }
                total_data_bytes = total_data_bytes
                    .checked_add(length)
                    .ok_or(SemanticScreenshotError::ImageLimit)?;
                seen_data = true;
                true
            }
            PNG_IEND => {
                if !seen_header || !seen_data || length != 0 || chunk_end != png.len() {
                    return Err(SemanticScreenshotError::MalformedPng);
                }
                seen_end = true;
                true
            }
            PNG_SRGB => {
                let data = png
                    .get(data_start..data_end)
                    .ok_or(SemanticScreenshotError::MalformedPng)?;
                if !seen_header || seen_data || seen_srgb || length != 1 || data[0] > 3 {
                    return Err(SemanticScreenshotError::UnsupportedPng);
                }
                seen_srgb = true;
                true
            }
            PNG_GAMA => {
                let data = png
                    .get(data_start..data_end)
                    .ok_or(SemanticScreenshotError::MalformedPng)?;
                if !seen_header || seen_data || seen_gama || length != 4 || data == [0, 0, 0, 0] {
                    return Err(SemanticScreenshotError::UnsupportedPng);
                }
                seen_gama = true;
                true
            }
            PNG_CHRM => {
                if !seen_header || seen_data || seen_chrm || length != 32 {
                    return Err(SemanticScreenshotError::UnsupportedPng);
                }
                seen_chrm = true;
                true
            }
            _ => {
                if kind[0].is_ascii_uppercase() {
                    return Err(SemanticScreenshotError::UnsupportedPng);
                }
                if !matches!(
                    kind,
                    [b't', b'E', b'X', b't']
                        | [b'z', b'T', b'X', b't']
                        | [b'i', b'T', b'X', b't']
                        | [b't', b'I', b'M', b'E']
                        | [b'p', b'H', b'Y', b's']
                        | [b'e', b'X', b'I', b'f']
                        | [b's', b'P', b'L', b'T']
                ) {
                    return Err(SemanticScreenshotError::UnsupportedPng);
                }
                if seen_data {
                    data_finished = true;
                }
                false
            }
        };

        if retain {
            if write != read {
                png.copy_within(read..chunk_end, write);
            }
            write = write
                .checked_add(chunk_end - read)
                .ok_or(SemanticScreenshotError::Invariant)?;
            retained_chunks = retained_chunks
                .checked_add(1)
                .ok_or(SemanticScreenshotError::Invariant)?;
        } else {
            dropped_chunks = dropped_chunks
                .checked_add(1)
                .ok_or(SemanticScreenshotError::Invariant)?;
            dropped_bytes = dropped_bytes
                .checked_add(
                    u32::try_from(chunk_end - read)
                        .map_err(|_| SemanticScreenshotError::Invariant)?,
                )
                .ok_or(SemanticScreenshotError::Invariant)?;
        }
        read = chunk_end;
        if kind == PNG_IEND {
            break;
        }
    }

    if !seen_header || !seen_data || !seen_end || total_data_bytes == 0 || read != png.len() {
        return Err(SemanticScreenshotError::MalformedPng);
    }
    let (width, height) = dimensions.ok_or(SemanticScreenshotError::MalformedPng)?;
    let layout = layout.ok_or(SemanticScreenshotError::MalformedPng)?;
    png.truncate(write);
    let width = u16::try_from(width).map_err(|_| SemanticScreenshotError::Dimensions)?;
    let height = u16::try_from(height).map_err(|_| SemanticScreenshotError::Dimensions)?;
    let pixels = u32::from(width)
        .checked_mul(u32::from(height))
        .ok_or(SemanticScreenshotError::ImageLimit)?;
    let canonical_png_bytes =
        u32::try_from(png.len()).map_err(|_| SemanticScreenshotError::Invariant)?;
    Ok((
        png,
        SemanticScreenshotStats {
            width,
            height,
            pixels,
            native_png_bytes,
            canonical_png_bytes,
            native_chunks: chunk_count,
            retained_chunks,
            dropped_ancillary_chunks: dropped_chunks,
            dropped_ancillary_bytes: dropped_bytes,
            layout,
        },
    ))
}

fn validate_dimensions(
    width: u32,
    height: u32,
    declared_width: u32,
    declared_height: u32,
    budget: SemanticScreenshotBudget,
) -> Result<(), SemanticScreenshotError> {
    if width == 0
        || height == 0
        || width != declared_width
        || height != declared_height
        || width > u32::from(MAX_SEMANTIC_SCREENSHOT_WIDTH)
        || height > u32::from(MAX_SEMANTIC_SCREENSHOT_HEIGHT)
    {
        return Err(SemanticScreenshotError::Dimensions);
    }
    let pixels = width
        .checked_mul(height)
        .ok_or(SemanticScreenshotError::ImageLimit)?;
    if width > u32::from(budget.max_width())
        || height > u32::from(budget.max_height())
        || pixels > budget.max_pixels()
        || pixels > MAX_SEMANTIC_SCREENSHOT_PIXELS
    {
        return Err(SemanticScreenshotError::ImageLimit);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        decode_semantic_snapshot, encode_semantic_observation, ContextCapabilities,
        ContextCapability, ContextId, ContextIdentity, ContextKind, ContextOperationId,
        ContextRegistry, ContextRunId, ContextSettlement, SemanticDecodeContext, SemanticFrameJoin,
        SemanticFrameTrust, SemanticInvocationId, SemanticModelDeliverySettlement,
        SemanticModelEncodingBudget, SemanticObservationAssembler, SemanticObservationBudget,
        SemanticOrigin, SemanticSnapshotGeneration, SemanticTokenCountQuality,
        SemanticTokenCountRequirement, SemanticTokenCounter, SemanticTokenCounterError,
        SemanticTokenMeasurement, SemanticTokenizerRevision, SEMANTIC_WIRE_VERSION,
    };
    use serde_json::json;
    use zephium_core::ids::ProfileId;

    fn observation(seed: u64) -> SemanticObservation {
        observation_with(seed, "complete", false)
    }

    fn observation_with(seed: u64, completeness: &str, secret: bool) -> SemanticObservation {
        let identity = ContextIdentity::new(
            ContextId::from_raw(u128::from(seed)),
            ContextRunId::from_raw(u128::from(seed + 1)),
            ProfileId::from(u128::from(seed + 2)),
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
            SemanticOrigin::parse(&format!("https://shot-{seed}.example.test/private"))
                .expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        let nodes = if secret {
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "password", "n": "Password",
                 "v": {"k": "redacted"}, "q": "secret"}
            ])
        } else {
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "paragraph", "t": "Private page pixels",
                 "q": "sensitive"}
            ])
        };
        let bytes = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": seed + 10,
            "g": 1,
            "c": completeness,
            "n": nodes
        }))
        .expect("wire");
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(seed + 10).expect("invocation"),
                frame,
                SemanticSnapshotGeneration::new(1).expect("generation"),
            ),
            &bytes,
        )
        .expect("snapshot");
        let request = crate::SemanticObservationRequest::initial(
            SemanticObservationId::new(seed + 20).expect("observation"),
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        SemanticObservationAssembler::new(request, snapshot)
            .expect("assembler")
            .finish()
            .expect("observation")
    }

    struct FixedCounter {
        revision: SemanticTokenizerRevision,
    }

    impl SemanticTokenCounter for FixedCounter {
        fn count_tokens(
            &self,
            input: &str,
        ) -> Result<SemanticTokenMeasurement, SemanticTokenCounterError> {
            if input.is_empty() {
                return Err(SemanticTokenCounterError::InvalidResult);
            }
            SemanticTokenMeasurement::try_new(
                self.revision.clone(),
                100,
                SemanticTokenCountQuality::ExactLocal,
            )
            .map_err(|_| SemanticTokenCounterError::InvalidResult)
        }
    }

    fn acknowledgement(observation: &SemanticObservation) -> SemanticObservationAcknowledgement {
        let revision =
            SemanticTokenizerRevision::try_new("screenshot-test-v1".to_owned()).expect("revision");
        let counter = FixedCounter {
            revision: revision.clone(),
        };
        let budget = SemanticModelEncodingBudget::try_new(
            32 * 1024,
            1_000,
            SemanticTokenCountRequirement::Exact,
        )
        .expect("budget");
        encode_semantic_observation(observation, budget)
            .expect("encode")
            .admit(&counter, &revision)
            .expect("admit")
            .settle_delivery(SemanticModelDeliverySettlement::Committed)
            .expect("commit")
    }

    fn request(
        id: u64,
        observation: &SemanticObservation,
        requested_at: u64,
        deadline: u64,
        budget: SemanticScreenshotBudget,
    ) -> SemanticScreenshotRequest {
        prepare_semantic_screenshot(
            SemanticScreenshotRequestId::new(id).expect("id"),
            observation,
            &acknowledgement(observation),
            SemanticCaptureInstant::from_millis(requested_at),
            SemanticCaptureInstant::from_millis(deadline),
            budget,
        )
        .expect("request")
    }

    fn append_chunk(png: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) {
        png.extend_from_slice(
            &u32::try_from(data.len())
                .expect("chunk length")
                .to_be_bytes(),
        );
        png.extend_from_slice(&kind);
        png.extend_from_slice(data);
        let mut crc = Crc32::new();
        crc.update(&kind);
        crc.update(data);
        png.extend_from_slice(&crc.finalize().to_be_bytes());
    }

    fn zlib_stored(data: &[u8]) -> Vec<u8> {
        let mut encoded = vec![0x78, 0x01];
        let mut chunks = data.chunks(u16::MAX.into()).peekable();
        while let Some(chunk) = chunks.next() {
            encoded.push(if chunks.peek().is_none() { 1 } else { 0 });
            let length = u16::try_from(chunk.len()).expect("stored block");
            encoded.extend_from_slice(&length.to_le_bytes());
            encoded.extend_from_slice(&(!length).to_le_bytes());
            encoded.extend_from_slice(chunk);
        }
        let mut a = 1_u32;
        let mut b = 0_u32;
        for byte in data {
            a = (a + u32::from(*byte)) % 65_521;
            b = (b + a) % 65_521;
        }
        encoded.extend_from_slice(&((b << 16) | a).to_be_bytes());
        encoded
    }

    fn image_data(width: u32, height: u32, color_type: u8) -> Vec<u8> {
        let channels = if color_type == 2 { 3 } else { 4 };
        let row_bytes = usize::try_from(width)
            .expect("width")
            .checked_mul(channels)
            .expect("row");
        let mut data = Vec::new();
        for _ in 0..height {
            data.push(0);
            data.resize(data.len() + row_bytes, 0x7f);
        }
        zlib_stored(&data)
    }

    fn png_with_metadata(
        width: u32,
        height: u32,
        color_type: u8,
        before_data: usize,
        after_data: usize,
        split_data: bool,
    ) -> Vec<u8> {
        let mut png = PNG_SIGNATURE.to_vec();
        let mut header = Vec::with_capacity(13);
        header.extend_from_slice(&width.to_be_bytes());
        header.extend_from_slice(&height.to_be_bytes());
        header.extend_from_slice(&[8, color_type, 0, 0, 0]);
        append_chunk(&mut png, PNG_IHDR, &header);
        for index in 0..before_data {
            append_chunk(
                &mut png,
                *b"tEXt",
                format!("Comment\0private-metadata-{index}").as_bytes(),
            );
        }
        let data = image_data(width, height, color_type);
        if split_data {
            let split = data.len() / 2;
            append_chunk(&mut png, PNG_IDAT, &data[..split]);
            append_chunk(&mut png, PNG_IDAT, &data[split..]);
        } else {
            append_chunk(&mut png, PNG_IDAT, &data);
        }
        for index in 0..after_data {
            append_chunk(
                &mut png,
                *b"tEXt",
                format!("Comment\0trailing-metadata-{index}").as_bytes(),
            );
        }
        append_chunk(&mut png, PNG_IEND, &[]);
        png
    }

    fn admission_error(
        png: Vec<u8>,
        declared_width: u32,
        declared_height: u32,
        started_at: u64,
        completed_at: u64,
        current_context: ContextJoin,
    ) -> SemanticScreenshotCoordinatorError {
        let observation = observation(100);
        let mut coordinator = SemanticScreenshotCoordinator::new();
        let (pending, native) = coordinator
            .begin(request(
                1,
                &observation,
                1_000,
                2_000,
                SemanticScreenshotBudget::STANDARD,
            ))
            .expect("begin");
        let capture = native.complete(
            SemanticScreenshotPaintEvidence::ExactDocumentContentAvailable,
            SemanticCaptureInstant::from_millis(started_at),
            SemanticCaptureInstant::from_millis(completed_at),
            declared_width,
            declared_height,
            png,
        );
        coordinator
            .admit(pending, current_context, capture)
            .expect_err("capture must fail")
    }

    #[test]
    fn admits_one_shot_sensitive_viewport_png_and_strips_native_metadata() {
        let observation = observation(100);
        let context = observation.request().context();
        let png = png_with_metadata(2, 1, 6, 1, 1, true);
        let native_bytes = png.len();
        let mut coordinator = SemanticScreenshotCoordinator::new();
        let prepared = request(
            1,
            &observation,
            1_000,
            2_000,
            SemanticScreenshotBudget::STANDARD,
        );
        assert_eq!(prepared.scope(), SemanticScreenshotScope::Viewport);
        assert_eq!(prepared.observation(), observation.request().id());
        assert_eq!(prepared.observation_generation().get(), 1);
        assert_eq!(prepared.snapshot_generation().get(), 1);
        assert_eq!(prepared.context(), context);
        assert_eq!(prepared.requested_at().millis(), 1_000);
        assert_eq!(prepared.deadline().millis(), 2_000);
        let (pending, native) = coordinator.begin(prepared).expect("begin");
        assert_eq!(coordinator.status().pending(), 1);
        assert_eq!(pending.id().get(), 1);
        assert_eq!(native.id().get(), 1);
        assert_eq!(native.scope(), SemanticScreenshotScope::Viewport);
        assert_eq!(native.context(), context);
        assert_eq!(native.requested_at().millis(), 1_000);
        assert_eq!(native.deadline().millis(), 2_000);
        assert_eq!(native.budget(), SemanticScreenshotBudget::STANDARD);
        assert_eq!(native.snapshot_generation().get(), 1);
        let capture = native.complete(
            SemanticScreenshotPaintEvidence::ExactDocumentContentAvailable,
            SemanticCaptureInstant::from_millis(1_100),
            SemanticCaptureInstant::from_millis(1_200),
            2,
            1,
            png,
        );
        let screenshot = coordinator
            .admit(pending, context, capture)
            .expect("admit screenshot");
        assert_eq!(coordinator.status().pending(), 0);
        assert_eq!(screenshot.id().get(), 1);
        assert_eq!(screenshot.scope(), SemanticScreenshotScope::Viewport);
        assert_eq!(screenshot.observation(), observation.request().id());
        assert_eq!(screenshot.observation_generation().get(), 1);
        assert_eq!(screenshot.context(), context);
        assert_eq!(screenshot.captured_at().millis(), 1_200);
        assert_eq!(screenshot.sensitivity(), SemanticSensitivity::Sensitive);
        assert_eq!(
            screenshot.trust(),
            SemanticScreenshotTrust::BrowserRenderedPage
        );
        assert_eq!(screenshot.stats().width(), 2);
        assert_eq!(screenshot.stats().height(), 1);
        assert_eq!(screenshot.stats().pixels(), 2);
        assert_eq!(
            usize::try_from(screenshot.stats().native_png_bytes()).expect("bytes"),
            native_bytes
        );
        assert_eq!(screenshot.stats().native_chunks(), 6);
        assert_eq!(screenshot.stats().retained_chunks(), 4);
        assert_eq!(screenshot.stats().dropped_ancillary_chunks(), 2);
        assert!(screenshot.stats().dropped_ancillary_bytes() > 0);
        assert_eq!(
            screenshot.stats().layout(),
            SemanticScreenshotPixelLayout::Rgba8
        );
        assert_eq!(
            usize::try_from(screenshot.stats().canonical_png_bytes()).expect("bytes"),
            screenshot.as_png().len()
        );
        assert!(screenshot.as_png().starts_with(PNG_SIGNATURE));
        assert!(!screenshot
            .as_png()
            .windows("private-metadata".len())
            .any(|window| window == b"private-metadata"));
        assert!(!screenshot
            .as_png()
            .windows("trailing-metadata".len())
            .any(|window| window == b"trailing-metadata"));
        let debug = format!("{screenshot:?}");
        assert!(!debug.contains("private-metadata"));
        assert!(!debug.contains("Private page pixels"));
        assert!(screenshot.into_png().starts_with(PNG_SIGNATURE));
    }

    #[test]
    fn provider_delivery_authority_binds_exact_pixels_source_and_redacted_receipt() {
        let source = observation(300);
        let acknowledgement = acknowledgement(&source);
        let first = admitted_test_screenshot(&source, &acknowledgement, 7, 0x11);
        let second = admitted_test_screenshot(&source, &acknowledgement, 7, 0x22);
        let (_, first_stats, first_delivery) = first.into_provider_parts();
        let (_, second_stats, second_delivery) = second.into_provider_parts();

        assert_eq!(first_stats, second_stats);
        assert!(first_delivery.matches_observation(&source));
        assert!(second_delivery.matches_observation(&source));
        assert_ne!(
            first_delivery.guard(),
            second_delivery.guard(),
            "the exact canonical PNG bytes must affect disclosure authority"
        );
        assert!(!first_delivery.matches_observation(&observation(400)));
        let authority_debug = format!("{first_delivery:?}");
        assert!(authority_debug.contains("[redacted]"));
        assert!(!authority_debug.contains("Private page pixels"));

        let receipt = first_delivery.commit();
        assert_eq!(receipt.id().get(), 7);
        assert_eq!(receipt.scope(), SemanticScreenshotScope::Viewport);
        assert_eq!(receipt.observation(), source.request().id());
        assert_eq!(
            receipt.observation_generation(),
            source.request().generation()
        );
        assert_eq!(receipt.context(), source.request().context());
        assert_eq!(receipt.captured_at().millis(), 1_200);
        let receipt_debug = format!("{receipt:?}");
        assert!(receipt_debug.contains("[redacted]"));
        assert!(!receipt_debug.contains("Private page pixels"));
    }

    #[test]
    fn requires_exact_committed_observation_and_bounded_monotonic_deadline() {
        assert!(SemanticScreenshotRequestId::new(0).is_none());
        let first = observation(100);
        let second = observation(200);
        assert_eq!(
            prepare_semantic_screenshot(
                SemanticScreenshotRequestId::new(1).expect("id"),
                &second,
                &acknowledgement(&first),
                SemanticCaptureInstant::from_millis(1_000),
                SemanticCaptureInstant::from_millis(2_000),
                SemanticScreenshotBudget::STANDARD,
            )
            .expect_err("ack substitution"),
            SemanticScreenshotRequestError::ObservationNotDelivered
        );
        for deadline in [999, 1_000, 6_001] {
            assert_eq!(
                prepare_semantic_screenshot(
                    SemanticScreenshotRequestId::new(1).expect("id"),
                    &first,
                    &acknowledgement(&first),
                    SemanticCaptureInstant::from_millis(1_000),
                    SemanticCaptureInstant::from_millis(deadline),
                    SemanticScreenshotBudget::STANDARD,
                )
                .expect_err("deadline"),
                SemanticScreenshotRequestError::Deadline
            );
        }
        let incomplete = observation_with(300, "node_limit", false);
        assert_eq!(
            prepare_semantic_screenshot(
                SemanticScreenshotRequestId::new(2).expect("id"),
                &incomplete,
                &acknowledgement(&incomplete),
                SemanticCaptureInstant::from_millis(1_000),
                SemanticCaptureInstant::from_millis(2_000),
                SemanticScreenshotBudget::STANDARD,
            )
            .expect_err("incomplete"),
            SemanticScreenshotRequestError::IncompleteObservation
        );
        let secret = observation_with(400, "complete", true);
        assert_eq!(
            prepare_semantic_screenshot(
                SemanticScreenshotRequestId::new(3).expect("id"),
                &secret,
                &acknowledgement(&secret),
                SemanticCaptureInstant::from_millis(1_000),
                SemanticCaptureInstant::from_millis(2_000),
                SemanticScreenshotBudget::STANDARD,
            )
            .expect_err("secret"),
            SemanticScreenshotRequestError::SecretContent
        );
        let debug = format!(
            "{:?}",
            request(1, &first, 1_000, 2_000, SemanticScreenshotBudget::STANDARD)
        );
        assert!(!debug.contains("Private page pixels"));
        assert!(!debug.contains("shot-100"));
    }

    #[test]
    fn validates_dimension_pixel_and_native_stream_budgets() {
        for (width, height, pixels, bytes) in [
            (0, 1, 1, 100),
            (1, 0, 1, 100),
            (MAX_SEMANTIC_SCREENSHOT_WIDTH + 1, 1, 1, 100),
            (1, MAX_SEMANTIC_SCREENSHOT_HEIGHT + 1, 1, 100),
            (1, 1, 0, 100),
            (1, 1, 2, 100),
            (1, 1, 1, PNG_MINIMUM_STRUCTURAL_BYTES - 1),
            (1, 1, 1, MAX_SEMANTIC_SCREENSHOT_PNG_BYTES + 1),
        ] {
            assert_eq!(
                SemanticScreenshotBudget::try_new(width, height, pixels, bytes)
                    .expect_err("budget"),
                SemanticScreenshotBudgetError::Invalid
            );
        }
        assert_eq!(
            SemanticScreenshotBudget::try_new(1, 1, 1, PNG_MINIMUM_STRUCTURAL_BYTES)
                .expect("minimal")
                .max_pixels(),
            1
        );

        let small_budget = SemanticScreenshotBudget::try_new(2, 1, 2, 57).expect("budget");
        let png = png_with_metadata(2, 1, 6, 0, 0, false);
        assert_eq!(
            admit_png(png, 2, 1, small_budget).expect_err("byte budget"),
            SemanticScreenshotError::ImageLimit
        );
        assert_eq!(
            admit_png(
                png_with_metadata(1281, 1, 2, 0, 0, false),
                1281,
                1,
                SemanticScreenshotBudget::STANDARD,
            )
            .expect_err("width budget"),
            SemanticScreenshotError::ImageLimit
        );
        assert_eq!(
            admit_png(
                png_with_metadata(2, 1, 6, 0, 0, false),
                3,
                1,
                SemanticScreenshotBudget::STANDARD,
            )
            .expect_err("declared dimensions"),
            SemanticScreenshotError::Dimensions
        );
        let pixel_budget =
            SemanticScreenshotBudget::try_new(100, 100, 5_000, 100_000).expect("pixel budget");
        assert_eq!(
            admit_png(
                png_with_metadata(100, 60, 6, 0, 0, false),
                100,
                60,
                pixel_budget,
            )
            .expect_err("pixel budget"),
            SemanticScreenshotError::ImageLimit
        );
    }

    #[test]
    fn coordinator_bounds_identity_context_concurrency_and_terminal_cleanup() {
        let first = observation(100);
        let second = observation(200);
        let third = observation(300);
        let mut coordinator = SemanticScreenshotCoordinator::new();
        let (first_pending, _first_native) = coordinator
            .begin(request(
                1,
                &first,
                1_000,
                2_000,
                SemanticScreenshotBudget::STANDARD,
            ))
            .expect("first");
        assert_eq!(
            coordinator
                .begin(request(
                    1,
                    &second,
                    1_000,
                    2_000,
                    SemanticScreenshotBudget::STANDARD,
                ))
                .expect_err("duplicate"),
            SemanticScreenshotCoordinatorError::DuplicateRequest
        );
        assert_eq!(
            coordinator
                .begin(request(
                    2,
                    &first,
                    1_000,
                    2_000,
                    SemanticScreenshotBudget::STANDARD,
                ))
                .expect_err("context busy"),
            SemanticScreenshotCoordinatorError::ContextBusy
        );
        let (second_pending, _second_native) = coordinator
            .begin(request(
                2,
                &second,
                1_000,
                2_000,
                SemanticScreenshotBudget::STANDARD,
            ))
            .expect("second");
        assert_eq!(coordinator.status().pending(), 2);
        assert_eq!(
            coordinator
                .begin(request(
                    3,
                    &third,
                    1_000,
                    2_000,
                    SemanticScreenshotBudget::STANDARD,
                ))
                .expect_err("capacity"),
            SemanticScreenshotCoordinatorError::Capacity
        );
        let mut other = SemanticScreenshotCoordinator::new();
        let (wrong_pending, _wrong_native) = other
            .begin(request(
                1,
                &second,
                1_000,
                2_000,
                SemanticScreenshotBudget::STANDARD,
            ))
            .expect("wrong pending");
        assert_eq!(
            coordinator.cancel(wrong_pending),
            Err(SemanticScreenshotCoordinatorError::RequestMismatch)
        );
        assert_eq!(coordinator.status().pending(), 2);
        let first_id = first_pending.id();
        coordinator.cancel(first_pending).expect("cancel first");
        coordinator.cancel(second_pending).expect("cancel second");
        assert_eq!(coordinator.status().pending(), 0);
        let (already_cancelled, _native) = other
            .begin(request(
                3,
                &third,
                1_000,
                2_000,
                SemanticScreenshotBudget::STANDARD,
            ))
            .expect("already cancelled");
        assert_ne!(already_cancelled.id(), first_id);
        assert_eq!(
            coordinator.cancel(already_cancelled),
            Err(SemanticScreenshotCoordinatorError::UnknownRequest)
        );
        assert_eq!(format!("{coordinator:?}"), "SemanticScreenshotCoordinator { status: SemanticScreenshotCoordinatorStatus { pending: 0 } }");
    }

    #[test]
    fn rejects_cross_request_halves_and_releases_failed_reservation() {
        let first = observation(100);
        let second = observation(200);
        let mut coordinator = SemanticScreenshotCoordinator::new();
        let (first_pending, _first_native) = coordinator
            .begin(request(
                1,
                &first,
                1_000,
                2_000,
                SemanticScreenshotBudget::STANDARD,
            ))
            .expect("first");
        let (second_pending, second_native) = coordinator
            .begin(request(
                2,
                &second,
                1_000,
                2_000,
                SemanticScreenshotBudget::STANDARD,
            ))
            .expect("second");
        let wrong_capture = second_native.complete(
            SemanticScreenshotPaintEvidence::ExactDocumentContentAvailable,
            SemanticCaptureInstant::from_millis(1_100),
            SemanticCaptureInstant::from_millis(1_200),
            2,
            1,
            png_with_metadata(2, 1, 6, 0, 0, false),
        );
        assert_eq!(
            coordinator
                .admit(first_pending, first.request().context(), wrong_capture)
                .expect_err("cross request"),
            SemanticScreenshotCoordinatorError::Admission(SemanticScreenshotError::RequestMismatch)
        );
        assert_eq!(coordinator.status().pending(), 1);
        coordinator
            .cancel(second_pending)
            .expect("cancel remaining");
        assert_eq!(coordinator.status().pending(), 0);
    }

    #[test]
    fn rejects_stale_context_and_all_invalid_capture_time_orderings() {
        let first = observation(100);
        let stale = observation(200);
        assert_eq!(
            admission_error(
                png_with_metadata(2, 1, 6, 0, 0, false),
                2,
                1,
                1_100,
                1_200,
                stale.request().context(),
            ),
            SemanticScreenshotCoordinatorError::Admission(SemanticScreenshotError::StaleContext)
        );
        for (started, completed) in [(999, 1_100), (1_200, 1_100), (1_100, 2_001)] {
            assert_eq!(
                admission_error(
                    png_with_metadata(2, 1, 6, 0, 0, false),
                    2,
                    1,
                    started,
                    completed,
                    first.request().context(),
                ),
                SemanticScreenshotCoordinatorError::Admission(SemanticScreenshotError::Timing)
            );
        }
    }

    #[test]
    fn validates_png_crc_terminal_shape_chunk_order_and_fixed_pixel_format() {
        let budget = SemanticScreenshotBudget::STANDARD;
        let mut bad_crc = png_with_metadata(2, 1, 6, 0, 0, false);
        let idat = bad_crc
            .windows(4)
            .position(|window| window == PNG_IDAT)
            .expect("idat");
        bad_crc[idat + 4] ^= 1;
        assert_eq!(
            admit_png(bad_crc, 2, 1, budget).expect_err("CRC"),
            SemanticScreenshotError::PngChecksum
        );

        let mut missing_end = png_with_metadata(2, 1, 6, 0, 0, false);
        missing_end.truncate(missing_end.len() - 12);
        assert_eq!(
            admit_png(missing_end, 2, 1, budget).expect_err("IEND"),
            SemanticScreenshotError::MalformedPng
        );

        let mut bad_signature = png_with_metadata(2, 1, 6, 0, 0, false);
        bad_signature[0] = 0;
        assert_eq!(
            admit_png(bad_signature, 2, 1, budget).expect_err("signature"),
            SemanticScreenshotError::MalformedPng
        );

        let mut unknown_critical = PNG_SIGNATURE.to_vec();
        let base = png_with_metadata(2, 1, 6, 0, 0, false);
        let ihdr_end = PNG_SIGNATURE.len() + 25;
        unknown_critical.extend_from_slice(&base[PNG_SIGNATURE.len()..ihdr_end]);
        append_chunk(&mut unknown_critical, *b"ABCD", b"x");
        unknown_critical.extend_from_slice(&base[ihdr_end..]);
        assert_eq!(
            admit_png(unknown_critical, 2, 1, budget).expect_err("critical"),
            SemanticScreenshotError::UnsupportedPng
        );

        let mut color_declared = PNG_SIGNATURE.to_vec();
        color_declared.extend_from_slice(&base[PNG_SIGNATURE.len()..ihdr_end]);
        append_chunk(&mut color_declared, PNG_SRGB, &[0]);
        color_declared.extend_from_slice(&base[ihdr_end..]);
        let (color_declared, stats) = admit_png(color_declared, 2, 1, budget).expect("sRGB");
        assert_eq!(stats.retained_chunks(), 4);
        assert!(color_declared
            .windows(PNG_SRGB.len())
            .any(|window| window == PNG_SRGB));

        for kind in [*b"tRNS", *b"iCCP", *b"aaAa"] {
            let mut unsupported_ancillary = PNG_SIGNATURE.to_vec();
            unsupported_ancillary.extend_from_slice(&base[PNG_SIGNATURE.len()..ihdr_end]);
            append_chunk(&mut unsupported_ancillary, kind, b"x");
            unsupported_ancillary.extend_from_slice(&base[ihdr_end..]);
            assert_eq!(
                admit_png(unsupported_ancillary, 2, 1, budget)
                    .expect_err("rendering or unknown ancillary"),
                SemanticScreenshotError::UnsupportedPng
            );
        }

        for color_type in [0, 3, 4] {
            assert_eq!(
                admit_png(
                    png_with_metadata(2, 1, color_type, 0, 0, false),
                    2,
                    1,
                    budget,
                )
                .expect_err("color"),
                SemanticScreenshotError::UnsupportedPng
            );
        }

        let mut interlaced = png_with_metadata(2, 1, 6, 0, 0, false);
        let interlace_index = PNG_SIGNATURE.len() + 8 + 12;
        interlaced[interlace_index] = 1;
        let crc_start = PNG_SIGNATURE.len() + 4;
        let crc_end = PNG_SIGNATURE.len() + 8 + 13;
        let mut crc = Crc32::new();
        crc.update(&interlaced[crc_start..crc_end]);
        interlaced[crc_end..crc_end + 4].copy_from_slice(&crc.finalize().to_be_bytes());
        assert_eq!(
            admit_png(interlaced, 2, 1, budget).expect_err("interlace"),
            SemanticScreenshotError::UnsupportedPng
        );

        let mut interrupted_data = PNG_SIGNATURE.to_vec();
        interrupted_data.extend_from_slice(&base[PNG_SIGNATURE.len()..ihdr_end]);
        append_chunk(&mut interrupted_data, PNG_IDAT, b"first");
        append_chunk(&mut interrupted_data, *b"tEXt", b"metadata");
        append_chunk(&mut interrupted_data, PNG_IDAT, b"second");
        append_chunk(&mut interrupted_data, PNG_IEND, &[]);
        assert_eq!(
            admit_png(interrupted_data, 2, 1, budget).expect_err("IDAT order"),
            SemanticScreenshotError::MalformedPng
        );
    }

    #[test]
    fn png_parser_chunk_work_is_explicitly_bounded() {
        let mut png = PNG_SIGNATURE.to_vec();
        let base = png_with_metadata(1, 1, 2, 0, 0, false);
        let ihdr_end = PNG_SIGNATURE.len() + 25;
        png.extend_from_slice(&base[PNG_SIGNATURE.len()..ihdr_end]);
        for _ in 0..MAX_SEMANTIC_SCREENSHOT_PNG_CHUNKS {
            append_chunk(&mut png, *b"tEXt", &[]);
        }
        png.extend_from_slice(&base[ihdr_end..]);
        assert_eq!(
            admit_png(png, 1, 1, SemanticScreenshotBudget::STANDARD).expect_err("chunk ceiling"),
            SemanticScreenshotError::ChunkLimit
        );
    }
}
