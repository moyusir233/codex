//! Experimental typed definitions and feasibility work for persisted Codex workflows.
//!
//! Workflows are replayable reducers over versioned checkpoints. Definitions
//! are registered statically; runtime effects are exposed only through typed,
//! journal-backed capabilities.

pub mod api;
#[doc(hidden)]
pub mod feasibility;
pub mod registry;

pub use api::ArgumentError;
pub use api::EffectKey;
pub use api::IdentifierError;
pub use api::InteractionId;
pub use api::NodeAttemptId;
pub use api::NodeId;
pub use api::NodeKey;
pub use api::WakeCondition;
pub use api::Workflow;
pub use api::WorkflowArguments;
pub use api::WorkflowCancellation;
pub use api::WorkflowContext;
pub use api::WorkflowError;
pub use api::WorkflowMetadata;
pub use api::WorkflowName;
pub use api::WorkflowOutput;
pub use api::WorkflowRunId;
pub use api::WorkflowSequence;
pub use api::WorkflowStability;
pub use api::WorkflowState;
pub use api::WorkflowTransition;
pub use api::WorkflowVersion;
pub use registry::RegistryError;
pub use registry::WorkflowCheckpoint;
pub use registry::WorkflowDefinition;
pub use registry::WorkflowDefinitionMetadata;
pub use registry::WorkflowRegistry;
pub use registry::WorkflowRegistryBuilder;
