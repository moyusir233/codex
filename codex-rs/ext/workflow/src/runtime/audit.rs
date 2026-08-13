use codex_state::WorkflowEffectPlan;
use codex_state::WorkflowEffectPlanOutcome;
use codex_state::WorkflowEffectState;
use codex_state::WorkflowEffectUpdate;
use codex_state::WorkflowStore;
use serde_json::json;

use crate::WorkflowAuditRecord;
use crate::WorkflowRunId;
use crate::integrations::fornax::redaction::Redactor;

/// Run-scoped, redacting audit facade exposed to reducer code.
#[derive(Clone)]
pub struct WorkflowAuditClient {
    store: WorkflowStore,
    run_id: WorkflowRunId,
    redactor: Redactor,
}

impl WorkflowAuditClient {
    pub(crate) fn new(store: WorkflowStore, run_id: WorkflowRunId, redactor: Redactor) -> Self {
        Self {
            store,
            run_id,
            redactor,
        }
    }

    /// Appends one idempotent audit record after redacting every text field.
    pub async fn append(
        &self,
        record: WorkflowAuditRecord,
        now_ms: i64,
    ) -> Result<(), WorkflowAuditError> {
        let request = json!({
            "kind": record.kind,
            "subject_id": record.subject_id.map(|value| self.redactor.redact(&value)),
            "metadata": record.metadata.into_iter().map(|(key, value)| {
                (self.redactor.redact(&key), self.redactor.redact(&value))
            }).collect::<std::collections::BTreeMap<_, _>>(),
        });
        let planned = self
            .store
            .plan_effect(WorkflowEffectPlan {
                run_id: self.run_id.to_string(),
                effect_key: record.effect_key.to_string(),
                kind: "audit.append".to_string(),
                request,
                created_at_ms: now_ms,
            })
            .await?;
        let effect = match planned {
            WorkflowEffectPlanOutcome::Planned(effect)
            | WorkflowEffectPlanOutcome::Existing(effect) => effect,
        };
        if effect.state == WorkflowEffectState::Applied {
            return Ok(());
        }
        if effect.state != WorkflowEffectState::Planned
            || !self
                .store
                .update_effect(WorkflowEffectUpdate {
                    run_id: self.run_id.to_string(),
                    effect_key: record.effect_key.to_string(),
                    expected_state: WorkflowEffectState::Planned,
                    state: WorkflowEffectState::Applied,
                    response: Some(json!({"recorded": true})),
                    error_code: None,
                    updated_at_ms: now_ms,
                })
                .await?
        {
            return Err(WorkflowAuditError::ConcurrentMutation);
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WorkflowAuditError {
    #[error("durable audit record changed concurrently")]
    ConcurrentMutation,
    #[error(transparent)]
    Store(#[from] codex_state::WorkflowStoreError),
}
