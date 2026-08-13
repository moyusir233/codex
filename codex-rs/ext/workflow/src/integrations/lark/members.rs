use std::collections::BTreeSet;
use std::ffi::OsString;
use std::sync::atomic::AtomicBool;

use serde::Deserialize;
use serde_json::json;

use super::ChatId;
use super::ChatMember;
use super::ChatMemberAddResult;
use super::ChatMemberMatch;
use super::ChatMembers;
use super::LarkCli;
use super::LarkCliError;
use super::LarkIdentity;
use super::OpenId;
use super::error::invalid;

const MAX_MEMBER_PAGES: usize = 100;

#[derive(Deserialize)]
struct ChatMemberPage {
    #[serde(default)]
    items: Vec<ChatMember>,
    #[serde(default)]
    member_total: u64,
    #[serde(default)]
    has_more: bool,
    page_token: Option<String>,
}

impl LarkCli {
    /// Fetches every chat member page and rejects truncated or looping pagination.
    pub fn list_all_chat_members(
        &self,
        identity: LarkIdentity,
        chat_id: &ChatId,
        cancelled: &AtomicBool,
    ) -> Result<ChatMembers, LarkCliError> {
        let mut page_token = None;
        let mut seen_tokens = BTreeSet::new();
        let mut members = Vec::new();
        for _ in 0..MAX_MEMBER_PAGES {
            let params = json!({
                "chat_id": chat_id.as_str(),
                "member_id_type": "open_id",
                "page_size": 100,
                "page_token": page_token,
            });
            let page: ChatMemberPage = self.run_json(
                [
                    OsString::from("im"),
                    OsString::from("chat.members"),
                    OsString::from("get"),
                    OsString::from("--as"),
                    OsString::from(identity.as_str()),
                    OsString::from("--params"),
                    json_arg(&params)?,
                    OsString::from("--format"),
                    OsString::from("json"),
                ],
                &[],
                cancelled,
            )?;
            let member_total = page.member_total;
            for member in page.items {
                if member.member_id_type != "open_id" {
                    return Err(LarkCliError::InvalidResponse {
                        message: "chat member response used another ID type".to_string(),
                    });
                }
                if members
                    .iter()
                    .any(|known: &ChatMember| known.member_id == member.member_id)
                {
                    return Err(LarkCliError::InvalidResponse {
                        message: "chat member pagination returned a duplicate".to_string(),
                    });
                }
                members.push(member);
            }
            if !page.has_more {
                if u64::try_from(members.len()).ok() != Some(member_total) {
                    return Err(LarkCliError::InvalidResponse {
                        message: "chat member pagination was incomplete".to_string(),
                    });
                }
                return Ok(ChatMembers {
                    members,
                    member_total,
                });
            }
            let next = page
                .page_token
                .filter(|token| !token.is_empty())
                .ok_or_else(|| LarkCliError::InvalidResponse {
                    message: "chat member page omitted its continuation token".to_string(),
                })?;
            if !seen_tokens.insert(next.clone()) {
                return Err(LarkCliError::InvalidResponse {
                    message: "chat member pagination looped".to_string(),
                });
            }
            page_token = Some(next);
        }
        Err(LarkCliError::InvalidResponse {
            message: "chat member pagination exceeded its page limit".to_string(),
        })
    }

    /// Adds a bounded member batch using fail-closed all-or-nothing semantics.
    pub fn add_chat_members(
        &self,
        identity: LarkIdentity,
        chat_id: &ChatId,
        members: &BTreeSet<OpenId>,
        cancelled: &AtomicBool,
    ) -> Result<ChatMemberAddResult, LarkCliError> {
        if members.is_empty() || members.len() > 50 {
            return Err(invalid("chat member add requires between 1 and 50 IDs"));
        }
        let params = json!({
            "chat_id": chat_id.as_str(),
            "member_id_type": "open_id",
            "succeed_type": 2,
        });
        let data = json!({
            "id_list": members.iter().map(OpenId::as_str).collect::<Vec<_>>(),
        });
        let result: ChatMemberAddResult = self.run_json(
            [
                OsString::from("im"),
                OsString::from("chat.members"),
                OsString::from("create"),
                OsString::from("--as"),
                OsString::from(identity.as_str()),
                OsString::from("--params"),
                json_arg(&params)?,
                OsString::from("--data"),
                json_arg(&data)?,
                OsString::from("--format"),
                OsString::from("json"),
            ],
            &[],
            cancelled,
        )?;
        if !result.invalid_id_list.is_empty()
            || !result.not_existed_id_list.is_empty()
            || !result.pending_approval_id_list.is_empty()
        {
            return Err(LarkCliError::AmbiguousMutation);
        }
        Ok(result)
    }

    /// Reconciles an ambiguous add using the complete authoritative member set.
    pub fn reconcile_chat_members(
        &self,
        identity: LarkIdentity,
        chat_id: &ChatId,
        expected: &BTreeSet<OpenId>,
        cancelled: &AtomicBool,
    ) -> Result<ChatMemberMatch, LarkCliError> {
        if expected.is_empty() {
            return Err(invalid("member reconciliation requires expected IDs"));
        }
        let current = self.list_all_chat_members(identity, chat_id, cancelled)?;
        let present = current
            .members
            .into_iter()
            .map(|member| member.member_id)
            .collect::<BTreeSet<_>>();
        let missing = expected.difference(&present).cloned().collect::<Vec<_>>();
        Ok(if missing.is_empty() {
            ChatMemberMatch::Complete
        } else {
            ChatMemberMatch::Incomplete(missing)
        })
    }
}

fn json_arg(value: &serde_json::Value) -> Result<OsString, LarkCliError> {
    serde_json::to_string(value)
        .map(OsString::from)
        .map_err(|error| invalid(error.to_string()))
}
