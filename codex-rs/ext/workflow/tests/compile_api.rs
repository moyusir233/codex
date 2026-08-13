mod downstream_definition {
    use clap::Parser;
    use codex_workflow_extension::ArgumentError;
    use codex_workflow_extension::Workflow;
    use codex_workflow_extension::WorkflowArguments;
    use codex_workflow_extension::WorkflowContext;
    use codex_workflow_extension::WorkflowError;
    use codex_workflow_extension::WorkflowMetadata;
    use codex_workflow_extension::WorkflowName;
    use codex_workflow_extension::WorkflowOutput;
    use codex_workflow_extension::WorkflowState;
    use codex_workflow_extension::WorkflowTransition;
    use codex_workflow_extension::WorkflowVersion;
    use schemars::JsonSchema;
    use serde::Deserialize;
    use serde::Serialize;

    #[derive(Parser, Serialize, Deserialize, JsonSchema)]
    pub struct Arguments {
        #[arg(long)]
        pub value: String,
    }

    impl WorkflowArguments for Arguments {
        fn validate(&self) -> Result<(), ArgumentError> {
            Ok(())
        }
    }

    #[derive(Serialize, Deserialize)]
    pub struct State {
        value: String,
    }

    impl WorkflowState for State {
        const SCHEMA_VERSION: u32 = 1;
    }

    #[derive(Serialize, Deserialize, JsonSchema)]
    pub struct Output {
        value: String,
    }

    impl WorkflowOutput for Output {}

    pub struct Definition;

    impl Workflow for Definition {
        type Arguments = Arguments;
        type State = State;
        type Output = Output;

        fn metadata(&self) -> WorkflowMetadata {
            WorkflowMetadata::new(
                WorkflowName::new("downstream")
                    .unwrap_or_else(|err| panic!("downstream name must be valid: {err}")),
                WorkflowVersion::parse("1.0.0")
                    .unwrap_or_else(|err| panic!("downstream version must be valid: {err}")),
                "External-style definition.",
            )
            .with_default(true)
        }

        fn initialize(&self, args: Self::Arguments) -> Result<Self::State, WorkflowError> {
            Ok(State { value: args.value })
        }

        async fn step(
            &self,
            ctx: WorkflowContext<'_>,
            state: Self::State,
        ) -> Result<WorkflowTransition<Self::State, Self::Output>, WorkflowError> {
            let _nodes = ctx.nodes();
            let _artifacts = ctx.artifacts();
            let _approvals = ctx.approvals();
            let _goals = ctx.goals();
            let _lark_feature = ctx.lark_feature();
            let _audit = ctx.audit();
            Ok(WorkflowTransition::Complete {
                output: Output { value: state.value },
            })
        }
    }
}

#[test]
fn compile_api_external_style_module_implements_public_api() {
    fn assert_workflow<W: codex_workflow_extension::Workflow>(_workflow: W) {}
    fn assert_goal_capability<T: ?Sized + codex_workflow_extension::WorkflowGoalCapability>() {}

    assert_workflow(downstream_definition::Definition);
    assert_goal_capability::<dyn codex_workflow_extension::WorkflowGoalCapability>();
    let _ = std::mem::size_of::<codex_workflow_extension::WorkflowGoalEnsureRequest>();
    let _ = std::mem::size_of::<codex_workflow_extension::WorkflowGoalSnapshot>();
    let _ = std::mem::size_of::<codex_workflow_extension::WorkflowGoalDeliveryEvidence>();
}
