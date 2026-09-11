//! Versioned semantic Work boundary. Layout, live handles, credentials and
//! model transcripts do not participate in these contracts.
use serde::{Deserialize, Serialize};
use specta::Type;
use zephium_core::work::{runtime::*, *};

pub type WorkProjectionV1 = WorkRuntimeProjection;
pub use zephium_core::work::artifact::{
    WorkArtifactDataV1, WorkArtifactV1, WorkEvidenceLink, WorkEvidencePreviewV1,
};

/// Retry the same command and expected revision after a lost reply. The Store
/// returns its original receipt plus current facts. Reusing an ID with changed
/// operands is a conflict. Receipt eviction is never automatic.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Type)]
#[serde(deny_unknown_fields)]
pub struct WorkCommandV1 {
    pub version: u16,
    pub work: WorkId,
    pub expected_revision: WorkRevision,
    pub command: WorkCommandId,
    pub intent: WorkRuntimeIntent,
}
impl WorkCommandV1 {
    pub fn into_request(self) -> Result<port::WorkRequest, WorkError> {
        if self.version != 1 {
            return Err(WorkError::Invalid);
        }
        Ok(port::WorkRequest::RuntimeCommand {
            id: self.work,
            expected: self.expected_revision,
            command: self.command,
            intent: self.intent,
        })
    }
}

/// A stale, missed or reordered signal never changes durable execution state.
/// Render only for the matching owner, Work and durable revision.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Type)]
#[serde(deny_unknown_fields)]
pub struct WorkSignalV1 {
    pub version: u16,
    pub profile: String,
    pub work: WorkId,
    pub basis_revision: WorkRevision,
    pub execution: WorkExecutionId,
    pub node: WorkPlanNodeId,
    pub attempt: WorkAttemptId,
    pub activity: WorkActivityV1,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq, Type)]
#[serde(rename_all = "snake_case")]
pub enum WorkActivityV1 {
    Planning,
    Delegating,
    Reading,
    Comparing,
    ProducingArtifact,
    WaitingForApproval,
    WaitingForHuman,
    Cancelling,
    Finishing,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixtures_match_validated_rust_contracts() {
        let state: WorkProjectionV1 =
            serde_json::from_str(include_str!("../fixtures/work-approved-v1.json")).unwrap();
        state.work.validate().unwrap();
        state.executions[0]
            .validate(state.work.plan.as_ref().unwrap(), state.work.revision)
            .unwrap();
        let command: WorkCommandV1 =
            serde_json::from_str(include_str!("../fixtures/work-approve-command-v1.json")).unwrap();
        command.into_request().unwrap().validate().unwrap();
    }
    #[test]
    fn wire_cannot_submit_worker_settlement_or_authority() {
        let id = WorkId::from(1).to_string();
        let input = serde_json::json!({ "version": 1, "work": id, "expected_revision": "2", "command": id,
            "intent": { "kind": "settle", "execution": id, "status": "succeeded" }});
        assert!(serde_json::from_value::<WorkCommandV1>(input).is_err());
        // The generated contract must accept the actual structured projection;
        // there is no opaque JSON escape hatch at the rendering boundary.
        let _types = specta::Types::default()
            .register::<WorkProjectionV1>()
            .register::<WorkCommandV1>()
            .register::<WorkEvidencePreviewV1>()
            .register::<WorkSignalV1>();
    }
}
