use std::collections::BTreeMap;

use serde::Deserialize;
use serde::Serialize;

use super::EffectKey;

/// Closed set of reducer-authored audit event categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowAuditKind {
    StageStarted,
    StageCompleted,
    GateRequested,
    GateInvalidated,
    OperatorRequired,
    SecurityDisposition,
}

/// Journaled audit record. Values are redacted before persistence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowAuditRecord {
    pub effect_key: EffectKey,
    pub kind: WorkflowAuditKind,
    pub subject_id: Option<String>,
    pub metadata: BTreeMap<String, String>,
}
