use serde_json::json;
use sqlx::Row;

use crate::WorkflowInteractionState;
use crate::WorkflowLarkInteractionRecord;

use super::WorkflowStore;
use super::WorkflowStoreError;
use super::effects::canonical_workflow_request_hash;
use super::events::append_event;

/// Immutable routing information planned before a Lark request is sent.
pub struct WorkflowLarkInteractionPlan {
    pub interaction_id: String,
    pub run_id: String,
    pub effect_key: String,
    pub chat_id: String,
    pub thread_id: Option<String>,
    pub correlation_token: String,
    pub allowed_senders: Vec<String>,
    pub watermark_ms: i64,
    pub created_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkflowLarkInteractionPlanOutcome {
    Planned(WorkflowLarkInteractionRecord),
    Existing(WorkflowLarkInteractionRecord),
}

/// Verified external message proposed as an interaction response.
pub struct WorkflowLarkResolve {
    pub interaction_id: String,
    pub source: String,
    pub event_id: String,
    pub message_id: String,
    pub chat_id: String,
    pub thread_id: Option<String>,
    pub sender_id: String,
    pub response_artifact_id: String,
    pub received_at_ms: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkflowLarkResolveOutcome {
    Resolved,
    Duplicate,
    NotWaiting,
    Late,
    WrongChat,
    WrongThread,
    WrongSender,
}

impl WorkflowStore {
    pub async fn plan_lark_interaction(
        &self,
        plan: WorkflowLarkInteractionPlan,
    ) -> Result<WorkflowLarkInteractionPlanOutcome, WorkflowStoreError> {
        let request = json!({
            "interaction_id": &plan.interaction_id,
            "run_id": &plan.run_id,
            "effect_key": &plan.effect_key,
            "chat_id": &plan.chat_id,
            "thread_id": &plan.thread_id,
            "correlation_token": &plan.correlation_token,
            "allowed_senders": &plan.allowed_senders,
            "watermark_ms": plan.watermark_ms,
        });
        let request_hash = canonical_workflow_request_hash(&request)?;
        let allowed_senders = serde_json::to_string(&plan.allowed_senders)?;
        let result = sqlx::query(
            r#"
INSERT INTO workflow_lark_interactions (
    interaction_id, run_id, effect_key, request_hash, chat_id, thread_id,
    correlation_token, allowed_senders_json, watermark_ms, updated_at_ms
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
ON CONFLICT(interaction_id) DO NOTHING
            "#,
        )
        .bind(&plan.interaction_id)
        .bind(&plan.run_id)
        .bind(&plan.effect_key)
        .bind(&request_hash)
        .bind(&plan.chat_id)
        .bind(&plan.thread_id)
        .bind(&plan.correlation_token)
        .bind(allowed_senders)
        .bind(plan.watermark_ms)
        .bind(plan.created_at_ms)
        .execute(self.pool())
        .await?;
        let record = self
            .read_lark_interaction(&plan.interaction_id)
            .await?
            .ok_or(WorkflowStoreError::StaleWrite)?;
        if record.request_hash != request_hash {
            return Err(WorkflowStoreError::InteractionConflict);
        }
        Ok(if result.rows_affected() == 1 {
            WorkflowLarkInteractionPlanOutcome::Planned(record)
        } else {
            WorkflowLarkInteractionPlanOutcome::Existing(record)
        })
    }

    pub async fn read_lark_interaction(
        &self,
        interaction_id: &str,
    ) -> Result<Option<WorkflowLarkInteractionRecord>, WorkflowStoreError> {
        let row = sqlx::query(
            r#"
SELECT interaction_id, run_id, effect_key, request_hash, chat_id, thread_id,
       request_message_id, correlation_token, allowed_senders_json,
       watermark_ms, poll_page_token, updated_at_ms
FROM workflow_lark_interactions
WHERE interaction_id = ?
            "#,
        )
        .bind(interaction_id)
        .fetch_optional(self.pool())
        .await?;
        row.map(lark_from_row).transpose()
    }

    pub async fn mark_lark_message_sent(
        &self,
        interaction_id: &str,
        request_message_id: &str,
        updated_at_ms: i64,
    ) -> Result<bool, WorkflowStoreError> {
        Ok(sqlx::query(
            r#"
UPDATE workflow_lark_interactions
SET request_message_id = COALESCE(request_message_id, ?), updated_at_ms = ?
WHERE interaction_id = ?
  AND (request_message_id IS NULL OR request_message_id = ?)
            "#,
        )
        .bind(request_message_id)
        .bind(updated_at_ms)
        .bind(interaction_id)
        .bind(request_message_id)
        .execute(self.pool())
        .await?
        .rows_affected()
            == 1)
    }

    pub async fn update_lark_poll_cursor(
        &self,
        interaction_id: &str,
        watermark_ms: i64,
        page_token: Option<&str>,
        updated_at_ms: i64,
    ) -> Result<bool, WorkflowStoreError> {
        Ok(sqlx::query(
            r#"
UPDATE workflow_lark_interactions
SET watermark_ms = MAX(watermark_ms, ?), poll_page_token = ?, updated_at_ms = ?
WHERE interaction_id = ?
            "#,
        )
        .bind(watermark_ms)
        .bind(page_token)
        .bind(updated_at_ms)
        .bind(interaction_id)
        .execute(self.pool())
        .await?
        .rows_affected()
            == 1)
    }

    pub async fn resolve_lark_interaction(
        &self,
        resolve: WorkflowLarkResolve,
    ) -> Result<WorkflowLarkResolveOutcome, WorkflowStoreError> {
        let mut tx = self.pool().begin().await?;
        let inserted = sqlx::query(
            r#"
INSERT INTO workflow_external_events (
    run_id, source, external_id, received_at_ms, metadata_json
) SELECT run_id, ?, ?, ?, ?
FROM workflow_lark_interactions
WHERE interaction_id = ?
ON CONFLICT(source, external_id) DO NOTHING
            "#,
        )
        .bind(&resolve.source)
        .bind(&resolve.event_id)
        .bind(resolve.received_at_ms)
        .bind(serde_json::to_string(&json!({
            "message_id": &resolve.message_id,
            "chat_id": &resolve.chat_id,
            "thread_id": &resolve.thread_id,
            "sender_id": &resolve.sender_id,
        }))?)
        .bind(&resolve.interaction_id)
        .execute(&mut *tx)
        .await?;
        if inserted.rows_affected() == 0 {
            tx.rollback().await?;
            return Ok(WorkflowLarkResolveOutcome::Duplicate);
        }
        let row = sqlx::query(
            r#"
SELECT l.run_id, l.chat_id, l.thread_id, l.allowed_senders_json,
       i.state, i.deadline_ms
FROM workflow_lark_interactions l
JOIN workflow_interactions i ON i.interaction_id = l.interaction_id
WHERE l.interaction_id = ?
            "#,
        )
        .bind(&resolve.interaction_id)
        .fetch_one(&mut *tx)
        .await?;
        let state =
            WorkflowInteractionState::parse(&row.try_get::<String, _>("state")?)?;
        let deadline_ms = row.try_get::<Option<i64>, _>("deadline_ms")?;
        let expected_chat = row.try_get::<String, _>("chat_id")?;
        let expected_thread = row.try_get::<Option<String>, _>("thread_id")?;
        let allowed_senders: Vec<String> =
            serde_json::from_str(&row.try_get::<String, _>("allowed_senders_json")?)?;
        let outcome = if state != WorkflowInteractionState::Waiting {
            WorkflowLarkResolveOutcome::NotWaiting
        } else if deadline_ms.is_some_and(|deadline| resolve.received_at_ms > deadline) {
            WorkflowLarkResolveOutcome::Late
        } else if resolve.chat_id != expected_chat {
            WorkflowLarkResolveOutcome::WrongChat
        } else if expected_thread.is_some() && resolve.thread_id != expected_thread {
            WorkflowLarkResolveOutcome::WrongThread
        } else if !allowed_senders.contains(&resolve.sender_id) {
            WorkflowLarkResolveOutcome::WrongSender
        } else {
            WorkflowLarkResolveOutcome::Resolved
        };
        if outcome != WorkflowLarkResolveOutcome::Resolved {
            tx.commit().await?;
            return Ok(outcome);
        }
        let run_id = row.try_get::<String, _>("run_id")?;
        sqlx::query(
            r#"
UPDATE workflow_interactions
SET state = 'resolved', response_artifact_id = ?, updated_at_ms = ?
WHERE interaction_id = ? AND state = 'waiting'
            "#,
        )
        .bind(&resolve.response_artifact_id)
        .bind(resolve.received_at_ms)
        .bind(&resolve.interaction_id)
        .execute(&mut *tx)
        .await?;
        append_event(
            &mut tx,
            &run_id,
            "interaction.updated",
            Some(&resolve.interaction_id),
            &json!({"kind": "lark.reply", "state": "resolved"}),
            resolve.received_at_ms,
        )
        .await?;
        let resumed = sqlx::query(
            r#"
UPDATE workflow_runs
SET status = 'pending', wake_json = NULL, row_version = row_version + 1,
    updated_at_ms = ?
WHERE run_id = ? AND status = 'waiting'
  AND json_extract(wake_json, '$.HumanInteraction') = ?
            "#,
        )
        .bind(resolve.received_at_ms)
        .bind(&run_id)
        .bind(&resolve.interaction_id)
        .execute(&mut *tx)
        .await?;
        if resumed.rows_affected() == 1 {
            append_event(
                &mut tx,
                &run_id,
                "run.resumed",
                Some(&run_id),
                &json!({"source": "lark.reply"}),
                resolve.received_at_ms,
            )
            .await?;
        }
        tx.commit().await?;
        Ok(outcome)
    }
}

fn lark_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<WorkflowLarkInteractionRecord, WorkflowStoreError> {
    Ok(WorkflowLarkInteractionRecord {
        interaction_id: row.try_get("interaction_id")?,
        run_id: row.try_get("run_id")?,
        effect_key: row.try_get("effect_key")?,
        request_hash: row.try_get("request_hash")?,
        chat_id: row.try_get("chat_id")?,
        thread_id: row.try_get("thread_id")?,
        request_message_id: row.try_get("request_message_id")?,
        correlation_token: row.try_get("correlation_token")?,
        allowed_senders: serde_json::from_str(
            &row.try_get::<String, _>("allowed_senders_json")?,
        )?,
        watermark_ms: row.try_get("watermark_ms")?,
        poll_page_token: row.try_get("poll_page_token")?,
        updated_at_ms: row.try_get("updated_at_ms")?,
    })
}
