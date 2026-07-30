use serde::Deserialize;
use serde::Serialize;

use crate::integrations::lark::ChatId;
use crate::integrations::lark::ContentSensitivity;
use crate::integrations::lark::OpenId;
use crate::integrations::lark::ThreadId;

use super::ArtifactId;
use super::EffectKey;
use super::InteractionId;

/// One restart-safe request for a correlated Lark reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HumanInteractionRequest {
    pub effect_key: EffectKey,
    pub prompt: String,
    pub chat_id: ChatId,
    pub thread_id: Option<ThreadId>,
    pub allowed_senders: Vec<OpenId>,
    pub deadline_ms: i64,
    pub sensitivity: ContentSensitivity,
}

/// Current durable result of a human interaction request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum HumanInteractionOutcome {
    Waiting {
        interaction_id: InteractionId,
    },
    Resolved {
        interaction_id: InteractionId,
        artifact_id: ArtifactId,
    },
    TimedOut {
        interaction_id: InteractionId,
    },
    Cancelled {
        interaction_id: InteractionId,
    },
    NeedsOperator {
        interaction_id: InteractionId,
        reason: String,
    },
}
