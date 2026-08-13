use serde_json::json;
use sqlx::Row;

use crate::WorkflowFornaxDeliveryProofRecord;
use crate::WorkflowFornaxTraceRecord;
use crate::WorkflowFornaxTraceState;

use super::WorkflowStore;
use super::WorkflowStoreError;
use super::events::append_event;

pub struct WorkflowFornaxTracePlan {
    pub run_id: String,
    pub effect_key: String,
    pub operation_id: String,
    pub request_hash: String,
    pub created_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkflowFornaxTracePlanOutcome {
    Planned(WorkflowFornaxTraceRecord),
    Existing(WorkflowFornaxTraceRecord),
}

pub struct WorkflowFornaxTraceUpdate {
    pub run_id: String,
    pub effect_key: String,
    pub expected_state: WorkflowFornaxTraceState,
    pub state: WorkflowFornaxTraceState,
    pub span_handle_id: Option<String>,
    pub trace_context_id: Option<String>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub bridge_instance_id: Option<String>,
    pub error_code: Option<String>,
    pub updated_at_ms: i64,
}

pub struct WorkflowFornaxDeliveryProofPlan {
    pub run_id: String,
    pub effect_key: String,
    pub operation_id: String,
    pub request_hash: String,
    pub remote_trace_id: String,
    pub remote_span_id: String,
    pub proof_kind: String,
    pub proof_digest: String,
    pub reconciled_at_ms: i64,
}

impl WorkflowStore {
    /// Persists a stable bridge operation before dispatching it.
    pub async fn plan_fornax_trace(
        &self,
        plan: WorkflowFornaxTracePlan,
    ) -> Result<WorkflowFornaxTracePlanOutcome, WorkflowStoreError> {
        let mut tx = self.pool().begin().await?;
        let result = sqlx::query(
            r#"
INSERT INTO workflow_fornax_traces (
    run_id, effect_key, operation_id, request_hash, state,
    created_at_ms, updated_at_ms
) VALUES (?, ?, ?, ?, 'planned', ?, ?)
ON CONFLICT(run_id, effect_key) DO NOTHING
            "#,
        )
        .bind(&plan.run_id)
        .bind(&plan.effect_key)
        .bind(&plan.operation_id)
        .bind(&plan.request_hash)
        .bind(plan.created_at_ms)
        .bind(plan.created_at_ms)
        .execute(&mut *tx)
        .await?;
        let row = sqlx::query(
            r#"
SELECT run_id, effect_key, operation_id, request_hash, span_handle_id,
       trace_context_id, trace_id, span_id, bridge_instance_id, state, error_code
FROM workflow_fornax_traces
WHERE run_id = ? AND effect_key = ?
            "#,
        )
        .bind(&plan.run_id)
        .bind(&plan.effect_key)
        .fetch_one(&mut *tx)
        .await?;
        let record = fornax_trace_from_row(row)?;
        if record.operation_id != plan.operation_id || record.request_hash != plan.request_hash {
            return Err(WorkflowStoreError::FornaxCorrelationConflict);
        }
        if result.rows_affected() == 1 {
            append_event(
                &mut tx,
                &plan.run_id,
                "fornax.trace_planned",
                Some(&plan.operation_id),
                &json!({
                    "effect_key": plan.effect_key,
                    "request_hash": plan.request_hash,
                }),
                plan.created_at_ms,
            )
            .await?;
            tx.commit().await?;
            Ok(WorkflowFornaxTracePlanOutcome::Planned(record))
        } else {
            tx.commit().await?;
            Ok(WorkflowFornaxTracePlanOutcome::Existing(record))
        }
    }

    pub async fn read_fornax_trace(
        &self,
        run_id: &str,
        effect_key: &str,
    ) -> Result<Option<WorkflowFornaxTraceRecord>, WorkflowStoreError> {
        let row = sqlx::query(
            r#"
SELECT run_id, effect_key, operation_id, request_hash, span_handle_id,
       trace_context_id, trace_id, span_id, bridge_instance_id, state, error_code
FROM workflow_fornax_traces
WHERE run_id = ? AND effect_key = ?
            "#,
        )
        .bind(run_id)
        .bind(effect_key)
        .fetch_optional(self.pool())
        .await?;
        row.map(fornax_trace_from_row).transpose()
    }

    /// Lists the durable local-delivery journal for one run in effect-key order.
    pub async fn list_fornax_traces(
        &self,
        run_id: &str,
    ) -> Result<Vec<WorkflowFornaxTraceRecord>, WorkflowStoreError> {
        let rows = sqlx::query(
            r#"
SELECT run_id, effect_key, operation_id, request_hash, span_handle_id,
       trace_context_id, trace_id, span_id, bridge_instance_id, state, error_code
FROM workflow_fornax_traces
WHERE run_id = ?
ORDER BY effect_key
            "#,
        )
        .bind(run_id)
        .fetch_all(self.pool())
        .await?;
        rows.into_iter().map(fornax_trace_from_row).collect()
    }

    /// Appends exact remote reconciliation evidence without rewriting local bridge history.
    pub async fn record_fornax_delivery_proof(
        &self,
        proof: WorkflowFornaxDeliveryProofPlan,
    ) -> Result<WorkflowFornaxDeliveryProofRecord, WorkflowStoreError> {
        let mut tx = self.pool().begin().await?;
        let correlation = sqlx::query(
            r#"
SELECT operation_id, request_hash
FROM workflow_fornax_traces
WHERE run_id = ? AND effect_key = ?
            "#,
        )
        .bind(&proof.run_id)
        .bind(&proof.effect_key)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(correlation) = correlation else {
            return Err(WorkflowStoreError::FornaxDeliveryProofConflict);
        };
        if correlation.try_get::<String, _>("operation_id")? != proof.operation_id
            || correlation.try_get::<String, _>("request_hash")? != proof.request_hash
        {
            return Err(WorkflowStoreError::FornaxDeliveryProofConflict);
        }
        let result = sqlx::query(
            r#"
INSERT INTO workflow_fornax_delivery_proofs (
    run_id, effect_key, operation_id, request_hash, remote_trace_id,
    remote_span_id, proof_kind, proof_digest, reconciled_at_ms
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
ON CONFLICT(run_id, effect_key) DO NOTHING
            "#,
        )
        .bind(&proof.run_id)
        .bind(&proof.effect_key)
        .bind(&proof.operation_id)
        .bind(&proof.request_hash)
        .bind(&proof.remote_trace_id)
        .bind(&proof.remote_span_id)
        .bind(&proof.proof_kind)
        .bind(&proof.proof_digest)
        .bind(proof.reconciled_at_ms)
        .execute(&mut *tx)
        .await?;
        let row = sqlx::query(
            r#"
SELECT run_id, effect_key, operation_id, request_hash, remote_trace_id,
       remote_span_id, proof_kind, proof_digest, reconciled_at_ms
FROM workflow_fornax_delivery_proofs
WHERE run_id = ? AND effect_key = ?
            "#,
        )
        .bind(&proof.run_id)
        .bind(&proof.effect_key)
        .fetch_one(&mut *tx)
        .await?;
        let record = fornax_delivery_proof_from_row(row)?;
        if record.operation_id != proof.operation_id
            || record.request_hash != proof.request_hash
            || record.remote_trace_id != proof.remote_trace_id
            || record.remote_span_id != proof.remote_span_id
            || record.proof_kind != proof.proof_kind
            || record.proof_digest != proof.proof_digest
        {
            return Err(WorkflowStoreError::FornaxDeliveryProofConflict);
        }
        if result.rows_affected() == 1 {
            append_event(
                &mut tx,
                &proof.run_id,
                "fornax.delivery_reconciled",
                Some(&proof.operation_id),
                &json!({
                    "effect_key": proof.effect_key,
                    "request_hash": proof.request_hash,
                    "proof_kind": proof.proof_kind,
                    "proof_digest": proof.proof_digest,
                }),
                proof.reconciled_at_ms,
            )
            .await?;
        }
        tx.commit().await?;
        Ok(record)
    }

    pub async fn list_fornax_delivery_proofs(
        &self,
        run_id: &str,
    ) -> Result<Vec<WorkflowFornaxDeliveryProofRecord>, WorkflowStoreError> {
        let rows = sqlx::query(
            r#"
SELECT run_id, effect_key, operation_id, request_hash, remote_trace_id,
       remote_span_id, proof_kind, proof_digest, reconciled_at_ms
FROM workflow_fornax_delivery_proofs
WHERE run_id = ?
ORDER BY effect_key
            "#,
        )
        .bind(run_id)
        .fetch_all(self.pool())
        .await?;
        rows.into_iter()
            .map(fornax_delivery_proof_from_row)
            .collect()
    }

    /// Advances recovery state while making all observed identifiers immutable.
    pub async fn update_fornax_trace(
        &self,
        update: WorkflowFornaxTraceUpdate,
    ) -> Result<bool, WorkflowStoreError> {
        if !valid_fornax_transition(update.expected_state, update.state) {
            return Err(anyhow::anyhow!("invalid workflow Fornax trace state transition").into());
        }
        let mut tx = self.pool().begin().await?;
        let result = sqlx::query(
            r#"
UPDATE workflow_fornax_traces
SET state = ?,
    span_handle_id = COALESCE(span_handle_id, ?),
    trace_context_id = COALESCE(trace_context_id, ?),
    trace_id = COALESCE(trace_id, ?),
    span_id = COALESCE(span_id, ?),
    bridge_instance_id = COALESCE(bridge_instance_id, ?),
    error_code = ?,
    updated_at_ms = ?
WHERE run_id = ? AND effect_key = ? AND state = ?
  AND (span_handle_id IS NULL OR ? IS NULL OR span_handle_id = ?)
  AND (trace_context_id IS NULL OR ? IS NULL OR trace_context_id = ?)
  AND (trace_id IS NULL OR ? IS NULL OR trace_id = ?)
  AND (span_id IS NULL OR ? IS NULL OR span_id = ?)
  AND (bridge_instance_id IS NULL OR ? IS NULL OR bridge_instance_id = ?)
            "#,
        )
        .bind(update.state.as_str())
        .bind(&update.span_handle_id)
        .bind(&update.trace_context_id)
        .bind(&update.trace_id)
        .bind(&update.span_id)
        .bind(&update.bridge_instance_id)
        .bind(&update.error_code)
        .bind(update.updated_at_ms)
        .bind(&update.run_id)
        .bind(&update.effect_key)
        .bind(update.expected_state.as_str())
        .bind(&update.span_handle_id)
        .bind(&update.span_handle_id)
        .bind(&update.trace_context_id)
        .bind(&update.trace_context_id)
        .bind(&update.trace_id)
        .bind(&update.trace_id)
        .bind(&update.span_id)
        .bind(&update.span_id)
        .bind(&update.bridge_instance_id)
        .bind(&update.bridge_instance_id)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() != 1 {
            tx.rollback().await?;
            return Ok(false);
        }
        append_event(
            &mut tx,
            &update.run_id,
            "fornax.trace_updated",
            update.span_handle_id.as_deref(),
            &json!({
                "effect_key": update.effect_key,
                "state": update.state.as_str(),
                "error_code": update.error_code,
            }),
            update.updated_at_ms,
        )
        .await?;
        tx.commit().await?;
        Ok(true)
    }
}

fn valid_fornax_transition(from: WorkflowFornaxTraceState, to: WorkflowFornaxTraceState) -> bool {
    matches!(
        (from, to),
        (
            WorkflowFornaxTraceState::Planned,
            WorkflowFornaxTraceState::Live
                | WorkflowFornaxTraceState::Applied
                | WorkflowFornaxTraceState::Finished
                | WorkflowFornaxTraceState::Ambiguous
                | WorkflowFornaxTraceState::Failed
        ) | (
            WorkflowFornaxTraceState::Live,
            WorkflowFornaxTraceState::Finished
                | WorkflowFornaxTraceState::Orphaned
                | WorkflowFornaxTraceState::Ambiguous
                | WorkflowFornaxTraceState::Failed
        )
    )
}

fn fornax_trace_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<WorkflowFornaxTraceRecord, WorkflowStoreError> {
    Ok(WorkflowFornaxTraceRecord {
        run_id: row.try_get("run_id")?,
        effect_key: row.try_get("effect_key")?,
        operation_id: row.try_get("operation_id")?,
        request_hash: row.try_get("request_hash")?,
        span_handle_id: row.try_get("span_handle_id")?,
        trace_context_id: row.try_get("trace_context_id")?,
        trace_id: row.try_get("trace_id")?,
        span_id: row.try_get("span_id")?,
        bridge_instance_id: row.try_get("bridge_instance_id")?,
        state: WorkflowFornaxTraceState::parse(&row.try_get::<String, _>("state")?)?,
        error_code: row.try_get("error_code")?,
    })
}

fn fornax_delivery_proof_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<WorkflowFornaxDeliveryProofRecord, WorkflowStoreError> {
    Ok(WorkflowFornaxDeliveryProofRecord {
        run_id: row.try_get("run_id")?,
        effect_key: row.try_get("effect_key")?,
        operation_id: row.try_get("operation_id")?,
        request_hash: row.try_get("request_hash")?,
        remote_trace_id: row.try_get("remote_trace_id")?,
        remote_span_id: row.try_get("remote_span_id")?,
        proof_kind: row.try_get("proof_kind")?,
        proof_digest: row.try_get("proof_digest")?,
        reconciled_at_ms: row.try_get("reconciled_at_ms")?,
    })
}
