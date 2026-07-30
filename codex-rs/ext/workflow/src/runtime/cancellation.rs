use std::time::Duration;

use codex_state::WorkflowNodeAttemptStatus;
use codex_state::WorkflowNodeAttemptTransition;
use codex_state::WorkflowRunRecord;
use codex_state::WorkflowStore;

use crate::CancellationReason;
use crate::WorkflowRunId;

use super::WorkflowService;

/// Observable result of one durable cooperative cancellation.
#[derive(Clone, Debug, PartialEq)]
pub struct CancellationReport {
    pub run: WorkflowRunRecord,
    pub interrupted_turns: usize,
    pub ambiguous_turns: usize,
}

/// Coordinates durable cancellation with normal Codex interrupts.
#[derive(Clone)]
pub struct WorkflowCancellationController {
    service: WorkflowService,
    store: WorkflowStore,
}

impl WorkflowCancellationController {
    pub fn new(service: WorkflowService) -> Self {
        Self {
            store: service.store().clone(),
            service,
        }
    }

    pub async fn cancel_run(
        &self,
        run_id: WorkflowRunId,
        reason: CancellationReason,
        now_ms: i64,
        grace: Duration,
    ) -> Result<CancellationReport, CancellationError> {
        let requested = self
            .store
            .request_run_cancellation(&run_id.to_string(), now_ms)
            .await?;
        if requested.status == codex_state::WorkflowRunStatus::Cancelled {
            return Ok(CancellationReport {
                run: requested,
                interrupted_turns: 0,
                ambiguous_turns: 0,
            });
        }
        self.service.cancellation_signals().request(run_id);
        let nodes = self.store.list_nodes(&run_id.to_string()).await?;
        let mut interrupted_turns = 0usize;
        let mut ambiguous_turns = 0usize;
        for node in nodes {
            let attempts = self.store.list_node_attempts(&node.node_id).await?;
            for attempt in attempts
                .into_iter()
                .filter(|attempt| !attempt.status.is_terminal())
            {
                let Some(turn_id) = attempt.turn_id.clone() else {
                    self.store
                        .transition_node_attempt(
                            &attempt.attempt_id,
                            WorkflowNodeAttemptTransition {
                                expected_status: attempt.status,
                                status: WorkflowNodeAttemptStatus::Cancelled,
                                turn_id: None,
                                error_code: Some("cancelled_before_submission".to_string()),
                                updated_at_ms: now_ms,
                            },
                        )
                        .await?;
                    continue;
                };
                let handle = self
                    .service
                    .nodes(run_id)
                    .get(crate::NodeId::parse(&node.node_id)?)
                    .await?;
                let accepted = handle.cancel(reason.clone()).await.is_ok();
                if accepted
                    && tokio::time::timeout(grace, handle.await_turn(turn_id))
                        .await
                        .is_ok_and(|result| result.is_ok())
                {
                    interrupted_turns += 1;
                } else {
                    self.mark_ambiguous(&attempt, now_ms).await?;
                    ambiguous_turns += 1;
                }
                let _ = handle.detach_observer().await;
            }
        }
        let run = self
            .store
            .finish_run_cancellation(&run_id.to_string(), now_ms)
            .await?;
        self.service.cancellation_signals().release(run_id);
        Ok(CancellationReport {
            run,
            interrupted_turns,
            ambiguous_turns,
        })
    }

    async fn mark_ambiguous(
        &self,
        attempt: &codex_state::WorkflowNodeAttemptRecord,
        now_ms: i64,
    ) -> Result<(), CancellationError> {
        if attempt.status.is_terminal() {
            return Ok(());
        }
        self.store
            .transition_node_attempt(
                &attempt.attempt_id,
                WorkflowNodeAttemptTransition {
                    expected_status: attempt.status,
                    status: WorkflowNodeAttemptStatus::Ambiguous,
                    turn_id: attempt.turn_id.clone(),
                    error_code: Some("cancellation_grace_expired".to_string()),
                    updated_at_ms: now_ms,
                },
            )
            .await?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CancellationError {
    #[error(transparent)]
    Store(#[from] codex_state::WorkflowStoreError),
    #[error(transparent)]
    Identifier(#[from] crate::IdentifierError),
    #[error(transparent)]
    Node(#[from] super::NodeError),
}
