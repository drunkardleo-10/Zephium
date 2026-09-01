//! Bounded domain contracts for Zephium's browser-execution proof.
//!
//! The default contract deliberately contains no browser engine, native
//! object, page script, selector, navigable page URL, profile path, provider
//! secret, or model adapter. It defines the closed vocabulary shared by the
//! non-shipping probe and the eventual product ports. Canonical origins are
//! retained only as native-attested provenance with redacted diagnostics.
//! Diagnostic fixtures and their fixed recipe enum are behind `probe-harness`
//! and cannot be compiled in an optimized build.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

#[cfg(all(feature = "probe-harness", not(debug_assertions)))]
compile_error!("the agentic probe harness is forbidden in optimized builds");

mod agent_audit;
mod agent_manifest;
mod agent_policy;
mod agent_provider;
mod agent_supervisor;
mod context;
mod context_port;
mod context_registry;
#[cfg(feature = "probe-harness")]
mod contract;
#[cfg(feature = "probe-harness")]
mod control;
mod cookie_transfer;
#[cfg(feature = "probe-harness")]
mod evidence;
#[cfg(feature = "probe-harness")]
mod fixture_server;
#[cfg(feature = "probe-harness")]
mod probe_recipes;
mod profile_lease;
#[cfg(feature = "probe-harness")]
mod protocol;
mod semantic;
mod semantic_action;
mod semantic_action_batch_result;
mod semantic_action_result;
mod semantic_diff;
mod semantic_diff_model;
mod semantic_extract;
mod semantic_model;
mod semantic_observation;
mod semantic_read;
mod semantic_read_model;
mod semantic_runtime;
mod semantic_screenshot;
mod semantic_settle;
mod semantic_verify;
mod semantic_wire;
mod sign_in_handoff;

pub use agent_audit::{
    AgentAuditCompletion, AgentAuditDelivery, AgentAuditDeliveryId, AgentAuditDeliveryOutcome,
    AgentAuditDeliveryProof, AgentAuditDeliverySettlement, AgentAuditDispatch, AgentAuditError,
    AgentAuditEvent, AgentAuditEventId, AgentAuditLedger, AgentAuditLedgerStatus, AgentAuditPort,
    AgentAuditRecordV1, AgentAuditSinkFailure, AGENT_AUDIT_RECORD_V1_BYTES,
    MAX_AGENT_AUDIT_DELIVERY_EVENTS, MAX_PENDING_AGENT_AUDIT_EVENTS,
};
pub use agent_manifest::{
    AgentAccountAttestationId, AgentAccountId, AgentAccountScope, AgentContextAccountBinding,
    AgentDataFlowRule, AgentEffectScope, AgentManifestContractError, AgentPlanLeaseId,
    AgentPlanNodeAuthority, AgentPlanNodeId, AgentPlanNodeScope, AgentPolicyInstant,
    AgentRunBudget, AgentRunManifest, AgentRunManifestId, AgentRunScope, MAX_AGENT_DATA_FLOW_RULES,
    MAX_AGENT_PLAN_NODES, MAX_AGENT_RUN_ACCOUNTS, MAX_AGENT_RUN_CONTEXTS,
    MAX_AGENT_RUN_COST_MICRO_USD, MAX_AGENT_RUN_LIFETIME_MILLIS, MAX_AGENT_RUN_MODEL_TOKENS,
    MAX_AGENT_RUN_OPERATIONS, MAX_AGENT_RUN_ORIGINS, MAX_AGENT_RUN_PROFILES,
};
pub use agent_policy::{
    AgentActiveEffect, AgentActiveModelCall, AgentEffectAssessment, AgentEffectAuthorization,
    AgentEffectCancellation, AgentEffectDispatchRequest, AgentEffectId, AgentEffectPermit,
    AgentEffectReceipt, AgentEffectRequest, AgentEffectSettlement, AgentModelCallAdmission,
    AgentModelCallBudget, AgentModelCallId, AgentModelCallReceipt, AgentModelCallRequest,
    AgentModelCallSettlement, AgentModelCallUnaccountedSettlement, AgentModelInputCancellation,
    AgentModelUsageAccounting, AgentNeedsHumanReason, AgentNeedsHumanTransition,
    AgentPlanLeaseBinding, AgentPolicyAccounting, AgentPolicyError, AgentRunPolicy,
    AgentTaintCohort, MAX_AGENT_ACCOUNT_ATTESTATION_AGE_MILLIS, MAX_AGENT_PENDING_EFFECTS,
    MAX_AGENT_PENDING_MODEL_CALLS, MAX_AGENT_TAINT_COHORTS, MAX_AGENT_TAINT_REFERENCES,
};
pub use agent_provider::{
    AgentBrowserActProposal, AgentBrowserHumanReason, AgentBrowserScopeProposal,
    AgentBrowserSemanticQuery, AgentBrowserToolCall, AgentBrowserToolCallId,
    AgentBrowserToolContractError, AgentBrowserToolKind, AgentBrowserToolProposal,
    AgentCommittedProviderRequest, AgentPreparedObservationRequest, AgentPreparedReadRequest,
    AgentProviderCallConfig, AgentProviderCallIdentity, AgentProviderCompletion,
    AgentProviderContractError, AgentProviderEndpoint, AgentProviderFailure,
    AgentProviderFailureClass, AgentProviderInputOutcome, AgentProviderKind,
    AgentProviderModelRevision, AgentProviderObjective, AgentProviderObjectiveError,
    AgentProviderProtocolError, AgentProviderRequest, AgentProviderRequestError,
    AgentProviderRequestSettlement, AgentProviderRetryAfter, AgentProviderRetryDisposition,
    AgentProviderStopReason, AgentProviderStreamBatch, AgentProviderStreamBudget,
    AgentProviderStreamConclusion, AgentProviderStreamDecoder, AgentProviderStreamEvent,
    AgentProviderStreamStats, AgentProviderTerminalFailure, AgentProviderTextDelta,
    AgentProviderTransportInput, AgentProviderUsage, MAX_AGENT_BROWSER_NAVIGATION_URL_BYTES,
    MAX_AGENT_BROWSER_SEMANTIC_QUERY_BYTES, MAX_AGENT_PROVIDER_MODEL_REVISION_BYTES,
    MAX_AGENT_PROVIDER_OBJECTIVE_BYTES, MAX_AGENT_PROVIDER_OBJECTIVE_TOKENS,
    MAX_AGENT_PROVIDER_OUTPUT_TEXT_BYTES, MAX_AGENT_PROVIDER_REQUEST_BYTES,
    MAX_AGENT_PROVIDER_RETRY_AFTER_MILLIS, MAX_AGENT_PROVIDER_SSE_EVENT_BYTES,
    MAX_AGENT_PROVIDER_SSE_LINE_BYTES, MAX_AGENT_PROVIDER_STREAM_EVENTS,
    MAX_AGENT_PROVIDER_STREAM_WIRE_BYTES, MAX_AGENT_PROVIDER_TOOL_ARGUMENT_BYTES,
    MAX_AGENT_PROVIDER_TOOL_CALLS, MAX_AGENT_PROVIDER_TOOL_CALL_ID_BYTES,
};
pub use agent_supervisor::{
    AgentDelegationNode, AgentDelegationSpec, AgentDelegationTopology, AgentNodeExecution,
    AgentProgressActivity, AgentProgressBlocker, AgentProgressOperation, AgentProgressResource,
    AgentProgressResult, AgentProgressState, AgentRunSupervisor, AgentSemanticProgress,
    AgentSupervisorAttemptId, AgentSupervisorCancellation, AgentSupervisorCancellationBatch,
    AgentSupervisorCancellationId, AgentSupervisorCancellationReason,
    AgentSupervisorCancellationTarget, AgentSupervisorCompletion, AgentSupervisorContextAssignment,
    AgentSupervisorContextCancellationTarget, AgentSupervisorContextRelease,
    AgentSupervisorContextReleaseOutcome, AgentSupervisorContractError,
    AgentSupervisorExecutionOutcome, AgentSupervisorExecutionReceipt, AgentSupervisorFailure,
    AgentSupervisorId, AgentSupervisorNodeCancellation, AgentSupervisorNodeSnapshot,
    AgentSupervisorNodeStatus, AgentSupervisorRuntimeError, AgentSupervisorRuntimeStatus,
    AgentSupervisorWait, MAX_AGENT_DELEGATION_DEPTH, MAX_AGENT_EXECUTING_SUPERVISOR_NODES,
    MAX_AGENT_LIVE_SUPERVISOR_NODES,
};

pub use context::{
    ContextAutomationState, ContextCapabilities, ContextCapability, ContextCapabilityError,
    ContextControl, ContextFreshness, ContextGeneration, ContextId, ContextIdentity, ContextJoin,
    ContextKind, ContextLifecycle, ContextOperationId, ContextOperationJoin, ContextOperationKind,
    ContextRunId, ContextSettlement, ContextStatus, ContextTerminal, ContextTransitionError,
    ContextVisibility, FrameGeneration, FrameId, NavigationEpoch, RunCancellationGeneration,
};
pub use context_port::{
    AgentBrowserPort, BorrowedTabLeaseId, ContextCancellationRequest,
    ContextCancellationSettlement, ContextConstructionProof, ContextConstructionRequest,
    ContextConstructionSettlement, ContextConstructionSource, ContextDispatch, ContextNativeEvent,
    ContextNativePlatform, ContextNativeRequest, ContextNativeResourceCounts,
    ContextNativeResourceSnapshot, ContextNavigationReplacement, ContextNavigationRequest,
    ContextNavigationSettlement, ContextNavigationTarget, ContextPortContractError,
    ContextPortFailure, ContextRendererLoss, ContextResourceAuditId,
    ContextResourceAuditSettlement, ContextTransitionRequest, ContextTransitionSettlement,
    MAX_PENDING_NATIVE_CONTEXT_TASKS,
};
pub use context_registry::{
    ContextRegistry, ContextRegistryEntry, ContextRegistryEntryState, ContextRegistryError,
    ContextRegistryStatus, ContextResourceDisposition, RetiredContext, MAX_EXECUTING_CONTEXTS,
    MAX_LIVE_CONTEXTS,
};
#[cfg(feature = "probe-harness")]
pub use contract::{
    FixtureCase, FixtureTarget, FocusOwner, GateOutcome, InputBackend, InputEventKind,
    PresentationState,
};
#[cfg(feature = "probe-harness")]
pub use control::{ProbeAdmissionError, ProbeGate, ProbeRunPermit};
pub use cookie_transfer::{
    ContextCookieOrigin, ContextCookieScope, ContextCookieTransferCounts,
    ContextCookieTransferDirection, ContextCookieTransferError, ContextCookieTransferFailure,
    ContextCookieTransferId, ContextCookieTransferOutcome, ContextCookieTransferRegistry,
    ContextCookieTransferRegistryStatus, ContextCookieTransferRequest,
    ContextCookieTransferSettlement, ContextCookieTransferStats, MAX_COOKIES_PER_TRANSFER,
    MAX_COOKIE_BYTES, MAX_COOKIE_TRANSFER_BYTES, MAX_COOKIE_TRANSFER_ORIGINS,
    MAX_PENDING_COOKIE_TRANSFERS,
};
#[cfg(feature = "probe-harness")]
pub use evidence::{
    ActivationEvidence, BackendAvailability, BackendCapability, CaseEvidence, CaseOutcome,
    EvidenceLabel, EvidenceValidationError, FocusEvidence, InputEventEvidence, Platform,
    ProbeFailure, ProbeFailureCode, ProbeStage, ResourceEvidence, RunEvidence, RuntimeFingerprint,
    TargetEvidence, TeardownEvidence, MAX_CASE_EVIDENCE, MAX_EVENT_EVIDENCE,
};
#[cfg(feature = "probe-harness")]
pub use fixture_server::{FixtureRoute, FixtureServer, FixtureServerError};
#[cfg(feature = "probe-harness")]
pub use probe_recipes::{
    windows_input_plan, FixedProbeScript, ProbeScriptWorld, WindowsInputStep, WindowsProbeGeometry,
    WindowsProbeKey, WindowsProbePoint, MACOS_NATIVE_INPUT_RUNTIME_V1,
    MACOS_PROBE_CONTENT_WORLD_V1, MACOS_PROBE_HANDLER_V1, MAX_NATIVE_INPUT_RUNTIME_ROW,
    NATIVE_INPUT_RUNTIME_PROTOCOL_V1,
};
pub use profile_lease::{
    ContextProfileLease, ContextProfileLeaseError, ContextProfileLeaseId,
    ContextProfileLeasePurpose, ContextProfileLeaseRegistry, ContextProfileLeaseStatus,
    MAX_CONTEXT_PROFILE_TOMBSTONES,
};
#[cfg(feature = "probe-harness")]
pub use protocol::{
    decode_request_line, encode_response_line, CancelRequest, CancelledReply, HelloReply,
    HelloRequest, ProbeCommand, ProbeProtocolError, ProbeReply, ProbeRequest, ProbeResponse,
    RunMatrixRequest, ShutdownReply, ShutdownRequest, MAX_BACKENDS_PER_REQUEST,
    MAX_CASES_PER_REQUEST, MAX_PROTOCOL_INPUT_BYTES, MAX_PROTOCOL_OUTPUT_BYTES,
    PROBE_PROTOCOL_VERSION,
};
pub use semantic::{
    SemanticCompleteness, SemanticContractError, SemanticFrameJoin, SemanticFrameTrust,
    SemanticHeadingLevel, SemanticInvocationId, SemanticNode, SemanticOperationClass,
    SemanticOperations, SemanticOrigin, SemanticRect, SemanticReference, SemanticReferenceError,
    SemanticReferenceId, SemanticRole, SemanticSensitivity, SemanticSnapshot,
    SemanticSnapshotGeneration, SemanticState, SemanticStates, SemanticText, SemanticTruncation,
    SemanticTrust, SemanticValueSummary, MAX_SEMANTIC_DEPTH, MAX_SEMANTIC_FRAMES,
    MAX_SEMANTIC_NAME_BYTES, MAX_SEMANTIC_NODES, MAX_SEMANTIC_TEXT_BYTES,
    MAX_SEMANTIC_TOTAL_TEXT_BYTES, MAX_SEMANTIC_VALUE_BYTES,
};
pub use semantic_action::{
    SemanticActionBatch, SemanticActionBatchId, SemanticActionBindingError,
    SemanticActionContractError, SemanticActionIntent, SemanticActionKind,
    SemanticActionPreparationError, SemanticActionProposal, SemanticActionRevalidationError,
    SemanticActionText, SemanticActionTextError, SemanticBoundAction, SemanticDialogState,
    SemanticEffectClass, SemanticMutationQuietPeriod, SemanticPreparedAction, SemanticPressKey,
    SemanticScrollAmount, SemanticScrollDirection, SemanticSettleBudget, SemanticVerification,
    SemanticWaitCondition, MAX_SEMANTIC_ACTIONS_PER_BATCH, MAX_SEMANTIC_ACTION_BATCH_SETTLE_MILLIS,
    MAX_SEMANTIC_ACTION_BATCH_TEXT_BYTES, MAX_SEMANTIC_ACTION_SETTLE_MILLIS,
    MAX_SEMANTIC_ACTION_TEXT_BYTES, MAX_SEMANTIC_MUTATION_QUIET_MILLIS,
};
pub use semantic_action_batch_result::{
    SemanticActionBatchCompletion, SemanticActionBatchContinuation, SemanticActionBatchExecution,
    SemanticActionBatchExecutionError, SemanticActionBatchOutcome, SemanticActionBatchResult,
    SemanticActionBatchStopReason,
};
pub use semantic_action_result::{
    finalize_semantic_action_result, SemanticActionNextState, SemanticActionResult,
    SemanticActionResultError, SemanticPostActionObservation,
};
pub use semantic_diff::{
    compute_semantic_diff, SemanticDiff, SemanticDiffBudget, SemanticDiffBudgetError,
    SemanticDiffEntry, SemanticDiffEntryKind, SemanticDiffFrame, SemanticDiffOutcome,
    SemanticDiffStats, SemanticFreshSnapshotReason, SemanticNodeChange, SemanticNodeChanges,
    SemanticNodeMove, SemanticObservationAcknowledgement, SemanticReferenceRebase,
    SemanticRetiredReferenceId, MAX_SEMANTIC_DIFF_ENTRIES,
};
pub use semantic_diff_model::{
    encode_semantic_diff, SemanticDiffEncodingStats, SemanticDiffModelPayload, SemanticEncodedDiff,
    SEMANTIC_DIFF_MODEL_SCHEMA_VERSION,
};
pub use semantic_extract::{
    extract_semantic_read, SemanticExtractedBoolean, SemanticExtractedField, SemanticExtractedText,
    SemanticExtractedTextList, SemanticExtractedUnsigned, SemanticExtractedValue,
    SemanticExtractionError, SemanticExtractionFieldSchema, SemanticExtractionResult,
    SemanticExtractionSchema, SemanticExtractionSchemaError, SemanticExtractionSchemaId,
    SemanticExtractionSource, SemanticExtractionSourceSpan, SemanticExtractionStats,
    SemanticExtractionTrust, SemanticExtractionValueKind, MAX_SEMANTIC_EXTRACTION_FIELDS,
    MAX_SEMANTIC_EXTRACTION_FIELD_NAME_BYTES, MAX_SEMANTIC_EXTRACTION_INPUT_BYTES,
    MAX_SEMANTIC_EXTRACTION_LIST_ITEMS, MAX_SEMANTIC_EXTRACTION_LIST_ITEM_BYTES,
    MAX_SEMANTIC_EXTRACTION_SCHEMA_NAME_BYTES, MAX_SEMANTIC_EXTRACTION_SOURCES_PER_VALUE,
    MAX_SEMANTIC_EXTRACTION_SOURCE_EDGES, MAX_SEMANTIC_EXTRACTION_TEXT_BYTES,
    MAX_SEMANTIC_EXTRACTION_TOTAL_TEXT_BYTES, MAX_SEMANTIC_EXTRACTION_VALUES,
    SEMANTIC_EXTRACTION_SCHEMA_VERSION,
};
pub use semantic_model::{
    encode_semantic_observation, SemanticEncodedObservation, SemanticEncodingStats,
    SemanticModelDeliveryError, SemanticModelDeliverySettlement, SemanticModelEncodingBudget,
    SemanticModelEncodingError, SemanticModelPayload, SemanticTokenCountQuality,
    SemanticTokenCountRequirement, SemanticTokenCounter, SemanticTokenCounterError,
    SemanticTokenMeasurement, SemanticTokenMeasurementError, SemanticTokenizerRevision,
    SemanticTokenizerRevisionError, ACTION_SEMANTIC_DIFF_TOKEN_TARGET,
    INITIAL_SEMANTIC_MODEL_TOKEN_TARGET, MAX_SEMANTIC_MODEL_BYTES, MAX_SEMANTIC_MODEL_TOKENS,
    MAX_SEMANTIC_TOKENIZER_REVISION_BYTES, SEMANTIC_MODEL_SCHEMA_VERSION,
};
pub use semantic_observation::{
    SemanticExpansionKind, SemanticFrameBoundary, SemanticFrameBoundaryStatus,
    SemanticFrameDeferral, SemanticFrameUnsupported, SemanticObservation,
    SemanticObservationAssembler, SemanticObservationBudget, SemanticObservationError,
    SemanticObservationGeneration, SemanticObservationId, SemanticObservationParent,
    SemanticObservationRequest, SemanticScope, SemanticScopeAnchor, SemanticTextWindow,
    MAX_SEMANTIC_OBSERVATION_EXPANSIONS, MAX_SEMANTIC_OBSERVATION_NODES,
    MAX_SEMANTIC_OBSERVATION_TEXT_BYTES, MAX_SEMANTIC_SURROUNDING_TEXT_BYTES,
};
pub use semantic_read::{
    read_semantic_observation, SemanticCaptureInstant, SemanticReadAuthority, SemanticReadBudget,
    SemanticReadBudgetError, SemanticReadContent, SemanticReadError, SemanticReadField,
    SemanticReadFragment, SemanticReadFragmentId, SemanticReadOmission, SemanticReadOmissions,
    SemanticReadProvenance, SemanticReadResult, SemanticReadSensitivityLimit, SemanticReadStats,
    MAX_SEMANTIC_READ_BYTES, MAX_SEMANTIC_READ_ITEMS,
};
pub use semantic_read_model::{
    encode_semantic_read, SemanticEncodedRead, SemanticReadDeliveryReceipt,
    SemanticReadEncodingStats, SemanticReadModelPayload, SEMANTIC_READ_MODEL_SCHEMA_VERSION,
};
pub use semantic_runtime::{
    encode_semantic_runtime_invocation, SemanticRuntimeBudget, SemanticRuntimeBudgetError,
    SemanticRuntimeFault, SemanticRuntimeInvocation, SemanticRuntimeInvocationError,
    SemanticRuntimeProgram, SemanticRuntimeResultError, SemanticRuntimeScopeClass,
    MAX_SEMANTIC_RUNTIME_REQUEST_BYTES, MAX_SEMANTIC_RUNTIME_SAFE_INTEGER,
    MAX_SEMANTIC_RUNTIME_SOURCE_BYTES, MAX_SEMANTIC_RUNTIME_VISITED_NODES,
    MIN_SEMANTIC_RUNTIME_WIRE_BYTES, SEMANTIC_RUNTIME_GLOBAL_NAME, SEMANTIC_RUNTIME_PROGRAM,
    SEMANTIC_RUNTIME_PROTOCOL_VERSION,
};
pub use semantic_screenshot::{
    prepare_semantic_screenshot, SemanticScreenshot, SemanticScreenshotBudget,
    SemanticScreenshotBudgetError, SemanticScreenshotCoordinator,
    SemanticScreenshotCoordinatorError, SemanticScreenshotCoordinatorStatus,
    SemanticScreenshotError, SemanticScreenshotNativeCapture, SemanticScreenshotNativeRequest,
    SemanticScreenshotPaintEvidence, SemanticScreenshotPending, SemanticScreenshotPixelLayout,
    SemanticScreenshotRequest, SemanticScreenshotRequestError, SemanticScreenshotRequestId,
    SemanticScreenshotScope, SemanticScreenshotStats, SemanticScreenshotTrust,
    MAX_PENDING_SEMANTIC_SCREENSHOTS, MAX_SEMANTIC_SCREENSHOT_CAPTURE_MILLIS,
    MAX_SEMANTIC_SCREENSHOT_HEIGHT, MAX_SEMANTIC_SCREENSHOT_PIXELS,
    MAX_SEMANTIC_SCREENSHOT_PNG_BYTES, MAX_SEMANTIC_SCREENSHOT_PNG_CHUNKS,
    MAX_SEMANTIC_SCREENSHOT_WIDTH,
};
pub use semantic_settle::{
    SemanticActionAttemptId, SemanticActionFailure, SemanticActionRecoveryHint,
    SemanticSettleError, SemanticSettleEvent, SemanticSettleFact, SemanticSettleInstant,
    SemanticSettleStatus, SemanticSettleTracker, MAX_SEMANTIC_SETTLE_EVENTS,
};
pub use semantic_verify::{
    verify_semantic_action, SemanticEffectEvidence, SemanticEffectProofKind,
    SemanticScrollPosition, SemanticScrollPositionError, SemanticVerificationError,
    SemanticVerifiedAction, MAX_SEMANTIC_SCROLL_COORDINATE,
};
pub use semantic_wire::{
    decode_semantic_snapshot, SemanticDecodeContext, SemanticDecodeError, MAX_SEMANTIC_WIRE_BYTES,
    SEMANTIC_WIRE_VERSION,
};
pub use sign_in_handoff::{
    ContextSignInHandoff, ContextSignInHandoffBlocker, ContextSignInHandoffCleanup,
    ContextSignInHandoffError, ContextSignInHandoffId, ContextSignInHandoffPlatform,
    ContextSignInHandoffState,
};
