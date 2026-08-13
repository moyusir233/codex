use std::future::Future;
use std::pin::Pin;

use crate::HumanInteractionOutcome;
use crate::WorkflowCancellation;
use crate::WorkflowError;
use crate::WorkflowRunId;

use super::LarkFeatureArguments;
use super::LarkFeatureBootstrap;
use super::LarkFeatureStageId;
use super::StageQuestion;

pub type LarkFeatureFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, WorkflowError>> + Send + 'a>>;

/// Narrow host boundary for preflight/group reconciliation and `/grilling`
/// question transport. Stage reasoning remains in normal persistent nodes.
pub trait LarkFeatureCapability: Send + Sync {
    /// Performs all read-only checks before reconciling exactly one owned group.
    fn preflight_and_group<'a>(
        &'a self,
        run_id: WorkflowRunId,
        args: &'a LarkFeatureArguments,
        cancellation: &'a WorkflowCancellation,
    ) -> LarkFeatureFuture<'a, LarkFeatureBootstrap>;

    /// Sends schema-extracted stage questions through the durable Lark
    /// interaction transport. A resolved reply is submitted back to the same
    /// owning node thread by the reducer.
    fn poll_stage_questions<'a>(
        &'a self,
        run_id: WorkflowRunId,
        stage: LarkFeatureStageId,
        owning_thread_id: &'a str,
        questions: &'a [StageQuestion],
        cancellation: &'a WorkflowCancellation,
    ) -> LarkFeatureFuture<'a, HumanInteractionOutcome>;
}
