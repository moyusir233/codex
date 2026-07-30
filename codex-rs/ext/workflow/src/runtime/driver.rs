use std::num::NonZeroUsize;
use std::sync::Arc;

use codex_state::WorkflowRunStatus;
use codex_state::WorkflowRunTransition;
use codex_state::WorkflowStore;
use serde_json::json;

use crate::WorkflowCheckpoint;
use crate::WorkflowContext;
use crate::WorkflowName;
use crate::WorkflowRegistry;
use crate::WorkflowRunId;
use crate::WorkflowVersion;
use crate::api::WorkflowContextParts;
use crate::integrations::fornax::FornaxWorkflowClient;
use crate::registry::ErasedWorkflowTransition;

use super::CancellationSignals;

/// Result of advancing a durable reducer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriveOutcome {
    Continued,
    Waiting,
    Completed,
    Failed,
    Cancelling,
    Busy,
    NeedsOperator,
    StepLimitReached,
}

/// Fenced repeated load/step/commit workflow driver.
pub struct WorkflowDriver {
    store: WorkflowStore,
    registry: Arc<WorkflowRegistry>,
    owner: String,
    lease_duration_ms: i64,
    cancellation_signals: CancellationSignals,
    fornax: Option<Arc<FornaxWorkflowClient>>,
    lark: Option<Arc<super::LarkInteractionService>>,
}

impl WorkflowDriver {
    pub fn new(
        store: WorkflowStore,
        registry: Arc<WorkflowRegistry>,
        owner: impl Into<String>,
        lease_duration_ms: i64,
    ) -> Self {
        Self {
            store,
            registry,
            owner: owner.into(),
            lease_duration_ms: lease_duration_ms.max(1),
            cancellation_signals: CancellationSignals::default(),
            fornax: None,
            lark: None,
        }
    }

    pub(crate) fn new_with_signals(
        store: WorkflowStore,
        registry: Arc<WorkflowRegistry>,
        owner: impl Into<String>,
        lease_duration_ms: i64,
        cancellation_signals: CancellationSignals,
        fornax: Option<Arc<FornaxWorkflowClient>>,
        lark: Option<Arc<super::LarkInteractionService>>,
    ) -> Self {
        Self {
            store,
            registry,
            owner: owner.into(),
            lease_duration_ms: lease_duration_ms.max(1),
            cancellation_signals,
            fornax,
            lark,
        }
    }

    pub fn with_fornax_client(mut self, client: Arc<FornaxWorkflowClient>) -> Self {
        self.fornax = Some(client);
        self
    }

    pub fn with_lark_service(mut self, service: Arc<super::LarkInteractionService>) -> Self {
        self.lark = Some(service);
        self
    }

    pub async fn step_once(
        &self,
        run_id: WorkflowRunId,
        now_ms: i64,
    ) -> Result<DriveOutcome, WorkflowDriverError> {
        let Some(lease) = self
            .store
            .acquire_lease(
                &run_id.to_string(),
                &self.owner,
                now_ms,
                self.lease_duration_ms,
            )
            .await?
        else {
            return Ok(DriveOutcome::Busy);
        };
        let result = self.step_with_lease(run_id, now_ms, &lease).await;
        let release = self.store.release_lease(&lease, now_ms).await;
        match (result, release) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error.into()),
            (Ok(outcome), Ok(_)) => Ok(outcome),
        }
    }

    pub async fn drive_until_blocked(
        &self,
        run_id: WorkflowRunId,
        now_ms: i64,
        maximum_steps: NonZeroUsize,
    ) -> Result<DriveOutcome, WorkflowDriverError> {
        let Some(lease) = self
            .store
            .acquire_lease(
                &run_id.to_string(),
                &self.owner,
                now_ms,
                self.lease_duration_ms,
            )
            .await?
        else {
            return Ok(DriveOutcome::Busy);
        };
        let mut result = Ok(DriveOutcome::StepLimitReached);
        for step in 0..maximum_steps.get() {
            let heartbeat_ms = now_ms.saturating_add(i64::try_from(step).unwrap_or(i64::MAX));
            let outcome = match self.step_with_lease(run_id, heartbeat_ms, &lease).await {
                Ok(outcome) => outcome,
                Err(error) => {
                    result = Err(error);
                    break;
                }
            };
            if outcome != DriveOutcome::Continued {
                result = Ok(outcome);
                break;
            }
            match self
                .store
                .renew_lease(&lease, heartbeat_ms, self.lease_duration_ms)
                .await
            {
                Ok(true) => {}
                Ok(false) => {
                    result = Err(codex_state::WorkflowStoreError::StaleWrite.into());
                    break;
                }
                Err(error) => {
                    result = Err(error.into());
                    break;
                }
            }
        }
        let release = self.store.release_lease(&lease, now_ms).await;
        match (result, release) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error.into()),
            (Ok(outcome), Ok(_)) => Ok(outcome),
        }
    }

    async fn step_with_lease(
        &self,
        run_id: WorkflowRunId,
        now_ms: i64,
        lease: &codex_state::WorkflowLease,
    ) -> Result<DriveOutcome, WorkflowDriverError> {
        let run = self
            .store
            .read_run(&run_id.to_string())
            .await?
            .ok_or(codex_state::WorkflowStoreError::RunNotFound)?;
        let cancellation = self
            .cancellation_signals
            .signal(run_id, run.cancellation_requested_at_ms.is_some());
        if run.status == WorkflowRunStatus::Cancelling || run.cancellation_requested_at_ms.is_some()
        {
            return Ok(DriveOutcome::Cancelling);
        }
        if matches!(
            run.status,
            WorkflowRunStatus::Succeeded | WorkflowRunStatus::Failed | WorkflowRunStatus::Cancelled
        ) {
            return Ok(match run.status {
                WorkflowRunStatus::Succeeded => DriveOutcome::Completed,
                WorkflowRunStatus::Failed | WorkflowRunStatus::Cancelled => DriveOutcome::Failed,
                _ => unreachable!(),
            });
        }
        let name = WorkflowName::new(run.definition_name.clone())?;
        let version = WorkflowVersion::parse(&run.definition_version)?;
        let definition = match self.registry.resolve(&name, Some(&version)) {
            Ok(definition) => definition,
            Err(_) => {
                self.store
                    .mark_run_needs_operator(
                        &run.run_id,
                        None,
                        "definition_unavailable",
                        json!({
                            "definition_name": run.definition_name,
                            "definition_version": run.definition_version,
                        }),
                        now_ms,
                    )
                    .await?;
                return Ok(DriveOutcome::NeedsOperator);
            }
        };
        let checkpoint = WorkflowCheckpoint::new(run.state_schema_version, run.state.clone());
        let parts = WorkflowContextParts::with_cancellation(
            run_id,
            cancellation,
            self.fornax.as_ref().map(Arc::clone),
            self.lark.as_ref().map(Arc::clone),
        );
        let transition = definition
            .step(WorkflowContext::new(&parts), checkpoint)
            .await;
        let (status, state_schema_version, state, output, error_code, wake, event_kind, outcome) =
            match transition {
                Ok(ErasedWorkflowTransition::Continue { checkpoint }) => (
                    WorkflowRunStatus::Running,
                    checkpoint.state_schema_version(),
                    checkpoint.state().clone(),
                    None,
                    None,
                    None,
                    "run.continued",
                    DriveOutcome::Continued,
                ),
                Ok(ErasedWorkflowTransition::Wait { checkpoint, wake }) => (
                    WorkflowRunStatus::Waiting,
                    checkpoint.state_schema_version(),
                    checkpoint.state().clone(),
                    None,
                    None,
                    Some(serde_json::to_value(wake)?),
                    "run.waiting",
                    DriveOutcome::Waiting,
                ),
                Ok(ErasedWorkflowTransition::Complete { output }) => (
                    WorkflowRunStatus::Succeeded,
                    run.state_schema_version,
                    run.state.clone(),
                    Some(output),
                    None,
                    None,
                    "run.succeeded",
                    DriveOutcome::Completed,
                ),
                Err(error) => (
                    WorkflowRunStatus::Failed,
                    run.state_schema_version,
                    run.state.clone(),
                    None,
                    Some("workflow_step_failed".to_string()),
                    None,
                    "run.failed",
                    {
                        tracing::warn!(workflow_run_id = %run_id, %error, "workflow reducer failed");
                        DriveOutcome::Failed
                    },
                ),
            };
        self.store
            .transition_run(
                &run.run_id,
                &lease.owner,
                lease.fence,
                run.row_version,
                WorkflowRunTransition {
                    status,
                    state_schema_version,
                    state,
                    output,
                    error_code,
                    wake,
                    event_kind: event_kind.to_string(),
                    event_entity_id: Some(run.run_id.clone()),
                    event_metadata: json!({}),
                    updated_at_ms: now_ms,
                },
            )
            .await?;
        if matches!(outcome, DriveOutcome::Completed | DriveOutcome::Failed) {
            self.cancellation_signals.release(run_id);
        }
        Ok(outcome)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WorkflowDriverError {
    #[error(transparent)]
    Store(#[from] codex_state::WorkflowStoreError),
    #[error(transparent)]
    Identifier(#[from] crate::IdentifierError),
    #[error(transparent)]
    Serialization(#[from] serde_json::Error),
}
