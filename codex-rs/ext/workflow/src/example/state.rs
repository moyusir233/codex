use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

use crate::ArtifactId;
use crate::InteractionId;
use crate::WorkflowOutput;
use crate::WorkflowState;

/// Correlations produced after prompt retrieval/rendering and trace start.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptReviewPrepared {
    pub prompt_id: String,
    pub prompt_version: Option<String>,
    pub rendered_prompt_artifact_id: ArtifactId,
    pub trace_id: String,
    pub root_span_id: String,
}

/// Durable node/session correlations produced by chain and fan-out/fan-in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptReviewReview {
    pub planner_thread_id: String,
    pub reviewer_thread_ids: Vec<String>,
    pub synthesizer_thread_id: String,
    pub synthesis_artifact_id: ArtifactId,
    pub lark_document_id: Option<String>,
}

/// Reducer checkpoint for the registered example.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "phase")]
pub enum PromptReviewState {
    Preparing {
        args: super::PromptReviewArguments,
    },
    Reviewing {
        args: super::PromptReviewArguments,
        prepared: PromptReviewPrepared,
    },
    AwaitingHuman {
        args: super::PromptReviewArguments,
        prepared: PromptReviewPrepared,
        review: PromptReviewReview,
        interaction_id: Option<InteractionId>,
    },
    FollowingUp {
        args: super::PromptReviewArguments,
        prepared: PromptReviewPrepared,
        review: PromptReviewReview,
        reply_artifact_id: ArtifactId,
    },
}

impl WorkflowState for PromptReviewState {
    const SCHEMA_VERSION: u32 = 1;
}

/// Successful prompt-review result with resumable and external correlations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PromptReviewOutput {
    pub prompt_id: String,
    pub prompt_version: Option<String>,
    pub planner_thread_id: String,
    pub reviewer_thread_ids: Vec<String>,
    pub synthesizer_thread_id: String,
    pub trace_id: String,
    pub root_span_id: String,
    pub lark_document_id: Option<String>,
    pub human_reply_artifact_id: String,
    pub draft_saved: bool,
    pub final_artifact_ids: Vec<String>,
    pub resume_commands: Vec<String>,
}

impl WorkflowOutput for PromptReviewOutput {}
