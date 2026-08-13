use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use tokio::sync::Notify;

use crate::WorkflowGoalClient;
use crate::api::WorkflowRunId;
use crate::example::PromptReviewCapability;
use crate::integrations::fornax::FornaxWorkflowClient;
use crate::runtime::LarkInteractionService;
use crate::runtime::NodeClient;
use crate::runtime::WorkflowApprovalService;
use crate::runtime::WorkflowArtifactClient;
use crate::runtime::WorkflowAuditClient;
use crate::workflows::lark_feature::LarkFeatureCapability;

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

    /// Returns the shared flag accepted by bounded synchronous capability
    /// adapters.
    pub fn flag(&self) -> &AtomicBool {
        &self.state.requested
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

    /// Returns the run-scoped persistent node facade configured by the host.
    pub fn nodes(&self) -> Option<&NodeClient> {
        self.parts.nodes.as_ref()
    }

    /// Returns the run-scoped immutable artifact facade configured by the host.
    pub fn artifacts(&self) -> Option<&WorkflowArtifactClient> {
        self.parts.artifacts.as_ref()
    }

    /// Returns revision-bound approvals configured by the host.
    pub fn approvals(&self) -> Option<&WorkflowApprovalService> {
        self.parts.approvals.as_deref()
    }

    /// Returns the run-scoped goal reconciliation facade configured by the host.
    pub fn goals(&self) -> Option<&WorkflowGoalClient> {
        self.parts.goals.as_ref()
    }

    /// Returns preflight/group and `/grilling` transport for the Lark SDK workflow.
    pub fn lark_feature(&self) -> Option<&dyn LarkFeatureCapability> {
        self.parts.lark_feature.as_deref()
    }

    /// Returns the run-scoped redacting audit facade.
    pub fn audit(&self) -> &WorkflowAuditClient {
        &self.parts.audit
    }

    /// Returns the narrow capability used by the registered prompt-review
    /// reference workflow.
    pub fn prompt_review(&self) -> Option<&dyn PromptReviewCapability> {
        self.parts.prompt_review.as_deref()
    }
}

pub(crate) struct WorkflowContextParts {
    pub(crate) run_id: WorkflowRunId,
    pub(crate) cancellation: WorkflowCancellation,
    pub(crate) fornax: Option<Arc<FornaxWorkflowClient>>,
    pub(crate) lark: Option<Arc<LarkInteractionService>>,
    pub(crate) prompt_review: Option<Arc<dyn PromptReviewCapability>>,
    pub(crate) nodes: Option<NodeClient>,
    pub(crate) artifacts: Option<WorkflowArtifactClient>,
    pub(crate) approvals: Option<Arc<WorkflowApprovalService>>,
    pub(crate) goals: Option<WorkflowGoalClient>,
    pub(crate) lark_feature: Option<Arc<dyn LarkFeatureCapability>>,
    pub(crate) audit: WorkflowAuditClient,
}
