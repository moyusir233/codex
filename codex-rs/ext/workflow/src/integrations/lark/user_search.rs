use std::collections::BTreeSet;
use std::ffi::OsString;
use std::sync::atomic::AtomicBool;

use serde::Deserialize;

use super::LarkCli;
use super::LarkCliError;
use super::UserMatch;
use super::UserRecord;
use super::UserSearchRequest;
use super::error::invalid;

const MAX_IDENTITY_PAGES: usize = 100;

#[derive(Deserialize)]
struct UserPage {
    #[serde(default)]
    users: Vec<UserRecord>,
    #[serde(default)]
    has_more: bool,
    page_token: Option<String>,
}

impl LarkCli {
    /// Resolves a user claim uniquely across every search page.
    pub fn resolve_user(
        &self,
        request: &UserSearchRequest,
        cancelled: &AtomicBool,
    ) -> Result<UserMatch, LarkCliError> {
        if request.query.trim().is_empty() || !(1..=100).contains(&request.page_size) {
            return Err(invalid("user search query or page size is invalid"));
        }
        let mut page_token = request.page_token.clone();
        let mut seen_tokens = BTreeSet::new();
        let mut users = Vec::new();
        for _ in 0..MAX_IDENTITY_PAGES {
            let mut args = vec![
                OsString::from("contact"),
                OsString::from("+search-user"),
                OsString::from("--as"),
                OsString::from(request.identity.as_str()),
                OsString::from("--query"),
                OsString::from(&request.query),
                OsString::from("--page-size"),
                OsString::from(request.page_size.to_string()),
            ];
            if let Some(page_token) = &page_token {
                args.extend([OsString::from("--page-token"), OsString::from(page_token)]);
            }
            args.extend([OsString::from("--format"), OsString::from("json")]);
            let page: UserPage = self.run_json(args, &[], cancelled)?;
            for user in page.users {
                if !users
                    .iter()
                    .any(|known: &UserRecord| known.open_id == user.open_id)
                {
                    users.push(user);
                }
            }
            if !page.has_more {
                return Ok(match users.as_slice() {
                    [] => UserMatch::Missing,
                    [user] => UserMatch::Found(user.clone()),
                    _ => UserMatch::Ambiguous(users),
                });
            }
            let next = page
                .page_token
                .filter(|token| !token.is_empty())
                .ok_or_else(|| LarkCliError::InvalidResponse {
                    message: "user search page omitted its continuation token".to_string(),
                })?;
            if !seen_tokens.insert(next.clone()) {
                return Err(LarkCliError::InvalidResponse {
                    message: "user search pagination looped".to_string(),
                });
            }
            page_token = Some(next);
        }
        Err(LarkCliError::InvalidResponse {
            message: "user search exceeded its page limit".to_string(),
        })
    }
}
