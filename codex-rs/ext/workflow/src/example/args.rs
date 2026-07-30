use clap::Parser;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

use crate::ArgumentError;
use crate::WorkflowArguments;

/// Arguments for the registered `prompt-review` reference workflow.
#[derive(Clone, Debug, PartialEq, Eq, Parser, Serialize, Deserialize, JsonSchema)]
#[command(name = "prompt-review")]
pub struct PromptReviewArguments {
    /// Global Fornax prompt key.
    #[arg(long)]
    pub prompt_key: String,

    /// Number of bounded parallel reviewers.
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u8).range(1..=3))]
    pub reviewers: u8,

    /// Comma-delimited Lark open IDs allowed to answer.
    #[arg(long, value_delimiter = ',', required = true)]
    pub lark_users: Vec<String>,

    /// Existing private Lark chat used for the review request.
    #[arg(long)]
    pub lark_chat_id: String,

    /// Optional immutable committed prompt version.
    #[arg(long)]
    pub prompt_version: Option<String>,

    /// Milliseconds to wait for the correlated human reply.
    #[arg(long, default_value_t = 3_600_000)]
    pub reply_timeout_ms: u64,
}

impl WorkflowArguments for PromptReviewArguments {
    fn validate(&self) -> Result<(), ArgumentError> {
        if self.prompt_key.trim().is_empty() {
            return Err(ArgumentError::validation("prompt key must not be empty"));
        }
        if self.lark_users.is_empty() || self.lark_users.iter().any(|user| !user.starts_with("ou_"))
        {
            return Err(ArgumentError::validation(
                "at least one valid Lark `ou_` user ID is required",
            ));
        }
        if !self.lark_chat_id.starts_with("oc_") {
            return Err(ArgumentError::validation(
                "Lark chat ID must start with `oc_`",
            ));
        }
        if self.reply_timeout_ms == 0 {
            return Err(ArgumentError::validation(
                "reply timeout must be greater than zero",
            ));
        }
        Ok(())
    }
}
