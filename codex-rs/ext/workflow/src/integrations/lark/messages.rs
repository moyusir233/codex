use std::ffi::OsString;
use std::path::Component;
use std::sync::atomic::AtomicBool;

use super::ChatId;
use super::LarkCli;
use super::LarkCliError;
use super::MessageBody;
use super::MessagePage;
use super::MessageReplyRequest;
use super::MessageSendRequest;
use super::MessageTarget;
use super::SentMessage;
use super::ThreadId;
use super::error::invalid;

impl LarkCli {
    pub fn send_message(
        &self,
        request: &MessageSendRequest,
        cancelled: &AtomicBool,
    ) -> Result<SentMessage, LarkCliError> {
        request.sensitivity.require_argv_safe()?;
        validate_key(&request.idempotency_key)?;
        let mut args = vec![
            OsString::from("im"),
            OsString::from("+messages-send"),
            OsString::from("--as"),
            OsString::from("bot"),
        ];
        match &request.target {
            MessageTarget::Chat(chat) => push_value(&mut args, "--chat-id", chat.as_str()),
            MessageTarget::User(user) => push_value(&mut args, "--user-id", user.as_str()),
        }
        let redactions = push_body(&mut args, &request.body, &self.config.cwd)?;
        push_value(&mut args, "--idempotency-key", &request.idempotency_key);
        args.extend([OsString::from("--format"), OsString::from("json")]);
        self.run_json(args, &redactions, cancelled)
    }

    pub fn reply_message(
        &self,
        request: &MessageReplyRequest,
        cancelled: &AtomicBool,
    ) -> Result<SentMessage, LarkCliError> {
        request.sensitivity.require_argv_safe()?;
        validate_key(&request.idempotency_key)?;
        let mut args = vec![
            OsString::from("im"),
            OsString::from("+messages-reply"),
            OsString::from("--as"),
            OsString::from("bot"),
            OsString::from("--message-id"),
            OsString::from(request.message_id.as_str()),
        ];
        let redactions = push_body(&mut args, &request.body, &self.config.cwd)?;
        if request.in_thread {
            args.push(OsString::from("--reply-in-thread"));
        }
        push_value(&mut args, "--idempotency-key", &request.idempotency_key);
        args.extend([OsString::from("--format"), OsString::from("json")]);
        self.run_json(args, &redactions, cancelled)
    }

    pub fn list_chat_messages(
        &self,
        chat_id: &ChatId,
        page_token: Option<&str>,
        cancelled: &AtomicBool,
    ) -> Result<MessagePage, LarkCliError> {
        let mut args = vec![
            OsString::from("im"),
            OsString::from("+chat-messages-list"),
            OsString::from("--as"),
            OsString::from("bot"),
            OsString::from("--chat-id"),
            OsString::from(chat_id.as_str()),
            OsString::from("--sort"),
            OsString::from("asc"),
            OsString::from("--page-size"),
            OsString::from("50"),
        ];
        if let Some(page_token) = page_token {
            push_value(&mut args, "--page-token", page_token);
        }
        args.extend([OsString::from("--format"), OsString::from("json")]);
        self.run_json(args, &[], cancelled)
    }

    pub fn list_thread_messages(
        &self,
        thread_id: &ThreadId,
        page_token: Option<&str>,
        cancelled: &AtomicBool,
    ) -> Result<MessagePage, LarkCliError> {
        let mut args = vec![
            OsString::from("im"),
            OsString::from("+threads-messages-list"),
            OsString::from("--as"),
            OsString::from("bot"),
            OsString::from("--thread"),
            OsString::from(thread_id.as_str()),
            OsString::from("--sort"),
            OsString::from("asc"),
            OsString::from("--page-size"),
            OsString::from("50"),
        ];
        if let Some(page_token) = page_token {
            push_value(&mut args, "--page-token", page_token);
        }
        args.extend([OsString::from("--format"), OsString::from("json")]);
        self.run_json(args, &[], cancelled)
    }
}

fn push_body(
    args: &mut Vec<OsString>,
    body: &MessageBody,
    cwd: &std::path::Path,
) -> Result<Vec<String>, LarkCliError> {
    match body {
        MessageBody::Text(text) if !text.is_empty() => {
            push_value(args, "--text", text);
            Ok(vec![text.clone()])
        }
        MessageBody::RichPost(post) => {
            let content = serde_json::to_string(post).map_err(|error| invalid(error.to_string()))?;
            push_value(args, "--msg-type", "post");
            push_value(args, "--content", &content);
            Ok(vec![content])
        }
        MessageBody::ArtifactFile(path) => {
            if path.is_absolute()
                || path.as_os_str().is_empty()
                || path
                    .components()
                    .any(|component| !matches!(component, Component::Normal(_)))
            {
                return Err(invalid("artifact file must be a normalized relative path"));
            }
            let absolute = cwd.join(path);
            let metadata = std::fs::symlink_metadata(&absolute)
                .map_err(|_| invalid("artifact file does not exist"))?;
            if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
                return Err(invalid("artifact file must be a regular non-symlink file"));
            }
            push_value(args, "--file", path.to_string_lossy().as_ref());
            Ok(Vec::new())
        }
        MessageBody::Text(_) => Err(invalid("message text must not be empty")),
    }
}

fn validate_key(key: &str) -> Result<(), LarkCliError> {
    if key.is_empty()
        || key.len() > 128
        || !key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(invalid("invalid message idempotency key"));
    }
    Ok(())
}

fn push_value(args: &mut Vec<OsString>, flag: &str, value: &str) {
    args.extend([OsString::from(flag), OsString::from(value)]);
}
