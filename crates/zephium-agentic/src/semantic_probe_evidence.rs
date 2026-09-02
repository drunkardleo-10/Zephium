//! Closed, content-free evidence for physical Windows semantic qualification.
//!
//! This diagnostic contract is available only with `probe-harness`. It carries
//! engine/runtime identity and aggregate pass/fail facts, never a world name,
//! context id, URL, profile path, snapshot, page string, native error, or trace.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    Platform, RuntimeFingerprint, MAX_CONTEXT_NAVIGATION_REDIRECTS, MAX_RESOURCE_HELPER_PROCESSES,
    MAX_RESOURCE_RESIDENT_BYTES,
};

/// Version of the Windows semantic-probe result grammar.
pub const WINDOWS_SEMANTIC_PROBE_PROTOCOL_VERSION: u16 = 5;
/// Maximum canonical JSONL bytes emitted by one semantic-probe process.
pub const MAX_WINDOWS_SEMANTIC_PROBE_OUTPUT_BYTES: usize = 16 * 1024;
const MAX_RUN_ELAPSED_MS: u64 = 2 * 60_000;
const MAX_CLEANUP_MS: u32 = 10_000;

/// Exact physical runs required before Windows semantic support can be reviewed.
pub const WINDOWS_SEMANTIC_PHYSICAL_REVIEW_MODES: [WindowsSemanticProbeMode; 7] = [
    WindowsSemanticProbeMode::HiddenFixedDocuments,
    WindowsSemanticProbeMode::HiddenRedirectLifecycle,
    WindowsSemanticProbeMode::HiddenLocationReplacement,
    WindowsSemanticProbeMode::HiddenSuspendResume,
    WindowsSemanticProbeMode::HiddenEventFlood,
    WindowsSemanticProbeMode::HiddenRendererLoss,
    WindowsSemanticProbeMode::HiddenDebuggerCoexistence,
];

/// Closed release-excluded Windows semantic qualifier mode.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowsSemanticProbeMode {
    /// Two fixed documents prove isolation, redaction, replacement, and reuse.
    HiddenFixedDocuments,
    /// A fixed loop and chain prove bounded redirects, recovery, and final identity.
    HiddenRedirectLifecycle,
    /// A host-gated History API mutation proves native replacement and rejoin.
    HiddenLocationReplacement,
    /// One fixed document proves native suspend/readback/resume and fresh observation.
    HiddenSuspendResume,
    /// A bounded context-event flood must fail closed and recover on navigation.
    HiddenEventFlood,
    /// A fixed diagnostics-only renderer crash must revoke semantic authority.
    HiddenRendererLoss,
    /// The fixed-document case must pass while an attached debugger is present.
    HiddenDebuggerCoexistence,
}

impl WindowsSemanticProbeMode {
    /// Parses only one literal release-excluded executable argument.
    pub fn from_argument(argument: &str) -> Option<Self> {
        match argument {
            "--ci-hidden-fixed-documents" => Some(Self::HiddenFixedDocuments),
            "--ci-hidden-redirect-lifecycle" => Some(Self::HiddenRedirectLifecycle),
            "--ci-hidden-location-replacement" => Some(Self::HiddenLocationReplacement),
            "--ci-hidden-suspend-resume" => Some(Self::HiddenSuspendResume),
            "--ci-hidden-event-flood" => Some(Self::HiddenEventFlood),
            "--ci-hidden-renderer-loss" => Some(Self::HiddenRendererLoss),
            "--ci-hidden-debugger-coexistence" => Some(Self::HiddenDebuggerCoexistence),
            _ => None,
        }
    }

    /// Literal runner argument for this mode.
    pub const fn argument(self) -> &'static str {
        match self {
            Self::HiddenFixedDocuments => "--ci-hidden-fixed-documents",
            Self::HiddenRedirectLifecycle => "--ci-hidden-redirect-lifecycle",
            Self::HiddenLocationReplacement => "--ci-hidden-location-replacement",
            Self::HiddenSuspendResume => "--ci-hidden-suspend-resume",
            Self::HiddenEventFlood => "--ci-hidden-event-flood",
            Self::HiddenRendererLoss => "--ci-hidden-renderer-loss",
            Self::HiddenDebuggerCoexistence => "--ci-hidden-debugger-coexistence",
        }
    }

    /// Fixed ignored local filename consumed by the offline reviewer.
    pub const fn local_result_filename(self) -> &'static str {
        match self {
            Self::HiddenFixedDocuments => "windows-semantic-fixed-documents.jsonl",
            Self::HiddenRedirectLifecycle => "windows-semantic-redirect-lifecycle.jsonl",
            Self::HiddenLocationReplacement => "windows-semantic-location-replacement.jsonl",
            Self::HiddenSuspendResume => "windows-semantic-suspend-resume.jsonl",
            Self::HiddenEventFlood => "windows-semantic-event-flood.jsonl",
            Self::HiddenRendererLoss => "windows-semantic-renderer-loss.jsonl",
            Self::HiddenDebuggerCoexistence => "windows-semantic-debugger-coexistence.jsonl",
        }
    }
}

/// Content-free teardown facts for one physical semantic run.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WindowsSemanticTeardownEvidence {
    /// Production semantic registration retired exactly.
    pub runtime_retired: bool,
    /// Native WebView2 controller cleanup completed.
    pub view_closed: bool,
    /// Browser-process exit was observed for the exact environment.
    pub browser_process_exited: bool,
    /// Ephemeral user-data directory removal succeeded after process exit.
    pub profile_removed: bool,
    /// Fixed loopback fixture worker joined and closed.
    pub fixture_drained: bool,
    /// Native callback and command accounting reached quiescence.
    pub work_drained: bool,
    /// Native owned views retained after teardown.
    pub retained_native_views: u8,
    /// Bounded cleanup wall-clock duration.
    pub cleanup_ms: u32,
}

/// One bounded content-free WebView2 process resource observation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WindowsSemanticResourceEvidence {
    /// Complete process count returned for the exact ephemeral user-data folder.
    pub webview2_processes: u8,
    /// Checked aggregate resident working set for that exact process cohort.
    pub resident_bytes: u64,
}

/// One machine-readable, content-free physical Windows semantic result.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WindowsSemanticProbeEvidence {
    /// Controller-minted nonzero one-shot identity.
    pub run_id: u64,
    /// Exact OS/WebView2/adapter fingerprint.
    pub runtime: RuntimeFingerprint,
    /// Closed executable mode.
    pub mode: WindowsSemanticProbeMode,
    /// The run used a newly created ephemeral user-data directory.
    pub ephemeral_profile: bool,
    /// The production controller profile was InPrivate.
    pub in_private: bool,
    /// Native extension inventory was proven empty at construction.
    pub extensions_absent: bool,
    /// Host, container, and controller remained hidden.
    pub presentation_hidden: bool,
    /// Domain-owned logical viewport width.
    pub viewport_width: u16,
    /// Domain-owned logical viewport height.
    pub viewport_height: u16,
    /// Every navigable fixture URL belonged to the exact loopback origin.
    pub loopback_only: bool,
    /// Number of successfully decoded snapshots; snapshot bytes are absent.
    pub snapshots: u8,
    /// Number of host-authorized fixture document epochs.
    pub document_epochs: u8,
    /// First fixed snapshot passed its closed semantic assertions.
    pub first_snapshot_verified: bool,
    /// Replacement snapshot passed its closed semantic assertions.
    pub replacement_snapshot_verified: bool,
    /// No first-document semantic value survived in the replacement result.
    pub replacement_stale_state_absent: bool,
    /// Page-world runtime forgery/bridge remained unable to answer the adapter.
    pub page_world_bridge_absent: bool,
    /// Password and token fixtures were absent or typed redacted.
    pub secrets_redacted: bool,
    /// Context-event pressure reached the fixed rejection boundary.
    pub event_flood_refused: bool,
    /// A fresh document recovered after the event-flood refusal.
    pub recovered_after_event_flood: bool,
    /// Production crash observation reported renderer loss.
    pub renderer_loss_observed: bool,
    /// A later semantic dispatch returned the typed renderer-lost refusal.
    pub renderer_lost_refused: bool,
    /// The native `TrySuspend` callback reported successful suspension.
    pub suspend_callback_succeeded: bool,
    /// Hidden-owner attestation followed by `IsSuspended` proved suspended state.
    pub suspended_state_attested: bool,
    /// `Resume` plus hidden-owner and final native-bit readback proved active state.
    pub resume_state_attested: bool,
    /// A fresh exact semantic observation passed after native resume.
    pub post_resume_snapshot_verified: bool,
    /// Bounded wall-clock duration through the native suspend callback.
    pub suspend_ms: u32,
    /// The fixed allowed redirect chain committed its authoritative final target.
    pub redirect_chain_verified: bool,
    /// Exact redirect events observed for the allowed fixed chain.
    pub redirect_chain_hops_observed: u8,
    /// The fixed loop was refused by Zephium's own redirect ceiling.
    pub redirect_limit_refused: bool,
    /// Exact admitted redirect events retained before the loop refusal.
    pub redirect_limit_hops_observed: u8,
    /// A fresh semantic snapshot passed after the redirect-limit refusal.
    pub redirect_recovery_verified: bool,
    /// Native WebView2 source observation found the exact same-origin History API target.
    pub same_document_replacement_observed: bool,
    /// The exact functional-core successor rejoined the native replacement barrier.
    pub same_document_replacement_rejoined: bool,
    /// The pre-replacement context join was refused after the successor was minted.
    pub stale_location_join_refused: bool,
    /// A fresh semantic snapshot passed against the replacement target and successor join.
    pub post_location_snapshot_verified: bool,
    /// Windows reported a debugger attached for the complete mode.
    pub debugger_attached: bool,
    /// Probe host ever displaced foreground, active-window, or thread focus.
    pub focus_theft_observed: bool,
    /// Highest number of concurrently pending semantic invocations.
    pub peak_pending_invocations: u8,
    /// No semantic work remained immediately before teardown.
    pub semantic_work_drained: bool,
    /// Stable process/RSS cohort sampled before the mode-specific work.
    pub resources_before: WindowsSemanticResourceEvidence,
    /// Stable process/RSS cohort sampled after all semantic work drained.
    pub resources_after: WindowsSemanticResourceEvidence,
    /// Bounded whole-run wall-clock duration.
    pub elapsed_ms: u64,
    /// Exact teardown evidence.
    pub teardown: WindowsSemanticTeardownEvidence,
}

impl WindowsSemanticProbeEvidence {
    /// Validates structural ceilings independently of mode qualification.
    pub fn validate(&self) -> Result<(), WindowsSemanticProbeValidationError> {
        if self.run_id == 0 {
            return Err(WindowsSemanticProbeValidationError::Identity);
        }
        if self.runtime.platform != Platform::Windows
            || self.runtime.engine.as_str() != "WebView2"
            || self.runtime.adapter_revision.as_str()
                != "semantic-runtime-m3-lifecycle-m2-redirect-location-resources-v2"
        {
            return Err(WindowsSemanticProbeValidationError::Runtime);
        }
        if self.viewport_width == 0
            || self.viewport_height == 0
            || self.snapshots > 3
            || self.document_epochs == 0
            || self.document_epochs > 3
            || self.peak_pending_invocations > 1
            || self.suspend_ms > 10_000
            || self.redirect_chain_hops_observed > MAX_CONTEXT_NAVIGATION_REDIRECTS as u8
            || self.redirect_limit_hops_observed > MAX_CONTEXT_NAVIGATION_REDIRECTS as u8
            || self.elapsed_ms > MAX_RUN_ELAPSED_MS
            || !valid_resource_sample(self.resources_before)
            || !valid_resource_sample(self.resources_after)
        {
            return Err(WindowsSemanticProbeValidationError::Bounds);
        }
        if self.teardown.retained_native_views > 1 || self.teardown.cleanup_ms > MAX_CLEANUP_MS {
            return Err(WindowsSemanticProbeValidationError::Bounds);
        }
        Ok(())
    }
}

const fn valid_resource_sample(sample: WindowsSemanticResourceEvidence) -> bool {
    sample.webview2_processes != 0
        && sample.webview2_processes <= MAX_RESOURCE_HELPER_PROCESSES
        && sample.resident_bytes != 0
        && sample.resident_bytes <= MAX_RESOURCE_RESIDENT_BYTES
}

/// Closed semantic-probe failure code with no native or page-controlled detail.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowsSemanticProbeFailureCode {
    /// Executable arguments or one-shot identity were invalid.
    InvalidRequest,
    /// Fixed environment, profile, view, policy, or fixture construction failed.
    HarnessFailure,
    /// A closed semantic or lifecycle assertion did not hold.
    VerificationFailed,
    /// One absolute deadline expired.
    TimedOut,
    /// The debugger-only mode lacked an attached debugger.
    DebuggerRequired,
    /// A non-debugger mode unexpectedly ran under a debugger.
    DebuggerForbidden,
    /// Native view, process, profile, callback, or fixture teardown was incomplete.
    TeardownIncomplete,
}

/// Closed stage for a semantic-probe rejection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowsSemanticProbeStage {
    /// Literal argument and one-shot admission.
    Admit,
    /// Ephemeral environment and production view construction.
    Construct,
    /// Fixed loopback navigation.
    Navigate,
    /// Bounded semantic invocation.
    Observe,
    /// Native hidden suspend, readback, and resume.
    Suspend,
    /// Fault injection or recovery.
    Fault,
    /// Closed fixture assertion.
    Verify,
    /// Exact resource retirement.
    Teardown,
}

/// Serialized typed rejection from one physical semantic run.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WindowsSemanticProbeFailure {
    /// Content-free failure class.
    pub code: WindowsSemanticProbeFailureCode,
    /// Pipeline stage that settled the run.
    pub stage: WindowsSemanticProbeStage,
    /// Whether a wholly new run may be attempted.
    pub retryable: bool,
}

/// One exact terminal semantic-probe payload.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum WindowsSemanticProbeReply {
    /// Run returned bounded evidence, independently qualified afterward.
    Completed(Box<WindowsSemanticProbeEvidence>),
    /// Run failed with a closed rejection.
    Rejected(WindowsSemanticProbeFailure),
}

/// Versioned one-shot semantic-probe response.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WindowsSemanticProbeResponse {
    /// Exact grammar version.
    pub protocol_version: u16,
    /// Exact one-shot request identity.
    pub request_id: u64,
    /// Terminal reply.
    pub reply: WindowsSemanticProbeReply,
}

/// Encodes one canonical bounded JSONL semantic-probe response.
pub fn encode_windows_semantic_probe_response(
    response: &WindowsSemanticProbeResponse,
) -> Result<Vec<u8>, WindowsSemanticProbeProtocolError> {
    if response.protocol_version != WINDOWS_SEMANTIC_PROBE_PROTOCOL_VERSION
        || response.request_id == 0
    {
        return Err(WindowsSemanticProbeProtocolError::Identity);
    }
    let mut bytes = serde_json::to_vec(response)
        .map_err(|_| WindowsSemanticProbeProtocolError::InvalidResponse)?;
    bytes.push(b'\n');
    if bytes.len() > MAX_WINDOWS_SEMANTIC_PROBE_OUTPUT_BYTES {
        return Err(WindowsSemanticProbeProtocolError::Limit);
    }
    Ok(bytes)
}

/// Decodes only the canonical bounded JSONL spelling produced above.
pub fn decode_windows_semantic_probe_response(
    bytes: &[u8],
) -> Result<WindowsSemanticProbeResponse, WindowsSemanticProbeProtocolError> {
    if bytes.is_empty()
        || bytes.len() > MAX_WINDOWS_SEMANTIC_PROBE_OUTPUT_BYTES
        || bytes.last() != Some(&b'\n')
        || bytes[..bytes.len() - 1].contains(&b'\n')
        || bytes.contains(&b'\r')
    {
        return Err(WindowsSemanticProbeProtocolError::Limit);
    }
    let response: WindowsSemanticProbeResponse = serde_json::from_slice(&bytes[..bytes.len() - 1])
        .map_err(|_| WindowsSemanticProbeProtocolError::InvalidResponse)?;
    if response.protocol_version != WINDOWS_SEMANTIC_PROBE_PROTOCOL_VERSION
        || response.request_id == 0
    {
        return Err(WindowsSemanticProbeProtocolError::Identity);
    }
    if encode_windows_semantic_probe_response(&response)? != bytes {
        return Err(WindowsSemanticProbeProtocolError::NonCanonical);
    }
    Ok(response)
}

/// Content-free aggregate for one exactly qualified physical mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WindowsSemanticProbeAggregate {
    mode: WindowsSemanticProbeMode,
    snapshots: u8,
    document_epochs: u8,
    suspend_ms: u32,
    elapsed_ms: u64,
    cleanup_ms: u32,
    maximum_webview2_processes: u8,
    maximum_resident_bytes: u64,
    status: WindowsSemanticProbeAggregateStatus,
}

/// Terminal state of a qualified semantic aggregate.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum WindowsSemanticProbeAggregateStatus {
    Qualified,
}

impl WindowsSemanticProbeAggregate {
    /// Qualified closed mode.
    pub const fn mode(&self) -> WindowsSemanticProbeMode {
        self.mode
    }
}

/// Structural semantic evidence failure.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum WindowsSemanticProbeValidationError {
    /// Run identity was zero.
    #[error("Windows semantic-probe identity is invalid")]
    Identity,
    /// Runtime fingerprint did not identify the production Windows adapter.
    #[error("Windows semantic-probe runtime is invalid")]
    Runtime,
    /// A count or duration exceeded the closed evidence ceiling.
    #[error("Windows semantic-probe evidence exceeds its bounds")]
    Bounds,
}

/// Canonical response encoding failure.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum WindowsSemanticProbeProtocolError {
    /// Version or request identity was invalid.
    #[error("Windows semantic-probe response identity is invalid")]
    Identity,
    /// JSON did not decode through the closed response schema.
    #[error("Windows semantic-probe response is invalid")]
    InvalidResponse,
    /// Record exceeded its byte/line bounds.
    #[error("Windows semantic-probe response exceeds its limit")]
    Limit,
    /// Record was valid JSON but not the canonical producer spelling.
    #[error("Windows semantic-probe response is noncanonical")]
    NonCanonical,
}

/// Exact mode-qualification failure.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum WindowsSemanticProbeQualificationError {
    /// Structural evidence validation failed.
    #[error("Windows semantic-probe evidence is invalid")]
    Evidence(#[from] WindowsSemanticProbeValidationError),
    /// Common profile, isolation, viewport, focus, or teardown facts failed.
    #[error("Windows semantic-probe common invariant failed")]
    CommonInvariant,
    /// Result mode or its exact expected observations differed.
    #[error("Windows semantic-probe mode evidence is invalid")]
    Mode,
}

/// Qualifies one exact physical mode and returns only a content-free aggregate.
pub fn qualify_windows_semantic_probe_evidence(
    expected_mode: WindowsSemanticProbeMode,
    evidence: &WindowsSemanticProbeEvidence,
) -> Result<WindowsSemanticProbeAggregate, WindowsSemanticProbeQualificationError> {
    evidence.validate()?;
    if evidence.mode != expected_mode
        || !evidence.ephemeral_profile
        || !evidence.in_private
        || !evidence.extensions_absent
        || !evidence.presentation_hidden
        || evidence.viewport_width != 1_280
        || evidence.viewport_height != 800
        || !evidence.loopback_only
        || !evidence.page_world_bridge_absent
        || !evidence.secrets_redacted
        || evidence.focus_theft_observed
        || evidence.peak_pending_invocations != 1
        || !evidence.semantic_work_drained
        || !evidence.teardown.runtime_retired
        || !evidence.teardown.view_closed
        || !evidence.teardown.browser_process_exited
        || !evidence.teardown.profile_removed
        || !evidence.teardown.fixture_drained
        || !evidence.teardown.work_drained
        || evidence.teardown.retained_native_views != 0
    {
        return Err(WindowsSemanticProbeQualificationError::CommonInvariant);
    }

    let mode_valid = match expected_mode {
        WindowsSemanticProbeMode::HiddenFixedDocuments => {
            fixed_documents(evidence)
                && no_suspend_evidence(evidence)
                && no_redirect_evidence(evidence)
                && no_location_evidence(evidence)
                && !evidence.debugger_attached
        }
        WindowsSemanticProbeMode::HiddenRedirectLifecycle => {
            !evidence.debugger_attached
                && evidence.snapshots == 2
                && evidence.document_epochs == 2
                && evidence.first_snapshot_verified
                && !evidence.replacement_snapshot_verified
                && !evidence.replacement_stale_state_absent
                && !evidence.event_flood_refused
                && !evidence.recovered_after_event_flood
                && !evidence.renderer_loss_observed
                && !evidence.renderer_lost_refused
                && no_suspend_evidence(evidence)
                && evidence.redirect_chain_verified
                && evidence.redirect_chain_hops_observed == 2
                && evidence.redirect_limit_refused
                && evidence.redirect_limit_hops_observed == MAX_CONTEXT_NAVIGATION_REDIRECTS as u8
                && evidence.redirect_recovery_verified
                && no_location_evidence(evidence)
        }
        WindowsSemanticProbeMode::HiddenLocationReplacement => {
            !evidence.debugger_attached
                && evidence.snapshots == 3
                && evidence.document_epochs == 2
                && evidence.first_snapshot_verified
                && !evidence.replacement_snapshot_verified
                && !evidence.replacement_stale_state_absent
                && !evidence.event_flood_refused
                && !evidence.recovered_after_event_flood
                && !evidence.renderer_loss_observed
                && !evidence.renderer_lost_refused
                && no_suspend_evidence(evidence)
                && no_redirect_evidence(evidence)
                && evidence.same_document_replacement_observed
                && evidence.same_document_replacement_rejoined
                && evidence.stale_location_join_refused
                && evidence.post_location_snapshot_verified
        }
        WindowsSemanticProbeMode::HiddenSuspendResume => {
            !evidence.debugger_attached
                && evidence.snapshots == 2
                && evidence.document_epochs == 1
                && evidence.first_snapshot_verified
                && !evidence.replacement_snapshot_verified
                && !evidence.replacement_stale_state_absent
                && !evidence.event_flood_refused
                && !evidence.recovered_after_event_flood
                && !evidence.renderer_loss_observed
                && !evidence.renderer_lost_refused
                && evidence.suspend_callback_succeeded
                && evidence.suspended_state_attested
                && evidence.resume_state_attested
                && evidence.post_resume_snapshot_verified
                && no_redirect_evidence(evidence)
                && no_location_evidence(evidence)
        }
        WindowsSemanticProbeMode::HiddenDebuggerCoexistence => {
            fixed_documents(evidence)
                && no_suspend_evidence(evidence)
                && no_redirect_evidence(evidence)
                && no_location_evidence(evidence)
                && evidence.debugger_attached
        }
        WindowsSemanticProbeMode::HiddenEventFlood => {
            !evidence.debugger_attached
                && evidence.snapshots == 2
                && evidence.document_epochs == 3
                && evidence.first_snapshot_verified
                && evidence.replacement_snapshot_verified
                && evidence.replacement_stale_state_absent
                && evidence.event_flood_refused
                && evidence.recovered_after_event_flood
                && !evidence.renderer_loss_observed
                && !evidence.renderer_lost_refused
                && no_suspend_evidence(evidence)
                && no_redirect_evidence(evidence)
                && no_location_evidence(evidence)
        }
        WindowsSemanticProbeMode::HiddenRendererLoss => {
            !evidence.debugger_attached
                && evidence.snapshots == 1
                && evidence.document_epochs == 1
                && evidence.first_snapshot_verified
                && !evidence.replacement_snapshot_verified
                && !evidence.replacement_stale_state_absent
                && !evidence.event_flood_refused
                && !evidence.recovered_after_event_flood
                && evidence.renderer_loss_observed
                && evidence.renderer_lost_refused
                && no_suspend_evidence(evidence)
                && no_redirect_evidence(evidence)
                && no_location_evidence(evidence)
        }
    };
    if !mode_valid {
        return Err(WindowsSemanticProbeQualificationError::Mode);
    }

    Ok(WindowsSemanticProbeAggregate {
        mode: evidence.mode,
        snapshots: evidence.snapshots,
        document_epochs: evidence.document_epochs,
        suspend_ms: evidence.suspend_ms,
        elapsed_ms: evidence.elapsed_ms,
        cleanup_ms: evidence.teardown.cleanup_ms,
        maximum_webview2_processes: evidence
            .resources_before
            .webview2_processes
            .max(evidence.resources_after.webview2_processes),
        maximum_resident_bytes: evidence
            .resources_before
            .resident_bytes
            .max(evidence.resources_after.resident_bytes),
        status: WindowsSemanticProbeAggregateStatus::Qualified,
    })
}

fn fixed_documents(evidence: &WindowsSemanticProbeEvidence) -> bool {
    evidence.snapshots == 2
        && evidence.document_epochs == 2
        && evidence.first_snapshot_verified
        && evidence.replacement_snapshot_verified
        && evidence.replacement_stale_state_absent
        && !evidence.event_flood_refused
        && !evidence.recovered_after_event_flood
        && !evidence.renderer_loss_observed
        && !evidence.renderer_lost_refused
}

fn no_suspend_evidence(evidence: &WindowsSemanticProbeEvidence) -> bool {
    !evidence.suspend_callback_succeeded
        && !evidence.suspended_state_attested
        && !evidence.resume_state_attested
        && !evidence.post_resume_snapshot_verified
        && evidence.suspend_ms == 0
}

fn no_redirect_evidence(evidence: &WindowsSemanticProbeEvidence) -> bool {
    !evidence.redirect_chain_verified
        && evidence.redirect_chain_hops_observed == 0
        && !evidence.redirect_limit_refused
        && evidence.redirect_limit_hops_observed == 0
        && !evidence.redirect_recovery_verified
}

fn no_location_evidence(evidence: &WindowsSemanticProbeEvidence) -> bool {
    !evidence.same_document_replacement_observed
        && !evidence.same_document_replacement_rejoined
        && !evidence.stale_location_join_refused
        && !evidence.post_location_snapshot_verified
}

#[cfg(test)]
pub(crate) fn tests_fixture(mode: WindowsSemanticProbeMode) -> WindowsSemanticProbeEvidence {
    use crate::EvidenceLabel;

    let flood = mode == WindowsSemanticProbeMode::HiddenEventFlood;
    let renderer = mode == WindowsSemanticProbeMode::HiddenRendererLoss;
    let suspension = mode == WindowsSemanticProbeMode::HiddenSuspendResume;
    let redirects = mode == WindowsSemanticProbeMode::HiddenRedirectLifecycle;
    let location = mode == WindowsSemanticProbeMode::HiddenLocationReplacement;
    WindowsSemanticProbeEvidence {
        run_id: 1,
        runtime: RuntimeFingerprint {
            platform: Platform::Windows,
            os_version: EvidenceLabel::new("10.0.26100").expect("OS label"),
            engine: EvidenceLabel::new("WebView2").expect("engine label"),
            engine_version: EvidenceLabel::new("140.0.0.0").expect("version label"),
            adapter_revision: EvidenceLabel::new(
                "semantic-runtime-m3-lifecycle-m2-redirect-location-resources-v2",
            )
            .expect("adapter label"),
        },
        mode,
        ephemeral_profile: true,
        in_private: true,
        extensions_absent: true,
        presentation_hidden: true,
        viewport_width: 1_280,
        viewport_height: 800,
        loopback_only: true,
        snapshots: if renderer {
            1
        } else if location {
            3
        } else {
            2
        },
        document_epochs: if flood {
            3
        } else if renderer || suspension {
            1
        } else {
            2
        },
        first_snapshot_verified: true,
        replacement_snapshot_verified: !renderer && !suspension && !redirects && !location,
        replacement_stale_state_absent: !renderer && !suspension && !redirects && !location,
        page_world_bridge_absent: true,
        secrets_redacted: true,
        event_flood_refused: flood,
        recovered_after_event_flood: flood,
        renderer_loss_observed: renderer,
        renderer_lost_refused: renderer,
        suspend_callback_succeeded: suspension,
        suspended_state_attested: suspension,
        resume_state_attested: suspension,
        post_resume_snapshot_verified: suspension,
        suspend_ms: if suspension { 25 } else { 0 },
        redirect_chain_verified: redirects,
        redirect_chain_hops_observed: if redirects { 2 } else { 0 },
        redirect_limit_refused: redirects,
        redirect_limit_hops_observed: if redirects {
            MAX_CONTEXT_NAVIGATION_REDIRECTS as u8
        } else {
            0
        },
        redirect_recovery_verified: redirects,
        same_document_replacement_observed: location,
        same_document_replacement_rejoined: location,
        stale_location_join_refused: location,
        post_location_snapshot_verified: location,
        debugger_attached: mode == WindowsSemanticProbeMode::HiddenDebuggerCoexistence,
        focus_theft_observed: false,
        peak_pending_invocations: 1,
        semantic_work_drained: true,
        resources_before: WindowsSemanticResourceEvidence {
            webview2_processes: 4,
            resident_bytes: 256 * 1_024 * 1_024,
        },
        resources_after: WindowsSemanticResourceEvidence {
            webview2_processes: 5,
            resident_bytes: 320 * 1_024 * 1_024,
        },
        elapsed_ms: 800,
        teardown: WindowsSemanticTeardownEvidence {
            runtime_retired: true,
            view_closed: true,
            browser_process_exited: true,
            profile_removed: true,
            fixture_drained: true,
            work_drained: true,
            retained_native_views: 0,
            cleanup_ms: 50,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_required_mode_has_one_exact_qualification() {
        for mode in WINDOWS_SEMANTIC_PHYSICAL_REVIEW_MODES {
            let evidence = tests_fixture(mode);
            let aggregate =
                qualify_windows_semantic_probe_evidence(mode, &evidence).expect("qualified");
            assert_eq!(aggregate.mode(), mode);
            assert_eq!(aggregate.maximum_webview2_processes, 5);
            assert_eq!(aggregate.maximum_resident_bytes, 320 * 1_024 * 1_024);
            assert_eq!(
                WindowsSemanticProbeMode::from_argument(mode.argument()),
                Some(mode)
            );
        }
    }

    #[test]
    fn response_round_trip_is_bounded_and_canonical() {
        let response = WindowsSemanticProbeResponse {
            protocol_version: WINDOWS_SEMANTIC_PROBE_PROTOCOL_VERSION,
            request_id: 1,
            reply: WindowsSemanticProbeReply::Completed(Box::new(tests_fixture(
                WindowsSemanticProbeMode::HiddenFixedDocuments,
            ))),
        };
        let encoded = encode_windows_semantic_probe_response(&response).expect("encode");
        assert_eq!(
            decode_windows_semantic_probe_response(&encoded).expect("decode"),
            response
        );
        let mut noncanonical = encoded;
        noncanonical.insert(0, b' ');
        assert_eq!(
            decode_windows_semantic_probe_response(&noncanonical),
            Err(WindowsSemanticProbeProtocolError::NonCanonical)
        );
    }

    #[test]
    fn substitutions_focus_and_teardown_fail_closed() {
        let mode = WindowsSemanticProbeMode::HiddenFixedDocuments;
        let mut evidence = tests_fixture(mode);
        evidence.mode = WindowsSemanticProbeMode::HiddenEventFlood;
        assert_eq!(
            qualify_windows_semantic_probe_evidence(mode, &evidence),
            Err(WindowsSemanticProbeQualificationError::CommonInvariant)
        );

        let mut evidence = tests_fixture(mode);
        evidence.focus_theft_observed = true;
        assert_eq!(
            qualify_windows_semantic_probe_evidence(mode, &evidence),
            Err(WindowsSemanticProbeQualificationError::CommonInvariant)
        );

        let mut evidence = tests_fixture(mode);
        evidence.teardown.profile_removed = false;
        assert_eq!(
            qualify_windows_semantic_probe_evidence(mode, &evidence),
            Err(WindowsSemanticProbeQualificationError::CommonInvariant)
        );

        let mut evidence = tests_fixture(mode);
        evidence.resources_before.webview2_processes = 0;
        assert_eq!(
            qualify_windows_semantic_probe_evidence(mode, &evidence),
            Err(WindowsSemanticProbeQualificationError::Evidence(
                WindowsSemanticProbeValidationError::Bounds,
            ))
        );

        let mut evidence = tests_fixture(mode);
        evidence.resources_after.resident_bytes = MAX_RESOURCE_RESIDENT_BYTES + 1;
        assert_eq!(
            qualify_windows_semantic_probe_evidence(mode, &evidence),
            Err(WindowsSemanticProbeQualificationError::Evidence(
                WindowsSemanticProbeValidationError::Bounds,
            ))
        );
    }

    #[test]
    fn debugger_and_fault_modes_cannot_cross() {
        let mut debugger = tests_fixture(WindowsSemanticProbeMode::HiddenDebuggerCoexistence);
        debugger.debugger_attached = false;
        assert_eq!(
            qualify_windows_semantic_probe_evidence(
                WindowsSemanticProbeMode::HiddenDebuggerCoexistence,
                &debugger,
            ),
            Err(WindowsSemanticProbeQualificationError::Mode)
        );

        let mut renderer = tests_fixture(WindowsSemanticProbeMode::HiddenRendererLoss);
        renderer.renderer_lost_refused = false;
        assert_eq!(
            qualify_windows_semantic_probe_evidence(
                WindowsSemanticProbeMode::HiddenRendererLoss,
                &renderer,
            ),
            Err(WindowsSemanticProbeQualificationError::Mode)
        );

        let mut suspension = tests_fixture(WindowsSemanticProbeMode::HiddenSuspendResume);
        suspension.resume_state_attested = false;
        assert_eq!(
            qualify_windows_semantic_probe_evidence(
                WindowsSemanticProbeMode::HiddenSuspendResume,
                &suspension,
            ),
            Err(WindowsSemanticProbeQualificationError::Mode)
        );

        let mut unexpected_suspend = tests_fixture(WindowsSemanticProbeMode::HiddenFixedDocuments);
        unexpected_suspend.suspend_ms = 1;
        assert_eq!(
            qualify_windows_semantic_probe_evidence(
                WindowsSemanticProbeMode::HiddenFixedDocuments,
                &unexpected_suspend,
            ),
            Err(WindowsSemanticProbeQualificationError::Mode)
        );

        let mut unbounded_suspend = tests_fixture(WindowsSemanticProbeMode::HiddenSuspendResume);
        unbounded_suspend.suspend_ms = 10_001;
        assert_eq!(
            qualify_windows_semantic_probe_evidence(
                WindowsSemanticProbeMode::HiddenSuspendResume,
                &unbounded_suspend,
            ),
            Err(WindowsSemanticProbeQualificationError::Evidence(
                WindowsSemanticProbeValidationError::Bounds,
            ))
        );

        let redirect_mode = WindowsSemanticProbeMode::HiddenRedirectLifecycle;
        let mut incomplete_redirect = tests_fixture(redirect_mode);
        incomplete_redirect.redirect_limit_hops_observed -= 1;
        assert_eq!(
            qualify_windows_semantic_probe_evidence(redirect_mode, &incomplete_redirect),
            Err(WindowsSemanticProbeQualificationError::Mode)
        );

        let mut smuggled_redirect = tests_fixture(WindowsSemanticProbeMode::HiddenFixedDocuments);
        smuggled_redirect.redirect_chain_verified = true;
        assert_eq!(
            qualify_windows_semantic_probe_evidence(
                WindowsSemanticProbeMode::HiddenFixedDocuments,
                &smuggled_redirect,
            ),
            Err(WindowsSemanticProbeQualificationError::Mode)
        );

        let location_mode = WindowsSemanticProbeMode::HiddenLocationReplacement;
        let mut incomplete_location = tests_fixture(location_mode);
        incomplete_location.stale_location_join_refused = false;
        assert_eq!(
            qualify_windows_semantic_probe_evidence(location_mode, &incomplete_location),
            Err(WindowsSemanticProbeQualificationError::Mode)
        );

        let mut smuggled_location = tests_fixture(WindowsSemanticProbeMode::HiddenFixedDocuments);
        smuggled_location.same_document_replacement_observed = true;
        assert_eq!(
            qualify_windows_semantic_probe_evidence(
                WindowsSemanticProbeMode::HiddenFixedDocuments,
                &smuggled_location,
            ),
            Err(WindowsSemanticProbeQualificationError::Mode)
        );
    }
}
