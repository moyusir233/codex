//! Registered reference workflows and their narrow host capability contracts.

mod args;
mod prompt_review;
mod state;

use std::future::Future;
use std::pin::Pin;

pub use args::PromptReviewArguments;
pub use prompt_review::PromptReviewWorkflow;
pub use state::PromptReviewOutput;
pub use state::PromptReviewPrepared;
pub use state::PromptReviewReview;
pub use state::PromptReviewState;

use crate::HumanInteractionOutcome;
use crate::WorkflowError;
use crate::WorkflowRegistry;
use crate::WorkflowRegistryBuilder;
use crate::WorkflowRunId;

/// Boxed future used by the object-safe reference-workflow host boundary.
pub type PromptReviewCapabilityFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, WorkflowError>> + Send + 'a>>;

/// Narrow, replaceable boundary that performs the reference workflow's
/// journal-backed Codex, Fornax, Lark, and artifact operations.
///
/// Implementations must preflight every declared capability before `prepare`
/// performs its first external mutation. Production hosts leave this
/// capability absent until all live approvals and exact versions are present.
pub trait PromptReviewCapability: Send + Sync {
    fn prepare<'a>(
        &'a self,
        run_id: WorkflowRunId,
        args: &'a PromptReviewArguments,
        cancellation: &'a crate::WorkflowCancellation,
    ) -> PromptReviewCapabilityFuture<'a, PromptReviewPrepared>;

    fn review<'a>(
        &'a self,
        run_id: WorkflowRunId,
        args: &'a PromptReviewArguments,
        prepared: &'a PromptReviewPrepared,
        cancellation: &'a crate::WorkflowCancellation,
    ) -> PromptReviewCapabilityFuture<'a, PromptReviewReview>;

    fn request_human<'a>(
        &'a self,
        run_id: WorkflowRunId,
        args: &'a PromptReviewArguments,
        review: &'a PromptReviewReview,
        cancellation: &'a crate::WorkflowCancellation,
    ) -> PromptReviewCapabilityFuture<'a, HumanInteractionOutcome>;

    fn follow_up_and_save<'a>(
        &'a self,
        run_id: WorkflowRunId,
        args: &'a PromptReviewArguments,
        prepared: &'a PromptReviewPrepared,
        review: &'a PromptReviewReview,
        reply_artifact_id: crate::ArtifactId,
        cancellation: &'a crate::WorkflowCancellation,
    ) -> PromptReviewCapabilityFuture<'a, PromptReviewOutput>;
}

/// Builds the reviewed definitions installed by the app-server composition
/// root when experimental workflows are enabled.
pub fn default_registry() -> Result<WorkflowRegistry, crate::RegistryError> {
    let mut builder = WorkflowRegistryBuilder::new();
    builder.register(PromptReviewWorkflow)?;
    builder.build()
}
