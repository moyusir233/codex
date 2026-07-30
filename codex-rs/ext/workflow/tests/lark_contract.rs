#![cfg(unix)]
#![allow(clippy::expect_used)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use codex_workflow_extension::integrations::lark::ChatCreateRequest;
use codex_workflow_extension::integrations::lark::ChatId;
use codex_workflow_extension::integrations::lark::ChatMatch;
use codex_workflow_extension::integrations::lark::ChatSearchRequest;
use codex_workflow_extension::integrations::lark::ContentSensitivity;
use codex_workflow_extension::integrations::lark::DocumentCreateRequest;
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
use codex_workflow_extension::integrations::lark::MessageBody;
use codex_workflow_extension::integrations::lark::MessageId;
use codex_workflow_extension::integrations::lark::MessageReplyRequest;
use codex_workflow_extension::integrations::lark::MessageSendRequest;
use codex_workflow_extension::integrations::lark::MessageTarget;
use codex_workflow_extension::integrations::lark::OpenId;
use codex_workflow_extension::integrations::lark::ThreadId;
use pretty_assertions::assert_eq;

const DOCUMENT_CREATE: &str = include_str!("fixtures/lark/v1.0.0/document-create.json");
const DOCUMENT_UPDATE: &str = include_str!("fixtures/lark/v1.0.0/document-update.json");
const CHAT_SEARCH: &str = include_str!("fixtures/lark/v1.0.0/chat-search.json");
const CHAT_CREATE: &str = include_str!("fixtures/lark/v1.0.0/chat-create.json");
const MESSAGE_SEND: &str = include_str!("fixtures/lark/v1.0.0/message-send.json");
const MESSAGE_PAGE: &str = include_str!("fixtures/lark/v1.0.0/message-page.json");
const RAW_MESSAGE: &str = include_str!("fixtures/lark/v1.0.0/raw-message.ndjson");

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
            ("document-update.json", DOCUMENT_UPDATE),
            ("chat-search.json", CHAT_SEARCH),
            ("chat-create.json", CHAT_CREATE),
            ("message-send.json", MESSAGE_SEND),
            ("message-page.json", MESSAGE_PAGE),
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
    assert!(capabilities.chats);
    assert!(capabilities.idempotent_messages);
    assert!(capabilities.raw_events);
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
fn documents_chats_and_messages_use_typed_commands() -> Result<(), Box<dyn std::error::Error>> {
    let fake = FakeCli::new()?;
    let client = fake.client("normal", "1.0.0");
    let cancelled = AtomicBool::new(false);
    let document = client.create_document(
        &DocumentCreateRequest {
            identity: LarkIdentity::Bot,
            title: Some("fixture document".to_string()),
            markdown: "safe fixture body".to_string(),
            parent: Some(DocumentParent::Folder("fldcn_demo".to_string())),
            sensitivity: ContentSensitivity::NonSensitive,
        },
        &cancelled,
    )?;
    assert_eq!("doxcn_demo_01", document.doc_id);
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
        page_size: 20,
        page_token: None,
    };
    assert!(matches!(
        client.reconcile_chat(&search, "workflow-demo", &cancelled)?,
        ChatMatch::Found(_)
    ));
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
    let page =
        client.list_chat_messages(&ChatId::parse("oc_demo_01")?, None, &cancelled)?;
    assert_eq!(1, page.messages.len());
    client.list_thread_messages(
        &ThreadId::parse("omt_demo_01")?,
        None,
        &cancelled,
    )?;

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
  *"docs +create"*) /bin/cat "$FIXTURE_ROOT/document-create.json" ;;
  *"docs +update"*) /bin/cat "$FIXTURE_ROOT/document-update.json" ;;
  *"im +chat-search"*) /bin/cat "$FIXTURE_ROOT/chat-search.json" ;;
  *"im +chat-create"*) /bin/cat "$FIXTURE_ROOT/chat-create.json" ;;
  *"im +messages-send"*|*"im +messages-reply"*)
    /bin/cat "$FIXTURE_ROOT/message-send.json"
    ;;
  *"im +chat-messages-list"*|*"im +threads-messages-list"*)
    /bin/cat "$FIXTURE_ROOT/message-page.json"
    ;;
  *) printf 'unsupported fake command\n' >&2; exit 2 ;;
esac
"#;
