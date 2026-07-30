use serde_json::json;
use sqlx::Row;

use crate::WorkflowNodeCreate;
use crate::WorkflowNodeRecord;
use crate::WorkflowNodeStatus;

use super::WorkflowStore;
use super::WorkflowStoreError;
use super::events::append_event;
use super::runs::is_unique_violation;
use super::runs::to_u64;

impl WorkflowStore {
    /// Adds one node and its dependency edges to a run graph atomically.
    pub async fn create_node(
        &self,
        run_id: &str,
        create: WorkflowNodeCreate,
        dependency_node_ids: &[String],
    ) -> Result<WorkflowNodeRecord, WorkflowStoreError> {
        let mut tx = self.pool().begin().await?;
        let result = sqlx::query(
            r#"
INSERT INTO workflow_nodes (
    node_id, run_id, node_key, spec_json, status,
    row_version, created_at_ms, updated_at_ms
) VALUES (?, ?, ?, ?, ?, 1, ?, ?)
            "#,
        )
        .bind(&create.node_id)
        .bind(run_id)
        .bind(&create.node_key)
        .bind(serde_json::to_string(&create.spec)?)
        .bind(create.status.as_str())
        .bind(create.created_at_ms)
        .bind(create.created_at_ms)
        .execute(&mut *tx)
        .await;
        match result {
            Ok(_) => {}
            Err(err) if is_unique_violation(&err) => {
                return Err(WorkflowStoreError::DuplicateNode);
            }
            Err(err) => return Err(err.into()),
        }
        for dependency_node_id in dependency_node_ids {
            let result = sqlx::query(
                r#"
INSERT INTO workflow_node_dependencies (
    run_id, node_id, depends_on_node_id
)
SELECT ?, ?, node_id
FROM workflow_nodes
WHERE node_id = ? AND run_id = ?
                "#,
            )
            .bind(run_id)
            .bind(&create.node_id)
            .bind(dependency_node_id)
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
            if result.rows_affected() != 1 {
                return Err(
                    anyhow::anyhow!("workflow dependency does not belong to the run").into(),
                );
            }
        }
        append_event(
            &mut tx,
            run_id,
            "node.created",
            Some(&create.node_id),
            &json!({
                "node_key": create.node_key,
                "dependency_count": dependency_node_ids.len(),
            }),
            create.created_at_ms,
        )
        .await?;
        tx.commit().await?;
        self.read_node(&create.node_id)
            .await?
            .ok_or(WorkflowStoreError::RunNotFound)
    }

    /// Reads one workflow node snapshot.
    pub async fn read_node(
        &self,
        node_id: &str,
    ) -> Result<Option<WorkflowNodeRecord>, WorkflowStoreError> {
        let row = sqlx::query(
            r#"
SELECT node_id, run_id, node_key, thread_id, spec_json, status,
       row_version, created_at_ms, updated_at_ms
FROM workflow_nodes
WHERE node_id = ?
            "#,
        )
        .bind(node_id)
        .fetch_optional(self.pool())
        .await?;
        row.map(node_from_row).transpose()
    }

    /// Reads the workflow node bound to a durable Codex thread.
    pub async fn read_node_by_thread_id(
        &self,
        thread_id: &str,
    ) -> Result<Option<WorkflowNodeRecord>, WorkflowStoreError> {
        let row = sqlx::query(
            r#"
SELECT node_id, run_id, node_key, thread_id, spec_json, status,
       row_version, created_at_ms, updated_at_ms
FROM workflow_nodes
WHERE thread_id = ?
            "#,
        )
        .bind(thread_id)
        .fetch_optional(self.pool())
        .await?;
        row.map(node_from_row).transpose()
    }

    /// Binds a newly materialized Codex thread to a workflow node exactly once.
    pub async fn bind_node_thread(
        &self,
        node_id: &str,
        expected_row_version: u64,
        thread_id: &str,
        updated_at_ms: i64,
    ) -> Result<WorkflowNodeRecord, WorkflowStoreError> {
        let mut tx = self.pool().begin().await?;
        let expected_row_version = i64::try_from(expected_row_version)
            .map_err(|_| anyhow::anyhow!("node row version exceeds SQLite range"))?;
        let result = sqlx::query(
            r#"
UPDATE workflow_nodes
SET thread_id = ?, row_version = row_version + 1, updated_at_ms = ?
WHERE node_id = ? AND row_version = ? AND thread_id IS NULL
            "#,
        )
        .bind(thread_id)
        .bind(updated_at_ms)
        .bind(node_id)
        .bind(expected_row_version)
        .execute(&mut *tx)
        .await;
        let result = match result {
            Ok(result) => result,
            Err(err) if is_unique_violation(&err) => {
                return Err(WorkflowStoreError::DuplicateNode);
            }
            Err(err) => return Err(err.into()),
        };
        if result.rows_affected() != 1 {
            tx.rollback().await?;
            let existing = self.read_node(node_id).await?;
            if existing.as_ref().and_then(|node| node.thread_id.as_deref()) == Some(thread_id) {
                return existing.ok_or(WorkflowStoreError::RunNotFound);
            }
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
            "node.thread_bound",
            Some(node_id),
            &json!({ "thread_id": thread_id }),
            updated_at_ms,
        )
        .await?;
        tx.commit().await?;
        self.read_node(node_id)
            .await?
            .ok_or(WorkflowStoreError::RunNotFound)
    }

    /// Lists the nodes in stable node-key order.
    pub async fn list_nodes(
        &self,
        run_id: &str,
    ) -> Result<Vec<WorkflowNodeRecord>, WorkflowStoreError> {
        let rows = sqlx::query(
            r#"
SELECT node_id, run_id, node_key, thread_id, spec_json, status,
       row_version, created_at_ms, updated_at_ms
FROM workflow_nodes
WHERE run_id = ?
ORDER BY node_key, node_id
            "#,
        )
        .bind(run_id)
        .fetch_all(self.pool())
        .await?;
        rows.into_iter().map(node_from_row).collect()
    }
}

fn node_from_row(row: sqlx::sqlite::SqliteRow) -> Result<WorkflowNodeRecord, WorkflowStoreError> {
    Ok(WorkflowNodeRecord {
        node_id: row.try_get("node_id")?,
        run_id: row.try_get("run_id")?,
        node_key: row.try_get("node_key")?,
        thread_id: row.try_get("thread_id")?,
        spec: serde_json::from_str(&row.try_get::<String, _>("spec_json")?)?,
        status: WorkflowNodeStatus::parse(&row.try_get::<String, _>("status")?)?,
        row_version: to_u64(row.try_get::<i64, _>("row_version")?, "node row version")?,
        created_at_ms: row.try_get("created_at_ms")?,
        updated_at_ms: row.try_get("updated_at_ms")?,
    })
}
