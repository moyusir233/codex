use std::ffi::OsString;

use serde::Deserialize;
use serde::Serialize;

use super::ChatId;
use super::LarkCliError;
use super::MessageId;
use super::OpenId;

/// Safe singleton subscription process specification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LarkSubscriberSpec {
    pub executable: std::path::PathBuf,
    pub args: Vec<OsString>,
    pub(crate) cwd: std::path::PathBuf,
    pub(crate) environment: std::collections::BTreeMap<OsString, OsString>,
}

impl LarkSubscriberSpec {
    pub fn from_cli(cli: &super::LarkCli) -> Self {
        Self {
            executable: cli.config.executable.clone(),
            args: vec![
                OsString::from("event"),
                OsString::from("+subscribe"),
                OsString::from("--as"),
                OsString::from("bot"),
                OsString::from("--event-types"),
                OsString::from("im.message.receive_v1"),
                OsString::from("--quiet"),
            ],
            cwd: cli.config.cwd.clone(),
            environment: cli.config.environment.values.clone(),
        }
    }
}

/// Bounded typed projection of one raw Lark IM event.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LarkEvent {
    pub event_id: String,
    pub event_type: String,
    pub created_at_ms: i64,
    pub message_id: MessageId,
    pub chat_id: ChatId,
    pub thread_id: Option<String>,
    pub sender_id: OpenId,
    pub message_type: String,
    pub text: String,
}

/// Strict decoder for raw NDJSON emitted by `event +subscribe`.
#[derive(Clone, Debug)]
pub struct LarkEventDecoder {
    maximum_line_bytes: usize,
    maximum_text_bytes: usize,
}

impl LarkEventDecoder {
    pub fn new(maximum_line_bytes: usize, maximum_text_bytes: usize) -> Self {
        Self {
            maximum_line_bytes,
            maximum_text_bytes,
        }
    }

    pub fn decode(&self, line: &[u8]) -> Result<LarkEvent, LarkCliError> {
        if line.len() > self.maximum_line_bytes {
            return Err(invalid_response("event line exceeded its configured bound"));
        }
        let raw: RawEnvelope = serde_json::from_slice(line)
            .map_err(|_| invalid_response("event line was not valid JSON"))?;
        if raw.header.event_type != "im.message.receive_v1" {
            return Err(invalid_response("event type was not registered"));
        }
        let content: TextContent = serde_json::from_str(&raw.event.message.content)
            .map_err(|_| invalid_response("message content was not valid text JSON"))?;
        if content.text.len() > self.maximum_text_bytes {
            return Err(invalid_response("message text exceeded its configured bound"));
        }
        Ok(LarkEvent {
            event_id: nonempty(raw.header.event_id, "event id")?,
            event_type: raw.header.event_type,
            created_at_ms: raw
                .header
                .create_time
                .parse()
                .map_err(|_| invalid_response("event create time was invalid"))?,
            message_id: MessageId::parse(raw.event.message.message_id)?,
            chat_id: ChatId::parse(raw.event.message.chat_id)?,
            thread_id: raw.event.message.thread_id,
            sender_id: OpenId::parse(raw.event.sender.sender_id.open_id)?,
            message_type: nonempty(raw.event.message.message_type, "message type")?,
            text: content.text,
        })
    }

    pub(crate) fn maximum_line_bytes(&self) -> usize {
        self.maximum_line_bytes
    }
}

#[derive(Deserialize)]
struct RawEnvelope {
    header: RawHeader,
    event: RawEvent,
}

#[derive(Deserialize)]
struct RawHeader {
    event_id: String,
    event_type: String,
    create_time: String,
}

#[derive(Deserialize)]
struct RawEvent {
    message: RawMessage,
    sender: RawSender,
}

#[derive(Deserialize)]
struct RawMessage {
    chat_id: String,
    content: String,
    message_id: String,
    message_type: String,
    thread_id: Option<String>,
}

#[derive(Deserialize)]
struct RawSender {
    sender_id: RawSenderId,
}

#[derive(Deserialize)]
struct RawSenderId {
    open_id: String,
}

#[derive(Deserialize)]
struct TextContent {
    text: String,
}

fn nonempty(value: String, label: &str) -> Result<String, LarkCliError> {
    if value.is_empty() {
        Err(invalid_response(&format!("{label} was empty")))
    } else {
        Ok(value)
    }
}

fn invalid_response(message: &str) -> LarkCliError {
    LarkCliError::InvalidResponse {
        message: message.to_string(),
    }
}
