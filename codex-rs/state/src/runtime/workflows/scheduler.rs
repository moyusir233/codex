use serde_json::Value;
use serde_json::json;
use sqlx::Row;

use crate::WorkflowDependencyRecord;
use crate::WorkflowNodeAttemptRecord;
use crate::WorkflowNodeRecord;
use crate::WorkflowNodeStatus;
use crate::WorkflowNodeThreadRecord;

use super::WorkflowStore;
use super::WorkflowStoreError;
use super::attempts::attempt_from_row;
use super::events::append_event;
use super::nodes::node_from_row;
use super::runs::is_unique_violation;
use super::runs::to_i64;

/// CAS update for scheduler-owned node state.
pub struct WorkflowNodeTransition {
    pub status: WorkflowNodeStatus,
    pub retry_at_ms: Option<i64>,
    pub event_kind: String,
    pub event_metadata: Value,
    pub updated_at_ms: i64,
}

impl WorkflowStore {
    /// Adds one policy-bearing dependency edge after a durable cycle check.
    pub async fn add_node_dependency(
        &self,
        run_id: &str,
        node_id: &str,
        depends_on_node_id: &str,
        policy: &str,
        at_least: Option<u32>,
        created_at_ms: i64,
    ) -> Result<(), WorkflowStoreError> {
        if node_id == depends_on_node_id {
            return Err(WorkflowStoreError::DependencyCycle);
        }
        match policy {
            "all_succeeded" | "all_terminal" if at_least.is_none() => {}
            "at_least" if at_least.is_some_and(|value| value > 0) => {}
            _ => return Err(WorkflowStoreError::InvalidDependencyPolicy),
        }
        let mut tx = self.pool().begin().await?;
        let node_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM workflow_nodes WHERE run_id = ? AND node_id IN (?, ?)",
        )
        .bind(run_id)
        .bind(node_id)
        .bind(depends_on_node_id)
        .fetch_one(&mut *tx)
        .await?;
        if node_count != 2 {
            return Err(WorkflowStoreError::RunNotFound);
        }
        let closes_cycle: Option<i64> = sqlx::query_scalar(
            r#"
WITH RECURSIVE reachable(node_id) AS (
    SELECT ?
    UNION
    SELECT edge.node_id
    FROM workflow_node_dependencies AS edge
    JOIN reachable ON edge.depends_on_node_id = reachable.node_id
    WHERE edge.run_id = ?
)
SELECT 1 FROM reachable WHERE node_id = ? LIMIT 1
            "#,
        )
        .bind(node_id)
        .bind(run_id)
        .bind(depends_on_node_id)
        .fetch_optional(&mut *tx)
        .await?;
        if closes_cycle.is_some() {
            return Err(WorkflowStoreError::DependencyCycle);
        }
        let result = sqlx::query(
            r#"
INSERT INTO workflow_node_dependencies (
    run_id, node_id, depends_on_node_id, policy, at_least
) VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(run_id)
        .bind(node_id)
        .bind(depends_on_node_id)
        .bind(policy)
        .bind(at_least.map(i64::from))
        .execute(&mut *tx)
        .await;
        match result {
            Ok(_) => {}
            Err(error) if is_unique_violation(&error) => {
                return Err(WorkflowStoreError::DuplicateDependency);
            }
            Err(error) => return Err(error.into()),
        }
        append_event(
            &mut tx,
            run_id,
            "node.dependency_added",
            Some(node_id),
            &json!({
                "depends_on_node_id": depends_on_node_id,
                "policy": policy,
                "at_least": at_least,
            }),
            created_at_ms,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Lists dependency edges in stable downstream/upstream order.
    pub async fn list_node_dependencies(
        &self,
        run_id: &str,
    ) -> Result<Vec<WorkflowDependencyRecord>, WorkflowStoreError> {
        let rows = sqlx::query(
            r#"
SELECT run_id, node_id, depends_on_node_id, policy, at_least
FROM workflow_node_dependencies
WHERE run_id = ?
ORDER BY node_id, depends_on_node_id
            "#,
        )
        .bind(run_id)
        .fetch_all(self.pool())
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(WorkflowDependencyRecord {
                    run_id: row.try_get("run_id")?,
                    node_id: row.try_get("node_id")?,
                    depends_on_node_id: row.try_get("depends_on_node_id")?,
                    policy: row.try_get("policy")?,
                    at_least: row
                        .try_get::<Option<i64>, _>("at_least")?
                        .map(u32::try_from)
                        .transpose()
                        .map_err(|_| anyhow::anyhow!("dependency threshold is out of range"))?,
                })
            })
            .collect()
    }

    /// Advances one node with row-version fencing and an ordered event.
    pub async fn transition_node(
        &self,
        node_id: &str,
        expected_row_version: u64,
        transition: WorkflowNodeTransition,
    ) -> Result<WorkflowNodeRecord, WorkflowStoreError> {
        let mut tx = self.pool().begin().await?;
        let result = sqlx::query(
            r#"
UPDATE workflow_nodes
SET status = ?, retry_at_ms = ?, row_version = row_version + 1, updated_at_ms = ?
WHERE node_id = ? AND row_version = ?
            "#,
        )
        .bind(transition.status.as_str())
        .bind(transition.retry_at_ms)
        .bind(transition.updated_at_ms)
        .bind(node_id)
        .bind(to_i64(expected_row_version, "node row version")?)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() != 1 {
            return Err(WorkflowStoreError::StaleWrite);
        }
        let run_id: String =
            sqlx::query_scalar("SELECT run_id FROM workflow_nodes WHERE node_id = ?")
                .bind(node_id)
                .fetch_one(&mut *tx)
                .await?;
        append_event(
            &mut tx,
            &run_id,
            &transition.event_kind,
            Some(node_id),
            &transition.event_metadata,
            transition.updated_at_ms,
        )
        .await?;
        tx.commit().await?;
        self.read_node(node_id)
            .await?
            .ok_or(WorkflowStoreError::RunNotFound)
    }

    /// Lists immutable attempts for one node in attempt-number order.
    pub async fn list_node_attempts(
        &self,
        node_id: &str,
    ) -> Result<Vec<WorkflowNodeAttemptRecord>, WorkflowStoreError> {
        let rows = sqlx::query(
            r#"
SELECT attempt_id, run_id, node_id, attempt_number, submission_id,
       input_hash, thread_id, turn_id, status, started_at_ms, completed_at_ms,
       error_code, created_at_ms, updated_at_ms
FROM workflow_node_attempts
WHERE node_id = ?
ORDER BY attempt_number
            "#,
        )
        .bind(node_id)
        .fetch_all(self.pool())
        .await?;
        rows.into_iter().map(attempt_from_row).collect()
    }

    /// Appends another normal Codex thread reference for a fresh-thread retry.
    pub async fn append_node_thread(
        &self,
        node_id: &str,
        thread_id: &str,
        created_at_ms: i64,
    ) -> Result<WorkflowNodeThreadRecord, WorkflowStoreError> {
        let mut tx = self.pool().begin().await?;
        let result = sqlx::query(
            r#"
INSERT INTO workflow_node_threads (node_id, thread_id, ordinal, created_at_ms)
SELECT ?, ?, COALESCE(MAX(ordinal), 0) + 1, ?
FROM workflow_node_threads
WHERE node_id = ?
            "#,
        )
        .bind(node_id)
        .bind(thread_id)
        .bind(created_at_ms)
        .bind(node_id)
        .execute(&mut *tx)
        .await;
        match result {
            Ok(result) if result.rows_affected() == 1 => {}
            Ok(_) => return Err(WorkflowStoreError::RunNotFound),
            Err(error) if is_unique_violation(&error) => {
                return Err(WorkflowStoreError::DuplicateNode);
            }
            Err(error) => return Err(error.into()),
        }
        let run_id: String =
            sqlx::query_scalar("SELECT run_id FROM workflow_nodes WHERE node_id = ?")
                .bind(node_id)
                .fetch_one(&mut *tx)
                .await?;
        append_event(
            &mut tx,
            &run_id,
            "node.thread_appended",
            Some(node_id),
            &json!({ "thread_id": thread_id }),
            created_at_ms,
        )
        .await?;
        tx.commit().await?;
        self.list_node_threads(node_id)
            .await?
            .into_iter()
            .find(|record| record.thread_id == thread_id)
            .ok_or(WorkflowStoreError::RunNotFound)
    }

    /// Lists every resumable thread ever owned by a node.
    pub async fn list_node_threads(
        &self,
        node_id: &str,
    ) -> Result<Vec<WorkflowNodeThreadRecord>, WorkflowStoreError> {
        let rows = sqlx::query(
            r#"
SELECT node_id, thread_id, ordinal, created_at_ms
FROM workflow_node_threads
WHERE node_id = ?
ORDER BY ordinal
            "#,
        )
        .bind(node_id)
        .fetch_all(self.pool())
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(WorkflowNodeThreadRecord {
                    node_id: row.try_get("node_id")?,
                    thread_id: row.try_get("thread_id")?,
                    ordinal: u32::try_from(row.try_get::<i64, _>("ordinal")?)
                        .map_err(|_| anyhow::anyhow!("thread ordinal is out of range"))?,
                    created_at_ms: row.try_get("created_at_ms")?,
                })
            })
            .collect()
    }

    /// Lists all nodes using stable scheduler order.
    pub async fn list_nodes_for_scheduler(
        &self,
        run_id: &str,
    ) -> Result<Vec<WorkflowNodeRecord>, WorkflowStoreError> {
        let rows = sqlx::query(
            r#"
SELECT node_id, run_id, node_key, thread_id, spec_json, status,
       retry_at_ms, failure_policy, row_version, created_at_ms, updated_at_ms
FROM workflow_nodes
WHERE run_id = ?
ORDER BY updated_at_ms, node_key, node_id
            "#,
        )
        .bind(run_id)
        .fetch_all(self.pool())
        .await?;
        rows.into_iter().map(node_from_row).collect()
    }
}
