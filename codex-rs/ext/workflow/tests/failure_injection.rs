#![allow(clippy::expect_used)]

mod support;

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use codex_state::WorkflowEffectState;
use codex_state::WorkflowNodeAttemptStatus;
use codex_workflow_extension::DurableOutbox;
use codex_workflow_extension::EffectKey;
use codex_workflow_extension::FailureInjector;
use codex_workflow_extension::FailurePoint;
use codex_workflow_extension::NodeInput;
use codex_workflow_extension::NodeKey;
use codex_workflow_extension::NodeSpec;
use codex_workflow_extension::OutboxError;
use codex_workflow_extension::WorkflowNodeHostSlot;
use codex_workflow_extension::WorkflowRegistry;
use codex_workflow_extension::WorkflowRunId;
use codex_workflow_extension::WorkflowService;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn failure_injector_covers_every_declared_crash_boundary_deterministically() {
    for point in [
        FailurePoint::BeforeDbWrite,
        FailurePoint::AfterDbWrite,
        FailurePoint::BeforeThreadCreate,
        FailurePoint::AfterThreadCreate,
        FailurePoint::BeforeTurnQueue,
        FailurePoint::AfterTurnQueue,
        FailurePoint::AfterUserBoundaryAppend,
        FailurePoint::AfterTerminalDelivery,
        FailurePoint::AfterTerminalFlush,
        FailurePoint::BeforeExternalDispatch,
        FailurePoint::AfterExternalDispatch,
        FailurePoint::BeforeReplyCommit,
        FailurePoint::AfterReplyCommit,
    ] {
        let injector = FailureInjector::fail_on(point, 2);
        injector.checkpoint(point).expect("first visit passes");
        assert_eq!(
            injector
                .checkpoint(point)
                .expect_err("second visit fails")
                .point,
            point
        );
        assert_eq!(injector.visits(point), 2);
    }
}

#[tokio::test]
async fn failure_injection_reconciles_thread_creation_without_duplicate_threads() {
    for point in [
        FailurePoint::BeforeDbWrite,
        FailurePoint::AfterDbWrite,
        FailurePoint::BeforeThreadCreate,
        FailurePoint::AfterThreadCreate,
    ] {
        let (_home, runtime) = support::runtime().await;
        let run_id = WorkflowRunId::new();
        support::create_run(
            runtime.as_ref(),
            run_id,
            "failure-thread",
            "1.0.0",
            1,
            json!({}),
        )
        .await;
        let host = Arc::new(support::TestHost::default());
        let crashing = service_with_injector(
            runtime.as_ref(),
            Arc::clone(&host),
            FailureInjector::fail_on(point, 1),
        );
        let spec = NodeSpec::builder(NodeKey::new("thread-node").expect("node key"))
            .build()
            .expect("node spec");
        crashing
            .nodes(run_id)
            .ensure(
                EffectKey::new("ensure-thread").expect("effect key"),
                spec.clone(),
            )
            .await
            .err()
            .expect("inject thread crash");
        let recovered = service_with_injector(
            runtime.as_ref(),
            Arc::clone(&host),
            FailureInjector::disabled(),
        );
        recovered
            .nodes(run_id)
            .ensure(EffectKey::new("ensure-thread").expect("effect key"), spec)
            .await
            .expect("reconcile thread creation");
        assert_eq!(host.threads().len(), 1);
        assert!(host.materializations.load(Ordering::Acquire) <= 1);
        runtime.close().await;
    }
}

#[tokio::test]
async fn failure_injection_replays_prepared_turn_once_at_every_queue_window() {
    for point in [
        FailurePoint::BeforeDbWrite,
        FailurePoint::AfterDbWrite,
        FailurePoint::BeforeTurnQueue,
        FailurePoint::AfterTurnQueue,
        FailurePoint::AfterUserBoundaryAppend,
    ] {
        let (_home, runtime) = support::runtime().await;
        let run_id = WorkflowRunId::new();
        support::create_run(
            runtime.as_ref(),
            run_id,
            "failure-turn",
            "1.0.0",
            1,
            json!({}),
        )
        .await;
        let host = Arc::new(support::TestHost::default());
        let setup = service_with_injector(
            runtime.as_ref(),
            Arc::clone(&host),
            FailureInjector::disabled(),
        );
        let spec = NodeSpec::builder(NodeKey::new("turn-node").expect("node key"))
            .build()
            .expect("node spec");
        let node = setup
            .nodes(run_id)
            .ensure(EffectKey::new("ensure-turn").expect("effect key"), spec)
            .await
            .expect("ensure turn node");
        let crashing = service_with_injector(
            runtime.as_ref(),
            Arc::clone(&host),
            FailureInjector::fail_on(point, 1),
        );
        crashing
            .nodes(run_id)
            .get(node.id())
            .await
            .expect("get crashing node")
            .start(
                EffectKey::new("prepared-turn").expect("effect key"),
                NodeInput::text("same input"),
            )
            .await
            .expect_err("inject prepared turn crash");
        let recovered = service_with_injector(
            runtime.as_ref(),
            Arc::clone(&host),
            FailureInjector::disabled(),
        );
        let turn = recovered
            .nodes(run_id)
            .get(node.id())
            .await
            .expect("get recovered node")
            .start(
                EffectKey::new("prepared-turn").expect("effect key"),
                NodeInput::text("same input"),
            )
            .await
            .expect("reconcile prepared turn");
        assert!(!turn.turn_id.is_empty());
        assert_eq!(host.unique_queues.load(Ordering::Acquire), 1);
        runtime.close().await;
    }
}

#[tokio::test]
async fn failure_injection_replays_terminal_flush_before_attempt_commit() {
    for point in [
        FailurePoint::AfterTerminalDelivery,
        FailurePoint::AfterTerminalFlush,
    ] {
        let (_home, runtime) = support::runtime().await;
        let run_id = WorkflowRunId::new();
        support::create_run(
            runtime.as_ref(),
            run_id,
            "failure-terminal",
            "1.0.0",
            1,
            json!({}),
        )
        .await;
        let host = Arc::new(support::TestHost::default());
        let setup = service_with_injector(
            runtime.as_ref(),
            Arc::clone(&host),
            FailureInjector::disabled(),
        );
        let node = setup
            .nodes(run_id)
            .ensure(
                EffectKey::new("terminal-node").expect("effect key"),
                NodeSpec::builder(NodeKey::new("terminal-node").expect("node key"))
                    .build()
                    .expect("node spec"),
            )
            .await
            .expect("ensure terminal node");
        let turn = node
            .start(
                EffectKey::new("terminal-turn").expect("effect key"),
                NodeInput::text("finish"),
            )
            .await
            .expect("start terminal turn");
        let crashing = service_with_injector(
            runtime.as_ref(),
            Arc::clone(&host),
            FailureInjector::fail_on(point, 1),
        );
        crashing
            .nodes(run_id)
            .get(node.id())
            .await
            .expect("get terminal node")
            .await_turn(turn.turn_id.clone())
            .await
            .expect_err("inject terminal crash");
        let recovered = service_with_injector(runtime.as_ref(), host, FailureInjector::disabled());
        recovered
            .nodes(run_id)
            .get(node.id())
            .await
            .expect("get recovered terminal node")
            .await_turn(turn.turn_id.clone())
            .await
            .expect("reconcile terminal turn");
        assert_eq!(
            runtime
                .workflows()
                .read_node_attempt_by_turn_id(&node.id().to_string(), &turn.turn_id)
                .await
                .expect("read terminal attempt")
                .expect("terminal attempt")
                .status,
            WorkflowNodeAttemptStatus::Succeeded
        );
        runtime.close().await;
    }
}

#[tokio::test]
async fn failure_injection_never_blindly_repeats_external_dispatch() {
    for point in [
        FailurePoint::BeforeExternalDispatch,
        FailurePoint::AfterExternalDispatch,
    ] {
        let (_home, runtime) = support::runtime().await;
        let run_id = WorkflowRunId::new();
        support::create_run(
            runtime.as_ref(),
            run_id,
            "failure-outbox",
            "1.0.0",
            1,
            json!({}),
        )
        .await;
        let calls = Arc::new(AtomicUsize::new(0));
        let outbox = DurableOutbox::new(
            runtime.workflows().clone(),
            FailureInjector::fail_on(point, 1),
        );
        let operation_calls = Arc::clone(&calls);
        outbox
            .dispatch(
                run_id,
                EffectKey::new("external-effect").expect("effect key"),
                "external.test",
                json!({"value": 1}),
                200,
                move || async move {
                    operation_calls.fetch_add(1, Ordering::AcqRel);
                    Ok(json!({"external_id": "one"}))
                },
            )
            .await
            .expect_err("inject external crash");
        let replay = DurableOutbox::new(runtime.workflows().clone(), FailureInjector::disabled());
        let replay_calls = Arc::clone(&calls);
        assert!(matches!(
            replay
                .dispatch(
                    run_id,
                    EffectKey::new("external-effect").expect("effect key"),
                    "external.test",
                    json!({"value": 1}),
                    201,
                    move || async move {
                        replay_calls.fetch_add(1, Ordering::AcqRel);
                        Ok(json!({"external_id": "duplicate"}))
                    },
                )
                .await
                .expect_err("dispatched effect requires reconciliation"),
            OutboxError::RequiresReconciliation(WorkflowEffectState::Dispatched)
        ));
        assert_eq!(
            calls.load(Ordering::Acquire),
            usize::from(point == FailurePoint::AfterExternalDispatch)
        );
        replay
            .reconcile_success(
                &run_id.to_string(),
                "external-effect",
                json!({"external_id": "reconciled"}),
                202,
            )
            .await
            .expect("reconcile external effect");
        runtime.close().await;
    }
}

fn service_with_injector(
    runtime: &codex_state::StateRuntime,
    host: Arc<support::TestHost>,
    injector: FailureInjector,
) -> WorkflowService {
    let slot = WorkflowNodeHostSlot::new();
    slot.bind(host).expect("bind failure host");
    WorkflowService::new_with_injector(
        runtime.workflows().clone(),
        slot,
        Arc::new(WorkflowRegistry::default()),
        injector,
    )
}
