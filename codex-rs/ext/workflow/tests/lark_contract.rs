#![cfg(unix)]
#![allow(clippy::expect_used)]

mod support;

use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use codex_state::WorkflowEffectState;
use codex_workflow_extension::EffectKey;
use codex_workflow_extension::LarkDocumentService;
use codex_workflow_extension::OwnedDocumentUpdateOutcome;
use codex_workflow_extension::OwnedDocumentUpdateRequest;
use codex_workflow_extension::WorkflowRunId;
use codex_workflow_extension::integrations::lark::ChatCreateRequest;
use codex_workflow_extension::integrations::lark::ChatId;
use codex_workflow_extension::integrations::lark::ChatMatch;
use codex_workflow_extension::integrations::lark::ChatMemberMatch;
use codex_workflow_extension::integrations::lark::ChatSearchRequest;
use codex_workflow_extension::integrations::lark::ContentSensitivity;
use codex_workflow_extension::integrations::lark::DocumentCreateRequest;
use codex_workflow_extension::integrations::lark::DocumentMatch;
use codex_workflow_extension::integrations::lark::DocumentParent;
use codex_workflow_extension::integrations::lark::DocumentUpdateMode;
use codex_workflow_extension::integrations::lark::DocumentUpdateRequest;
use codex_workflow_extension::integrations::lark::LarkCli;
use codex_workflow_extension::integrations::lark::LarkCliConfig;
use codex_workflow_extension::integrations::lark::LarkCliEnvironment;
use codex_workflow_extension::integrations::lark::LarkCliError;
use codex_workflow_extension::integrations::lark::LarkEventDecoder;
use codex_workflow_extension::integrations::lark::LarkIdentity;
use codex_workflow_extension::integrations::lark::LarkSubscriberSpec;
use codex_workflow_extension::integrations::lark::LarkWorkflowCompatibility;
use codex_workflow_extension::integrations::lark::MessageBody;
use codex_workflow_extension::integrations::lark::MessageId;
use codex_workflow_extension::integrations::lark::MessageReplyRequest;
use codex_workflow_extension::integrations::lark::MessageSendRequest;
use codex_workflow_extension::integrations::lark::MessageTarget;
use codex_workflow_extension::integrations::lark::OpenId;
use codex_workflow_extension::integrations::lark::ThreadId;
use codex_workflow_extension::integrations::lark::UserMatch;
use codex_workflow_extension::integrations::lark::UserSearchRequest;
use pretty_assertions::assert_eq;
use sha2::Digest;
use sha2::Sha256;

const DOCUMENT_CREATE: &str = include_str!("fixtures/lark/v1.0.0/document-create.json");
const DOCUMENT_FETCH: &str = include_str!("fixtures/lark/v1.0.0/document-fetch.json");
const DOCUMENT_FETCH_OTHER: &str = include_str!("fixtures/lark/v1.0.0/document-fetch-other.json");
const DOCUMENT_SEARCH_PAGE_1: &str =
    include_str!("fixtures/lark/v1.0.0/document-search-page-1.json");
const DOCUMENT_SEARCH_PAGE_2: &str =
    include_str!("fixtures/lark/v1.0.0/document-search-page-2.json");
const DOCUMENT_SEARCH_LOOP: &str = include_str!("fixtures/lark/v1.0.0/document-search-loop.json");
const DOCUMENT_UPDATE: &str = include_str!("fixtures/lark/v1.0.0/document-update.json");
const DOCUMENT_UPDATE_OVERWRITE: &str =
    include_str!("fixtures/lark/v1.0.0/document-update-overwrite.json");
const DOCUMENT_FETCH_UPDATED: &str =
    include_str!("fixtures/lark/v1.0.0/document-fetch-updated.json");
const DOCUMENT_FETCH_DRIFT: &str = include_str!("fixtures/lark/v1.0.0/document-fetch-drift.json");
const DOCUMENT_FETCH_MISMATCH: &str =
    include_str!("fixtures/lark/v1.0.0/document-fetch-mismatch.json");
const CHAT_SEARCH: &str = include_str!("fixtures/lark/v1.0.0/chat-search.json");
const CHAT_SEARCH_PAGE_2: &str = include_str!("fixtures/lark/v1.0.0/chat-search-page-2.json");
const CHAT_SEARCH_AMBIGUOUS: &str = include_str!("fixtures/lark/v1.0.0/chat-search-ambiguous.json");
const CHAT_SEARCH_LOOP: &str = include_str!("fixtures/lark/v1.0.0/chat-search-loop.json");
const CHAT_CREATE: &str = include_str!("fixtures/lark/v1.0.0/chat-create.json");
const MESSAGE_SEND: &str = include_str!("fixtures/lark/v1.0.0/message-send.json");
const MESSAGE_PAGE: &str = include_str!("fixtures/lark/v1.0.0/message-page.json");
const RAW_MESSAGE: &str = include_str!("fixtures/lark/v1.0.0/raw-message.ndjson");
const USER_PAGE_1: &str = include_str!("fixtures/lark/v1.0.0/user-page-1.json");
const USER_PAGE_2: &str = include_str!("fixtures/lark/v1.0.0/user-page-2.json");
const MEMBER_PAGE_1: &str = include_str!("fixtures/lark/v1.0.0/member-page-1.json");
const MEMBER_PAGE_2: &str = include_str!("fixtures/lark/v1.0.0/member-page-2.json");
const MEMBER_ADD: &str = include_str!("fixtures/lark/v1.0.0/member-add.json");
const MEMBER_ADD_PARTIAL: &str = include_str!("fixtures/lark/v1.0.0/member-add-partial.json");
const MEMBER_RECONCILED_PAGE_1: &str =
    include_str!("fixtures/lark/v1.0.0/member-reconciled-page-1.json");
const MEMBER_RECONCILED_PAGE_2: &str =
    include_str!("fixtures/lark/v1.0.0/member-reconciled-page-2.json");

struct FakeCli {
    temp: tempfile::TempDir,
    executable: PathBuf,
    fixture_root: PathBuf,
    arg_log: PathBuf,
}

impl FakeCli {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let fixture_root = temp.path().join("fixtures");
        std::fs::create_dir(&fixture_root)?;
        for (name, contents) in [
            ("document-create.json", DOCUMENT_CREATE),
            ("document-fetch.json", DOCUMENT_FETCH),
            ("document-fetch-other.json", DOCUMENT_FETCH_OTHER),
            ("document-search-page-1.json", DOCUMENT_SEARCH_PAGE_1),
            ("document-search-page-2.json", DOCUMENT_SEARCH_PAGE_2),
            ("document-search-loop.json", DOCUMENT_SEARCH_LOOP),
            ("document-update.json", DOCUMENT_UPDATE),
            ("document-update-overwrite.json", DOCUMENT_UPDATE_OVERWRITE),
            ("document-fetch-updated.json", DOCUMENT_FETCH_UPDATED),
            ("document-fetch-drift.json", DOCUMENT_FETCH_DRIFT),
            ("document-fetch-mismatch.json", DOCUMENT_FETCH_MISMATCH),
            ("chat-search.json", CHAT_SEARCH),
            ("chat-search-page-2.json", CHAT_SEARCH_PAGE_2),
            ("chat-search-ambiguous.json", CHAT_SEARCH_AMBIGUOUS),
            ("chat-search-loop.json", CHAT_SEARCH_LOOP),
            ("chat-create.json", CHAT_CREATE),
            ("message-send.json", MESSAGE_SEND),
            ("message-page.json", MESSAGE_PAGE),
            ("user-page-1.json", USER_PAGE_1),
            ("user-page-2.json", USER_PAGE_2),
            ("member-page-1.json", MEMBER_PAGE_1),
            ("member-page-2.json", MEMBER_PAGE_2),
            ("member-add.json", MEMBER_ADD),
            ("member-add-partial.json", MEMBER_ADD_PARTIAL),
            ("member-reconciled-page-1.json", MEMBER_RECONCILED_PAGE_1),
            ("member-reconciled-page-2.json", MEMBER_RECONCILED_PAGE_2),
        ] {
            std::fs::write(fixture_root.join(name), contents)?;
        }
        let executable = temp.path().join("lark-cli");
        std::fs::write(&executable, FAKE_CLI)?;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
        Ok(Self {
            arg_log: temp.path().join("args.log"),
            temp,
            executable,
            fixture_root,
        })
    }

    fn client(&self, mode: &str, version: &str) -> LarkCli {
        LarkCli::new(LarkCliConfig {
            executable: self.executable.clone(),
            cwd: self.temp.path().to_path_buf(),
            environment: LarkCliEnvironment::default()
                .with_value("PATH", "/usr/bin:/bin")
                .with_value("FIXTURE_ROOT", self.fixture_root.as_os_str())
                .with_value("ARG_LOG", self.arg_log.as_os_str())
                .with_value("FAKE_MODE", mode)
                .with_value("FAKE_VERSION", version)
                .with_secret("LARK_APP_SECRET", "fixture-secret"),
            timeout: Duration::from_secs(1),
            stdout_limit: 64 * 1024,
            stderr_limit: 4096,
        })
    }

    fn args(&self) -> String {
        std::fs::read_to_string(&self.arg_log).unwrap_or_default()
    }
}

#[test]
fn version_and_safe_subscription_are_exact() -> Result<(), Box<dyn std::error::Error>> {
    let fake = FakeCli::new()?;
    let client = fake.client("normal", "1.0.0");
    let (version, capabilities) = client.verify(&AtomicBool::new(false))?;
    assert_eq!("1.0.0", version.as_version().to_string());
    assert!(capabilities.documents);
    assert!(capabilities.document_revisions);
    assert!(capabilities.document_revision_aware_updates);
    assert!(capabilities.chats);
    assert!(capabilities.idempotent_messages);
    assert!(capabilities.raw_events);
    assert_eq!(
        LarkWorkflowCompatibility::Supported,
        capabilities.lark_feature_workflow_compatibility()
    );
    assert!(matches!(
        fake.client("normal", "1.0.1")
            .verify(&AtomicBool::new(false)),
        Err(LarkCliError::UnsupportedVersion { .. })
    ));

    let spec = LarkSubscriberSpec::from_cli(&client);
    let args = spec
        .args
        .iter()
        .map(|arg| arg.to_string_lossy())
        .collect::<Vec<_>>();
    assert_eq!(
        vec![
            "event",
            "+subscribe",
            "--as",
            "bot",
            "--event-types",
            "im.message.receive_v1",
            "--quiet",
        ],
        args
    );
    assert!(!args.contains(&std::borrow::Cow::Borrowed("--force")));
    Ok(())
}

#[test]
fn identity_members_and_document_revisions_are_complete_and_fail_closed()
-> Result<(), Box<dyn std::error::Error>> {
    let fake = FakeCli::new()?;
    let cancelled = AtomicBool::new(false);
    let client = fake.client("normal", "1.0.0");
    let snapshot = client.fetch_document(LarkIdentity::Bot, "doxcn_demo_01", &cancelled)?;
    assert_eq!("42", snapshot.revision_id);
    assert_eq!("synthetic reviewed body", snapshot.markdown);

    let users = client.resolve_user(
        &UserSearchRequest {
            identity: LarkIdentity::User,
            query: "Alex Example".to_string(),
            page_size: 20,
            page_token: None,
        },
        &cancelled,
    )?;
    let UserMatch::Ambiguous(users) = users else {
        panic!("two paginated candidates must remain ambiguous");
    };
    assert_eq!(2, users.len());

    let chat_id = ChatId::parse("oc_demo_01")?;
    let members = client.list_all_chat_members(LarkIdentity::Bot, &chat_id, &cancelled)?;
    assert_eq!(2, members.member_total);
    assert_eq!(2, members.members.len());
    let requested = BTreeSet::from([OpenId::parse("ou_member_03")?]);
    assert_eq!(
        Vec::<String>::new(),
        client
            .add_chat_members(LarkIdentity::Bot, &chat_id, &requested, &cancelled)?
            .invalid_id_list
    );
    assert_eq!(
        Err(LarkCliError::AmbiguousMutation),
        fake.client("partial-member", "1.0.0").add_chat_members(
            LarkIdentity::Bot,
            &chat_id,
            &requested,
            &cancelled,
        )
    );
    assert_eq!(
        fake.client("member-reconciled", "1.0.0")
            .reconcile_chat_members(LarkIdentity::Bot, &chat_id, &requested, &cancelled)?,
        ChatMemberMatch::Complete
    );
    assert!(matches!(
        client.reconcile_chat_members(LarkIdentity::Bot, &chat_id, &requested, &cancelled)?,
        ChatMemberMatch::Incomplete(missing)
            if missing == vec![OpenId::parse("ou_member_03")?]
    ));
    let args = fake.args();
    assert!(args.contains("docs +fetch --as bot --doc doxcn_demo_01"));
    assert!(args.contains("contact +search-user --as user"));
    assert!(args.contains("im chat.members get --as bot"));
    assert!(args.contains("\"succeed_type\":2"));
    Ok(())
}

#[test]
fn document_create_reconciliation_paginates_and_matches_one_exact_body()
-> Result<(), Box<dyn std::error::Error>> {
    let fake = FakeCli::new()?;
    let cancelled = AtomicBool::new(false);
    let expected_sha256 = format!("{:x}", Sha256::digest(b"synthetic reviewed body"));
    let DocumentMatch::Found(document) = fake.client("normal", "1.0.0").reconcile_document_create(
        LarkIdentity::User,
        "Codex prompt review fixture",
        &expected_sha256,
        &cancelled,
    )?
    else {
        panic!("one exact title/body digest must reconcile");
    };
    assert_eq!(document.doc_id, "doxcn_demo_01");

    assert!(matches!(
        fake.client("ambiguous-document", "1.0.0")
            .reconcile_document_create(
                LarkIdentity::User,
                "Codex prompt review fixture",
                &expected_sha256,
                &cancelled,
            )?,
        DocumentMatch::Ambiguous(documents) if documents.len() == 2
    ));
    assert!(
        fake.client("loop-document-search", "1.0.0")
            .reconcile_document_create(
                LarkIdentity::User,
                "Codex prompt review fixture",
                &expected_sha256,
                &cancelled,
            )
            .is_err()
    );
    assert!(
        fake.client("normal", "1.0.0")
            .reconcile_document_create(
                LarkIdentity::Bot,
                "Codex prompt review fixture",
                &expected_sha256,
                &cancelled,
            )
            .is_err()
    );
    let args = fake.args();
    assert!(args.contains("docs +search --as user"));
    assert!(args.contains("--page-token docs-2"));
    Ok(())
}

#[test]
fn documents_chats_and_messages_use_typed_commands() -> Result<(), Box<dyn std::error::Error>> {
    let fake = FakeCli::new()?;
    let client = fake.client("normal", "1.0.0");
    let cancelled = AtomicBool::new(false);
    let document = client.create_document(
        &DocumentCreateRequest {
            identity: LarkIdentity::Bot,
            title: Some("fixture document".to_string()),
            markdown: "synthetic reviewed body".to_string(),
            parent: Some(DocumentParent::Folder("fldcn_demo".to_string())),
            sensitivity: ContentSensitivity::NonSensitive,
        },
        &cancelled,
    )?;
    assert_eq!("doxcn_demo_01", document.doc_id);
    assert_eq!("42", document.revision_id);
    assert_eq!(
        Err(LarkCliError::AmbiguousMutation),
        fake.client("mismatched-document-create", "1.0.0")
            .create_document(
                &DocumentCreateRequest {
                    identity: LarkIdentity::Bot,
                    title: Some("fixture document".to_string()),
                    markdown: "synthetic reviewed body".to_string(),
                    parent: Some(DocumentParent::Folder("fldcn_demo".to_string())),
                    sensitivity: ContentSensitivity::NonSensitive,
                },
                &cancelled,
            )
    );
    let update = client.update_document(
        &DocumentUpdateRequest {
            identity: LarkIdentity::Bot,
            document: document.doc_id,
            mode: DocumentUpdateMode::Append,
            markdown: Some("safe append".to_string()),
            selection: None,
            new_title: None,
            sensitivity: ContentSensitivity::NonSensitive,
        },
        &cancelled,
    )?;
    assert!(update.success);

    let search = ChatSearchRequest {
        identity: LarkIdentity::Bot,
        query: Some("workflow-demo".to_string()),
        member_ids: vec![OpenId::parse("ou_allowed_01")?],
        managed_only: true,
        page_size: 20,
        page_token: None,
    };
    assert!(matches!(
        client.reconcile_chat(&search, "workflow-demo", &cancelled)?,
        ChatMatch::Found(_)
    ));
    assert!(matches!(
        client.reconcile_owned_chat(
            &search,
            "workflow-demo",
            "synthetic fixture",
            Some(&OpenId::parse("ou_owner_01")?),
            &cancelled,
        )?,
        ChatMatch::Found(_)
    ));
    assert!(matches!(
        fake.client("ambiguous-chat", "1.0.0")
            .reconcile_owned_chat(
                &search,
                "workflow-demo",
                "synthetic fixture",
                Some(&OpenId::parse("ou_owner_01")?),
                &cancelled,
            )?,
        ChatMatch::Ambiguous(chats) if chats.len() == 2
    ));
    assert!(
        fake.client("loop-chat-search", "1.0.0")
            .reconcile_owned_chat(
                &search,
                "workflow-demo",
                "synthetic fixture",
                Some(&OpenId::parse("ou_owner_01")?),
                &cancelled,
            )
            .is_err()
    );
    let created = client.create_chat(
        &ChatCreateRequest {
            name: "workflow-new".to_string(),
            description: Some("synthetic fixture".to_string()),
            current_user: Some(OpenId::parse("ou_owner_01")?),
            public: false,
            sensitivity: ContentSensitivity::NonSensitive,
        },
        &cancelled,
    )?;
    assert_eq!(ChatId::parse("oc_demo_02")?, created.chat_id);

    let sent = client.send_message(
        &MessageSendRequest {
            target: MessageTarget::Chat(ChatId::parse("oc_demo_01")?),
            body: MessageBody::Text("safe [wf:demo]".to_string()),
            idempotency_key: "wf-demo-send-01".to_string(),
            sensitivity: ContentSensitivity::NonSensitive,
        },
        &cancelled,
    )?;
    assert_eq!(MessageId::parse("om_demo_01")?, sent.message_id);
    client.reply_message(
        &MessageReplyRequest {
            message_id: sent.message_id,
            body: MessageBody::Text("safe reply".to_string()),
            in_thread: true,
            idempotency_key: "wf-demo-reply-01".to_string(),
            sensitivity: ContentSensitivity::NonSensitive,
        },
        &cancelled,
    )?;
    let page = client.list_chat_messages(&ChatId::parse("oc_demo_01")?, None, &cancelled)?;
    assert_eq!(1, page.messages.len());
    client.list_thread_messages(&ThreadId::parse("omt_demo_01")?, None, &cancelled)?;

    let args = fake.args();
    for expected in [
        "docs +create --as bot",
        "docs +update --as bot",
        "im +chat-search --as bot",
        "im +chat-create --as bot",
        "--idempotency-key wf-demo-send-01",
        "--reply-in-thread",
        "im +chat-messages-list --as bot",
        "im +threads-messages-list --as bot",
    ] {
        assert!(args.contains(expected), "missing `{expected}` in {args}");
    }
    Ok(())
}

#[tokio::test]
async fn owned_document_update_is_journaled_verified_and_replay_safe()
-> Result<(), Box<dyn std::error::Error>> {
    let fake = FakeCli::new()?;
    let (_home, runtime) = support::runtime().await;
    let run_id = WorkflowRunId::new();
    support::create_run(
        runtime.as_ref(),
        run_id,
        "lark-document-contract",
        "1.0.0",
        1,
        serde_json::json!({}),
    )
    .await;
    let service = LarkDocumentService::new(
        Arc::new(fake.client("revision-success", "1.0.0")),
        runtime.workflows().clone(),
    );
    let request = owned_update_request("document-update-success")?;
    let outcome = service
        .update_owned_document(run_id, &request, 200, &AtomicBool::new(false))
        .await?;
    let OwnedDocumentUpdateOutcome::Applied(evidence) = outcome else {
        panic!("revision-aware update must apply");
    };
    assert_eq!("43", evidence.revision_id);
    assert!(!evidence.reconciled);

    assert_eq!(
        OwnedDocumentUpdateOutcome::Applied(evidence),
        service
            .update_owned_document(run_id, &request, 201, &AtomicBool::new(false))
            .await?
    );
    let effect = runtime
        .workflows()
        .read_effect(&run_id.to_string(), "document-update-success")
        .await?
        .expect("document effect");
    assert_eq!(WorkflowEffectState::Applied, effect.state);
    assert!(
        !effect
            .request
            .to_string()
            .contains("synthetic revised body")
    );
    let journal = runtime
        .workflows()
        .events_after(&run_id.to_string(), 0, 100)
        .await?
        .into_iter()
        .filter(|event| event.entity_id.as_deref() == Some("document-update-success"))
        .map(|event| {
            (
                event.kind,
                event
                    .metadata
                    .get("state")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        vec![
            ("effect.planned".to_string(), None),
            ("effect.updated".to_string(), Some("dispatched".to_string())),
            ("effect.updated".to_string(), Some("applied".to_string())),
        ],
        journal
    );
    assert_eq!(1, fake.args().matches("docs +update").count());
    runtime.close().await;
    Ok(())
}

#[tokio::test]
async fn owned_document_update_reconciles_timeout_and_retries_only_after_fetch()
-> Result<(), Box<dyn std::error::Error>> {
    let fake = FakeCli::new()?;
    let (_home, runtime) = support::runtime().await;
    let run_id = WorkflowRunId::new();
    support::create_run(
        runtime.as_ref(),
        run_id,
        "lark-document-contract",
        "1.0.0",
        1,
        serde_json::json!({}),
    )
    .await;
    let service = LarkDocumentService::new(
        Arc::new(fake.client("revision-timeout-applied", "1.0.0")),
        runtime.workflows().clone(),
    );
    let outcome = service
        .update_owned_document(
            run_id,
            &owned_update_request("document-update-timeout-applied")?,
            210,
            &AtomicBool::new(false),
        )
        .await?;
    assert!(matches!(
        outcome,
        OwnedDocumentUpdateOutcome::Applied(evidence)
            if evidence.reconciled && evidence.revision_id == "43"
    ));
    runtime.close().await;

    let retry_fake = FakeCli::new()?;
    let (_home, runtime) = support::runtime().await;
    let run_id = WorkflowRunId::new();
    support::create_run(
        runtime.as_ref(),
        run_id,
        "lark-document-contract",
        "1.0.0",
        1,
        serde_json::json!({}),
    )
    .await;
    let service = LarkDocumentService::new(
        Arc::new(retry_fake.client("revision-timeout-retry", "1.0.0")),
        runtime.workflows().clone(),
    );
    let request = owned_update_request("document-update-timeout-retry")?;
    assert_eq!(
        OwnedDocumentUpdateOutcome::Retryable {
            reason: "document_update_not_observed".to_string(),
        },
        service
            .update_owned_document(run_id, &request, 220, &AtomicBool::new(false))
            .await?
    );
    assert!(matches!(
        service
            .update_owned_document(run_id, &request, 221, &AtomicBool::new(false))
            .await?,
        OwnedDocumentUpdateOutcome::Applied(evidence)
            if evidence.reconciled && evidence.revision_id == "43"
    ));
    let args = retry_fake.args();
    assert_eq!(2, args.matches("docs +update").count());
    assert!(args.find("docs +fetch").is_some_and(|fetch| {
        args[fetch..]
            .find("docs +update")
            .is_some_and(|update| update > 0)
    }));
    runtime.close().await;

    let exhausted_fake = FakeCli::new()?;
    let (_home, runtime) = support::runtime().await;
    let run_id = WorkflowRunId::new();
    support::create_run(
        runtime.as_ref(),
        run_id,
        "lark-document-contract",
        "1.0.0",
        1,
        serde_json::json!({}),
    )
    .await;
    let service = LarkDocumentService::new(
        Arc::new(exhausted_fake.client("revision-never-observed", "1.0.0")),
        runtime.workflows().clone(),
    );
    let request = owned_update_request("document-update-retry-exhausted")?;
    for now_ms in [240, 241] {
        assert!(matches!(
            service
                .update_owned_document(run_id, &request, now_ms, &AtomicBool::new(false))
                .await?,
            OwnedDocumentUpdateOutcome::Retryable { .. }
        ));
    }
    assert!(matches!(
        service
            .update_owned_document(run_id, &request, 242, &AtomicBool::new(false))
            .await?,
        OwnedDocumentUpdateOutcome::NeedsOperator { reason, .. }
            if reason == "document_update_retry_exhausted"
    ));
    assert_eq!(3, exhausted_fake.args().matches("docs +update").count());
    runtime.close().await;
    Ok(())
}

#[tokio::test]
async fn owned_document_update_needs_operator_on_drift_or_mismatch()
-> Result<(), Box<dyn std::error::Error>> {
    for (mode, key, reason, expected_updates) in [
        (
            "revision-drift",
            "document-update-drift",
            "unexpected_revision_drift",
            0,
        ),
        (
            "revision-mismatch",
            "document-update-mismatch",
            "unexpected_post_update_content",
            1,
        ),
        (
            "revision-post-apply-drift",
            "document-update-post-apply-drift",
            "unexpected_post_apply_drift",
            1,
        ),
    ] {
        let fake = FakeCli::new()?;
        let (_home, runtime) = support::runtime().await;
        let run_id = WorkflowRunId::new();
        support::create_run(
            runtime.as_ref(),
            run_id,
            "lark-document-contract",
            "1.0.0",
            1,
            serde_json::json!({}),
        )
        .await;
        let service = LarkDocumentService::new(
            Arc::new(fake.client(mode, "1.0.0")),
            runtime.workflows().clone(),
        );
        let request = owned_update_request(key)?;
        let first = service
            .update_owned_document(run_id, &request, 230, &AtomicBool::new(false))
            .await?;
        let outcome = if mode == "revision-post-apply-drift" {
            assert!(matches!(first, OwnedDocumentUpdateOutcome::Applied(_)));
            service
                .update_owned_document(run_id, &request, 231, &AtomicBool::new(false))
                .await?
        } else {
            first
        };
        assert!(matches!(
            outcome,
            OwnedDocumentUpdateOutcome::NeedsOperator {
                reason: actual,
                observed_revision_id: Some(_),
                observed_sha256: Some(_),
            } if actual == reason
        ));
        assert_eq!(
            expected_updates,
            fake.args().matches("docs +update").count()
        );
        assert_eq!(
            WorkflowEffectState::Ambiguous,
            runtime
                .workflows()
                .read_effect(&run_id.to_string(), key)
                .await?
                .expect("document effect")
                .state
        );
        runtime.close().await;
    }
    Ok(())
}

fn owned_update_request(
    effect_key: &str,
) -> Result<OwnedDocumentUpdateRequest, Box<dyn std::error::Error>> {
    Ok(OwnedDocumentUpdateRequest {
        effect_key: EffectKey::new(effect_key)?,
        identity: LarkIdentity::Bot,
        document: "doxcn_demo_01".to_string(),
        expected_revision_id: "42".to_string(),
        expected_sha256: format!("{:x}", Sha256::digest("synthetic reviewed body".as_bytes())),
        desired_markdown: "synthetic revised body".to_string(),
        sensitivity: ContentSensitivity::NonSensitive,
    })
}

#[test]
fn sensitivity_validation_and_diagnostics_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
    let fake = FakeCli::new()?;
    let client = fake.client("normal", "1.0.0");
    let before = fake.args();
    assert_eq!(
        Err(LarkCliError::SensitiveArgv),
        client.send_message(
            &MessageSendRequest {
                target: MessageTarget::Chat(ChatId::parse("oc_demo_01")?),
                body: MessageBody::Text("must-not-spawn".to_string()),
                idempotency_key: "wf-sensitive-01".to_string(),
                sensitivity: ContentSensitivity::Sensitive,
            },
            &AtomicBool::new(false),
        )
    );
    assert_eq!(before, fake.args());

    let error = fake
        .client("echo-error", "1.0.0")
        .send_message(
            &MessageSendRequest {
                target: MessageTarget::Chat(ChatId::parse("oc_demo_01")?),
                body: MessageBody::Text("safe-but-private-fixture".to_string()),
                idempotency_key: "wf-redact-01".to_string(),
                sensitivity: ContentSensitivity::NonSensitive,
            },
            &AtomicBool::new(false),
        )
        .expect_err("fake command should fail")
        .to_string();
    assert!(!error.contains("safe-but-private-fixture"));
    assert!(!error.contains("fixture-secret"));
    assert!(error.contains("[REDACTED]"));
    assert_eq!(
        LarkCliError::MissingScope,
        fake.client("scope", "1.0.0")
            .verify(&AtomicBool::new(false))
            .expect_err("scope failure")
    );
    Ok(())
}

#[test]
fn raw_ndjson_is_bounded_and_typed() -> Result<(), Box<dyn std::error::Error>> {
    let decoder = LarkEventDecoder::new(16 * 1024, 1024);
    let event = decoder.decode(RAW_MESSAGE.trim().as_bytes())?;
    assert_eq!("evt_demo_01", event.event_id);
    assert_eq!(ChatId::parse("oc_demo_01")?, event.chat_id);
    assert_eq!(OpenId::parse("ou_allowed_01")?, event.sender_id);
    assert_eq!("approved [wf:demo]", event.text);
    assert!(decoder.decode(&vec![b'x'; 16 * 1024 + 1]).is_err());
    Ok(())
}

const FAKE_CLI: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$ARG_LOG"
next_fetch_count() {
  count_file="$FIXTURE_ROOT/fetch-count-$FAKE_MODE"
  count=0
  if [ -f "$count_file" ]; then
    read count < "$count_file"
  fi
  count=$((count + 1))
  printf '%s\n' "$count" > "$count_file"
}
case "$FAKE_MODE" in
  scope) printf 'permission denied missing scope 99991672\n' >&2; exit 9 ;;
  echo-error)
    printf 'failed body=%s secret=%s\n' "$6" "$LARK_APP_SECRET" >&2
    exit 8
    ;;
esac
if [ "$1" = "--version" ]; then
  printf 'lark-cli version %s\n' "$FAKE_VERSION"
  exit 0
fi
case "$*" in
  *"docs +search"*)
    if [ "$FAKE_MODE" = "loop-document-search" ]; then
      /bin/cat "$FIXTURE_ROOT/document-search-loop.json"
    else
      case "$*" in
        *"--page-token docs-2"*) /bin/cat "$FIXTURE_ROOT/document-search-page-2.json" ;;
        *) /bin/cat "$FIXTURE_ROOT/document-search-page-1.json" ;;
      esac
    fi
    ;;
  *"docs +fetch"*"doxcn_other_01"*)
    if [ "$FAKE_MODE" = "ambiguous-document" ]; then
      /bin/cat "$FIXTURE_ROOT/document-fetch.json"
    else
      /bin/cat "$FIXTURE_ROOT/document-fetch-other.json"
    fi
    ;;
  *"docs +fetch"*)
    case "$FAKE_MODE" in
      mismatched-document-create) /bin/cat "$FIXTURE_ROOT/document-fetch-other.json" ;;
      revision-drift) /bin/cat "$FIXTURE_ROOT/document-fetch-drift.json" ;;
      revision-mismatch)
        next_fetch_count
        if [ "$count" -eq 1 ]; then
          /bin/cat "$FIXTURE_ROOT/document-fetch.json"
        else
          /bin/cat "$FIXTURE_ROOT/document-fetch-mismatch.json"
        fi
        ;;
      revision-timeout-retry)
        next_fetch_count
        if [ "$count" -le 3 ]; then
          /bin/cat "$FIXTURE_ROOT/document-fetch.json"
        else
          /bin/cat "$FIXTURE_ROOT/document-fetch-updated.json"
        fi
        ;;
      revision-post-apply-drift)
        next_fetch_count
        if [ "$count" -eq 1 ]; then
          /bin/cat "$FIXTURE_ROOT/document-fetch.json"
        elif [ "$count" -eq 2 ]; then
          /bin/cat "$FIXTURE_ROOT/document-fetch-updated.json"
        else
          /bin/cat "$FIXTURE_ROOT/document-fetch-drift.json"
        fi
        ;;
      revision-success|revision-timeout-applied)
        next_fetch_count
        if [ "$count" -eq 1 ]; then
          /bin/cat "$FIXTURE_ROOT/document-fetch.json"
        else
          /bin/cat "$FIXTURE_ROOT/document-fetch-updated.json"
        fi
        ;;
      revision-never-observed) /bin/cat "$FIXTURE_ROOT/document-fetch.json" ;;
      *) /bin/cat "$FIXTURE_ROOT/document-fetch.json" ;;
    esac
    ;;
  *"docs +create"*) /bin/cat "$FIXTURE_ROOT/document-create.json" ;;
  *"docs +update"*)
    case "$FAKE_MODE" in
      revision-timeout-applied) printf 'synthetic timeout\n' >&2; exit 8 ;;
      revision-timeout-retry)
        update_count_file="$FIXTURE_ROOT/update-count-$FAKE_MODE"
        update_count=0
        if [ -f "$update_count_file" ]; then
          read update_count < "$update_count_file"
        fi
        update_count=$((update_count + 1))
        printf '%s\n' "$update_count" > "$update_count_file"
        if [ "$update_count" -eq 1 ]; then
          printf 'synthetic timeout\n' >&2
          exit 8
        fi
        /bin/cat "$FIXTURE_ROOT/document-update-overwrite.json"
        ;;
      revision-success|revision-mismatch|revision-post-apply-drift|revision-never-observed)
        /bin/cat "$FIXTURE_ROOT/document-update-overwrite.json"
        ;;
      *) /bin/cat "$FIXTURE_ROOT/document-update.json" ;;
    esac
    ;;
  *"im +chat-search"*)
    if [ "$FAKE_MODE" = "loop-chat-search" ]; then
      /bin/cat "$FIXTURE_ROOT/chat-search-loop.json"
    else
      case "$*" in
        *"--page-token chats-2"*) /bin/cat "$FIXTURE_ROOT/chat-search-page-2.json" ;;
        *)
          if [ "$FAKE_MODE" = "ambiguous-chat" ]; then
            /bin/cat "$FIXTURE_ROOT/chat-search-ambiguous.json"
          else
            /bin/cat "$FIXTURE_ROOT/chat-search.json"
          fi
          ;;
      esac
    fi
    ;;
  *"im +chat-create"*) /bin/cat "$FIXTURE_ROOT/chat-create.json" ;;
  *"contact +search-user"*"users-2"*) /bin/cat "$FIXTURE_ROOT/user-page-2.json" ;;
  *"contact +search-user"*) /bin/cat "$FIXTURE_ROOT/user-page-1.json" ;;
  *"im chat.members get"*"members-2"*)
    if [ "$FAKE_MODE" = "member-reconciled" ]; then
      /bin/cat "$FIXTURE_ROOT/member-reconciled-page-2.json"
    else
      /bin/cat "$FIXTURE_ROOT/member-page-2.json"
    fi
    ;;
  *"im chat.members get"*)
    if [ "$FAKE_MODE" = "member-reconciled" ]; then
      /bin/cat "$FIXTURE_ROOT/member-reconciled-page-1.json"
    else
      /bin/cat "$FIXTURE_ROOT/member-page-1.json"
    fi
    ;;
  *"im chat.members create"*)
    if [ "$FAKE_MODE" = "partial-member" ]; then
      /bin/cat "$FIXTURE_ROOT/member-add-partial.json"
    else
      /bin/cat "$FIXTURE_ROOT/member-add.json"
    fi
    ;;
  *"im +messages-send"*|*"im +messages-reply"*)
    /bin/cat "$FIXTURE_ROOT/message-send.json"
    ;;
  *"im +chat-messages-list"*|*"im +threads-messages-list"*)
    /bin/cat "$FIXTURE_ROOT/message-page.json"
    ;;
  *) printf 'unsupported fake command\n' >&2; exit 2 ;;
esac
"#;
