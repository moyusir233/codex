use serde_json::Value;
use sqlx::Row;
use sqlx::Sqlite;
use sqlx::Transaction;

use crate::WorkflowEventRecord;

use super::WorkflowStore;
use super::WorkflowStoreError;
use super::runs::to_i64;
use super::runs::to_u64;

impl WorkflowStore {
    /// Reads durable events strictly after a sequence in ascending order.
    pub async fn events_after(
        &self,
        run_id: &str,
        after_sequence: u64,
        limit: u32,
    ) -> Result<Vec<WorkflowEventRecord>, WorkflowStoreError> {
        let rows = sqlx::query(
            r#"
SELECT run_id, sequence, kind, entity_id, metadata_json, created_at_ms
FROM workflow_events
WHERE run_id = ? AND sequence > ?
ORDER BY sequence ASC
LIMIT ?
            "#,
        )
        .bind(run_id)
        .bind(to_i64(after_sequence, "event sequence")?)
        .bind(i64::from(limit.clamp(1, 1_000)))
        .fetch_all(self.pool())
        .await?;
        rows.into_iter().map(event_from_row).collect()
    }
}

pub(super) async fn append_event(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: &str,
    kind: &str,
    entity_id: Option<&str>,
    metadata: &Value,
    created_at_ms: i64,
) -> Result<WorkflowEventRecord, WorkflowStoreError> {
    let row = sqlx::query(
        r#"
UPDATE workflow_runs
SET next_sequence = next_sequence + 1,
    updated_at_ms = MAX(updated_at_ms, ?)
WHERE run_id = ?
RETURNING next_sequence - 1 AS sequence
        "#,
    )
    .bind(created_at_ms)
    .bind(run_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(WorkflowStoreError::RunNotFound)?;
    let sequence = to_u64(row.try_get::<i64, _>("sequence")?, "event sequence")?;
    sqlx::query(
        r#"
INSERT INTO workflow_events (
    run_id, sequence, kind, entity_id, metadata_json, created_at_ms
) VALUES (?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(run_id)
    .bind(to_i64(sequence, "event sequence")?)
    .bind(kind)
    .bind(entity_id)
    .bind(serde_json::to_string(metadata)?)
    .bind(created_at_ms)
    .execute(&mut **tx)
    .await?;
    Ok(WorkflowEventRecord {
        run_id: run_id.to_string(),
        sequence,
        kind: kind.to_string(),
        entity_id: entity_id.map(str::to_string),
        metadata: metadata.clone(),
        created_at_ms,
    })
}

fn event_from_row(row: sqlx::sqlite::SqliteRow) -> Result<WorkflowEventRecord, WorkflowStoreError> {
    Ok(WorkflowEventRecord {
        run_id: row.try_get("run_id")?,
        sequence: to_u64(row.try_get::<i64, _>("sequence")?, "event sequence")?,
        kind: row.try_get("kind")?,
        entity_id: row.try_get("entity_id")?,
        metadata: serde_json::from_str(&row.try_get::<String, _>("metadata_json")?)?,
        created_at_ms: row.try_get("created_at_ms")?,
    })
}
