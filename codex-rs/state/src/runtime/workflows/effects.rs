use serde_json::Value;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;
use sqlx::Row;

use crate::WorkflowEffectRecord;
use crate::WorkflowEffectState;

use super::WorkflowStore;
use super::WorkflowStoreError;
use super::events::append_event;

/// Parameters for planning a journaled external effect.
pub struct WorkflowEffectPlan {
    pub run_id: String,
    pub effect_key: String,
    pub kind: String,
    pub request: Value,
    pub created_at_ms: i64,
}

/// Whether a plan inserted a new effect or reconciled an identical retry.
#[derive(Clone, Debug, PartialEq)]
pub enum WorkflowEffectPlanOutcome {
    Planned(WorkflowEffectRecord),
    Existing(WorkflowEffectRecord),
}

/// Conditional state transition for one journaled effect.
pub struct WorkflowEffectUpdate {
    pub run_id: String,
    pub effect_key: String,
    pub expected_state: WorkflowEffectState,
    pub state: WorkflowEffectState,
    pub response: Option<Value>,
    pub error_code: Option<String>,
    pub updated_at_ms: i64,
}

impl WorkflowStore {
    /// Plans an effect exactly once by `(run_id, effect_key)`.
    pub async fn plan_effect(
        &self,
        plan: WorkflowEffectPlan,
    ) -> Result<WorkflowEffectPlanOutcome, WorkflowStoreError> {
        let mut tx = self.pool().begin().await?;
        let request_hash = canonical_workflow_request_hash(&plan.request)?;
        let request_json = serde_json::to_string(&plan.request)?;
        let result = sqlx::query(
            r#"
INSERT INTO workflow_effects (
    run_id, effect_key, kind, request_hash, request_json,
    state, created_at_ms, updated_at_ms
) VALUES (?, ?, ?, ?, ?, 'planned', ?, ?)
ON CONFLICT(run_id, effect_key) DO NOTHING
            "#,
        )
        .bind(&plan.run_id)
        .bind(&plan.effect_key)
        .bind(&plan.kind)
        .bind(&request_hash)
        .bind(request_json)
        .bind(plan.created_at_ms)
        .bind(plan.created_at_ms)
        .execute(&mut *tx)
        .await?;
        let row = sqlx::query(
            r#"
SELECT run_id, effect_key, kind, request_hash, request_json,
       state, response_json, error_code
FROM workflow_effects
WHERE run_id = ? AND effect_key = ?
            "#,
        )
        .bind(&plan.run_id)
        .bind(&plan.effect_key)
        .fetch_one(&mut *tx)
        .await?;
        let record = effect_from_row(row)?;
        if record.request_hash != request_hash
            || record.kind != plan.kind
            || record.request != plan.request
        {
            return Err(WorkflowStoreError::EffectConflict);
        }
        if result.rows_affected() == 1 {
            append_event(
                &mut tx,
                &plan.run_id,
                "effect.planned",
                Some(&plan.effect_key),
                &json!({"kind": plan.kind, "request_hash": request_hash}),
                plan.created_at_ms,
            )
            .await?;
            tx.commit().await?;
            Ok(WorkflowEffectPlanOutcome::Planned(record))
        } else {
            tx.commit().await?;
            Ok(WorkflowEffectPlanOutcome::Existing(record))
        }
    }

    /// Reads one effect journal row.
    pub async fn read_effect(
        &self,
        run_id: &str,
        effect_key: &str,
    ) -> Result<Option<WorkflowEffectRecord>, WorkflowStoreError> {
        let row = sqlx::query(
            r#"
SELECT run_id, effect_key, kind, request_hash, request_json,
       state, response_json, error_code
FROM workflow_effects
WHERE run_id = ? AND effect_key = ?
            "#,
        )
        .bind(run_id)
        .bind(effect_key)
        .fetch_optional(self.pool())
        .await?;
        row.map(effect_from_row).transpose()
    }

    /// Lists one run's effects in stable key order for recovery reconciliation.
    pub async fn list_effects(
        &self,
        run_id: &str,
    ) -> Result<Vec<WorkflowEffectRecord>, WorkflowStoreError> {
        let rows = sqlx::query(
            r#"
SELECT run_id, effect_key, kind, request_hash, request_json,
       state, response_json, error_code
FROM workflow_effects
WHERE run_id = ?
ORDER BY effect_key
            "#,
        )
        .bind(run_id)
        .fetch_all(self.pool())
        .await?;
        rows.into_iter().map(effect_from_row).collect()
    }

    /// Advances an effect journal state without changing its identity or request.
    pub async fn update_effect(
        &self,
        update: WorkflowEffectUpdate,
    ) -> Result<bool, WorkflowStoreError> {
        let mut tx = self.pool().begin().await?;
        let response_json = update
            .response
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let result = sqlx::query(
            r#"
UPDATE workflow_effects
SET state = ?, response_json = ?, error_code = ?, updated_at_ms = ?
WHERE run_id = ? AND effect_key = ? AND state = ?
            "#,
        )
        .bind(update.state.as_str())
        .bind(response_json)
        .bind(&update.error_code)
        .bind(update.updated_at_ms)
        .bind(&update.run_id)
        .bind(&update.effect_key)
        .bind(update.expected_state.as_str())
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 1 {
            append_event(
                &mut tx,
                &update.run_id,
                "effect.updated",
                Some(&update.effect_key),
                &json!({
                    "state": update.state.as_str(),
                    "error_code": update.error_code,
                }),
                update.updated_at_ms,
            )
            .await?;
            tx.commit().await?;
            Ok(true)
        } else {
            tx.rollback().await?;
            Ok(false)
        }
    }
}

/// Computes a stable SHA-256 hash for a JSON workflow request.
pub fn canonical_workflow_request_hash(request: &Value) -> Result<String, serde_json::Error> {
    let canonical = canonicalize_json(request);
    let bytes = serde_json::to_vec(&canonical)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn canonicalize_json(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut entries = object.iter().collect::<Vec<_>>();
            entries.sort_unstable_by_key(|(key, _)| *key);
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key.clone(), canonicalize_json(value)))
                    .collect(),
            )
        }
        Value::Array(values) => Value::Array(values.iter().map(canonicalize_json).collect()),
        _ => value.clone(),
    }
}

fn effect_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<WorkflowEffectRecord, WorkflowStoreError> {
    let response_json = row.try_get::<Option<String>, _>("response_json")?;
    Ok(WorkflowEffectRecord {
        run_id: row.try_get("run_id")?,
        effect_key: row.try_get("effect_key")?,
        kind: row.try_get("kind")?,
        request_hash: row.try_get("request_hash")?,
        request: serde_json::from_str(&row.try_get::<String, _>("request_json")?)?,
        state: WorkflowEffectState::parse(&row.try_get::<String, _>("state")?)?,
        response: response_json
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
        error_code: row.try_get("error_code")?,
    })
}
