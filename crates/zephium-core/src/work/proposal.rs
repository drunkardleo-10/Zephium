//! Untrusted draft vocabulary. Keys are temporary references inside one
//! proposal; they are never durable Work identities or execution authority.
use super::*;
use std::collections::BTreeMap;

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkPlanProposal {
    pub nodes: Vec<WorkNodeProposal>,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkNodeProposal {
    pub key: u8,
    pub objective: String,
    pub dependencies: Vec<u8>,
    pub outputs: Vec<WorkExpectedOutput>,
}
impl WorkPlanProposal {
    pub fn validate(&self) -> Result<(), WorkError> {
        if self.nodes.is_empty() || self.nodes.len() > MAX_WORK_NODES {
            return Err(WorkError::Invalid);
        }
        let mut bytes = 0;
        for node in &self.nodes {
            if node.dependencies.len() > 16 {
                return Err(WorkError::Invalid);
            }
            bytes += super::validate_node_content(&node.objective, &node.outputs)?;
        }
        if bytes > MAX_WORK_REQUEST_BYTES / 2 {
            return Err(WorkError::Capacity);
        }
        // Synthetic identities exist only inside validation, never in a reply.
        self.resolve(WorkPlanId::from(0), |key| {
            WorkPlanNodeId::from(u128::from(key) + 1)
        })?
        .validate()
    }
    /// Call only at the trusted Rust application boundary after admission.
    pub fn mint(&self) -> Result<WorkPlanDraft, WorkError> {
        self.validate()?;
        self.resolve(WorkPlanId::generate(), |_| WorkPlanNodeId::generate())
    }
    fn resolve(
        &self,
        id: WorkPlanId,
        mut mint: impl FnMut(u8) -> WorkPlanNodeId,
    ) -> Result<WorkPlanDraft, WorkError> {
        if self.nodes.is_empty() || self.nodes.len() > MAX_WORK_NODES {
            return Err(WorkError::Invalid);
        }
        let mut ids = BTreeMap::new();
        for node in &self.nodes {
            if usize::from(node.key) >= MAX_WORK_NODES
                || ids.insert(node.key, mint(node.key)).is_some()
            {
                return Err(WorkError::Invalid);
            }
        }
        let nodes = self
            .nodes
            .iter()
            .map(|node| {
                Ok(WorkPlanNode {
                    id: *ids.get(&node.key).ok_or(WorkError::Invalid)?,
                    objective: node.objective.clone(),
                    dependencies: node
                        .dependencies
                        .iter()
                        .map(|key| ids.get(key).copied().ok_or(WorkError::Invalid))
                        .collect::<Result<_, _>>()?,
                    outputs: node.outputs.clone(),
                })
            })
            .collect::<Result<_, WorkError>>()?;
        Ok(WorkPlanDraft { id, nodes })
    }
}
impl std::fmt::Debug for WorkPlanProposal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WorkPlanProposal([redacted])")
    }
}
impl std::fmt::Debug for WorkNodeProposal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WorkNodeProposal([redacted])")
    }
}
