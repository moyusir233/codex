use serde_json::json;
use sqlx::Row;

use crate::WorkflowNodeAttemptCreate;
use crate::WorkflowNodeAttemptRecord;
use crate::WorkflowNodeAttemptStatus;
use crate::WorkflowNodeAttemptTransition;

use super::WorkflowStore;
use super::WorkflowStoreError;
use super::events::append_event;
use super::runs::is_unique_violation;

impl WorkflowStore {
    /// Appends a prepared attempt without overwriting any prior attempt evidence.
    pub async fn create_node_attempt(
        &self,
        run_id: &str,
        create: WorkflowNodeAttemptCreate,
    ) -> Result<WorkflowNodeAttemptRecord, WorkflowStoreError> {
        let mut tx = self.pool().begin().await?;
        let result = sqlx::query(
            r#"
INSERT INTO workflow_node_attempts (
    attempt_id, run_id, node_id, attempt_number, submission_id,
    input_hash, status, created_at_ms, updated_at_ms
)
SELECT ?, ?, node_id,
       COALESCE((
           SELECT MAX(existing.attempt_number)
           FROM workflow_node_attempts AS existing
           WHERE existing.node_id = workflow_nodes.node_id
       ), 0) + 1,
       ?, ?, 'planned', ?, ?
FROM workflow_nodes
WHERE node_id = ? AND run_id = ?
            "#,
        )
        .bind(&create.attempt_id)
        .bind(run_id)
        .bind(&create.submission_id)
        .bind(&create.input_hash)
        .bind(create.created_at_ms)
        .bind(create.created_at_ms)
        .bind(&create.node_id)
        .bind(run_id)
        .execute(&mut *tx)
        .await;
        let result = match result {
            Ok(result) => result,
            Err(error) if is_unique_violation(&error) => {
                return Err(WorkflowStoreError::DuplicateAttempt);
            }
            Err(error) => return Err(error.into()),
        };
        if result.rows_affected() != 1 {
            return Err(WorkflowStoreError::RunNotFound);
        }
        append_event(
            &mut tx,
            run_id,
            "node.attempt_planned",
            Some(&create.attempt_id),
            &json!({
                "node_id": create.node_id,
                "submission_id": create.submission_id,
            }),
            create.created_at_ms,
        )
        .await?;
        tx.commit().await?;
        self.read_node_attempt(&create.attempt_id)
            .await?
            .ok_or(WorkflowStoreError::RunNotFound)
    }

    /// Reads one node attempt by its stable attempt ID.
    pub async fn read_node_attempt(
        &self,
        attempt_id: &str,
    ) -> Result<Option<WorkflowNodeAttemptRecord>, WorkflowStoreError> {
        self.read_node_attempt_where(
            r#"
SELECT attempt_id, run_id, node_id, attempt_number, submission_id,
       input_hash, turn_id, status, started_at_ms, completed_at_ms,
       error_code, created_at_ms, updated_at_ms
FROM workflow_node_attempts
WHERE attempt_id = ?
            "#,
            attempt_id,
        )
        .await
    }

    /// Reads the attempt associated with a prepared-submission boundary.
    pub async fn read_node_attempt_by_submission_id(
        &self,
        submission_id: &str,
    ) -> Result<Option<WorkflowNodeAttemptRecord>, WorkflowStoreError> {
        self.read_node_attempt_where(
            r#"
SELECT attempt_id, run_id, node_id, attempt_number, submission_id,
       input_hash, turn_id, status, started_at_ms, completed_at_ms,
       error_code, created_at_ms, updated_at_ms
FROM workflow_node_attempts
WHERE submission_id = ?
            "#,
            submission_id,
        )
        .await
    }

    /// Reads the attempt associated with a normal Codex turn.
    pub async fn read_node_attempt_by_turn_id(
        &self,
        node_id: &str,
        turn_id: &str,
    ) -> Result<Option<WorkflowNodeAttemptRecord>, WorkflowStoreError> {
        let row = sqlx::query(
            r#"
SELECT attempt_id, run_id, node_id, attempt_number, submission_id,
       input_hash, turn_id, status, started_at_ms, completed_at_ms,
       error_code, created_at_ms, updated_at_ms
FROM workflow_node_attempts
WHERE node_id = ? AND turn_id = ?
            "#,
        )
        .bind(node_id)
        .bind(turn_id)
        .fetch_optional(self.pool())
        .await?;
        row.map(attempt_from_row).transpose()
    }

    /// Advances one attempt and records the transition event in the same transaction.
    pub async fn transition_node_attempt(
        &self,
        attempt_id: &str,
        transition: WorkflowNodeAttemptTransition,
    ) -> Result<WorkflowNodeAttemptRecord, WorkflowStoreError> {
        if !valid_attempt_transition(transition.expected_status, transition.status) {
            return Err(anyhow::anyhow!(
                "invalid workflow node attempt transition from {} to {}",
                transition.expected_status.as_str(),
                transition.status.as_str()
            )
            .into());
        }
        let mut tx = self.pool().begin().await?;
        let terminal_at = transition
            .status
            .is_terminal()
            .then_some(transition.updated_at_ms);
        let result = sqlx::query(
            r#"
UPDATE workflow_node_attempts
SET status = ?,
    turn_id = COALESCE(?, turn_id),
    started_at_ms = CASE
        WHEN ? IN ('submitted', 'running') THEN COALESCE(started_at_ms, ?)
        ELSE started_at_ms
    END,
    completed_at_ms = COALESCE(?, completed_at_ms),
    error_code = ?,
    updated_at_ms = ?
WHERE attempt_id = ? AND status = ?
            "#,
        )
        .bind(transition.status.as_str())
        .bind(&transition.turn_id)
        .bind(transition.status.as_str())
        .bind(transition.updated_at_ms)
        .bind(terminal_at)
        .bind(&transition.error_code)
        .bind(transition.updated_at_ms)
        .bind(attempt_id)
        .bind(transition.expected_status.as_str())
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() != 1 {
            return Err(WorkflowStoreError::StaleWrite);
        }
        let run_id: String =
            sqlx::query_scalar("SELECT run_id FROM workflow_node_attempts WHERE attempt_id = ?")
                .bind(attempt_id)
                .fetch_one(&mut *tx)
                .await?;
        append_event(
            &mut tx,
            &run_id,
            "node.attempt_transitioned",
            Some(attempt_id),
            &json!({ "status": transition.status.as_str() }),
            transition.updated_at_ms,
        )
        .await?;
        tx.commit().await?;
        self.read_node_attempt(attempt_id)
            .await?
            .ok_or(WorkflowStoreError::RunNotFound)
    }

    async fn read_node_attempt_where(
        &self,
        sql: &'static str,
        value: &str,
    ) -> Result<Option<WorkflowNodeAttemptRecord>, WorkflowStoreError> {
        let row = sqlx::query(sql)
            .bind(value)
            .fetch_optional(self.pool())
            .await?;
        row.map(attempt_from_row).transpose()
    }
}

fn valid_attempt_transition(
    from: WorkflowNodeAttemptStatus,
    to: WorkflowNodeAttemptStatus,
) -> bool {
    match from {
        WorkflowNodeAttemptStatus::Planned => matches!(
            to,
            WorkflowNodeAttemptStatus::Submitted
                | WorkflowNodeAttemptStatus::Cancelled
                | WorkflowNodeAttemptStatus::Ambiguous
        ),
        WorkflowNodeAttemptStatus::Submitted => matches!(
            to,
            WorkflowNodeAttemptStatus::Running
                | WorkflowNodeAttemptStatus::Succeeded
                | WorkflowNodeAttemptStatus::Failed
                | WorkflowNodeAttemptStatus::Interrupted
                | WorkflowNodeAttemptStatus::Cancelled
                | WorkflowNodeAttemptStatus::Ambiguous
        ),
        WorkflowNodeAttemptStatus::Running => matches!(
            to,
            WorkflowNodeAttemptStatus::Succeeded
                | WorkflowNodeAttemptStatus::Failed
                | WorkflowNodeAttemptStatus::Interrupted
                | WorkflowNodeAttemptStatus::Cancelled
                | WorkflowNodeAttemptStatus::Ambiguous
        ),
        WorkflowNodeAttemptStatus::Succeeded
        | WorkflowNodeAttemptStatus::Failed
        | WorkflowNodeAttemptStatus::Interrupted
        | WorkflowNodeAttemptStatus::Cancelled
        | WorkflowNodeAttemptStatus::Ambiguous => false,
    }
}

fn attempt_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<WorkflowNodeAttemptRecord, WorkflowStoreError> {
    Ok(WorkflowNodeAttemptRecord {
        attempt_id: row.try_get("attempt_id")?,
        run_id: row.try_get("run_id")?,
        node_id: row.try_get("node_id")?,
        attempt_number: u32::try_from(row.try_get::<i64, _>("attempt_number")?)
            .map_err(|_| anyhow::anyhow!("attempt number is out of range"))?,
        submission_id: row.try_get("submission_id")?,
        input_hash: row.try_get("input_hash")?,
        turn_id: row.try_get("turn_id")?,
        status: WorkflowNodeAttemptStatus::parse(&row.try_get::<String, _>("status")?)?,
        started_at_ms: row.try_get("started_at_ms")?,
        completed_at_ms: row.try_get("completed_at_ms")?,
        error_code: row.try_get("error_code")?,
        created_at_ms: row.try_get("created_at_ms")?,
        updated_at_ms: row.try_get("updated_at_ms")?,
    })
}
