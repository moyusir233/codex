use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::OnceLock;

use codex_protocol::ThreadId;

use crate::CancellationReason;
use crate::NodeInput;
use crate::NodeSpec;
use crate::NodeTurnResult;
use crate::WorkflowNodeBinding;

pub type NodeHostFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, NodeHostError>> + Send + 'a>>;

/// Narrow host capability used by workflow nodes without exposing app-server internals.
pub trait WorkflowNodeHost: Send + Sync {
    /// Resolves and validates persisted node capabilities before materialization.
    fn resolve_node_spec(&self, spec: NodeSpec) -> NodeHostFuture<'_, NodeSpec> {
        Box::pin(async move { Ok(spec) })
    }

    fn materialize_node(
        &self,
        request: MaterializeNodeRequest,
    ) -> NodeHostFuture<'_, MaterializedNode>;

    fn find_materialized_nodes(
        &self,
        binding: WorkflowNodeBinding,
    ) -> NodeHostFuture<'_, Vec<ThreadId>>;

    fn submit_prepared_turn(
        &self,
        request: PreparedTurnRequest,
    ) -> NodeHostFuture<'_, SubmittedTurn>;

    fn await_terminal_turn(&self, request: AwaitTurnRequest) -> NodeHostFuture<'_, NodeTurnResult>;

    fn recover_prepared_turn(
        &self,
        request: RecoverTurnRequest,
    ) -> NodeHostFuture<'_, RecoveredTurnState>;

    fn steer(&self, request: SteerTurnRequest) -> NodeHostFuture<'_, ()>;

    fn status(&self, thread_id: ThreadId) -> NodeHostFuture<'_, NodeRuntimeStatus>;

    fn interrupt(&self, thread_id: ThreadId, turn_id: String) -> NodeHostFuture<'_, ()>;

    fn cancel(&self, thread_id: ThreadId, reason: CancellationReason) -> NodeHostFuture<'_, ()>;

    fn shutdown_runtime(
        &self,
        thread_id: ThreadId,
        mode: RuntimeShutdown,
    ) -> NodeHostFuture<'_, ()>;

    fn detach_observer(&self, thread_id: ThreadId) -> NodeHostFuture<'_, ()>;

    fn archive(&self, thread_id: ThreadId, archived: bool) -> NodeHostFuture<'_, ()>;

    fn delete(&self, confirmation: ConfirmedHistoryDeletion) -> NodeHostFuture<'_, ()>;
}

/// One-time late binding used to break the app-server/ThreadManager construction cycle.
#[derive(Clone, Default)]
pub struct WorkflowNodeHostSlot {
    host: Arc<OnceLock<Arc<dyn WorkflowNodeHost>>>,
}

impl WorkflowNodeHostSlot {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bind(&self, host: Arc<dyn WorkflowNodeHost>) -> Result<(), NodeHostBindError> {
        self.host
            .set(host)
            .map_err(|_| NodeHostBindError::AlreadyBound)
    }

    pub(crate) fn get(&self) -> Result<Arc<dyn WorkflowNodeHost>, NodeHostError> {
        self.host
            .get()
            .cloned()
            .ok_or(NodeHostError::RuntimeUnavailable)
    }
}

#[derive(Clone, Debug)]
pub struct MaterializeNodeRequest {
    pub binding: WorkflowNodeBinding,
    pub spec: NodeSpec,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaterializedNode {
    pub thread_id: ThreadId,
}

#[derive(Clone, Debug)]
pub struct PreparedTurnRequest {
    pub thread_id: ThreadId,
    pub submission_id: String,
    pub input_hash: [u8; 32],
    pub input: NodeInput,
    pub spec: NodeSpec,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreparedTurnDisposition {
    Queued,
    AlreadyQueued,
    BoundaryAlreadyPersisted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubmittedTurn {
    pub turn_id: String,
    pub disposition: PreparedTurnDisposition,
}

#[derive(Clone, Debug)]
pub struct AwaitTurnRequest {
    pub thread_id: ThreadId,
    pub turn_id: String,
}

#[derive(Clone, Debug)]
pub struct RecoverTurnRequest {
    pub thread_id: ThreadId,
    pub submission_id: String,
    pub input_hash: [u8; 32],
}

#[derive(Clone, Debug, PartialEq)]
pub enum RecoveredTurnState {
    NoBoundary,
    Unterminated,
    Terminal(NodeTurnResult),
    Conflict,
}

#[derive(Clone, Debug)]
pub struct SteerTurnRequest {
    pub thread_id: ThreadId,
    pub turn_id: String,
    pub input: NodeInput,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeRuntimeStatus {
    Running,
    Idle,
    Shutdown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeShutdown {
    Graceful,
    InterruptThenShutdown,
}

/// Explicit confirmation bound to the exact workflow node and current thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfirmedHistoryDeletion {
    pub binding: WorkflowNodeBinding,
    pub thread_id: ThreadId,
}

impl ConfirmedHistoryDeletion {
    pub fn new(binding: WorkflowNodeBinding, thread_id: ThreadId) -> Self {
        Self { binding, thread_id }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NodeHostBindError {
    #[error("workflow node host was already bound")]
    AlreadyBound,
}

#[derive(Debug, thiserror::Error)]
pub enum NodeHostError {
    #[error("workflow node runtime is unavailable")]
    RuntimeUnavailable,
    #[error("workflow node thread was not found")]
    ThreadNotFound,
    #[error("workflow node host rejected the request: {0}")]
    InvalidRequest(String),
    #[error("workflow node host operation failed: {0}")]
    Host(String),
}
