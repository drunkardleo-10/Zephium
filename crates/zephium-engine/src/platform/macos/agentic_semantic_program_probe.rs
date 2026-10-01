//! Release-excluded, fixed source variants for local owned-view qualifications.

use zephium_agentic::{SemanticActionRuntimeFault, SEMANTIC_RUNTIME_PROGRAM};

pub(super) const PREPARE_MESSAGE: &str = "ZEPHIUM_PREPARED_FILL_WAIT_V1";
const PREPARE_CONTINUE: &str = "ZEPHIUM_PREPARED_FILL_CONTINUE_V1";

pub(super) struct PreparedFill {
    reply: Option<super::ReplyBlock>,
    ready_at: std::time::Instant,
}

impl PreparedFill {
    pub(super) fn new(reply: super::ReplyBlock, now: std::time::Instant) -> Self {
        Self {
            reply: Some(reply),
            ready_at: now + std::time::Duration::from_millis(100),
        }
    }
    pub(super) fn waiting(&self) -> bool {
        self.reply.is_some()
    }
    pub(super) fn ready(&self, now: std::time::Instant) -> bool {
        self.waiting() && now >= self.ready_at
    }
    pub(super) fn stop(&mut self) -> Option<super::ReplyAction> {
        self.reply.take().map(super::ReplyAction::error)
    }
    #[allow(clippy::print_stderr)]
    pub(super) fn release(&mut self) -> Option<super::ReplyAction> {
        let reply = self.reply.take()?;
        eprintln!("isolated-fill-preparation: phase=released minimum_settle_ms=100 authority=rechecked mutation=not_yet_dispatched command_entered=false content=redacted");
        Some(super::ReplyAction::success(reply, PREPARE_CONTINUE))
    }
}

pub(super) fn preparation_selected() -> bool {
    isolated_fill_selected()
        && (std::env::var("ZEPHIUM_LOCAL_ISOLATED_FILL_PREPARATION_PROBE").as_deref() == Ok("1")
            || preparation_only_selected())
}

fn preparation_only_selected() -> bool {
    isolated_fill_selected()
        && std::env::var("ZEPHIUM_LOCAL_ISOLATED_FILL_PREPARATION_ONLY_PROBE").as_deref() == Ok("1")
}

impl super::SemanticRuntimeChannelState {
    #[allow(clippy::print_stderr)]
    pub(super) fn on_fill_preparation(
        &mut self,
        reply: super::ReplyBlock,
    ) -> super::ChannelActions {
        if self.phase == super::DocumentPhase::AuthorityRevoked && self.awaiting_result {
            let mut actions = super::ChannelActions::default();
            actions.push_reply(super::ReplyAction::error(reply));
            return actions;
        }
        if self.phase != super::DocumentPhase::Ready
            || !self.awaiting_result
            || self.pull.is_some()
            || self.prepared_fill.is_some()
            || !matches!(
                &self.pending,
                Some(super::PendingInvocation::Action {
                    authority: Some(_),
                    ..
                })
            )
        {
            return self.fail_transport(Some(reply));
        }
        self.prepared_fill = Some(PreparedFill::new(reply, std::time::Instant::now()));
        eprintln!("isolated-fill-preparation: phase=waiting minimum_settle_ms=100 mutation=not_yet_dispatched command_entered=false content=redacted");
        super::ChannelActions::default()
    }

    pub(super) fn poll_fill_preparation(&mut self) -> super::ChannelActions {
        let mut actions = super::ChannelActions::default();
        if !self
            .prepared_fill
            .as_ref()
            .is_some_and(PreparedFill::waiting)
        {
            return actions;
        }
        if self.phase != super::DocumentPhase::Ready
            || !self.awaiting_result
            || !matches!(&self.pending, Some(super::PendingInvocation::Action { authority: Some(authority), .. }) if authority())
        {
            return self.cancel();
        }
        if self
            .prepared_fill
            .as_ref()
            .is_some_and(|prepared| prepared.ready(std::time::Instant::now()))
        {
            if let Some(reply) = self.prepared_fill.as_mut().and_then(PreparedFill::release) {
                actions.push_reply(reply);
            }
        }
        actions
    }
}

const PREFLIGHT_REASONS: &[&str] = &[
    "ancestor_context",
    "ancestor_declaration",
    "sibling_count",
    "sibling_node_kind",
    "sibling_editability",
    "sibling_tag_br",
    "sibling_tag_wbr",
    "sibling_tag_p",
    "sibling_tag_other",
    "sibling_text",
    "sibling_sensitivity",
    "root_identity",
    "root_editability",
    "root_writability",
    "root_sensitivity",
    "root_visibility",
    "root_credential",
    "root_context",
    "target_structure",
    "child_count",
    "protected_identity",
    "protected_editability",
    "protected_tag",
    "protected_structure",
    "protected_text",
    "protected_sensitivity",
    "editable_child_count",
    "target_control",
    "protected_count",
    "logical_value",
    "primitives_or_text",
    "value_unchanged",
    "selection_count",
    "exception_primitives",
    "exception_witness",
    "exception_value",
    "exception_selection",
    "exception_range",
];

const POSTCONDITION_REASONS: &[&str] = &[
    "root_identity",
    "root_editability",
    "root_writability",
    "root_sensitivity",
    "root_visibility",
    "root_credential",
    "root_context",
    "ancestor_declaration",
    "target_structure",
    "child_count",
    "protected_identity",
    "protected_editability",
    "protected_tag",
    "protected_structure",
    "protected_text",
    "protected_sensitivity",
    "editable_child_count",
    "target_control",
    "protected_count",
    "logical_value",
    "value_mismatch",
    "restoration_exception",
    "reconciliation_exception",
];

fn isolated_fill_selected() -> bool {
    matches!(
        std::env::var("ZEPHIUM_LOCAL_OWNED_SURFACE_PROBE").as_deref(),
        Ok("isolated-fill-normal"
            | "isolated-fill-cancel"
            | "isolated-fill-replace"
            | "isolated-fill-adopt"
            | "isolated-fill-retarget"
            | "isolated-fill-protected"
            | "isolated-fill-ancestor"
            | "isolated-fill-div-rich"
            | "isolated-fill-div-nested"
            | "isolated-fill-div-identity"
            | "isolated-fill-div-editable"
            | "isolated-fill-div-structure")
            | Ok("isolated-fill-prepare-selection" | "isolated-fill-prepare-ancestor")
    )
}

fn preflight_reason(bytes: &[u8]) -> Option<&'static str> {
    let reason = bytes.strip_prefix(b"E2:unsupported_interaction_")?;
    PREFLIGHT_REASONS
        .iter()
        .copied()
        .find(|known| known.as_bytes() == reason)
}

fn postcondition_reason(bytes: &[u8]) -> Option<&'static str> {
    let reason = bytes.strip_prefix(b"E2:applied_unverified_postcondition_")?;
    POSTCONDITION_REASONS
        .iter()
        .copied()
        .find(|known| known.as_bytes() == reason)
}

fn command_behavior(bytes: &[u8]) -> Option<(&[u8], &'static str, &'static str)> {
    for (suffix, immediate) in [
        (b"_immediate_match".as_slice(), "match"),
        (b"_immediate_mismatch", "mismatch"),
        (b"_immediate_guarded", "guarded"),
        (b"_immediate_exception", "exception"),
    ] {
        let Some(prefix) = bytes.strip_suffix(suffix) else {
            continue;
        };
        for (suffix, returned) in [
            (b"_command_true".as_slice(), "true"),
            (b"_command_false", "false"),
            (b"_command_other", "other"),
        ] {
            let Some(base) = prefix.strip_suffix(suffix) else {
                continue;
            };
            if base == b"E2:applied_unverified_logical_editor"
                || postcondition_reason(base).is_some()
            {
                return Some((base, returned, immediate));
            }
        }
    }
    None
}

/// Only this fixed diagnostic program may emit these closed diagnostic codes.
/// Normalize to its original refusal before the ordinary decoder; never grant
/// success, expose page text, or make a post-dispatch outcome retryable.
#[allow(clippy::print_stderr)]
pub(super) fn normalize_diagnostic(bytes: &[u8]) -> &[u8] {
    if isolated_fill_selected() {
        if preparation_only_selected() && bytes == b"E2:applied_unverified_preparation_only" {
            eprintln!("isolated-fill-preparation-only: phase=revalidated command_entered=false result=uncertain content=redacted");
            return b"E2:applied_unverified";
        }
        let bytes = if let Some((base, returned, immediate)) = command_behavior(bytes) {
            eprintln!(
                "isolated-fill-command: returned={returned} immediate={immediate} content=redacted"
            );
            base
        } else {
            bytes
        };
        if let Some(reason) = preflight_reason(bytes) {
            eprintln!(
                "isolated-fill-preflight: reason={reason} command_entered=false content=redacted"
            );
            return b"E2:unsupported_interaction";
        }
        if let Some(reason) = postcondition_reason(bytes) {
            eprintln!(
                "isolated-fill-postcondition: reason={reason} result=uncertain content=redacted"
            );
            return b"E2:applied_unverified_postcondition";
        }
        return bytes;
    }
    bytes
}

pub(super) fn source() -> std::borrow::Cow<'static, str> {
    if isolated_fill_selected() {
        return std::borrow::Cow::Owned(isolated_fill_candidate_source());
    }
    if std::env::var("ZEPHIUM_LOCAL_OWNED_SURFACE_PROBE").as_deref() == Ok("display-document-start")
    {
        return std::borrow::Cow::Owned(format!(
            "{}\n{}",
            SEMANTIC_RUNTIME_PROGRAM.source(),
            include_str!("agentic_owned_display_command_probe.js")
        ));
    }
    std::borrow::Cow::Borrowed(SEMANTIC_RUNTIME_PROGRAM.source())
}

#[allow(clippy::print_stderr)]
pub(super) fn isolated_fill_candidate_source() -> String {
    // Two exact closed substitutions; one immutable, attested document-start
    // script. No invocation chooses a backend or supplies an editor program.
    let source = SEMANTIC_RUNTIME_PROGRAM.source()
        .replacen("  function runFixedFill(target, descriptor, request) {",
            &format!("{}\n  function runSyntheticFill(target, descriptor, request) {{",
                include_str!("agentic_isolated_fill_candidate.js")), 1)
        .replacen("const result = runFixedFill(target, descriptor, request);",
            "const result = runFixedFill(target, descriptor, request);\n      if (typeof result !== \"string\") return apply(promiseThen, result, [finishFill, () => actionFault(\"applied_unverified_postcondition\")]);", 1);
    let source = if preparation_only_selected() {
        eprintln!("isolated-fill-preparation-only: phase=source_selected command_entered=false content=redacted");
        source.replacen(
            "const commandPreparationOnly = false;",
            "const commandPreparationOnly = true;",
            1,
        )
    } else {
        source
    };
    if preparation_selected() {
        source.replacen("  async function serveNativeInvocations(channel, post) {",
            "  async function serveNativeInvocations(channel, post) {\n    commandPreparationBarrier = () => apply(post, channel, [\"ZEPHIUM_PREPARED_FILL_WAIT_V1\"]);", 1)
    } else {
        source
    }
}

#[allow(clippy::print_stderr)]
pub(super) fn record_fault(fault: &SemanticActionRuntimeFault) {
    if std::env::var("ZEPHIUM_LOCAL_OWNED_SURFACE_PROBE")
        .is_ok_and(|case| case.starts_with("isolated-fill-"))
    {
        eprintln!("isolated-fill-terminal: {fault:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply() -> super::super::ReplyBlock {
        block2::RcBlock::new(
            |_: *mut objc2::runtime::AnyObject, _: *mut objc2_foundation::NSString| {},
        )
    }

    fn prepared_state(
        allowed: std::rc::Rc<std::cell::Cell<bool>>,
    ) -> super::super::SemanticRuntimeChannelState {
        use super::super::*;
        let native = crate::agent_context_port::WorkActionTask::native_for_test();
        let invocation =
            zephium_agentic::encode_semantic_action_runtime_invocation(&native).unwrap();
        let mut state = SemanticRuntimeChannelState {
            phase: DocumentPhase::Ready,
            expected_view: Some(7),
            active_world: Some(8),
            ..SemanticRuntimeChannelState::default()
        };
        state
            .dispatch_action(
                invocation,
                Box::new(|_| {}),
                Some(Box::new(move || allowed.get())),
            )
            .unwrap_or_else(|_| panic!("admission"));
        let handed = state.on_pull(reply());
        assert!(handed.first_reply.is_some());
        assert!(state.awaiting_result);
        state
    }

    #[test]
    fn preparation_releases_once_after_interval_and_rechecked_authority() {
        let allowed = std::rc::Rc::new(std::cell::Cell::new(true));
        let mut state = prepared_state(allowed);
        let original = state.pending.as_ref().unwrap().as_str().to_owned();
        let waiting = state.on_fill_preparation(reply());
        assert!(waiting.first_reply.is_none() && waiting.completion.is_none());
        assert!(state.poll_fill_preparation().first_reply.is_none());
        state.prepared_fill.as_mut().unwrap().ready_at = std::time::Instant::now();
        let released = state.poll_fill_preparation();
        assert!(released.first_reply.is_some() && released.completion.is_none());
        assert_eq!(state.pending.as_ref().unwrap().as_str(), original);
        assert!(state.awaiting_result);
        assert_eq!(state.completed_invocations, 0);
        assert!(state.poll_fill_preparation().first_reply.is_none());
        let terminal = state.on_result(b"E2:applied_unverified_logical_editor", reply());
        assert!(terminal.completion.is_some());
        assert_eq!(state.completed_invocations, 1);
        assert!(state.pending.is_none() && state.prepared_fill.is_none());
    }

    #[test]
    fn preparation_revocation_cancellation_and_document_loss_never_release_insertion() {
        for failure in 0..6 {
            let allowed = std::rc::Rc::new(std::cell::Cell::new(true));
            let mut state = prepared_state(allowed.clone());
            state.on_fill_preparation(reply());
            state.prepared_fill.as_mut().unwrap().ready_at = std::time::Instant::now();
            let stopped = match failure {
                0 => {
                    allowed.set(false);
                    state.poll_fill_preparation()
                }
                1 => state.cancel(),
                2 => state.begin_document_load(),
                3 => state.renderer_lost(),
                4 => state.retire(),
                _ => {
                    let Some(super::super::PendingInvocation::Action { invocation, .. }) =
                        &state.pending
                    else {
                        panic!("action")
                    };
                    let attempt = invocation.attempt();
                    let (actions, matched) = state.timeout_action(attempt);
                    assert!(matched);
                    actions
                }
            };
            assert!(stopped.completion.is_some());
            assert!(matches!(
                stopped.first_reply.unwrap().value,
                super::super::ReplyValue::Error
            ));
            assert!(state.prepared_fill.is_none() && state.pending.is_none());
            assert!(state.poll_fill_preparation().first_reply.is_none());
        }
    }

    #[test]
    fn preparation_duplicate_and_early_result_fail_closed() {
        for duplicate in [false, true] {
            let mut state = prepared_state(std::rc::Rc::new(std::cell::Cell::new(true)));
            state.on_fill_preparation(reply());
            let failed = if duplicate {
                state.on_fill_preparation(reply())
            } else {
                state.on_result(b"E2:applied_unverified_logical_editor", reply())
            };
            assert!(failed.completion.is_some() && failed.invariant_failed);
            assert!(state.prepared_fill.is_none() && state.pending.is_none());
        }
        let mut state = super::super::SemanticRuntimeChannelState::default();
        assert!(state.on_fill_preparation(reply()).invariant_failed);
    }

    #[test]
    fn released_command_cancellation_consumes_receiver_not_proof_of_no_effect() {
        // Reproduce the native URL-invalidation ordering without a website or
        // provider. A continuation has left native; its effect is uncertain.
        let mut state = prepared_state(std::rc::Rc::new(std::cell::Cell::new(true)));
        state.on_fill_preparation(reply());
        state.prepared_fill.as_mut().unwrap().ready_at = std::time::Instant::now();
        assert!(state.poll_fill_preparation().first_reply.is_some());
        let cancelled = state.cancel();
        assert!(matches!(
            cancelled.completion,
            Some(super::super::CompletionAction::Action {
                outcome: Err(super::super::AgentSemanticActionRuntimeFailure::Cancelled),
                ..
            })
        ));
        assert!(!state.awaiting_result && state.pending.is_none());
        assert_eq!(state.completed_invocations, 0);
        // The old diagnostic is no longer decoded, even if the original
        // runtime reports an applied command. Debt settled once, evidence lost.
        let late = state.on_result(
            b"E2:applied_unverified_logical_editor_command_true_immediate_match",
            reply(),
        );
        assert!(late.completion.is_none() && late.invariant_failed);
        assert_eq!(state.completed_invocations, 0);
        assert!(state.cancel().completion.is_none());
        assert!(state.admission_failure().is_some());
    }

    #[test]
    fn command_behavior_diagnostic_accepts_only_closed_uncertain_outcomes() {
        for returned in ["true", "false", "other"] {
            for immediate in ["match", "mismatch", "guarded", "exception"] {
                for base in [
                    "E2:applied_unverified_logical_editor",
                    "E2:applied_unverified_postcondition_value_mismatch",
                ] {
                    let encoded = format!("{base}_command_{returned}_immediate_{immediate}");
                    assert_eq!(
                        command_behavior(encoded.as_bytes()),
                        Some((base.as_bytes(), returned, immediate))
                    );
                }
            }
        }
        for invalid in [
            "E2:unsupported_interaction_command_true_immediate_match",
            "E2:applied_unverified_postcondition_unknown_command_true_immediate_match",
            "E2:applied_unverified_logical_editor_command_true_immediate_page-content",
            "E2:applied_unverified_logical_editor_command_true_immediate_match\n",
            "E2:applied_unverified_logical_editor_command_unknown_immediate_match",
        ] {
            assert!(command_behavior(invalid.as_bytes()).is_none());
        }
    }

    #[test]
    fn postcondition_diagnostics_accept_only_closed_uncertain_reasons() {
        for reason in POSTCONDITION_REASONS {
            let bytes = format!("E2:applied_unverified_postcondition_{reason}");
            assert_eq!(postcondition_reason(bytes.as_bytes()), Some(*reason));
            assert_eq!(preflight_reason(bytes.as_bytes()), None);
        }
        for bytes in [
            b"E2:applied_unverified_postcondition".as_slice(),
            b"E2:applied_unverified_postcondition_unknown",
            b"E2:applied_unverified_postcondition_value_mismatch page content",
            b"E2:applied_unverified_postcondition_value_mismatch\n",
            b"E2:unsupported_interaction_value_mismatch",
            b"E2:applied_unverified_postcondition_success",
        ] {
            assert_eq!(postcondition_reason(bytes), None);
        }
    }

    #[test]
    fn preflight_diagnostics_accept_only_closed_pre_mutation_reasons() {
        for reason in PREFLIGHT_REASONS {
            let bytes = format!("E2:unsupported_interaction_{reason}");
            assert_eq!(preflight_reason(bytes.as_bytes()), Some(*reason));
        }
        for bytes in [
            b"E2:unsupported_interaction".as_slice(),
            b"E2:unsupported_interaction_unknown",
            b"E2:unsupported_interaction_root_identity page content",
            b"E2:unsupported_interaction_root_identity\n",
            b"E2:applied_unverified_root_identity",
            b"E2:unsupported_interaction_mutation",
            b"E2:unsupported_interaction_sibling_tag_x-private-marker",
        ] {
            assert_eq!(preflight_reason(bytes), None);
        }
    }

    #[test]
    fn isolated_fill_candidate_is_closed_and_absent_from_shipping_program() {
        let shipping = SEMANTIC_RUNTIME_PROGRAM.source();
        assert_eq!(
            shipping
                .matches("  function runFixedFill(target, descriptor, request) {")
                .count(),
            1
        );
        assert_eq!(
            shipping
                .matches("const result = runFixedFill(target, descriptor, request);")
                .count(),
            1
        );
        assert!(!shipping.contains("execCommand"));
        let candidate = isolated_fill_candidate_source();
        assert!(candidate.len() < shipping.len() + 16 * 1024);
        assert_eq!(
            candidate.matches("Document.prototype.execCommand").count(),
            1
        );
        assert_eq!(
            candidate
                .matches("apply(commandInsertText, document, [\"insertText\", false, request.z])")
                .count(),
            1
        );
        assert_eq!(candidate.matches("function runFixedFill(").count(), 1);
        assert_eq!(candidate.matches("function runSyntheticFill(").count(), 1);
        assert!(candidate.contains("if (commandEntered) return \"applied_unverified\""));
        assert!(candidate.contains("commandEntered = true;"));
        assert!(candidate.contains("applied_unverified_logical_editor"));
        for forbidden in [
            "evaluateJavaScript",
            "callAsyncJavaScript",
            "addEventListener",
            "postMessage(",
            "querySelector",
            "setTimeout",
            "insertHTML",
            "deleteFromDocument",
        ] {
            assert!(
                !candidate.contains(forbidden),
                "unexpected candidate capability: {forbidden}"
            );
        }
    }
}
