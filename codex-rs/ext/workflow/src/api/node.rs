use std::collections::BTreeMap;

use codex_protocol::config_types::CollaborationMode;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::SandboxPolicy;
use codex_protocol::user_input::UserInput;
use codex_utils_absolute_path::AbsolutePathBuf;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use crate::NodeId;
use crate::NodeKey;
use crate::WorkflowRunId;

/// Working-directory selection for a workflow-owned Codex node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum NodeWorkingDirectory {
    /// Use the workflow launcher's working directory.
    WorkflowDefault,
    /// Use this exact validated absolute path.
    Exact(AbsolutePathBuf),
}

/// Model selection for a workflow-owned Codex node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum NodeModel {
    /// Use the workflow launcher's model.
    WorkflowDefault,
    /// Use the named model.
    Named(String),
}

/// Reasoning-effort selection for a workflow-owned Codex node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum NodeReasoningEffort {
    /// Use the workflow launcher's reasoning effort.
    WorkflowDefault,
    /// Set the exact effort, including explicitly clearing it.
    Exact(Option<ReasoningEffort>),
}

/// Sandbox selection for a workflow-owned Codex node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum NodeSandbox {
    /// Use the workflow launcher's sandbox.
    WorkflowDefault,
    /// Apply this exact sandbox policy after configuration constraints validate it.
    Policy(SandboxPolicy),
}

/// Approval behavior for a workflow-owned Codex node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum NodeApprovals {
    /// Use the workflow launcher's approval policy.
    WorkflowDefault,
    /// Keep the policy supplied by the existing app-server client.
    ExistingClient,
    /// Reject approval requests when no client is attached.
    RejectWhenDetached,
    /// Apply this exact policy after configuration constraints validate it.
    Policy(AskForApproval),
}

/// Collaboration-mode selection for turns submitted to a node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum NodeCollaborationMode {
    /// Use the workflow launcher's collaboration mode.
    WorkflowDefault,
    /// Apply this exact collaboration mode to node turns.
    Exact(CollaborationMode),
}

/// Skill authority selected for a node-local allow-list.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SkillAuthoritySelector {
    /// Provider kind such as `host`, `executor`, or `orchestrator`.
    pub kind: String,
    /// Opaque authority ID returned by the owning provider.
    pub id: String,
}

/// Opaque package selection within a skill authority.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SkillPackageSelector(pub String);

/// Whether an allowed skill should also be invoked on the initial turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillInitialInvocation {
    Available,
    InvokeOnInitialTurn,
}

/// One exact skill selection for a node.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SkillSelector {
    pub authority: SkillAuthoritySelector,
    pub package: SkillPackageSelector,
    pub initial_invocation: SkillInitialInvocation,
}

/// Per-node skill visibility policy.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "skills")]
pub enum SkillPolicy {
    #[default]
    Inherit,
    Disabled,
    AllowOnly(Vec<SkillSelector>),
}

/// Validated settings for one logical workflow node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeSpec {
    key: NodeKey,
    working_directory: NodeWorkingDirectory,
    model: NodeModel,
    reasoning_effort: NodeReasoningEffort,
    sandbox: NodeSandbox,
    approvals: NodeApprovals,
    collaboration_mode: NodeCollaborationMode,
    skills: SkillPolicy,
}

impl NodeSpec {
    /// Starts a typed builder for the stable run-local node key.
    pub fn builder(key: NodeKey) -> NodeSpecBuilder {
        NodeSpecBuilder {
            spec: Self {
                key,
                working_directory: NodeWorkingDirectory::WorkflowDefault,
                model: NodeModel::WorkflowDefault,
                reasoning_effort: NodeReasoningEffort::WorkflowDefault,
                sandbox: NodeSandbox::WorkflowDefault,
                approvals: NodeApprovals::WorkflowDefault,
                collaboration_mode: NodeCollaborationMode::WorkflowDefault,
                skills: SkillPolicy::Inherit,
            },
        }
    }

    pub fn key(&self) -> &NodeKey {
        &self.key
    }

    pub fn working_directory(&self) -> &NodeWorkingDirectory {
        &self.working_directory
    }

    pub fn model(&self) -> &NodeModel {
        &self.model
    }

    pub fn reasoning_effort(&self) -> &NodeReasoningEffort {
        &self.reasoning_effort
    }

    pub fn sandbox(&self) -> &NodeSandbox {
        &self.sandbox
    }

    pub fn approvals(&self) -> &NodeApprovals {
        &self.approvals
    }

    pub fn collaboration_mode(&self) -> &NodeCollaborationMode {
        &self.collaboration_mode
    }

    pub fn skills(&self) -> &SkillPolicy {
        &self.skills
    }
}

/// Builder for a [`NodeSpec`].
pub struct NodeSpecBuilder {
    spec: NodeSpec,
}

impl NodeSpecBuilder {
    pub fn working_directory(mut self, value: NodeWorkingDirectory) -> Self {
        self.spec.working_directory = value;
        self
    }

    pub fn model(mut self, value: NodeModel) -> Self {
        self.spec.model = value;
        self
    }

    pub fn reasoning_effort(mut self, value: NodeReasoningEffort) -> Self {
        self.spec.reasoning_effort = value;
        self
    }

    pub fn sandbox(mut self, value: NodeSandbox) -> Self {
        self.spec.sandbox = value;
        self
    }

    pub fn approvals(mut self, value: NodeApprovals) -> Self {
        self.spec.approvals = value;
        self
    }

    pub fn collaboration_mode(mut self, value: NodeCollaborationMode) -> Self {
        self.spec.collaboration_mode = value;
        self
    }

    pub fn skills(mut self, value: SkillPolicy) -> Self {
        self.spec.skills = value;
        self
    }

    pub fn build(self) -> Result<NodeSpec, NodeSpecError> {
        if let NodeModel::Named(model) = &self.spec.model
            && model.trim().is_empty()
        {
            return Err(NodeSpecError::EmptyModel);
        }
        if let SkillPolicy::AllowOnly(skills) = &self.spec.skills
            && skills.iter().any(|skill| {
                skill.authority.kind.trim().is_empty()
                    || skill.authority.id.trim().is_empty()
                    || skill.package.0.trim().is_empty()
            })
        {
            return Err(NodeSpecError::InvalidSkillSelector);
        }
        Ok(self.spec)
    }
}

/// Node specification validation failure.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NodeSpecError {
    #[error("node model must not be empty")]
    EmptyModel,
    #[error("node skill selectors must contain non-empty authority and package identifiers")]
    InvalidSkillSelector,
}

/// User-visible input and output contract for one normal Codex turn.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeInput {
    pub items: Vec<UserInput>,
    #[serde(default)]
    pub final_output_json_schema: Option<Value>,
    #[serde(default)]
    pub responsesapi_client_metadata: Option<BTreeMap<String, String>>,
}

impl NodeInput {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            items: vec![UserInput::Text {
                text: text.into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            responsesapi_client_metadata: None,
        }
    }
}

/// Durable association restored into every workflow-owned node thread.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowNodeBinding {
    pub run_id: WorkflowRunId,
    pub node_id: NodeId,
}

impl WorkflowNodeBinding {
    pub const THREAD_SOURCE_PREFIX: &'static str = "codex-workflow/v1/";

    pub fn thread_source(&self) -> String {
        format!(
            "{}{}/{}",
            Self::THREAD_SOURCE_PREFIX,
            self.run_id,
            self.node_id
        )
    }

    pub fn from_thread_source(source: &str) -> Option<Self> {
        let suffix = source.strip_prefix(Self::THREAD_SOURCE_PREFIX)?;
        let (run_id, node_id) = suffix.split_once('/')?;
        Some(Self {
            run_id: WorkflowRunId::parse(run_id).ok()?,
            node_id: NodeId::parse(node_id).ok()?,
        })
    }
}

/// Host-supplied launch attachment used before extension lifecycle callbacks run.
#[derive(Clone, Debug)]
pub struct WorkflowNodeLaunch {
    pub binding: WorkflowNodeBinding,
    pub spec: NodeSpec,
}

/// Terminal status observed after the normal app-server listener reduced the event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeTurnStatus {
    Completed,
    Interrupted,
    Failed,
}

/// Normal persisted result of one node turn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeTurnResult {
    pub turn_id: String,
    pub status: NodeTurnStatus,
    pub final_output: Option<String>,
    pub error: Option<String>,
}
