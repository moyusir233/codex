use std::num::NonZeroUsize;

use codex_state::WorkflowNodeRecord;
use codex_state::WorkflowNodeStatus;
use codex_state::WorkflowNodeTransition;
use codex_state::WorkflowStore;
use serde_json::json;

use super::DependencyResolution;
use super::WorkflowGraph;
use super::graph::GraphError;

/// Durable fair scheduler over one persisted run graph.
#[derive(Clone)]
pub struct DurableScheduler {
    store: WorkflowStore,
    maximum_active_nodes: NonZeroUsize,
}

impl DurableScheduler {
    pub fn new(store: WorkflowStore, maximum_active_nodes: NonZeroUsize) -> Self {
        Self {
            store,
            maximum_active_nodes,
        }
    }

    /// Resolves dependency state and returns the fair launch batch.
    pub async fn schedule_ready(
        &self,
        run_id: &str,
        now_ms: i64,
    ) -> Result<Vec<WorkflowNodeRecord>, SchedulerError> {
        if self.store.run_cancellation_requested(run_id).await? {
            return Ok(Vec::new());
        }
        let nodes = self.store.list_nodes_for_scheduler(run_id).await?;
        let dependencies = self.store.list_node_dependencies(run_id).await?;
        let graph = WorkflowGraph::new(nodes, dependencies)?;
        let mut runnable = Vec::new();
        let mut active = 0usize;
        for node in graph.nodes() {
            if node.status == WorkflowNodeStatus::Running {
                active += 1;
                continue;
            }
            if node.status == WorkflowNodeStatus::Waiting
                && node.retry_at_ms.is_some_and(|retry_at| retry_at > now_ms)
            {
                continue;
            }
            if !matches!(
                node.status,
                WorkflowNodeStatus::Pending
                    | WorkflowNodeStatus::Blocked
                    | WorkflowNodeStatus::Ready
                    | WorkflowNodeStatus::Waiting
            ) {
                continue;
            }
            match graph.resolve(&node.node_id)? {
                DependencyResolution::Blocked => {
                    self.transition_if_needed(
                        node,
                        WorkflowNodeStatus::Blocked,
                        None,
                        "node.blocked",
                        now_ms,
                    )
                    .await?;
                }
                DependencyResolution::Ready => {
                    let ready = self
                        .transition_if_needed(
                            node,
                            WorkflowNodeStatus::Ready,
                            None,
                            "node.ready",
                            now_ms,
                        )
                        .await?;
                    runnable.push(ready);
                }
                DependencyResolution::Skip => {
                    self.transition_if_needed(
                        node,
                        WorkflowNodeStatus::Cancelled,
                        None,
                        "node.skipped",
                        now_ms,
                    )
                    .await?;
                }
                DependencyResolution::FailFast => {
                    return Err(SchedulerError::FailFast(node.node_id.clone()));
                }
            }
        }
        let capacity = self.maximum_active_nodes.get().saturating_sub(active);
        runnable.truncate(capacity);
        Ok(runnable)
    }

    async fn transition_if_needed(
        &self,
        node: &WorkflowNodeRecord,
        status: WorkflowNodeStatus,
        retry_at_ms: Option<i64>,
        event_kind: &str,
        now_ms: i64,
    ) -> Result<WorkflowNodeRecord, SchedulerError> {
        if node.status == status && node.retry_at_ms == retry_at_ms {
            return Ok(node.clone());
        }
        self.store
            .transition_node(
                &node.node_id,
                node.row_version,
                WorkflowNodeTransition {
                    status,
                    retry_at_ms,
                    event_kind: event_kind.to_string(),
                    event_metadata: json!({}),
                    updated_at_ms: now_ms,
                },
            )
            .await
            .map_err(Into::into)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SchedulerError {
    #[error(transparent)]
    Store(#[from] codex_state::WorkflowStoreError),
    #[error(transparent)]
    Graph(#[from] GraphError),
    #[error("node {0} triggered fail-fast dependency handling")]
    FailFast(String),
}
