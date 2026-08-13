use std::collections::BTreeSet;
use std::ffi::OsString;
use std::sync::atomic::AtomicBool;

use serde::Deserialize;
use sha2::Digest;
use sha2::Sha256;

use super::DocumentMatch;
use super::LarkCli;
use super::LarkCliError;
use super::LarkIdentity;
use super::error::invalid;

const MAX_DOCUMENT_SEARCH_PAGES: usize = 100;

#[derive(Deserialize)]
struct DocumentSearchPage {
    #[serde(default)]
    results: Vec<DocumentSearchResult>,
    has_more: bool,
    page_token: Option<String>,
}

#[derive(Deserialize)]
struct DocumentSearchResult {
    title_highlighted: String,
    result_meta: DocumentSearchResultMeta,
}

#[derive(Deserialize)]
struct DocumentSearchResultMeta {
    doc_types: String,
    url: String,
}

impl LarkCli {
    /// Reconciles a document create by a run-unique exact title and body digest.
    ///
    /// The pinned search shortcut is user-only. Every page is consumed and every
    /// exact DOCX candidate is fetched before deciding; a duplicate digest remains
    /// ambiguous instead of selecting the first result.
    pub fn reconcile_document_create(
        &self,
        identity: LarkIdentity,
        exact_title: &str,
        expected_sha256: &str,
        cancelled: &AtomicBool,
    ) -> Result<DocumentMatch, LarkCliError> {
        if identity != LarkIdentity::User {
            return Err(invalid(
                "document search reconciliation requires user identity",
            ));
        }
        if exact_title.trim().is_empty() || !valid_sha256(expected_sha256) {
            return Err(invalid(
                "document reconciliation title or SHA-256 is invalid",
            ));
        }

        let mut page_token = None;
        let mut seen_page_tokens = BTreeSet::new();
        let mut seen_urls = BTreeSet::new();
        let mut matches = Vec::new();
        for _ in 0..MAX_DOCUMENT_SEARCH_PAGES {
            let mut args = vec![
                OsString::from("docs"),
                OsString::from("+search"),
                OsString::from("--as"),
                OsString::from(identity.as_str()),
                OsString::from("--query"),
                OsString::from(exact_title),
                OsString::from("--page-size"),
                OsString::from("20"),
            ];
            if let Some(page_token) = &page_token {
                args.extend([OsString::from("--page-token"), OsString::from(page_token)]);
            }
            args.extend([OsString::from("--format"), OsString::from("json")]);
            let page: DocumentSearchPage = self.run_json(args, &[], cancelled)?;
            for result in page.results {
                if result.result_meta.doc_types != "DOCX"
                    || strip_search_highlights(&result.title_highlighted) != exact_title
                {
                    continue;
                }
                if result.result_meta.url.trim().is_empty()
                    || !seen_urls.insert(result.result_meta.url.clone())
                {
                    return Err(LarkCliError::InvalidResponse {
                        message: "document search returned an empty or duplicate URL".to_string(),
                    });
                }
                let snapshot = self.fetch_document(identity, &result.result_meta.url, cancelled)?;
                if format!("{:x}", Sha256::digest(snapshot.markdown.as_bytes())) == expected_sha256
                {
                    matches.push(snapshot);
                }
            }
            if !page.has_more {
                return Ok(match matches.as_slice() {
                    [] => DocumentMatch::Missing,
                    [document] => DocumentMatch::Found(document.clone()),
                    _ => DocumentMatch::Ambiguous(matches),
                });
            }
            let next = page
                .page_token
                .filter(|token| !token.is_empty())
                .ok_or_else(|| LarkCliError::InvalidResponse {
                    message: "document search page omitted its continuation token".to_string(),
                })?;
            if !seen_page_tokens.insert(next.clone()) {
                return Err(LarkCliError::InvalidResponse {
                    message: "document search pagination looped".to_string(),
                });
            }
            page_token = Some(next);
        }
        Err(LarkCliError::InvalidResponse {
            message: "document search exceeded its page limit".to_string(),
        })
    }
}

fn strip_search_highlights(value: &str) -> String {
    value
        .replace("<h>", "")
        .replace("</h>", "")
        .replace("<hb>", "")
        .replace("</hb>", "")
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase())
}
