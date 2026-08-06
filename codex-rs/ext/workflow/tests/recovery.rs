#![allow(clippy::expect_used)]

mod support;

use std::sync::Arc;

use clap::Parser;
use codex_protocol::ThreadId;
use codex_state::WorkflowNodeAttemptCreate;
use codex_state::WorkflowNodeAttemptStatus;
use codex_state::WorkflowNodeAttemptTransition;
use codex_state::WorkflowNodeCreate;
use codex_state::WorkflowNodeStatus;
use codex_state::WorkflowRunStatus;
use codex_workflow_extension::ArgumentError;
use codex_workflow_extension::NodeKey;
use codex_workflow_extension::NodeSpec;
use codex_workflow_extension::NodeTurnResult;
use codex_workflow_extension::NodeTurnStatus;
use codex_workflow_extension::RecoveredTurnState;
use codex_workflow_extension::Workflow;
use codex_workflow_extension::WorkflowArguments;
use codex_workflow_extension::WorkflowContext;
use codex_workflow_extension::WorkflowMetadata;
use codex_workflow_extension::WorkflowName;
use codex_workflow_extension::WorkflowOutput;
use codex_workflow_extension::WorkflowRecovery;
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

#[derive(Clone, Debug, Default, Parser, Serialize, Deserialize, JsonSchema)]
struct Arguments {}

impl WorkflowArguments for Arguments {
    fn validate(&self) -> Result<(), ArgumentError> {
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct State {
    step: u32,
}

impl WorkflowState for State {
    const SCHEMA_VERSION: u32 = 1;
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
struct Output {
    step: u32,
}

impl WorkflowOutput for Output {}

struct RecoveryWorkflow;

impl Workflow for RecoveryWorkflow {
    type Arguments = Arguments;
    type State = State;
    type Output = Output;

    fn metadata(&self) -> WorkflowMetadata {
        WorkflowMetadata::new(
            WorkflowName::new("recovery-test").expect("workflow name"),
            WorkflowVersion::parse("1.0.0").expect("workflow version"),
            "Recovery test workflow",
        )
        .with_default(true)
    }

    fn initialize(
        &self,
        _args: Self::Arguments,
    ) -> Result<Self::State, codex_workflow_extension::WorkflowError> {
        Ok(State { step: 0 })
    }

    async fn step(
        &self,
        _ctx: WorkflowContext<'_>,
        state: Self::State,
    ) -> Result<
        WorkflowTransition<Self::State, Self::Output>,
        codex_workflow_extension::WorkflowError,
    > {
        if state.step == 0 {
            Ok(WorkflowTransition::Continue {
                state: State { step: 1 },
            })
        } else {
            Ok(WorkflowTransition::Complete {
                output: Output { step: state.step },
            })
        }
    }
}

#[tokio::test]
async fn recovery_repairs_thread_and_terminal_attempt_then_drives_exact_definition_once() {
    let (_home, runtime) = support::runtime().await;
    let run_id = WorkflowRunId::new();
    support::create_run(
        runtime.as_ref(),
        run_id,
        "recovery-test",
        "1.0.0",
        1,
        json!({"step": 0}),
    )
    .await;
    let node_id = codex_workflow_extension::NodeId::new();
    let spec = NodeSpec::builder(NodeKey::new("primary").expect("node key"))
        .build()
        .expect("node spec");
    runtime
        .workflows()
        .create_node(
            &run_id.to_string(),
            WorkflowNodeCreate {
                node_id: node_id.to_string(),
                node_key: "primary".to_string(),
                spec: serde_json::to_value(spec).expect("serialize node spec"),
                status: WorkflowNodeStatus::Running,
                failure_policy: "fail_fast".to_string(),
                created_at_ms: 101,
            },
            &[],
        )
        .await
        .expect("create recovery node");
    let thread_id = ThreadId::new();
    let attempt_id = codex_workflow_extension::NodeAttemptId::new().to_string();
    runtime
        .workflows()
        .create_node_attempt(
            &run_id.to_string(),
            WorkflowNodeAttemptCreate {
                attempt_id: attempt_id.clone(),
                node_id: node_id.to_string(),
                submission_id: attempt_id.clone(),
                input_hash: "11".repeat(32),
                thread_id: Some(thread_id.to_string()),
                created_at_ms: 102,
            },
        )
        .await
        .expect("create prepared attempt");
    runtime
        .workflows()
        .transition_node_attempt(
            &attempt_id,
            WorkflowNodeAttemptTransition {
                expected_status: WorkflowNodeAttemptStatus::Planned,
                status: WorkflowNodeAttemptStatus::Submitted,
                turn_id: Some(attempt_id.clone()),
                error_code: None,
                updated_at_ms: 103,
            },
        )
        .await
        .expect("submit attempt");

    let host = Arc::new(support::TestHost::with_thread(thread_id));
    host.set_recovery(
        &attempt_id,
        RecoveredTurnState::Terminal(NodeTurnResult {
            turn_id: attempt_id.clone(),
            status: NodeTurnStatus::Completed,
            final_output: Some("done".to_string()),
            error: None,
        }),
    );
    let mut registry = WorkflowRegistryBuilder::new();
    registry
        .register(RecoveryWorkflow)
        .expect("register recovery workflow");
    let registry = Arc::new(registry.build().expect("build recovery registry"));
    let service = support::service(runtime.as_ref(), Arc::clone(&host), Arc::clone(&registry));
    let recovery = WorkflowRecovery::new(service, registry, "recovery-owner", 1_000);

    let first = recovery
        .recover_nonterminal_runs(200)
        .await
        .expect("first recovery");
    assert_eq!(first.repaired_thread_bindings, 1);
    assert_eq!(first.reconciled_terminal_attempts, 1);
    assert_eq!(first.driven_runs, 1);
    assert_eq!(host.detaches.load(std::sync::atomic::Ordering::Acquire), 1);
    assert_eq!(
        runtime
            .workflows()
            .read_node_attempt(&attempt_id)
            .await
            .expect("read attempt")
            .expect("attempt")
            .status,
        WorkflowNodeAttemptStatus::Succeeded
    );
    assert_eq!(
        runtime
            .workflows()
            .read_node(&node_id.to_string())
            .await
            .expect("read node")
            .expect("node")
            .thread_id,
        Some(thread_id.to_string())
    );

    let second = recovery
        .recover_nonterminal_runs(201)
        .await
        .expect("second recovery");
    assert_eq!(second.reconciled_terminal_attempts, 0);
    assert_eq!(
        runtime
            .workflows()
            .read_run(&run_id.to_string())
            .await
            .expect("read run")
            .expect("run")
            .status,
        WorkflowRunStatus::Succeeded
    );
    let third = recovery
        .recover_nonterminal_runs(202)
        .await
        .expect("terminal recovery");
    assert_eq!(third.inspected_runs, 0);
    assert_eq!(
        host.materializations
            .load(std::sync::atomic::Ordering::Acquire),
        0
    );
    assert_eq!(
        host.unique_queues
            .load(std::sync::atomic::Ordering::Acquire),
        0
    );
    runtime.close().await;
}

#[tokio::test]
async fn recovery_fails_closed_for_missing_definition() {
    let (_home, runtime) = support::runtime().await;
    let missing_run = WorkflowRunId::new();
    support::create_run(
        runtime.as_ref(),
        missing_run,
        "missing-definition",
        "1.0.0",
        1,
        json!({}),
    )
    .await;
    let host = Arc::new(support::TestHost::default());
    let service = support::service(
        runtime.as_ref(),
        host,
        Arc::new(codex_workflow_extension::WorkflowRegistry::default()),
    );
    let recovery = WorkflowRecovery::new(
        service,
        Arc::new(codex_workflow_extension::WorkflowRegistry::default()),
        "missing-owner",
        1_000,
    );
    let report = recovery
        .recover_nonterminal_runs(200)
        .await
        .expect("missing definition recovery");
    assert_eq!(report.needs_operator_runs, 1);
    let run = runtime
        .workflows()
        .read_run(&missing_run.to_string())
        .await
        .expect("read missing run")
        .expect("missing run");
    assert_eq!(run.status, WorkflowRunStatus::NeedsOperator);
    assert_eq!(run.error_code.as_deref(), Some("definition_unavailable"));
    runtime.close().await;
}

#[tokio::test]
async fn recovery_advances_planned_unterminated_turn_before_scheduling_retry() {
    let (_home, runtime) = support::runtime().await;
    let run_id = WorkflowRunId::new();
    support::create_run(
        runtime.as_ref(),
        run_id,
        "recovery-test",
        "1.0.0",
        1,
        json!({"step": 0}),
    )
    .await;
    let node_id = codex_workflow_extension::NodeId::new();
    let thread_id = ThreadId::new();
    let spec = NodeSpec::builder(NodeKey::new("interrupted").expect("node key"))
        .build()
        .expect("node spec");
    let node = runtime
        .workflows()
        .create_node(
            &run_id.to_string(),
            WorkflowNodeCreate {
                node_id: node_id.to_string(),
                node_key: "interrupted".to_string(),
                spec: serde_json::to_value(spec).expect("serialize node spec"),
                status: WorkflowNodeStatus::Running,
                failure_policy: "fail_fast".to_string(),
                created_at_ms: 101,
            },
            &[],
        )
        .await
        .expect("create interrupted node");
    runtime
        .workflows()
        .bind_node_thread(&node.node_id, node.row_version, &thread_id.to_string(), 102)
        .await
        .expect("bind interrupted thread");
    let attempt_id = codex_workflow_extension::NodeAttemptId::new().to_string();
    runtime
        .workflows()
        .create_node_attempt(
            &run_id.to_string(),
            WorkflowNodeAttemptCreate {
                attempt_id: attempt_id.clone(),
                node_id: node_id.to_string(),
                submission_id: attempt_id.clone(),
                input_hash: "22".repeat(32),
                thread_id: Some(thread_id.to_string()),
                created_at_ms: 103,
            },
        )
        .await
        .expect("create interrupted attempt");
    let host = Arc::new(support::TestHost::with_thread(thread_id));
    host.set_recovery(&attempt_id, RecoveredTurnState::Unterminated);
    let mut registry = WorkflowRegistryBuilder::new();
    registry
        .register(RecoveryWorkflow)
        .expect("register workflow");
    let registry = Arc::new(registry.build().expect("build registry"));
    let service = support::service(runtime.as_ref(), host, Arc::clone(&registry));
    let recovery = WorkflowRecovery::new(service, registry, "interrupted-owner", 1_000);
    let report = recovery
        .recover_nonterminal_runs(200)
        .await
        .expect("recover interrupted attempt");
    assert_eq!(report.interrupted_attempts, 1);
    assert_eq!(
        runtime
            .workflows()
            .read_node_attempt(&attempt_id)
            .await
            .expect("read interrupted attempt")
            .expect("interrupted attempt")
            .status,
        WorkflowNodeAttemptStatus::Interrupted
    );
    let node = runtime
        .workflows()
        .read_node(&node_id.to_string())
        .await
        .expect("read retry node")
        .expect("retry node");
    assert_eq!(node.status, WorkflowNodeStatus::Waiting);
    assert_eq!(node.retry_at_ms, Some(200));
    runtime.close().await;
}
