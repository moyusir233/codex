use serde::Deserialize;
use serde::Serialize;

use super::ArtifactId;
use super::WorkflowRunId;

/// Data-handling classification stored with a workflow artifact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactClassification {
    /// Safe for explicit user-facing publication.
    Public,
    /// Local workflow data that is not published by default.
    Internal,
    /// Sensitive local data that must not enter telemetry by default.
    Sensitive,
}

/// Immutable metadata for one locally stored workflow artifact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactMetadata {
    /// Unique artifact identity.
    pub artifact_id: ArtifactId,
    /// Run that owns the artifact.
    pub run_id: WorkflowRunId,
    /// Normalized path relative to the run artifact root.
    pub relative_path: String,
    /// Data-handling classification.
    pub classification: ArtifactClassification,
    /// Declared media type.
    pub media_type: String,
    /// Exact stored byte count.
    pub byte_count: u64,
    /// Lowercase SHA-256 digest.
    pub sha256: String,
    /// Creation time in Unix epoch milliseconds.
    pub created_at_ms: i64,
}
