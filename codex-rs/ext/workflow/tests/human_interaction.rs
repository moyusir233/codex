#![cfg(unix)]
#![allow(clippy::expect_used)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use codex_state::StateRuntime;
use codex_state::WorkflowRunCreate;
use codex_state::WorkflowRunStatus;
use codex_state::WorkflowRunTransition;
use codex_workflow_extension::EffectKey;
use codex_workflow_extension::HumanInteractionOutcome;
use codex_workflow_extension::HumanInteractionRequest;
use codex_workflow_extension::LarkEventSupervisor;
use codex_workflow_extension::LarkInteractionService;
use codex_workflow_extension::WakeCondition;
use codex_workflow_extension::WorkflowArtifactStore;
use codex_workflow_extension::WorkflowRunId;
use codex_workflow_extension::integrations::lark::ChatId;
use codex_workflow_extension::integrations::lark::ContentSensitivity;
use codex_workflow_extension::integrations::lark::LarkCli;
use codex_workflow_extension::integrations::lark::LarkCliConfig;
use codex_workflow_extension::integrations::lark::LarkCliEnvironment;
use codex_workflow_extension::integrations::lark::OpenId;
use pretty_assertions::assert_eq;
use serde_json::json;

struct FakeLark {
    home: tempfile::TempDir,
    executable: PathBuf,
    message_page: PathBuf,
    event_one: PathBuf,
    event_two: PathBuf,
}

impl FakeLark {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let home = tempfile::tempdir()?;
        let executable = home.path().join("lark-cli");
        let message_page = home.path().join("message-page.json");
        let event_one = home.path().join("event-one.json");
        let event_two = home.path().join("event-two.json");
        std::fs::write(&executable, FAKE_CLI)?;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
        write_page(&message_page, &[])?;
        std::fs::write(&event_one, raw_event("evt-supervisor-1", "om_supervisor_1"))?;
        std::fs::write(&event_two, raw_event("evt-supervisor-2", "om_supervisor_2"))?;
        Ok(Self {
            home,
            executable,
            message_page,
            event_one,
            event_two,
        })
    }

    fn client(&self) -> Arc<LarkCli> {
        Arc::new(LarkCli::new(LarkCliConfig {
            executable: self.executable.clone(),
            cwd: self.home.path().to_path_buf(),
            environment: LarkCliEnvironment::default()
                .with_value("PATH", "/usr/bin:/bin")
                .with_value("MESSAGE_PAGE", self.message_page.as_os_str())
                .with_value("EVENT_ONE", self.event_one.as_os_str())
                .with_value("EVENT_TWO", self.event_two.as_os_str()),
            timeout: Duration::from_secs(1),
            stdout_limit: 64 * 1024,
            stderr_limit: 4096,
        }))
    }
}

#[tokio::test]
async fn send_reply_timeout_restart_and_poll_resolve_exactly_once()
-> Result<(), Box<dyn std::error::Error>> {
    let fake = FakeLark::new()?;
    let codex_home = tempfile::tempdir()?;
    let run_id = WorkflowRunId::new();
    let runtime = StateRuntime::init(
        codex_home.path().to_path_buf(),
        "test-provider".to_string(),
    )
    .await?;
    create_run(&runtime, run_id).await?;
    let service = new_service(&runtime, codex_home.path(), fake.client());
    let request = interaction_request("approval", 1_000);
    let first = service
        .poll_or_request(run_id, request.clone(), 100, &AtomicBool::new(false))
        .await
        .expect("initial interaction request");
    let HumanInteractionOutcome::Waiting { interaction_id } = first else {
        panic!("first request should wait");
    };
    mark_run_waiting(&runtime, run_id, interaction_id, 110).await?;
    let correlation = runtime
        .workflows()
        .read_lark_interaction(&interaction_id.to_string())
        .await?
        .expect("Lark correlation");
    assert_eq!(Some("om_request_01"), correlation.request_message_id.as_deref());

    write_page(
        &fake.message_page,
        &[fixture_message(
            "om_wrong_sender",
            "ou_wrong_01",
            &correlation.correlation_token,
            120,
        )],
    )?;
    assert!(matches!(
        service
            .poll_or_request(run_id, request.clone(), 121, &AtomicBool::new(false))
            .await
            .expect("wrong sender poll"),
        HumanInteractionOutcome::Waiting { .. }
    ));
    drop(service);
    runtime.close().await;

    write_page(
        &fake.message_page,
        &[fixture_message(
            "om_reply_01",
            "ou_allowed_01",
            &correlation.correlation_token,
            130,
        )],
    )?;
    let reopened = StateRuntime::init(
        codex_home.path().to_path_buf(),
        "test-provider".to_string(),
    )
    .await?;
    let restarted = new_service(&reopened, codex_home.path(), fake.client());
    let resolved = restarted
        .poll_or_request(run_id, request.clone(), 131, &AtomicBool::new(false))
        .await
        .expect("restart poll");
    let HumanInteractionOutcome::Resolved { artifact_id, .. } = resolved else {
        panic!("polled reply should resolve");
    };
    assert_eq!(
        WorkflowRunStatus::Pending,
        reopened
            .workflows()
            .read_run(&run_id.to_string())
            .await?
            .expect("run")
            .status
    );
    assert_eq!(
        1,
        reopened
            .workflows()
            .list_artifacts(&run_id.to_string())
            .await?
            .len()
    );
    assert!(reopened
        .workflows()
        .read_artifact(&artifact_id.to_string())
        .await?
        .is_some());
    assert_eq!(
        resolved,
        restarted
            .poll_or_request(run_id, request, 132, &AtomicBool::new(false))
            .await?
    );

    write_page(&fake.message_page, &[])?;
    let timeout_request = interaction_request("timeout", 150);
    let timeout_wait = restarted
        .poll_or_request(
            run_id,
            timeout_request.clone(),
            140,
            &AtomicBool::new(false),
        )
        .await?;
    let HumanInteractionOutcome::Waiting {
        interaction_id: timeout_id,
    } = timeout_wait
    else {
        panic!("timeout request should initially wait");
    };
    mark_run_waiting(&reopened, run_id, timeout_id, 145).await?;
    assert!(matches!(
        restarted
            .poll_or_request(run_id, timeout_request, 151, &AtomicBool::new(false))
            .await?,
        HumanInteractionOutcome::TimedOut { .. }
    ));
    reopened.close().await;
    Ok(())
}

#[tokio::test]
async fn singleton_subscriber_survives_one_waiter_cancellation()
-> Result<(), Box<dyn std::error::Error>> {
    let fake = FakeLark::new()?;
    let codex_home = tempfile::tempdir()?;
    let runtime = StateRuntime::init(
        codex_home.path().to_path_buf(),
        "test-provider".to_string(),
    )
    .await?;
    let service = Arc::new(new_service(&runtime, codex_home.path(), fake.client()));
    let supervisor = LarkEventSupervisor::new(
        service,
        16 * 1024,
        1024,
        Duration::from_millis(20),
    );
    let mut cancelled_waiter = supervisor.subscribe();
    let mut live_waiter = supervisor.subscribe();
    assert!(supervisor.ensure().await);
    assert!(!supervisor.ensure().await);
    let first = tokio::time::timeout(Duration::from_secs(2), live_waiter.recv()).await??;
    assert_eq!("evt-supervisor-1", first.event_id);
    let _ = cancelled_waiter.try_recv();
    drop(cancelled_waiter);
    let second = tokio::time::timeout(Duration::from_secs(2), live_waiter.recv()).await??;
    assert_eq!("evt-supervisor-2", second.event_id);
    let reconnected = tokio::time::timeout(Duration::from_secs(2), live_waiter.recv()).await??;
    assert_eq!("evt-supervisor-1", reconnected.event_id);
    assert!(supervisor.is_running().await);
    supervisor.stop().await?;
    assert!(!supervisor.is_running().await);
    runtime.close().await;
    Ok(())
}

fn new_service(
    runtime: &StateRuntime,
    home: &std::path::Path,
    cli: Arc<LarkCli>,
) -> LarkInteractionService {
    LarkInteractionService::new(
        cli,
        runtime.workflows().clone(),
        WorkflowArtifactStore::new(home, runtime.workflows().clone()),
    )
}

async fn create_run(
    runtime: &StateRuntime,
    run_id: WorkflowRunId,
) -> Result<(), Box<dyn std::error::Error>> {
    runtime
        .workflows()
        .create_run(WorkflowRunCreate {
            run_id: run_id.to_string(),
            definition_name: "test-workflow".to_string(),
            definition_version: "1.0.0".to_string(),
            state_schema_version: 1,
            state: json!({"step": 0}),
            arguments: json!({}),
            non_interactive: false,
            detached: false,
            concurrency: None,
            created_at_ms: 10,
        })
        .await?;
    Ok(())
}

async fn mark_run_waiting(
    runtime: &StateRuntime,
    run_id: WorkflowRunId,
    interaction_id: codex_workflow_extension::InteractionId,
    now_ms: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    let store = runtime.workflows();
    let lease = store
        .acquire_lease(&run_id.to_string(), "test-owner", now_ms, 100)
        .await?
        .expect("lease");
    let run = store
        .read_run(&run_id.to_string())
        .await?
        .expect("run");
    store
        .transition_run(
            &run_id.to_string(),
            &lease.owner,
            lease.fence,
            run.row_version,
            WorkflowRunTransition {
                status: WorkflowRunStatus::Waiting,
                state_schema_version: run.state_schema_version,
                state: run.state,
                output: None,
                error_code: None,
                wake: Some(serde_json::to_value(WakeCondition::HumanInteraction(
                    interaction_id,
                ))?),
                event_kind: "run.waiting".to_string(),
                event_entity_id: Some(interaction_id.to_string()),
                event_metadata: json!({}),
                updated_at_ms: now_ms,
            },
        )
        .await?;
    store.release_lease(&lease, now_ms + 1).await?;
    Ok(())
}

fn interaction_request(key: &str, deadline_ms: i64) -> HumanInteractionRequest {
    HumanInteractionRequest {
        effect_key: EffectKey::new(key).expect("effect key"),
        prompt: "Please reply with approval.".to_string(),
        chat_id: ChatId::parse("oc_demo_01").expect("chat ID"),
        thread_id: None,
        allowed_senders: vec![OpenId::parse("ou_allowed_01").expect("open ID")],
        deadline_ms,
        sensitivity: ContentSensitivity::NonSensitive,
    }
}

fn write_page(path: &std::path::Path, messages: &[serde_json::Value]) -> std::io::Result<()> {
    std::fs::write(
        path,
        serde_json::to_vec(&json!({
            "data": {
                "messages": messages,
                "total": messages.len(),
                "has_more": false,
                "page_token": null,
            }
        }))
        .expect("serialize page"),
    )
}

fn fixture_message(
    message_id: &str,
    sender_id: &str,
    token: &str,
    created_at_ms: i64,
) -> serde_json::Value {
    json!({
        "message_id": message_id,
        "msg_type": "text",
        "create_time": created_at_ms.to_string(),
        "sender": {"id": sender_id},
        "content": {"text": format!("approved [wf:{token}]")},
        "deleted": false,
        "updated": false,
    })
}

fn raw_event(event_id: &str, message_id: &str) -> String {
    json!({
        "schema": "2.0",
        "header": {
            "event_id": event_id,
            "event_type": "im.message.receive_v1",
            "create_time": "1775000001000",
            "app_id": "cli_demo",
        },
        "event": {
            "message": {
                "chat_id": "oc_supervisor_01",
                "content": "{\"text\":\"uncorrelated fixture\"}",
                "message_id": message_id,
                "message_type": "text",
            },
            "sender": {
                "sender_id": {"open_id": "ou_supervisor_01"},
                "sender_type": "user",
            },
        },
    })
    .to_string()
}

const FAKE_CLI: &str = r#"#!/bin/sh
case "$*" in
  *"event +subscribe"*)
    /bin/cat "$EVENT_ONE"
    printf '\n'
    /bin/sleep 0.1
    /bin/cat "$EVENT_TWO"
    printf '\n'
    exit 0
    ;;
  *"im +chat-messages-list"*) /bin/cat "$MESSAGE_PAGE" ;;
  *"im +messages-send"*)
    printf '{"data":{"message_id":"om_request_01","chat_id":"oc_demo_01","create_time":"100"}}\n'
    ;;
  *) printf 'unsupported fake command\n' >&2; exit 2 ;;
esac
"#;
