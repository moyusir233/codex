#![allow(clippy::expect_used)]

mod support;

use std::collections::BTreeSet;
use std::num::NonZeroUsize;
use std::sync::Arc;

use clap::Parser;
use codex_state::WorkflowRunStatus;
use codex_workflow_extension::ArgumentError;
use codex_workflow_extension::DriveOutcome;
use codex_workflow_extension::Workflow;
use codex_workflow_extension::WorkflowArguments;
use codex_workflow_extension::WorkflowContext;
use codex_workflow_extension::WorkflowMetadata;
use codex_workflow_extension::WorkflowName;
use codex_workflow_extension::WorkflowOutput;
use codex_workflow_extension::WorkflowRegistryBuilder;
use codex_workflow_extension::WorkflowRunId;
use codex_workflow_extension::WorkflowState;
use codex_workflow_extension::WorkflowTransition;
use codex_workflow_extension::WorkflowVersion;
use codex_workflow_extension::runtime::WorkflowDriver;
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct State {
    operator_checkpointed: bool,
}

impl WorkflowState for State {
    const SCHEMA_VERSION: u32 = 1;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct Output {
    resumed_from_operator_checkpoint: bool,
}

impl WorkflowOutput for Output {}

struct OperatorRecoveryWorkflow;

impl Workflow for OperatorRecoveryWorkflow {
    type Arguments = Arguments;
    type State = State;
    type Output = Output;

    fn metadata(&self) -> WorkflowMetadata {
        WorkflowMetadata::new(
            WorkflowName::new("lark-feature-recovery-probe").expect("name"),
            WorkflowVersion::parse("1.0.0").expect("version"),
            "Proves reducer-owned needs-operator checkpoints resume durably.",
        )
        .with_default(true)
    }

    fn initialize(
        &self,
        _args: Self::Arguments,
    ) -> Result<Self::State, codex_workflow_extension::WorkflowError> {
        Ok(State {
            operator_checkpointed: false,
        })
    }

    async fn step(
        &self,
        _ctx: WorkflowContext<'_>,
        state: Self::State,
    ) -> Result<
        WorkflowTransition<Self::State, Self::Output>,
        codex_workflow_extension::WorkflowError,
    > {
        if !state.operator_checkpointed {
            return Ok(WorkflowTransition::NeedsOperator {
                state: State {
                    operator_checkpointed: true,
                },
                error_code: "lark_feature_fixture_needs_operator".to_string(),
                metadata: json!({"stage":"technical_design","evidence":"artifact-only"}),
            });
        }
        Ok(WorkflowTransition::Complete {
            output: Output {
                resumed_from_operator_checkpoint: true,
            },
        })
    }
}

#[tokio::test]
async fn lark_feature_recovery_needs_operator_checkpoint_survives_new_driver()
-> Result<(), Box<dyn std::error::Error>> {
    let (_home, runtime) = support::runtime().await;
    let mut builder = WorkflowRegistryBuilder::new();
    builder.register(OperatorRecoveryWorkflow)?;
    let registry = Arc::new(builder.build()?);
    let run_id = WorkflowRunId::new();
    support::create_run(
        runtime.as_ref(),
        run_id,
        "lark-feature-recovery-probe",
        "1.0.0",
        1,
        json!({"operator_checkpointed":false}),
    )
    .await;
    let first = WorkflowDriver::new(
        runtime.workflows().clone(),
        registry.clone(),
        "recovery-before-restart",
        30_000,
    );
    assert_eq!(
        first
            .drive_until_blocked(run_id, 1_000, NonZeroUsize::new(4).expect("steps"))
            .await?,
        DriveOutcome::NeedsOperator
    );
    let checkpoint = runtime
        .workflows()
        .read_run(&run_id.to_string())
        .await?
        .expect("run");
    assert_eq!(checkpoint.status, WorkflowRunStatus::NeedsOperator);
    assert_eq!(
        checkpoint.error_code.as_deref(),
        Some("lark_feature_fixture_needs_operator")
    );
    assert_eq!(checkpoint.state["operator_checkpointed"], true);
    let events = runtime
        .workflows()
        .events_after(&run_id.to_string(), 0, 20)
        .await?;
    assert!(events.iter().any(|event| {
        event.kind == "run.needs_operator" && event.metadata["evidence"] == "artifact-only"
    }));

    runtime
        .workflows()
        .resume_run(&run_id.to_string(), 2_000)
        .await?;
    let restarted = WorkflowDriver::new(
        runtime.workflows().clone(),
        registry,
        "recovery-after-restart",
        30_000,
    );
    assert_eq!(
        restarted
            .drive_until_blocked(run_id, 2_001, NonZeroUsize::new(4).expect("steps"))
            .await?,
        DriveOutcome::Completed
    );
    let completed = runtime
        .workflows()
        .read_run(&run_id.to_string())
        .await?
        .expect("run");
    assert_eq!(completed.status, WorkflowRunStatus::Succeeded);
    assert_eq!(
        completed.output.expect("output")["resumed_from_operator_checkpoint"],
        true
    );
    Ok(())
}

#[test]
fn lark_feature_recovery_matrix_names_every_journal_boundary_once() {
    let boundaries: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "fixtures/lark_feature/scenarios/crash-boundaries.json"
    ))
    .expect("crash-boundary fixture");
    assert!(boundaries.len() >= 19);
    let names = boundaries
        .iter()
        .map(|entry| entry["boundary"].as_str().expect("boundary"))
        .collect::<BTreeSet<_>>();
    assert_eq!(names.len(), boundaries.len());
    assert!(boundaries.iter().all(|entry| {
        entry["max_logical_resources"] == 0 || entry["max_logical_resources"] == 1
    }));
    for required in [
        "group.dispatched",
        "document.create.dispatched",
        "turn.prepared",
        "approval.response.resolved",
        "goal.dispatched",
        "trace.finish.dispatched",
        "delivery.before_commit",
    ] {
        assert!(
            names.contains(required),
            "missing recovery boundary {required}"
        );
    }
}
