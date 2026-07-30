use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use tokio::sync::Notify;

use crate::api::WorkflowRunId;
use crate::integrations::fornax::FornaxWorkflowClient;
use crate::runtime::LarkInteractionService;

/// Cooperative cancellation view exposed to workflow reducer code.
#[derive(Clone)]
pub struct WorkflowCancellation {
    state: Arc<WorkflowCancellationState>,
}

struct WorkflowCancellationState {
    requested: AtomicBool,
    notify: Notify,
}

impl WorkflowCancellation {
    pub(crate) fn new(requested: bool) -> Self {
        Self {
            state: Arc::new(WorkflowCancellationState {
                requested: AtomicBool::new(requested),
                notify: Notify::new(),
            }),
        }
    }

    /// Returns whether cancellation has been requested for the run.
    pub fn is_requested(&self) -> bool {
        self.state.requested.load(Ordering::Acquire)
    }

    /// Waits until cancellation is requested.
    ///
    /// Process-backed effects should race this future with child completion and
    /// terminate their child when cancellation wins.
    pub async fn cancelled(&self) {
        loop {
            let notified = self.state.notify.notified();
            if self.is_requested() {
                return;
            }
            notified.await;
        }
    }

    pub(crate) fn request(&self) {
        if !self.state.requested.swap(true, Ordering::AcqRel) {
            self.state.notify.notify_waiters();
        }
    }
}

/// Private-capability context supplied to one workflow reducer step.
///
/// The context deliberately exposes no raw thread manager, state database,
/// process launcher, credentials, or unrestricted network client. Additional
/// typed effect clients are added here only alongside their durable journals.
pub struct WorkflowContext<'a> {
    parts: &'a WorkflowContextParts,
}

impl<'a> WorkflowContext<'a> {
    pub(crate) fn new(parts: &'a WorkflowContextParts) -> Self {
        Self { parts }
    }

    /// Returns the durable run identity executing this step.
    pub fn run_id(&self) -> WorkflowRunId {
        self.parts.run_id
    }

    /// Returns the cooperative cancellation view for this run.
    pub fn cancellation(&self) -> &WorkflowCancellation {
        &self.parts.cancellation
    }

    /// Returns the typed Fornax facade configured by the workflow host.
    pub fn fornax(&self) -> Option<&FornaxWorkflowClient> {
        self.parts.fornax.as_deref()
    }

    /// Returns the durable Lark interaction service configured by the host.
    pub fn lark(&self) -> Option<&LarkInteractionService> {
        self.parts.lark.as_deref()
    }
}

pub(crate) struct WorkflowContextParts {
    run_id: WorkflowRunId,
    cancellation: WorkflowCancellation,
    fornax: Option<Arc<FornaxWorkflowClient>>,
    lark: Option<Arc<LarkInteractionService>>,
}

impl WorkflowContextParts {
    pub(crate) fn with_cancellation(
        run_id: WorkflowRunId,
        cancellation: WorkflowCancellation,
        fornax: Option<Arc<FornaxWorkflowClient>>,
        lark: Option<Arc<LarkInteractionService>>,
    ) -> Self {
        Self {
            run_id,
            cancellation,
            fornax,
            lark,
        }
    }
}
