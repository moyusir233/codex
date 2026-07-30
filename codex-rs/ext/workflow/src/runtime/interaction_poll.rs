use std::sync::atomic::AtomicBool;

use serde_json::Value;

use crate::InteractionId;
use crate::integrations::lark::ChatId;
use crate::integrations::lark::LarkEvent;
use crate::integrations::lark::LarkMessage;
use crate::integrations::lark::OpenId;

use super::LarkInteractionError;
use super::LarkInteractionService;

impl LarkInteractionService {
    pub(super) async fn poll_for_request_marker(
        &self,
        interaction_id: InteractionId,
        cancelled: &AtomicBool,
    ) -> Result<Option<String>, LarkInteractionError> {
        let correlation = self
            .store
            .read_lark_interaction(&interaction_id.to_string())
            .await?
            .ok_or(LarkInteractionError::MissingInteraction)?;
        let chat_id = ChatId::parse(correlation.chat_id)?;
        let mut page_token = correlation.poll_page_token.clone();
        for _ in 0..10 {
            let page = self
                .cli
                .list_chat_messages(&chat_id, page_token.as_deref(), cancelled)?;
            if let Some(message) = page.messages.iter().find(|message| {
                message_text(message).contains(&format!("[wf:{}]", correlation.correlation_token))
            }) {
                return Ok(Some(message.message_id.as_str().to_string()));
            }
            if !page.has_more {
                break;
            }
            page_token = page.page_token;
        }
        Ok(None)
    }

    pub(super) async fn poll_replies(
        &self,
        interaction_id: InteractionId,
        cancelled: &AtomicBool,
    ) -> Result<(), LarkInteractionError> {
        let correlation = self
            .store
            .read_lark_interaction(&interaction_id.to_string())
            .await?
            .ok_or(LarkInteractionError::MissingInteraction)?;
        let chat_id = ChatId::parse(correlation.chat_id)?;
        let mut page_token = correlation.poll_page_token;
        let mut watermark = correlation.watermark_ms;
        for _ in 0..10 {
            let page = self
                .cli
                .list_chat_messages(&chat_id, page_token.as_deref(), cancelled)?;
            for message in &page.messages {
                let created_at_ms = message.create_time.parse().unwrap_or_default();
                watermark = watermark.max(created_at_ms);
                let Some(event) = poll_event(message, &chat_id, created_at_ms) else {
                    continue;
                };
                let _ = self.ingest_event(&event, "lark.poll").await?;
            }
            page_token = page.page_token;
            self.store
                .update_lark_poll_cursor(
                    &interaction_id.to_string(),
                    watermark,
                    page_token.as_deref(),
                    watermark,
                )
                .await?;
            if !page.has_more {
                break;
            }
        }
        Ok(())
    }
}

fn message_text(message: &LarkMessage) -> String {
    message
        .content
        .get("text")
        .and_then(Value::as_str)
        .or_else(|| message.content.as_str())
        .unwrap_or_default()
        .to_string()
}

fn poll_event(message: &LarkMessage, chat_id: &ChatId, created_at_ms: i64) -> Option<LarkEvent> {
    let sender_id = message
        .sender
        .get("open_id")
        .or_else(|| message.sender.get("id"))
        .and_then(Value::as_str)
        .and_then(|value| OpenId::parse(value).ok())?;
    Some(LarkEvent {
        event_id: format!("poll:{}", message.message_id.as_str()),
        event_type: "im.message.receive_v1".to_string(),
        created_at_ms,
        message_id: message.message_id.clone(),
        chat_id: chat_id.clone(),
        thread_id: message
            .thread_id
            .as_ref()
            .map(|thread| thread.as_str().to_string()),
        sender_id,
        message_type: message.msg_type.clone(),
        text: message_text(message),
    })
}
