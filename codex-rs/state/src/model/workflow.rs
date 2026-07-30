use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

/// Durable workflow run lifecycle state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunStatus {
    Pending,
    Running,
    Waiting,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
    NeedsOperator,
}

impl WorkflowRunStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Cancelling => "cancelling",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::NeedsOperator => "needs_operator",
        }
    }

    pub(crate) fn parse(value: &str) -> anyhow::Result<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "waiting" => Ok(Self::Waiting),
            "cancelling" => Ok(Self::Cancelling),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "needs_operator" => Ok(Self::NeedsOperator),
            _ => Err(anyhow::anyhow!("unknown workflow run status")),
        }
    }
}

/// Parameters for atomically creating one durable workflow run.
#[derive(Clone, Debug)]
pub struct WorkflowRunCreate {
    pub run_id: String,
    pub definition_name: String,
    pub definition_version: String,
    pub state_schema_version: u32,
    pub state: Value,
    pub arguments: Value,
    pub non_interactive: bool,
    pub detached: bool,
    pub concurrency: Option<u32>,
    pub created_at_ms: i64,
}

/// Durable workflow run snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkflowRunRecord {
    pub run_id: String,
    pub definition_name: String,
    pub definition_version: String,
    pub state_schema_version: u32,
    pub state: Value,
    pub arguments: Value,
    pub non_interactive: bool,
    pub detached: bool,
    pub concurrency: Option<u32>,
    pub status: WorkflowRunStatus,
    pub output: Option<Value>,
    pub error_code: Option<String>,
    pub wake: Option<Value>,
    pub cancellation_requested_at_ms: Option<i64>,
    pub deadline_ms: Option<i64>,
    pub row_version: u64,
    pub next_sequence: u64,
    pub lease_owner: Option<String>,
    pub lease_expires_at_ms: Option<i64>,
    pub lease_fence: u64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// Successfully acquired fenced workflow lease.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowLease {
    pub run_id: String,
    pub owner: String,
    pub expires_at_ms: i64,
    pub fence: u64,
}

/// Atomically persisted workflow event.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkflowEventRecord {
    pub run_id: String,
    pub sequence: u64,
    pub kind: String,
    pub entity_id: Option<String>,
    pub metadata: Value,
    pub created_at_ms: i64,
}

/// State of a journaled external workflow effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkflowEffectState {
    Planned,
    Dispatched,
    Applied,
    Ambiguous,
    Cancelled,
}

impl WorkflowEffectState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Dispatched => "dispatched",
            Self::Applied => "applied",
            Self::Ambiguous => "ambiguous",
            Self::Cancelled => "cancelled",
        }
    }

    pub(crate) fn parse(value: &str) -> anyhow::Result<Self> {
        match value {
            "planned" => Ok(Self::Planned),
            "dispatched" => Ok(Self::Dispatched),
            "applied" => Ok(Self::Applied),
            "ambiguous" => Ok(Self::Ambiguous),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(anyhow::anyhow!("unknown workflow effect state")),
        }
    }
}

/// Durable journal entry for one idempotent workflow effect key.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkflowEffectRecord {
    pub run_id: String,
    pub effect_key: String,
    pub kind: String,
    pub request_hash: String,
    pub request: Value,
    pub state: WorkflowEffectState,
    pub response: Option<Value>,
    pub error_code: Option<String>,
}

/// Durable workflow node lifecycle state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkflowNodeStatus {
    Pending,
    Ready,
    Running,
    Waiting,
    Succeeded,
    Failed,
    Cancelled,
    Blocked,
}

impl WorkflowNodeStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Blocked => "blocked",
        }
    }

    pub(crate) fn parse(value: &str) -> anyhow::Result<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "ready" => Ok(Self::Ready),
            "running" => Ok(Self::Running),
            "waiting" => Ok(Self::Waiting),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "blocked" => Ok(Self::Blocked),
            _ => Err(anyhow::anyhow!("unknown workflow node status")),
        }
    }
}

/// Parameters for adding one node to a durable workflow graph.
#[derive(Clone, Debug)]
pub struct WorkflowNodeCreate {
    pub node_id: String,
    pub node_key: String,
    pub spec: Value,
    pub status: WorkflowNodeStatus,
    pub failure_policy: String,
    pub created_at_ms: i64,
}

/// Durable workflow node snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkflowNodeRecord {
    pub node_id: String,
    pub run_id: String,
    pub node_key: String,
    pub thread_id: Option<String>,
    pub spec: Value,
    pub status: WorkflowNodeStatus,
    pub retry_at_ms: Option<i64>,
    pub failure_policy: String,
    pub row_version: u64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// Durable lifecycle state for one immutable node attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowNodeAttemptStatus {
    Planned,
    Submitted,
    Running,
    Succeeded,
    Failed,
    Interrupted,
    Cancelled,
    Ambiguous,
}

impl WorkflowNodeAttemptStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Submitted => "submitted",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
            Self::Cancelled => "cancelled",
            Self::Ambiguous => "ambiguous",
        }
    }

    pub(crate) fn parse(value: &str) -> anyhow::Result<Self> {
        match value {
            "planned" => Ok(Self::Planned),
            "submitted" => Ok(Self::Submitted),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "interrupted" => Ok(Self::Interrupted),
            "cancelled" => Ok(Self::Cancelled),
            "ambiguous" => Ok(Self::Ambiguous),
            _ => Err(anyhow::anyhow!("unknown workflow node attempt status")),
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Interrupted | Self::Cancelled | Self::Ambiguous
        )
    }
}

/// Parameters for recording a prepared node attempt before host submission.
#[derive(Clone, Debug)]
pub struct WorkflowNodeAttemptCreate {
    pub attempt_id: String,
    pub node_id: String,
    pub submission_id: String,
    pub input_hash: String,
    pub thread_id: Option<String>,
    pub created_at_ms: i64,
}

/// One immutable node-attempt snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowNodeAttemptRecord {
    pub attempt_id: String,
    pub run_id: String,
    pub node_id: String,
    pub attempt_number: u32,
    pub submission_id: String,
    pub input_hash: String,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub status: WorkflowNodeAttemptStatus,
    pub started_at_ms: Option<i64>,
    pub completed_at_ms: Option<i64>,
    pub error_code: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// CAS update for one node attempt.
#[derive(Clone, Debug)]
pub struct WorkflowNodeAttemptTransition {
    pub expected_status: WorkflowNodeAttemptStatus,
    pub status: WorkflowNodeAttemptStatus,
    pub turn_id: Option<String>,
    pub error_code: Option<String>,
    pub updated_at_ms: i64,
}

/// One persisted dependency edge and its fan-in policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowDependencyRecord {
    pub run_id: String,
    pub node_id: String,
    pub depends_on_node_id: String,
    pub policy: String,
    pub at_least: Option<u32>,
}

/// One thread ever owned by a logical workflow node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowNodeThreadRecord {
    pub node_id: String,
    pub thread_id: String,
    pub ordinal: u32,
    pub created_at_ms: i64,
}

/// State of a deduplicated workflow interaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkflowInteractionState {
    Planned,
    Waiting,
    Resolved,
    TimedOut,
    Cancelled,
}

impl WorkflowInteractionState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Waiting => "waiting",
            Self::Resolved => "resolved",
            Self::TimedOut => "timed_out",
            Self::Cancelled => "cancelled",
        }
    }

    pub(crate) fn parse(value: &str) -> anyhow::Result<Self> {
        match value {
            "planned" => Ok(Self::Planned),
            "waiting" => Ok(Self::Waiting),
            "resolved" => Ok(Self::Resolved),
            "timed_out" => Ok(Self::TimedOut),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(anyhow::anyhow!("unknown workflow interaction state")),
        }
    }
}

/// Durable interaction journal entry.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkflowInteractionRecord {
    pub interaction_id: String,
    pub run_id: String,
    pub dedupe_key: String,
    pub kind: String,
    pub request: Value,
    pub state: WorkflowInteractionState,
    pub response_artifact_id: Option<String>,
    pub deadline_ms: Option<i64>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// Classification attached to a durable workflow artifact manifest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkflowArtifactClassification {
    Public,
    Internal,
    Sensitive,
}

impl WorkflowArtifactClassification {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Internal => "internal",
            Self::Sensitive => "sensitive",
        }
    }

    pub(crate) fn parse(value: &str) -> anyhow::Result<Self> {
        match value {
            "public" => Ok(Self::Public),
            "internal" => Ok(Self::Internal),
            "sensitive" => Ok(Self::Sensitive),
            _ => Err(anyhow::anyhow!("unknown workflow artifact classification")),
        }
    }
}

/// Metadata for one immutable workflow artifact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowArtifactRecord {
    pub artifact_id: String,
    pub run_id: String,
    pub relative_path: String,
    pub classification: WorkflowArtifactClassification,
    pub media_type: String,
    pub byte_count: u64,
    pub sha256: String,
    pub created_at_ms: i64,
}
