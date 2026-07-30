use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use crate::api::WorkflowRunId;

/// Cooperative cancellation view exposed to workflow reducer code.
pub struct WorkflowCancellation {
    requested: AtomicBool,
}

impl WorkflowCancellation {
    pub(crate) fn new(requested: bool) -> Self {
        Self {
            requested: AtomicBool::new(requested),
        }
    }

    /// Returns whether cancellation has been requested for the run.
    pub fn is_requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
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
    #[expect(
        dead_code,
        reason = "the durable runtime constructs reducer contexts in Milestone 2"
    )]
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
}

pub(crate) struct WorkflowContextParts {
    run_id: WorkflowRunId,
    cancellation: WorkflowCancellation,
}

impl WorkflowContextParts {
    #[expect(
        dead_code,
        reason = "the durable runtime constructs reducer contexts in Milestone 2"
    )]
    pub(crate) fn new(run_id: WorkflowRunId, cancellation_requested: bool) -> Self {
        Self {
            run_id,
            cancellation: WorkflowCancellation::new(cancellation_requested),
        }
    }
}
