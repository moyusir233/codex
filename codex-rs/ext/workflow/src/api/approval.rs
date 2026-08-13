use std::collections::BTreeSet;
use std::num::NonZeroUsize;

use serde::Deserialize;
use serde::Serialize;

use crate::integrations::lark::ChatId;
use crate::integrations::lark::OpenId;
use crate::integrations::lark::ThreadId;

use super::ApprovalId;
use super::ArtifactId;
use super::EffectKey;

/// Immutable content identity reviewed by an approval gate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalSubject {
    pub requirement_generation: u32,
    pub artifact_id: ArtifactId,
    pub sha256: String,
    pub document_id: Option<String>,
    pub document_revision: Option<String>,
}

/// One restart-safe request for explicit, attributable approval.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalRequest {
    pub effect_key: EffectKey,
    pub gate: String,
    pub subject: ApprovalSubject,
    pub allowed_approvers: BTreeSet<OpenId>,
    pub quorum: NonZeroUsize,
    pub chat_id: ChatId,
    pub thread_id: Option<ThreadId>,
    pub deadline_ms: i64,
    pub prompt: String,
}

/// Accepted explicit decision grammar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approve,
    ChangesRequested,
    Revoke,
}

/// Immutable evidence for one accepted approval decision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalDecisionEvidence {
    pub sender: OpenId,
    pub decision: ApprovalDecision,
    pub message_id: Option<String>,
    pub response_artifact_id: Option<ArtifactId>,
    pub reason: Option<String>,
    pub created_at_ms: i64,
}

/// Current result derived from the append-only approval history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApprovalOutcome {
    Waiting {
        approval_id: ApprovalId,
        interaction_id: super::InteractionId,
    },
    Approved {
        approval_id: ApprovalId,
        decisions: Vec<ApprovalDecisionEvidence>,
    },
    ChangesRequested {
        approval_id: ApprovalId,
        decisions: Vec<ApprovalDecisionEvidence>,
    },
    Revoked {
        approval_id: ApprovalId,
        decisions: Vec<ApprovalDecisionEvidence>,
    },
    TimedOut {
        approval_id: ApprovalId,
    },
    Cancelled {
        approval_id: ApprovalId,
    },
    NeedsOperator {
        approval_id: ApprovalId,
        reason: String,
    },
}
