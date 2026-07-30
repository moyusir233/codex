use clap::Parser;
use codex_workflow_extension::RegistryError;
use codex_workflow_extension::Workflow;
use codex_workflow_extension::WorkflowArguments;
use codex_workflow_extension::WorkflowContext;
use codex_workflow_extension::WorkflowError;
use codex_workflow_extension::WorkflowMetadata;
use codex_workflow_extension::WorkflowName;
use codex_workflow_extension::WorkflowOutput;
use codex_workflow_extension::WorkflowRegistryBuilder;
use codex_workflow_extension::WorkflowState;
use codex_workflow_extension::WorkflowTransition;
use codex_workflow_extension::WorkflowVersion;
use pretty_assertions::assert_eq;
use schemars::JsonSchema;
use schemars::schema::Schema;
use schemars::schema::SchemaObject;
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Parser, Serialize, Deserialize, JsonSchema)]
struct EmptyArguments {}

impl WorkflowArguments for EmptyArguments {
    fn validate(&self) -> Result<(), codex_workflow_extension::ArgumentError> {
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct State;

impl WorkflowState for State {
    const SCHEMA_VERSION: u32 = 1;
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
struct Output;

impl WorkflowOutput for Output {}

struct TestWorkflow {
    name: &'static str,
    version: &'static str,
    is_default: bool,
    description: &'static str,
}

impl Workflow for TestWorkflow {
    type Arguments = EmptyArguments;
    type State = State;
    type Output = Output;

    fn metadata(&self) -> WorkflowMetadata {
        WorkflowMetadata::new(
            WorkflowName::new(self.name)
                .unwrap_or_else(|err| panic!("test name must be valid: {err}")),
            WorkflowVersion::parse(self.version)
                .unwrap_or_else(|err| panic!("test version must be valid: {err}")),
            self.description,
        )
        .with_default(self.is_default)
    }

    fn initialize(&self, _args: Self::Arguments) -> Result<Self::State, WorkflowError> {
        Ok(State)
    }

    async fn step(
        &self,
        _ctx: WorkflowContext<'_>,
        state: Self::State,
    ) -> Result<WorkflowTransition<Self::State, Self::Output>, WorkflowError> {
        Ok(WorkflowTransition::Continue { state })
    }
}

#[test]
fn registry_orders_versions_and_resolves_explicit_default() {
    let mut builder = WorkflowRegistryBuilder::new();
    builder
        .register(workflow("zeta", "2.0.0", true))
        .expect("zeta");
    builder
        .register(workflow("alpha", "2.0.0", true))
        .expect("alpha 2");
    builder
        .register(workflow("alpha", "1.0.0", false))
        .expect("alpha 1");
    let registry = builder.build().expect("registry");

    let ordered = registry
        .definitions()
        .map(|definition| {
            format!(
                "{}@{}",
                definition.metadata().name(),
                definition.metadata().version()
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(ordered, ["alpha@1.0.0", "alpha@2.0.0", "zeta@2.0.0"]);
    assert_eq!(
        "2.0.0",
        registry
            .resolve(&WorkflowName::new("alpha").expect("name"), None)
            .expect("default")
            .metadata()
            .version()
            .to_string()
    );
}

#[test]
fn registry_rejects_duplicate_versions_defaults_and_missing_defaults() {
    let mut duplicate_version = WorkflowRegistryBuilder::new();
    duplicate_version
        .register(workflow("alpha", "1.0.0", true))
        .expect("first");
    assert!(matches!(
        duplicate_version.register(workflow("alpha", "1.0.0", false)),
        Err(RegistryError::DuplicateVersion { .. })
    ));

    let mut duplicate_default = WorkflowRegistryBuilder::new();
    duplicate_default
        .register(workflow("alpha", "1.0.0", true))
        .expect("first");
    assert!(matches!(
        duplicate_default.register(workflow("alpha", "2.0.0", true)),
        Err(RegistryError::DuplicateDefault { .. })
    ));

    let mut missing_default = WorkflowRegistryBuilder::new();
    missing_default
        .register(workflow("alpha", "1.0.0", false))
        .expect("register");
    assert!(matches!(
        missing_default.build(),
        Err(RegistryError::MissingDefault { .. })
    ));
}

#[test]
fn registry_rejects_invalid_metadata_and_unresolved_schema_refs() {
    let mut invalid_description = WorkflowRegistryBuilder::new();
    assert!(matches!(
        invalid_description.register(workflow("alpha", "1.0.0", true).with_description(" ")),
        Err(RegistryError::InvalidDescription { .. })
    ));

    let mut invalid_schema = WorkflowRegistryBuilder::new();
    assert!(matches!(
        invalid_schema.register(InvalidSchemaWorkflow),
        Err(RegistryError::InvalidSchema {
            target: "output",
            ..
        })
    ));
}

fn workflow(name: &'static str, version: &'static str, is_default: bool) -> TestWorkflow {
    TestWorkflow {
        name,
        version,
        is_default,
        description: "Test workflow.",
    }
}

impl TestWorkflow {
    fn with_description(mut self, description: &'static str) -> Self {
        self.description = description;
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct InvalidOutput;

impl JsonSchema for InvalidOutput {
    fn schema_name() -> String {
        "InvalidOutput".to_string()
    }

    fn json_schema(_generator: &mut schemars::r#gen::SchemaGenerator) -> Schema {
        Schema::Object(SchemaObject {
            reference: Some("#/definitions/missing".to_string()),
            ..Default::default()
        })
    }
}

impl WorkflowOutput for InvalidOutput {}

struct InvalidSchemaWorkflow;

impl Workflow for InvalidSchemaWorkflow {
    type Arguments = EmptyArguments;
    type State = State;
    type Output = InvalidOutput;

    fn metadata(&self) -> WorkflowMetadata {
        WorkflowMetadata::new(
            WorkflowName::new("invalid-schema")
                .unwrap_or_else(|err| panic!("test name must be valid: {err}")),
            WorkflowVersion::parse("1.0.0")
                .unwrap_or_else(|err| panic!("test version must be valid: {err}")),
            "Invalid schema.",
        )
        .with_default(true)
    }

    fn initialize(&self, _args: Self::Arguments) -> Result<Self::State, WorkflowError> {
        Ok(State)
    }

    async fn step(
        &self,
        _ctx: WorkflowContext<'_>,
        state: Self::State,
    ) -> Result<WorkflowTransition<Self::State, Self::Output>, WorkflowError> {
        Ok(WorkflowTransition::Continue { state })
    }
}
