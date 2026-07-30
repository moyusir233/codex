use std::ffi::OsString;

use clap::Parser;
use codex_workflow_extension::EffectKey;
use codex_workflow_extension::IdentifierError;
use codex_workflow_extension::InteractionId;
use codex_workflow_extension::NodeAttemptId;
use codex_workflow_extension::NodeId;
use codex_workflow_extension::NodeKey;
use codex_workflow_extension::Workflow;
use codex_workflow_extension::WorkflowArguments;
use codex_workflow_extension::WorkflowContext;
use codex_workflow_extension::WorkflowError;
use codex_workflow_extension::WorkflowMetadata;
use codex_workflow_extension::WorkflowName;
use codex_workflow_extension::WorkflowOutput;
use codex_workflow_extension::WorkflowRegistryBuilder;
use codex_workflow_extension::WorkflowRunId;
use codex_workflow_extension::WorkflowSequence;
use codex_workflow_extension::WorkflowState;
use codex_workflow_extension::WorkflowTransition;
use codex_workflow_extension::WorkflowVersion;
use pretty_assertions::assert_eq;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Parser, Serialize, Deserialize, JsonSchema)]
#[command(name = "counter")]
struct CounterArguments {
    #[arg(long)]
    count: u32,
}

impl WorkflowArguments for CounterArguments {
    fn validate(&self) -> Result<(), codex_workflow_extension::ArgumentError> {
        if self.count == 0 {
            return Err(codex_workflow_extension::ArgumentError::validation(
                "count must be positive",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct CounterState {
    count: u32,
}

impl WorkflowState for CounterState {
    const SCHEMA_VERSION: u32 = 3;
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
struct CounterOutput {
    count: u32,
}

impl WorkflowOutput for CounterOutput {}

struct CounterWorkflow;

impl Workflow for CounterWorkflow {
    type Arguments = CounterArguments;
    type State = CounterState;
    type Output = CounterOutput;

    fn metadata(&self) -> WorkflowMetadata {
        WorkflowMetadata::new(
            WorkflowName::new("counter")
                .unwrap_or_else(|err| panic!("counter name must be valid: {err}")),
            WorkflowVersion::parse("1.2.3")
                .unwrap_or_else(|err| panic!("counter version must be valid: {err}")),
            "Counts durably.",
        )
        .with_default(true)
        .with_required_capabilities(["codex.nodes", "example.counter"])
    }

    fn initialize(&self, args: Self::Arguments) -> Result<Self::State, WorkflowError> {
        Ok(CounterState { count: args.count })
    }

    async fn step(
        &self,
        _ctx: WorkflowContext<'_>,
        state: Self::State,
    ) -> Result<WorkflowTransition<Self::State, Self::Output>, WorkflowError> {
        Ok(WorkflowTransition::Complete {
            output: CounterOutput { count: state.count },
        })
    }
}

#[test]
fn definition_parses_schema_and_round_trips_checkpoint() {
    let mut builder = WorkflowRegistryBuilder::new();
    builder.register(CounterWorkflow).expect("register");
    let registry = builder.build().expect("build");
    let definition = registry
        .resolve(&WorkflowName::new("counter").expect("name"), None)
        .expect("default definition");

    let arguments = definition
        .parse_cli(&[OsString::from("--count"), OsString::from("4")])
        .expect("parse arguments");
    let checkpoint = definition.initialize(arguments).expect("initialize");
    assert_eq!(3, checkpoint.state_schema_version());
    assert_eq!(&serde_json::json!({"count": 4}), checkpoint.state());

    let encoded = serde_json::to_string(&checkpoint).expect("serialize checkpoint");
    let decoded = serde_json::from_str(&encoded).expect("restore checkpoint");
    assert_eq!(checkpoint, decoded);
    definition
        .validate_checkpoint(&checkpoint)
        .expect("matching state version");

    let metadata = definition.metadata();
    assert!(metadata.argv_help().contains("--count"));
    assert_eq!(Some("object"), metadata.argument_schema()["type"].as_str());
    assert_eq!(Some("object"), metadata.output_schema()["type"].as_str());
    assert_eq!(
        metadata.required_capabilities(),
        ["codex.nodes", "example.counter"]
    );
}

#[test]
fn definition_rejects_invalid_arguments_and_unsupported_checkpoint_versions() {
    let mut builder = WorkflowRegistryBuilder::new();
    builder.register(CounterWorkflow).expect("register");
    let registry = builder.build().expect("build");
    let definition = registry
        .resolve(&WorkflowName::new("counter").expect("name"), None)
        .expect("definition");

    assert!(
        definition
            .parse_cli(&[OsString::from("--count"), OsString::from("0")])
            .is_err()
    );
    assert_eq!(
        Err(WorkflowError::UnsupportedStateVersion {
            expected: 3,
            actual: 2,
        }),
        definition.validate_checkpoint(&codex_workflow_extension::WorkflowCheckpoint::new(
            2,
            serde_json::json!({"count": 4}),
        ))
    );
}

#[test]
fn definition_identifiers_reject_invalid_names_versions_keys_and_ids() {
    assert!(WorkflowName::new("PromptReview").is_err());
    assert!(WorkflowName::new("prompt--review").is_err());
    assert!(WorkflowVersion::parse("1.0").is_err());
    assert!(NodeKey::new("../escape").is_err());
    assert!(EffectKey::new("effect//duplicate").is_err());
    assert_eq!(Err(IdentifierError::ZeroSequence), WorkflowSequence::new(0));

    let v4 = "550e8400-e29b-41d4-a716-446655440000";
    assert!(WorkflowRunId::parse(v4).is_err());
    assert!(NodeId::parse(v4).is_err());
    assert!(NodeAttemptId::parse(v4).is_err());
    assert!(InteractionId::parse(v4).is_err());
    assert!(serde_json::from_str::<WorkflowRunId>(&format!("\"{v4}\"")).is_err());
    assert!(serde_json::from_str::<WorkflowSequence>("0").is_err());
    assert_eq!(1, WorkflowSequence::new(1).expect("sequence").get());
}
