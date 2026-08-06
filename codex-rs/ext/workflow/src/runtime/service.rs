use std::sync::Arc;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use codex_protocol::ThreadId;
use codex_state::WorkflowStore;
use serde_json::json;

use crate::WorkflowNodeBinding;
use crate::WorkflowRunId;

use super::CancellationSignals;
use super::FailureInjector;
use super::NodeClient;
use super::NodeHostError;
use super::WorkflowNodeHostSlot;

/// Process-scoped workflow runtime services shared by extension callbacks and request handlers.
#[derive(Clone)]
pub struct WorkflowService {
    pub(super) inner: Arc<WorkflowServiceInner>,
}

pub(super) struct WorkflowServiceInner {
    pub(super) store: WorkflowStore,
    pub(super) node_host: WorkflowNodeHostSlot,
    pub(super) registry: Arc<crate::WorkflowRegistry>,
    pub(super) failure_injector: FailureInjector,
    pub(super) cancellation_signals: CancellationSignals,
    pub(super) prompt_review: Option<Arc<dyn crate::PromptReviewCapability>>,
}

impl WorkflowService {
    pub fn new(store: WorkflowStore, node_host: WorkflowNodeHostSlot) -> Self {
        Self::new_with_registry(
            store,
            node_host,
            Arc::new(crate::WorkflowRegistry::default()),
        )
    }

    pub fn new_with_registry(
        store: WorkflowStore,
        node_host: WorkflowNodeHostSlot,
        registry: Arc<crate::WorkflowRegistry>,
    ) -> Self {
        Self::new_with_injector(store, node_host, registry, FailureInjector::disabled())
    }

    pub fn new_with_injector(
        store: WorkflowStore,
        node_host: WorkflowNodeHostSlot,
        registry: Arc<crate::WorkflowRegistry>,
        failure_injector: FailureInjector,
    ) -> Self {
        Self {
            inner: Arc::new(WorkflowServiceInner {
                store,
                node_host,
                registry,
                failure_injector,
                cancellation_signals: CancellationSignals::default(),
                prompt_review: None,
            }),
        }
    }

    /// Installs the preflighted capability used by the registered reference
    /// workflow. This must be called before the service is shared.
    pub fn with_prompt_review_capability(
        mut self,
        capability: Arc<dyn crate::PromptReviewCapability>,
    ) -> Self {
        Arc::get_mut(&mut self.inner)
            .unwrap_or_else(|| panic!("workflow service was shared before configuration"))
            .prompt_review = Some(capability);
        self
    }

    pub fn node_host_slot(&self) -> &WorkflowNodeHostSlot {
        &self.inner.node_host
    }

    pub fn nodes(&self, run_id: WorkflowRunId) -> NodeClient<'_> {
        NodeClient {
            service: self,
            run_id,
        }
    }

    pub fn store(&self) -> &WorkflowStore {
        &self.inner.store
    }

    pub fn registry(&self) -> Arc<crate::WorkflowRegistry> {
        Arc::clone(&self.inner.registry)
    }

    pub fn failure_injector(&self) -> &FailureInjector {
        &self.inner.failure_injector
    }

    pub fn prompt_review_capability(&self) -> Option<Arc<dyn crate::PromptReviewCapability>> {
        self.inner.prompt_review.as_ref().map(Arc::clone)
    }

    /// Releases process-owned listeners for every thread retained by a terminal run.
    pub async fn detach_run_observers(&self, run_id: WorkflowRunId) -> Result<(), NodeError> {
        let host = self.inner.node_host.get()?;
        for node in self.store().list_nodes(&run_id.to_string()).await? {
            for thread in self.store().list_node_threads(&node.node_id).await? {
                let thread_id = ThreadId::from_string(&thread.thread_id)
                    .map_err(|error| NodeError::InvalidThreadId(error.to_string()))?;
                host.detach_observer(thread_id).await?;
            }
        }
        Ok(())
    }

    pub(crate) fn cancellation_signals(&self) -> CancellationSignals {
        self.inner.cancellation_signals.clone()
    }

    /// Records an out-of-band turn as visible operator ownership.
    pub async fn observe_turn_start(
        &self,
        binding: &WorkflowNodeBinding,
        turn_id: &str,
    ) -> Result<bool, NodeError> {
        let known = self
            .store()
            .read_node_attempt_by_turn_id(&binding.node_id.to_string(), turn_id)
            .await?
            .is_some()
            || self
                .store()
                .read_node_attempt_by_submission_id(turn_id)
                .await?
                .is_some_and(|attempt| {
                    attempt.run_id == binding.run_id.to_string()
                        && attempt.node_id == binding.node_id.to_string()
                });
        if known {
            return Ok(false);
        }
        let node_id = binding.node_id.to_string();
        self.store()
            .mark_run_needs_operator(
                &binding.run_id.to_string(),
                Some(&node_id),
                "external_turn_observed",
                json!({ "turn_id": turn_id }),
                now_ms()?,
            )
            .await?;
        Ok(true)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum NodeError {
    #[error("workflow node was not found")]
    NotFound,
    #[error("node key `{node_key}` already exists with a different resolved specification")]
    SpecificationConflict { node_key: String },
    #[error("workflow node state is invalid: {0}")]
    InvalidState(String),
    #[error("workflow node thread id is invalid: {0}")]
    InvalidThreadId(String),
    #[error(transparent)]
    Identifier(#[from] crate::IdentifierError),
    #[error(transparent)]
    Store(#[from] codex_state::WorkflowStoreError),
    #[error(transparent)]
    Serialization(#[from] serde_json::Error),
    #[error(transparent)]
    Host(#[from] NodeHostError),
    #[error(transparent)]
    Injected(#[from] super::InjectedFailure),
    #[error("system clock is before the Unix epoch")]
    InvalidClock,
    #[error("workflow node retry policy is exhausted or does not classify this failure")]
    RetryExhausted,
    #[error("workflow node retry is not ready until {retry_at_ms}")]
    RetryNotReady { retry_at_ms: i64 },
}

pub(super) fn now_ms() -> Result<i64, NodeError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| NodeError::InvalidClock)?;
    i64::try_from(elapsed.as_millis()).map_err(|_| NodeError::InvalidClock)
}
