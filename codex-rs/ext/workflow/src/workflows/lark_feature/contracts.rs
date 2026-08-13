use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use crate::ApprovalId;
use crate::ArtifactId;
use crate::NodeId;
use crate::WorkflowOutput;
use crate::WorkflowRunId;

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum LarkFeatureStageId {
    Requirements,
    TechnicalDesign,
    ExecPlanDesign,
    Execution,
}

impl LarkFeatureStageId {
    pub const ALL: [Self; 4] = [
        Self::Requirements,
        Self::TechnicalDesign,
        Self::ExecPlanDesign,
        Self::Execution,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Requirements => "requirements",
            Self::TechnicalDesign => "technical_design",
            Self::ExecPlanDesign => "exec_plan_design",
            Self::Execution => "execution",
        }
    }

    pub fn node_key(self) -> &'static str {
        match self {
            Self::Requirements => "stage-1-requirements",
            Self::TechnicalDesign => "stage-2-technical-design",
            Self::ExecPlanDesign => "stage-3-exec-plan-design",
            Self::Execution => "stage-4-execution",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptSnapshot {
    pub name: String,
    pub version: String,
    pub content_sha256: String,
    pub fornax_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LarkFeatureBootstrap {
    pub chat_id: String,
    pub group_owner_marker_sha256: String,
    pub required_member_ids: Vec<String>,
    pub repository_identity_sha256: String,
    pub source_manifest_artifact_id: ArtifactId,
    pub instruction_ledger_artifact_id: ArtifactId,
    pub trace_id: String,
    pub root_span_id: String,
    pub trace_context_id: String,
    pub observability_delivery_state: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalEvidenceRef {
    pub gate: String,
    pub approval_id: ApprovalId,
    pub artifact_id: ArtifactId,
    pub sha256: String,
    pub document_id: Option<String>,
    pub document_revision: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageArtifactRef {
    pub logical_name: String,
    pub artifact_id: ArtifactId,
    pub sha256: String,
    pub classification: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageQuestion {
    pub question_id: String,
    pub text: String,
    pub required_for_advance: bool,
    pub asked_to: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageHandoffV1 {
    pub schema_version: u32,
    pub workflow_run_id: WorkflowRunId,
    pub requirement_id: String,
    pub requirement_generation: u32,
    pub stage_id: LarkFeatureStageId,
    pub stage_attempt_id: String,
    pub prompt: PromptSnapshot,
    pub owning_thread_id: String,
    pub participant_ids: Vec<String>,
    pub source_artifact_ids: Vec<ArtifactId>,
    pub approved_inputs: Vec<ApprovalEvidenceRef>,
    pub repository_path: String,
    pub worktree_path: String,
    pub branch: String,
    pub base_commit: String,
    pub parent_trace_id: String,
    pub parent_span_id: String,
    pub trace_context_id: String,
    pub approval_policy_id: String,
    pub security_profile_id: String,
    pub retry_profile_id: String,
    pub tool_allowlist_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageResultV1 {
    pub schema_version: u32,
    pub stage_id: LarkFeatureStageId,
    pub stage_attempt_id: String,
    pub requirement_generation: u32,
    pub disposition: String,
    pub output: Value,
    pub produced_artifacts: Vec<StageArtifactRef>,
    #[serde(default)]
    pub questions: Vec<StageQuestion>,
    #[serde(default)]
    pub risks: Vec<Value>,
    #[serde(default)]
    pub decisions: Vec<Value>,
    #[serde(default)]
    pub validations: Vec<Value>,
    pub trace_context: Value,
    pub requested_transition: Value,
    #[serde(default)]
    pub error: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageRunRecord {
    pub stage_id: LarkFeatureStageId,
    pub stage_attempt_id: String,
    pub node_id: NodeId,
    pub thread_id: String,
    pub handoff_artifact_id: ArtifactId,
    pub handoff_sha256: String,
    pub result_artifact_id: ArtifactId,
    pub result_sha256: String,
    pub subject_artifact_id: ArtifactId,
    pub subject_sha256: String,
    pub document_id: Option<String>,
    pub document_revision: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LarkFeatureWorkflowOutput {
    pub workflow_run_id: String,
    pub requirement_id: String,
    pub requirement_generation: u32,
    pub chat_id: String,
    pub stage_node_ids: Vec<String>,
    pub stage_thread_ids: Vec<String>,
    pub requirements_artifact_id: String,
    pub technical_design_document_id: String,
    pub technical_design_revision: String,
    pub exec_plan_document_id: String,
    pub exec_plan_revision: String,
    pub approval_ids: Vec<String>,
    pub goal_id: String,
    pub validation_artifact_id: String,
    pub acceptance_artifact_id: String,
    pub completion_report_artifact_id: String,
    pub trace_id: String,
    pub observability_delivery_state: String,
}

impl WorkflowOutput for LarkFeatureWorkflowOutput {}
