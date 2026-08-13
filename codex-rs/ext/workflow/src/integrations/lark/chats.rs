use std::collections::BTreeSet;
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
    #[serde(default)]
    has_more: bool,
    page_token: Option<String>,
}

const MAX_CHAT_SEARCH_PAGES: usize = 100;

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
        Ok(self.search_chat_page(request, cancelled)?.chats)
    }

    fn search_chat_page(
        &self,
        request: &ChatSearchRequest,
        cancelled: &AtomicBool,
    ) -> Result<ChatPage, LarkCliError> {
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
        if request.managed_only {
            args.push(OsString::from("--is-manager"));
        }
        args.extend([
            OsString::from("--page-size"),
            OsString::from(request.page_size.to_string()),
        ]);
        push_option(&mut args, "--page-token", request.page_token.as_deref());
        args.extend([OsString::from("--format"), OsString::from("json")]);
        self.run_json(args, &[], cancelled)
    }

    pub fn reconcile_chat(
        &self,
        request: &ChatSearchRequest,
        expected_name: &str,
        cancelled: &AtomicBool,
    ) -> Result<ChatMatch, LarkCliError> {
        let exact = self
            .search_all_chats(request, cancelled)?
            .into_iter()
            .filter(|chat| chat.name == expected_name && chat.chat_status != "dissolved")
            .collect::<Vec<_>>();
        Ok(match exact.as_slice() {
            [] => ChatMatch::Missing,
            [chat] => ChatMatch::Found(chat.clone()),
            _ => ChatMatch::Ambiguous(exact),
        })
    }

    /// Reconciles exactly one managed, non-external group with an ownership marker.
    pub fn reconcile_owned_chat(
        &self,
        request: &ChatSearchRequest,
        expected_name: &str,
        expected_description: &str,
        expected_owner: Option<&OpenId>,
        cancelled: &AtomicBool,
    ) -> Result<ChatMatch, LarkCliError> {
        if !request.managed_only || expected_description.trim().is_empty() {
            return Err(invalid(
                "owned chat reconciliation requires managed-only search and a marker",
            ));
        }
        let exact = self
            .search_all_chats(request, cancelled)?
            .into_iter()
            .filter(|chat| {
                chat.name == expected_name
                    && chat.description == expected_description
                    && !chat.external
                    && chat.chat_status == "normal"
                    && expected_owner.is_none_or(|owner| chat.owner_id == owner.as_str())
            })
            .collect::<Vec<_>>();
        Ok(match exact.as_slice() {
            [] => ChatMatch::Missing,
            [chat] => ChatMatch::Found(chat.clone()),
            _ => ChatMatch::Ambiguous(exact),
        })
    }

    fn search_all_chats(
        &self,
        request: &ChatSearchRequest,
        cancelled: &AtomicBool,
    ) -> Result<Vec<ChatRecord>, LarkCliError> {
        let mut request = request.clone();
        let mut seen_page_tokens = BTreeSet::new();
        let mut seen_chat_ids = BTreeSet::new();
        let mut chats = Vec::new();
        for _ in 0..MAX_CHAT_SEARCH_PAGES {
            let page = self.search_chat_page(&request, cancelled)?;
            for chat in page.chats {
                if !seen_chat_ids.insert(chat.chat_id.clone()) {
                    return Err(LarkCliError::InvalidResponse {
                        message: "chat search pagination returned a duplicate".to_string(),
                    });
                }
                chats.push(chat);
            }
            if !page.has_more {
                return Ok(chats);
            }
            let next = page
                .page_token
                .filter(|token| !token.is_empty())
                .ok_or_else(|| LarkCliError::InvalidResponse {
                    message: "chat search page omitted its continuation token".to_string(),
                })?;
            if !seen_page_tokens.insert(next.clone()) {
                return Err(LarkCliError::InvalidResponse {
                    message: "chat search pagination looped".to_string(),
                });
            }
            request.page_token = Some(next);
        }
        Err(LarkCliError::InvalidResponse {
            message: "chat search exceeded its page limit".to_string(),
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
