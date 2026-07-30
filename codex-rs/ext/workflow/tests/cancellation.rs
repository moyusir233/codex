#![allow(clippy::expect_used)]

mod support;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use clap::Parser;
use codex_state::WorkflowNodeAttemptStatus;
use codex_state::WorkflowRunStatus;
use codex_workflow_extension::ArgumentError;
use codex_workflow_extension::CancellationReason;
use codex_workflow_extension::EffectKey;
use codex_workflow_extension::NodeInput;
use codex_workflow_extension::NodeKey;
use codex_workflow_extension::NodeSpec;
use codex_workflow_extension::Workflow;
use codex_workflow_extension::WorkflowArguments;
use codex_workflow_extension::WorkflowCancellationController;
use codex_workflow_extension::WorkflowContext;
use codex_workflow_extension::WorkflowMetadata;
use codex_workflow_extension::WorkflowName;
use codex_workflow_extension::WorkflowOutput;
use codex_workflow_extension::WorkflowRecovery;
use codex_workflow_extension::WorkflowRegistry;
use codex_workflow_extension::WorkflowRegistryBuilder;
use codex_workflow_extension::WorkflowRunId;
use codex_workflow_extension::WorkflowState;
use codex_workflow_extension::WorkflowTransition;
use codex_workflow_extension::WorkflowVersion;
use pretty_assertions::assert_eq;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use tokio::sync::Notify;

#[derive(Clone, Debug, Default, Parser, Serialize, Deserialize, JsonSchema)]
struct CancellationArguments {}

impl WorkflowArguments for CancellationArguments {
    fn validate(&self) -> Result<(), ArgumentError> {
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CancellationState {}

impl WorkflowState for CancellationState {
    const SCHEMA_VERSION: u32 = 1;
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
struct CancellationOutput {}

impl WorkflowOutput for CancellationOutput {}

struct CancellationAwareWorkflow {
    started: Arc<Notify>,
    observed: Arc<AtomicBool>,
}

impl Workflow for CancellationAwareWorkflow {
    type Arguments = CancellationArguments;
    type State = CancellationState;
    type Output = CancellationOutput;

    fn metadata(&self) -> WorkflowMetadata {
        WorkflowMetadata::new(
            WorkflowName::new("cancellation-aware").expect("workflow name"),
            WorkflowVersion::parse("1.0.0").expect("workflow version"),
            "Cancellation propagation test workflow",
        )
        .with_default(true)
    }

    fn initialize(
        &self,
        _args: Self::Arguments,
    ) -> Result<Self::State, codex_workflow_extension::WorkflowError> {
        Ok(CancellationState {})
    }

    async fn step(
        &self,
        ctx: WorkflowContext<'_>,
        _state: Self::State,
    ) -> Result<
        WorkflowTransition<Self::State, Self::Output>,
        codex_workflow_extension::WorkflowError,
    > {
        self.started.notify_waiters();
        ctx.cancellation().cancelled().await;
        self.observed.store(true, Ordering::Release);
        Ok(WorkflowTransition::Complete {
            output: CancellationOutput {},
        })
    }
}

#[tokio::test]
async fn cancellation_is_durable_interrupts_turns_releases_observers_and_preserves_history() {
    let (_home, runtime) = support::runtime().await;
    let run_id = WorkflowRunId::new();
    support::create_run(
        runtime.as_ref(),
        run_id,
        "cancellation-test",
        "1.0.0",
        1,
        json!({}),
    )
    .await;
    let host = Arc::new(support::TestHost::default());
    let service = support::service(
        runtime.as_ref(),
        Arc::clone(&host),
        Arc::new(WorkflowRegistry::default()),
    );
    let node = service
        .nodes(run_id)
        .ensure(
            EffectKey::new("cancel-node").expect("effect key"),
            NodeSpec::builder(NodeKey::new("cancel-node").expect("node key"))
                .build()
                .expect("node spec"),
        )
        .await
        .expect("ensure cancellation node");
    let turn = node
        .start(
            EffectKey::new("cancel-turn").expect("effect key"),
            NodeInput::text("wait for cancellation"),
        )
        .await
        .expect("start cancellation turn");
    let controller = WorkflowCancellationController::new(service.clone());
    let report = controller
        .cancel_run(
            run_id,
            CancellationReason::WorkflowRequested,
            200,
            Duration::from_secs(1),
        )
        .await
        .expect("cancel workflow run");
    assert_eq!(report.run.status, WorkflowRunStatus::Cancelled);
    assert_eq!(report.interrupted_turns, 1);
    assert_eq!(report.ambiguous_turns, 0);
    assert_eq!(host.cancels.load(Ordering::Acquire), 1);
    assert_eq!(host.deletes.load(Ordering::Acquire), 0);
    assert_eq!(
        runtime
            .workflows()
            .read_node_attempt_by_turn_id(&node.id().to_string(), &turn.turn_id)
            .await
            .expect("read cancelled attempt")
            .expect("cancelled attempt")
            .status,
        WorkflowNodeAttemptStatus::Interrupted
    );
    assert_eq!(node.thread_refs().await.expect("thread refs").len(), 1);

    let replay = controller
        .cancel_run(
            run_id,
            CancellationReason::Operator("repeat".to_string()),
            201,
            Duration::from_millis(1),
        )
        .await
        .expect("replay cancellation");
    assert_eq!(replay.run.status, WorkflowRunStatus::Cancelled);
    assert_eq!(host.cancels.load(Ordering::Acquire), 1);
    runtime.close().await;
}

#[tokio::test]
async fn cancellation_grace_expiry_marks_attempt_ambiguous_without_deleting_session() {
    let (_home, runtime) = support::runtime().await;
    let run_id = WorkflowRunId::new();
    support::create_run(
        runtime.as_ref(),
        run_id,
        "cancellation-timeout",
        "1.0.0",
        1,
        json!({}),
    )
    .await;
    let host = Arc::new(support::TestHost::default());
    host.set_await_delay(Duration::from_millis(50));
    let service = support::service(
        runtime.as_ref(),
        Arc::clone(&host),
        Arc::new(WorkflowRegistry::default()),
    );
    let node = service
        .nodes(run_id)
        .ensure(
            EffectKey::new("timeout-node").expect("effect key"),
            NodeSpec::builder(NodeKey::new("timeout-node").expect("node key"))
                .build()
                .expect("node spec"),
        )
        .await
        .expect("ensure timeout node");
    let turn = node
        .start(
            EffectKey::new("timeout-turn").expect("effect key"),
            NodeInput::text("ignore cancellation"),
        )
        .await
        .expect("start timeout turn");
    let report = WorkflowCancellationController::new(service)
        .cancel_run(
            run_id,
            CancellationReason::DeadlineExceeded,
            300,
            Duration::from_millis(1),
        )
        .await
        .expect("finish bounded cancellation");
    assert_eq!(report.ambiguous_turns, 1);
    assert_eq!(report.run.status, WorkflowRunStatus::Cancelled);
    assert_eq!(
        runtime
            .workflows()
            .read_node_attempt_by_turn_id(&node.id().to_string(), &turn.turn_id)
            .await
            .expect("read ambiguous attempt")
            .expect("ambiguous attempt")
            .status,
        WorkflowNodeAttemptStatus::Ambiguous
    );
    assert_eq!(host.deletes.load(Ordering::Acquire), 0);
    assert_eq!(node.thread_refs().await.expect("preserved thread").len(), 1);
    runtime.close().await;
}

#[tokio::test]
async fn durable_cancellation_wakes_process_effect_waiters() {
    let (_home, runtime) = support::runtime().await;
    let run_id = WorkflowRunId::new();
    support::create_run(
        runtime.as_ref(),
        run_id,
        "cancellation-aware",
        "1.0.0",
        1,
        json!({}),
    )
    .await;
    let started = Arc::new(Notify::new());
    let observed = Arc::new(AtomicBool::new(false));
    let mut registry = WorkflowRegistryBuilder::new();
    registry
        .register(CancellationAwareWorkflow {
            started: Arc::clone(&started),
            observed: Arc::clone(&observed),
        })
        .expect("register cancellation workflow");
    let registry = Arc::new(registry.build().expect("build cancellation registry"));
    let service = support::service(
        runtime.as_ref(),
        Arc::new(support::TestHost::default()),
        Arc::clone(&registry),
    );
    let recovery = WorkflowRecovery::new(service.clone(), registry, "cancellation-driver", 1_000);
    let started_wait = started.notified();
    let driver = tokio::spawn(async move { recovery.recover_nonterminal_runs(400).await });
    tokio::time::timeout(Duration::from_secs(1), started_wait)
        .await
        .expect("reducer started");

    let report = WorkflowCancellationController::new(service)
        .cancel_run(
            run_id,
            CancellationReason::WorkflowRequested,
            401,
            Duration::from_millis(10),
        )
        .await
        .expect("cancel active reducer");
    assert_eq!(report.run.status, WorkflowRunStatus::Cancelled);
    assert!(
        tokio::time::timeout(Duration::from_secs(1), driver)
            .await
            .expect("driver observed cancellation")
            .expect("join cancellation driver")
            .is_err(),
        "the stale reducer transition must be fenced after cancellation"
    );
    assert!(observed.load(Ordering::Acquire));
    runtime.close().await;
}
