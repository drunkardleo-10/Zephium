//! Canonical non-widening delegation topology for the bounded run supervisor.
//!
//! This immutable functional core pre-approves which manifest plan nodes may
//! delegate to which children. It owns no model input, objective text, task,
//! timer, queue, browser context, provider, or native resource.

use std::fmt;

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    AgentPlanNodeId, AgentPlanNodeScope, AgentRunManifest, AgentRunManifestId, ContextRunId,
    MAX_AGENT_PLAN_NODES,
};

mod runtime;
pub use runtime::{
    AgentNodeExecution, AgentProgressActivity, AgentProgressBlocker, AgentProgressOperation,
    AgentProgressResource, AgentProgressResult, AgentProgressState, AgentRunSupervisor,
    AgentSemanticProgress, AgentSupervisorAttemptId, AgentSupervisorCancellation,
    AgentSupervisorCancellationBatch, AgentSupervisorCancellationId,
    AgentSupervisorCancellationReason, AgentSupervisorCancellationTarget,
    AgentSupervisorCompletion, AgentSupervisorContextAssignment,
    AgentSupervisorContextCancellationTarget, AgentSupervisorContextRelease,
    AgentSupervisorContextReleaseOutcome, AgentSupervisorExecutionOutcome,
    AgentSupervisorExecutionReceipt, AgentSupervisorFailure, AgentSupervisorId,
    AgentSupervisorNodeCancellation, AgentSupervisorNodeSnapshot, AgentSupervisorNodeStatus,
    AgentSupervisorRuntimeError, AgentSupervisorRuntimeStatus, AgentSupervisorWait,
};

/// Initial maximum simultaneously live nodes in one supervisor tree.
pub const MAX_AGENT_LIVE_SUPERVISOR_NODES: usize = 8;
/// Initial maximum simultaneously executing nodes in one supervisor tree.
pub const MAX_AGENT_EXECUTING_SUPERVISOR_NODES: usize = 4;
/// Maximum root-to-child delegation edges.
pub const MAX_AGENT_DELEGATION_DEPTH: u8 = 2;

/// One shell-approved manifest node and its direct delegation parent.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AgentDelegationSpec {
    node: AgentPlanNodeId,
    parent: Option<AgentPlanNodeId>,
}

impl AgentDelegationSpec {
    /// Declares one root or direct parent relation without granting authority.
    pub const fn new(node: AgentPlanNodeId, parent: Option<AgentPlanNodeId>) -> Self {
        Self { node, parent }
    }

    /// Exact approved manifest plan node.
    pub const fn node(&self) -> AgentPlanNodeId {
        self.node
    }

    /// Direct parent, or `None` for the sole root.
    pub const fn parent(&self) -> Option<AgentPlanNodeId> {
        self.parent
    }
}

impl fmt::Debug for AgentDelegationSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentDelegationSpec")
            .field("node", &self.node)
            .field("parent", &self.parent)
            .finish()
    }
}

/// Canonical privacy-preserving node projection after inheritance validation.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AgentDelegationNode {
    node: AgentPlanNodeId,
    parent: Option<AgentPlanNodeId>,
    depth: u8,
}

impl AgentDelegationNode {
    /// Exact approved manifest plan node.
    pub const fn node(&self) -> AgentPlanNodeId {
        self.node
    }

    /// Direct delegation parent, absent only for the sole root.
    pub const fn parent(&self) -> Option<AgentPlanNodeId> {
        self.parent
    }

    /// Exact root-relative delegation depth.
    pub const fn depth(&self) -> u8 {
        self.depth
    }
}

impl fmt::Debug for AgentDelegationNode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentDelegationNode")
            .field("node", &self.node)
            .field("parent", &self.parent)
            .field("depth", &self.depth)
            .finish()
    }
}

/// Immutable exact delegation topology for one approved manifest revision.
#[must_use]
pub struct AgentDelegationTopology {
    manifest: AgentRunManifestId,
    manifest_guard: [u8; 32],
    run: ContextRunId,
    root: AgentPlanNodeId,
    nodes: Vec<AgentDelegationNode>,
    guard: [u8; 32],
}

impl AgentDelegationTopology {
    /// Validates connectivity, depth, and parent-to-child non-widening.
    pub fn try_new(
        manifest: &AgentRunManifest,
        specs: Vec<AgentDelegationSpec>,
    ) -> Result<Self, AgentSupervisorContractError> {
        if specs.is_empty() {
            return Err(AgentSupervisorContractError::Empty);
        }
        if specs.len() > MAX_AGENT_PLAN_NODES {
            return Err(AgentSupervisorContractError::TopologyLimit);
        }
        let mut specs = specs;
        specs.sort_by_key(AgentDelegationSpec::node);
        if specs
            .windows(2)
            .any(|pair| pair[0].node() == pair[1].node())
        {
            return Err(AgentSupervisorContractError::DuplicateNode);
        }

        let mut roots = specs.iter().filter(|spec| spec.parent().is_none());
        let root = roots
            .next()
            .map(|spec| spec.node())
            .ok_or(AgentSupervisorContractError::Root)?;
        if roots.next().is_some() {
            return Err(AgentSupervisorContractError::Root);
        }
        for spec in &specs {
            if manifest.plan_node(spec.node()).is_none() {
                return Err(AgentSupervisorContractError::ManifestNodeMissing);
            }
            if spec.parent() == Some(spec.node()) {
                return Err(AgentSupervisorContractError::Cycle);
            }
            if spec.parent().is_some_and(|parent| {
                specs
                    .binary_search_by_key(&parent, AgentDelegationSpec::node)
                    .is_err()
            }) {
                return Err(AgentSupervisorContractError::ParentMissing);
            }
        }

        let mut nodes = Vec::with_capacity(specs.len());
        for spec in &specs {
            let depth = topology_depth(*spec, &specs)?;
            if depth > MAX_AGENT_DELEGATION_DEPTH {
                return Err(AgentSupervisorContractError::Depth);
            }
            if let Some(parent) = spec.parent() {
                let parent_scope = manifest
                    .plan_node(parent)
                    .ok_or(AgentSupervisorContractError::ManifestNodeMissing)?;
                let child_scope = manifest
                    .plan_node(spec.node())
                    .ok_or(AgentSupervisorContractError::ManifestNodeMissing)?;
                if !scope_contains(parent_scope, child_scope) {
                    return Err(AgentSupervisorContractError::Widening);
                }
            }
            nodes.push(AgentDelegationNode {
                node: spec.node(),
                parent: spec.parent(),
                depth,
            });
        }
        if nodes
            .iter()
            .any(|node| node.node() != root && !reaches_root(*node, root, &nodes))
        {
            return Err(AgentSupervisorContractError::Cycle);
        }

        let guard = topology_guard(manifest.guard(), root, &nodes);
        Ok(Self {
            manifest: manifest.id(),
            manifest_guard: manifest.guard(),
            run: manifest.run(),
            root,
            nodes,
            guard,
        })
    }

    /// Exact immutable manifest revision identity.
    pub const fn manifest(&self) -> AgentRunManifestId {
        self.manifest
    }

    /// Exact owning run identity.
    pub const fn run(&self) -> ContextRunId {
        self.run
    }

    /// Sole root plan node.
    pub const fn root(&self) -> AgentPlanNodeId {
        self.root
    }

    /// Canonical node-id ordered topology.
    pub fn nodes(&self) -> &[AgentDelegationNode] {
        &self.nodes
    }

    /// Resolves one exact approved delegation node.
    pub fn node(&self, id: AgentPlanNodeId) -> Option<AgentDelegationNode> {
        let index = self
            .nodes
            .binary_search_by_key(&id, AgentDelegationNode::node)
            .ok()?;
        self.nodes.get(index).copied()
    }

    /// Whether this topology was proven against the exact manifest revision.
    pub fn matches_manifest(&self, manifest: &AgentRunManifest) -> bool {
        self.matches_manifest_revision(manifest.id(), manifest.guard())
    }

    /// Whether two values represent the exact same canonical topology revision.
    pub fn matches_revision(&self, other: &Self) -> bool {
        self.manifest == other.manifest && self.guard == other.guard
    }

    pub(super) const fn guard(&self) -> [u8; 32] {
        self.guard
    }

    pub(super) const fn manifest_guard(&self) -> [u8; 32] {
        self.manifest_guard
    }

    pub(super) fn matches_manifest_revision(
        &self,
        manifest: AgentRunManifestId,
        manifest_guard: [u8; 32],
    ) -> bool {
        self.manifest == manifest && self.manifest_guard == manifest_guard
    }
}

impl fmt::Debug for AgentDelegationTopology {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentDelegationTopology")
            .field("manifest", &self.manifest)
            .field("run", &self.run)
            .field("root", &self.root)
            .field("nodes", &self.nodes.len())
            .field("guard", &"[redacted]")
            .finish()
    }
}

/// Closed immutable-supervisor topology refusal.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentSupervisorContractError {
    /// A supervisor topology requires one root node.
    #[error("agent supervisor topology is empty")]
    Empty,
    /// Approved topology exceeded the manifest node ceiling.
    #[error("agent supervisor topology exceeds its node ceiling")]
    TopologyLimit,
    /// One plan node appeared more than once.
    #[error("agent supervisor topology repeats a plan node")]
    DuplicateNode,
    /// The topology did not contain exactly one root.
    #[error("agent supervisor topology requires exactly one root")]
    Root,
    /// A topology node was absent from the exact manifest revision.
    #[error("agent supervisor node is absent from the manifest")]
    ManifestNodeMissing,
    /// A direct parent was absent from the approved topology.
    #[error("agent supervisor parent is absent from the topology")]
    ParentMissing,
    /// Parent relationships contained a cycle.
    #[error("agent supervisor topology contains a cycle")]
    Cycle,
    /// Root-relative delegation depth exceeded the hard ceiling.
    #[error("agent supervisor delegation depth exceeds its ceiling")]
    Depth,
    /// A child widened at least one parent authority dimension.
    #[error("agent supervisor child widens parent authority")]
    Widening,
}

fn topology_depth(
    spec: AgentDelegationSpec,
    specs: &[AgentDelegationSpec],
) -> Result<u8, AgentSupervisorContractError> {
    let mut depth = 0_u8;
    let mut current = spec;
    while let Some(parent) = current.parent() {
        depth = depth
            .checked_add(1)
            .ok_or(AgentSupervisorContractError::Depth)?;
        if usize::from(depth) > specs.len() {
            return Err(AgentSupervisorContractError::Cycle);
        }
        let index = specs
            .binary_search_by_key(&parent, AgentDelegationSpec::node)
            .map_err(|_| AgentSupervisorContractError::ParentMissing)?;
        current = specs[index];
    }
    Ok(depth)
}

fn reaches_root(
    node: AgentDelegationNode,
    root: AgentPlanNodeId,
    nodes: &[AgentDelegationNode],
) -> bool {
    let mut current = node;
    for _ in 0..nodes.len() {
        let Some(parent) = current.parent() else {
            return current.node() == root;
        };
        let Ok(index) = nodes.binary_search_by_key(&parent, AgentDelegationNode::node) else {
            return false;
        };
        current = nodes[index];
    }
    false
}

fn scope_contains(parent: &AgentPlanNodeScope, child: &AgentPlanNodeScope) -> bool {
    ordered_subset(child.profiles(), parent.profiles())
        && ordered_subset(child.accounts(), parent.accounts())
        && ordered_subset(child.origins(), parent.origins())
        && child.effects().is_subset_of(parent.effects())
        && child.max_sensitivity() <= parent.max_sensitivity()
        && parent.budget().contains(child.budget())
        && child.expires_at() <= parent.expires_at()
}

fn ordered_subset<T: Ord>(child: &[T], parent: &[T]) -> bool {
    child
        .iter()
        .all(|value| parent.binary_search(value).is_ok())
}

fn topology_guard(
    manifest_guard: [u8; 32],
    root: AgentPlanNodeId,
    nodes: &[AgentDelegationNode],
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"ZEPHIUM-AGENT-DELEGATION-TOPOLOGY-1\0");
    hasher.update(manifest_guard);
    hasher.update(root.bytes());
    hasher.update((nodes.len() as u64).to_be_bytes());
    for node in nodes {
        hasher.update(node.node().bytes());
        match node.parent() {
            Some(parent) => {
                hasher.update([1]);
                hasher.update(parent.bytes());
            }
            None => hasher.update([0]),
        }
        hasher.update([node.depth()]);
    }
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AgentAccountScope, AgentActiveEffect, AgentActiveModelCall, AgentEffectId,
        AgentEffectPermit, AgentEffectReceipt, AgentEffectScope, AgentEffectSettlement,
        AgentModelCallId, AgentModelCallReceipt, AgentModelCallSettlement, AgentNeedsHumanReason,
        AgentNeedsHumanTransition, AgentPlanLeaseId, AgentPlanNodeAuthority, AgentPolicyInstant,
        AgentRunBudget, AgentRunScope, ContextCapabilities, ContextCapability, ContextId,
        ContextIdentity, ContextKind, ContextOperationId, ContextProfileLeaseId,
        ContextProfileLeasePurpose, ContextProfileLeaseRegistry, ContextProfileStorageClass,
        ContextRegistry, ContextRegistryError, ContextResourceDisposition, ContextSettlement,
        ContextTerminal, SemanticActionAttemptId, SemanticActionFailure, SemanticEffectClass,
        SemanticOrigin, SemanticSensitivity,
    };
    use zephium_core::ids::ProfileId;

    fn profile(value: u128) -> ProfileId {
        ProfileId::from(value)
    }

    fn origin(host: &str) -> SemanticOrigin {
        SemanticOrigin::parse(&format!("https://{host}.example.test/private")).expect("origin")
    }

    fn effects(values: &[SemanticEffectClass]) -> AgentEffectScope {
        AgentEffectScope::try_new(values).expect("effects")
    }

    fn budget(operations: u32) -> AgentRunBudget {
        AgentRunBudget::try_new(operations, 1_000, 1_000, 1).expect("budget")
    }

    struct NodeInput {
        id: u128,
        origins: Vec<SemanticOrigin>,
        effects: AgentEffectScope,
        sensitivity: SemanticSensitivity,
        operations: u32,
        expiry: u64,
    }

    fn make_manifest(nodes: Vec<NodeInput>) -> AgentRunManifest {
        let origins = vec![origin("a"), origin("b"), origin("c")];
        let run_effects = effects(&[
            SemanticEffectClass::Read,
            SemanticEffectClass::LocalWrite,
            SemanticEffectClass::ExternalWrite,
        ]);
        let scope = AgentRunScope::try_new(
            vec![profile(1)],
            vec![AgentAccountScope::Anonymous],
            origins,
            SemanticSensitivity::Sensitive,
            run_effects,
            Vec::new(),
        )
        .expect("scope");
        let nodes = nodes
            .into_iter()
            .map(|node| {
                AgentPlanNodeScope::new(
                    AgentPlanNodeId::from_raw(node.id),
                    AgentPlanNodeAuthority::try_new(
                        vec![profile(1)],
                        vec![AgentAccountScope::Anonymous],
                        node.origins,
                        node.sensitivity,
                        node.effects,
                    )
                    .expect("authority"),
                    budget(node.operations),
                    AgentPolicyInstant::from_millis(node.expiry),
                )
            })
            .collect();
        AgentRunManifest::try_new(
            AgentRunManifestId::from_raw(1),
            ContextRunId::from_raw(2),
            scope,
            budget(100),
            AgentPolicyInstant::from_millis(1_000),
            AgentPolicyInstant::from_millis(10_000),
            nodes,
        )
        .expect("manifest")
    }

    fn node(
        id: u128,
        origins: &[&str],
        effects_values: &[SemanticEffectClass],
        sensitivity: SemanticSensitivity,
        operations: u32,
        expiry: u64,
    ) -> NodeInput {
        NodeInput {
            id,
            origins: origins.iter().map(|value| origin(value)).collect(),
            effects: effects(effects_values),
            sensitivity,
            operations,
            expiry,
        }
    }

    fn standard_manifest() -> AgentRunManifest {
        make_manifest(vec![
            node(
                1,
                &["a", "b"],
                &[SemanticEffectClass::Read, SemanticEffectClass::LocalWrite],
                SemanticSensitivity::Sensitive,
                50,
                9_000,
            ),
            node(
                2,
                &["a"],
                &[SemanticEffectClass::Read, SemanticEffectClass::LocalWrite],
                SemanticSensitivity::Sensitive,
                25,
                8_000,
            ),
            node(
                3,
                &["a"],
                &[SemanticEffectClass::Read],
                SemanticSensitivity::Public,
                10,
                7_000,
            ),
        ])
    }

    fn standard_topology(manifest: &AgentRunManifest) -> AgentDelegationTopology {
        AgentDelegationTopology::try_new(
            manifest,
            vec![
                AgentDelegationSpec::new(AgentPlanNodeId::from_raw(1), None),
                AgentDelegationSpec::new(
                    AgentPlanNodeId::from_raw(2),
                    Some(AgentPlanNodeId::from_raw(1)),
                ),
                AgentDelegationSpec::new(
                    AgentPlanNodeId::from_raw(3),
                    Some(AgentPlanNodeId::from_raw(2)),
                ),
            ],
        )
        .expect("topology")
    }

    fn single_node_context_manifest(
        manifest_id: u128,
        run_contexts: u8,
        node_contexts: u8,
    ) -> AgentRunManifest {
        let effect_scope = effects(&[SemanticEffectClass::Read]);
        AgentRunManifest::try_new(
            AgentRunManifestId::from_raw(manifest_id),
            ContextRunId::from_raw(2),
            AgentRunScope::try_new(
                vec![profile(1)],
                vec![AgentAccountScope::Anonymous],
                vec![origin("a")],
                SemanticSensitivity::Public,
                effect_scope,
                Vec::new(),
            )
            .expect("scope"),
            AgentRunBudget::try_new(10, 100, 100, run_contexts).expect("run budget"),
            AgentPolicyInstant::from_millis(1_000),
            AgentPolicyInstant::from_millis(10_000),
            vec![AgentPlanNodeScope::new(
                AgentPlanNodeId::from_raw(1),
                AgentPlanNodeAuthority::try_new(
                    vec![profile(1)],
                    vec![AgentAccountScope::Anonymous],
                    vec![origin("a")],
                    SemanticSensitivity::Public,
                    effect_scope,
                )
                .expect("authority"),
                AgentRunBudget::try_new(10, 100, 100, node_contexts).expect("node budget"),
                AgentPolicyInstant::from_millis(9_000),
            )],
        )
        .expect("context manifest")
    }

    fn attempt(value: u64) -> AgentSupervisorAttemptId {
        AgentSupervisorAttemptId::new(value).expect("attempt")
    }

    fn cancellation(value: u64) -> AgentSupervisorCancellationId {
        AgentSupervisorCancellationId::new(value).expect("cancellation")
    }

    fn context(value: u128) -> ContextId {
        ContextId::from_raw(value)
    }

    fn context_identity(value: u128, owner: u128, profile_value: u128) -> ContextIdentity {
        ContextIdentity::new(
            context(value),
            ContextRunId::from_raw(owner),
            profile(profile_value),
            ContextKind::Owned,
        )
    }

    fn context_capabilities() -> ContextCapabilities {
        ContextCapabilities::try_new(
            ContextKind::Owned,
            &[
                ContextCapability::Navigate,
                ContextCapability::Observe,
                ContextCapability::Act,
                ContextCapability::Suspend,
                ContextCapability::Recover,
            ],
        )
        .expect("context capabilities")
    }

    fn context_operation(value: u64) -> ContextOperationId {
        ContextOperationId::new(value).expect("context operation")
    }

    #[test]
    fn canonical_topology_proves_exact_depth_and_manifest_revision() {
        let manifest = standard_manifest();
        let topology = AgentDelegationTopology::try_new(
            &manifest,
            vec![
                AgentDelegationSpec::new(
                    AgentPlanNodeId::from_raw(3),
                    Some(AgentPlanNodeId::from_raw(2)),
                ),
                AgentDelegationSpec::new(AgentPlanNodeId::from_raw(1), None),
                AgentDelegationSpec::new(
                    AgentPlanNodeId::from_raw(2),
                    Some(AgentPlanNodeId::from_raw(1)),
                ),
            ],
        )
        .expect("topology");
        assert_eq!(topology.root(), AgentPlanNodeId::from_raw(1));
        assert_eq!(topology.nodes().len(), 3);
        assert_eq!(
            topology
                .node(AgentPlanNodeId::from_raw(3))
                .expect("node")
                .depth(),
            MAX_AGENT_DELEGATION_DEPTH
        );
        assert!(topology.matches_manifest(&manifest));
        let changed_revision = make_manifest(vec![node(
            1,
            &["a"],
            &[SemanticEffectClass::Read],
            SemanticSensitivity::Public,
            1,
            8_000,
        )]);
        assert!(!topology.matches_manifest(&changed_revision));

        let same = AgentDelegationTopology::try_new(
            &manifest,
            vec![
                AgentDelegationSpec::new(AgentPlanNodeId::from_raw(1), None),
                AgentDelegationSpec::new(
                    AgentPlanNodeId::from_raw(2),
                    Some(AgentPlanNodeId::from_raw(1)),
                ),
                AgentDelegationSpec::new(
                    AgentPlanNodeId::from_raw(3),
                    Some(AgentPlanNodeId::from_raw(2)),
                ),
            ],
        )
        .expect("canonical topology");
        assert!(topology.matches_revision(&same));
        let debug = format!("{topology:?} {:?}", topology.nodes());
        assert!(!debug.contains("a.example.test"));
        assert!(!debug.contains("b.example.test"));
        assert!(debug.contains("[redacted]"));
    }

    #[test]
    fn topology_rejects_shape_cycles_depth_and_unknown_nodes() {
        let approved = standard_manifest();
        assert_eq!(
            AgentDelegationTopology::try_new(&approved, Vec::new()).expect_err("empty"),
            AgentSupervisorContractError::Empty
        );
        assert_eq!(
            AgentDelegationTopology::try_new(
                &approved,
                vec![
                    AgentDelegationSpec::new(AgentPlanNodeId::from_raw(1), None),
                    AgentDelegationSpec::new(AgentPlanNodeId::from_raw(1), None),
                ],
            )
            .expect_err("duplicate"),
            AgentSupervisorContractError::DuplicateNode
        );
        assert_eq!(
            AgentDelegationTopology::try_new(
                &approved,
                vec![
                    AgentDelegationSpec::new(AgentPlanNodeId::from_raw(1), None),
                    AgentDelegationSpec::new(AgentPlanNodeId::from_raw(2), None),
                ],
            )
            .expect_err("two roots"),
            AgentSupervisorContractError::Root
        );
        assert_eq!(
            AgentDelegationTopology::try_new(
                &approved,
                vec![
                    AgentDelegationSpec::new(AgentPlanNodeId::from_raw(1), None),
                    AgentDelegationSpec::new(
                        AgentPlanNodeId::from_raw(2),
                        Some(AgentPlanNodeId::from_raw(99)),
                    ),
                ],
            )
            .expect_err("missing parent"),
            AgentSupervisorContractError::ParentMissing
        );
        assert_eq!(
            AgentDelegationTopology::try_new(
                &approved,
                vec![
                    AgentDelegationSpec::new(AgentPlanNodeId::from_raw(1), None),
                    AgentDelegationSpec::new(
                        AgentPlanNodeId::from_raw(2),
                        Some(AgentPlanNodeId::from_raw(3)),
                    ),
                    AgentDelegationSpec::new(
                        AgentPlanNodeId::from_raw(3),
                        Some(AgentPlanNodeId::from_raw(2)),
                    ),
                ],
            )
            .expect_err("cycle"),
            AgentSupervisorContractError::Cycle
        );

        let deep_manifest = make_manifest(vec![
            node(
                1,
                &["a"],
                &[SemanticEffectClass::Read],
                SemanticSensitivity::Public,
                10,
                9_000,
            ),
            node(
                2,
                &["a"],
                &[SemanticEffectClass::Read],
                SemanticSensitivity::Public,
                9,
                8_000,
            ),
            node(
                3,
                &["a"],
                &[SemanticEffectClass::Read],
                SemanticSensitivity::Public,
                8,
                7_000,
            ),
            node(
                4,
                &["a"],
                &[SemanticEffectClass::Read],
                SemanticSensitivity::Public,
                7,
                6_000,
            ),
        ]);
        assert_eq!(
            AgentDelegationTopology::try_new(
                &deep_manifest,
                vec![
                    AgentDelegationSpec::new(AgentPlanNodeId::from_raw(1), None),
                    AgentDelegationSpec::new(
                        AgentPlanNodeId::from_raw(2),
                        Some(AgentPlanNodeId::from_raw(1))
                    ),
                    AgentDelegationSpec::new(
                        AgentPlanNodeId::from_raw(3),
                        Some(AgentPlanNodeId::from_raw(2))
                    ),
                    AgentDelegationSpec::new(
                        AgentPlanNodeId::from_raw(4),
                        Some(AgentPlanNodeId::from_raw(3))
                    ),
                ],
            )
            .expect_err("depth"),
            AgentSupervisorContractError::Depth
        );
        assert_eq!(
            AgentDelegationTopology::try_new(
                &approved,
                vec![AgentDelegationSpec::new(
                    AgentPlanNodeId::from_raw(99),
                    None
                )],
            )
            .expect_err("unknown"),
            AgentSupervisorContractError::ManifestNodeMissing
        );
        assert_eq!(
            AgentDelegationTopology::try_new(
                &approved,
                (1..=MAX_AGENT_PLAN_NODES + 1)
                    .map(|id| AgentDelegationSpec::new(AgentPlanNodeId::from_raw(id as u128), None))
                    .collect(),
            )
            .expect_err("topology limit"),
            AgentSupervisorContractError::TopologyLimit
        );
    }

    #[test]
    fn child_cannot_widen_any_parent_authority_or_resource_dimension() {
        let cases = [
            make_manifest(vec![
                node(
                    1,
                    &["a"],
                    &[SemanticEffectClass::Read],
                    SemanticSensitivity::Sensitive,
                    50,
                    8_000,
                ),
                node(
                    2,
                    &["a", "b"],
                    &[SemanticEffectClass::Read],
                    SemanticSensitivity::Sensitive,
                    25,
                    7_000,
                ),
            ]),
            make_manifest(vec![
                node(
                    1,
                    &["a"],
                    &[SemanticEffectClass::Read],
                    SemanticSensitivity::Sensitive,
                    50,
                    8_000,
                ),
                node(
                    2,
                    &["a"],
                    &[SemanticEffectClass::Read, SemanticEffectClass::LocalWrite],
                    SemanticSensitivity::Sensitive,
                    25,
                    7_000,
                ),
            ]),
            make_manifest(vec![
                node(
                    1,
                    &["a"],
                    &[SemanticEffectClass::Read],
                    SemanticSensitivity::Public,
                    50,
                    8_000,
                ),
                node(
                    2,
                    &["a"],
                    &[SemanticEffectClass::Read],
                    SemanticSensitivity::Sensitive,
                    25,
                    7_000,
                ),
            ]),
            make_manifest(vec![
                node(
                    1,
                    &["a"],
                    &[SemanticEffectClass::Read],
                    SemanticSensitivity::Sensitive,
                    20,
                    8_000,
                ),
                node(
                    2,
                    &["a"],
                    &[SemanticEffectClass::Read],
                    SemanticSensitivity::Sensitive,
                    25,
                    7_000,
                ),
            ]),
            make_manifest(vec![
                node(
                    1,
                    &["a"],
                    &[SemanticEffectClass::Read],
                    SemanticSensitivity::Sensitive,
                    50,
                    7_000,
                ),
                node(
                    2,
                    &["a"],
                    &[SemanticEffectClass::Read],
                    SemanticSensitivity::Sensitive,
                    25,
                    8_000,
                ),
            ]),
        ];
        for manifest in cases {
            assert_eq!(
                AgentDelegationTopology::try_new(
                    &manifest,
                    vec![
                        AgentDelegationSpec::new(AgentPlanNodeId::from_raw(1), None),
                        AgentDelegationSpec::new(
                            AgentPlanNodeId::from_raw(2),
                            Some(AgentPlanNodeId::from_raw(1)),
                        ),
                    ],
                )
                .expect_err("widening"),
                AgentSupervisorContractError::Widening
            );
        }
    }

    #[test]
    fn mutable_scheduler_executes_a_depth_two_tree_without_retained_wait_work() {
        let manifest = standard_manifest();
        let topology = standard_topology(&manifest);
        let mut supervisor =
            AgentRunSupervisor::new(AgentSupervisorId::new(1).expect("supervisor"), topology);
        let initial = supervisor.status();
        assert_eq!(initial.activated(), 1);
        assert_eq!(initial.live(), 1);
        assert_eq!(initial.executing(), 0);
        assert_eq!(initial.queued(), 1);
        assert_eq!(initial.waiting(), 0);
        assert_eq!(initial.terminal(), 0);
        assert!(!initial.is_sealed());

        let root = supervisor
            .start(AgentPlanNodeId::from_raw(1), attempt(1))
            .expect("start root");
        supervisor
            .delegate(&root, AgentPlanNodeId::from_raw(2))
            .expect("delegate child");
        let root_wait = supervisor
            .complete(root, AgentSupervisorCompletion::Succeeded)
            .expect("parent waits");
        assert_eq!(
            root_wait.outcome(),
            AgentSupervisorExecutionOutcome::Waiting(AgentSupervisorWait::Descendants)
        );
        assert_eq!(supervisor.status().executing(), 0);
        assert_eq!(supervisor.status().waiting(), 1);

        let child = supervisor
            .start(AgentPlanNodeId::from_raw(2), attempt(2))
            .expect("start child");
        supervisor
            .delegate(&child, AgentPlanNodeId::from_raw(3))
            .expect("delegate grandchild");
        let child_wait = supervisor
            .wait(child, AgentSupervisorWait::Descendants)
            .expect("wait child");
        assert_eq!(
            child_wait.outcome(),
            AgentSupervisorExecutionOutcome::Waiting(AgentSupervisorWait::Descendants)
        );

        let grandchild = supervisor
            .start(AgentPlanNodeId::from_raw(3), attempt(3))
            .expect("start grandchild");
        let grandchild_receipt = supervisor
            .complete(grandchild, AgentSupervisorCompletion::Succeeded)
            .expect("complete grandchild");
        assert_eq!(
            grandchild_receipt.outcome(),
            AgentSupervisorExecutionOutcome::Succeeded
        );
        let child = supervisor
            .start(AgentPlanNodeId::from_raw(2), attempt(4))
            .expect("resume child");
        supervisor
            .complete(child, AgentSupervisorCompletion::Succeeded)
            .expect("complete child");
        let root = supervisor
            .start(AgentPlanNodeId::from_raw(1), attempt(5))
            .expect("resume root");
        supervisor
            .complete(root, AgentSupervisorCompletion::Succeeded)
            .expect("complete root");

        assert_eq!(supervisor.status().live(), 0);
        assert_eq!(supervisor.status().executing(), 0);
        assert_eq!(supervisor.status().terminal(), 3);
        assert!(supervisor.nodes().all(|node| node.status().is_terminal()));
        assert_eq!(
            supervisor
                .start(AgentPlanNodeId::from_raw(1), attempt(6))
                .expect_err("terminal node cannot reopen"),
            AgentSupervisorRuntimeError::NotRunnable
        );
        assert!(!supervisor.status().is_sealed());
    }

    #[test]
    fn mutable_scheduler_enforces_live_and_execution_limits_without_eviction() {
        let mut inputs = vec![node(
            1,
            &["a"],
            &[SemanticEffectClass::Read],
            SemanticSensitivity::Public,
            50,
            9_000,
        )];
        for id in 2..=9_u128 {
            inputs.push(node(
                id,
                &["a"],
                &[SemanticEffectClass::Read],
                SemanticSensitivity::Public,
                10,
                8_000,
            ));
        }
        let manifest = make_manifest(inputs);
        let mut specs = vec![AgentDelegationSpec::new(AgentPlanNodeId::from_raw(1), None)];
        for id in 2..=9_u128 {
            specs.push(AgentDelegationSpec::new(
                AgentPlanNodeId::from_raw(id),
                Some(AgentPlanNodeId::from_raw(1)),
            ));
        }
        let topology = AgentDelegationTopology::try_new(&manifest, specs).expect("wide topology");
        let mut supervisor =
            AgentRunSupervisor::new(AgentSupervisorId::new(2).expect("supervisor"), topology);
        let root = supervisor
            .start(AgentPlanNodeId::from_raw(1), attempt(1))
            .expect("start root");
        for id in 2..=8_u128 {
            supervisor
                .delegate(&root, AgentPlanNodeId::from_raw(id))
                .expect("delegate within live ceiling");
        }
        assert_eq!(supervisor.status().live(), MAX_AGENT_LIVE_SUPERVISOR_NODES);
        assert_eq!(
            supervisor
                .delegate(&root, AgentPlanNodeId::from_raw(9))
                .expect_err("live ceiling"),
            AgentSupervisorRuntimeError::LiveLimit
        );
        assert_eq!(supervisor.node_status(AgentPlanNodeId::from_raw(9)), None);

        let child_two = supervisor
            .start(AgentPlanNodeId::from_raw(2), attempt(2))
            .expect("start child two");
        supervisor
            .complete(child_two, AgentSupervisorCompletion::Succeeded)
            .expect("complete child two");
        supervisor
            .delegate(&root, AgentPlanNodeId::from_raw(9))
            .expect("terminal child released live slot");
        assert_eq!(supervisor.status().activated(), 9);
        assert_eq!(supervisor.status().live(), MAX_AGENT_LIVE_SUPERVISOR_NODES);

        let child_three = supervisor
            .start(AgentPlanNodeId::from_raw(3), attempt(3))
            .expect("start child three");
        let _child_four = supervisor
            .start(AgentPlanNodeId::from_raw(4), attempt(4))
            .expect("start child four");
        let _child_five = supervisor
            .start(AgentPlanNodeId::from_raw(5), attempt(5))
            .expect("start child five");
        assert_eq!(
            supervisor.status().executing(),
            MAX_AGENT_EXECUTING_SUPERVISOR_NODES
        );
        assert_eq!(
            supervisor
                .start(AgentPlanNodeId::from_raw(6), attempt(6))
                .expect_err("execution ceiling"),
            AgentSupervisorRuntimeError::ExecutionLimit
        );
        supervisor
            .complete(child_three, AgentSupervisorCompletion::Succeeded)
            .expect("release execution slot");
        let _child_six = supervisor
            .start(AgentPlanNodeId::from_raw(6), attempt(6))
            .expect("failed admission did not consume attempt");
        assert_eq!(supervisor.status().executing(), 4);
        assert!(!supervisor.status().is_sealed());
    }

    #[test]
    fn mutable_scheduler_rejects_attempt_replay_and_cross_instance_tokens() {
        let manifest = standard_manifest();
        let topology_one = standard_topology(&manifest);
        let topology_two = standard_topology(&manifest);
        let mut first = AgentRunSupervisor::new(
            AgentSupervisorId::new(10).expect("supervisor"),
            topology_one,
        );
        let mut second = AgentRunSupervisor::new(
            AgentSupervisorId::new(11).expect("supervisor"),
            topology_two,
        );
        let first_execution = first
            .start(AgentPlanNodeId::from_raw(1), attempt(1))
            .expect("first execution");
        let _second_execution = second
            .start(AgentPlanNodeId::from_raw(1), attempt(1))
            .expect("second execution");
        assert_eq!(
            second
                .wait(first_execution, AgentSupervisorWait::Yielded)
                .expect_err("cross-instance token"),
            AgentSupervisorRuntimeError::ExecutionMismatch
        );
        assert!(second.status().is_sealed());
        assert_eq!(second.status().executing(), 1);

        let manifest = standard_manifest();
        let mut replay = AgentRunSupervisor::new(
            AgentSupervisorId::new(12).expect("supervisor"),
            standard_topology(&manifest),
        );
        let execution = replay
            .start(AgentPlanNodeId::from_raw(1), attempt(1))
            .expect("execution");
        replay
            .wait(execution, AgentSupervisorWait::Yielded)
            .expect("yield");
        assert_eq!(
            replay
                .start(AgentPlanNodeId::from_raw(1), attempt(1))
                .expect_err("attempt replay"),
            AgentSupervisorRuntimeError::AttemptReplay
        );
        let execution = replay
            .start(AgentPlanNodeId::from_raw(1), attempt(2))
            .expect("next attempt");
        let failure = AgentSupervisorFailure::Action(SemanticActionFailure::BackendRefused);
        let receipt = replay
            .complete(execution, AgentSupervisorCompletion::Failed(failure))
            .expect("terminal failure");
        assert_eq!(
            receipt.outcome(),
            AgentSupervisorExecutionOutcome::Failed(failure)
        );
        let debug = format!("{replay:?} {receipt:?}");
        assert!(!debug.contains("a.example.test"));
        assert!(debug.contains("[redacted]"));
    }

    #[test]
    fn cancellation_tree_retains_execution_capacity_until_exact_terminal_drain() {
        let manifest = standard_manifest();
        let mut supervisor = AgentRunSupervisor::new(
            AgentSupervisorId::new(20).expect("supervisor"),
            standard_topology(&manifest),
        );
        let root_id = AgentPlanNodeId::from_raw(1);
        let child_id = AgentPlanNodeId::from_raw(2);
        let grandchild_id = AgentPlanNodeId::from_raw(3);
        let root = supervisor.start(root_id, attempt(1)).expect("start root");
        supervisor
            .delegate(&root, child_id)
            .expect("delegate child");
        supervisor
            .wait(root, AgentSupervisorWait::Descendants)
            .expect("wait root");
        let child = supervisor.start(child_id, attempt(2)).expect("start child");
        supervisor
            .delegate(&child, grandchild_id)
            .expect("delegate grandchild");
        supervisor
            .wait(child, AgentSupervisorWait::Descendants)
            .expect("wait child");
        let grandchild = supervisor
            .start(grandchild_id, attempt(3))
            .expect("start grandchild");

        let batch = supervisor
            .cancel_subtree(
                root_id,
                cancellation(1),
                AgentSupervisorCancellationReason::UserRequested,
            )
            .expect("cancel complete tree");
        assert_eq!(batch.root(), root_id);
        assert_eq!(batch.affected(), 3);
        assert_eq!(batch.terminal(), 0);
        assert_eq!(batch.targets().len(), 1);
        let target = batch.targets().next().expect("one drain target");
        assert_eq!(target.node(), grandchild_id);
        assert_eq!(target.attempt(), attempt(3));
        assert_eq!(target.cancellation().id(), cancellation(1));
        assert_eq!(supervisor.status().live(), 3);
        assert_eq!(supervisor.status().executing(), 1);
        assert_eq!(supervisor.status().cancelling(), 3);
        assert_eq!(supervisor.status().cancelled(), 0);
        assert_eq!(
            supervisor
                .start(root_id, attempt(4))
                .expect_err("cancelling root cannot resume"),
            AgentSupervisorRuntimeError::NotRunnable
        );

        let receipt = supervisor
            .drain_cancelled(grandchild, cancellation(1))
            .expect("exact drain");
        let AgentSupervisorExecutionOutcome::Cancelled(cancelled) = receipt.outcome() else {
            panic!("execution must be cancelled");
        };
        assert_eq!(cancelled.id(), cancellation(1));
        assert_eq!(
            cancelled.reason(),
            AgentSupervisorCancellationReason::UserRequested
        );
        assert_eq!(supervisor.status().live(), 0);
        assert_eq!(supervisor.status().executing(), 0);
        assert_eq!(supervisor.status().cancelling(), 0);
        assert_eq!(supervisor.status().terminal(), 3);
        assert_eq!(supervisor.status().cancelled(), 3);
        assert!(supervisor
            .nodes()
            .all(|node| matches!(node.status(), AgentSupervisorNodeStatus::Cancelled(_))));
    }

    #[test]
    fn queued_cancellation_is_immediate_and_late_completion_cannot_claim_success() {
        let manifest = standard_manifest();
        let mut supervisor = AgentRunSupervisor::new(
            AgentSupervisorId::new(21).expect("supervisor"),
            standard_topology(&manifest),
        );
        let root_id = AgentPlanNodeId::from_raw(1);
        let child_id = AgentPlanNodeId::from_raw(2);
        let grandchild_id = AgentPlanNodeId::from_raw(3);
        let root = supervisor.start(root_id, attempt(1)).expect("start root");
        supervisor
            .delegate(&root, child_id)
            .expect("delegate child");

        let child_cancel = supervisor
            .cancel_subtree(
                child_id,
                cancellation(1),
                AgentSupervisorCancellationReason::ParentTerminated,
            )
            .expect("cancel queued child");
        assert_eq!(child_cancel.affected(), 1);
        assert_eq!(child_cancel.terminal(), 1);
        assert_eq!(child_cancel.targets().len(), 0);
        assert_eq!(supervisor.status().live(), 1);
        assert_eq!(supervisor.status().executing(), 1);
        assert_eq!(supervisor.status().cancelled(), 1);
        assert_eq!(
            supervisor
                .cancel_subtree(
                    child_id,
                    cancellation(1),
                    AgentSupervisorCancellationReason::Shutdown,
                )
                .expect_err("cancellation identity cannot replay"),
            AgentSupervisorRuntimeError::CancellationReplay
        );
        assert_eq!(
            supervisor
                .delegate(&root, grandchild_id)
                .expect_err("grandchild requires its exact direct parent"),
            AgentSupervisorRuntimeError::DelegationMismatch
        );

        let root_cancel = supervisor
            .cancel_subtree(
                root_id,
                cancellation(2),
                AgentSupervisorCancellationReason::Shutdown,
            )
            .expect("cancel running root");
        assert_eq!(root_cancel.targets().len(), 1);
        assert_eq!(supervisor.status().executing(), 1);
        assert_eq!(
            supervisor
                .delegate(&root, child_id)
                .expect_err("cancellation revokes delegation"),
            AgentSupervisorRuntimeError::CancellationPending
        );
        let receipt = supervisor
            .complete(root, AgentSupervisorCompletion::Succeeded)
            .expect("late completion is a drain callback");
        let AgentSupervisorExecutionOutcome::Cancelled(cancelled) = receipt.outcome() else {
            panic!("late success must settle as cancelled");
        };
        assert_eq!(cancelled.id(), cancellation(2));
        assert_eq!(
            cancelled.reason(),
            AgentSupervisorCancellationReason::Shutdown
        );
        assert_eq!(supervisor.status().live(), 0);
        assert_eq!(supervisor.status().terminal(), 2);
        assert_eq!(supervisor.status().cancelled(), 2);
    }

    #[test]
    fn nested_cancellation_preserves_prior_branch_and_mismatch_fails_stopped() {
        let manifest = standard_manifest();
        let mut supervisor = AgentRunSupervisor::new(
            AgentSupervisorId::new(22).expect("supervisor"),
            standard_topology(&manifest),
        );
        let root_id = AgentPlanNodeId::from_raw(1);
        let child_id = AgentPlanNodeId::from_raw(2);
        let grandchild_id = AgentPlanNodeId::from_raw(3);
        let root = supervisor.start(root_id, attempt(1)).expect("start root");
        supervisor
            .delegate(&root, child_id)
            .expect("delegate child");
        let child = supervisor.start(child_id, attempt(2)).expect("start child");
        supervisor
            .delegate(&child, grandchild_id)
            .expect("delegate grandchild");
        let grandchild = supervisor
            .start(grandchild_id, attempt(3))
            .expect("start grandchild");

        let branch = supervisor
            .cancel_subtree(
                child_id,
                cancellation(1),
                AgentSupervisorCancellationReason::BudgetExhausted,
            )
            .expect("cancel child branch");
        assert_eq!(branch.affected(), 2);
        assert_eq!(branch.targets().len(), 2);
        let run = supervisor
            .cancel_subtree(
                root_id,
                cancellation(2),
                AgentSupervisorCancellationReason::UserRequested,
            )
            .expect("cancel remaining run");
        assert_eq!(run.affected(), 1);
        assert_eq!(run.targets().len(), 3);
        assert_eq!(supervisor.cancellation_targets().count(), 3);
        assert_eq!(
            run.targets()
                .find(|target| target.node() == child_id)
                .expect("existing branch target")
                .cancellation()
                .id(),
            cancellation(1)
        );
        assert_eq!(supervisor.status().executing(), 3);
        let AgentSupervisorNodeStatus::Cancelling(child_cancellation) = supervisor
            .node_status(child_id)
            .expect("child cancellation")
        else {
            panic!("child must remain cancellation-pending");
        };
        assert_eq!(child_cancellation.cancellation().id(), cancellation(1));
        assert_eq!(
            child_cancellation.cancellation().reason(),
            AgentSupervisorCancellationReason::BudgetExhausted
        );
        assert_eq!(child_cancellation.draining_attempt(), Some(attempt(2)));

        supervisor
            .drain_cancelled(root, cancellation(2))
            .expect("drain root");
        supervisor
            .drain_cancelled(grandchild, cancellation(1))
            .expect("drain grandchild");
        assert_eq!(supervisor.status().executing(), 1);
        assert_eq!(supervisor.status().cancelling(), 2);
        assert_eq!(supervisor.status().cancelled(), 1);
        assert_eq!(
            supervisor
                .drain_cancelled(child, cancellation(2))
                .expect_err("wrong branch cancellation"),
            AgentSupervisorRuntimeError::CancellationMismatch
        );
        assert!(supervisor.status().is_sealed());
        assert_eq!(supervisor.status().executing(), 1);
        assert_eq!(supervisor.status().live(), 2);
        let debug = format!("{supervisor:?} {branch:?} {run:?}");
        assert!(!debug.contains("a.example.test"));
        assert!(debug.contains("[redacted]"));
    }

    #[test]
    fn context_assignment_rejoins_manifest_authority_and_both_context_budgets() {
        let manifest = standard_manifest();
        let mut supervisor = AgentRunSupervisor::new(
            AgentSupervisorId::new(30).expect("supervisor"),
            standard_topology(&manifest),
        );
        let root_id = AgentPlanNodeId::from_raw(1);
        let root = supervisor.start(root_id, attempt(1)).expect("start root");
        let mut registry = ContextRegistry::new();
        let capabilities = context_capabilities();

        assert_eq!(
            supervisor
                .reserve_context(
                    &root,
                    &manifest,
                    &mut registry,
                    context_identity(1, 99, 1),
                    capabilities,
                )
                .expect_err("wrong owner"),
            AgentSupervisorRuntimeError::ContextAuthority
        );
        assert_eq!(
            supervisor
                .reserve_context(
                    &root,
                    &manifest,
                    &mut registry,
                    context_identity(1, 2, 99),
                    capabilities,
                )
                .expect_err("wrong profile"),
            AgentSupervisorRuntimeError::ContextAuthority
        );
        let other_manifest = make_manifest(vec![node(
            1,
            &["a"],
            &[SemanticEffectClass::Read],
            SemanticSensitivity::Public,
            1,
            2_000,
        )]);
        assert_eq!(
            supervisor
                .reserve_context(
                    &root,
                    &other_manifest,
                    &mut registry,
                    context_identity(1, 2, 1),
                    capabilities,
                )
                .expect_err("different manifest revision"),
            AgentSupervisorRuntimeError::ManifestMismatch
        );

        let preexisting = context_identity(9, 2, 1);
        registry
            .reserve(preexisting, capabilities)
            .expect("preexisting registry row");
        assert_eq!(
            supervisor
                .reserve_context(&root, &manifest, &mut registry, preexisting, capabilities,)
                .expect_err("registry duplicate"),
            AgentSupervisorRuntimeError::ContextRegistry(ContextRegistryError::Duplicate)
        );
        assert_eq!(supervisor.status().contexts(), 0);
        registry
            .cancel_queued(preexisting.id())
            .expect("remove preexisting row");

        let identity = context_identity(1, 2, 1);
        let assignment = supervisor
            .reserve_context(&root, &manifest, &mut registry, identity, capabilities)
            .expect("reserve exact context");
        assert_eq!(assignment.node(), root_id);
        assert_eq!(assignment.identity(), identity);
        assert_eq!(supervisor.status().contexts(), 1);
        assert_eq!(registry.status().total(), 1);
        assert_eq!(
            supervisor
                .reserve_context(&root, &manifest, &mut registry, identity, capabilities)
                .expect_err("duplicate assignment"),
            AgentSupervisorRuntimeError::ContextDuplicate
        );
        assert_eq!(
            supervisor
                .reserve_context(
                    &root,
                    &manifest,
                    &mut registry,
                    context_identity(2, 2, 1),
                    capabilities,
                )
                .expect_err("run context budget"),
            AgentSupervisorRuntimeError::RunContextLimit
        );
        assert_eq!(registry.status().total(), 1);
        assert!(!supervisor.status().is_sealed());

        let node_manifest = single_node_context_manifest(90, 2, 1);
        let node_topology = AgentDelegationTopology::try_new(
            &node_manifest,
            vec![AgentDelegationSpec::new(root_id, None)],
        )
        .expect("single-node topology");
        let mut node_supervisor = AgentRunSupervisor::new(
            AgentSupervisorId::new(34).expect("supervisor"),
            node_topology,
        );
        let node_execution = node_supervisor
            .start(root_id, attempt(1))
            .expect("start single node");
        let mut node_registry = ContextRegistry::new();
        node_supervisor
            .reserve_context(
                &node_execution,
                &node_manifest,
                &mut node_registry,
                context_identity(3, 2, 1),
                capabilities,
            )
            .expect("first node context");
        assert_eq!(
            node_supervisor
                .reserve_context(
                    &node_execution,
                    &node_manifest,
                    &mut node_registry,
                    context_identity(4, 2, 1),
                    capabilities,
                )
                .expect_err("node context budget"),
            AgentSupervisorRuntimeError::NodeContextLimit
        );
        assert_eq!(node_registry.status().total(), 1);
    }

    #[test]
    fn node_completion_waits_for_registry_proven_context_disposition() {
        let manifest = standard_manifest();
        let mut supervisor = AgentRunSupervisor::new(
            AgentSupervisorId::new(31).expect("supervisor"),
            standard_topology(&manifest),
        );
        let root_id = AgentPlanNodeId::from_raw(1);
        let root = supervisor.start(root_id, attempt(1)).expect("start root");
        let mut registry = ContextRegistry::new();
        let identity = context_identity(10, 2, 1);
        supervisor
            .reserve_context(
                &root,
                &manifest,
                &mut registry,
                identity,
                context_capabilities(),
            )
            .expect("assign context");
        let waiting = supervisor
            .complete(root, AgentSupervisorCompletion::Succeeded)
            .expect("context wait");
        assert_eq!(
            waiting.outcome(),
            AgentSupervisorExecutionOutcome::Waiting(AgentSupervisorWait::Contexts)
        );
        assert_eq!(supervisor.status().executing(), 0);
        assert_eq!(
            supervisor
                .start(root_id, attempt(2))
                .expect_err("assigned context blocks terminal retry"),
            AgentSupervisorRuntimeError::ContextsLive
        );
        let release = supervisor
            .cancel_queued_context(&mut registry, identity.id())
            .expect("registry-proven queued cancellation");
        assert_eq!(release.assignment().identity(), identity);
        assert_eq!(
            release.outcome(),
            AgentSupervisorContextReleaseOutcome::QueuedCancelled
        );
        assert_eq!(supervisor.status().contexts(), 0);
        assert_eq!(registry.status().total(), 0);
        let root = supervisor
            .start(root_id, attempt(2))
            .expect("resume after disposition");
        supervisor
            .complete(root, AgentSupervisorCompletion::Succeeded)
            .expect("terminal root");
        assert_eq!(supervisor.status().terminal(), 1);
    }

    #[test]
    fn cancellation_keeps_context_owner_live_until_cleanup_and_active_reap_is_exact() {
        let manifest = standard_manifest();
        let root_id = AgentPlanNodeId::from_raw(1);
        let mut supervisor = AgentRunSupervisor::new(
            AgentSupervisorId::new(32).expect("supervisor"),
            standard_topology(&manifest),
        );
        let root = supervisor.start(root_id, attempt(1)).expect("start root");
        let mut registry = ContextRegistry::new();
        let mut profile_leases = ContextProfileLeaseRegistry::new();
        let identity = context_identity(20, 2, 1);
        let profile_lease = profile_leases
            .acquire(
                ContextProfileLeaseId::new(1).expect("profile lease"),
                identity,
                ContextProfileStorageClass::Durable,
                ContextProfileLeasePurpose::Owned,
            )
            .expect("profile lease");
        supervisor
            .reserve_context(
                &root,
                &manifest,
                &mut registry,
                identity,
                context_capabilities(),
            )
            .expect("assign context");
        let batch = supervisor
            .cancel_subtree(
                root_id,
                cancellation(1),
                AgentSupervisorCancellationReason::Shutdown,
            )
            .expect("cancel run");
        assert_eq!(batch.contexts().len(), 1);
        let target = batch.contexts().next().expect("context target");
        assert_eq!(target.assignment().identity(), identity);
        assert_eq!(target.cancellation().id(), cancellation(1));
        supervisor
            .drain_cancelled(root, cancellation(1))
            .expect("execution drained");
        assert_eq!(supervisor.status().executing(), 0);
        assert_eq!(supervisor.status().live(), 1);
        assert_eq!(supervisor.status().contexts(), 1);
        assert_eq!(supervisor.context_cancellation_targets().count(), 1);
        let queued_release = supervisor
            .cancel_queued_context(&mut registry, identity.id())
            .expect("cleanup context");
        assert_eq!(
            profile_leases.release(profile_lease, queued_release),
            Ok(identity)
        );
        assert_eq!(supervisor.status().live(), 0);
        assert_eq!(supervisor.status().cancelled(), 1);

        let manifest = standard_manifest();
        let mut active_supervisor = AgentRunSupervisor::new(
            AgentSupervisorId::new(33).expect("supervisor"),
            standard_topology(&manifest),
        );
        let execution = active_supervisor
            .start(root_id, attempt(1))
            .expect("start root");
        let mut active_registry = ContextRegistry::new();
        let active_identity = context_identity(21, 2, 1);
        let active_profile_lease = profile_leases
            .acquire(
                ContextProfileLeaseId::new(2).expect("profile lease"),
                active_identity,
                ContextProfileStorageClass::Durable,
                ContextProfileLeasePurpose::Owned,
            )
            .expect("active profile lease");
        active_supervisor
            .reserve_context(
                &execution,
                &manifest,
                &mut active_registry,
                active_identity,
                context_capabilities(),
            )
            .expect("assign active context");
        let construction = active_registry
            .begin_context(active_identity.id(), context_operation(1))
            .expect("begin construction");
        active_registry
            .settle_construction(
                active_identity.id(),
                construction,
                ContextSettlement::Applied,
            )
            .expect("settle construction");
        let close = active_registry
            .begin_close(active_identity.id(), context_operation(2))
            .expect("begin close");
        active_registry
            .settle_close(active_identity.id(), close, ContextSettlement::Applied)
            .expect("settle close");
        let release = active_supervisor
            .reap_terminal_context(&mut active_registry, active_identity.id())
            .expect("reap exact terminal context");
        assert_eq!(
            release.outcome(),
            AgentSupervisorContextReleaseOutcome::Retired {
                terminal: ContextTerminal::Closed,
                resource: ContextResourceDisposition::Destroyed,
            }
        );
        assert_eq!(
            profile_leases.release(active_profile_lease, release),
            Ok(active_identity)
        );
        active_supervisor
            .complete(execution, AgentSupervisorCompletion::Succeeded)
            .expect("complete after reap");
        assert_eq!(active_supervisor.status().contexts(), 0);
        assert_eq!(active_supervisor.status().terminal(), 1);
    }

    #[test]
    fn semantic_progress_tracks_scheduler_activity_result_and_blocker_without_content() {
        let manifest = standard_manifest();
        let manifest_id = manifest.id();
        let supervisor_id = AgentSupervisorId::new(40).expect("supervisor");
        let root_id = AgentPlanNodeId::from_raw(1);
        let mut supervisor = AgentRunSupervisor::new(supervisor_id, standard_topology(&manifest));

        let queued = supervisor
            .semantic_progress(root_id)
            .expect("queued progress");
        assert_eq!(queued.manifest(), manifest_id);
        assert_eq!(queued.supervisor(), supervisor_id);
        assert_eq!(queued.responsibility(), root_id);
        assert_eq!(
            queued.activity().operation(),
            AgentProgressOperation::Scheduling
        );
        assert_eq!(queued.activity().resource(), None);
        assert_eq!(queued.state(), AgentProgressState::Queued);
        assert_eq!(queued.result(), None);
        assert_eq!(queued.blocker(), None);
        assert_eq!(supervisor.nodes().next().expect("root").progress(), queued);

        let execution = supervisor.start(root_id, attempt(1)).expect("start root");
        let active = supervisor
            .semantic_progress(root_id)
            .expect("active progress");
        assert_eq!(active.state(), AgentProgressState::Active);
        assert_eq!(
            active.activity().resource(),
            Some(AgentProgressResource::Execution(attempt(1)))
        );
        assert_eq!(
            AgentProgressActivity::try_new(AgentProgressOperation::Observation, None)
                .expect_err("observation needs a context"),
            AgentSupervisorRuntimeError::ProgressResourceMismatch
        );

        let observation = AgentProgressActivity::try_new(
            AgentProgressOperation::Observation,
            Some(AgentProgressResource::Context(context(900))),
        )
        .expect("typed observation activity");
        supervisor
            .record_progress_activity(&execution, observation)
            .expect("record observation");
        let waiting = supervisor
            .wait(execution, AgentSupervisorWait::Yielded)
            .expect("yield root");
        assert_eq!(
            waiting.outcome(),
            AgentSupervisorExecutionOutcome::Waiting(AgentSupervisorWait::Yielded)
        );
        let progress = supervisor
            .semantic_progress(root_id)
            .expect("waiting progress");
        assert_eq!(progress.activity(), observation);
        assert_eq!(progress.state(), AgentProgressState::Waiting);
        assert_eq!(
            progress.result(),
            Some(AgentProgressResult::Supervisor(
                AgentSupervisorExecutionOutcome::Waiting(AgentSupervisorWait::Yielded)
            ))
        );
        assert_eq!(
            progress.blocker(),
            Some(AgentProgressBlocker::Scheduler(
                AgentSupervisorWait::Yielded
            ))
        );

        let execution = supervisor.start(root_id, attempt(2)).expect("resume root");
        supervisor
            .complete(
                execution,
                AgentSupervisorCompletion::Failed(AgentSupervisorFailure::PolicyDenied),
            )
            .expect("terminal failure");
        let failed = supervisor
            .semantic_progress(root_id)
            .expect("failed progress");
        assert_eq!(failed.state(), AgentProgressState::Failed);
        assert_eq!(
            failed.blocker(),
            Some(AgentProgressBlocker::Supervisor(
                AgentSupervisorFailure::PolicyDenied
            ))
        );
        let debug = format!("{supervisor:?} {failed:?}");
        assert!(debug.contains("[redacted]"));
        assert!(!debug.contains("a.example.test"));
    }

    #[test]
    fn semantic_progress_accepts_only_exact_manifest_node_receipts() {
        let manifest = standard_manifest();
        let root_id = AgentPlanNodeId::from_raw(1);
        let lease = AgentPlanLeaseId::from_raw(501);
        let mut supervisor = AgentRunSupervisor::new(
            AgentSupervisorId::new(41).expect("supervisor"),
            standard_topology(&manifest),
        );
        let execution = supervisor.start(root_id, attempt(1)).expect("start root");
        let active_model = AgentActiveModelCall::for_progress_test(
            &manifest,
            AgentModelCallId::new(1).expect("model call"),
            lease,
            root_id,
        );
        let active_model_progress = supervisor
            .record_active_model_call(&execution, &active_model)
            .expect("active model progress");
        assert_eq!(active_model_progress.state(), AgentProgressState::Active);
        let model = AgentModelCallReceipt::for_progress_test(
            &manifest,
            AgentModelCallId::new(1).expect("model call"),
            lease,
            root_id,
            AgentModelCallSettlement::Completed,
        );
        let model_progress = supervisor
            .record_model_call_result(&execution, model)
            .expect("model progress");
        assert_eq!(model_progress.state(), AgentProgressState::Succeeded);
        assert_eq!(
            model_progress.result(),
            Some(AgentProgressResult::Model(
                AgentModelCallSettlement::Completed
            ))
        );

        let permit = AgentEffectPermit::for_progress_test(
            &manifest,
            AgentEffectId::new(1).expect("effect"),
            lease,
            root_id,
            SemanticEffectClass::LocalWrite,
        );
        supervisor
            .record_effect_permit(&execution, &permit)
            .expect("effect permit progress");
        let active_effect = AgentActiveEffect::for_progress_test(
            &manifest,
            AgentEffectId::new(1).expect("effect"),
            lease,
            root_id,
            SemanticEffectClass::LocalWrite,
            SemanticActionAttemptId::new(1).expect("action attempt"),
        );
        supervisor
            .record_active_effect(&execution, &active_effect)
            .expect("active effect progress");
        let effect = AgentEffectReceipt::for_progress_test(
            &manifest,
            AgentEffectId::new(1).expect("effect"),
            lease,
            root_id,
            SemanticEffectClass::LocalWrite,
            SemanticActionAttemptId::new(1).expect("action attempt"),
            AgentEffectSettlement::Failed(SemanticActionFailure::BackendRefused),
        );
        let effect_progress = supervisor
            .record_effect_result(&execution, effect)
            .expect("effect progress");
        assert_eq!(effect_progress.state(), AgentProgressState::Failed);
        assert_eq!(
            effect_progress.blocker(),
            Some(AgentProgressBlocker::Effect(
                SemanticActionFailure::BackendRefused
            ))
        );
        let guard_debug = format!("{:?}", manifest.guard());
        let value_debug =
            format!("{active_model:?} {model:?} {permit:?} {active_effect:?} {effect:?}");
        assert!(value_debug.contains("[redacted]"));
        assert!(!value_debug.contains(&guard_debug));

        let changed_revision = make_manifest(vec![node(
            1,
            &["a"],
            &[SemanticEffectClass::Read],
            SemanticSensitivity::Public,
            1,
            2_000,
        )]);
        assert_eq!(changed_revision.id(), manifest.id());
        assert!(!changed_revision.matches_revision(&manifest));

        let foreign_active_model = AgentActiveModelCall::for_progress_test(
            &changed_revision,
            AgentModelCallId::new(2).expect("model call"),
            lease,
            root_id,
        );
        assert_eq!(
            supervisor
                .record_active_model_call(&execution, &foreign_active_model)
                .expect_err("same-id foreign manifest revision"),
            AgentSupervisorRuntimeError::ProgressAuthority
        );
        let foreign_model = AgentModelCallReceipt::for_progress_test(
            &changed_revision,
            AgentModelCallId::new(2).expect("model call"),
            lease,
            root_id,
            AgentModelCallSettlement::Completed,
        );
        assert_eq!(
            supervisor
                .record_model_call_result(&execution, foreign_model)
                .expect_err("same-id foreign manifest revision"),
            AgentSupervisorRuntimeError::ProgressAuthority
        );
        let foreign_permit = AgentEffectPermit::for_progress_test(
            &changed_revision,
            AgentEffectId::new(2).expect("effect"),
            lease,
            root_id,
            SemanticEffectClass::Read,
        );
        assert_eq!(
            supervisor
                .record_effect_permit(&execution, &foreign_permit)
                .expect_err("same-id foreign manifest revision"),
            AgentSupervisorRuntimeError::ProgressAuthority
        );
        let foreign_active_effect = AgentActiveEffect::for_progress_test(
            &changed_revision,
            AgentEffectId::new(2).expect("effect"),
            lease,
            root_id,
            SemanticEffectClass::Read,
            SemanticActionAttemptId::new(2).expect("action attempt"),
        );
        assert_eq!(
            supervisor
                .record_active_effect(&execution, &foreign_active_effect)
                .expect_err("same-id foreign manifest revision"),
            AgentSupervisorRuntimeError::ProgressAuthority
        );
        let foreign_effect = AgentEffectReceipt::for_progress_test(
            &changed_revision,
            AgentEffectId::new(2).expect("effect"),
            lease,
            root_id,
            SemanticEffectClass::Read,
            SemanticActionAttemptId::new(2).expect("action attempt"),
            AgentEffectSettlement::Failed(SemanticActionFailure::BackendRefused),
        );
        assert_eq!(
            supervisor
                .record_effect_result(&execution, foreign_effect)
                .expect_err("same-id foreign manifest revision"),
            AgentSupervisorRuntimeError::ProgressAuthority
        );
        assert_eq!(supervisor.semantic_progress(root_id), Some(effect_progress));
        assert!(!supervisor.status().is_sealed());
        supervisor
            .complete(execution, AgentSupervisorCompletion::Succeeded)
            .expect("complete root");
    }

    #[test]
    fn context_assignment_and_cancellation_progress_keep_exact_supervisor_identity() {
        let manifest = standard_manifest();
        let supervisor_id = AgentSupervisorId::new(42).expect("supervisor");
        let root_id = AgentPlanNodeId::from_raw(1);
        let mut supervisor = AgentRunSupervisor::new(supervisor_id, standard_topology(&manifest));
        let execution = supervisor.start(root_id, attempt(1)).expect("start root");
        let mut registry = ContextRegistry::new();
        let identity = context_identity(901, 2, 1);
        let assignment = supervisor
            .reserve_context(
                &execution,
                &manifest,
                &mut registry,
                identity,
                context_capabilities(),
            )
            .expect("reserve context");
        assert_eq!(assignment.supervisor(), supervisor_id);
        assert_eq!(
            supervisor
                .semantic_progress(root_id)
                .expect("context progress")
                .activity()
                .resource(),
            Some(AgentProgressResource::Context(identity.id()))
        );

        let batch = supervisor
            .cancel_subtree(
                root_id,
                cancellation(1),
                AgentSupervisorCancellationReason::UserRequested,
            )
            .expect("cancel root");
        assert_eq!(
            batch
                .contexts()
                .next()
                .expect("context target")
                .assignment(),
            assignment
        );
        let cancelling = supervisor
            .semantic_progress(root_id)
            .expect("cancelling progress");
        assert_eq!(cancelling.state(), AgentProgressState::Waiting);
        assert_eq!(
            cancelling.blocker(),
            Some(AgentProgressBlocker::Cancellation(
                AgentSupervisorCancellationReason::UserRequested
            ))
        );
        supervisor
            .drain_cancelled(execution, cancellation(1))
            .expect("execution drained");
        supervisor
            .cancel_queued_context(&mut registry, identity.id())
            .expect("context cancelled");
        let cancelled = supervisor
            .semantic_progress(root_id)
            .expect("cancelled progress");
        assert_eq!(cancelled.state(), AgentProgressState::Cancelled);
        assert_eq!(supervisor.status().live(), 0);
    }

    #[test]
    fn policy_derived_human_wait_is_exact_content_free_and_releases_execution() {
        let manifest = standard_manifest();
        let root_id = AgentPlanNodeId::from_raw(1);
        let mut supervisor = AgentRunSupervisor::new(
            AgentSupervisorId::new(43).expect("supervisor"),
            standard_topology(&manifest),
        );
        let execution = supervisor.start(root_id, attempt(1)).expect("start root");
        let mut registry = ContextRegistry::new();
        let identity = context_identity(902, 2, 1);
        supervisor
            .reserve_context(
                &execution,
                &manifest,
                &mut registry,
                identity,
                context_capabilities(),
            )
            .expect("reserve context");
        let construction = registry
            .begin_context(identity.id(), context_operation(1))
            .expect("begin context");
        registry
            .settle_construction(identity.id(), construction, ContextSettlement::Applied)
            .expect("settle context");
        let join = registry.join(identity.id()).expect("context join");
        let changed_revision = make_manifest(vec![node(
            1,
            &["a"],
            &[SemanticEffectClass::Read],
            SemanticSensitivity::Public,
            1,
            2_000,
        )]);
        assert_eq!(changed_revision.id(), manifest.id());
        let foreign_transition = AgentNeedsHumanTransition::for_progress_test(
            &changed_revision,
            root_id,
            join,
            SemanticEffectClass::LocalWrite,
            AgentNeedsHumanReason::HumanControl,
        );
        assert_eq!(
            supervisor
                .wait_for_human(&execution, foreign_transition)
                .expect_err("same-id foreign manifest revision"),
            AgentSupervisorRuntimeError::ProgressAuthority
        );
        assert_eq!(supervisor.status().executing(), 1);
        let transition = AgentNeedsHumanTransition::for_progress_test(
            &manifest,
            root_id,
            join,
            SemanticEffectClass::LocalWrite,
            AgentNeedsHumanReason::HumanControl,
        );
        let transition_debug = format!("{transition:?}");
        assert!(transition_debug.contains("[redacted]"));
        assert!(!transition_debug.contains(&format!("{:?}", manifest.guard())));

        let receipt = supervisor
            .wait_for_human(&execution, transition)
            .expect("wait for human");
        assert_eq!(
            receipt.outcome(),
            AgentSupervisorExecutionOutcome::Waiting(AgentSupervisorWait::Yielded)
        );
        assert_eq!(supervisor.status().executing(), 0);
        assert_eq!(supervisor.status().waiting(), 1);
        let progress = supervisor
            .semantic_progress(root_id)
            .expect("human wait progress");
        assert_eq!(
            progress.activity().operation(),
            AgentProgressOperation::Approval(SemanticEffectClass::LocalWrite)
        );
        assert_eq!(progress.state(), AgentProgressState::Waiting);
        assert_eq!(
            progress.blocker(),
            Some(AgentProgressBlocker::NeedsHuman(
                AgentNeedsHumanReason::HumanControl
            ))
        );
        let debug = format!("{progress:?}");
        assert!(!debug.contains("a.example.test"));
    }
}
