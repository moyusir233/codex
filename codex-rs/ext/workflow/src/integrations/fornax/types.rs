use std::collections::BTreeMap;

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

/// Selects a prompt and an optional immutable revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptLookup {
    /// Resolve a published prompt by global key.
    Key {
        /// Prompt key.
        key: String,
        /// Exact committed version, or server-selected latest commit.
        version: Option<String>,
        /// Include the current full draft.
        with_draft: bool,
        /// Include commit detail.
        commit_version: Option<String>,
    },
    /// Resolve a prompt by opaque ID.
    Id {
        /// Prompt ID.
        prompt_id: String,
        /// Include the current full draft.
        with_draft: bool,
        /// Include commit detail.
        commit_version: Option<String>,
    },
}

/// Sanitized typed prompt metadata with provider-owned detail retained.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct FornaxPrompt {
    /// Opaque prompt ID.
    pub prompt_id: String,
    /// Optional global key.
    #[serde(default)]
    pub key: Option<String>,
    /// Optional display name.
    #[serde(default)]
    pub name: Option<String>,
    /// Provider-owned committed version.
    #[serde(default)]
    pub version: Option<String>,
    /// Current draft detail when requested.
    #[serde(default)]
    pub draft: Option<Value>,
    /// Commit detail when requested.
    #[serde(default)]
    pub commit: Option<Value>,
    /// Provider fields not yet stabilized by authenticated fixtures.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// One locally renderable normal-string prompt message.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct FornaxPromptMessage {
    /// Message role.
    pub role: String,
    /// Normal string template content.
    pub content: String,
}

/// Selects one skill read operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkillLookup {
    /// Read one skill by ID.
    Id {
        /// Opaque skill ID.
        skill_id: String,
        /// Optional exact commit version.
        version: Option<String>,
    },
    /// Batch-read skills by opaque IDs with an optional common version.
    Ids {
        /// Non-empty skill ID set.
        ids: Vec<String>,
        /// Optional exact version applied to every ID.
        version: Option<String>,
    },
    /// Batch-read skills by key with an optional common version.
    Keys {
        /// Non-empty skill key set.
        keys: Vec<String>,
        /// Optional exact version applied to every key.
        version: Option<String>,
    },
}

/// Sanitized typed skill metadata.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct FornaxSkill {
    /// Opaque skill ID.
    #[serde(default)]
    pub skill_id: Option<String>,
    /// Global skill key.
    pub skill_key: String,
    /// Optional display name.
    #[serde(default)]
    pub name: Option<String>,
    /// Optional version.
    #[serde(default)]
    pub version: Option<String>,
    /// Provider fields not yet stabilized by authenticated fixtures.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Absolute or relative time window accepted by trace readers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimeWindow {
    /// Recent rolling window.
    LastMinutes(u64),
    /// ISO-8601 closed interval.
    Iso8601 {
        /// Inclusive start.
        since: String,
        /// Inclusive end.
        until: String,
    },
    /// Epoch-millisecond closed interval.
    EpochMillis {
        /// Inclusive start.
        start_ms: i64,
        /// Inclusive end.
        end_ms: i64,
    },
}

/// Single-trace read parameters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceGetRequest {
    /// Exactly one trace or log identifier.
    pub lookup: TraceLookup,
    /// Optional span ID filters.
    pub span_ids: Vec<String>,
    /// Optional stable window.
    pub time_window: Option<TimeWindow>,
    /// Fetch the whole tree without input/output.
    pub tree: bool,
}

/// Selects one trace by its native ID or associated log ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TraceLookup {
    /// Native trace ID.
    TraceId(String),
    /// Associated log ID.
    LogId(String),
}

/// Multi-trace read parameters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceListRequest {
    /// Maximum traces to fetch.
    pub page_size: u32,
    /// Optional root-span filter.
    pub trace_filter: Option<String>,
    /// Optional returned-span filter.
    pub span_filter: Option<String>,
    /// Stable time window.
    pub time_window: TimeWindow,
}

/// Span-index read parameters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpanListRequest {
    /// Maximum spans to return.
    pub page_size: u32,
    /// Opaque continuation token.
    pub page_token: Option<String>,
    /// Optional span filter.
    pub span_filter: Option<String>,
    /// Stable time window.
    pub time_window: TimeWindow,
}

/// One trace/span record with stable correlation fields.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct FornaxSpan {
    /// Trace ID.
    pub trace_id: String,
    /// Span ID.
    pub span_id: String,
    /// Parent span ID when present.
    #[serde(default)]
    pub parent_span_id: Option<String>,
    /// Span name.
    #[serde(default)]
    pub span_name: Option<String>,
    /// Span type.
    #[serde(default)]
    pub span_type: Option<String>,
    /// Provider fields retained for read-only consumers.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Trace-list response normalized by the adapter.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct TracePage {
    /// Returned spans grouped only by their trace IDs.
    pub spans: Vec<FornaxSpan>,
}

/// Paginated span-index response.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct SpanPage {
    /// Returned spans.
    pub spans: Vec<FornaxSpan>,
    /// Opaque continuation token.
    #[serde(default)]
    pub next_page_token: Option<String>,
}

/// Selects one trajectory read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrajectoryLookup {
    /// One or more trace IDs.
    TraceIds(Vec<String>),
    /// Experiment ID.
    ExperimentId(String),
    /// Log ID.
    LogId(String),
}

/// Provider-owned trajectory payload keyed by stable trace correlation.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct FornaxTrajectory {
    /// Trace ID when the response exposes one.
    #[serde(default)]
    pub trace_id: Option<String>,
    /// Provider trajectory structure.
    #[serde(flatten)]
    pub detail: BTreeMap<String, Value>,
}
