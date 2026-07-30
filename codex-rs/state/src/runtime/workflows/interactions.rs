use serde_json::Value;
use serde_json::json;
use sqlx::Row;

use crate::WorkflowInteractionRecord;
use crate::WorkflowInteractionState;

use super::WorkflowStore;
use super::WorkflowStoreError;
use super::events::append_event;

/// Parameters for planning a deduplicated operator interaction.
pub struct WorkflowInteractionPlan {
    pub interaction_id: String,
    pub run_id: String,
    pub dedupe_key: String,
    pub kind: String,
    pub request: Value,
    pub deadline_ms: Option<i64>,
    pub created_at_ms: i64,
}

/// Whether an interaction was inserted or reconciled with an identical retry.
#[derive(Clone, Debug, PartialEq)]
pub enum WorkflowInteractionPlanOutcome {
    Planned(WorkflowInteractionRecord),
    Existing(WorkflowInteractionRecord),
}

impl WorkflowStore {
    /// Reads one interaction by its stable identity.
    pub async fn read_interaction(
        &self,
        interaction_id: &str,
    ) -> Result<Option<WorkflowInteractionRecord>, WorkflowStoreError> {
        let row = sqlx::query(
            r#"
SELECT interaction_id, run_id, dedupe_key, kind, request_json, state,
       response_artifact_id, deadline_ms, created_at_ms, updated_at_ms
FROM workflow_interactions
WHERE interaction_id = ?
            "#,
        )
        .bind(interaction_id)
        .fetch_optional(self.pool())
        .await?;
        row.map(interaction_from_row).transpose()
    }

    /// Plans an interaction exactly once by `(run_id, dedupe_key)`.
    pub async fn plan_interaction(
        &self,
        plan: WorkflowInteractionPlan,
    ) -> Result<WorkflowInteractionPlanOutcome, WorkflowStoreError> {
        let mut tx = self.pool().begin().await?;
        let request_json = serde_json::to_string(&plan.request)?;
        let result = sqlx::query(
            r#"
INSERT INTO workflow_interactions (
    interaction_id, run_id, dedupe_key, kind, request_json, state,
    deadline_ms, created_at_ms, updated_at_ms
) VALUES (?, ?, ?, ?, ?, 'planned', ?, ?, ?)
ON CONFLICT(run_id, dedupe_key) DO NOTHING
            "#,
        )
        .bind(&plan.interaction_id)
        .bind(&plan.run_id)
        .bind(&plan.dedupe_key)
        .bind(&plan.kind)
        .bind(request_json)
        .bind(plan.deadline_ms)
        .bind(plan.created_at_ms)
        .bind(plan.created_at_ms)
        .execute(&mut *tx)
        .await?;
        let row = sqlx::query(
            r#"
SELECT interaction_id, run_id, dedupe_key, kind, request_json, state,
       response_artifact_id, deadline_ms, created_at_ms, updated_at_ms
FROM workflow_interactions
WHERE run_id = ? AND dedupe_key = ?
            "#,
        )
        .bind(&plan.run_id)
        .bind(&plan.dedupe_key)
        .fetch_one(&mut *tx)
        .await?;
        let record = interaction_from_row(row)?;
        if record.kind != plan.kind || record.request != plan.request {
            return Err(WorkflowStoreError::InteractionConflict);
        }
        if result.rows_affected() == 1 {
            append_event(
                &mut tx,
                &plan.run_id,
                "interaction.planned",
                Some(&record.interaction_id),
                &json!({"kind": plan.kind, "dedupe_key": plan.dedupe_key}),
                plan.created_at_ms,
            )
            .await?;
            tx.commit().await?;
            Ok(WorkflowInteractionPlanOutcome::Planned(record))
        } else {
            tx.commit().await?;
            Ok(WorkflowInteractionPlanOutcome::Existing(record))
        }
    }

    /// Updates an interaction and appends its lifecycle event atomically.
    pub async fn update_interaction(
        &self,
        interaction_id: &str,
        expected_state: WorkflowInteractionState,
        state: WorkflowInteractionState,
        response_artifact_id: Option<&str>,
        updated_at_ms: i64,
    ) -> Result<bool, WorkflowStoreError> {
        let mut tx = self.pool().begin().await?;
        let row = sqlx::query(
            r#"
UPDATE workflow_interactions
SET state = ?, response_artifact_id = ?, updated_at_ms = ?
WHERE interaction_id = ? AND state = ?
RETURNING run_id, kind
            "#,
        )
        .bind(state.as_str())
        .bind(response_artifact_id)
        .bind(updated_at_ms)
        .bind(interaction_id)
        .bind(expected_state.as_str())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            tx.rollback().await?;
            return Ok(false);
        };
        let run_id = row.try_get::<String, _>("run_id")?;
        let kind = row.try_get::<String, _>("kind")?;
        append_event(
            &mut tx,
            &run_id,
            "interaction.updated",
            Some(interaction_id),
            &json!({"kind": kind, "state": state.as_str()}),
            updated_at_ms,
        )
        .await?;
        tx.commit().await?;
        Ok(true)
    }
}

fn interaction_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<WorkflowInteractionRecord, WorkflowStoreError> {
    Ok(WorkflowInteractionRecord {
        interaction_id: row.try_get("interaction_id")?,
        run_id: row.try_get("run_id")?,
        dedupe_key: row.try_get("dedupe_key")?,
        kind: row.try_get("kind")?,
        request: serde_json::from_str(&row.try_get::<String, _>("request_json")?)?,
        state: WorkflowInteractionState::parse(&row.try_get::<String, _>("state")?)?,
        response_artifact_id: row.try_get("response_artifact_id")?,
        deadline_ms: row.try_get("deadline_ms")?,
        created_at_ms: row.try_get("created_at_ms")?,
        updated_at_ms: row.try_get("updated_at_ms")?,
    })
}
