use codex_protocol::ThreadId;
use codex_state::WorkflowEffectPlan;
use codex_state::WorkflowEffectState;
use codex_state::WorkflowEffectUpdate;
use codex_state::WorkflowNodeAttemptCreate;
use codex_state::WorkflowNodeAttemptStatus;
use codex_state::WorkflowNodeAttemptTransition;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;

use crate::CancellationReason;
use crate::EffectKey;
use crate::NodeId;
use crate::NodeInput;
use crate::NodeSpec;
use crate::NodeThreadRef;
use crate::NodeTurnResult;
use crate::WorkflowNodeBinding;

use super::AwaitTurnRequest;
use super::ConfirmedHistoryDeletion;
use super::FailurePoint;
use super::NodeError;
use super::NodeRuntimeStatus;
use super::PreparedTurnRequest;
use super::RuntimeShutdown;
use super::SteerTurnRequest;
use super::SubmittedTurn;
use super::WorkflowService;
use super::service::now_ms;

/// Handle for a materialized workflow node. Dropping it intentionally performs no operation.
#[derive(Clone)]
pub struct NodeHandle {
    pub(super) service: WorkflowService,
    pub(super) binding: WorkflowNodeBinding,
    pub(super) thread_id: ThreadId,
    pub(super) spec: NodeSpec,
}

impl NodeHandle {
    pub fn id(&self) -> NodeId {
        self.binding.node_id
    }

    pub fn thread_id(&self) -> ThreadId {
        self.thread_id
    }

    pub async fn thread_refs(&self) -> Result<Vec<NodeThreadRef>, NodeError> {
        self.service
            .store()
            .list_node_threads(&self.binding.node_id.to_string())
            .await?
            .into_iter()
            .map(|record| {
                Ok(NodeThreadRef {
                    thread_id: ThreadId::from_string(&record.thread_id)
                        .map_err(|error| NodeError::InvalidThreadId(error.to_string()))?,
                    ordinal: record.ordinal,
                })
            })
            .collect()
    }

    pub async fn start(
        &self,
        effect: EffectKey,
        input: NodeInput,
    ) -> Result<SubmittedTurn, NodeError> {
        self.submit(effect, input).await
    }

    pub async fn submit(
        &self,
        effect: EffectKey,
        input: NodeInput,
    ) -> Result<SubmittedTurn, NodeError> {
        let now = now_ms()?;
        self.service
            .failure_injector()
            .checkpoint(FailurePoint::BeforeDbWrite)?;
        let effect_key = effect.to_string();
        let request = json!({
            "node_id": self.binding.node_id,
            "input": input,
        });
        let input_bytes = serde_json::to_vec(&request)?;
        let digest = Sha256::digest(input_bytes);
        let input_hash_text = format!("{digest:x}");
        let input_hash: [u8; 32] = digest.into();
        let planned = self
            .service
            .store()
            .plan_effect(WorkflowEffectPlan {
                run_id: self.binding.run_id.to_string(),
                effect_key: effect_key.clone(),
                kind: "node.turn.submit".to_string(),
                request,
                created_at_ms: now,
            })
            .await?;
        self.service
            .failure_injector()
            .checkpoint(FailurePoint::AfterDbWrite)?;
        let mut effect_record = match planned {
            codex_state::WorkflowEffectPlanOutcome::Planned(record)
            | codex_state::WorkflowEffectPlanOutcome::Existing(record) => record,
        };
        if effect_record.state == WorkflowEffectState::Applied {
            return submitted_turn_from_effect(&effect_record);
        }
        if effect_record.state == WorkflowEffectState::Planned {
            let attempt_id = crate::NodeAttemptId::new().to_string();
            let submission_id = attempt_id.clone();
            self.service
                .store()
                .create_node_attempt(
                    &self.binding.run_id.to_string(),
                    WorkflowNodeAttemptCreate {
                        attempt_id: attempt_id.clone(),
                        node_id: self.binding.node_id.to_string(),
                        submission_id: submission_id.clone(),
                        input_hash: input_hash_text,
                        thread_id: Some(self.thread_id.to_string()),
                        created_at_ms: now,
                    },
                )
                .await?;
            let updated = self
                .service
                .store()
                .update_effect(WorkflowEffectUpdate {
                    run_id: self.binding.run_id.to_string(),
                    effect_key: effect_key.clone(),
                    expected_state: WorkflowEffectState::Planned,
                    state: WorkflowEffectState::Dispatched,
                    response: Some(json!({
                        "attempt_id": attempt_id,
                        "submission_id": submission_id,
                    })),
                    error_code: None,
                    updated_at_ms: now,
                })
                .await?;
            effect_record = if updated {
                self.service
                    .store()
                    .read_effect(&self.binding.run_id.to_string(), &effect_key)
                    .await?
                    .ok_or_else(|| {
                        NodeError::InvalidState(
                            "dispatched node turn effect disappeared".to_string(),
                        )
                    })?
            } else {
                self.service
                    .store()
                    .read_effect(&self.binding.run_id.to_string(), &effect_key)
                    .await?
                    .ok_or_else(|| {
                        NodeError::InvalidState("node turn effect disappeared".to_string())
                    })?
            };
        }
        if effect_record.state == WorkflowEffectState::Applied {
            return submitted_turn_from_effect(&effect_record);
        }
        if effect_record.state != WorkflowEffectState::Dispatched {
            return Err(NodeError::InvalidState(format!(
                "node turn effect is in unexpected state {:?}",
                effect_record.state
            )));
        }
        let submission_id = effect_record
            .response
            .as_ref()
            .and_then(|response| response.get("submission_id"))
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                NodeError::InvalidState(
                    "dispatched node turn effect has no submission_id".to_string(),
                )
            })?
            .to_string();
        let attempt = self
            .service
            .store()
            .read_node_attempt_by_submission_id(&submission_id)
            .await?
            .ok_or_else(|| {
                NodeError::InvalidState(
                    "dispatched node turn effect has no durable attempt".to_string(),
                )
            })?;
        let host = self.service.inner.node_host.get()?;
        self.service
            .failure_injector()
            .checkpoint(FailurePoint::BeforeTurnQueue)?;
        let submitted = host
            .submit_prepared_turn(PreparedTurnRequest {
                thread_id: self.thread_id,
                submission_id,
                input_hash,
                input,
                spec: self.spec.clone(),
            })
            .await?;
        self.service
            .failure_injector()
            .checkpoint(FailurePoint::AfterTurnQueue)?;
        if attempt.status == WorkflowNodeAttemptStatus::Planned {
            self.service
                .store()
                .transition_node_attempt(
                    &attempt.attempt_id,
                    WorkflowNodeAttemptTransition {
                        expected_status: WorkflowNodeAttemptStatus::Planned,
                        status: WorkflowNodeAttemptStatus::Submitted,
                        turn_id: Some(submitted.turn_id.clone()),
                        error_code: None,
                        updated_at_ms: now_ms()?,
                    },
                )
                .await?;
            self.service
                .failure_injector()
                .checkpoint(FailurePoint::AfterUserBoundaryAppend)?;
        } else if attempt.status != WorkflowNodeAttemptStatus::Submitted
            || attempt.turn_id.as_deref() != Some(submitted.turn_id.as_str())
        {
            return Err(NodeError::InvalidState(
                "prepared turn reconciled to a conflicting durable attempt".to_string(),
            ));
        }
        let updated = self
            .service
            .store()
            .update_effect(WorkflowEffectUpdate {
                run_id: self.binding.run_id.to_string(),
                effect_key: effect_key.clone(),
                expected_state: WorkflowEffectState::Dispatched,
                state: WorkflowEffectState::Applied,
                response: Some(json!({
                    "attempt_id": attempt.attempt_id,
                    "turn_id": &submitted.turn_id,
                    "disposition": disposition_name(submitted.disposition),
                })),
                error_code: None,
                updated_at_ms: now_ms()?,
            })
            .await?;
        if updated {
            return Ok(submitted);
        }
        let effect_record = self
            .service
            .store()
            .read_effect(&self.binding.run_id.to_string(), &effect_key)
            .await?
            .ok_or_else(|| NodeError::InvalidState("node turn effect disappeared".to_string()))?;
        if effect_record.state != WorkflowEffectState::Applied {
            return Err(NodeError::InvalidState(
                "node turn effect lost its dispatched state".to_string(),
            ));
        }
        submitted_turn_from_effect(&effect_record)
    }

    pub async fn await_turn(
        &self,
        turn_id: impl Into<String>,
    ) -> Result<NodeTurnResult, NodeError> {
        let turn_id = turn_id.into();
        let attempt = self
            .service
            .store()
            .read_node_attempt_by_turn_id(&self.binding.node_id.to_string(), &turn_id)
            .await?
            .ok_or_else(|| {
                NodeError::InvalidState("turn has no durable node attempt".to_string())
            })?;
        let host = self.service.inner.node_host.get()?;
        let result = host
            .await_terminal_turn(AwaitTurnRequest {
                thread_id: self.thread_id,
                turn_id,
            })
            .await?;
        self.service
            .failure_injector()
            .checkpoint(FailurePoint::AfterTerminalDelivery)?;
        self.service
            .failure_injector()
            .checkpoint(FailurePoint::AfterTerminalFlush)?;
        let terminal_status = match result.status {
            crate::NodeTurnStatus::Completed => WorkflowNodeAttemptStatus::Succeeded,
            crate::NodeTurnStatus::Interrupted => WorkflowNodeAttemptStatus::Interrupted,
            crate::NodeTurnStatus::Failed => WorkflowNodeAttemptStatus::Failed,
        };
        if !attempt.status.is_terminal() {
            self.service
                .store()
                .transition_node_attempt(
                    &attempt.attempt_id,
                    WorkflowNodeAttemptTransition {
                        expected_status: attempt.status,
                        status: terminal_status,
                        turn_id: Some(result.turn_id.clone()),
                        error_code: result.error.as_ref().map(|_| "turn_failed".to_string()),
                        updated_at_ms: now_ms()?,
                    },
                )
                .await?;
        } else if attempt.status != terminal_status {
            return Err(NodeError::InvalidState(
                "terminal turn conflicts with its durable attempt".to_string(),
            ));
        }
        Ok(result)
    }

    pub async fn steer(
        &self,
        turn_id: impl Into<String>,
        input: NodeInput,
    ) -> Result<(), NodeError> {
        let host = self.service.inner.node_host.get()?;
        host.steer(SteerTurnRequest {
            thread_id: self.thread_id,
            turn_id: turn_id.into(),
            input,
        })
        .await
        .map_err(Into::into)
    }

    pub async fn status(&self) -> Result<NodeRuntimeStatus, NodeError> {
        self.service
            .inner
            .node_host
            .get()?
            .status(self.thread_id)
            .await
            .map_err(Into::into)
    }

    pub async fn interrupt(&self, turn_id: impl Into<String>) -> Result<(), NodeError> {
        self.service
            .inner
            .node_host
            .get()?
            .interrupt(self.thread_id, turn_id.into())
            .await
            .map_err(Into::into)
    }

    pub async fn cancel(&self, reason: CancellationReason) -> Result<(), NodeError> {
        self.service
            .inner
            .node_host
            .get()?
            .cancel(self.thread_id, reason)
            .await
            .map_err(Into::into)
    }

    pub async fn shutdown_runtime(&self, mode: RuntimeShutdown) -> Result<(), NodeError> {
        self.service
            .inner
            .node_host
            .get()?
            .shutdown_runtime(self.thread_id, mode)
            .await
            .map_err(Into::into)
    }

    pub async fn detach_observer(self) -> Result<(), NodeError> {
        self.service
            .inner
            .node_host
            .get()?
            .detach_observer(self.thread_id)
            .await
            .map_err(Into::into)
    }

    pub async fn archive_history(&self) -> Result<(), NodeError> {
        self.service
            .inner
            .node_host
            .get()?
            .archive(self.thread_id, true)
            .await
            .map_err(Into::into)
    }

    pub async fn unarchive_history(&self) -> Result<(), NodeError> {
        self.service
            .inner
            .node_host
            .get()?
            .archive(self.thread_id, false)
            .await
            .map_err(Into::into)
    }

    pub async fn delete_history(
        &self,
        confirmation: ConfirmedHistoryDeletion,
    ) -> Result<(), NodeError> {
        if confirmation.binding != self.binding || confirmation.thread_id != self.thread_id {
            return Err(NodeError::InvalidState(
                "history deletion confirmation does not match this node".to_string(),
            ));
        }
        self.service
            .inner
            .node_host
            .get()?
            .delete(confirmation)
            .await
            .map_err(Into::into)
    }
}

fn disposition_name(disposition: super::PreparedTurnDisposition) -> &'static str {
    match disposition {
        super::PreparedTurnDisposition::Queued => "queued",
        super::PreparedTurnDisposition::AlreadyQueued => "already_queued",
        super::PreparedTurnDisposition::BoundaryAlreadyPersisted => "boundary_already_persisted",
    }
}

fn submitted_turn_from_effect(
    effect: &codex_state::WorkflowEffectRecord,
) -> Result<SubmittedTurn, NodeError> {
    let response = effect.response.as_ref().ok_or_else(|| {
        NodeError::InvalidState("applied node turn effect has no response".to_string())
    })?;
    let turn_id = response
        .get("turn_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            NodeError::InvalidState("applied node turn effect has no turn_id".to_string())
        })?;
    let disposition = match response
        .get("disposition")
        .and_then(serde_json::Value::as_str)
    {
        Some("queued") => super::PreparedTurnDisposition::Queued,
        Some("already_queued") => super::PreparedTurnDisposition::AlreadyQueued,
        Some("boundary_already_persisted") => {
            super::PreparedTurnDisposition::BoundaryAlreadyPersisted
        }
        _ => {
            return Err(NodeError::InvalidState(
                "applied node turn effect has invalid disposition".to_string(),
            ));
        }
    };
    Ok(SubmittedTurn {
        turn_id: turn_id.to_string(),
        disposition,
    })
}
