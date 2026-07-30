use crate::HumanInteractionOutcome;
use crate::WakeCondition;
use crate::Workflow;
use crate::WorkflowContext;
use crate::WorkflowError;
use crate::WorkflowMetadata;
use crate::WorkflowName;
use crate::WorkflowTransition;
use crate::WorkflowVersion;

use super::PromptReviewArguments;
use super::PromptReviewOutput;
use super::PromptReviewState;

/// Version 1.0.0 of the complete experimental reference workflow.
pub struct PromptReviewWorkflow;

impl Workflow for PromptReviewWorkflow {
    type Arguments = PromptReviewArguments;
    type State = PromptReviewState;
    type Output = PromptReviewOutput;

    fn metadata(&self) -> WorkflowMetadata {
        WorkflowMetadata::new(
            WorkflowName::new("prompt-review")
                .unwrap_or_else(|error| panic!("static workflow name is invalid: {error}")),
            WorkflowVersion::parse("1.0.0")
                .unwrap_or_else(|error| panic!("static workflow version is invalid: {error}")),
            "Reviews a Fornax prompt with durable Codex fan-out and a correlated Lark approval.",
        )
        .with_default(true)
        .with_required_capabilities([
            "codex.nodes",
            "codex.skills.explicit",
            "fornax.cli.read",
            "fornax.bridge.trace-write-live-approved",
            "lark.cli.authenticated",
            "lark.human-interaction",
            "workflow.artifacts",
        ])
    }

    fn initialize(&self, args: Self::Arguments) -> Result<Self::State, WorkflowError> {
        Ok(PromptReviewState::Preparing { args })
    }

    async fn step(
        &self,
        ctx: WorkflowContext<'_>,
        state: Self::State,
    ) -> Result<WorkflowTransition<Self::State, Self::Output>, WorkflowError> {
        let capability = ctx.prompt_review().ok_or_else(|| {
            WorkflowError::definition(
                "prompt-review capabilities are unavailable; configure exact Fornax/Lark \
                 versions, live trace approval, auth, and the workflow artifact root",
            )
        })?;
        match state {
            PromptReviewState::Preparing { args } => {
                let prepared = capability.prepare(ctx.run_id(), &args).await?;
                Ok(WorkflowTransition::Continue {
                    state: PromptReviewState::Reviewing { args, prepared },
                })
            }
            PromptReviewState::Reviewing { args, prepared } => {
                let review = capability.review(ctx.run_id(), &args, &prepared).await?;
                Ok(WorkflowTransition::Continue {
                    state: PromptReviewState::AwaitingHuman {
                        args,
                        prepared,
                        review,
                        interaction_id: None,
                    },
                })
            }
            PromptReviewState::AwaitingHuman {
                args,
                prepared,
                review,
                interaction_id: _,
            } => {
                match capability
                    .request_human(ctx.run_id(), &args, &review)
                    .await?
                {
                    HumanInteractionOutcome::Waiting { interaction_id } => {
                        Ok(WorkflowTransition::Wait {
                            state: PromptReviewState::AwaitingHuman {
                                args,
                                prepared,
                                review,
                                interaction_id: Some(interaction_id),
                            },
                            wake: WakeCondition::HumanInteraction(interaction_id),
                        })
                    }
                    HumanInteractionOutcome::Resolved {
                        artifact_id,
                        interaction_id: _,
                    } => Ok(WorkflowTransition::Continue {
                        state: PromptReviewState::FollowingUp {
                            args,
                            prepared,
                            review,
                            reply_artifact_id: artifact_id,
                        },
                    }),
                    HumanInteractionOutcome::TimedOut { .. } => {
                        Err(WorkflowError::definition("Lark review request timed out"))
                    }
                    HumanInteractionOutcome::Cancelled { .. } => Err(WorkflowError::definition(
                        "Lark review request was cancelled",
                    )),
                    HumanInteractionOutcome::NeedsOperator { reason, .. } => Err(
                        WorkflowError::definition(format!("Lark review needs operator: {reason}")),
                    ),
                }
            }
            PromptReviewState::FollowingUp {
                args,
                prepared,
                review,
                reply_artifact_id,
            } => {
                let output = capability
                    .follow_up_and_save(ctx.run_id(), &args, &prepared, &review, reply_artifact_id)
                    .await?;
                Ok(WorkflowTransition::Complete { output })
            }
        }
    }
}
