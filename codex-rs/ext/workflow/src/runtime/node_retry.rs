use codex_protocol::ThreadId;
use codex_state::WorkflowEffectPlan;
use codex_state::WorkflowEffectState;
use codex_state::WorkflowEffectUpdate;
use serde_json::json;

use crate::EffectKey;
use crate::RetryDecision;
use crate::RetryPlanner;
use crate::RetryRequest;
use crate::RetrySession;

use super::MaterializeNodeRequest;
use super::NodeError;
use super::NodeHandle;
use super::SubmittedTurn;
use super::service::now_ms;

impl NodeHandle {
    pub async fn retry(
        &self,
        effect: EffectKey,
        request: RetryRequest,
    ) -> Result<SubmittedTurn, NodeError> {
        let attempts = self
            .service
            .store()
            .list_node_attempts(&self.binding.node_id.to_string())
            .await?;
        let run = self
            .service
            .store()
            .read_run(&self.binding.run_id.to_string())
            .await?
            .ok_or(NodeError::NotFound)?;
        let last_attempt_id = attempts
            .last()
            .map(|attempt| attempt.attempt_id.as_str())
            .unwrap_or("initial");
        let now = now_ms()?;
        let decision = RetryPlanner::decide(
            self.spec.retry(),
            u32::try_from(attempts.len()).unwrap_or(u32::MAX),
            request.classification,
            last_attempt_id,
            now,
            run.deadline_ms,
        );
        let RetryDecision::RetryAt {
            retry_at_ms,
            session,
        } = decision
        else {
            return Err(NodeError::RetryExhausted);
        };
        if retry_at_ms > now {
            return Err(NodeError::RetryNotReady { retry_at_ms });
        }
        let handle = match session {
            RetrySession::NewTurnOnSameThread => self.clone(),
            RetrySession::FreshThread => self.materialize_fresh_retry(&effect, now).await?,
        };
        handle.submit(effect, request.input).await
    }

    async fn materialize_fresh_retry(
        &self,
        turn_effect: &EffectKey,
        now: i64,
    ) -> Result<Self, NodeError> {
        let effect = EffectKey::new(format!("{turn_effect}/fresh-thread"))?;
        let planned = self
            .service
            .store()
            .plan_effect(WorkflowEffectPlan {
                run_id: self.binding.run_id.to_string(),
                effect_key: effect.to_string(),
                kind: "node.retry.thread".to_string(),
                request: json!({ "node_id": self.binding.node_id }),
                created_at_ms: now,
            })
            .await?;
        let mut record = match planned {
            codex_state::WorkflowEffectPlanOutcome::Planned(record)
            | codex_state::WorkflowEffectPlanOutcome::Existing(record) => record,
        };
        let thread_id = if record.state == WorkflowEffectState::Applied {
            let value = record
                .response
                .as_ref()
                .and_then(|response| response.get("thread_id"))
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    NodeError::InvalidState("fresh-thread effect has no thread_id".to_string())
                })?;
            ThreadId::from_string(value)
                .map_err(|error| NodeError::InvalidThreadId(error.to_string()))?
        } else {
            let host = self.service.inner.node_host.get()?;
            if record.state == WorkflowEffectState::Planned {
                self.service
                    .store()
                    .update_effect(WorkflowEffectUpdate {
                        run_id: self.binding.run_id.to_string(),
                        effect_key: effect.to_string(),
                        expected_state: WorkflowEffectState::Planned,
                        state: WorkflowEffectState::Dispatched,
                        response: None,
                        error_code: None,
                        updated_at_ms: now,
                    })
                    .await?;
                record.state = WorkflowEffectState::Dispatched;
            }
            if record.state != WorkflowEffectState::Dispatched {
                return Err(NodeError::InvalidState(
                    "fresh-thread effect requires reconciliation".to_string(),
                ));
            }
            let known = self
                .service
                .store()
                .list_node_threads(&self.binding.node_id.to_string())
                .await?;
            let thread_id = if let Some(value) = record
                .response
                .as_ref()
                .and_then(|response| response.get("thread_id"))
                .and_then(serde_json::Value::as_str)
            {
                ThreadId::from_string(value)
                    .map_err(|error| NodeError::InvalidThreadId(error.to_string()))?
            } else {
                let discovered = host.find_materialized_nodes(self.binding.clone()).await?;
                let mut unknown = discovered.into_iter().filter(|thread_id| {
                    !known
                        .iter()
                        .any(|known| known.thread_id == thread_id.to_string())
                });
                let candidate = unknown.next();
                if unknown.next().is_some() {
                    return Err(NodeError::InvalidState(
                        "fresh-thread effect discovered multiple unadopted threads".to_string(),
                    ));
                }
                let thread_id = match candidate {
                    Some(thread_id) => thread_id,
                    None => {
                        host.materialize_node(MaterializeNodeRequest {
                            binding: self.binding.clone(),
                            spec: self.spec.clone(),
                        })
                        .await?
                        .thread_id
                    }
                };
                self.service
                    .store()
                    .update_effect(WorkflowEffectUpdate {
                        run_id: self.binding.run_id.to_string(),
                        effect_key: effect.to_string(),
                        expected_state: WorkflowEffectState::Dispatched,
                        state: WorkflowEffectState::Dispatched,
                        response: Some(json!({ "thread_id": thread_id })),
                        error_code: None,
                        updated_at_ms: now,
                    })
                    .await?;
                thread_id
            };
            if !known
                .iter()
                .any(|known| known.thread_id == thread_id.to_string())
            {
                self.service
                    .store()
                    .append_node_thread(
                        &self.binding.node_id.to_string(),
                        &thread_id.to_string(),
                        now,
                    )
                    .await?;
            }
            if !self
                .service
                .store()
                .update_effect(WorkflowEffectUpdate {
                    run_id: self.binding.run_id.to_string(),
                    effect_key: effect.to_string(),
                    expected_state: WorkflowEffectState::Dispatched,
                    state: WorkflowEffectState::Applied,
                    response: Some(json!({ "thread_id": thread_id })),
                    error_code: None,
                    updated_at_ms: now,
                })
                .await?
            {
                return Err(NodeError::InvalidState(
                    "fresh-thread effect lost its planned state".to_string(),
                ));
            }
            thread_id
        };
        Ok(Self {
            service: self.service.clone(),
            binding: self.binding.clone(),
            thread_id,
            spec: self.spec.clone(),
        })
    }
}
