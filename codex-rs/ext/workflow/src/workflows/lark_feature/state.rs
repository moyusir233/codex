use serde::Deserialize;
use serde::Serialize;

use crate::ApprovalId;
use crate::ArtifactId;
use crate::InteractionId;
use crate::NodeId;
use crate::WorkflowState;

use super::ApprovalEvidenceRef;
use super::LarkFeatureArguments;
use super::LarkFeatureBootstrap;
use super::LarkFeatureStageId;
use super::StageQuestion;
use super::StageRunRecord;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LarkFeatureProgress {
    pub args: LarkFeatureArguments,
    pub requirement_generation: u32,
    pub bootstrap: LarkFeatureBootstrap,
    pub stages: Vec<StageRunRecord>,
    pub approvals: Vec<ApprovalEvidenceRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "phase")]
pub enum LarkFeatureState {
    Preflight {
        args: LarkFeatureArguments,
    },
    StageReady {
        progress: LarkFeatureProgress,
        stage: LarkFeatureStageId,
        attempt: u32,
        reply_artifact_id: Option<ArtifactId>,
    },
    StageQuestions {
        progress: LarkFeatureProgress,
        stage: LarkFeatureStageId,
        attempt: u32,
        node_id: NodeId,
        thread_id: String,
        handoff_artifact_id: ArtifactId,
        handoff_sha256: String,
        questions: Vec<StageQuestion>,
        interaction_id: Option<InteractionId>,
    },
    Approval {
        progress: LarkFeatureProgress,
        stage: LarkFeatureStageId,
        attempt: u32,
        stage_record: StageRunRecord,
        approval_id: Option<ApprovalId>,
        deadline_ms: i64,
    },
    DeliveryValidation {
        progress: LarkFeatureProgress,
        execution: StageRunRecord,
    },
}

impl WorkflowState for LarkFeatureState {
    const SCHEMA_VERSION: u32 = 1;
}
