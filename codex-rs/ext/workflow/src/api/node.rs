use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::num::NonZeroUsize;

use codex_protocol::ThreadId;
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

/// Exact persisted skill selection produced by materialization-time resolution.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResolvedSkillSelection {
    pub authority: SkillAuthoritySelector,
    pub package: SkillPackageSelector,
    pub name: String,
    pub invocation_path: String,
    pub initial_invocation: SkillInitialInvocation,
}

/// Per-node skill visibility policy.
///
/// This selects capabilities exposed to the node. It is not filesystem or
/// process sandbox enforcement.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "skills")]
pub enum SkillPolicy {
    #[default]
    Inherit,
    Disabled,
    AllowOnly(Vec<SkillSelector>),
}

/// Fan-in condition applied to a downstream node's persisted dependencies.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "count")]
pub enum DependencyPolicy {
    AllSucceeded,
    AllTerminal,
    AtLeast(NonZeroUsize),
}

/// Run behavior when an upstream node reaches a terminal failure.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailurePolicy {
    #[default]
    FailFast,
    ContinueIndependentBranches,
    SkipDependents,
}

/// Session choice for a classified retry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrySession {
    #[default]
    NewTurnOnSameThread,
    FreshThread,
}

/// Bounded retry delay expressed without floating-point ambiguity.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum BackoffPolicy {
    #[default]
    None,
    Fixed {
        delay_ms: u64,
    },
    Exponential {
        initial_delay_ms: u64,
        maximum_delay_ms: u64,
        jitter_percent: u8,
    },
}

/// Failure classes that a node definition explicitly allows to retry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryClassification {
    ModelTransient,
    ToolTransient,
    RateLimited,
    Interrupted,
}

/// Validated retry policy snapshotted into a durable node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub maximum_attempts: NonZeroU32,
    pub backoff: BackoffPolicy,
    pub session: RetrySession,
    pub retry_on: Vec<RetryClassification>,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            maximum_attempts: NonZeroU32::MIN,
            backoff: BackoffPolicy::None,
            session: RetrySession::NewTurnOnSameThread,
            retry_on: Vec::new(),
        }
    }
}

/// Sensitivity attached to the node's final result.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeOutputClassification {
    Public,
    #[default]
    Internal,
    Sensitive,
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
    #[serde(default)]
    resolved_skills: Vec<ResolvedSkillSelection>,
    #[serde(default)]
    retry: RetryPolicy,
    #[serde(default)]
    output_classification: NodeOutputClassification,
    #[serde(default)]
    failure_policy: FailurePolicy,
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
                resolved_skills: Vec::new(),
                retry: RetryPolicy::default(),
                output_classification: NodeOutputClassification::Internal,
                failure_policy: FailurePolicy::FailFast,
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

    pub fn resolved_skills(&self) -> &[ResolvedSkillSelection] {
        &self.resolved_skills
    }

    /// Stores exact catalog resolution in the durable node snapshot.
    pub fn with_resolved_skills(mut self, resolved: Vec<ResolvedSkillSelection>) -> Self {
        self.resolved_skills = resolved;
        self
    }

    pub fn retry(&self) -> &RetryPolicy {
        &self.retry
    }

    pub fn output_classification(&self) -> NodeOutputClassification {
        self.output_classification
    }

    pub fn failure_policy(&self) -> FailurePolicy {
        self.failure_policy
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

    pub fn retry(mut self, value: RetryPolicy) -> Self {
        self.spec.retry = value;
        self
    }

    pub fn output_classification(mut self, value: NodeOutputClassification) -> Self {
        self.spec.output_classification = value;
        self
    }

    pub fn failure_policy(mut self, value: FailurePolicy) -> Self {
        self.spec.failure_policy = value;
        self
    }

    pub fn build(self) -> Result<NodeSpec, NodeSpecError> {
        if let NodeModel::Named(model) = &self.spec.model
            && model.trim().is_empty()
        {
            return Err(NodeSpecError::EmptyModel);
        }
        if let SkillPolicy::AllowOnly(skills) = &self.spec.skills {
            if skills.iter().any(|skill| {
                skill.authority.kind.trim().is_empty()
                    || skill.authority.id.trim().is_empty()
                    || skill.package.0.trim().is_empty()
            }) {
                return Err(NodeSpecError::InvalidSkillSelector);
            }
            let mut identities = std::collections::HashSet::new();
            if skills.iter().any(|skill| {
                !identities.insert((
                    skill.authority.kind.as_str(),
                    skill.authority.id.as_str(),
                    skill.package.0.as_str(),
                ))
            }) {
                return Err(NodeSpecError::DuplicateSkillSelector);
            }
        }
        if let BackoffPolicy::Exponential {
            initial_delay_ms,
            maximum_delay_ms,
            jitter_percent,
        } = &self.spec.retry.backoff
            && (*initial_delay_ms == 0
                || maximum_delay_ms < initial_delay_ms
                || *jitter_percent > 100)
        {
            return Err(NodeSpecError::InvalidRetryBackoff);
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
    #[error("node skill selectors must not repeat an authority/package identity")]
    DuplicateSkillSelector,
    #[error("node retry backoff must be positive, capped, and use at most 100% jitter")]
    InvalidRetryBackoff,
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

/// One resumable normal Codex thread ever owned by a logical node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeThreadRef {
    pub thread_id: ThreadId,
    pub ordinal: u32,
}

/// Terminal status observed after the normal app-server listener reduced the event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeTurnStatus {
    Completed,
    Interrupted,
    Failed,
}

/// Why workflow execution requested cancellation of a node's active turn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
pub enum CancellationReason {
    WorkflowRequested,
    DependencyFailed,
    DeadlineExceeded,
    Operator(String),
}

/// Same-thread turn request used by the explicit retry operation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RetryRequest {
    pub input: NodeInput,
    pub classification: RetryClassification,
}

impl RetryRequest {
    pub fn same_thread(input: NodeInput) -> Self {
        Self {
            input,
            classification: RetryClassification::Interrupted,
        }
    }

    pub fn classified(input: NodeInput, classification: RetryClassification) -> Self {
        Self {
            input,
            classification,
        }
    }
}

/// Normal persisted result of one node turn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeTurnResult {
    pub turn_id: String,
    pub status: NodeTurnStatus,
    pub final_output: Option<String>,
    pub error: Option<String>,
}
