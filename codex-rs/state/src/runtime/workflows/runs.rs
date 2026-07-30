use std::sync::Arc;

use serde_json::Value;
use sqlx::Row;
use sqlx::SqlitePool;

use crate::WorkflowEventRecord;
use crate::WorkflowRunCreate;
use crate::WorkflowRunRecord;
use crate::WorkflowRunStatus;

/// Optimistic-concurrency or validation failure from the workflow store.
#[derive(Debug, thiserror::Error)]
pub enum WorkflowStoreError {
    #[error("workflow run already exists")]
    DuplicateRun,
    #[error("workflow run was not found")]
    RunNotFound,
    #[error("workflow run changed or the lease fence is stale")]
    StaleWrite,
    #[error("workflow effect key was reused with a different request")]
    EffectConflict,
    #[error("workflow Fornax correlation was reused with different identifiers")]
    FornaxCorrelationConflict,
    #[error("workflow interaction dedupe key was reused with a different request")]
    InteractionConflict,
    #[error("workflow node key or identifier already exists")]
    DuplicateNode,
    #[error("workflow node attempt identifier or submission already exists")]
    DuplicateAttempt,
    #[error("workflow dependency already exists")]
    DuplicateDependency,
    #[error("workflow dependency would create a cycle")]
    DependencyCycle,
    #[error("workflow dependency policy is invalid")]
    InvalidDependencyPolicy,
    #[error("workflow artifact identifier or path already exists")]
    DuplicateArtifact,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    InvalidData(#[from] anyhow::Error),
}

/// One fenced run transition and its redacted durable event.
pub struct WorkflowRunTransition {
    pub status: WorkflowRunStatus,
    pub state_schema_version: u32,
    pub state: Value,
    pub output: Option<Value>,
    pub error_code: Option<String>,
    pub wake: Option<Value>,
    pub event_kind: String,
    pub event_entity_id: Option<String>,
    pub event_metadata: Value,
    pub updated_at_ms: i64,
}

/// Process-owned access to the dedicated workflow control-plane database.
#[derive(Clone)]
pub struct WorkflowStore {
    pool: Arc<SqlitePool>,
}

impl WorkflowStore {
    pub(crate) fn new(pool: Arc<SqlitePool>) -> Self {
        Self { pool }
    }

    /// Creates a run and its sequence-one `run.created` event atomically.
    pub async fn create_run(
        &self,
        create: WorkflowRunCreate,
    ) -> Result<WorkflowRunRecord, WorkflowStoreError> {
        let mut tx = self.pool.begin().await?;
        let result = sqlx::query(
            r#"
INSERT INTO workflow_runs (
    run_id, definition_name, definition_version, state_schema_version,
    state_json, arguments_json, non_interactive, detached, concurrency,
    status, row_version, next_sequence,
    lease_fence, created_at_ms, updated_at_ms
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending', 1, 2, 0, ?, ?)
            "#,
        )
        .bind(&create.run_id)
        .bind(&create.definition_name)
        .bind(&create.definition_version)
        .bind(i64::from(create.state_schema_version))
        .bind(serde_json::to_string(&create.state)?)
        .bind(serde_json::to_string(&create.arguments)?)
        .bind(create.non_interactive)
        .bind(create.detached)
        .bind(create.concurrency.map(i64::from))
        .bind(create.created_at_ms)
        .bind(create.created_at_ms)
        .execute(&mut *tx)
        .await;
        match result {
            Ok(_) => {}
            Err(err) if is_unique_violation(&err) => {
                return Err(WorkflowStoreError::DuplicateRun);
            }
            Err(err) => return Err(err.into()),
        }
        sqlx::query(
            r#"
INSERT INTO workflow_events (
    run_id, sequence, kind, entity_id, metadata_json, created_at_ms
) VALUES (?, 1, 'run.created', ?, '{}', ?)
            "#,
        )
        .bind(&create.run_id)
        .bind(&create.run_id)
        .bind(create.created_at_ms)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.read_run(&create.run_id)
            .await?
            .ok_or(WorkflowStoreError::RunNotFound)
    }

    /// Reads one durable run snapshot.
    pub async fn read_run(
        &self,
        run_id: &str,
    ) -> Result<Option<WorkflowRunRecord>, WorkflowStoreError> {
        let row = sqlx::query(
            r#"
SELECT run_id, definition_name, definition_version, state_schema_version,
       state_json, arguments_json, non_interactive, detached, concurrency,
       status, output_json, error_code,
       wake_json, cancellation_requested_at_ms, deadline_ms,
       row_version, next_sequence, lease_owner, lease_expires_at_ms,
       lease_fence, created_at_ms, updated_at_ms
FROM workflow_runs
WHERE run_id = ?
            "#,
        )
        .bind(run_id)
        .fetch_optional(self.pool.as_ref())
        .await?;
        row.map(run_from_row).transpose()
    }

    /// Lists run snapshots in newest-first order after an optional run cursor.
    pub async fn list_runs(
        &self,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<Vec<WorkflowRunRecord>, WorkflowStoreError> {
        let rows = sqlx::query(
            r#"
SELECT run_id, definition_name, definition_version, state_schema_version,
       state_json, arguments_json, non_interactive, detached, concurrency,
       status, output_json, error_code,
       wake_json, cancellation_requested_at_ms, deadline_ms,
       row_version, next_sequence, lease_owner, lease_expires_at_ms,
       lease_fence, created_at_ms, updated_at_ms
FROM workflow_runs
WHERE ? IS NULL
   OR (created_at_ms, run_id) < (
       SELECT created_at_ms, run_id FROM workflow_runs WHERE run_id = ?
   )
ORDER BY created_at_ms DESC, run_id DESC
LIMIT ?
            "#,
        )
        .bind(cursor)
        .bind(cursor)
        .bind(i64::from(limit.clamp(1, 1_000)))
        .fetch_all(self.pool.as_ref())
        .await?;
        rows.into_iter().map(run_from_row).collect()
    }

    /// Applies one fenced CAS transition and appends its event in the same transaction.
    pub async fn transition_run(
        &self,
        run_id: &str,
        lease_owner: &str,
        lease_fence: u64,
        expected_row_version: u64,
        transition: WorkflowRunTransition,
    ) -> Result<WorkflowEventRecord, WorkflowStoreError> {
        let mut tx = self.pool.begin().await?;
        let output_json = transition
            .output
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let row = sqlx::query(
            r#"
UPDATE workflow_runs
SET status = ?,
    state_schema_version = ?,
    state_json = ?,
    output_json = ?,
    error_code = ?,
    wake_json = ?,
    row_version = row_version + 1,
    next_sequence = next_sequence + 1,
    updated_at_ms = ?
WHERE run_id = ?
  AND row_version = ?
  AND lease_owner = ?
  AND lease_fence = ?
  AND lease_expires_at_ms > ?
RETURNING next_sequence - 1 AS sequence
            "#,
        )
        .bind(transition.status.as_str())
        .bind(i64::from(transition.state_schema_version))
        .bind(serde_json::to_string(&transition.state)?)
        .bind(output_json)
        .bind(&transition.error_code)
        .bind(
            transition
                .wake
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
        )
        .bind(transition.updated_at_ms)
        .bind(run_id)
        .bind(to_i64(expected_row_version, "row version")?)
        .bind(lease_owner)
        .bind(to_i64(lease_fence, "lease fence")?)
        .bind(transition.updated_at_ms)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            tx.rollback().await?;
            return Err(WorkflowStoreError::StaleWrite);
        };
        let sequence = to_u64(row.try_get::<i64, _>("sequence")?, "event sequence")?;
        let metadata_json = serde_json::to_string(&transition.event_metadata)?;
        sqlx::query(
            r#"
INSERT INTO workflow_events (
    run_id, sequence, kind, entity_id, metadata_json, created_at_ms
) VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(run_id)
        .bind(to_i64(sequence, "event sequence")?)
        .bind(&transition.event_kind)
        .bind(&transition.event_entity_id)
        .bind(metadata_json)
        .bind(transition.updated_at_ms)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(WorkflowEventRecord {
            run_id: run_id.to_string(),
            sequence,
            kind: transition.event_kind,
            entity_id: transition.event_entity_id,
            metadata: transition.event_metadata,
            created_at_ms: transition.updated_at_ms,
        })
    }

    pub(crate) fn pool(&self) -> &SqlitePool {
        self.pool.as_ref()
    }

    pub(crate) async fn close(&self) {
        self.pool.close().await;
    }
}

pub(super) fn run_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<WorkflowRunRecord, WorkflowStoreError> {
    let output_json = row.try_get::<Option<String>, _>("output_json")?;
    let wake_json = row.try_get::<Option<String>, _>("wake_json")?;
    Ok(WorkflowRunRecord {
        run_id: row.try_get("run_id")?,
        definition_name: row.try_get("definition_name")?,
        definition_version: row.try_get("definition_version")?,
        state_schema_version: to_u32(
            row.try_get::<i64, _>("state_schema_version")?,
            "state schema version",
        )?,
        state: serde_json::from_str(&row.try_get::<String, _>("state_json")?)?,
        arguments: serde_json::from_str(&row.try_get::<String, _>("arguments_json")?)?,
        non_interactive: row.try_get("non_interactive")?,
        detached: row.try_get("detached")?,
        concurrency: row
            .try_get::<Option<i64>, _>("concurrency")?
            .map(|value| to_u32(value, "workflow concurrency"))
            .transpose()?,
        status: WorkflowRunStatus::parse(&row.try_get::<String, _>("status")?)?,
        output: output_json
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
        error_code: row.try_get("error_code")?,
        wake: wake_json
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
        cancellation_requested_at_ms: row.try_get("cancellation_requested_at_ms")?,
        deadline_ms: row.try_get("deadline_ms")?,
        row_version: to_u64(row.try_get::<i64, _>("row_version")?, "row version")?,
        next_sequence: to_u64(row.try_get::<i64, _>("next_sequence")?, "next sequence")?,
        lease_owner: row.try_get("lease_owner")?,
        lease_expires_at_ms: row.try_get("lease_expires_at_ms")?,
        lease_fence: to_u64(row.try_get::<i64, _>("lease_fence")?, "lease fence")?,
        created_at_ms: row.try_get("created_at_ms")?,
        updated_at_ms: row.try_get("updated_at_ms")?,
    })
}

pub(super) fn to_i64(value: u64, field: &'static str) -> Result<i64, WorkflowStoreError> {
    i64::try_from(value).map_err(|_| anyhow::anyhow!("{field} exceeds SQLite integer range").into())
}

pub(super) fn to_u64(value: i64, field: &'static str) -> Result<u64, WorkflowStoreError> {
    u64::try_from(value).map_err(|_| anyhow::anyhow!("{field} is negative").into())
}

fn to_u32(value: i64, field: &'static str) -> Result<u32, WorkflowStoreError> {
    u32::try_from(value).map_err(|_| anyhow::anyhow!("{field} is out of range").into())
}

pub(super) fn is_unique_violation(err: &sqlx::Error) -> bool {
    matches!(
        err,
        sqlx::Error::Database(database) if database.is_unique_violation()
    )
}
