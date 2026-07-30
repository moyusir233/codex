//! Version-pinned `fornax-cli` adapters.

mod cli_config;
mod cli_version;
mod error;
mod prompt;
pub(crate) mod redaction;
mod skill;
mod trace_read;
mod types;

pub use cli_config::FornaxCli;
pub use cli_config::FornaxCliConfig;
pub use cli_config::FornaxCliEnvironment;
pub use cli_version::FornaxCliCapabilities;
pub use cli_version::FornaxCliVersion;
pub use error::FornaxCliError;
pub use prompt::PromptDraft;
pub use prompt::PromptDraftValidationError;
pub use prompt::PromptRenderError;
pub use prompt::render_normal_prompt;
pub use types::FornaxPrompt;
pub use types::FornaxPromptMessage;
pub use types::FornaxSkill;
pub use types::FornaxSpan;
pub use types::FornaxTrajectory;
pub use types::PromptLookup;
pub use types::SkillLookup;
pub use types::SpanListRequest;
pub use types::SpanPage;
pub use types::TimeWindow;
pub use types::TraceGetRequest;
pub use types::TraceListRequest;
pub use types::TraceLookup;
pub use types::TracePage;
pub use types::TrajectoryLookup;
