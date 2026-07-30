use pretty_assertions::assert_eq;
use serde_json::json;

use crate::StateRuntime;
use crate::WorkflowInteractionPlan;
use crate::WorkflowInteractionState;
use crate::WorkflowLarkInteractionPlan;
use crate::WorkflowLarkInteractionPlanOutcome;
use crate::WorkflowLarkResolve;
use crate::WorkflowLarkResolveOutcome;
use crate::WorkflowRunCreate;
use crate::WorkflowRunStatus;
use crate::runtime::test_support::unique_temp_dir;

use super::WorkflowRunTransition;

#[tokio::test]
async fn lark_interaction_is_deduplicated_validated_and_restart_durable() {
    let home = unique_temp_dir();
    let runtime = StateRuntime::init(home.clone(), "test-provider".to_string())
        .await
        .expect("initialize state runtime");
    let store = runtime.workflows();
    store
        .create_run(WorkflowRunCreate {
            run_id: "run-lark".to_string(),
            definition_name: "test-workflow".to_string(),
            definition_version: "1.0.0".to_string(),
            state_schema_version: 1,
            state: json!({"step": 0}),
            arguments: json!({}),
            non_interactive: false,
            detached: false,
            concurrency: None,
            created_at_ms: 100,
        })
        .await
        .expect("create run");
    store
        .plan_interaction(WorkflowInteractionPlan {
            interaction_id: "interaction-lark".to_string(),
            run_id: "run-lark".to_string(),
            dedupe_key: "approval".to_string(),
            kind: "lark.reply".to_string(),
            request: json!({"prompt": "safe"}),
            deadline_ms: Some(1_000),
            created_at_ms: 101,
        })
        .await
        .expect("plan interaction");
    assert!(
        store
            .update_interaction(
                "interaction-lark",
                WorkflowInteractionState::Planned,
                WorkflowInteractionState::Waiting,
                None,
                102,
            )
            .await
            .expect("wait interaction")
    );
    let lease = store
        .acquire_lease("run-lark", "test-owner", 103, 100)
        .await
        .expect("acquire lease")
        .expect("lease available");
    let run = store
        .read_run("run-lark")
        .await
        .expect("read run")
        .expect("run exists");
    store
        .transition_run(
            "run-lark",
            &lease.owner,
            lease.fence,
            run.row_version,
            WorkflowRunTransition {
                status: WorkflowRunStatus::Waiting,
                state_schema_version: 1,
                state: json!({"step": 1}),
                output: None,
                error_code: None,
                wake: Some(json!({"HumanInteraction": "interaction-lark"})),
                event_kind: "run.waiting".to_string(),
                event_entity_id: Some("interaction-lark".to_string()),
                event_metadata: json!({}),
                updated_at_ms: 104,
            },
        )
        .await
        .expect("mark run waiting");
    store
        .release_lease(&lease, 105)
        .await
        .expect("release lease");

    let plan = || WorkflowLarkInteractionPlan {
        interaction_id: "interaction-lark".to_string(),
        run_id: "run-lark".to_string(),
        effect_key: "lark-request".to_string(),
        chat_id: "oc_demo_01".to_string(),
        thread_id: Some("omt_demo_01".to_string()),
        correlation_token: "wf-demo-token".to_string(),
        allowed_senders: vec!["ou_allowed_01".to_string()],
        watermark_ms: 100,
        created_at_ms: 106,
    };
    assert!(matches!(
        store
            .plan_lark_interaction(plan())
            .await
            .expect("plan Lark correlation"),
        WorkflowLarkInteractionPlanOutcome::Planned(_)
    ));
    assert!(matches!(
        store
            .plan_lark_interaction(plan())
            .await
            .expect("replay Lark correlation"),
        WorkflowLarkInteractionPlanOutcome::Existing(_)
    ));
    assert!(
        store
            .mark_lark_message_sent("interaction-lark", "om_request_01", 107)
            .await
            .expect("record sent message")
    );
    assert_eq!(
        WorkflowLarkResolveOutcome::WrongSender,
        store
            .resolve_lark_interaction(resolve("evt-wrong", "ou_wrong_01", 108))
            .await
            .expect("classify wrong sender")
    );
    assert_eq!(
        WorkflowLarkResolveOutcome::Duplicate,
        store
            .resolve_lark_interaction(resolve("evt-wrong", "ou_allowed_01", 109))
            .await
            .expect("deduplicate event")
    );
    assert_eq!(
        WorkflowLarkResolveOutcome::Resolved,
        store
            .resolve_lark_interaction(resolve("evt-correct", "ou_allowed_01", 110))
            .await
            .expect("resolve interaction")
    );
    let interaction = store
        .read_interaction("interaction-lark")
        .await
        .expect("read interaction")
        .expect("interaction exists");
    assert_eq!(WorkflowInteractionState::Resolved, interaction.state);
    assert_eq!(
        Some("artifact-response-01"),
        interaction.response_artifact_id.as_deref()
    );
    assert_eq!(
        WorkflowRunStatus::Pending,
        store
            .read_run("run-lark")
            .await
            .expect("read run")
            .expect("run exists")
            .status
    );
    runtime.close().await;

    let reopened = StateRuntime::init(home.clone(), "test-provider".to_string())
        .await
        .expect("reopen state runtime");
    let correlation = reopened
        .workflows()
        .read_lark_interaction("interaction-lark")
        .await
        .expect("read reopened correlation")
        .expect("correlation exists");
    assert_eq!(Some("om_request_01"), correlation.request_message_id.as_deref());
    reopened.close().await;
    let _ = tokio::fs::remove_dir_all(home).await;
}

fn resolve(event_id: &str, sender_id: &str, received_at_ms: i64) -> WorkflowLarkResolve {
    WorkflowLarkResolve {
        interaction_id: "interaction-lark".to_string(),
        source: "lark.event".to_string(),
        event_id: event_id.to_string(),
        message_id: format!("om_{event_id}"),
        chat_id: "oc_demo_01".to_string(),
        thread_id: Some("omt_demo_01".to_string()),
        sender_id: sender_id.to_string(),
        response_artifact_id: "artifact-response-01".to_string(),
        received_at_ms,
    }
}
