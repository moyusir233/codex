use std::collections::BTreeMap;
use std::collections::BTreeSet;

use codex_state::WorkflowDependencyRecord;
use codex_state::WorkflowNodeRecord;
use codex_state::WorkflowNodeStatus;

/// Durable dependency outcome for one downstream node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DependencyResolution {
    Blocked,
    Ready,
    Skip,
    FailFast,
}

/// Validated in-memory view of one persisted run graph.
pub struct WorkflowGraph {
    nodes: BTreeMap<String, WorkflowNodeRecord>,
    incoming: BTreeMap<String, Vec<WorkflowDependencyRecord>>,
}

impl WorkflowGraph {
    pub fn new(
        nodes: Vec<WorkflowNodeRecord>,
        dependencies: Vec<WorkflowDependencyRecord>,
    ) -> Result<Self, GraphError> {
        let nodes = nodes
            .into_iter()
            .map(|node| (node.node_id.clone(), node))
            .collect::<BTreeMap<_, _>>();
        let mut incoming = BTreeMap::<String, Vec<WorkflowDependencyRecord>>::new();
        for dependency in dependencies {
            if !nodes.contains_key(&dependency.node_id)
                || !nodes.contains_key(&dependency.depends_on_node_id)
            {
                return Err(GraphError::UnknownNode);
            }
            incoming
                .entry(dependency.node_id.clone())
                .or_default()
                .push(dependency);
        }
        validate_acyclic(&nodes, &incoming)?;
        Ok(Self { nodes, incoming })
    }

    pub fn node(&self, node_id: &str) -> Option<&WorkflowNodeRecord> {
        self.nodes.get(node_id)
    }

    pub fn nodes(&self) -> impl Iterator<Item = &WorkflowNodeRecord> {
        self.nodes.values()
    }

    pub fn resolve(&self, node_id: &str) -> Result<DependencyResolution, GraphError> {
        let node = self.nodes.get(node_id).ok_or(GraphError::UnknownNode)?;
        let Some(dependencies) = self.incoming.get(node_id) else {
            return Ok(DependencyResolution::Ready);
        };
        let policy = dependencies
            .first()
            .map(|dependency| dependency.policy.as_str())
            .ok_or(GraphError::InvalidPolicy)?;
        if dependencies
            .iter()
            .any(|dependency| dependency.policy != policy)
        {
            return Err(GraphError::MixedPolicies);
        }
        let statuses = dependencies
            .iter()
            .map(|dependency| {
                self.nodes
                    .get(&dependency.depends_on_node_id)
                    .map(|node| node.status)
                    .ok_or(GraphError::UnknownNode)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let succeeded = statuses
            .iter()
            .filter(|status| **status == WorkflowNodeStatus::Succeeded)
            .count();
        let terminal = statuses
            .iter()
            .filter(|status| is_terminal(**status))
            .count();
        let ready = match policy {
            "all_succeeded" => succeeded == statuses.len(),
            "all_terminal" => terminal == statuses.len(),
            "at_least" => {
                let threshold = dependencies
                    .first()
                    .and_then(|dependency| dependency.at_least)
                    .ok_or(GraphError::InvalidPolicy)?;
                succeeded >= usize::try_from(threshold).map_err(|_| GraphError::InvalidPolicy)?
            }
            _ => return Err(GraphError::InvalidPolicy),
        };
        if ready {
            return Ok(DependencyResolution::Ready);
        }
        let impossible = match policy {
            "all_succeeded" => statuses
                .iter()
                .any(|status| is_terminal(*status) && *status != WorkflowNodeStatus::Succeeded),
            "all_terminal" => false,
            "at_least" => {
                let threshold = dependencies
                    .first()
                    .and_then(|dependency| dependency.at_least)
                    .ok_or(GraphError::InvalidPolicy)?;
                terminal == statuses.len()
                    && succeeded
                        < usize::try_from(threshold).map_err(|_| GraphError::InvalidPolicy)?
            }
            _ => false,
        };
        if !impossible {
            return Ok(DependencyResolution::Blocked);
        }
        Ok(match node.failure_policy.as_str() {
            "skip_dependents" => DependencyResolution::Skip,
            "continue_independent" => DependencyResolution::Skip,
            "fail_fast" => DependencyResolution::FailFast,
            _ => return Err(GraphError::InvalidFailurePolicy),
        })
    }
}

fn validate_acyclic(
    nodes: &BTreeMap<String, WorkflowNodeRecord>,
    incoming: &BTreeMap<String, Vec<WorkflowDependencyRecord>>,
) -> Result<(), GraphError> {
    fn visit(
        node_id: &str,
        incoming: &BTreeMap<String, Vec<WorkflowDependencyRecord>>,
        visiting: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
    ) -> Result<(), GraphError> {
        if visited.contains(node_id) {
            return Ok(());
        }
        if !visiting.insert(node_id.to_string()) {
            return Err(GraphError::Cycle);
        }
        if let Some(edges) = incoming.get(node_id) {
            for edge in edges {
                visit(&edge.depends_on_node_id, incoming, visiting, visited)?;
            }
        }
        visiting.remove(node_id);
        visited.insert(node_id.to_string());
        Ok(())
    }

    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for node_id in nodes.keys() {
        visit(node_id, incoming, &mut visiting, &mut visited)?;
    }
    Ok(())
}

pub fn is_terminal(status: WorkflowNodeStatus) -> bool {
    matches!(
        status,
        WorkflowNodeStatus::Succeeded | WorkflowNodeStatus::Failed | WorkflowNodeStatus::Cancelled
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GraphError {
    #[error("workflow graph references an unknown node")]
    UnknownNode,
    #[error("workflow graph contains a dependency cycle")]
    Cycle,
    #[error("workflow dependency policy is invalid")]
    InvalidPolicy,
    #[error("one downstream node mixes dependency policies")]
    MixedPolicies,
    #[error("workflow failure policy is invalid")]
    InvalidFailurePolicy,
}
