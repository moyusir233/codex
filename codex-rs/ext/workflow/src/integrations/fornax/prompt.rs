use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;
use std::sync::atomic::AtomicBool;

use serde_json::Value;

use super::FornaxCli;
use super::FornaxCliError;
use super::FornaxPrompt;
use super::FornaxPromptMessage;
use super::PromptLookup;

/// A complete prompt draft accepted by the pinned CLI save contract.
#[derive(Clone, Debug, PartialEq)]
pub struct PromptDraft(Value);

impl PromptDraft {
    /// Validates a complete draft object before it can overwrite remote state.
    pub fn new(value: Value) -> Result<Self, PromptDraftValidationError> {
        validate_draft(&value)?;
        Ok(Self(value))
    }

    /// Returns the complete provider draft object.
    pub fn as_value(&self) -> &Value {
        &self.0
    }
}

/// Full-draft validation failures.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PromptDraftValidationError {
    /// Required full-draft structure was absent.
    #[error("prompt draft is missing `{0}`")]
    Missing(&'static str),
    /// A field used an unverified shape or value.
    #[error("prompt draft field `{field}` is invalid: {message}")]
    Invalid {
        /// Field path.
        field: &'static str,
        /// Stable explanation.
        message: String,
    },
}

/// Limited local normal-string rendering failures.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PromptRenderError {
    /// Only the verified normal string template kind is supported locally.
    #[error("local rendering supports only normal string templates")]
    UnsupportedTemplate,
    /// A message uses multipart or placeholder semantics.
    #[error("message {index} is not a normal string message")]
    UnsupportedMessage {
        /// Zero-based message index.
        index: usize,
    },
    /// A required string variable was not supplied.
    #[error("missing prompt variable `{0}`")]
    MissingVariable(String),
    /// A supplied variable is not declared as a normal string.
    #[error("prompt variable `{0}` is not declared as a string")]
    UnsupportedVariable(String),
}

impl FornaxCli {
    /// Reads one prompt with explicit key/ID revision semantics.
    pub fn get_prompt(
        &self,
        lookup: &PromptLookup,
        cancelled: &AtomicBool,
    ) -> Result<FornaxPrompt, FornaxCliError> {
        let args = prompt_lookup_args(lookup)?;
        self.run_json(args, cancelled)
    }

    /// Saves a validated complete draft through a private mode-0600 file.
    ///
    /// This is a remote mutation. Callers must journal it before dispatch and
    /// must not blindly repeat an ambiguous invocation.
    pub fn save_prompt_draft(
        &self,
        prompt_id: &str,
        draft: &PromptDraft,
        cancelled: &AtomicBool,
    ) -> Result<Value, FornaxCliError> {
        require_non_empty("prompt_id", prompt_id)?;
        let mut file = tempfile::Builder::new()
            .prefix(".codex-fornax-draft-")
            .suffix(".json")
            .tempfile_in(&self.config.cwd)
            .map_err(draft_file_error)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.as_file()
                .set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(draft_file_error)?;
        }
        serde_json::to_writer(file.as_file_mut(), draft.as_value()).map_err(draft_file_error)?;
        file.as_file_mut().flush().map_err(draft_file_error)?;
        let draft_path = file.path().as_os_str().to_os_string();
        self.run_json(
            [
                OsString::from("prompt"),
                OsString::from("draft"),
                OsString::from("save"),
                OsString::from("--prompt-id"),
                OsString::from(prompt_id),
                OsString::from("--draft-file"),
                draft_path,
            ],
            cancelled,
        )
    }
}

/// Renders only verified `normal` string messages with `{{variable}}` placeholders.
pub fn render_normal_prompt(
    draft: &PromptDraft,
    variables: &BTreeMap<String, String>,
) -> Result<Vec<FornaxPromptMessage>, PromptRenderError> {
    let template = draft.0["detail"]["prompt_template"]
        .as_object()
        .ok_or(PromptRenderError::UnsupportedTemplate)?;
    if template.get("template_type").and_then(Value::as_str) != Some("normal") {
        return Err(PromptRenderError::UnsupportedTemplate);
    }
    let definitions = template
        .get("variable_defs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let declared = definitions
        .iter()
        .filter_map(|definition| {
            let object = definition.as_object()?;
            (object.get("type")?.as_str()? == "string")
                .then(|| object.get("key")?.as_str().map(str::to_string))
                .flatten()
        })
        .collect::<std::collections::BTreeSet<_>>();
    for key in variables.keys() {
        if !declared.contains(key) {
            return Err(PromptRenderError::UnsupportedVariable(key.clone()));
        }
    }
    let messages = template
        .get("messages")
        .and_then(Value::as_array)
        .ok_or(PromptRenderError::UnsupportedTemplate)?;
    messages
        .iter()
        .enumerate()
        .map(|(index, message)| {
            let object = message
                .as_object()
                .ok_or(PromptRenderError::UnsupportedMessage { index })?;
            if object.contains_key("parts")
                || object.get("role").and_then(Value::as_str) == Some("placeholder")
            {
                return Err(PromptRenderError::UnsupportedMessage { index });
            }
            let role = object
                .get("role")
                .and_then(Value::as_str)
                .ok_or(PromptRenderError::UnsupportedMessage { index })?;
            let content = object
                .get("content")
                .and_then(Value::as_str)
                .ok_or(PromptRenderError::UnsupportedMessage { index })?;
            Ok(FornaxPromptMessage {
                role: role.to_string(),
                content: render_string(content, variables)?,
            })
        })
        .collect()
}

fn prompt_lookup_args(lookup: &PromptLookup) -> Result<Vec<OsString>, FornaxCliError> {
    let mut args = vec![OsString::from("prompt")];
    match lookup {
        PromptLookup::Key {
            key,
            version,
            with_draft,
            commit_version,
        } => {
            require_non_empty("key", key)?;
            args.extend([
                OsString::from("get-by-key"),
                OsString::from("--key"),
                OsString::from(key),
            ]);
            push_option(&mut args, "--version", version);
            push_revision_flags(&mut args, *with_draft, commit_version);
        }
        PromptLookup::Id {
            prompt_id,
            with_draft,
            commit_version,
        } => {
            require_non_empty("prompt_id", prompt_id)?;
            args.extend([
                OsString::from("get-by-id"),
                OsString::from("--prompt-id"),
                OsString::from(prompt_id),
            ]);
            push_revision_flags(&mut args, *with_draft, commit_version);
        }
    }
    Ok(args)
}

fn push_revision_flags(
    args: &mut Vec<OsString>,
    with_draft: bool,
    commit_version: &Option<String>,
) {
    if with_draft {
        args.push(OsString::from("--with-draft"));
    }
    if let Some(version) = commit_version {
        args.extend([
            OsString::from("--with-commit"),
            OsString::from("--commit-version"),
            OsString::from(version),
        ]);
    }
}

fn push_option(args: &mut Vec<OsString>, name: &'static str, value: &Option<String>) {
    if let Some(value) = value {
        args.extend([OsString::from(name), OsString::from(value)]);
    }
}

fn require_non_empty(field: &'static str, value: &str) -> Result<(), FornaxCliError> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        Err(FornaxCliError::InvalidRequest {
            message: format!("{field} must be non-empty and contain no control characters"),
        })
    } else {
        Ok(())
    }
}

fn validate_draft(value: &Value) -> Result<(), PromptDraftValidationError> {
    let root = value
        .as_object()
        .ok_or(PromptDraftValidationError::Missing("object"))?;
    let detail = root
        .get("detail")
        .and_then(Value::as_object)
        .ok_or(PromptDraftValidationError::Missing("detail"))?;
    let template = detail
        .get("prompt_template")
        .and_then(Value::as_object)
        .ok_or(PromptDraftValidationError::Missing(
            "detail.prompt_template",
        ))?;
    if template.get("template_type").and_then(Value::as_str) != Some("normal") {
        return Err(PromptDraftValidationError::Invalid {
            field: "detail.prompt_template.template_type",
            message: "only the verified `normal` kind is supported".to_string(),
        });
    }
    let messages = template
        .get("messages")
        .and_then(Value::as_array)
        .filter(|messages| !messages.is_empty())
        .ok_or(PromptDraftValidationError::Missing(
            "detail.prompt_template.messages",
        ))?;
    for message in messages {
        let object = message
            .as_object()
            .ok_or(PromptDraftValidationError::Invalid {
                field: "detail.prompt_template.messages",
                message: "every message must be an object".to_string(),
            })?;
        let role = object.get("role").and_then(Value::as_str);
        if !matches!(role, Some("system" | "user" | "assistant" | "placeholder")) {
            return Err(PromptDraftValidationError::Invalid {
                field: "detail.prompt_template.messages.role",
                message: "unsupported role".to_string(),
            });
        }
        if object.get("content").and_then(Value::as_str).is_none()
            && object.get("parts").and_then(Value::as_array).is_none()
        {
            return Err(PromptDraftValidationError::Invalid {
                field: "detail.prompt_template.messages",
                message: "message must contain string content or parts".to_string(),
            });
        }
    }
    if let Some(definitions) = template.get("variable_defs") {
        let definitions = definitions
            .as_array()
            .ok_or(PromptDraftValidationError::Invalid {
                field: "detail.prompt_template.variable_defs",
                message: "must be an array".to_string(),
            })?;
        for definition in definitions {
            let object = definition
                .as_object()
                .ok_or(PromptDraftValidationError::Invalid {
                    field: "detail.prompt_template.variable_defs",
                    message: "every definition must be an object".to_string(),
                })?;
            if object.get("key").and_then(Value::as_str).is_none()
                || !matches!(
                    object.get("type").and_then(Value::as_str),
                    Some("string" | "multi_part" | "placeholder")
                )
            {
                return Err(PromptDraftValidationError::Invalid {
                    field: "detail.prompt_template.variable_defs",
                    message: "each definition needs a key and verified type".to_string(),
                });
            }
        }
    }
    validate_optional_array(detail, "tools")?;
    validate_optional_object(detail, "tool_call_config")?;
    validate_optional_object(detail, "model_config")?;
    validate_optional_object(detail, "skill_execute_config")?;
    if let Some(draft_info) = root.get("draft_info") {
        let draft_info = draft_info
            .as_object()
            .ok_or(PromptDraftValidationError::Invalid {
                field: "draft_info",
                message: "must be an object".to_string(),
            })?;
        if draft_info
            .get("base_version")
            .is_some_and(|version| !version.is_string())
        {
            return Err(PromptDraftValidationError::Invalid {
                field: "draft_info.base_version",
                message: "must be a string".to_string(),
            });
        }
    }
    Ok(())
}

fn validate_optional_array(
    object: &serde_json::Map<String, Value>,
    field: &'static str,
) -> Result<(), PromptDraftValidationError> {
    if object.get(field).is_some_and(|value| !value.is_array()) {
        Err(PromptDraftValidationError::Invalid {
            field,
            message: "must be an array".to_string(),
        })
    } else {
        Ok(())
    }
}

fn validate_optional_object(
    object: &serde_json::Map<String, Value>,
    field: &'static str,
) -> Result<(), PromptDraftValidationError> {
    if object.get(field).is_some_and(|value| !value.is_object()) {
        Err(PromptDraftValidationError::Invalid {
            field,
            message: "must be an object".to_string(),
        })
    } else {
        Ok(())
    }
}

fn render_string(
    template: &str,
    variables: &BTreeMap<String, String>,
) -> Result<String, PromptRenderError> {
    let mut rendered = String::new();
    let mut remainder = template;
    while let Some(start) = remainder.find("{{") {
        rendered.push_str(&remainder[..start]);
        let after_start = &remainder[start + 2..];
        let Some(end) = after_start.find("}}") else {
            return Err(PromptRenderError::UnsupportedTemplate);
        };
        let key = after_start[..end].trim();
        let value = variables
            .get(key)
            .ok_or_else(|| PromptRenderError::MissingVariable(key.to_string()))?;
        rendered.push_str(value);
        remainder = &after_start[end + 2..];
    }
    rendered.push_str(remainder);
    Ok(rendered)
}

fn draft_file_error(error: impl std::fmt::Display) -> FornaxCliError {
    FornaxCliError::DraftFile {
        message: error.to_string().chars().take(256).collect(),
    }
}
