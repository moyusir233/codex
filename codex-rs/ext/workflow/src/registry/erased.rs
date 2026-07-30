use std::ffi::OsString;
use std::future::Future;
use std::pin::Pin;

use serde_json::Value;

use crate::api::ArgumentError;
use crate::api::Workflow;
use crate::api::WorkflowArguments;
use crate::api::WorkflowContext;
use crate::api::WorkflowError;
use crate::api::WorkflowTransition;
use crate::registry::WorkflowCheckpoint;

pub(crate) trait ErasedWorkflow: Send + Sync {
    fn parse_cli(&self, argv: &[OsString]) -> Result<Value, ArgumentError>;

    fn initialize(&self, arguments: Value) -> Result<WorkflowCheckpoint, WorkflowError>;

    fn step<'a>(
        &'a self,
        context: WorkflowContext<'a>,
        checkpoint: WorkflowCheckpoint,
    ) -> Pin<Box<dyn Future<Output = Result<ErasedWorkflowTransition, WorkflowError>> + Send + 'a>>;
}

pub(crate) struct ConcreteWorkflow<W> {
    workflow: W,
}

impl<W> ConcreteWorkflow<W> {
    pub(crate) fn new(workflow: W) -> Self {
        Self { workflow }
    }
}

impl<W> ErasedWorkflow for ConcreteWorkflow<W>
where
    W: Workflow,
{
    fn parse_cli(&self, argv: &[OsString]) -> Result<Value, ArgumentError> {
        let arguments = W::Arguments::parse_cli(argv)?;
        serde_json::to_value(arguments).map_err(|err| ArgumentError::Serialization(err.to_string()))
    }

    fn initialize(&self, arguments: Value) -> Result<WorkflowCheckpoint, WorkflowError> {
        let arguments: W::Arguments = serde_json::from_value(arguments)
            .map_err(|err| WorkflowError::Serialization(err.to_string()))?;
        arguments
            .validate()
            .map_err(|err| WorkflowError::Definition(err.to_string()))?;
        let state = self.workflow.initialize(arguments)?;
        WorkflowCheckpoint::from_state::<W::State>(&state)
    }

    fn step<'a>(
        &'a self,
        context: WorkflowContext<'a>,
        checkpoint: WorkflowCheckpoint,
    ) -> Pin<Box<dyn Future<Output = Result<ErasedWorkflowTransition, WorkflowError>> + Send + 'a>>
    {
        Box::pin(async move {
            let state = checkpoint.into_state::<W::State>()?;
            match self.workflow.step(context, state).await? {
                WorkflowTransition::Continue { state } => Ok(ErasedWorkflowTransition::Continue {
                    checkpoint: WorkflowCheckpoint::from_state::<W::State>(&state)?,
                }),
                WorkflowTransition::Wait { state, wake } => Ok(ErasedWorkflowTransition::Wait {
                    checkpoint: WorkflowCheckpoint::from_state::<W::State>(&state)?,
                    wake,
                }),
                WorkflowTransition::Complete { output } => {
                    let output = serde_json::to_value(output)
                        .map_err(|err| WorkflowError::Serialization(err.to_string()))?;
                    Ok(ErasedWorkflowTransition::Complete { output })
                }
            }
        })
    }
}

#[expect(
    dead_code,
    reason = "the durable runtime consumes erased transitions in Milestone 2"
)]
pub(crate) enum ErasedWorkflowTransition {
    Continue {
        checkpoint: WorkflowCheckpoint,
    },
    Wait {
        checkpoint: WorkflowCheckpoint,
        wake: crate::api::WakeCondition,
    },
    Complete {
        output: Value,
    },
}
