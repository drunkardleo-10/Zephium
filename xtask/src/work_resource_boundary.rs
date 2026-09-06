//! Mechanical guards for persistent resource ownership and its opt-in adapter.

use std::path::Path;

pub(crate) fn check(root: &Path) -> Result<(), String> {
    let source =
        std::fs::read_to_string(root.join("crates/zephium-agentic/src/work_browser_resource.rs"))
            .map_err(|e| e.to_string())?;
    let port = std::fs::read_to_string(root.join("crates/zephium-agentic/src/context_port.rs"))
        .map_err(|e| e.to_string())?;
    validate(&source, &port)?;
    for (path, required, forbidden) in ADAPTER_RULES {
        let source = std::fs::read_to_string(root.join(path)).map_err(|e| e.to_string())?;
        validate_adapter(&source, required, forbidden).map_err(|e| format!("{path}: {e}"))?;
    }
    Ok(())
}

const ADAPTER_RULES: &[(&str, &[&str], &[&str])] = &[
    (
        "crates/zephium-agentic/src/work_browser_observation.rs",
        &[
            "self.admits_lease(lease,now)?",
            "row.observation_sequence.checked_add(1)",
            "*sequence<=MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS",
            "ContextGeneration::new(lease.generation)",
            "row.observation_sequence=sequence;row.observation=Some(join.clone())",
            "row.observation.as_ref()!=Some(&completion.join)",
            "row.lease.as_ref()!=Some(&completion.join.lease)",
            "now>=completion.join.lease.deadline",
            "SemanticRuntimeSettlement::try_new(self.join.correlation.clone(),outcome)",
            "SemanticObservationBudget::INITIAL_FILTERED",
            "SemanticRuntimeBudget::INITIAL_FILTERED",
        ],
        &[".cancel_run(", "evaluateJavaScript", "ForegroundRendering"],
    ),
    (
        "crates/zephium-engine/src/work_resource_port.rs",
        &[
            "request.resource()!=&self.resource",
            "request.document()!=self.document.as_ref()",
            "state.phase=Phase::Revoking",
            "state.callbacks==0",
            "now<lease.deadline()",
            "state.reads==0&&state.callbacks==0&&!state.notification_pending",
            "admission.reserve_audit().ok()",
            "state.notification_pending||state.phase==Phase::Destroyed",
            "completion(request.complete(outcome))",
            "self.guard.read_terminal_begin()",
            "self.guard.read_terminal_end()",
            "self.permit.release()",
            "Arc::ptr_eq(current,guard)",
            "ingress.rows.len()>=MAX_LIVE_CONTEXTS",
            ">=zephium_agentic::MAX_EXECUTING_CONTEXTS",
        ],
        &[
            "ContextIdentity::new(",
            "ContextJoin",
            "cancel_run(",
            "ForegroundRendering",
        ],
    ),
    (
        "crates/zephium-engine/src/host/work_resource.rs",
        &[
            "self.agent_contexts.len()+self.work_resources.len()>=MAX_LIVE_CONTEXTS",
            "execution_count<=MAX_EXECUTING_CONTEXTS",
            "guard.acquisition_current(lease,now)",
            "!self.erasure_tombstones.contains(&resource.profile())",
            "native_resource.reclassify(NativeResourceClass::AgentContext)",
            "retirement_clean:false",
            "crate::platform::imp::build_owned_work_view(",
            "resource_retained:true",
            "guard.lease_drained(lease)",
            "guard.callbacks_drained()",
            "resource.observation.is_none()",
            "view.semantic_pending_for_audit()==Some(false)",
            "request.invocation().invocation().get()>resource.last_invocation",
            "request.invocation().scope()==zephium_agentic::SemanticRuntimeScopeClass::Initial",
            "dispatch2::DispatchQueue::main().exec_async(move||",
            "Arc::ptr_eq(&resource.guard,&guard)",
            "request.operation()==self.operation&&request.lease()==self.lease.as_ref()",
            "resource.lifecycle_deadline.is_some_and(|deadline|Instant::now()>=deadline)",
        ],
        &[
            "ContextJoin",
            "view_origin",
            "cancel_run(",
            "cancel_semantic(",
            "AgentOwnedContext",
            "ContextNativePermit",
            "dispatch_action(",
            "evaluateJavaScript",
            "ForegroundRendering",
            "probe_",
        ],
    ),
    (
        "crates/zephium-engine/src/platform/work_document_navigation.rs",
        &[
            "state.phase!=Phase::Bootstrap||!state.bootstrap_finished||state.target.is_some()",
            "expected.as_url().as_str()==target",
            "state.native_id==Some(event.id)",
            "Some(target.as_url().as_str())==current",
            "ifstate.phase==Phase::Ready{state.phase=Phase::Refused",
            "state.phase=Phase::Retired",
        ],
        &[
            "ContextRunId",
            "ContextJoin",
            "AgentNavigationOperation",
            "evaluateJavaScript",
        ],
    ),
    (
        "crates/zephium-engine/src/agent_context_port.rs",
        &[
            "if!self.work_is_absent(){return;}",
            "if!self.work_is_absent(){returnErr(ContextPortFailure::ProfileBusy);}",
            "self.schedule_work_lifecycle(request,completion)",
            "self.schedule_work_observation(request,completion)",
        ],
        &[],
    ),
    (
        "crates/zephium-engine/src/host/agent_context.rs",
        &[
            "checked_add(self.work_resources.len())",
            ".filter(|resource|resource.resident())",
            ".filter(|resource|resource.pending())",
            "task.work_ingress_matches(",
            ".any(|resource|resource.profile()==profile)",
            "self.force_shutdown_work_resources()&&self.agent_contexts.is_empty()",
        ],
        &[],
    ),
    (
        "crates/zephium-engine/src/host/content_rules.rs",
        &[
            "self.work_resources",
            "resource.replace_content_policy_registration(registration)",
        ],
        &[],
    ),
];

fn validate_adapter(source: &str, required: &[&str], forbidden: &[&str]) -> Result<(), String> {
    let source = compact(production(source));
    for needle in required {
        require(&source, needle)?;
    }
    for needle in forbidden {
        if source.contains(needle) {
            return Err(format!("Work native boundary forbids {needle}"));
        }
    }
    Ok(())
}

fn production(source: &str) -> &str {
    source
        .split_once("\n#[cfg(test)]\nmod tests")
        .map_or(source, |(production, _)| production)
}

fn compact(source: &str) -> String {
    source.chars().filter(|c| !c.is_whitespace()).collect()
}
fn require(source: &str, needle: &str) -> Result<(), String> {
    if source.contains(needle) {
        Ok(())
    } else {
        Err(format!("Work resource boundary missing {needle}"))
    }
}
fn validate(source: &str, port: &str) -> Result<(), String> {
    let source = compact(source);
    let port = compact(port);
    require(&source, "pubstructWorkBrowserResourceIdentity{work:WorkId,resource:WorkBrowserResourceId,profile:ProfileId,context:ContextId,}")?;
    for needle in [
        "Arc::ptr_eq(&self.0,&other.0)",
        "self.sequence.checked_add(1)",
        "self.rows.len()>=MAX_LIVE_CONTEXTS",
        "row.lease.is_some()).count()>=MAX_EXECUTING_CONTEXTS",
        "row.phase=WorkBrowserResourcePhase::Revoking;row.pending=Some(operation.clone())",
        "row.destruction_attempted=true;row.destruction=Some(operation.clone())",
        "sequence:lease.generation",
        "sequence:resource.incarnation",
        "ifsealed||now>=lease.deadline",
        "WorkBrowserResourceEvent::RevocationRequired(lease.clone())",
        "ifdebt.bounded()&&debt.is_empty()&&resource_retained",
        "resource_retained&&row.observation.is_none()",
        "row.pending.is_none()&&row.destruction.is_none()&&row.lease.is_none()",
        "row.pending.as_ref()!=Some(&completion.operation)",
        "row.destruction.as_ref()==Some(&completion.operation)",
        "WorkBrowserResourceEvent::DebtSettled(row.join.clone())",
        "request:Box<WorkBrowserResourceRequest>",
    ] {
        require(&source, needle)?;
    }
    for forbidden in [
        "ContextIdentity::new(",
        "AgentNativeShutdownProof",
        "std::thread",
        "tokio::",
        "ContextNativeResourceSnapshot",
        "cancel_run(",
        "begin_document_load(",
        "evaluateJavaScript",
        "SEMANTIC_RUNTIME_CHANNEL_STOP",
    ] {
        if source.contains(forbidden) {
            return Err(format!("Work resource boundary forbids {forbidden}"));
        }
    }
    require(&port, "fnwork_resource_lifecycle(&self,request:crate::WorkBrowserResourceRequest,_completion:crate::WorkBrowserResourceCompletionCallback,)->crate::WorkBrowserResourceDispatch{crate::WorkBrowserResourceDispatch::Rejected{request:Box::new(request),failure:ContextPortFailure::Unsupported,}}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const SOURCE: &str = include_str!("../../crates/zephium-agentic/src/work_browser_resource.rs");
    const PORT: &str = include_str!("../../crates/zephium-agentic/src/context_port.rs");
    #[test]
    fn resource_boundary_rejects_run_ownership_false_drain_and_probe_authority() {
        validate(SOURCE, PORT).unwrap();
        let source = compact(production(SOURCE));
        for changed in [
            source.replace("work:WorkId,", "work:ContextRunId,"),
            source.replace("Arc::ptr_eq(&self.0,&other.0)", "true"),
            source.replace("debt.is_empty()&&resource_retained", "true"),
            source.replace("sealed||now>=lease.deadline", "false"),
            source.replace("row.destruction.is_none()", "true"),
            source.replace("sequence:lease.generation", "sequence:self.next()?"),
            format!("{source}\nlet fake = AgentNativeShutdownProof;"),
            format!("{source}\ncancel_run();"),
        ] {
            assert!(validate(&changed, PORT).is_err());
        }
        assert!(validate(
            SOURCE,
            &PORT.replace(
                "failure: ContextPortFailure::Unsupported,",
                "failure: ContextPortFailure::Cancelled,"
            )
        )
        .is_err());
    }
    #[test]
    fn native_boundary_rejects_missing_ownership_guards_and_legacy_authority() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        for (path, required, forbidden) in ADAPTER_RULES {
            let original = std::fs::read_to_string(root.join(path)).unwrap();
            let source = compact(production(&original));
            validate_adapter(&source, required, forbidden)
                .unwrap_or_else(|e| panic!("{path}: {e}"));
            for needle in *required {
                let changed = source.replace(needle, "false");
                assert!(
                    validate_adapter(&changed, required, forbidden).is_err(),
                    "{path}: {needle}"
                );
            }
            for needle in *forbidden {
                assert!(
                    validate_adapter(&format!("{source}{needle}"), required, forbidden).is_err(),
                    "{path}: {needle}"
                );
            }
        }
    }
}
