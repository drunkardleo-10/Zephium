//! Mechanical guards for the first product-shaped resource/lease boundary.

use std::path::Path;

pub(crate) fn check(root: &Path) -> Result<(), String> {
    let source =
        std::fs::read_to_string(root.join("crates/zephium-agentic/src/work_browser_resource.rs"))
            .map_err(|e| e.to_string())?;
    let port = std::fs::read_to_string(root.join("crates/zephium-agentic/src/context_port.rs"))
        .map_err(|e| e.to_string())?;
    validate(&source, &port)?;
    let native =
        std::fs::read_to_string(root.join("crates/zephium-engine/src/agent_context_port.rs"))
            .map_err(|e| e.to_string())?;
    validate_adapter(&native)
}

fn validate_adapter(source: &str) -> Result<(), String> {
    if compact(source).contains("fnwork_resource_lifecycle(") {
        Err(
            "Work resource native enablement requires an independently reviewed adapter proof"
                .into(),
        )
    } else {
        Ok(())
    }
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
        for changed in [
            SOURCE.replace("work: WorkId,", "work: ContextRunId,"),
            SOURCE.replace("Arc::ptr_eq(&self.0, &other.0)", "true"),
            SOURCE.replace("debt.is_empty() && resource_retained", "true"),
            SOURCE.replace("sealed || now >= lease.deadline", "false"),
            SOURCE.replace("row.destruction.is_none()", "true"),
            SOURCE.replace("sequence: lease.generation", "sequence: self.next()?"),
            format!("{SOURCE}\nlet fake = AgentNativeShutdownProof;"),
            format!("{SOURCE}\ncancel_run();"),
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
    fn resource_boundary_requires_review_before_native_enablement() {
        const NATIVE: &str = include_str!("../../crates/zephium-engine/src/agent_context_port.rs");
        validate_adapter(NATIVE).unwrap();
        assert!(validate_adapter(&format!("{NATIVE}\nfn work_resource_lifecycle() {{}}")).is_err());
    }
}
