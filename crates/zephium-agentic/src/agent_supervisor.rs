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
    AgentNodeExecution, AgentRunSupervisor, AgentSupervisorAttemptId, AgentSupervisorCancellation,
    AgentSupervisorCancellationBatch, AgentSupervisorCancellationId,
    AgentSupervisorCancellationReason, AgentSupervisorCancellationTarget,
    AgentSupervisorCompletion, AgentSupervisorExecutionOutcome, AgentSupervisorExecutionReceipt,
    AgentSupervisorFailure, AgentSupervisorId, AgentSupervisorNodeCancellation,
    AgentSupervisorNodeSnapshot, AgentSupervisorNodeStatus, AgentSupervisorRuntimeError,
    AgentSupervisorRuntimeStatus, AgentSupervisorWait,
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
        self.manifest == manifest.id() && self.manifest_guard == manifest.guard()
    }

    /// Whether two values represent the exact same canonical topology revision.
    pub fn matches_revision(&self, other: &Self) -> bool {
        self.manifest == other.manifest && self.guard == other.guard
    }

    pub(super) const fn guard(&self) -> [u8; 32] {
        self.guard
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
        AgentAccountScope, AgentEffectScope, AgentPlanNodeAuthority, AgentPolicyInstant,
        AgentRunBudget, AgentRunScope, SemanticActionFailure, SemanticEffectClass, SemanticOrigin,
        SemanticSensitivity,
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

    fn attempt(value: u64) -> AgentSupervisorAttemptId {
        AgentSupervisorAttemptId::new(value).expect("attempt")
    }

    fn cancellation(value: u64) -> AgentSupervisorCancellationId {
        AgentSupervisorCancellationId::new(value).expect("cancellation")
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
}
