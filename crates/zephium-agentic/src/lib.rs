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
#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]
#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unreachable, clippy::unwrap_used)
)]

#[cfg(all(feature = "probe-harness", not(debug_assertions)))]
compile_error!("the agentic probe harness is forbidden in optimized builds");

mod agent_action_metrics;
mod agent_audit;
mod agent_input_metrics;
mod agent_lifecycle;
mod agent_manifest;
mod agent_metric_closure;
mod agent_metrics;
mod agent_native_shutdown;
mod agent_native_shutdown_driver;
mod agent_policy;
mod agent_progress_metrics;
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
mod probe_evidence_path;
#[cfg(feature = "probe-harness")]
mod probe_qualification;
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
mod semantic_execute;
mod semantic_execute_coordinator;
mod semantic_extract;
mod semantic_extract_model;
mod semantic_locate;
mod semantic_locate_model;
mod semantic_model;
mod semantic_observation;
#[cfg(feature = "probe-harness")]
mod semantic_probe_evidence;
mod semantic_read;
mod semantic_read_model;
mod semantic_runtime;
mod semantic_screenshot;
mod semantic_settle;
mod semantic_settle_coordinator;
mod semantic_verify;
mod semantic_wire;
mod sign_in_handoff;

pub use agent_action_metrics::{
    AgentActionBackendMetrics, AgentActionDurationMetrics, AgentActionMetricError,
    AgentActionSettleEventMetrics, AgentRunActionPerformanceMetrics,
    AgentRunActionPerformanceSnapshot, AGENT_ACTION_DURATION_BUCKET_COUNT,
    AGENT_ACTION_DURATION_BUCKET_UPPER_BOUNDS_MILLIS, MAX_AGENT_ACTION_PERFORMANCE_SNAPSHOT_BYTES,
};
pub use agent_audit::{
    AgentAuditCompletion, AgentAuditDelivery, AgentAuditDeliveryId, AgentAuditDeliveryOutcome,
    AgentAuditDeliveryProof, AgentAuditDeliverySettlement, AgentAuditDispatch, AgentAuditError,
    AgentAuditEvent, AgentAuditEventId, AgentAuditLedger, AgentAuditLedgerStatus, AgentAuditPort,
    AgentAuditRecordV1, AgentAuditSinkFailure, AGENT_AUDIT_RECORD_V1_BYTES,
    MAX_AGENT_AUDIT_DELIVERY_EVENTS, MAX_PENDING_AGENT_AUDIT_EVENTS,
};
pub use agent_input_metrics::{
    AgentProviderInputKind, AgentProviderInputKindMetrics, AgentProviderInputMetricError,
    AgentProviderInputNodeMetrics, AgentProviderInputShapeMetrics, AgentRunProviderInputMetrics,
    AgentRunProviderInputSnapshot, MAX_AGENT_PROVIDER_INPUT_SNAPSHOT_BYTES,
};
pub use agent_lifecycle::{AgentBrowserLifecycle, AgentBrowserShutdownOutcome};
pub use agent_manifest::{
    AgentAccountAttestationId, AgentAccountId, AgentAccountScope, AgentContextAccountBinding,
    AgentDataFlowRule, AgentEffectScope, AgentManifestContractError, AgentPlanLeaseId,
    AgentPlanNodeAuthority, AgentPlanNodeId, AgentPlanNodeScope, AgentPolicyInstant,
    AgentRunBudget, AgentRunManifest, AgentRunManifestId, AgentRunScope, MAX_AGENT_DATA_FLOW_RULES,
    MAX_AGENT_PLAN_NODES, MAX_AGENT_RUN_ACCOUNTS, MAX_AGENT_RUN_CONTEXTS,
    MAX_AGENT_RUN_COST_MICRO_USD, MAX_AGENT_RUN_LIFETIME_MILLIS, MAX_AGENT_RUN_MODEL_TOKENS,
    MAX_AGENT_RUN_OPERATIONS, MAX_AGENT_RUN_ORIGINS, MAX_AGENT_RUN_PROFILES,
};
pub use agent_metric_closure::{
    AgentRunMetricClosure, AgentRunMetricClosureError, MAX_AGENT_RUN_METRIC_CLOSURE_BYTES,
};
pub use agent_metrics::{
    AgentEffectAccountingMetrics, AgentEffectClassMetrics, AgentMetricError,
    AgentModelAccountingMetrics, AgentNodeAccountingMetrics, AgentPricingScheduleMetrics,
    AgentRunAccountingMetrics, AgentRunAccountingSnapshot, MAX_AGENT_METRIC_PRICING_SCHEDULES,
};
pub use agent_native_shutdown::{
    AgentNativeShutdownAdmissionError, AgentNativeShutdownAdmissionRefusal,
    AgentNativeShutdownCoordinator, AgentNativeShutdownError, AgentNativeShutdownFinishRefusal,
    AgentNativeShutdownProof, AgentNativeShutdownResources, AgentNativeShutdownStage,
    AgentNativeShutdownStatus, MAX_AGENT_NATIVE_SHUTDOWN_AUDITS,
    MAX_AGENT_NATIVE_SHUTDOWN_PROOF_BYTES,
};
pub use agent_native_shutdown_driver::{
    drive_agent_native_shutdown_until, AgentNativeShutdownDriveError,
    AgentNativeShutdownEventSource, AgentNativeShutdownWait,
    AGENT_NATIVE_SHUTDOWN_RETRY_BASE_MILLIS, AGENT_NATIVE_SHUTDOWN_RETRY_MAX_MILLIS,
};
pub use agent_policy::{
    AgentActiveEffect, AgentActiveModelCall, AgentEffectAssessment, AgentEffectAuthorization,
    AgentEffectCancellation, AgentEffectDispatchRequest, AgentEffectId, AgentEffectPermit,
    AgentEffectReceipt, AgentEffectRequest, AgentEffectSettlement, AgentFailedSemanticEffect,
    AgentModelCallAdmission, AgentModelCallBudget, AgentModelCallId, AgentModelCallReceipt,
    AgentModelCallRequest, AgentModelCallSettlement, AgentModelCallUnaccountedSettlement,
    AgentModelInputCancellation, AgentModelUsageAccounting, AgentNeedsHumanReason,
    AgentNeedsHumanTransition, AgentPlanLeaseBinding, AgentPolicyAccounting, AgentPolicyError,
    AgentRunPolicy, AgentRunPolicySettlement, AgentRunPolicySettlementError,
    AgentRunPolicySettlementRefusal, AgentTaintCohort, AgentVerifiedSemanticEffect,
    MAX_AGENT_ACCOUNT_ATTESTATION_AGE_MILLIS, MAX_AGENT_PENDING_EFFECTS,
    MAX_AGENT_PENDING_MODEL_CALLS, MAX_AGENT_RUN_POLICY_SETTLEMENT_BYTES, MAX_AGENT_TAINT_COHORTS,
    MAX_AGENT_TAINT_REFERENCES,
};
pub use agent_progress_metrics::{
    AgentDurationMetrics, AgentNeedsHumanMetrics, AgentProgressMetricError,
    AgentRunProgressMetrics, AgentRunProgressOutcome, AgentRunProgressSnapshot,
};
pub use agent_provider::{
    AgentBrowserActProposal, AgentBrowserHumanReason, AgentBrowserScopeProposal,
    AgentBrowserSemanticQuery, AgentBrowserToolCall, AgentBrowserToolCallId,
    AgentBrowserToolContractError, AgentBrowserToolKind, AgentBrowserToolProposal,
    AgentBrowserWaitCondition, AgentCommittedProviderInput, AgentCommittedProviderRequest,
    AgentPreparedDiffRequest, AgentPreparedExtractionRequest, AgentPreparedLocateRequest,
    AgentPreparedObservationRequest, AgentPreparedReadContinuationRequest,
    AgentPreparedReadRequest, AgentPreparedScreenshotRequest, AgentProviderBillingClass,
    AgentProviderBoundDiffContinuation, AgentProviderBoundExtractionContinuation,
    AgentProviderBoundLocateContinuation, AgentProviderBoundReadContinuation,
    AgentProviderBoundScreenshotContinuation, AgentProviderCallConfig, AgentProviderCallIdentity,
    AgentProviderCompletion, AgentProviderContinuation, AgentProviderContinuationError,
    AgentProviderContinuationSeed, AgentProviderContractError, AgentProviderDiffRequestDraft,
    AgentProviderEndpoint, AgentProviderExactInputCount, AgentProviderExtractionOutputBinding,
    AgentProviderExtractionOutputCollector, AgentProviderExtractionOutputError,
    AgentProviderExtractionRequestDraft, AgentProviderFailure, AgentProviderFailureClass,
    AgentProviderInputEvidence, AgentProviderInputMetricReceipt, AgentProviderInputMetrics,
    AgentProviderInputOutcome, AgentProviderInputTokenBinding, AgentProviderInputTokenCount,
    AgentProviderInputTokenRequest, AgentProviderKind, AgentProviderLocalInputTokenCounter,
    AgentProviderLocateRequestDraft, AgentProviderModelRevision, AgentProviderObjective,
    AgentProviderObjectiveError, AgentProviderPricedUsage, AgentProviderPricingAttribution,
    AgentProviderPricingContractError, AgentProviderPricingError, AgentProviderPricingProfile,
    AgentProviderPricingRevision, AgentProviderPricingSchedule, AgentProviderProtocolError,
    AgentProviderReadContinuationRequestDraft, AgentProviderRequest, AgentProviderRequestDigest,
    AgentProviderRequestError, AgentProviderRequestSettlement, AgentProviderRetryAfter,
    AgentProviderRetryDisposition, AgentProviderScreenshotRequestDraft,
    AgentProviderSemanticInputStats, AgentProviderStopReason, AgentProviderStreamBatch,
    AgentProviderStreamBudget, AgentProviderStreamConclusion, AgentProviderStreamDecoder,
    AgentProviderStreamEvent, AgentProviderStreamStats, AgentProviderTerminalFailure,
    AgentProviderTextDelta, AgentProviderTokenRates, AgentProviderToolCallCorrelation,
    AgentProviderTransportInput, AgentProviderUsage, MAX_AGENT_BROWSER_NAVIGATION_URL_BYTES,
    MAX_AGENT_BROWSER_SEMANTIC_QUERY_BYTES,
    MAX_AGENT_PROVIDER_CONTINUATION_INITIAL_OBSERVATION_BYTES,
    MAX_AGENT_PROVIDER_CONTINUATION_TRANSCRIPT_BYTES, MAX_AGENT_PROVIDER_CONTINUATION_TURNS,
    MAX_AGENT_PROVIDER_INPUT_METRIC_RECEIPT_BYTES, MAX_AGENT_PROVIDER_MODEL_REVISION_BYTES,
    MAX_AGENT_PROVIDER_OBJECTIVE_BYTES, MAX_AGENT_PROVIDER_OBJECTIVE_TOKENS,
    MAX_AGENT_PROVIDER_OUTPUT_TEXT_BYTES, MAX_AGENT_PROVIDER_RATE_MICRO_USD_PER_MILLION_TOKENS,
    MAX_AGENT_PROVIDER_REQUEST_BYTES, MAX_AGENT_PROVIDER_RETRY_AFTER_MILLIS,
    MAX_AGENT_PROVIDER_SCREENSHOT_PNG_BYTES, MAX_AGENT_PROVIDER_SCREENSHOT_TRANSCRIPT_BYTES,
    MAX_AGENT_PROVIDER_SSE_EVENT_BYTES, MAX_AGENT_PROVIDER_SSE_LINE_BYTES,
    MAX_AGENT_PROVIDER_STREAM_EVENTS, MAX_AGENT_PROVIDER_STREAM_WIRE_BYTES,
    MAX_AGENT_PROVIDER_TOOL_ARGUMENT_BYTES, MAX_AGENT_PROVIDER_TOOL_CALLS,
    MAX_AGENT_PROVIDER_TOOL_CALL_ID_BYTES,
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

pub use semantic_locate::{
    locate_semantic_observation, SemanticLocateBudget, SemanticLocateError, SemanticLocateId,
    SemanticLocateMatch, SemanticLocateMatchQuality, SemanticLocateQuery, SemanticLocateRequest,
    SemanticLocateResult, SemanticLocateScope, SemanticLocateStats, MAX_SEMANTIC_LOCATE_MATCHES,
    MAX_SEMANTIC_LOCATE_QUERY_BYTES, MAX_SEMANTIC_LOCATE_QUERY_TERMS,
};
pub use semantic_locate_model::{
    encode_semantic_locate_result, SemanticEncodedLocateResult, SemanticLocateDeliveryReceipt,
    SemanticLocateEncodingStats, SemanticLocateModelPayload, SEMANTIC_LOCATE_MODEL_SCHEMA_VERSION,
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
    ContextNativeResourceSnapshot, ContextNavigationRedirectPolicy, ContextNavigationReplacement,
    ContextNavigationRequest, ContextNavigationSettlement, ContextNavigationTarget,
    ContextOwnedViewport, ContextPortContractError, ContextPortFailure, ContextRendererLoss,
    ContextResourceAuditId, ContextResourceAuditSettlement, ContextShutdownAuditSettlement,
    ContextShutdownDispatch, ContextTransitionRequest, ContextTransitionSettlement,
    SemanticActionNativeCompletion, SemanticScreenshotNativeCompletion,
    MAX_CONTEXT_NAVIGATION_REDIRECTS, MAX_CONTEXT_NAVIGATION_REDIRECT_ORIGINS,
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
    ContextCookieTransferId, ContextCookieTransferInstant, ContextCookieTransferOutcome,
    ContextCookieTransferRegistry, ContextCookieTransferRegistryStatus,
    ContextCookieTransferRequest, ContextCookieTransferSettlement, ContextCookieTransferStats,
    ContextCookieTransferWindow, MAX_COOKIES_PER_TRANSFER, MAX_COOKIE_BYTES,
    MAX_COOKIE_TRANSFER_BYTES, MAX_COOKIE_TRANSFER_MILLIS, MAX_COOKIE_TRANSFER_ORIGINS,
    MAX_PENDING_COOKIE_TRANSFERS,
};
#[cfg(feature = "probe-harness")]
pub use evidence::{
    ActivationEvidence, BackendAvailability, BackendCapability, CaseEvidence, CaseOutcome,
    EvidenceLabel, EvidenceValidationError, FocusEvidence, InputEventEvidence, Platform,
    ProbeFailure, ProbeFailureCode, ProbeStage, ResourceEvidence, RunEvidence, RuntimeFingerprint,
    TargetEvidence, TeardownEvidence, MAX_CASE_EVIDENCE, MAX_EVENT_EVIDENCE,
    MAX_RESOURCE_HELPER_PROCESSES, MAX_RESOURCE_RESIDENT_BYTES, MAX_RESOURCE_WEBVIEW2_PROCESSES,
};
#[cfg(feature = "probe-harness")]
pub use fixture_server::{FixtureRoute, FixtureServer, FixtureServerError};
#[cfg(feature = "probe-harness")]
pub use probe_evidence_path::{
    evidence_metadata_is_direct_directory, evidence_metadata_is_direct_file,
};
#[cfg(feature = "probe-harness")]
pub use probe_qualification::{
    qualify_windows_probe_evidence, WindowsProbeAggregate, WindowsProbeMode,
    WindowsProbeQualificationError, WINDOWS_PHYSICAL_REVIEW_MODES, WINDOWS_PROBE_CAPABILITIES,
    WINDOWS_PROBE_CASES,
};
#[cfg(feature = "probe-harness")]
pub use probe_recipes::{
    windows_input_plan, windows_key_message_lparam, FixedProbeScript, ProbeScriptWorld,
    WindowsInputStep, WindowsProbeGeometry, WindowsProbeKey, WindowsProbePoint,
    MACOS_NATIVE_INPUT_RUNTIME_V1, MACOS_PROBE_CONTENT_WORLD_V1, MACOS_PROBE_HANDLER_V1,
    MAX_NATIVE_INPUT_RUNTIME_ROW, NATIVE_INPUT_RUNTIME_PROTOCOL_V1,
};
pub use profile_lease::{
    ContextProfileLease, ContextProfileLeaseError, ContextProfileLeaseId,
    ContextProfileLeasePurpose, ContextProfileLeaseRegistry, ContextProfileLeaseStatus,
    ContextProfileStorageClass, MAX_CONTEXT_PROFILE_TOMBSTONES,
};
#[cfg(feature = "probe-harness")]
pub use protocol::{
    decode_request_line, decode_response_line, encode_response_line, CancelRequest, CancelledReply,
    HelloReply, HelloRequest, ProbeCommand, ProbeProtocolError, ProbeReply, ProbeRequest,
    ProbeResponse, RunMatrixRequest, ShutdownReply, ShutdownRequest, MAX_BACKENDS_PER_REQUEST,
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
pub(crate) use semantic_action::SemanticActionRuntimeDescriptor;
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
    SemanticActionBatchAdmissionRefusal, SemanticActionBatchCompletion,
    SemanticActionBatchContinuation, SemanticActionBatchExecution,
    SemanticActionBatchExecutionError, SemanticActionBatchFailure,
    SemanticActionBatchFailureAdmissionRefusal, SemanticActionBatchFailureStage,
    SemanticActionBatchOutcome, SemanticActionBatchResult, SemanticActionBatchStopReason,
    MAX_SEMANTIC_ACTION_BATCH_COMPLETION_BYTES, MAX_SEMANTIC_ACTION_BATCH_FAILURE_BYTES,
};
pub use semantic_action_result::{
    finalize_accounted_semantic_action_result, AgentAccountedSemanticActionResult,
    AgentAccountedSemanticActionResultRefusal, SemanticActionNextState, SemanticActionResult,
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
    encode_semantic_diff, SemanticDiffDeliveryReceipt, SemanticDiffEncodingStats,
    SemanticDiffModelPayload, SemanticEncodedDiff, SEMANTIC_DIFF_MODEL_SCHEMA_VERSION,
};
pub use semantic_execute::{
    begin_semantic_action_settlement, SemanticActionExecutionApplied,
    SemanticActionExecutionBackend, SemanticActionExecutionContractError,
    SemanticActionExecutionDisposition, SemanticActionExecutionInstant,
    SemanticActionExecutionOutcome, SemanticActionExecutionPreparationError,
    SemanticActionNativeFailure, SemanticActionNativeReadiness, SemanticActionNativeRequest,
    SemanticActionNativeSettlement, SemanticActionNativeTargetId, SemanticActionNativeViewport,
    SemanticActionNativeViewportError, SemanticActionSettlementRefusal,
    SemanticActionSettlementStart, SemanticActionSettlementStartError,
    MAX_SEMANTIC_ACTION_NATIVE_EXECUTION_MILLIS, MAX_SEMANTIC_ACTION_VIEWPORT_DIMENSION,
};
pub(crate) use semantic_execute::{
    prepare_semantic_action_execution, SemanticActionExecutionPending,
};
pub use semantic_execute_coordinator::{
    SemanticActionExecutionCoordinator, SemanticActionExecutionCoordinatorError,
    SemanticActionExecutionCoordinatorRefusal, SemanticActionExecutionCoordinatorStatus,
    SemanticActionExecutionDispatch, SemanticActionExecutionReservation,
    MAX_PENDING_SEMANTIC_ACTION_EXECUTIONS,
};
#[cfg(feature = "probe-harness")]
pub use semantic_execute_coordinator::{
    SemanticClickQualificationError, SemanticClickQualificationExecution,
};
pub use semantic_extract::{
    extract_delivered_semantic_read, extract_semantic_read, SemanticExtractedBoolean,
    SemanticExtractedField, SemanticExtractedText, SemanticExtractedTextList,
    SemanticExtractedUnsigned, SemanticExtractedValue, SemanticExtractionError,
    SemanticExtractionFieldSchema, SemanticExtractionResult, SemanticExtractionSchema,
    SemanticExtractionSchemaError, SemanticExtractionSchemaId, SemanticExtractionSource,
    SemanticExtractionSourceSpan, SemanticExtractionStats, SemanticExtractionTrust,
    SemanticExtractionValueKind, MAX_SEMANTIC_EXTRACTION_FIELDS,
    MAX_SEMANTIC_EXTRACTION_FIELD_NAME_BYTES, MAX_SEMANTIC_EXTRACTION_INPUT_BYTES,
    MAX_SEMANTIC_EXTRACTION_LIST_ITEMS, MAX_SEMANTIC_EXTRACTION_LIST_ITEM_BYTES,
    MAX_SEMANTIC_EXTRACTION_SCHEMA_NAME_BYTES, MAX_SEMANTIC_EXTRACTION_SOURCES_PER_VALUE,
    MAX_SEMANTIC_EXTRACTION_SOURCE_EDGES, MAX_SEMANTIC_EXTRACTION_TEXT_BYTES,
    MAX_SEMANTIC_EXTRACTION_TOTAL_TEXT_BYTES, MAX_SEMANTIC_EXTRACTION_VALUES,
    SEMANTIC_EXTRACTION_SCHEMA_VERSION,
};
pub use semantic_extract_model::{
    encode_semantic_extraction_request, SemanticEncodedExtractionRequest,
    SemanticExtractionDeliveryReceipt, SemanticExtractionEncodingStats,
    SemanticExtractionModelPayload, SEMANTIC_EXTRACTION_MODEL_SCHEMA_VERSION,
};
pub use semantic_model::{
    encode_semantic_observation, SemanticEncodedObservation, SemanticEncodingStats,
    SemanticModelDeliveryError, SemanticModelDeliverySettlement, SemanticModelEncodingBudget,
    SemanticModelEncodingError, SemanticModelPayload, SemanticTokenCountQuality,
    SemanticTokenCountRequirement, SemanticTokenCounter, SemanticTokenCounterError,
    SemanticTokenMeasurement, SemanticTokenMeasurementError, SemanticTokenizerRevision,
    SemanticTokenizerRevisionError, ACTION_SEMANTIC_DIFF_TOKEN_TARGET,
    INITIAL_SEMANTIC_MODEL_TOKEN_TARGET, MAX_SEMANTIC_MODEL_BYTES, MAX_SEMANTIC_MODEL_TOKENS,
    MAX_SEMANTIC_TOKENIZER_REVISION_BYTES, SEMANTIC_LOCATE_RESULT_TOKEN_CEILING,
    SEMANTIC_MODEL_SCHEMA_VERSION,
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
#[cfg(feature = "probe-harness")]
pub use semantic_probe_evidence::{
    decode_windows_semantic_probe_response, encode_windows_semantic_probe_response,
    qualify_windows_semantic_probe_evidence, WindowsSemanticProbeAggregate,
    WindowsSemanticProbeEvidence, WindowsSemanticProbeFailure, WindowsSemanticProbeFailureCode,
    WindowsSemanticProbeMode, WindowsSemanticProbeProtocolError,
    WindowsSemanticProbeQualificationError, WindowsSemanticProbeReply,
    WindowsSemanticProbeResponse, WindowsSemanticProbeStage, WindowsSemanticProbeValidationError,
    WindowsSemanticResourceEvidence, WindowsSemanticTeardownEvidence,
    MAX_WINDOWS_SEMANTIC_PROBE_OUTPUT_BYTES, WINDOWS_SEMANTIC_PHYSICAL_REVIEW_MODES,
    WINDOWS_SEMANTIC_PROBE_PROTOCOL_VERSION,
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
    encode_semantic_action_runtime_invocation, encode_semantic_runtime_invocation,
    SemanticActionRuntimeEvidence, SemanticActionRuntimeFault, SemanticActionRuntimeInvocation,
    SemanticActionRuntimeInvocationError, SemanticActionRuntimeResultError, SemanticRuntimeBudget,
    SemanticRuntimeBudgetError, SemanticRuntimeCorrelation, SemanticRuntimeFault,
    SemanticRuntimeInvocation, SemanticRuntimeInvocationError, SemanticRuntimePortFailure,
    SemanticRuntimeProgram, SemanticRuntimeResultError, SemanticRuntimeScopeClass,
    SemanticRuntimeSettlement, SemanticRuntimeSettlementError,
    MAX_SEMANTIC_ACTION_RUNTIME_REQUEST_BYTES, MAX_SEMANTIC_ACTION_RUNTIME_RESULT_BYTES,
    MAX_SEMANTIC_RUNTIME_CHANNEL_RESULT_BYTES, MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS,
    MAX_SEMANTIC_RUNTIME_REQUEST_BYTES, MAX_SEMANTIC_RUNTIME_SAFE_INTEGER,
    MAX_SEMANTIC_RUNTIME_SOURCE_BYTES, MAX_SEMANTIC_RUNTIME_VISITED_NODES,
    MIN_SEMANTIC_RUNTIME_WIRE_BYTES, SEMANTIC_RUNTIME_CHANNEL_ACK,
    SEMANTIC_RUNTIME_CHANNEL_EXHAUSTED, SEMANTIC_RUNTIME_CHANNEL_NAME,
    SEMANTIC_RUNTIME_CHANNEL_PULL, SEMANTIC_RUNTIME_CHANNEL_RESULT_PREFIX,
    SEMANTIC_RUNTIME_CHANNEL_STOP, SEMANTIC_RUNTIME_GLOBAL_NAME, SEMANTIC_RUNTIME_PROGRAM,
    SEMANTIC_RUNTIME_PROTOCOL_VERSION,
};
pub use semantic_screenshot::{
    prepare_semantic_screenshot, SemanticScreenshot, SemanticScreenshotBudget,
    SemanticScreenshotBudgetError, SemanticScreenshotCoordinator,
    SemanticScreenshotCoordinatorError, SemanticScreenshotCoordinatorStatus,
    SemanticScreenshotDeliveryReceipt, SemanticScreenshotError, SemanticScreenshotNativeCapture,
    SemanticScreenshotNativeFailure, SemanticScreenshotNativeRequest,
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
pub use semantic_settle_coordinator::{
    SemanticActionSettlementAdmissionRefusal, SemanticActionSettlementAdvanceRefusal,
    SemanticActionSettlementCoordinator, SemanticActionSettlementCoordinatorError,
    SemanticActionSettlementCoordinatorStatus, SemanticActionSettlementReservation,
    SemanticActionSettlementTerminal, SemanticActionSettlementUpdate,
    MAX_PENDING_SEMANTIC_ACTION_SETTLEMENTS,
};
#[cfg(test)]
pub(crate) use semantic_verify::verify_semantic_action;
pub use semantic_verify::{
    verify_semantic_action_terminal, SemanticActionVerificationRefusal,
    SemanticActionVerifiedTerminal, SemanticEffectEvidence, SemanticEffectProofKind,
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
