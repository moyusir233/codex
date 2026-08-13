use std::path::PathBuf;

use semver::Version;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use super::ChatId;
use super::LarkCliError;
use super::LarkIdentity;
use super::MessageId;
use super::OpenId;
use super::ThreadId;

/// Parsed, pinned `lark-cli` version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LarkCliVersion(Version);

impl LarkCliVersion {
    pub fn parse(stdout: &[u8]) -> Result<Self, LarkCliError> {
        let stdout = std::str::from_utf8(stdout).map_err(|_| LarkCliError::MalformedVersion)?;
        let version = stdout
            .trim()
            .strip_prefix("lark-cli version ")
            .ok_or(LarkCliError::MalformedVersion)?;
        let parsed = Version::parse(version).map_err(|_| LarkCliError::MalformedVersion)?;
        if parsed != Version::new(1, 0, 0) {
            return Err(LarkCliError::UnsupportedVersion {
                actual: parsed.to_string(),
            });
        }
        Ok(Self(parsed))
    }

    pub fn as_version(&self) -> &Version {
        &self.0
    }
}

/// Capabilities verified against `lark-cli 1.0.0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LarkCliCapabilities {
    pub documents: bool,
    pub document_revisions: bool,
    pub document_revision_aware_updates: bool,
    pub chats: bool,
    pub identity_search: bool,
    pub chat_members: bool,
    pub idempotent_messages: bool,
    pub raw_events: bool,
}

impl LarkCliCapabilities {
    pub(super) fn verified() -> Self {
        Self {
            documents: true,
            document_revisions: true,
            document_revision_aware_updates: true,
            chats: true,
            identity_search: true,
            chat_members: true,
            idempotent_messages: true,
            raw_events: true,
        }
    }
}

/// Whether content may cross the CLI argv boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentSensitivity {
    NonSensitive,
    Sensitive,
}

impl ContentSensitivity {
    pub(super) fn require_argv_safe(self) -> Result<(), LarkCliError> {
        match self {
            Self::NonSensitive => Ok(()),
            Self::Sensitive => Err(LarkCliError::SensitiveArgv),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentCreateRequest {
    pub identity: LarkIdentity,
    pub title: Option<String>,
    pub markdown: String,
    pub parent: Option<DocumentParent>,
    pub sensitivity: ContentSensitivity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocumentParent {
    Folder(String),
    WikiNode(String),
    WikiSpace(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct DocumentRecord {
    pub doc_id: String,
    pub doc_url: String,
    pub revision_id: String,
    #[serde(default)]
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct DocumentSnapshot {
    pub doc_id: String,
    pub doc_url: String,
    pub revision_id: String,
    pub markdown: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocumentMatch {
    Missing,
    Found(DocumentSnapshot),
    Ambiguous(Vec<DocumentSnapshot>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentUpdateMode {
    Append,
    Overwrite,
    ReplaceRange,
    ReplaceAll,
    InsertBefore,
    InsertAfter,
    DeleteRange,
}

impl DocumentUpdateMode {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Append => "append",
            Self::Overwrite => "overwrite",
            Self::ReplaceRange => "replace_range",
            Self::ReplaceAll => "replace_all",
            Self::InsertBefore => "insert_before",
            Self::InsertAfter => "insert_after",
            Self::DeleteRange => "delete_range",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentUpdateRequest {
    pub identity: LarkIdentity,
    pub document: String,
    pub mode: DocumentUpdateMode,
    pub markdown: Option<String>,
    pub selection: Option<DocumentSelection>,
    pub new_title: Option<String>,
    pub sensitivity: ContentSensitivity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocumentSelection {
    Ellipsis(String),
    Title(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct DocumentUpdateResult {
    #[serde(default)]
    pub success: bool,
    pub doc_id: Option<String>,
    pub mode: Option<String>,
    #[serde(default)]
    pub board_tokens: Vec<String>,
    pub task_id: Option<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatSearchRequest {
    pub identity: LarkIdentity,
    pub query: Option<String>,
    pub member_ids: Vec<OpenId>,
    pub managed_only: bool,
    pub page_size: u8,
    pub page_token: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct ChatRecord {
    pub chat_id: ChatId,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub owner_id: String,
    #[serde(default)]
    pub external: bool,
    #[serde(default)]
    pub chat_status: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatMatch {
    Missing,
    Found(ChatRecord),
    Ambiguous(Vec<ChatRecord>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatCreateRequest {
    pub name: String,
    pub description: Option<String>,
    pub current_user: Option<OpenId>,
    pub public: bool,
    pub sensitivity: ContentSensitivity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserSearchRequest {
    pub identity: LarkIdentity,
    pub query: String,
    pub page_size: u8,
    pub page_token: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct UserRecord {
    pub open_id: OpenId,
    pub name: String,
    pub email: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UserMatch {
    Missing,
    Found(UserRecord),
    Ambiguous(Vec<UserRecord>),
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct ChatMember {
    pub member_id: OpenId,
    pub member_id_type: String,
    pub name: String,
    pub tenant_key: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatMembers {
    pub members: Vec<ChatMember>,
    pub member_total: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct ChatMemberAddResult {
    #[serde(default)]
    pub invalid_id_list: Vec<String>,
    #[serde(default)]
    pub not_existed_id_list: Vec<String>,
    #[serde(default)]
    pub pending_approval_id_list: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatMemberMatch {
    Complete,
    Incomplete(Vec<OpenId>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageTarget {
    Chat(ChatId),
    User(OpenId),
}

#[derive(Clone, Debug, PartialEq)]
pub enum MessageBody {
    Text(String),
    RichPost(Value),
    ArtifactFile(PathBuf),
}

#[derive(Clone, Debug, PartialEq)]
pub struct MessageSendRequest {
    pub target: MessageTarget,
    pub body: MessageBody,
    pub idempotency_key: String,
    pub sensitivity: ContentSensitivity,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MessageReplyRequest {
    pub message_id: MessageId,
    pub body: MessageBody,
    pub in_thread: bool,
    pub idempotency_key: String,
    pub sensitivity: ContentSensitivity,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct SentMessage {
    pub message_id: MessageId,
    pub chat_id: ChatId,
    pub create_time: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct LarkMessage {
    pub message_id: MessageId,
    pub msg_type: String,
    pub create_time: String,
    #[serde(default)]
    pub sender: Value,
    #[serde(default)]
    pub content: Value,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub updated: bool,
    pub thread_id: Option<ThreadId>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct MessagePage {
    #[serde(default)]
    pub messages: Vec<LarkMessage>,
    #[serde(default)]
    pub total: u64,
    #[serde(default)]
    pub has_more: bool,
    pub page_token: Option<String>,
}
