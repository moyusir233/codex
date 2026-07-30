use serde_json::Value;
use serde_json::json;

use crate::WorkflowRunRecord;

use super::WorkflowStore;
use super::WorkflowStoreError;
use super::events::append_event;
use super::runs::run_from_row;

impl WorkflowStore {
    /// Requeues an operator-resumable run and records the explicit action.
    pub async fn resume_run(
        &self,
        run_id: &str,
        resumed_at_ms: i64,
    ) -> Result<WorkflowRunRecord, WorkflowStoreError> {
        let mut tx = self.pool().begin().await?;
        let result = sqlx::query(
            r#"
UPDATE workflow_runs
SET status = 'pending', error_code = NULL, wake_json = NULL,
    row_version = row_version + 1, updated_at_ms = ?
WHERE run_id = ? AND status IN ('waiting', 'needs_operator')
            "#,
        )
        .bind(resumed_at_ms)
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 1 {
            append_event(
                &mut tx,
                run_id,
                "run.resumed",
                Some(run_id),
                &json!({}),
                resumed_at_ms,
            )
            .await?;
            tx.commit().await?;
        } else {
            tx.rollback().await?;
            return match self.read_run(run_id).await? {
                Some(_) => Err(WorkflowStoreError::StaleWrite),
                None => Err(WorkflowStoreError::RunNotFound),
            };
        }
        self.read_run(run_id)
            .await?
            .ok_or(WorkflowStoreError::RunNotFound)
    }

    /// Durably requests cooperative cancellation exactly once.
    pub async fn request_run_cancellation(
        &self,
        run_id: &str,
        requested_at_ms: i64,
    ) -> Result<WorkflowRunRecord, WorkflowStoreError> {
        let mut tx = self.pool().begin().await?;
        let result = sqlx::query(
            r#"
UPDATE workflow_runs
SET status = 'cancelling',
    cancellation_requested_at_ms = COALESCE(cancellation_requested_at_ms, ?),
    row_version = row_version + 1,
    updated_at_ms = ?
WHERE run_id = ?
  AND status IN ('pending', 'running', 'waiting', 'cancelling')
  AND cancellation_requested_at_ms IS NULL
            "#,
        )
        .bind(requested_at_ms)
        .bind(requested_at_ms)
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 1 {
            append_event(
                &mut tx,
                run_id,
                "run.cancellation_requested",
                Some(run_id),
                &json!({}),
                requested_at_ms,
            )
            .await?;
            tx.commit().await?;
        } else {
            tx.rollback().await?;
        }
        self.read_run(run_id)
            .await?
            .ok_or(WorkflowStoreError::RunNotFound)
    }

    /// Marks a cancelling run and every remaining nonterminal node cancelled.
    pub async fn finish_run_cancellation(
        &self,
        run_id: &str,
        completed_at_ms: i64,
    ) -> Result<WorkflowRunRecord, WorkflowStoreError> {
        let mut tx = self.pool().begin().await?;
        sqlx::query(
            r#"
UPDATE workflow_nodes
SET status = 'cancelled', retry_at_ms = NULL,
    row_version = row_version + 1, updated_at_ms = ?
WHERE run_id = ? AND status IN ('pending', 'ready', 'running', 'waiting', 'blocked')
            "#,
        )
        .bind(completed_at_ms)
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
        let result = sqlx::query(
            r#"
UPDATE workflow_runs
SET status = 'cancelled', wake_json = NULL,
    row_version = row_version + 1, updated_at_ms = ?
WHERE run_id = ? AND status = 'cancelling'
            "#,
        )
        .bind(completed_at_ms)
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() != 1 {
            return Err(WorkflowStoreError::StaleWrite);
        }
        append_event(
            &mut tx,
            run_id,
            "run.cancelled",
            Some(run_id),
            &json!({}),
            completed_at_ms,
        )
        .await?;
        tx.commit().await?;
        self.read_run(run_id)
            .await?
            .ok_or(WorkflowStoreError::RunNotFound)
    }

    /// Moves a run to visible operator ownership after an unsafe observation.
    pub async fn mark_run_needs_operator(
        &self,
        run_id: &str,
        node_id: Option<&str>,
        error_code: &str,
        metadata: Value,
        observed_at_ms: i64,
    ) -> Result<WorkflowRunRecord, WorkflowStoreError> {
        let mut tx = self.pool().begin().await?;
        let result = sqlx::query(
            r#"
UPDATE workflow_runs
SET status = 'needs_operator', error_code = ?, wake_json = NULL,
    row_version = row_version + 1, updated_at_ms = ?
WHERE run_id = ? AND status IN ('pending', 'running', 'waiting', 'cancelling')
            "#,
        )
        .bind(error_code)
        .bind(observed_at_ms)
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() != 1 {
            return Err(WorkflowStoreError::StaleWrite);
        }
        if let Some(node_id) = node_id {
            sqlx::query(
                r#"
UPDATE workflow_nodes
SET status = 'waiting', row_version = row_version + 1, updated_at_ms = ?
WHERE run_id = ? AND node_id = ?
  AND status IN ('pending', 'ready', 'running', 'blocked')
                "#,
            )
            .bind(observed_at_ms)
            .bind(run_id)
            .bind(node_id)
            .execute(&mut *tx)
            .await?;
        }
        append_event(
            &mut tx,
            run_id,
            "run.needs_operator",
            node_id,
            &metadata,
            observed_at_ms,
        )
        .await?;
        tx.commit().await?;
        self.read_run(run_id)
            .await?
            .ok_or(WorkflowStoreError::RunNotFound)
    }

    /// Lists nonterminal runs whose lease is absent or expired.
    pub async fn list_recoverable_runs(
        &self,
        now_ms: i64,
    ) -> Result<Vec<WorkflowRunRecord>, WorkflowStoreError> {
        let rows = sqlx::query(
            r#"
SELECT run_id, definition_name, definition_version, state_schema_version,
       state_json, arguments_json, status, output_json, error_code,
       wake_json, cancellation_requested_at_ms, deadline_ms,
       row_version, next_sequence, lease_owner, lease_expires_at_ms,
       lease_fence, created_at_ms, updated_at_ms
FROM workflow_runs
WHERE status IN ('pending', 'running', 'waiting', 'cancelling')
  AND (lease_expires_at_ms IS NULL OR lease_expires_at_ms <= ?)
ORDER BY updated_at_ms, run_id
            "#,
        )
        .bind(now_ms)
        .fetch_all(self.pool())
        .await?;
        rows.into_iter().map(run_from_row).collect()
    }

    /// Returns whether cancellation has been durably requested.
    pub async fn run_cancellation_requested(
        &self,
        run_id: &str,
    ) -> Result<bool, WorkflowStoreError> {
        let value: Option<i64> = sqlx::query_scalar(
            "SELECT cancellation_requested_at_ms FROM workflow_runs WHERE run_id = ?",
        )
        .bind(run_id)
        .fetch_optional(self.pool())
        .await?
        .ok_or(WorkflowStoreError::RunNotFound)?;
        Ok(value.is_some())
    }
}
