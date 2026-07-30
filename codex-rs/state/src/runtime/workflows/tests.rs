use std::path::Path;

use pretty_assertions::assert_eq;
use serde_json::json;

use crate::StateRuntime;
use crate::WorkflowEffectPlan;
use crate::WorkflowEffectPlanOutcome;
use crate::WorkflowEffectState;
use crate::WorkflowEffectUpdate;
use crate::WorkflowInteractionPlan;
use crate::WorkflowInteractionPlanOutcome;
use crate::WorkflowInteractionState;
use crate::WorkflowNodeAttemptCreate;
use crate::WorkflowNodeAttemptStatus;
use crate::WorkflowNodeAttemptTransition;
use crate::WorkflowNodeCreate;
use crate::WorkflowNodeStatus;
use crate::WorkflowRunCreate;
use crate::WorkflowRunStatus;
use crate::backup_runtime_db_for_fresh_start;
use crate::canonical_workflow_request_hash;
use crate::runtime::runtime_db_path_for_corruption_error;
use crate::runtime::test_support::unique_temp_dir;
use crate::workflows_db_path;

use super::WorkflowRunTransition;
use super::WorkflowStoreError;

async fn init(home: &Path) -> std::sync::Arc<StateRuntime> {
    StateRuntime::init(home.to_path_buf(), "test-provider".to_string())
        .await
        .expect("initialize state runtime")
}

async fn create_run(runtime: &StateRuntime, run_id: &str) {
    runtime
        .workflows()
        .create_run(WorkflowRunCreate {
            run_id: run_id.to_string(),
            definition_name: "test-workflow".to_string(),
            definition_version: "1.0.0".to_string(),
            state_schema_version: 1,
            state: json!({"step": 0}),
            arguments: json!({"message": "hello"}),
            created_at_ms: 100,
        })
        .await
        .expect("create workflow run");
}

#[tokio::test]
async fn workflow_run_listing_paginates_and_explicit_resume_is_durable() {
    let home = unique_temp_dir();
    let runtime = init(&home).await;
    create_run(runtime.as_ref(), "run-a").await;
    create_run(runtime.as_ref(), "run-b").await;
    create_run(runtime.as_ref(), "run-c").await;
    let store = runtime.workflows();

    let first_page = store
        .list_runs(None, 2)
        .await
        .expect("list first workflow run page");
    assert_eq!(
        first_page
            .iter()
            .map(|run| run.run_id.as_str())
            .collect::<Vec<_>>(),
        vec!["run-c", "run-b"]
    );
    let second_page = store
        .list_runs(Some("run-b"), 2)
        .await
        .expect("list second workflow run page");
    assert_eq!(
        second_page
            .iter()
            .map(|run| run.run_id.as_str())
            .collect::<Vec<_>>(),
        vec!["run-a"]
    );
    assert!(matches!(
        store.resume_run("run-a", 199).await,
        Err(WorkflowStoreError::StaleWrite)
    ));

    store
        .mark_run_needs_operator(
            "run-b",
            None,
            "manual_review",
            json!({"reason": "unsafe recovery"}),
            200,
        )
        .await
        .expect("mark run as operator-owned");
    let resumed = store
        .resume_run("run-b", 201)
        .await
        .expect("resume operator-owned run");
    assert_eq!(resumed.status, WorkflowRunStatus::Pending);
    assert_eq!(resumed.error_code, None);
    assert_eq!(resumed.wake, None);
    let events = store
        .events_after("run-b", 0, 10)
        .await
        .expect("read resumed run events");
    assert_eq!(
        events
            .iter()
            .map(|event| event.kind.as_str())
            .collect::<Vec<_>>(),
        vec!["run.created", "run.needs_operator", "run.resumed"]
    );

    runtime.close().await;
    let reopened = init(&home).await;
    let resumed = reopened
        .workflows()
        .read_run("run-b")
        .await
        .expect("read reopened run")
        .expect("resumed run exists");
    assert_eq!(resumed.status, WorkflowRunStatus::Pending);
    reopened.close().await;
    let _ = tokio::fs::remove_dir_all(home).await;
}

#[tokio::test]
async fn workflow_lease_race_fences_stale_scheduler_and_reopens() {
    let home = unique_temp_dir();
    let runtime = init(&home).await;
    create_run(runtime.as_ref(), "run-race").await;
    let store_a = runtime.workflows().clone();
    let store_b = runtime.workflows().clone();

    let (lease_a, lease_b) = tokio::join!(
        store_a.acquire_lease("run-race", "scheduler-a", 1_000, 100),
        store_b.acquire_lease("run-race", "scheduler-b", 1_000, 100),
    );
    let lease_a = lease_a.expect("scheduler A lease request");
    let lease_b = lease_b.expect("scheduler B lease request");
    assert_eq!(
        usize::from(lease_a.is_some()) + usize::from(lease_b.is_some()),
        1
    );
    let first = lease_a.or(lease_b).expect("one scheduler owns lease");
    let second_owner = if first.owner == "scheduler-a" {
        "scheduler-b"
    } else {
        "scheduler-a"
    };
    assert!(
        runtime
            .workflows()
            .renew_lease(&first, 1_050, 100)
            .await
            .expect("renew current lease")
    );
    assert!(
        runtime
            .workflows()
            .acquire_lease("run-race", second_owner, 1_101, 100)
            .await
            .expect("blocked lease acquisition")
            .is_none()
    );
    assert!(
        runtime
            .workflows()
            .release_lease(&first, 1_102)
            .await
            .expect("release current lease")
    );
    let second = runtime
        .workflows()
        .acquire_lease("run-race", second_owner, 1_103, 100)
        .await
        .expect("released lease acquisition")
        .expect("second scheduler owns released lease");
    assert_eq!(second.fence, first.fence + 1);

    let transition = || WorkflowRunTransition {
        status: WorkflowRunStatus::Running,
        state_schema_version: 1,
        state: json!({"step": 1}),
        output: None,
        error_code: None,
        wake: None,
        event_kind: "run.started".to_string(),
        event_entity_id: Some("run-race".to_string()),
        event_metadata: json!({}),
        updated_at_ms: 1_104,
    };
    let stale = runtime
        .workflows()
        .transition_run("run-race", &first.owner, first.fence, 1, transition())
        .await
        .expect_err("stale fence must fail");
    assert!(matches!(stale, WorkflowStoreError::StaleWrite));
    let event = runtime
        .workflows()
        .transition_run("run-race", &second.owner, second.fence, 1, transition())
        .await
        .expect("current fence transition");
    assert_eq!(event.sequence, 2);

    runtime.close().await;
    let reopened = init(&home).await;
    let run = reopened
        .workflows()
        .read_run("run-race")
        .await
        .expect("read reopened run")
        .expect("reopened run exists");
    assert_eq!(run.status, WorkflowRunStatus::Running);
    assert_eq!(run.state, json!({"step": 1}));
    reopened.close().await;
    let _ = tokio::fs::remove_dir_all(home).await;
}

#[tokio::test]
async fn workflow_graph_dedupes_journals_and_replays_events_in_order() {
    let home = unique_temp_dir();
    let runtime = init(&home).await;
    create_run(runtime.as_ref(), "run-graph").await;
    let store = runtime.workflows();

    store
        .create_node(
            "run-graph",
            WorkflowNodeCreate {
                node_id: "node-a".to_string(),
                node_key: "prepare".to_string(),
                spec: json!({"prompt": "prepare"}),
                status: WorkflowNodeStatus::Ready,
                failure_policy: "fail_fast".to_string(),
                created_at_ms: 101,
            },
            &[],
        )
        .await
        .expect("create first node");
    store
        .create_node(
            "run-graph",
            WorkflowNodeCreate {
                node_id: "node-b".to_string(),
                node_key: "finish".to_string(),
                spec: json!({"prompt": "finish"}),
                status: WorkflowNodeStatus::Pending,
                failure_policy: "fail_fast".to_string(),
                created_at_ms: 102,
            },
            &["node-a".to_string()],
        )
        .await
        .expect("create dependent node");
    assert_eq!(
        store
            .list_nodes("run-graph")
            .await
            .expect("list nodes")
            .len(),
        2
    );

    let request = json!({"nested": {"b": 2, "a": 1}, "action": "send"});
    let reordered = json!({"action": "send", "nested": {"a": 1, "b": 2}});
    let request_hash = canonical_workflow_request_hash(&request).expect("hash request");
    assert_eq!(
        request_hash,
        canonical_workflow_request_hash(&reordered).expect("hash reordered request")
    );
    let effect_plan = || WorkflowEffectPlan {
        run_id: "run-graph".to_string(),
        effect_key: "notify".to_string(),
        kind: "message".to_string(),
        request: request.clone(),
        created_at_ms: 103,
    };
    assert!(matches!(
        store.plan_effect(effect_plan()).await.expect("plan effect"),
        WorkflowEffectPlanOutcome::Planned(_)
    ));
    assert!(matches!(
        store
            .plan_effect(effect_plan())
            .await
            .expect("retry effect"),
        WorkflowEffectPlanOutcome::Existing(_)
    ));
    let conflicting_effect = store
        .plan_effect(WorkflowEffectPlan {
            request: json!({"action": "delete"}),
            ..effect_plan()
        })
        .await
        .expect_err("conflicting effect retry must fail");
    assert!(matches!(
        conflicting_effect,
        WorkflowStoreError::EffectConflict
    ));
    assert!(
        store
            .update_effect(WorkflowEffectUpdate {
                run_id: "run-graph".to_string(),
                effect_key: "notify".to_string(),
                expected_state: WorkflowEffectState::Planned,
                state: WorkflowEffectState::Dispatched,
                response: None,
                error_code: None,
                updated_at_ms: 104,
            })
            .await
            .expect("advance effect state")
    );
    assert!(
        !store
            .update_effect(WorkflowEffectUpdate {
                run_id: "run-graph".to_string(),
                effect_key: "notify".to_string(),
                expected_state: WorkflowEffectState::Planned,
                state: WorkflowEffectState::Applied,
                response: None,
                error_code: None,
                updated_at_ms: 105,
            })
            .await
            .expect("reject stale effect state")
    );

    let interaction_plan = || WorkflowInteractionPlan {
        interaction_id: "interaction-1".to_string(),
        run_id: "run-graph".to_string(),
        dedupe_key: "approve".to_string(),
        kind: "approval".to_string(),
        request: json!({"question": "continue?"}),
        deadline_ms: Some(10_000),
        created_at_ms: 106,
    };
    assert!(matches!(
        store
            .plan_interaction(interaction_plan())
            .await
            .expect("plan interaction"),
        WorkflowInteractionPlanOutcome::Planned(_)
    ));
    assert!(matches!(
        store
            .plan_interaction(interaction_plan())
            .await
            .expect("retry interaction"),
        WorkflowInteractionPlanOutcome::Existing(_)
    ));
    let conflicting_interaction = store
        .plan_interaction(WorkflowInteractionPlan {
            request: json!({"question": "delete?"}),
            ..interaction_plan()
        })
        .await
        .expect_err("conflicting interaction retry must fail");
    assert!(matches!(
        conflicting_interaction,
        WorkflowStoreError::InteractionConflict
    ));
    assert!(
        store
            .update_interaction(
                "interaction-1",
                WorkflowInteractionState::Planned,
                WorkflowInteractionState::Waiting,
                None,
                107,
            )
            .await
            .expect("advance interaction state")
    );

    let events = store
        .events_after("run-graph", 0, 100)
        .await
        .expect("replay events");
    assert_eq!(
        events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5, 6, 7]
    );
    assert_eq!(
        events
            .iter()
            .map(|event| event.kind.as_str())
            .collect::<Vec<_>>(),
        vec![
            "run.created",
            "node.created",
            "node.created",
            "effect.planned",
            "effect.updated",
            "interaction.planned",
            "interaction.updated",
        ]
    );
    runtime.close().await;
    let _ = tokio::fs::remove_dir_all(home).await;
}

#[tokio::test]
async fn workflow_node_thread_binding_is_unique_idempotent_and_durable() {
    let home = unique_temp_dir();
    let runtime = init(&home).await;
    create_run(runtime.as_ref(), "run-node-binding").await;
    let store = runtime.workflows();
    for (node_id, node_key) in [("node-a", "primary"), ("node-b", "secondary")] {
        store
            .create_node(
                "run-node-binding",
                WorkflowNodeCreate {
                    node_id: node_id.to_string(),
                    node_key: node_key.to_string(),
                    spec: json!({"key": node_key}),
                    status: WorkflowNodeStatus::Ready,
                    failure_policy: "fail_fast".to_string(),
                    created_at_ms: 101,
                },
                &[],
            )
            .await
            .expect("create node");
    }

    let bound = store
        .bind_node_thread("node-a", 1, "thread-primary", 102)
        .await
        .expect("bind node thread");
    assert_eq!(bound.thread_id.as_deref(), Some("thread-primary"));
    assert_eq!(bound.row_version, 2);
    assert_eq!(
        store
            .read_node_by_thread_id("thread-primary")
            .await
            .expect("read thread binding"),
        Some(bound.clone())
    );
    assert_eq!(
        store
            .bind_node_thread("node-a", 1, "thread-primary", 103)
            .await
            .expect("idempotent thread binding"),
        bound
    );
    assert!(matches!(
        store
            .bind_node_thread("node-a", 2, "thread-other", 104)
            .await
            .expect_err("node cannot be rebound"),
        WorkflowStoreError::StaleWrite
    ));
    assert!(matches!(
        store
            .bind_node_thread("node-b", 1, "thread-primary", 105)
            .await
            .expect_err("thread cannot bind to two nodes"),
        WorkflowStoreError::DuplicateNode
    ));

    let events = store
        .events_after("run-node-binding", 0, 100)
        .await
        .expect("read binding events");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "node.thread_bound")
            .count(),
        1
    );

    runtime.close().await;
    let reopened = init(&home).await;
    assert_eq!(
        reopened
            .workflows()
            .read_node_by_thread_id("thread-primary")
            .await
            .expect("read reopened binding")
            .and_then(|node| node.thread_id),
        Some("thread-primary".to_string())
    );
    reopened.close().await;
    let _ = tokio::fs::remove_dir_all(home).await;
}

#[tokio::test]
async fn workflow_node_attempts_append_and_transition_without_overwriting_history() {
    let home = unique_temp_dir();
    let runtime = init(&home).await;
    create_run(runtime.as_ref(), "run-attempts").await;
    let store = runtime.workflows();
    store
        .create_node(
            "run-attempts",
            WorkflowNodeCreate {
                node_id: "node-attempts".to_string(),
                node_key: "primary".to_string(),
                spec: json!({}),
                status: WorkflowNodeStatus::Ready,
                failure_policy: "fail_fast".to_string(),
                created_at_ms: 101,
            },
            &[],
        )
        .await
        .expect("create node");
    let first = store
        .create_node_attempt(
            "run-attempts",
            WorkflowNodeAttemptCreate {
                attempt_id: "attempt-1".to_string(),
                node_id: "node-attempts".to_string(),
                submission_id: "submission-1".to_string(),
                input_hash: "abc".to_string(),
                thread_id: Some("thread-1".to_string()),
                created_at_ms: 102,
            },
        )
        .await
        .expect("create first attempt");
    assert_eq!(first.attempt_number, 1);
    let submitted = store
        .transition_node_attempt(
            "attempt-1",
            WorkflowNodeAttemptTransition {
                expected_status: WorkflowNodeAttemptStatus::Planned,
                status: WorkflowNodeAttemptStatus::Submitted,
                turn_id: Some("turn-1".to_string()),
                error_code: None,
                updated_at_ms: 103,
            },
        )
        .await
        .expect("submit first attempt");
    assert_eq!(submitted.started_at_ms, Some(103));
    let succeeded = store
        .transition_node_attempt(
            "attempt-1",
            WorkflowNodeAttemptTransition {
                expected_status: WorkflowNodeAttemptStatus::Submitted,
                status: WorkflowNodeAttemptStatus::Succeeded,
                turn_id: Some("turn-1".to_string()),
                error_code: None,
                updated_at_ms: 104,
            },
        )
        .await
        .expect("complete first attempt");
    assert_eq!(succeeded.completed_at_ms, Some(104));
    assert!(matches!(
        store
            .transition_node_attempt(
                "attempt-1",
                WorkflowNodeAttemptTransition {
                    expected_status: WorkflowNodeAttemptStatus::Submitted,
                    status: WorkflowNodeAttemptStatus::Failed,
                    turn_id: Some("turn-1".to_string()),
                    error_code: Some("late".to_string()),
                    updated_at_ms: 105,
                },
            )
            .await
            .expect_err("terminal evidence cannot be overwritten"),
        WorkflowStoreError::StaleWrite
    ));

    let second = store
        .create_node_attempt(
            "run-attempts",
            WorkflowNodeAttemptCreate {
                attempt_id: "attempt-2".to_string(),
                node_id: "node-attempts".to_string(),
                submission_id: "submission-2".to_string(),
                input_hash: "def".to_string(),
                thread_id: Some("thread-1".to_string()),
                created_at_ms: 106,
            },
        )
        .await
        .expect("append second attempt");
    assert_eq!(second.attempt_number, 2);
    assert_eq!(
        store
            .read_node_attempt_by_turn_id("node-attempts", "turn-1")
            .await
            .expect("read by turn"),
        Some(succeeded)
    );

    runtime.close().await;
    let _ = tokio::fs::remove_dir_all(home).await;
}

#[tokio::test]
async fn workflow_corruption_routes_only_workflow_database_to_backup() {
    let home = unique_temp_dir();
    let runtime = init(&home).await;
    runtime.close().await;
    let workflow_path = workflows_db_path(&home);
    tokio::fs::write(&workflow_path, b"not a sqlite database")
        .await
        .expect("corrupt workflow database");

    let error = match StateRuntime::init(home.clone(), "test-provider".to_string()).await {
        Ok(runtime) => {
            runtime.close().await;
            panic!("corrupt workflow database must fail initialization");
        }
        Err(error) => error,
    };
    assert_eq!(
        runtime_db_path_for_corruption_error(&error),
        Some(workflow_path.clone())
    );
    let backups = backup_runtime_db_for_fresh_start(&workflow_path)
        .await
        .expect("backup corrupt workflow database");
    assert!(
        backups
            .iter()
            .any(|backup| backup.original_path == workflow_path)
    );
    let rebuilt = init(&home).await;
    assert!(workflow_path.exists());
    rebuilt.close().await;
    let _ = tokio::fs::remove_dir_all(home).await;
}
