use std::ffi::OsString;
use std::sync::atomic::AtomicBool;

use serde::Deserialize;

use super::ChatCreateRequest;
use super::ChatMatch;
use super::ChatRecord;
use super::ChatSearchRequest;
use super::LarkCli;
use super::LarkCliError;
use super::OpenId;
use super::error::invalid;

#[derive(Deserialize)]
struct ChatPage {
    #[serde(default)]
    chats: Vec<ChatRecord>,
}

impl LarkCli {
    pub fn search_chats(
        &self,
        request: &ChatSearchRequest,
        cancelled: &AtomicBool,
    ) -> Result<Vec<ChatRecord>, LarkCliError> {
        if request.query.as_deref().is_none_or(str::is_empty) && request.member_ids.is_empty() {
            return Err(invalid("chat search needs a query or member IDs"));
        }
        if !(1..=100).contains(&request.page_size) {
            return Err(invalid("chat page size must be between 1 and 100"));
        }
        let mut args = vec![
            OsString::from("im"),
            OsString::from("+chat-search"),
            OsString::from("--as"),
            OsString::from(request.identity.as_str()),
        ];
        push_option(&mut args, "--query", request.query.as_deref());
        if !request.member_ids.is_empty() {
            let members = request
                .member_ids
                .iter()
                .map(OpenId::as_str)
                .collect::<Vec<_>>()
                .join(",");
            push_option(&mut args, "--member-ids", Some(&members));
        }
        args.extend([
            OsString::from("--page-size"),
            OsString::from(request.page_size.to_string()),
        ]);
        push_option(&mut args, "--page-token", request.page_token.as_deref());
        args.extend([OsString::from("--format"), OsString::from("json")]);
        let page: ChatPage = self.run_json(args, &[], cancelled)?;
        Ok(page.chats)
    }

    pub fn reconcile_chat(
        &self,
        request: &ChatSearchRequest,
        expected_name: &str,
        cancelled: &AtomicBool,
    ) -> Result<ChatMatch, LarkCliError> {
        let exact = self
            .search_chats(request, cancelled)?
            .into_iter()
            .filter(|chat| chat.name == expected_name && chat.chat_status != "dissolved")
            .collect::<Vec<_>>();
        Ok(match exact.as_slice() {
            [] => ChatMatch::Missing,
            [chat] => ChatMatch::Found(chat.clone()),
            _ => ChatMatch::Ambiguous(exact),
        })
    }

    pub fn create_chat(
        &self,
        request: &ChatCreateRequest,
        cancelled: &AtomicBool,
    ) -> Result<ChatRecord, LarkCliError> {
        request.sensitivity.require_argv_safe()?;
        if request.name.is_empty() || request.name.chars().count() > 60 {
            return Err(invalid("chat name must contain 1 to 60 characters"));
        }
        if request
            .description
            .as_ref()
            .is_some_and(|description| description.chars().count() > 100)
        {
            return Err(invalid("chat description must not exceed 100 characters"));
        }
        let mut args = vec![
            OsString::from("im"),
            OsString::from("+chat-create"),
            OsString::from("--as"),
            OsString::from("bot"),
            OsString::from("--name"),
            OsString::from(&request.name),
            OsString::from("--type"),
            OsString::from(if request.public { "public" } else { "private" }),
        ];
        push_option(&mut args, "--description", request.description.as_deref());
        if let Some(user) = &request.current_user {
            push_option(&mut args, "--users", Some(user.as_str()));
        }
        args.extend([OsString::from("--format"), OsString::from("json")]);
        let mut redactions = vec![request.name.clone()];
        redactions.extend(request.description.iter().cloned());
        self.run_json(args, &redactions, cancelled)
    }
}

fn push_option(args: &mut Vec<OsString>, flag: &str, value: Option<&str>) {
    if let Some(value) = value {
        args.extend([OsString::from(flag), OsString::from(value)]);
    }
}
