use super::WorkflowStore;
use super::WorkflowStoreError;
use super::events::append_event;
use super::runs::is_unique_violation;
use super::runs::to_i64;
use crate::WorkflowArtifactRecord;
use serde_json::json;
use sqlx::Row;

impl WorkflowStore {
    /// Records the immutable manifest for an artifact already stored on disk.
    pub async fn record_artifact(
        &self,
        artifact: &WorkflowArtifactRecord,
    ) -> Result<(), WorkflowStoreError> {
        let mut tx = self.pool().begin().await?;
        let result = sqlx::query(
            r#"
INSERT INTO workflow_artifacts (
    artifact_id, run_id, relative_path, classification,
    media_type, byte_count, sha256, created_at_ms
) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&artifact.artifact_id)
        .bind(&artifact.run_id)
        .bind(&artifact.relative_path)
        .bind(artifact.classification.as_str())
        .bind(&artifact.media_type)
        .bind(to_i64(artifact.byte_count, "artifact byte count")?)
        .bind(&artifact.sha256)
        .bind(artifact.created_at_ms)
        .execute(&mut *tx)
        .await;
        match result {
            Ok(_) => {
                append_event(
                    &mut tx,
                    &artifact.run_id,
                    "artifact.created",
                    Some(&artifact.artifact_id),
                    &json!({
                        "classification": artifact.classification.as_str(),
                        "media_type": artifact.media_type,
                        "byte_count": artifact.byte_count,
                        "sha256": artifact.sha256,
                    }),
                    artifact.created_at_ms,
                )
                .await?;
                tx.commit().await?;
                Ok(())
            }
            Err(err) if is_unique_violation(&err) => Err(WorkflowStoreError::DuplicateArtifact),
            Err(err) => Err(err.into()),
        }
    }

    /// Reads one immutable artifact manifest.
    pub async fn read_artifact(
        &self,
        artifact_id: &str,
    ) -> Result<Option<WorkflowArtifactRecord>, WorkflowStoreError> {
        let row = sqlx::query(
            r#"
SELECT artifact_id, run_id, relative_path, classification,
       media_type, byte_count, sha256, created_at_ms
FROM workflow_artifacts
WHERE artifact_id = ?
            "#,
        )
        .bind(artifact_id)
        .fetch_optional(self.pool())
        .await?;
        row.map(artifact_from_row).transpose()
    }

    /// Lists immutable artifact manifests for a run in creation order.
    pub async fn list_artifacts(
        &self,
        run_id: &str,
    ) -> Result<Vec<WorkflowArtifactRecord>, WorkflowStoreError> {
        let rows = sqlx::query(
            r#"
SELECT artifact_id, run_id, relative_path, classification,
       media_type, byte_count, sha256, created_at_ms
FROM workflow_artifacts
WHERE run_id = ?
ORDER BY created_at_ms, artifact_id
            "#,
        )
        .bind(run_id)
        .fetch_all(self.pool())
        .await?;
        rows.into_iter().map(artifact_from_row).collect()
    }
}

fn artifact_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<WorkflowArtifactRecord, WorkflowStoreError> {
    Ok(WorkflowArtifactRecord {
        artifact_id: row.try_get("artifact_id")?,
        run_id: row.try_get("run_id")?,
        relative_path: row.try_get("relative_path")?,
        classification: crate::WorkflowArtifactClassification::parse(
            &row.try_get::<String, _>("classification")?,
        )?,
        media_type: row.try_get("media_type")?,
        byte_count: super::runs::to_u64(
            row.try_get::<i64, _>("byte_count")?,
            "artifact byte count",
        )?,
        sha256: row.try_get("sha256")?,
        created_at_ms: row.try_get("created_at_ms")?,
    })
}
