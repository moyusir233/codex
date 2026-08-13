use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;
use serde_json::json;

use super::SpanRecord;
use super::SpanType;

const MAX_TRACE_TEXT_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TraceContent {
    pub content_type: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TraceMessage {
    pub role: String,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TracePromptArgument {
    pub key: String,
    pub value: String,
    pub source: TracePromptArgumentSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TracePromptArgumentSource {
    Input,
    Partial,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FornaxSpanInput {
    Root {
        contents: Vec<TraceContent>,
    },
    Prompt {
        prompt_provider: String,
        prompt_key: String,
        prompt_version: String,
        templates: Vec<TraceMessage>,
        arguments: Vec<TracePromptArgument>,
    },
    Model {
        model_provider: String,
        model_name: String,
        messages: Vec<TraceMessage>,
    },
    Tool {
        tool_name: String,
        input: String,
    },
    Agent {
        agent_name: String,
        agent_run_id: String,
        input: String,
    },
    Retriever {
        retriever_provider: String,
        query: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum FornaxSpanOutput {
    Root { contents: Vec<TraceContent> },
    Prompt { prompts: Vec<TraceMessage> },
    Model { choices: Vec<TraceMessage> },
    Tool { output: Value },
    Agent { output: String },
    Retriever { documents: Vec<Value> },
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FornaxSpanSemanticError {
    #[error("required Fornax span field `{0}` is empty or unbounded")]
    InvalidField(&'static str),
    #[error("successful Fornax span cannot contain an error")]
    ErrorOnSuccess,
    #[error("failed Fornax span requires a redacted error")]
    MissingError,
    #[error("Fornax semantic payload could not be serialized")]
    Serialization,
}

impl FornaxSpanInput {
    pub fn span_type(&self) -> SpanType {
        match self {
            Self::Root { .. } => SpanType::Root,
            Self::Prompt { .. } => SpanType::Prompt,
            Self::Model { .. } => SpanType::Model,
            Self::Tool { .. } => SpanType::Tool,
            Self::Agent { .. } => SpanType::Agent,
            Self::Retriever { .. } => SpanType::Retriever,
        }
    }

    /// Produces mandatory tags followed by one SDK-compatible input record.
    pub fn into_records(self) -> Result<Vec<SpanRecord>, FornaxSpanSemanticError> {
        let mut tags = BTreeMap::new();
        let value = match self {
            Self::Root { contents } => {
                json_string(&json!({ "contents": checked_contents(contents)? }))?
            }
            Self::Prompt {
                prompt_provider,
                prompt_key,
                prompt_version,
                templates,
                arguments,
            } => {
                insert_string(&mut tags, "prompt_provider", prompt_provider)?;
                insert_string(&mut tags, "prompt_key", prompt_key)?;
                insert_string(&mut tags, "prompt_version", prompt_version)?;
                checked_messages(&templates)?;
                if arguments.is_empty() {
                    return Err(FornaxSpanSemanticError::InvalidField("arguments"));
                }
                for argument in &arguments {
                    checked("argument.key", &argument.key)?;
                    checked("argument.value", &argument.value)?;
                }
                json_string(&json!({ "templates": templates, "arguments": arguments }))?
            }
            Self::Model {
                model_provider,
                model_name,
                messages,
            } => {
                insert_string(&mut tags, "model_provider", model_provider)?;
                insert_string(&mut tags, "model_name", model_name)?;
                checked_messages(&messages)?;
                json_string(&json!({ "messages": messages }))?
            }
            Self::Tool { tool_name, input } => {
                insert_string(&mut tags, "tool_name", tool_name)?;
                checked("input", &input)?.to_string()
            }
            Self::Agent {
                agent_name,
                agent_run_id,
                input,
            } => {
                insert_string(&mut tags, "agent_name", agent_name)?;
                insert_string(&mut tags, "agent_run_id", agent_run_id)?;
                checked("input", &input)?.to_string()
            }
            Self::Retriever {
                retriever_provider,
                query,
            } => {
                insert_string(&mut tags, "retriever_provider", retriever_provider)?;
                checked("query", &query)?;
                json_string(&json!({ "query": query }))?
            }
        };
        let mut records = Vec::with_capacity(2);
        if !tags.is_empty() {
            records.push(SpanRecord::Tags { values: tags });
        }
        records.push(plain_text_record(true, value));
        Ok(records)
    }
}

impl FornaxSpanOutput {
    pub fn span_type(&self) -> SpanType {
        match self {
            Self::Root { .. } => SpanType::Root,
            Self::Prompt { .. } => SpanType::Prompt,
            Self::Model { .. } => SpanType::Model,
            Self::Tool { .. } => SpanType::Tool,
            Self::Agent { .. } => SpanType::Agent,
            Self::Retriever { .. } => SpanType::Retriever,
        }
    }

    /// Produces an output record and mandatory common status/error tags.
    pub fn into_records(
        self,
        status_code: i64,
        error: Option<String>,
    ) -> Result<Vec<SpanRecord>, FornaxSpanSemanticError> {
        match (status_code, error.as_deref()) {
            (0, Some(_)) => return Err(FornaxSpanSemanticError::ErrorOnSuccess),
            (0, None) => {}
            (_, None) => return Err(FornaxSpanSemanticError::MissingError),
            (_, Some(error)) => {
                checked("error", error)?;
            }
        }
        let output = match self {
            Self::Root { contents } => plain_text_record(
                false,
                json_string(&json!({ "contents": checked_contents(contents)? }))?,
            ),
            Self::Prompt { prompts } => {
                checked_messages(&prompts)?;
                plain_text_record(false, json_string(&json!({ "prompts": prompts }))?)
            }
            Self::Model { choices } => {
                checked_messages(&choices)?;
                plain_text_record(false, json_string(&json!({ "choices": choices }))?)
            }
            Self::Tool { output } => json_record(false, output),
            Self::Agent { output } => {
                plain_text_record(false, checked("output", &output)?.to_string())
            }
            Self::Retriever { documents } => {
                if documents.is_empty() {
                    return Err(FornaxSpanSemanticError::InvalidField("documents"));
                }
                plain_text_record(false, json_string(&json!({ "documents": documents }))?)
            }
        };
        let mut values = BTreeMap::from([("_status_code".to_string(), Value::from(status_code))]);
        if let Some(error) = error {
            values.insert("error".to_string(), Value::String(error));
        }
        Ok(vec![output, SpanRecord::Tags { values }])
    }
}

fn plain_text_record(input: bool, value: String) -> SpanRecord {
    let value = json!({
        "type": "plainText",
        "value": value,
        "classification": "nonSensitive",
        "policyGrant": true,
    });
    if input {
        SpanRecord::Input { value }
    } else {
        SpanRecord::Output { value }
    }
}

fn json_record(input: bool, value: Value) -> SpanRecord {
    let value = json!({
        "type": "json",
        "value": value,
        "classification": "nonSensitive",
        "policyGrant": true,
    });
    if input {
        SpanRecord::Input { value }
    } else {
        SpanRecord::Output { value }
    }
}

fn checked_contents(
    contents: Vec<TraceContent>,
) -> Result<Vec<TraceContent>, FornaxSpanSemanticError> {
    if contents.is_empty() {
        return Err(FornaxSpanSemanticError::InvalidField("contents"));
    }
    for content in &contents {
        if content.content_type != "text" {
            return Err(FornaxSpanSemanticError::InvalidField("content_type"));
        }
        checked("content.text", &content.text)?;
    }
    Ok(contents)
}

fn checked_messages(messages: &[TraceMessage]) -> Result<(), FornaxSpanSemanticError> {
    if messages.is_empty() {
        return Err(FornaxSpanSemanticError::InvalidField("messages"));
    }
    for message in messages {
        checked("message.role", &message.role)?;
        checked("message.content", &message.content)?;
    }
    Ok(())
}

fn insert_string(
    tags: &mut BTreeMap<String, Value>,
    key: &'static str,
    value: String,
) -> Result<(), FornaxSpanSemanticError> {
    checked(key, &value)?;
    tags.insert(key.to_string(), Value::String(value));
    Ok(())
}

fn checked<'a>(name: &'static str, value: &'a str) -> Result<&'a str, FornaxSpanSemanticError> {
    if value.is_empty()
        || value.len() > MAX_TRACE_TEXT_BYTES
        || value.chars().any(|character| character == '\0')
    {
        return Err(FornaxSpanSemanticError::InvalidField(name));
    }
    Ok(value)
}

fn json_string(value: &Value) -> Result<String, FornaxSpanSemanticError> {
    serde_json::to_string(value).map_err(|_| FornaxSpanSemanticError::Serialization)
}
