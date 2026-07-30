use std::future::Future;

use codex_state::WorkflowEffectPlan;
use codex_state::WorkflowEffectPlanOutcome;
use codex_state::WorkflowEffectState;
use codex_state::WorkflowEffectUpdate;
use codex_state::WorkflowStore;
use serde_json::Value;

use crate::EffectKey;
use crate::WorkflowRunId;

use super::FailureInjector;
use super::FailurePoint;

/// Write-ahead external mutation outbox.
#[derive(Clone)]
pub struct DurableOutbox {
    store: WorkflowStore,
    failure_injector: FailureInjector,
}

impl DurableOutbox {
    pub fn new(store: WorkflowStore, failure_injector: FailureInjector) -> Self {
        Self {
            store,
            failure_injector,
        }
    }

    pub async fn dispatch<F, Fut>(
        &self,
        run_id: WorkflowRunId,
        effect_key: EffectKey,
        kind: &str,
        request: Value,
        now_ms: i64,
        operation: F,
    ) -> Result<Value, OutboxError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Value, OutboxError>>,
    {
        let run_id = run_id.to_string();
        let effect_key = effect_key.to_string();
        let planned = self
            .store
            .plan_effect(WorkflowEffectPlan {
                run_id: run_id.clone(),
                effect_key: effect_key.clone(),
                kind: kind.to_string(),
                request,
                created_at_ms: now_ms,
            })
            .await?;
        let effect = match planned {
            WorkflowEffectPlanOutcome::Planned(effect)
            | WorkflowEffectPlanOutcome::Existing(effect) => effect,
        };
        if effect.state == WorkflowEffectState::Applied {
            return effect.response.ok_or(OutboxError::MissingResponse);
        }
        if effect.state != WorkflowEffectState::Planned {
            return Err(OutboxError::RequiresReconciliation(effect.state));
        }
        if !self
            .store
            .update_effect(WorkflowEffectUpdate {
                run_id: run_id.clone(),
                effect_key: effect_key.clone(),
                expected_state: WorkflowEffectState::Planned,
                state: WorkflowEffectState::Dispatched,
                response: None,
                error_code: None,
                updated_at_ms: now_ms,
            })
            .await?
        {
            return Err(OutboxError::RequiresReconciliation(
                WorkflowEffectState::Dispatched,
            ));
        }
        self.failure_injector
            .checkpoint(FailurePoint::BeforeExternalDispatch)?;
        let response = operation().await?;
        self.failure_injector
            .checkpoint(FailurePoint::AfterExternalDispatch)?;
        self.reconcile_success(&run_id, &effect_key, response.clone(), now_ms)
            .await?;
        Ok(response)
    }

    pub async fn reconcile_success(
        &self,
        run_id: &str,
        effect_key: &str,
        response: Value,
        now_ms: i64,
    ) -> Result<(), OutboxError> {
        if self
            .store
            .update_effect(WorkflowEffectUpdate {
                run_id: run_id.to_string(),
                effect_key: effect_key.to_string(),
                expected_state: WorkflowEffectState::Dispatched,
                state: WorkflowEffectState::Applied,
                response: Some(response),
                error_code: None,
                updated_at_ms: now_ms,
            })
            .await?
        {
            return Ok(());
        }
        let effect = self
            .store
            .read_effect(run_id, effect_key)
            .await?
            .ok_or(OutboxError::MissingEffect)?;
        if effect.state == WorkflowEffectState::Applied {
            Ok(())
        } else {
            Err(OutboxError::RequiresReconciliation(effect.state))
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OutboxError {
    #[error(transparent)]
    Store(#[from] codex_state::WorkflowStoreError),
    #[error(transparent)]
    Injected(#[from] super::InjectedFailure),
    #[error("workflow outbox effect requires reconciliation from state {0:?}")]
    RequiresReconciliation(WorkflowEffectState),
    #[error("applied workflow outbox effect has no response")]
    MissingResponse,
    #[error("workflow outbox effect was not found")]
    MissingEffect,
    #[error("workflow outbox operation failed: {0}")]
    Operation(String),
}
