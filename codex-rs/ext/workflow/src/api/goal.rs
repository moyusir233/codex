use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use codex_protocol::ThreadId;
use serde::Deserialize;
use serde::Serialize;

use super::ArtifactId;
use super::EffectKey;
use super::WorkflowRunId;

pub type WorkflowGoalFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, WorkflowGoalError>> + Send + 'a>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowGoalStatus {
    Active,
    Paused,
    Blocked,
    UsageLimited,
    BudgetLimited,
    Complete,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowGoalEnsureRequest {
    pub effect_key: EffectKey,
    pub thread_id: ThreadId,
    pub objective: String,
    pub token_budget: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowGoalSnapshot {
    pub goal_id: String,
    pub thread_id: ThreadId,
    pub objective_sha256: String,
    pub status: WorkflowGoalStatus,
    pub token_budget: Option<i64>,
    pub tokens_used: i64,
    pub time_used_seconds: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowGoalDeliveryEvidence {
    pub goal_id: String,
    pub objective_sha256: String,
    pub validation_artifact_id: ArtifactId,
    pub validation_sha256: String,
    pub acceptance_artifact_id: ArtifactId,
    pub acceptance_sha256: String,
}

impl WorkflowGoalSnapshot {
    /// Requires goal completion plus two distinct immutable evidence artifacts.
    pub fn permits_delivery(&self, evidence: &WorkflowGoalDeliveryEvidence) -> bool {
        self.status == WorkflowGoalStatus::Complete
            && evidence.goal_id == self.goal_id
            && evidence.objective_sha256 == self.objective_sha256
            && evidence.validation_artifact_id != evidence.acceptance_artifact_id
            && valid_sha256(&evidence.validation_sha256)
            && valid_sha256(&evidence.acceptance_sha256)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum WorkflowGoalError {
    #[error("workflow goal capability is unavailable")]
    Unavailable,
    #[error("thread has an unrelated unfinished goal")]
    UnrelatedUnfinished,
    #[error("thread goal does not match the workflow objective")]
    ObjectiveMismatch,
    #[error("workflow goal effect was reused with different input")]
    EffectConflict,
    #[error("invalid workflow goal request: {0}")]
    InvalidRequest(String),
    #[error("workflow goal host failed: {0}")]
    Internal(String),
}

pub trait WorkflowGoalCapability: Send + Sync {
    fn ensure<'a>(
        &'a self,
        run_id: WorkflowRunId,
        request: &'a WorkflowGoalEnsureRequest,
    ) -> WorkflowGoalFuture<'a, WorkflowGoalSnapshot>;

    fn get<'a>(
        &'a self,
        run_id: WorkflowRunId,
        thread_id: ThreadId,
    ) -> WorkflowGoalFuture<'a, Option<WorkflowGoalSnapshot>>;
}

#[derive(Clone)]
pub struct WorkflowGoalClient {
    run_id: WorkflowRunId,
    capability: Arc<dyn WorkflowGoalCapability>,
}

impl WorkflowGoalClient {
    pub(crate) fn new(run_id: WorkflowRunId, capability: Arc<dyn WorkflowGoalCapability>) -> Self {
        Self { run_id, capability }
    }

    pub async fn ensure(
        &self,
        request: &WorkflowGoalEnsureRequest,
    ) -> Result<WorkflowGoalSnapshot, WorkflowGoalError> {
        self.capability.ensure(self.run_id, request).await
    }

    pub async fn get(
        &self,
        thread_id: ThreadId,
    ) -> Result<Option<WorkflowGoalSnapshot>, WorkflowGoalError> {
        self.capability.get(self.run_id, thread_id).await
    }
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
