//! Fail-closed WebView2 CDP isolated-world feasibility gate.
//!
//! WebView2 has no native isolated-world user-script API. The pinned SDK does
//! expose the Chrome DevTools Protocol (CDP), including session-scoped calls
//! for out-of-process iframes. This module deliberately remains a gated probe:
//! compiling the control plane is not evidence that every supported WebView2
//! runtime meets the live isolation, ordering, debugger-coexistence, and
//! performance criteria documented below and in `docs/architecture.md` section
//! 10.
//!
//! There is intentionally no page-world fallback. A failed or unavailable
//! probe means Windows userscripts remain disabled.
//!
//! # Audited blockers
//!
//! This is not yet safe to wire even as an opt-in product path. Script authority
//! is terminal after failure, but before a live harness can be admitted it must
//! complete terminal native cleanup, unwind every paused auto-attached target,
//! retain and remove native script/binding identities, generation-bind
//! navigation and late session events, serialize setup around initial
//! navigation, and bound source replication plus in-flight replies.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2CallDevToolsProtocolMethodCompletedHandler,
    ICoreWebView2DevToolsProtocolEventReceivedEventArgs,
    ICoreWebView2DevToolsProtocolEventReceivedEventArgs2,
    ICoreWebView2DevToolsProtocolEventReceiver, ICoreWebView2_11,
};
use webview2_com::{
    CallDevToolsProtocolMethodCompletedHandler, DevToolsProtocolEventReceivedEventHandler,
    NavigationStartingEventHandler,
};
use windows_core::{Interface, HSTRING, PWSTR};
use zephium_core::ports::engine::ScriptPrincipal;

const MAX_PRINCIPALS: usize = 32;
const MAX_TOTAL_SOURCE_BYTES: usize = 2 * 1_024 * 1_024;
const MAX_SOURCE_BYTES: usize = 256 * 1_024;
const MAX_SESSIONS: usize = 128;
const MAX_SESSION_ID_BYTES: usize = 256;
const MAX_CONTEXTS: usize = 1_024;
const MAX_CONTEXT_NAME_BYTES: usize = 128;
const MAX_UNIQUE_CONTEXT_ID_BYTES: usize = 256;
const MAX_FRAME_ID_BYTES: usize = 256;
const MAX_BINDING_NAME_BYTES: usize = 96;
const MAX_EVENT_JSON_BYTES: usize = 192 * 1_024;
const MAX_EVENT_JSON_UTF16_UNITS: usize = MAX_EVENT_JSON_BYTES;
const MAX_MESSAGE_BYTES: usize = 64 * 1_024;
const MAX_PENDING_MESSAGES: usize = 256;
const MAX_PENDING_PER_PRINCIPAL: usize = 64;
const MAX_PENDING_BYTES: usize = 512 * 1_024;
const MAX_MESSAGES_PER_RATE_WINDOW: u32 = 128;
const MESSAGE_RATE_WINDOW: Duration = Duration::from_secs(1);
const MAX_REPLY_AGE: Duration = Duration::from_secs(30);
const MAX_CDP_RESULT_BYTES: usize = 128 * 1_024;
const BRIDGE_API_NAME: &str = "__ZEPHIUM_NATIVE_V1__";

/// Product-facing capability remains disabled until live proof covers pre-page
/// injection in every required frame, native-bound bounded messaging, exact
/// navigation/reply lifecycle, stale and impersonating contexts, teardown and
/// renderer recovery, page-world spoof resistance, debugger coexistence, and
/// realistic performance on the supported WebView2 runtime range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CdpIsolatedWorldSupport {
    DisabledPendingNativeProof,
}

pub(crate) const fn isolated_world_support() -> CdpIsolatedWorldSupport {
    CdpIsolatedWorldSupport::DisabledPendingNativeProof
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CdpProbeStatus {
    Installing,
    ConfiguredAwaitingLiveProof,
    FailedClosed,
}

#[derive(Debug)]
struct CdpProbeLifecycle {
    status: Cell<CdpProbeStatus>,
}

impl CdpProbeLifecycle {
    fn new() -> Self {
        Self {
            status: Cell::new(CdpProbeStatus::Installing),
        }
    }

    fn status(&self) -> CdpProbeStatus {
        self.status.get()
    }

    fn is_terminal(&self) -> bool {
        self.status() == CdpProbeStatus::FailedClosed
    }

    fn fail_closed(&self) {
        self.status.set(CdpProbeStatus::FailedClosed);
    }

    fn mark_configured(&self) -> bool {
        if self.is_terminal() {
            return false;
        }
        self.status.set(CdpProbeStatus::ConfiguredAwaitingLiveProof);
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerminalSetupAction {
    Continue,
    Stop,
    ResumeOnly,
}

fn terminal_setup_action(
    lifecycle: &CdpProbeLifecycle,
    resume_waiting_target: bool,
) -> TerminalSetupAction {
    if !lifecycle.is_terminal() {
        TerminalSetupAction::Continue
    } else if resume_waiting_target {
        TerminalSetupAction::ResumeOnly
    } else {
        TerminalSetupAction::Stop
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CdpRejection {
    InvalidPrincipalSet,
    DuplicatePrincipal,
    PrincipalLimit,
    SourceLimit,
    InvalidSession,
    DuplicateSession,
    SessionLimit,
    UnknownSession,
    InvalidContext,
    ContextLimit,
    UnknownContext,
    InvalidEvent,
    PayloadLimit,
    PendingLimit,
    PendingBytesLimit,
    RateLimit,
    CounterExhausted,
    StaleContext,
    StalePrincipal,
    UnknownMessage,
    ReplyExpired,
}

#[derive(Clone, Debug)]
pub(crate) struct CdpPrincipalSpec {
    principal: ScriptPrincipal,
    source: String,
}

impl CdpPrincipalSpec {
    pub(crate) fn new(principal: ScriptPrincipal, source: String) -> Result<Self, &'static str> {
        if source.len() > MAX_SOURCE_BYTES {
            return Err("Windows isolated-world source exceeds the bounded per-principal limit");
        }
        Ok(Self { principal, source })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum CdpRoute {
    Root,
    Session(String),
}

impl CdpRoute {
    fn from_session_id(session_id: &str) -> Result<Self, CdpRejection> {
        if session_id.is_empty() {
            return Ok(Self::Root);
        }
        if session_id.len() > MAX_SESSION_ID_BYTES
            || session_id
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        {
            return Err(CdpRejection::InvalidSession);
        }
        Ok(Self::Session(session_id.to_owned()))
    }

    fn session_id(&self) -> Option<&str> {
        match self {
            Self::Root => None,
            Self::Session(value) => Some(value),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ContextKey {
    route: CdpRoute,
    execution_context_id: i64,
}

#[derive(Clone, Debug)]
struct PrincipalRecord {
    source: String,
    world_name: String,
    binding_name: String,
    reply_name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionPhase {
    Installing,
    Configured,
}

#[derive(Clone, Debug)]
struct SessionRecord {
    generation: u64,
    phase: SessionPhase,
}

#[derive(Clone, Debug)]
struct ContextRecord {
    principal: ScriptPrincipal,
    generation: u64,
    unique_context_id: String,
    frame_id: Option<String>,
}

#[derive(Debug)]
struct PendingMessage {
    principal: ScriptPrincipal,
    context: ContextKey,
    context_generation: u64,
    payload_bytes: usize,
    accepted_at: Instant,
}

#[derive(Clone, Copy, Debug, Default)]
struct MessageRate {
    window_started: Option<Instant>,
    admitted: u32,
}

/// Opaque, single-use native reply authority. None of these fields are
/// constructed from JavaScript; the state machine mints them only after a
/// world-scoped binding event matches the current native context generation.
#[derive(Debug)]
pub(crate) struct CdpReplyTicket {
    id: u64,
    principal: ScriptPrincipal,
    context: ContextKey,
    context_generation: u64,
}

#[derive(Debug)]
pub(crate) struct CdpInboundMessage {
    pub(crate) principal: ScriptPrincipal,
    pub(crate) payload: String,
    pub(crate) reply: CdpReplyTicket,
}

#[derive(Clone, Debug)]
struct SessionPermit {
    route: CdpRoute,
    generation: u64,
}

#[derive(Clone, Debug)]
struct CdpCommand {
    route: CdpRoute,
    method: &'static str,
    parameters: String,
}

#[derive(Debug)]
struct CdpRuntimeState {
    principals: HashMap<ScriptPrincipal, PrincipalRecord>,
    principals_by_world: HashMap<String, ScriptPrincipal>,
    principals_by_binding: HashMap<String, ScriptPrincipal>,
    sessions: HashMap<CdpRoute, SessionRecord>,
    contexts: HashMap<ContextKey, ContextRecord>,
    pending: HashMap<u64, PendingMessage>,
    message_rates: HashMap<ScriptPrincipal, MessageRate>,
    next_session_generation: u64,
    next_context_generation: u64,
    next_message_id: u64,
}

impl CdpRuntimeState {
    fn new(specs: Vec<CdpPrincipalSpec>) -> Result<Self, CdpRejection> {
        if specs.is_empty() {
            return Err(CdpRejection::InvalidPrincipalSet);
        }
        if specs.len() > MAX_PRINCIPALS {
            return Err(CdpRejection::PrincipalLimit);
        }
        let mut total_source_bytes = 0usize;
        let mut principals = HashMap::with_capacity(specs.len());
        let mut principals_by_world = HashMap::with_capacity(specs.len());
        let mut principals_by_binding = HashMap::with_capacity(specs.len());
        let mut message_rates = HashMap::with_capacity(specs.len());
        for spec in specs {
            if spec.source.len() > MAX_SOURCE_BYTES {
                return Err(CdpRejection::SourceLimit);
            }
            total_source_bytes = total_source_bytes
                .checked_add(spec.source.len())
                .ok_or(CdpRejection::SourceLimit)?;
            if total_source_bytes > MAX_TOTAL_SOURCE_BYTES {
                return Err(CdpRejection::SourceLimit);
            }
            if principals.contains_key(&spec.principal) {
                return Err(CdpRejection::DuplicatePrincipal);
            }
            let namespace = principal_namespace(spec.principal);
            let world_name = format!("zephium_{namespace}_world");
            let binding_name = format!("__zephium_{namespace}_post");
            let reply_name = format!("__zephium_{namespace}_reply");
            debug_assert!(world_name.len() <= MAX_CONTEXT_NAME_BYTES);
            debug_assert!(binding_name.len() <= MAX_BINDING_NAME_BYTES);
            let record = PrincipalRecord {
                source: spec.source,
                world_name: world_name.clone(),
                binding_name: binding_name.clone(),
                reply_name,
            };
            principals_by_world.insert(world_name, spec.principal);
            principals_by_binding.insert(binding_name, spec.principal);
            message_rates.insert(spec.principal, MessageRate::default());
            principals.insert(spec.principal, record);
        }

        let root = CdpRoute::Root;
        let mut sessions = HashMap::new();
        sessions.insert(
            root,
            SessionRecord {
                generation: 1,
                phase: SessionPhase::Installing,
            },
        );
        Ok(Self {
            principals,
            principals_by_world,
            principals_by_binding,
            sessions,
            contexts: HashMap::new(),
            pending: HashMap::new(),
            message_rates,
            next_session_generation: 2,
            next_context_generation: 1,
            next_message_id: 1,
        })
    }

    fn root_permit(&self) -> SessionPermit {
        let record = self
            .sessions
            .get(&CdpRoute::Root)
            .expect("root CDP route is an invariant");
        SessionPermit {
            route: CdpRoute::Root,
            generation: record.generation,
        }
    }

    fn register_session(&mut self, session_id: &str) -> Result<SessionPermit, CdpRejection> {
        let route = CdpRoute::from_session_id(session_id)?;
        if route == CdpRoute::Root {
            return Err(CdpRejection::InvalidSession);
        }
        if self.sessions.contains_key(&route) {
            return Err(CdpRejection::DuplicateSession);
        }
        if self.sessions.len() >= MAX_SESSIONS.saturating_add(1) {
            return Err(CdpRejection::SessionLimit);
        }
        let generation = self.next_session_generation;
        self.next_session_generation = self
            .next_session_generation
            .checked_add(1)
            .ok_or(CdpRejection::CounterExhausted)?;
        self.sessions.insert(
            route.clone(),
            SessionRecord {
                generation,
                phase: SessionPhase::Installing,
            },
        );
        Ok(SessionPermit { route, generation })
    }

    fn session_is_current(&self, permit: &SessionPermit) -> bool {
        self.sessions
            .get(&permit.route)
            .is_some_and(|record| record.generation == permit.generation)
    }

    fn mark_session_configured(&mut self, permit: &SessionPermit) -> bool {
        let Some(record) = self.sessions.get_mut(&permit.route) else {
            return false;
        };
        if record.generation != permit.generation {
            return false;
        }
        record.phase = SessionPhase::Configured;
        true
    }

    fn remove_session(&mut self, session_id: &str) -> Result<(), CdpRejection> {
        let route = CdpRoute::from_session_id(session_id)?;
        if route == CdpRoute::Root || self.sessions.remove(&route).is_none() {
            return Err(CdpRejection::UnknownSession);
        }
        self.invalidate_route(&route);
        Ok(())
    }

    fn remove_principal(&mut self, principal: ScriptPrincipal) -> Result<(), CdpRejection> {
        let Some(record) = self.principals.remove(&principal) else {
            return Err(CdpRejection::StalePrincipal);
        };
        self.principals_by_world.remove(&record.world_name);
        self.principals_by_binding.remove(&record.binding_name);
        self.message_rates.remove(&principal);
        self.contexts
            .retain(|_, context| context.principal != principal);
        self.pending
            .retain(|_, pending| pending.principal != principal);
        Ok(())
    }

    fn record_context_json(
        &mut self,
        route: CdpRoute,
        event_json: &str,
    ) -> Result<Option<ScriptPrincipal>, CdpRejection> {
        if event_json.len() > MAX_EVENT_JSON_BYTES {
            return Err(CdpRejection::InvalidEvent);
        }
        let event: Value =
            serde_json::from_str(event_json).map_err(|_| CdpRejection::InvalidEvent)?;
        let context = event
            .get("context")
            .and_then(Value::as_object)
            .ok_or(CdpRejection::InvalidEvent)?;
        let name = context
            .get("name")
            .and_then(Value::as_str)
            .ok_or(CdpRejection::InvalidEvent)?;
        if name.len() > MAX_CONTEXT_NAME_BYTES {
            return Err(CdpRejection::InvalidEvent);
        }
        let Some(principal) = self.principals_by_world.get(name).copied() else {
            return Ok(None);
        };
        let execution_context_id = context
            .get("id")
            .and_then(Value::as_i64)
            .filter(|id| *id > 0)
            .ok_or(CdpRejection::InvalidContext)?;
        let unique_context_id = context
            .get("uniqueId")
            .and_then(Value::as_str)
            .ok_or(CdpRejection::InvalidContext)?;
        if unique_context_id.is_empty() || unique_context_id.len() > MAX_UNIQUE_CONTEXT_ID_BYTES {
            return Err(CdpRejection::InvalidContext);
        }
        let frame_id = context
            .get("auxData")
            .and_then(Value::as_object)
            .and_then(|auxiliary| auxiliary.get("frameId"))
            .and_then(Value::as_str);
        if frame_id.is_some_and(|value| value.is_empty() || value.len() > MAX_FRAME_ID_BYTES) {
            return Err(CdpRejection::InvalidContext);
        }
        self.record_context(
            route,
            principal,
            execution_context_id,
            unique_context_id,
            frame_id,
        )?;
        Ok(Some(principal))
    }

    fn record_context(
        &mut self,
        route: CdpRoute,
        principal: ScriptPrincipal,
        execution_context_id: i64,
        unique_context_id: &str,
        frame_id: Option<&str>,
    ) -> Result<(), CdpRejection> {
        if !self.sessions.contains_key(&route) {
            return Err(CdpRejection::UnknownSession);
        }
        if !self.principals.contains_key(&principal) || execution_context_id <= 0 {
            return Err(CdpRejection::InvalidContext);
        }
        let key = ContextKey {
            route,
            execution_context_id,
        };
        if self.contexts.len() >= MAX_CONTEXTS && !self.contexts.contains_key(&key) {
            return Err(CdpRejection::ContextLimit);
        }
        if self.contexts.iter().any(|(candidate_key, context)| {
            candidate_key != &key
                && candidate_key.route == key.route
                && context.unique_context_id == unique_context_id
        }) {
            return Err(CdpRejection::InvalidContext);
        }
        if let Some(previous) = self.contexts.remove(&key) {
            self.pending.retain(|_, pending| {
                pending.context != key || pending.context_generation != previous.generation
            });
        }
        let generation = self.next_context_generation;
        self.next_context_generation = self
            .next_context_generation
            .checked_add(1)
            .ok_or(CdpRejection::CounterExhausted)?;
        self.contexts.insert(
            key,
            ContextRecord {
                principal,
                generation,
                unique_context_id: unique_context_id.to_owned(),
                frame_id: frame_id.map(str::to_owned),
            },
        );
        Ok(())
    }

    fn destroy_context(
        &mut self,
        route: CdpRoute,
        execution_context_id: i64,
    ) -> Result<(), CdpRejection> {
        let key = ContextKey {
            route,
            execution_context_id,
        };
        let Some(context) = self.contexts.remove(&key) else {
            // Runtime reports every context, including page worlds we never
            // registered. An unknown id is therefore benign.
            return Ok(());
        };
        self.pending.retain(|_, pending| {
            pending.context != key || pending.context_generation != context.generation
        });
        Ok(())
    }

    fn clear_contexts(&mut self, route: CdpRoute) -> Result<(), CdpRejection> {
        if !self.sessions.contains_key(&route) {
            return Err(CdpRejection::UnknownSession);
        }
        self.invalidate_route(&route);
        Ok(())
    }

    fn invalidate_route(&mut self, route: &CdpRoute) {
        self.contexts.retain(|key, _| &key.route != route);
        self.pending
            .retain(|_, pending| &pending.context.route != route);
    }

    /// Top-level navigation retires root and every child-target generation at
    /// the synchronous NavigationStarting boundary. Late callbacks may still
    /// arrive, but they cannot authorize a reply into the replacement page.
    fn invalidate_navigation(&mut self) -> Result<(), CdpRejection> {
        self.contexts.clear();
        self.pending.clear();
        self.sessions.retain(|route, _| *route == CdpRoute::Root);
        let generation = self.next_session_generation;
        self.next_session_generation = self
            .next_session_generation
            .checked_add(1)
            .ok_or(CdpRejection::CounterExhausted)?;
        let root = self
            .sessions
            .get_mut(&CdpRoute::Root)
            .ok_or(CdpRejection::UnknownSession)?;
        root.generation = generation;
        // CDP registrations persist across navigation. Only the exact
        // document/session identity is retired here; reinstalling would
        // duplicate scripts and bindings.
        Ok(())
    }

    fn accept_binding_json(
        &mut self,
        route: CdpRoute,
        event_json: &str,
        now: Instant,
    ) -> Result<CdpInboundMessage, CdpRejection> {
        if event_json.len() > MAX_EVENT_JSON_BYTES {
            return Err(CdpRejection::InvalidEvent);
        }
        let event: Value =
            serde_json::from_str(event_json).map_err(|_| CdpRejection::InvalidEvent)?;
        let binding_name = event
            .get("name")
            .and_then(Value::as_str)
            .ok_or(CdpRejection::InvalidEvent)?;
        if binding_name.len() > MAX_BINDING_NAME_BYTES {
            return Err(CdpRejection::InvalidEvent);
        }
        let principal = self
            .principals_by_binding
            .get(binding_name)
            .copied()
            .ok_or(CdpRejection::StalePrincipal)?;
        self.admit_message_rate(principal, now)?;
        let execution_context_id = event
            .get("executionContextId")
            .and_then(Value::as_i64)
            .filter(|id| *id > 0)
            .ok_or(CdpRejection::InvalidContext)?;
        let payload = event
            .get("payload")
            .and_then(Value::as_str)
            .ok_or(CdpRejection::InvalidEvent)?;
        if payload.len() > MAX_MESSAGE_BYTES {
            return Err(CdpRejection::PayloadLimit);
        }
        self.reap_expired(now);
        let context_key = ContextKey {
            route,
            execution_context_id,
        };
        let context = self
            .contexts
            .get(&context_key)
            .ok_or(CdpRejection::UnknownContext)?;
        // This check is the cross-principal boundary. The payload is never
        // consulted for identity; the binding registration and current native
        // context must independently agree on the same typed principal.
        if context.principal != principal {
            return Err(CdpRejection::StalePrincipal);
        }
        if self.pending.len() >= MAX_PENDING_MESSAGES
            || self
                .pending
                .values()
                .filter(|pending| pending.principal == principal)
                .count()
                >= MAX_PENDING_PER_PRINCIPAL
        {
            return Err(CdpRejection::PendingLimit);
        }
        let pending_bytes = self.pending.values().try_fold(0usize, |total, pending| {
            total.checked_add(pending.payload_bytes)
        });
        if pending_bytes
            .and_then(|total| total.checked_add(payload.len()))
            .is_none_or(|total| total > MAX_PENDING_BYTES)
        {
            return Err(CdpRejection::PendingBytesLimit);
        }
        let id = self.next_message_id;
        self.next_message_id = self
            .next_message_id
            .checked_add(1)
            .ok_or(CdpRejection::CounterExhausted)?;
        let context_generation = context.generation;
        self.pending.insert(
            id,
            PendingMessage {
                principal,
                context: context_key.clone(),
                context_generation,
                payload_bytes: payload.len(),
                accepted_at: now,
            },
        );
        Ok(CdpInboundMessage {
            principal,
            payload: payload.to_owned(),
            reply: CdpReplyTicket {
                id,
                principal,
                context: context_key,
                context_generation,
            },
        })
    }

    fn prepare_reply(
        &mut self,
        ticket: CdpReplyTicket,
        payload: &str,
        now: Instant,
    ) -> Result<CdpCommand, CdpRejection> {
        if payload.len() > MAX_MESSAGE_BYTES {
            return Err(CdpRejection::PayloadLimit);
        }
        let Some(principal) = self.principals.get(&ticket.principal) else {
            return Err(CdpRejection::StalePrincipal);
        };
        let Some(context) = self.contexts.get(&ticket.context) else {
            return Err(CdpRejection::StaleContext);
        };
        if context.principal != ticket.principal || context.generation != ticket.context_generation
        {
            return Err(CdpRejection::StaleContext);
        }
        let pending = self
            .pending
            .remove(&ticket.id)
            .ok_or(CdpRejection::UnknownMessage)?;
        if pending.principal != ticket.principal
            || pending.context != ticket.context
            || pending.context_generation != ticket.context_generation
        {
            return Err(CdpRejection::UnknownMessage);
        }
        if now.saturating_duration_since(pending.accepted_at) > MAX_REPLY_AGE {
            return Err(CdpRejection::ReplyExpired);
        }
        let callback =
            serde_json::to_string(&principal.reply_name).map_err(|_| CdpRejection::InvalidEvent)?;
        let reply_payload =
            serde_json::to_string(payload).map_err(|_| CdpRejection::PayloadLimit)?;
        let expression = format!(
            "(()=>{{const f=globalThis[{callback}];if(typeof f!==\"function\")return false;try{{f({reply_payload});return true;}}catch{{return false;}}}})()"
        );
        let parameters = json!({
            "expression": expression,
            "uniqueContextId": context.unique_context_id,
            "silent": true,
            // The bounded boolean lets the completion prove that evaluation
            // reached the exact unique context instead of treating COM
            // dispatch as delivery.
            "returnByValue": true,
            "awaitPromise": false,
            "includeCommandLineAPI": false,
            "userGesture": false,
        })
        .to_string();
        Ok(CdpCommand {
            route: ticket.context.route,
            method: "Runtime.evaluate",
            parameters,
        })
    }

    fn reap_expired(&mut self, now: Instant) {
        self.pending.retain(|_, pending| {
            now.saturating_duration_since(pending.accepted_at) <= MAX_REPLY_AGE
        });
    }

    fn admit_message_rate(
        &mut self,
        principal: ScriptPrincipal,
        now: Instant,
    ) -> Result<(), CdpRejection> {
        let rate = self
            .message_rates
            .get_mut(&principal)
            .ok_or(CdpRejection::StalePrincipal)?;
        if rate
            .window_started
            .is_none_or(|started| now.saturating_duration_since(started) >= MESSAGE_RATE_WINDOW)
        {
            rate.window_started = Some(now);
            rate.admitted = 0;
        }
        if rate.admitted >= MAX_MESSAGES_PER_RATE_WINDOW {
            return Err(CdpRejection::RateLimit);
        }
        rate.admitted = rate
            .admitted
            .checked_add(1)
            .ok_or(CdpRejection::CounterExhausted)?;
        Ok(())
    }

    fn setup_commands(
        &self,
        permit: &SessionPermit,
        resume_waiting_target: bool,
    ) -> Result<VecDeque<CdpCommand>, CdpRejection> {
        if !self.session_is_current(permit) {
            return Err(CdpRejection::UnknownSession);
        }
        let mut commands = VecDeque::new();
        let route = permit.route.clone();
        commands.push_back(command(route.clone(), "Runtime.enable", json!({})));
        commands.push_back(command(route.clone(), "Page.enable", json!({})));
        let mut principals = self.principals.iter().collect::<Vec<_>>();
        principals.sort_unstable_by_key(|(principal, _)| **principal);
        for (_, principal) in &principals {
            commands.push_back(command(
                route.clone(),
                "Runtime.addBinding",
                json!({
                    "name": principal.binding_name,
                    "executionContextName": principal.world_name,
                }),
            ));
        }
        for (_, principal) in principals {
            commands.push_back(command(
                route.clone(),
                "Page.addScriptToEvaluateOnNewDocument",
                json!({
                    "source": bridge_source(principal),
                    "worldName": principal.world_name,
                    "includeCommandLineAPI": false,
                    "runImmediately": true,
                }),
            ));
            commands.push_back(command(
                route.clone(),
                "Page.addScriptToEvaluateOnNewDocument",
                json!({
                    "source": principal.source,
                    "worldName": principal.world_name,
                    "includeCommandLineAPI": false,
                    "runImmediately": true,
                }),
            ));
        }
        commands.push_back(command(
            route.clone(),
            "Target.setAutoAttach",
            json!({
                "autoAttach": true,
                "waitForDebuggerOnStart": true,
                "flatten": true,
                "filter": [
                    { "type": "iframe", "exclude": false },
                    { "exclude": true }
                ],
            }),
        ));
        if resume_waiting_target {
            commands.push_back(command(route, "Runtime.runIfWaitingForDebugger", json!({})));
        }
        Ok(commands)
    }
}

fn command(route: CdpRoute, method: &'static str, parameters: Value) -> CdpCommand {
    CdpCommand {
        route,
        method,
        parameters: parameters.to_string(),
    }
}

fn principal_namespace(principal: ScriptPrincipal) -> String {
    let (kind, bytes) = match principal {
        ScriptPrincipal::Userscript(id) => ('u', id.bytes()),
        ScriptPrincipal::Extension(id) => ('e', id.bytes()),
    };
    let mut value = String::with_capacity(1 + bytes.len() * 2);
    value.push(kind);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(value, "{byte:02x}");
    }
    value
}

fn bridge_source(principal: &PrincipalRecord) -> String {
    let binding = serde_json::to_string(&principal.binding_name)
        .expect("CDP binding names are valid JSON strings");
    let reply = serde_json::to_string(&principal.reply_name)
        .expect("CDP reply names are valid JSON strings");
    let api = serde_json::to_string(BRIDGE_API_NAME).expect("bridge API name is valid JSON");
    // The captured native binding cannot be replaced by later script code.
    // Nothing is published to the DOM or page world; the only global API and
    // reply hook live inside this principal's named isolated world.
    format!(
        "(()=>{{\"use strict\";const post=globalThis[{binding}];let onReply=null;const api=Object.freeze({{postMessage(payload){{if(typeof payload!==\"string\")throw new TypeError(\"message must be a string\");if(payload.length>16384)throw new RangeError(\"message too large\");post(payload);}},setReplyHandler(handler){{if(handler!==null&&typeof handler!==\"function\")throw new TypeError(\"reply handler must be a function or null\");onReply=handler;}}}});Object.defineProperty(globalThis,{api},{{value:api,writable:false,configurable:false,enumerable:false}});Object.defineProperty(globalThis,{reply},{{value(payload){{const handler=onReply;if(handler!==null)handler(payload);}},writable:false,configurable:false,enumerable:false}});}})();"
    )
}

struct CdpEventRegistration {
    receiver: ICoreWebView2DevToolsProtocolEventReceiver,
    token: i64,
}

impl Drop for CdpEventRegistration {
    fn drop(&mut self) {
        let _ = unsafe {
            self.receiver
                .remove_DevToolsProtocolEventReceived(self.token)
        };
    }
}

struct CdpProbeNative {
    core: ICoreWebView2,
    core11: ICoreWebView2_11,
    state: Rc<RefCell<CdpRuntimeState>>,
    lifecycle: Rc<CdpProbeLifecycle>,
    sink: Rc<dyn Fn(CdpInboundMessage)>,
}

impl CdpProbeNative {
    fn fail_closed(&self) {
        self.lifecycle.fail_closed();
    }

    fn stop_terminal_setup(&self, route: &CdpRoute, resume_on_failure: bool) -> bool {
        match terminal_setup_action(&self.lifecycle, resume_on_failure) {
            TerminalSetupAction::Continue => false,
            TerminalSetupAction::Stop => true,
            TerminalSetupAction::ResumeOnly => {
                self.best_effort_resume(route);
                true
            }
        }
    }

    fn dispatch_setup(self: &Rc<Self>, permit: SessionPermit, resume_on_failure: bool) {
        if self.stop_terminal_setup(&permit.route, resume_on_failure) {
            return;
        }
        let commands = match self
            .state
            .borrow()
            .setup_commands(&permit, resume_on_failure)
        {
            Ok(commands) => commands,
            Err(_) => {
                self.fail_closed();
                if resume_on_failure {
                    self.best_effort_resume(&permit.route);
                }
                return;
            }
        };
        self.clone()
            .dispatch_next(permit, commands, resume_on_failure);
    }

    fn dispatch_next(
        self: Rc<Self>,
        permit: SessionPermit,
        mut commands: VecDeque<CdpCommand>,
        resume_on_failure: bool,
    ) {
        // WebView2 preserves dispatch order but explicitly allows CDP to
        // process calls out of order. Send exactly one command at a time and
        // advance only from its successful completed handler; in particular,
        // a paused OOPIF is never resumed before its world and binding settle.
        if self.stop_terminal_setup(&permit.route, resume_on_failure) {
            return;
        }
        if !self.state.borrow().session_is_current(&permit) {
            return;
        }
        let Some(command) = commands.pop_front() else {
            if self.state.borrow_mut().mark_session_configured(&permit)
                && permit.route == CdpRoute::Root
            {
                // This is deliberately not `Supported`: it proves only that
                // the runtime accepted setup calls, not the seven live gates.
                let _ = self.lifecycle.mark_configured();
            }
            return;
        };

        let continuation = self.clone();
        let continuation_permit = permit.clone();
        let failure_route = permit.route.clone();
        let completion = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(
            move |result, response| {
                if !cdp_completion_succeeded(result, &response) {
                    continuation.fail_closed();
                    if resume_on_failure {
                        continuation.best_effort_resume(&failure_route);
                    }
                    return Ok(());
                }
                continuation.dispatch_next(continuation_permit, commands, resume_on_failure);
                Ok(())
            },
        ));
        if self.call(&command, &completion).is_err() {
            self.fail_closed();
            if resume_on_failure {
                self.best_effort_resume(&permit.route);
            }
        }
    }

    fn call(
        &self,
        command: &CdpCommand,
        completion: &ICoreWebView2CallDevToolsProtocolMethodCompletedHandler,
    ) -> windows_core::Result<()> {
        let method = HSTRING::from(command.method);
        let parameters = HSTRING::from(&command.parameters);
        match command.route.session_id() {
            None => unsafe {
                self.core
                    .CallDevToolsProtocolMethod(&method, &parameters, completion)
            },
            Some(session_id) => {
                let session_id = HSTRING::from(session_id);
                unsafe {
                    self.core11.CallDevToolsProtocolMethodForSession(
                        &session_id,
                        &method,
                        &parameters,
                        completion,
                    )
                }
            }
        }
    }

    fn best_effort_resume(&self, route: &CdpRoute) {
        if route == &CdpRoute::Root {
            return;
        }
        let command = command(route.clone(), "Runtime.runIfWaitingForDebugger", json!({}));
        let completion =
            CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|_, _| Ok(())));
        let _ = self.call(&command, &completion);
    }

    fn dispatch_reply(
        &self,
        ticket: CdpReplyTicket,
        payload: &str,
    ) -> Result<(), CdpReplyDispatchError> {
        if self.lifecycle.is_terminal() {
            return Err(CdpReplyDispatchError::ProbeFailedClosed);
        }
        let command = self
            .state
            .borrow_mut()
            .prepare_reply(ticket, payload, Instant::now())
            .map_err(CdpReplyDispatchError::Rejected)?;
        if self.lifecycle.is_terminal() {
            return Err(CdpReplyDispatchError::ProbeFailedClosed);
        }
        let lifecycle = self.lifecycle.clone();
        let completion = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(
            move |result, response| {
                if !cdp_reply_succeeded(result, &response) {
                    lifecycle.fail_closed();
                }
                Ok(())
            },
        ));
        if let Err(error) = self.call(&command, &completion) {
            self.fail_closed();
            return Err(CdpReplyDispatchError::Native(error));
        }
        Ok(())
    }

    fn handle_binding_event(&self, args: &ICoreWebView2DevToolsProtocolEventReceivedEventArgs) {
        if self.lifecycle.is_terminal() {
            return;
        }
        let Some((route, event_json)) = decode_event(args) else {
            self.fail_closed();
            return;
        };
        // Invalid, oversized, stale, and spoofed script messages are local
        // refusals. They do not disable other principals or navigate content.
        let message =
            self.state
                .borrow_mut()
                .accept_binding_json(route, &event_json, Instant::now());
        if let Ok(message) = message {
            (self.sink)(message);
        }
    }

    fn handle_context_created(&self, args: &ICoreWebView2DevToolsProtocolEventReceivedEventArgs) {
        if self.lifecycle.is_terminal() {
            return;
        }
        let Some((route, event_json)) = decode_event(args) else {
            self.fail_closed();
            return;
        };
        if self
            .state
            .borrow_mut()
            .record_context_json(route, &event_json)
            .is_err()
        {
            self.fail_closed();
        }
    }

    fn handle_context_destroyed(&self, args: &ICoreWebView2DevToolsProtocolEventReceivedEventArgs) {
        if self.lifecycle.is_terminal() {
            return;
        }
        let Some((route, event_json)) = decode_event(args) else {
            self.fail_closed();
            return;
        };
        let execution_context_id = bounded_event_object(&event_json)
            .and_then(|event| event.get("executionContextId")?.as_i64())
            .filter(|id| *id > 0);
        if execution_context_id
            .is_none_or(|id| self.state.borrow_mut().destroy_context(route, id).is_err())
        {
            self.fail_closed();
        }
    }

    fn handle_contexts_cleared(&self, args: &ICoreWebView2DevToolsProtocolEventReceivedEventArgs) {
        if self.lifecycle.is_terminal() {
            return;
        }
        let Some((route, _)) = decode_event(args) else {
            self.fail_closed();
            return;
        };
        if self.state.borrow_mut().clear_contexts(route).is_err() {
            self.fail_closed();
        }
    }

    fn handle_target_attached(
        self: &Rc<Self>,
        args: &ICoreWebView2DevToolsProtocolEventReceivedEventArgs,
    ) {
        let Some((_, event_json)) = decode_event(args) else {
            self.fail_closed();
            return;
        };
        let Some((route, event)) = decode_attached_session(&event_json) else {
            self.fail_closed();
            return;
        };
        // Once failure is terminal, a newly reported paused child target must
        // never receive bindings, worlds, or principal source. Decoding its
        // bounded native route solely to release the debugger pause is the only
        // post-failure setup action.
        if self.lifecycle.is_terminal() {
            self.best_effort_resume(&route);
            return;
        }
        let Some((target_type, waiting_for_debugger)) = decode_attached_target(&event) else {
            self.fail_closed();
            self.best_effort_resume(&route);
            return;
        };
        if target_type != "iframe" || !waiting_for_debugger {
            self.fail_closed();
            self.best_effort_resume(&route);
            return;
        }
        let Some(session_id) = route.session_id() else {
            self.fail_closed();
            return;
        };
        let permit = match self.state.borrow_mut().register_session(session_id) {
            Ok(permit) => permit,
            Err(_) => {
                self.fail_closed();
                self.best_effort_resume(&route);
                return;
            }
        };
        self.dispatch_setup(permit, true);
    }

    fn handle_target_detached(&self, args: &ICoreWebView2DevToolsProtocolEventReceivedEventArgs) {
        let Some((_, event_json)) = decode_event(args) else {
            self.fail_closed();
            return;
        };
        let session_id = bounded_event_object(&event_json)
            .and_then(|event| event.get("sessionId")?.as_str().map(str::to_owned));
        if session_id
            .is_none_or(|session_id| self.state.borrow_mut().remove_session(&session_id).is_err())
        {
            self.fail_closed();
        }
    }
}

#[derive(Debug)]
pub(crate) enum CdpReplyDispatchError {
    Rejected(CdpRejection),
    Native(windows_core::Error),
    ProbeFailedClosed,
    ProbeRetired,
}

pub(crate) struct CdpReplyHandle {
    native: Weak<CdpProbeNative>,
}

impl CdpReplyHandle {
    pub(crate) fn reply(
        &self,
        ticket: CdpReplyTicket,
        payload: &str,
    ) -> Result<(), CdpReplyDispatchError> {
        let native = self
            .native
            .upgrade()
            .ok_or(CdpReplyDispatchError::ProbeRetired)?;
        native.dispatch_reply(ticket, payload)
    }
}

/// Owns all CDP event registrations for one WebView generation. This type is
/// intentionally not wired into production construction while
/// [`isolated_world_support`] remains `DisabledPendingNativeProof`.
#[must_use]
pub(crate) struct CdpProbeRegistration {
    native: Rc<CdpProbeNative>,
    events: Vec<CdpEventRegistration>,
    navigation_token: i64,
}

impl CdpProbeRegistration {
    pub(crate) fn status(&self) -> CdpProbeStatus {
        self.native.lifecycle.status()
    }

    pub(crate) fn reply_handle(&self) -> CdpReplyHandle {
        CdpReplyHandle {
            native: Rc::downgrade(&self.native),
        }
    }
}

impl Drop for CdpProbeRegistration {
    fn drop(&mut self) {
        self.native.fail_closed();
        self.native.state.borrow_mut().contexts.clear();
        self.native.state.borrow_mut().pending.clear();
        let _ = unsafe {
            self.native
                .core
                .remove_NavigationStarting(self.navigation_token)
        };
        // Explicitly drain registrations before dropping the native state so
        // no callback can upgrade its weak reference during later field drop.
        self.events.clear();
    }
}

/// Install the compile-complete but product-disabled native probe.
///
/// The caller must retain the registration on the owning STA. A successful
/// return means only that required COM interfaces and event registrations were
/// available. Even `ConfiguredAwaitingLiveProof` must never enable userscripts
/// until the audited blockers above are fixed and real Windows QA ratifies all
/// seven Phase 0a criteria. This function is available only to tests or an
/// explicitly feature-enabled spike build.
pub(crate) fn install_unvalidated_probe(
    core: &ICoreWebView2,
    principals: Vec<CdpPrincipalSpec>,
    sink: Rc<dyn Fn(CdpInboundMessage)>,
) -> windows_core::Result<CdpProbeRegistration> {
    let core11 = core.cast::<ICoreWebView2_11>()?;
    let state = CdpRuntimeState::new(principals).map_err(validation_error)?;
    let native = Rc::new(CdpProbeNative {
        core: core.clone(),
        core11,
        state: Rc::new(RefCell::new(state)),
        lifecycle: Rc::new(CdpProbeLifecycle::new()),
        sink,
    });
    let mut events = Vec::with_capacity(6);

    let weak = Rc::downgrade(&native);
    events.push(register_event(
        core,
        "Runtime.bindingCalled",
        move |args| {
            if let Some(native) = weak.upgrade() {
                if let Some(args) = args {
                    native.handle_binding_event(&args);
                } else {
                    native.fail_closed();
                }
            }
        },
    )?);

    let weak = Rc::downgrade(&native);
    events.push(register_event(
        core,
        "Runtime.executionContextCreated",
        move |args| {
            if let Some(native) = weak.upgrade() {
                if let Some(args) = args {
                    native.handle_context_created(&args);
                } else {
                    native.fail_closed();
                }
            }
        },
    )?);

    let weak = Rc::downgrade(&native);
    events.push(register_event(
        core,
        "Runtime.executionContextDestroyed",
        move |args| {
            if let Some(native) = weak.upgrade() {
                if let Some(args) = args {
                    native.handle_context_destroyed(&args);
                } else {
                    native.fail_closed();
                }
            }
        },
    )?);

    let weak = Rc::downgrade(&native);
    events.push(register_event(
        core,
        "Runtime.executionContextsCleared",
        move |args| {
            if let Some(native) = weak.upgrade() {
                if let Some(args) = args {
                    native.handle_contexts_cleared(&args);
                } else {
                    native.fail_closed();
                }
            }
        },
    )?);

    let weak = Rc::downgrade(&native);
    events.push(register_event(
        core,
        "Target.attachedToTarget",
        move |args| {
            if let Some(native) = weak.upgrade() {
                if let Some(args) = args {
                    native.handle_target_attached(&args);
                } else {
                    native.fail_closed();
                }
            }
        },
    )?);

    let weak = Rc::downgrade(&native);
    events.push(register_event(
        core,
        "Target.detachedFromTarget",
        move |args| {
            if let Some(native) = weak.upgrade() {
                if let Some(args) = args {
                    native.handle_target_detached(&args);
                } else {
                    native.fail_closed();
                }
            }
        },
    )?);

    let weak = Rc::downgrade(&native);
    let navigation = NavigationStartingEventHandler::create(Box::new(move |_, _| {
        if let Some(native) = weak.upgrade() {
            if !native.lifecycle.is_terminal()
                && native.state.borrow_mut().invalidate_navigation().is_err()
            {
                native.fail_closed();
            }
        }
        Ok(())
    }));
    let mut navigation_token = 0_i64;
    unsafe { core.add_NavigationStarting(&navigation, &mut navigation_token)? };

    let registration = CdpProbeRegistration {
        native: native.clone(),
        events,
        navigation_token,
    };
    let root = native.state.borrow().root_permit();
    native.dispatch_setup(root, false);
    Ok(registration)
}

fn register_event(
    core: &ICoreWebView2,
    event_name: &'static str,
    mut callback: impl FnMut(Option<ICoreWebView2DevToolsProtocolEventReceivedEventArgs>) + 'static,
) -> windows_core::Result<CdpEventRegistration> {
    let event_name = HSTRING::from(event_name);
    let receiver = unsafe { core.GetDevToolsProtocolEventReceiver(&event_name)? };
    let handler = DevToolsProtocolEventReceivedEventHandler::create(Box::new(move |_, args| {
        callback(args);
        Ok(())
    }));
    let mut token = 0_i64;
    unsafe { receiver.add_DevToolsProtocolEventReceived(&handler, &mut token)? };
    Ok(CdpEventRegistration { receiver, token })
}

fn decode_event(
    args: &ICoreWebView2DevToolsProtocolEventReceivedEventArgs,
) -> Option<(CdpRoute, String)> {
    // Args2 is mandatory for secure OOPIF attribution. Treat its absence as a
    // failed gate instead of silently assigning a child event to the root.
    let args2 = args
        .cast::<ICoreWebView2DevToolsProtocolEventReceivedEventArgs2>()
        .ok()?;
    let mut session_id = PWSTR::null();
    unsafe { args2.SessionId(&mut session_id).ok()? };
    let session_id =
        super::take_pwstr_bounded(session_id, MAX_SESSION_ID_BYTES, MAX_SESSION_ID_BYTES)?;
    let route = CdpRoute::from_session_id(&session_id).ok()?;

    let mut event_json = PWSTR::null();
    unsafe { args.ParameterObjectAsJson(&mut event_json).ok()? };
    let event_json =
        super::take_pwstr_bounded(event_json, MAX_EVENT_JSON_UTF16_UNITS, MAX_EVENT_JSON_BYTES)?;
    Some((route, event_json))
}

fn bounded_event_object(event_json: &str) -> Option<Value> {
    if event_json.len() > MAX_EVENT_JSON_BYTES {
        return None;
    }
    let value: Value = serde_json::from_str(event_json).ok()?;
    value.is_object().then_some(value)
}

fn decode_attached_session(event_json: &str) -> Option<(CdpRoute, Value)> {
    let event = bounded_event_object(event_json)?;
    let session_id = event.get("sessionId")?.as_str()?;
    let route = CdpRoute::from_session_id(session_id).ok()?;
    (route != CdpRoute::Root).then_some((route, event))
}

fn decode_attached_target(event: &Value) -> Option<(String, bool)> {
    let target_type = event.get("targetInfo")?.get("type")?.as_str()?;
    if target_type.len() > 32 {
        return None;
    }
    let waiting_for_debugger = event.get("waitingForDebugger")?.as_bool()?;
    Some((target_type.to_owned(), waiting_for_debugger))
}

fn cdp_completion_succeeded(result: windows_core::Result<()>, response: &str) -> bool {
    if result.is_err() || response.len() > MAX_CDP_RESULT_BYTES {
        return false;
    }
    if response.is_empty() {
        return false;
    }
    serde_json::from_str::<Value>(response)
        .ok()
        .is_some_and(|value| {
            value.is_object()
                && value.get("error").is_none()
                && value.get("exceptionDetails").is_none()
        })
}

fn cdp_reply_succeeded(result: windows_core::Result<()>, response: &str) -> bool {
    if !cdp_completion_succeeded(result, response) || response.is_empty() {
        return false;
    }
    serde_json::from_str::<Value>(response)
        .ok()
        .and_then(|value| value.get("result")?.get("value")?.as_bool())
        == Some(true)
}

fn validation_error(rejection: CdpRejection) -> windows_core::Error {
    let code = match rejection {
        CdpRejection::PrincipalLimit
        | CdpRejection::SourceLimit
        | CdpRejection::SessionLimit
        | CdpRejection::ContextLimit
        | CdpRejection::PendingLimit
        | CdpRejection::PendingBytesLimit
        | CdpRejection::RateLimit => windows::Win32::Foundation::E_OUTOFMEMORY,
        _ => windows::Win32::Foundation::E_INVALIDARG,
    };
    windows_core::Error::new(
        code,
        format!("invalid Windows CDP probe input: {rejection:?}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use zephium_core::ids::{ExtensionInstallId, UserscriptId};

    fn userscript(value: u128) -> ScriptPrincipal {
        ScriptPrincipal::Userscript(UserscriptId::from(value))
    }

    fn extension(value: u128) -> ScriptPrincipal {
        ScriptPrincipal::Extension(ExtensionInstallId::from(value))
    }

    fn spec(principal: ScriptPrincipal) -> CdpPrincipalSpec {
        CdpPrincipalSpec::new(principal, "globalThis.probeLoaded = true;".into()).unwrap()
    }

    fn state(principals: &[ScriptPrincipal]) -> CdpRuntimeState {
        CdpRuntimeState::new(principals.iter().copied().map(spec).collect()).unwrap()
    }

    fn add_context(
        state: &mut CdpRuntimeState,
        route: CdpRoute,
        principal: ScriptPrincipal,
        context_id: i64,
        unique_id: &str,
    ) {
        state
            .record_context(route, principal, context_id, unique_id, Some("frame-1"))
            .unwrap();
    }

    fn binding_event(
        state: &CdpRuntimeState,
        principal: ScriptPrincipal,
        context_id: i64,
        payload: &str,
    ) -> String {
        let binding = &state.principals[&principal].binding_name;
        json!({
            "name": binding,
            "payload": payload,
            "executionContextId": context_id,
        })
        .to_string()
    }

    #[test]
    fn failed_lifecycle_is_terminal_and_cannot_be_reconfigured() {
        let lifecycle = CdpProbeLifecycle::new();
        assert_eq!(lifecycle.status(), CdpProbeStatus::Installing);
        assert!(!lifecycle.is_terminal());

        lifecycle.fail_closed();
        lifecycle.fail_closed();

        assert!(lifecycle.is_terminal());
        assert!(!lifecycle.mark_configured());
        assert_eq!(lifecycle.status(), CdpProbeStatus::FailedClosed);
    }

    #[test]
    fn late_setup_continuation_after_failure_is_resume_only_for_a_paused_child() {
        let lifecycle = CdpProbeLifecycle::new();
        assert_eq!(
            terminal_setup_action(&lifecycle, true),
            TerminalSetupAction::Continue
        );

        lifecycle.fail_closed();

        assert_eq!(
            terminal_setup_action(&lifecycle, false),
            TerminalSetupAction::Stop
        );
        assert_eq!(
            terminal_setup_action(&lifecycle, true),
            TerminalSetupAction::ResumeOnly
        );
        assert!(!lifecycle.mark_configured());
    }

    #[test]
    fn failed_attach_can_recover_its_resume_route_without_trusting_target_metadata() {
        let event = json!({
            "sessionId": "paused-child",
            "targetInfo": { "type": 7 },
            "waitingForDebugger": "not-a-boolean",
        })
        .to_string();
        let (route, decoded) = decode_attached_session(&event).unwrap();
        assert_eq!(route.session_id(), Some("paused-child"));
        assert!(decode_attached_target(&decoded).is_none());

        let lifecycle = CdpProbeLifecycle::new();
        lifecycle.fail_closed();
        assert_eq!(
            terminal_setup_action(&lifecycle, true),
            TerminalSetupAction::ResumeOnly
        );
    }

    #[test]
    fn principal_variant_and_id_derive_distinct_native_names() {
        let user = userscript(7);
        let extension = extension(7);
        let state = state(&[user, extension]);
        assert_ne!(
            state.principals[&user].world_name,
            state.principals[&extension].world_name
        );
        assert_ne!(
            state.principals[&user].binding_name,
            state.principals[&extension].binding_name
        );
    }

    #[test]
    fn binding_and_context_must_agree_and_payload_cannot_spoof_principal() {
        let first = userscript(1);
        let second = extension(2);
        let mut state = state(&[first, second]);
        add_context(&mut state, CdpRoute::Root, first, 11, "first-context");
        add_context(&mut state, CdpRoute::Root, second, 22, "second-context");
        let now = Instant::now();

        let spoof_payload = r#"{"principal":"extension-2","value":"hello"}"#;
        let accepted = state
            .accept_binding_json(
                CdpRoute::Root,
                &binding_event(&state, first, 11, spoof_payload),
                now,
            )
            .unwrap();
        assert_eq!(accepted.principal, first);
        assert_eq!(accepted.payload, spoof_payload);

        let mismatched = state.accept_binding_json(
            CdpRoute::Root,
            &binding_event(&state, second, 11, "cannot cross principals"),
            now,
        );
        assert_eq!(mismatched.unwrap_err(), CdpRejection::StalePrincipal);
    }

    #[test]
    fn navigation_invalidates_contexts_pending_replies_and_child_sessions() {
        let principal = userscript(3);
        let mut state = state(&[principal]);
        let session = state.register_session("oopif-session").unwrap();
        add_context(
            &mut state,
            session.route.clone(),
            principal,
            31,
            "oopif-context",
        );
        let now = Instant::now();
        let message = state
            .accept_binding_json(
                session.route.clone(),
                &binding_event(&state, principal, 31, "request"),
                now,
            )
            .unwrap();

        state.invalidate_navigation().unwrap();
        assert!(!state.sessions.contains_key(&session.route));
        assert_eq!(
            state
                .prepare_reply(message.reply, "reply", now)
                .unwrap_err(),
            CdpRejection::StaleContext
        );
    }

    #[test]
    fn context_id_reuse_cannot_rehabilitate_an_old_reply_ticket() {
        let principal = userscript(4);
        let mut state = state(&[principal]);
        add_context(&mut state, CdpRoute::Root, principal, 41, "old-unique");
        let now = Instant::now();
        let message = state
            .accept_binding_json(
                CdpRoute::Root,
                &binding_event(&state, principal, 41, "request"),
                now,
            )
            .unwrap();
        add_context(&mut state, CdpRoute::Root, principal, 41, "new-unique");
        assert_eq!(
            state.prepare_reply(message.reply, "late", now).unwrap_err(),
            CdpRejection::StaleContext
        );
    }

    #[test]
    fn principal_removal_terminally_invalidates_pending_authority() {
        let principal = extension(5);
        let mut state = state(&[principal]);
        add_context(&mut state, CdpRoute::Root, principal, 51, "context-5");
        let now = Instant::now();
        let message = state
            .accept_binding_json(
                CdpRoute::Root,
                &binding_event(&state, principal, 51, "request"),
                now,
            )
            .unwrap();
        state.remove_principal(principal).unwrap();
        assert_eq!(
            state.prepare_reply(message.reply, "late", now).unwrap_err(),
            CdpRejection::StalePrincipal
        );
    }

    #[test]
    fn payload_and_pending_request_bounds_are_enforced_before_allocation_growth() {
        let principal = userscript(6);
        let mut state = state(&[principal]);
        add_context(&mut state, CdpRoute::Root, principal, 61, "context-6");
        let now = Instant::now();
        let oversized = "x".repeat(MAX_MESSAGE_BYTES + 1);
        assert_eq!(
            state
                .accept_binding_json(
                    CdpRoute::Root,
                    &binding_event(&state, principal, 61, &oversized),
                    now,
                )
                .unwrap_err(),
            CdpRejection::PayloadLimit
        );

        for index in 0..MAX_PENDING_PER_PRINCIPAL {
            state
                .accept_binding_json(
                    CdpRoute::Root,
                    &binding_event(&state, principal, 61, &format!("request-{index}")),
                    now,
                )
                .unwrap();
        }
        assert_eq!(
            state
                .accept_binding_json(
                    CdpRoute::Root,
                    &binding_event(&state, principal, 61, "one-too-many"),
                    now,
                )
                .unwrap_err(),
            CdpRejection::PendingLimit
        );
    }

    #[test]
    fn principal_message_rate_is_bounded_independently_of_pending_replies() {
        let principal = userscript(12);
        let mut state = state(&[principal]);
        add_context(&mut state, CdpRoute::Root, principal, 121, "context-12");
        let now = Instant::now();
        for index in 0..MAX_MESSAGES_PER_RATE_WINDOW {
            let message = state
                .accept_binding_json(
                    CdpRoute::Root,
                    &binding_event(&state, principal, 121, &format!("request-{index}")),
                    now,
                )
                .unwrap();
            state.prepare_reply(message.reply, "reply", now).unwrap();
        }
        assert_eq!(
            state
                .accept_binding_json(
                    CdpRoute::Root,
                    &binding_event(&state, principal, 121, "rate-exceeded"),
                    now,
                )
                .unwrap_err(),
            CdpRejection::RateLimit
        );
        let next_window = now + MESSAGE_RATE_WINDOW;
        let admitted = state.accept_binding_json(
            CdpRoute::Root,
            &binding_event(&state, principal, 121, "new-window"),
            next_window,
        );
        assert!(admitted.is_ok());
    }

    #[test]
    fn reply_routes_to_exact_oopif_session_and_unique_context() {
        let principal = extension(7);
        let mut state = state(&[principal]);
        let session = state.register_session("session-7").unwrap();
        add_context(
            &mut state,
            session.route.clone(),
            principal,
            71,
            "unique-context-7",
        );
        let now = Instant::now();
        let message = state
            .accept_binding_json(
                session.route.clone(),
                &binding_event(&state, principal, 71, "request"),
                now,
            )
            .unwrap();
        let reply = state.prepare_reply(message.reply, "response", now).unwrap();
        assert_eq!(reply.route.session_id(), Some("session-7"));
        assert_eq!(reply.method, "Runtime.evaluate");
        let parameters: Value = serde_json::from_str(&reply.parameters).unwrap();
        assert_eq!(parameters["uniqueContextId"], "unique-context-7");
        assert_eq!(parameters["returnByValue"], true);
    }

    #[test]
    fn reply_payload_is_encoded_as_data_not_javascript() {
        let principal = userscript(8);
        let mut state = state(&[principal]);
        add_context(
            &mut state,
            CdpRoute::Root,
            principal,
            81,
            "unique-context-8",
        );
        let now = Instant::now();
        let message = state
            .accept_binding_json(
                CdpRoute::Root,
                &binding_event(&state, principal, 81, "request"),
                now,
            )
            .unwrap();
        let attack = "\");globalThis.pageOwned=true;//";
        let reply = state.prepare_reply(message.reply, attack, now).unwrap();
        let parameters: Value = serde_json::from_str(&reply.parameters).unwrap();
        let expression = parameters["expression"].as_str().unwrap();
        assert!(expression.contains("\\\""));
        assert!(!expression.contains("f(\");globalThis"));
    }

    #[test]
    fn attached_target_setup_is_session_scoped_and_resumes_only_after_setup() {
        let principal = userscript(9);
        let mut state = state(&[principal]);
        let permit = state.register_session("session-9").unwrap();
        let commands = state.setup_commands(&permit, true).unwrap();
        assert!(commands
            .iter()
            .all(|command| command.route.session_id() == Some("session-9")));
        assert_eq!(
            commands.back().map(|command| command.method),
            Some("Runtime.runIfWaitingForDebugger")
        );
        assert!(commands.iter().any(|command| {
            command.method == "Page.addScriptToEvaluateOnNewDocument"
                && command.parameters.contains("worldName")
        }));
        assert!(commands.iter().any(|command| {
            command.method == "Runtime.addBinding"
                && command.parameters.contains("executionContextName")
        }));
        let methods = commands
            .iter()
            .map(|command| command.method)
            .collect::<Vec<_>>();
        assert_eq!(&methods[..2], &["Runtime.enable", "Page.enable"]);
        let attach = methods
            .iter()
            .position(|method| *method == "Target.setAutoAttach")
            .unwrap();
        let resume = methods
            .iter()
            .position(|method| *method == "Runtime.runIfWaitingForDebugger")
            .unwrap();
        assert_eq!(resume, attach + 1);
        assert!(methods[..attach].contains(&"Runtime.addBinding"));
        assert!(methods[..attach].contains(&"Page.addScriptToEvaluateOnNewDocument"));
    }

    #[test]
    fn command_and_reply_results_require_bounded_valid_protocol_objects() {
        assert!(cdp_completion_succeeded(Ok(()), "{}"));
        assert!(!cdp_completion_succeeded(Ok(()), ""));
        assert!(!cdp_completion_succeeded(
            Ok(()),
            r#"{"error":{"message":"method failed"}}"#
        ));
        assert!(!cdp_completion_succeeded(
            Ok(()),
            r#"{"exceptionDetails":{"text":"boom"}}"#
        ));
        assert!(cdp_reply_succeeded(
            Ok(()),
            r#"{"result":{"type":"boolean","value":true}}"#
        ));
        assert!(!cdp_reply_succeeded(
            Ok(()),
            r#"{"result":{"type":"boolean","value":false}}"#
        ));
        assert!(!cdp_completion_succeeded(
            Ok(()),
            &"x".repeat(MAX_CDP_RESULT_BYTES + 1)
        ));
    }

    #[test]
    fn expired_reply_is_rejected_and_consumed() {
        let principal = userscript(10);
        let mut state = state(&[principal]);
        add_context(
            &mut state,
            CdpRoute::Root,
            principal,
            101,
            "unique-context-10",
        );
        let now = Instant::now();
        let message = state
            .accept_binding_json(
                CdpRoute::Root,
                &binding_event(&state, principal, 101, "request"),
                now,
            )
            .unwrap();
        assert_eq!(
            state
                .prepare_reply(
                    message.reply,
                    "late",
                    now + MAX_REPLY_AGE + Duration::from_millis(1)
                )
                .unwrap_err(),
            CdpRejection::ReplyExpired
        );
    }

    #[test]
    fn oversized_sources_and_event_envelopes_are_rejected() {
        let principal = userscript(11);
        assert!(CdpPrincipalSpec::new(principal, "x".repeat(MAX_SOURCE_BYTES + 1)).is_err());
        let mut state = state(&[principal]);
        assert_eq!(
            state
                .record_context_json(CdpRoute::Root, &"x".repeat(MAX_EVENT_JSON_BYTES + 1))
                .unwrap_err(),
            CdpRejection::InvalidEvent
        );
    }
}
