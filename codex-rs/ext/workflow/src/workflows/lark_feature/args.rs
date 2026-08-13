use std::path::PathBuf;

use clap::Parser;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

use crate::ArgumentError;
use crate::WorkflowArguments;

/// Typed launch contract for `lark-rust-sdk-feature-development@1.0.0`.
#[derive(Clone, Debug, PartialEq, Eq, Parser, Serialize, Deserialize, JsonSchema)]
#[command(name = "lark-rust-sdk-feature-development")]
pub struct LarkFeatureArguments {
    /// Stable requirement identity using ASCII letters, digits, dot, dash or underscore.
    #[arg(long)]
    pub requirement_id: String,

    /// Human-readable requirement title.
    #[arg(long)]
    pub requirement_title: String,

    /// Initial self-contained requirement description.
    #[arg(long)]
    pub requirement: String,

    /// Immutable requester Lark open_id.
    #[arg(long)]
    pub requester: String,

    /// Comma-delimited immutable developer open_ids.
    #[arg(long, value_delimiter = ',', required = true)]
    pub developers: Vec<String>,

    /// Comma-delimited immutable approval principals used by all three gates.
    #[arg(long, value_delimiter = ',', required = true)]
    pub approvers: Vec<String>,

    /// Distinct-principal approval quorum for every mandatory gate.
    #[arg(long, default_value_t = 1)]
    pub approval_quorum: usize,

    /// Absolute SDK repository path used by read-only design stages.
    #[arg(long)]
    pub repository_path: PathBuf,

    /// Absolute isolated SDK worktree used only by Stage 4.
    #[arg(long)]
    pub worktree_path: PathBuf,

    /// Exact worktree branch.
    #[arg(long)]
    pub branch: String,

    /// Immutable 40-character lowercase Git base commit.
    #[arg(long)]
    pub base_commit: String,

    /// Required revisioned Lark technical-design template locator.
    #[arg(long)]
    pub design_template: String,

    /// Nonsecret Lark tenant identity.
    #[arg(long)]
    pub lark_tenant: String,

    /// Host-managed Lark credential reference, never secret bytes.
    #[arg(long)]
    pub lark_credential_ref: String,

    /// Existing authorized chat to reuse; absence requests owned-group reconciliation.
    #[arg(long)]
    pub lark_chat_id: Option<String>,

    /// Nonsecret Fornax workspace reference.
    #[arg(long)]
    pub fornax_workspace: String,

    /// Explicit Stage 4 goal objective.
    #[arg(long)]
    pub goal_objective: String,

    /// Optional explicit positive Stage 4 goal budget.
    #[arg(long)]
    pub goal_token_budget: Option<i64>,

    /// Milliseconds before each mandatory approval request expires.
    #[arg(long, default_value_t = 3_600_000)]
    pub approval_timeout_ms: u64,
}

impl WorkflowArguments for LarkFeatureArguments {
    fn validate(&self) -> Result<(), ArgumentError> {
        if self.requirement_id.is_empty()
            || self.requirement_id.len() > 128
            || !self
                .requirement_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        {
            return Err(ArgumentError::validation("invalid requirement ID"));
        }
        for (label, value) in [
            ("requirement title", self.requirement_title.as_str()),
            ("requirement", self.requirement.as_str()),
            ("branch", self.branch.as_str()),
            ("design template", self.design_template.as_str()),
            ("Lark tenant", self.lark_tenant.as_str()),
            (
                "Lark credential reference",
                self.lark_credential_ref.as_str(),
            ),
            ("Fornax workspace", self.fornax_workspace.as_str()),
            ("goal objective", self.goal_objective.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(ArgumentError::validation(format!(
                    "{label} must not be empty"
                )));
            }
        }
        if !valid_open_id(&self.requester)
            || self.developers.is_empty()
            || self.approvers.is_empty()
            || self.developers.iter().any(|value| !valid_open_id(value))
            || self.approvers.iter().any(|value| !valid_open_id(value))
        {
            return Err(ArgumentError::validation(
                "requester, developers and approvers must be immutable `ou_` open_ids",
            ));
        }
        let mut developers = self.developers.clone();
        developers.sort();
        developers.dedup();
        let mut approvers = self.approvers.clone();
        approvers.sort();
        approvers.dedup();
        if developers.len() != self.developers.len() || approvers.len() != self.approvers.len() {
            return Err(ArgumentError::validation(
                "developer and approver identities must be unique",
            ));
        }
        if self.approval_quorum == 0 || self.approval_quorum > approvers.len() {
            return Err(ArgumentError::validation(
                "approval quorum must be between one and the approver count",
            ));
        }
        if !self.repository_path.is_absolute()
            || !self.worktree_path.is_absolute()
            || self.repository_path == self.worktree_path
        {
            return Err(ArgumentError::validation(
                "repository and worktree paths must be distinct absolute paths",
            ));
        }
        if self.base_commit.len() != 40
            || !self
                .base_commit
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(ArgumentError::validation(
                "base commit must be 40 lowercase hexadecimal characters",
            ));
        }
        if self
            .lark_chat_id
            .as_ref()
            .is_some_and(|chat_id| !chat_id.starts_with("oc_"))
        {
            return Err(ArgumentError::validation(
                "Lark chat ID must start with `oc_`",
            ));
        }
        if self.goal_token_budget.is_some_and(|budget| budget <= 0) {
            return Err(ArgumentError::validation(
                "goal token budget must be positive when supplied",
            ));
        }
        if self.approval_timeout_ms == 0 {
            return Err(ArgumentError::validation(
                "approval timeout must be greater than zero",
            ));
        }
        Ok(())
    }
}

fn valid_open_id(value: &str) -> bool {
    value.starts_with("ou_") && value.len() > 3 && value.len() <= 128
}
