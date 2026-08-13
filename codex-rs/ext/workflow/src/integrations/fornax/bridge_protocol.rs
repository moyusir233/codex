use std::collections::BTreeMap;

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

pub const FORNAX_BRIDGE_PROTOCOL: u32 = 2;
pub const FORNAX_BRIDGE_VERSION: &str = "0.2.0";
pub const FORNAX_SDK_VERSION: &str = "1.0.46";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum SpanParent {
    NewTrace,
    LiveSpan { span_handle_id: Uuid },
    PersistedContext { trace_context_id: Uuid },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartSpanRequest {
    pub operation_id: Uuid,
    pub name: String,
    pub span_type: SpanType,
    pub parent: SpanParent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SpanType {
    Root,
    Prompt,
    Model,
    Agent,
    Tool,
    Retriever,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum SpanRecord {
    Tags { values: BTreeMap<String, Value> },
    Baggage { values: BTreeMap<String, String> },
    Input { value: Value },
    Output { value: Value },
    FinishTime { unix_seconds: f64 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordSpanRequest {
    pub operation_id: Uuid,
    pub record: SpanRecord,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishSpanRequest {
    pub operation_id: Uuid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartedSpan {
    pub protocol_version: u32,
    pub operation_id: Uuid,
    pub span_handle_id: Uuid,
    pub trace_context_id: Uuid,
    pub trace_id: String,
    pub span_id: String,
    pub w3c: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedSpan {
    pub protocol_version: u32,
    pub operation_id: Uuid,
    pub span_handle_id: Uuid,
    pub recorded: RecordKind,
    #[serde(default)]
    pub trace_context_id: Option<Uuid>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecordKind {
    Tags,
    Baggage,
    Input,
    Output,
    FinishTime,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishedSpan {
    pub protocol_version: u32,
    pub operation_id: Uuid,
    pub span_handle_id: Uuid,
    pub trace_context_id: Uuid,
    pub trace_id: String,
    pub span_id: String,
    pub w3c: String,
    pub state: SpanState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SpanState {
    Live,
    Finished,
    Orphaned,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpanStatus {
    pub span_handle_id: Uuid,
    pub trace_context_id: Uuid,
    pub trace_id: String,
    pub span_id: String,
    pub w3c: String,
    pub instance_id: String,
    pub state: SpanState,
    pub created_at_ms: u64,
    pub finished_at_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationState {
    NotDispatched,
    Queued,
    Running,
    Succeeded,
    Failed,
    Ambiguous,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationStatus {
    pub operation_id: Uuid,
    pub route: String,
    pub request_hash: String,
    pub state: OperationState,
    pub error_code: Option<String>,
    pub instance_id: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default)]
    pub response: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeHealth {
    pub bridge_version: String,
    pub protocol_version: u32,
    pub sdk_version: String,
    pub instance_id: String,
    pub status: HealthState,
    pub queue_depth: u32,
    pub active_span_count: u32,
    pub orphaned_span_count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HealthState {
    Ready,
    Draining,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeErrorEnvelope {
    pub error: BridgeRemoteError,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeRemoteError {
    pub code: String,
    pub message: String,
    pub operation_id: Option<Uuid>,
    pub retry: RetryDisposition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RetryDisposition {
    Safe,
    AfterStatusCheck,
    Never,
}
