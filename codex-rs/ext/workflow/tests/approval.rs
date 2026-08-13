#![cfg(unix)]
#![allow(clippy::expect_used)]

use std::collections::BTreeSet;
use std::num::NonZeroUsize;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use codex_state::StateRuntime;
use codex_state::WorkflowRunCreate;
use codex_workflow_extension::ApprovalDecision;
use codex_workflow_extension::ApprovalOutcome;
use codex_workflow_extension::ApprovalRequest;
use codex_workflow_extension::ApprovalSubject;
use codex_workflow_extension::ArtifactClassification;
use codex_workflow_extension::ArtifactId;
use codex_workflow_extension::EffectKey;
use codex_workflow_extension::LarkInteractionService;
use codex_workflow_extension::WorkflowApprovalError;
use codex_workflow_extension::WorkflowApprovalService;
use codex_workflow_extension::WorkflowArtifactStore;
use codex_workflow_extension::WorkflowArtifactWrite;
use codex_workflow_extension::WorkflowRunId;
use codex_workflow_extension::integrations::lark::ChatId;
use codex_workflow_extension::integrations::lark::LarkCli;
use codex_workflow_extension::integrations::lark::LarkCliConfig;
use codex_workflow_extension::integrations::lark::LarkCliEnvironment;
use codex_workflow_extension::integrations::lark::LarkEvent;
use codex_workflow_extension::integrations::lark::MessageId;
use codex_workflow_extension::integrations::lark::OpenId;
use pretty_assertions::assert_eq;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;

struct FakeLark {
    home: tempfile::TempDir,
    executable: PathBuf,
    empty_page: PathBuf,
}

impl FakeLark {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let home = tempfile::tempdir()?;
        let executable = home.path().join("lark-cli");
        let empty_page = home.path().join("empty-page.json");
        std::fs::write(&executable, FAKE_CLI)?;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
        std::fs::write(
            &empty_page,
            serde_json::to_vec(&json!({
                "data": {
                    "messages": [],
                    "total": 0,
                    "has_more": false,
                    "page_token": null,
                }
            }))?,
        )?;
        Ok(Self {
            home,
            executable,
            empty_page,
        })
    }

    fn client(&self) -> Arc<LarkCli> {
        Arc::new(LarkCli::new(LarkCliConfig {
            executable: self.executable.clone(),
            cwd: self.home.path().to_path_buf(),
            environment: LarkCliEnvironment::default()
                .with_value("PATH", "/usr/bin:/bin")
                .with_value("EMPTY_PAGE", self.empty_page.as_os_str()),
            timeout: Duration::from_secs(1),
            stdout_limit: 64 * 1024,
            stderr_limit: 4096,
        }))
    }
}

#[tokio::test]
async fn quorum_restart_invalid_response_revocation_and_run_scope_are_durable()
-> Result<(), Box<dyn std::error::Error>> {
    let fake = FakeLark::new()?;
    let codex_home = tempfile::tempdir()?;
    let run_id = WorkflowRunId::new();
    let runtime =
        StateRuntime::init(codex_home.path().to_path_buf(), "test-provider".to_string()).await?;
    create_run(&runtime, run_id).await?;
    let artifact_id =
        write_subject(&runtime, codex_home.path(), run_id, b"reviewed revision").await?;
    let request = approval_request("design-gate", artifact_id, b"reviewed revision", 2);
    let (lark, approvals) = services(&runtime, codex_home.path(), fake.client());

    let initial = approvals
        .poll_or_request(run_id, request.clone(), 100, &AtomicBool::new(false))
        .await?;
    let ApprovalOutcome::Waiting { approval_id, .. } = initial else {
        panic!("new approval should wait");
    };
    let approval = runtime
        .workflows()
        .read_approval(&run_id.to_string(), &approval_id.to_string())
        .await?
        .expect("approval record");
    let nonce = &approval.request_hash[..24];
    let correlations = runtime
        .workflows()
        .list_waiting_lark_interactions("oc_approval_01", 10)
        .await?;
    assert_eq!(1, correlations.len());

    let first = correlations
        .iter()
        .find(|candidate| candidate.allowed_senders == ["ou_approver_01"])
        .expect("first approver");
    assert_eq!(
        Vec::<codex_state::WorkflowLarkResolveOutcome>::new(),
        lark.ingest_event(
            &reply_event(
                "evt-unauthorized",
                "om_unauthorized",
                "ou_outsider_01",
                &first.correlation_token,
                format!(
                    "APPROVE {nonce} {} [wf:{}]",
                    request.subject.sha256, first.correlation_token
                ),
                120,
            ),
            "test",
        )
        .await?
    );
    lark.ingest_event(
        &reply_event(
            "evt-approve-1",
            "om_approve_01",
            "ou_approver_01",
            &first.correlation_token,
            format!(
                "APPROVE {nonce} {} [wf:{}]",
                request.subject.sha256, first.correlation_token
            ),
            121,
        ),
        "test",
    )
    .await?;
    assert!(matches!(
        approvals
            .poll_or_request(run_id, request.clone(), 122, &AtomicBool::new(false))
            .await?,
        ApprovalOutcome::Waiting { .. }
    ));
    drop(approvals);
    drop(lark);
    runtime.close().await;

    let reopened =
        StateRuntime::init(codex_home.path().to_path_buf(), "test-provider".to_string()).await?;
    let (restarted_lark, restarted) = services(&reopened, codex_home.path(), fake.client());
    let second = reopened
        .workflows()
        .list_waiting_lark_interactions("oc_approval_01", 10)
        .await?
        .into_iter()
        .find(|candidate| candidate.allowed_senders == ["ou_approver_02"])
        .expect("second approver");
    restarted_lark
        .ingest_event(
            &reply_event(
                "evt-approve-2",
                "om_approve_02",
                "ou_approver_02",
                &second.correlation_token,
                format!(
                    "APPROVE {nonce} {} [wf:{}]",
                    request.subject.sha256, second.correlation_token
                ),
                130,
            ),
            "test",
        )
        .await?;
    let approved = restarted
        .poll_or_request(run_id, request.clone(), 131, &AtomicBool::new(false))
        .await?;
    let ApprovalOutcome::Approved { ref decisions, .. } = approved else {
        panic!("exact quorum should approve");
    };
    assert_eq!(2, decisions.len());
    assert!(
        decisions
            .iter()
            .all(|decision| decision.decision == ApprovalDecision::Approve)
    );
    assert_eq!(
        approved,
        restarted
            .poll_or_request(run_id, request.clone(), 132, &AtomicBool::new(false))
            .await?
    );

    let revoked = restarted
        .revoke(
            run_id,
            approval_id,
            OpenId::parse("ou_approver_01")?,
            "requirements changed".to_string(),
            140,
        )
        .await?;
    let ApprovalOutcome::Revoked { ref decisions, .. } = revoked else {
        panic!("authorized revocation should dominate approval");
    };
    assert_eq!(3, decisions.len());
    assert_eq!(
        revoked,
        restarted
            .revoke(
                run_id,
                approval_id,
                OpenId::parse("ou_approver_01")?,
                "requirements changed".to_string(),
                140,
            )
            .await?
    );
    assert!(matches!(
        restarted
            .revoke(
                WorkflowRunId::new(),
                approval_id,
                OpenId::parse("ou_approver_01")?,
                "wrong run".to_string(),
                141,
            )
            .await,
        Err(WorkflowApprovalError::NotFound)
    ));

    let conflicting_artifact = write_subject(
        &reopened,
        codex_home.path(),
        run_id,
        b"conflicting revision",
    )
    .await?;
    let mut conflicting_request = approval_request(
        "design-gate",
        conflicting_artifact,
        b"conflicting revision",
        2,
    );
    conflicting_request.effect_key = request.effect_key.clone();
    assert!(matches!(
        restarted
            .poll_or_request(run_id, conflicting_request, 150, &AtomicBool::new(false),)
            .await,
        Err(WorkflowApprovalError::Store(
            codex_state::WorkflowStoreError::ApprovalConflict
        ))
    ));

    let expired_artifact =
        write_subject(&reopened, codex_home.path(), run_id, b"expired revision").await?;
    let mut expired_request =
        approval_request("expired-gate", expired_artifact, b"expired revision", 1);
    expired_request.deadline_ms = 250;
    assert!(matches!(
        restarted
            .poll_or_request(run_id, expired_request, 251, &AtomicBool::new(false))
            .await?,
        ApprovalOutcome::TimedOut { .. }
    ));

    let invalid_artifact = write_subject(
        &reopened,
        codex_home.path(),
        run_id,
        b"requirements revision",
    )
    .await?;
    let invalid_request = approval_request(
        "requirements-gate",
        invalid_artifact,
        b"requirements revision",
        1,
    );
    let waiting = restarted
        .poll_or_request(
            run_id,
            invalid_request.clone(),
            200,
            &AtomicBool::new(false),
        )
        .await?;
    let ApprovalOutcome::Waiting {
        approval_id: invalid_approval_id,
        ..
    } = waiting
    else {
        panic!("second approval should wait");
    };
    let invalid_approval = reopened
        .workflows()
        .read_approval(&run_id.to_string(), &invalid_approval_id.to_string())
        .await?
        .expect("second approval record");
    let invalid_nonce = &invalid_approval.request_hash[..24];
    let invalid_correlation = newest_correlation(
        &reopened,
        "ou_approver_01",
        &BTreeSet::from(["om_approve_01".to_string(), "om_approve_02".to_string()]),
    )
    .await?;
    restarted_lark
        .ingest_event(
            &reply_event(
                "evt-stale-revision",
                "om_stale_revision",
                "ou_approver_01",
                &invalid_correlation,
                format!(
                    "APPROVE {invalid_nonce} {} [wf:{invalid_correlation}]",
                    "0".repeat(64)
                ),
                210,
            ),
            "test",
        )
        .await?;
    assert!(matches!(
        restarted
            .poll_or_request(
                run_id,
                invalid_request.clone(),
                211,
                &AtomicBool::new(false),
            )
            .await?,
        ApprovalOutcome::Waiting { .. }
    ));
    let emoji_correlation =
        newest_correlation(&reopened, "ou_approver_01", &BTreeSet::new()).await?;
    restarted_lark
        .ingest_event(
            &reply_event(
                "evt-emoji",
                "om_emoji_01",
                "ou_approver_01",
                &emoji_correlation,
                format!("👍 [wf:{emoji_correlation}]"),
                212,
            ),
            "test",
        )
        .await?;
    assert!(matches!(
        restarted
            .poll_or_request(
                run_id,
                invalid_request.clone(),
                213,
                &AtomicBool::new(false),
            )
            .await?,
        ApprovalOutcome::Waiting { .. }
    ));
    assert_eq!(
        0,
        reopened
            .workflows()
            .list_approval_decisions(&invalid_approval_id.to_string())
            .await?
            .len()
    );
    let replacement_correlation =
        newest_correlation(&reopened, "ou_approver_01", &BTreeSet::new()).await?;
    restarted_lark
        .ingest_event(
            &reply_event(
                "evt-exact-after-invalid",
                "om_exact_after_invalid",
                "ou_approver_01",
                &replacement_correlation,
                format!(
                    "APPROVE {invalid_nonce} {} [wf:{replacement_correlation}]",
                    invalid_request.subject.sha256
                ),
                220,
            ),
            "test",
        )
        .await?;
    assert!(matches!(
        restarted
            .poll_or_request(run_id, invalid_request, 221, &AtomicBool::new(false))
            .await?,
        ApprovalOutcome::Approved { .. }
    ));
    reopened.close().await;
    Ok(())
}

async fn newest_correlation(
    runtime: &StateRuntime,
    sender: &str,
    excluded_messages: &BTreeSet<String>,
) -> Result<String, Box<dyn std::error::Error>> {
    let candidate = runtime
        .workflows()
        .list_waiting_lark_interactions("oc_approval_01", 100)
        .await?
        .into_iter()
        .filter(|candidate| candidate.allowed_senders == [sender])
        .filter(|candidate| {
            candidate
                .request_message_id
                .as_ref()
                .is_none_or(|message| !excluded_messages.contains(message))
        })
        .max_by_key(|candidate| candidate.updated_at_ms)
        .expect("waiting approval correlation");
    Ok(candidate.correlation_token)
}

fn services(
    runtime: &StateRuntime,
    home: &std::path::Path,
    cli: Arc<LarkCli>,
) -> (Arc<LarkInteractionService>, WorkflowApprovalService) {
    let artifacts = WorkflowArtifactStore::new(home, runtime.workflows().clone());
    let lark = Arc::new(LarkInteractionService::new(
        cli,
        runtime.workflows().clone(),
        artifacts.clone(),
    ));
    let approvals =
        WorkflowApprovalService::new(runtime.workflows().clone(), Arc::clone(&lark), artifacts);
    (lark, approvals)
}

async fn create_run(
    runtime: &StateRuntime,
    run_id: WorkflowRunId,
) -> Result<(), Box<dyn std::error::Error>> {
    runtime
        .workflows()
        .create_run(WorkflowRunCreate {
            run_id: run_id.to_string(),
            definition_name: "approval-test".to_string(),
            definition_version: "1.0.0".to_string(),
            state_schema_version: 1,
            state: json!({}),
            arguments: json!({}),
            non_interactive: false,
            detached: false,
            concurrency: None,
            created_at_ms: 10,
        })
        .await?;
    Ok(())
}

async fn write_subject(
    runtime: &StateRuntime,
    home: &std::path::Path,
    run_id: WorkflowRunId,
    bytes: &[u8],
) -> Result<ArtifactId, Box<dyn std::error::Error>> {
    let artifact_id = ArtifactId::new();
    WorkflowArtifactStore::new(home, runtime.workflows().clone())
        .write(WorkflowArtifactWrite {
            run_id,
            artifact_id,
            relative_path: &PathBuf::from(format!("subjects/{artifact_id}.md")),
            classification: ArtifactClassification::Internal,
            media_type: "text/markdown",
            bytes,
            created_at_ms: 20,
        })
        .await?;
    Ok(artifact_id)
}

fn approval_request(
    gate: &str,
    artifact_id: ArtifactId,
    bytes: &[u8],
    quorum: usize,
) -> ApprovalRequest {
    ApprovalRequest {
        effect_key: EffectKey::new(format!("{gate}-approval")).expect("effect key"),
        gate: gate.to_string(),
        subject: ApprovalSubject {
            requirement_generation: 1,
            artifact_id,
            sha256: format!("{:x}", Sha256::digest(bytes)),
            document_id: Some("doc_demo_01".to_string()),
            document_revision: Some("42".to_string()),
        },
        allowed_approvers: BTreeSet::from([
            OpenId::parse("ou_approver_01").expect("approver"),
            OpenId::parse("ou_approver_02").expect("approver"),
        ]),
        quorum: NonZeroUsize::new(quorum).expect("nonzero quorum"),
        chat_id: ChatId::parse("oc_approval_01").expect("chat"),
        thread_id: None,
        deadline_ms: 1_000,
        prompt: "Review the linked immutable revision.".to_string(),
    }
}

fn reply_event(
    event_id: &str,
    message_id: &str,
    sender_id: &str,
    _correlation: &str,
    text: String,
    created_at_ms: i64,
) -> LarkEvent {
    LarkEvent {
        event_id: event_id.to_string(),
        event_type: "im.message.receive_v1".to_string(),
        created_at_ms,
        message_id: MessageId::parse(message_id).expect("message ID"),
        chat_id: ChatId::parse("oc_approval_01").expect("chat ID"),
        thread_id: None,
        sender_id: OpenId::parse(sender_id).expect("sender ID"),
        message_type: "text".to_string(),
        text,
    }
}

const FAKE_CLI: &str = r#"#!/bin/sh
case "$*" in
  *"im +chat-messages-list"*) /bin/cat "$EMPTY_PAGE" ;;
  *"im +messages-send"*)
    printf '{"data":{"message_id":"om_request_shared","chat_id":"oc_approval_01","create_time":"100"}}\n'
    ;;
  *) printf 'unsupported fake command\n' >&2; exit 2 ;;
esac
"#;
