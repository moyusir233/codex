use std::ffi::OsString;
use std::sync::atomic::AtomicBool;

use super::DocumentCreateRequest;
use super::DocumentParent;
use super::DocumentRecord;
use super::DocumentSelection;
use super::DocumentUpdateMode;
use super::DocumentUpdateRequest;
use super::DocumentUpdateResult;
use super::LarkCli;
use super::LarkCliError;
use super::error::invalid;

impl LarkCli {
    pub fn create_document(
        &self,
        request: &DocumentCreateRequest,
        cancelled: &AtomicBool,
    ) -> Result<DocumentRecord, LarkCliError> {
        request.sensitivity.require_argv_safe()?;
        if request.markdown.is_empty() {
            return Err(invalid("document markdown must not be empty"));
        }
        let mut args = vec![
            OsString::from("docs"),
            OsString::from("+create"),
            OsString::from("--as"),
            OsString::from(request.identity.as_str()),
            OsString::from("--markdown"),
            OsString::from(&request.markdown),
        ];
        push_option(&mut args, "--title", request.title.as_deref());
        match &request.parent {
            Some(DocumentParent::Folder(value)) => push_option(&mut args, "--folder-token", Some(value)),
            Some(DocumentParent::WikiNode(value)) => push_option(&mut args, "--wiki-node", Some(value)),
            Some(DocumentParent::WikiSpace(value)) => push_option(&mut args, "--wiki-space", Some(value)),
            None => {}
        }
        args.extend([OsString::from("--format"), OsString::from("json")]);
        self.run_json(args, std::slice::from_ref(&request.markdown), cancelled)
    }

    pub fn update_document(
        &self,
        request: &DocumentUpdateRequest,
        cancelled: &AtomicBool,
    ) -> Result<DocumentUpdateResult, LarkCliError> {
        request.sensitivity.require_argv_safe()?;
        validate_update(request)?;
        let mut args = vec![
            OsString::from("docs"),
            OsString::from("+update"),
            OsString::from("--as"),
            OsString::from(request.identity.as_str()),
            OsString::from("--doc"),
            OsString::from(&request.document),
            OsString::from("--mode"),
            OsString::from(request.mode.as_str()),
        ];
        push_option(&mut args, "--markdown", request.markdown.as_deref());
        match &request.selection {
            Some(DocumentSelection::Ellipsis(value)) => {
                push_option(&mut args, "--selection-with-ellipsis", Some(value));
            }
            Some(DocumentSelection::Title(value)) => {
                push_option(&mut args, "--selection-by-title", Some(value));
            }
            None => {}
        }
        push_option(&mut args, "--new-title", request.new_title.as_deref());
        args.extend([OsString::from("--format"), OsString::from("json")]);
        let mut redactions = request.markdown.iter().cloned().collect::<Vec<_>>();
        redactions.extend(request.new_title.iter().cloned());
        self.run_json(args, &redactions, cancelled)
    }
}

fn validate_update(request: &DocumentUpdateRequest) -> Result<(), LarkCliError> {
    if request.document.trim().is_empty() {
        return Err(invalid("document identity must not be empty"));
    }
    let needs_markdown = request.mode != DocumentUpdateMode::DeleteRange;
    if needs_markdown != request.markdown.is_some() {
        return Err(invalid(
            "markdown is required except for delete_range, where it is forbidden",
        ));
    }
    let needs_selection = matches!(
        request.mode,
        DocumentUpdateMode::ReplaceRange
            | DocumentUpdateMode::ReplaceAll
            | DocumentUpdateMode::InsertBefore
            | DocumentUpdateMode::InsertAfter
            | DocumentUpdateMode::DeleteRange
    );
    if needs_selection != request.selection.is_some() {
        return Err(invalid("update mode has an invalid selection"));
    }
    Ok(())
}

fn push_option(args: &mut Vec<OsString>, flag: &str, value: Option<&str>) {
    if let Some(value) = value {
        args.extend([OsString::from(flag), OsString::from(value)]);
    }
}
