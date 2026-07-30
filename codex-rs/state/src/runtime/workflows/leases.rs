use sqlx::Row;

use crate::WorkflowLease;

use super::WorkflowStore;
use super::WorkflowStoreError;
use super::runs::to_u64;

impl WorkflowStore {
    /// Acquires or renews a lease and increments its fencing token.
    ///
    /// A different owner can acquire only after the previous deadline. The
    /// returned fence must accompany every subsequent mutation.
    pub async fn acquire_lease(
        &self,
        run_id: &str,
        owner: &str,
        now_ms: i64,
        lease_duration_ms: i64,
    ) -> Result<Option<WorkflowLease>, WorkflowStoreError> {
        let expires_at_ms = now_ms.saturating_add(lease_duration_ms.max(1));
        let row = sqlx::query(
            r#"
UPDATE workflow_runs
SET lease_owner = ?,
    lease_expires_at_ms = ?,
    lease_fence = lease_fence + 1,
    updated_at_ms = MAX(updated_at_ms, ?)
WHERE run_id = ?
  AND (
      lease_owner IS NULL
      OR lease_owner = ?
      OR lease_expires_at_ms IS NULL
      OR lease_expires_at_ms <= ?
  )
RETURNING lease_fence
            "#,
        )
        .bind(owner)
        .bind(expires_at_ms)
        .bind(now_ms)
        .bind(run_id)
        .bind(owner)
        .bind(now_ms)
        .fetch_optional(self.pool())
        .await?;
        row.map(|row| {
            Ok(WorkflowLease {
                run_id: run_id.to_string(),
                owner: owner.to_string(),
                expires_at_ms,
                fence: to_u64(row.try_get::<i64, _>("lease_fence")?, "lease fence")?,
            })
        })
        .transpose()
    }
}
