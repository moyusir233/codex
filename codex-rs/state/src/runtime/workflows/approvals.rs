use serde_json::Value;
use serde_json::json;
use sqlx::Row;

use crate::WorkflowApprovalDecisionRecord;
use crate::WorkflowApprovalRecord;

use super::WorkflowStore;
use super::WorkflowStoreError;
use super::canonical_workflow_request_hash;
use super::events::append_event;
use super::runs::is_unique_violation;

/// Parameters for planning one immutable approval request.
pub struct WorkflowApprovalPlan {
    pub approval_id: String,
    pub run_id: String,
    pub effect_key: String,
    pub gate: String,
    pub subject: Value,
    pub allowed_approvers: Vec<String>,
    pub quorum: u32,
    pub deadline_ms: i64,
    pub created_at_ms: i64,
}

/// Whether approval planning inserted a request or reconciled an identical retry.
#[derive(Clone, Debug, PartialEq)]
pub enum WorkflowApprovalPlanOutcome {
    Planned(WorkflowApprovalRecord),
    Existing(WorkflowApprovalRecord),
}

/// Parameters for one immutable approval decision.
pub struct WorkflowApprovalDecisionAppend {
    pub decision_id: String,
    pub approval_id: String,
    pub sender_id: String,
    pub message_id: Option<String>,
    pub response_artifact_id: Option<String>,
    pub decision: String,
    pub reason: Option<String>,
    pub created_at_ms: i64,
}

/// Whether a decision was appended or reconciled from the same evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkflowApprovalDecisionAppendOutcome {
    Appended(WorkflowApprovalDecisionRecord),
    Existing(WorkflowApprovalDecisionRecord),
}

impl WorkflowStore {
    /// Plans an approval exactly once by `(run_id, effect_key)`.
    pub async fn plan_approval(
        &self,
        plan: WorkflowApprovalPlan,
    ) -> Result<WorkflowApprovalPlanOutcome, WorkflowStoreError> {
        let request = json!({
            "gate": plan.gate,
            "subject": plan.subject,
            "allowed_approvers": plan.allowed_approvers,
            "quorum": plan.quorum,
            "deadline_ms": plan.deadline_ms,
        });
        let request_hash = canonical_workflow_request_hash(&request)?;
        let mut tx = self.pool().begin().await?;
        let result = sqlx::query(
            r#"
INSERT INTO workflow_approvals (
    approval_id, run_id, effect_key, request_hash, gate, subject_json,
    allowed_approvers_json, quorum, deadline_ms, created_at_ms
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
ON CONFLICT(run_id, effect_key) DO NOTHING
            "#,
        )
        .bind(&plan.approval_id)
        .bind(&plan.run_id)
        .bind(&plan.effect_key)
        .bind(&request_hash)
        .bind(&plan.gate)
        .bind(serde_json::to_string(&plan.subject)?)
        .bind(serde_json::to_string(&plan.allowed_approvers)?)
        .bind(i64::from(plan.quorum))
        .bind(plan.deadline_ms)
        .bind(plan.created_at_ms)
        .execute(&mut *tx)
        .await?;
        let row = sqlx::query(
            r#"
SELECT approval_id, run_id, effect_key, request_hash, gate, subject_json,
       allowed_approvers_json, quorum, deadline_ms, created_at_ms
FROM workflow_approvals
WHERE run_id = ? AND effect_key = ?
            "#,
        )
        .bind(&plan.run_id)
        .bind(&plan.effect_key)
        .fetch_one(&mut *tx)
        .await?;
        let record = approval_from_row(row)?;
        if record.request_hash != request_hash
            || record.gate != plan.gate
            || record.subject != plan.subject
            || record.allowed_approvers != plan.allowed_approvers
            || record.quorum != plan.quorum
            || record.deadline_ms != plan.deadline_ms
        {
            return Err(WorkflowStoreError::ApprovalConflict);
        }
        if result.rows_affected() == 1 {
            append_event(
                &mut tx,
                &plan.run_id,
                "approval.requested",
                Some(&record.approval_id),
                &json!({
                    "gate": record.gate,
                    "request_hash": record.request_hash,
                    "quorum": record.quorum,
                }),
                plan.created_at_ms,
            )
            .await?;
            tx.commit().await?;
            Ok(WorkflowApprovalPlanOutcome::Planned(record))
        } else {
            tx.commit().await?;
            Ok(WorkflowApprovalPlanOutcome::Existing(record))
        }
    }

    /// Reads an approval only when it belongs to the supplied run.
    pub async fn read_approval(
        &self,
        run_id: &str,
        approval_id: &str,
    ) -> Result<Option<WorkflowApprovalRecord>, WorkflowStoreError> {
        let row = sqlx::query(
            r#"
SELECT approval_id, run_id, effect_key, request_hash, gate, subject_json,
       allowed_approvers_json, quorum, deadline_ms, created_at_ms
FROM workflow_approvals
WHERE run_id = ? AND approval_id = ?
            "#,
        )
        .bind(run_id)
        .bind(approval_id)
        .fetch_optional(self.pool())
        .await?;
        row.map(approval_from_row).transpose()
    }

    /// Lists one approval's immutable decisions in arrival order.
    pub async fn list_approval_decisions(
        &self,
        approval_id: &str,
    ) -> Result<Vec<WorkflowApprovalDecisionRecord>, WorkflowStoreError> {
        let rows = sqlx::query(
            r#"
SELECT decision_id, approval_id, sender_id, message_id,
       response_artifact_id, decision, reason, created_at_ms
FROM workflow_approval_decisions
WHERE approval_id = ?
ORDER BY created_at_ms, decision_id
            "#,
        )
        .bind(approval_id)
        .fetch_all(self.pool())
        .await?;
        rows.into_iter().map(approval_decision_from_row).collect()
    }

    /// Appends a decision without mutating earlier history.
    pub async fn append_approval_decision(
        &self,
        append: WorkflowApprovalDecisionAppend,
    ) -> Result<WorkflowApprovalDecisionAppendOutcome, WorkflowStoreError> {
        let mut tx = self.pool().begin().await?;
        let result = sqlx::query(
            r#"
INSERT INTO workflow_approval_decisions (
    decision_id, approval_id, sender_id, message_id, response_artifact_id,
    decision, reason, created_at_ms
) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&append.decision_id)
        .bind(&append.approval_id)
        .bind(&append.sender_id)
        .bind(&append.message_id)
        .bind(&append.response_artifact_id)
        .bind(&append.decision)
        .bind(&append.reason)
        .bind(append.created_at_ms)
        .execute(&mut *tx)
        .await;
        match result {
            Ok(_) => {
                let record = decision_from_append(&append);
                let run_id = sqlx::query_scalar::<_, String>(
                    "SELECT run_id FROM workflow_approvals WHERE approval_id = ?",
                )
                .bind(&append.approval_id)
                .fetch_one(&mut *tx)
                .await?;
                append_event(
                    &mut tx,
                    &run_id,
                    "approval.decision_appended",
                    Some(&append.approval_id),
                    &json!({
                        "decision_id": record.decision_id,
                        "decision": record.decision,
                        "sender_id": record.sender_id,
                    }),
                    append.created_at_ms,
                )
                .await?;
                tx.commit().await?;
                Ok(WorkflowApprovalDecisionAppendOutcome::Appended(record))
            }
            Err(error) if is_unique_violation(&error) => {
                tx.rollback().await?;
                let existing = if let Some(message_id) = &append.message_id {
                    sqlx::query(
                        r#"
SELECT decision_id, approval_id, sender_id, message_id,
       response_artifact_id, decision, reason, created_at_ms
FROM workflow_approval_decisions
WHERE approval_id = ? AND message_id = ?
                        "#,
                    )
                    .bind(&append.approval_id)
                    .bind(message_id)
                    .fetch_optional(self.pool())
                    .await?
                } else {
                    sqlx::query(
                        r#"
SELECT decision_id, approval_id, sender_id, message_id,
       response_artifact_id, decision, reason, created_at_ms
FROM workflow_approval_decisions
WHERE decision_id = ?
                        "#,
                    )
                    .bind(&append.decision_id)
                    .fetch_optional(self.pool())
                    .await?
                };
                let Some(existing) = existing else {
                    return Err(WorkflowStoreError::ApprovalDecisionConflict);
                };
                let existing = approval_decision_from_row(existing)?;
                if existing != decision_from_append(&append) {
                    return Err(WorkflowStoreError::ApprovalDecisionConflict);
                }
                Ok(WorkflowApprovalDecisionAppendOutcome::Existing(existing))
            }
            Err(error) => Err(error.into()),
        }
    }
}

fn approval_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<WorkflowApprovalRecord, WorkflowStoreError> {
    Ok(WorkflowApprovalRecord {
        approval_id: row.try_get("approval_id")?,
        run_id: row.try_get("run_id")?,
        effect_key: row.try_get("effect_key")?,
        request_hash: row.try_get("request_hash")?,
        gate: row.try_get("gate")?,
        subject: serde_json::from_str(&row.try_get::<String, _>("subject_json")?)?,
        allowed_approvers: serde_json::from_str(
            &row.try_get::<String, _>("allowed_approvers_json")?,
        )?,
        quorum: u32::try_from(row.try_get::<i64, _>("quorum")?)
            .map_err(|_| anyhow::anyhow!("approval quorum is invalid"))?,
        deadline_ms: row.try_get("deadline_ms")?,
        created_at_ms: row.try_get("created_at_ms")?,
    })
}

fn approval_decision_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<WorkflowApprovalDecisionRecord, WorkflowStoreError> {
    Ok(WorkflowApprovalDecisionRecord {
        decision_id: row.try_get("decision_id")?,
        approval_id: row.try_get("approval_id")?,
        sender_id: row.try_get("sender_id")?,
        message_id: row.try_get("message_id")?,
        response_artifact_id: row.try_get("response_artifact_id")?,
        decision: row.try_get("decision")?,
        reason: row.try_get("reason")?,
        created_at_ms: row.try_get("created_at_ms")?,
    })
}

fn decision_from_append(append: &WorkflowApprovalDecisionAppend) -> WorkflowApprovalDecisionRecord {
    WorkflowApprovalDecisionRecord {
        decision_id: append.decision_id.clone(),
        approval_id: append.approval_id.clone(),
        sender_id: append.sender_id.clone(),
        message_id: append.message_id.clone(),
        response_artifact_id: append.response_artifact_id.clone(),
        decision: append.decision.clone(),
        reason: append.reason.clone(),
        created_at_ms: append.created_at_ms,
    }
}
