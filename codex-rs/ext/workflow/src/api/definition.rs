use std::future::Future;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::api::WorkflowArguments;
use crate::api::WorkflowContext;
use crate::api::WorkflowError;
use crate::api::WorkflowName;
use crate::api::WorkflowTransition;
use crate::api::WorkflowVersion;

/// Typed checkpoint state for a durable workflow reducer.
///
/// Changing the serialized representation requires incrementing
/// [`WorkflowState::SCHEMA_VERSION`] and supplying an explicit migration before
/// old runs can resume.
pub trait WorkflowState: Serialize + DeserializeOwned + Send + Sync + 'static {
    /// Version of this definition's serialized checkpoint representation.
    const SCHEMA_VERSION: u32;
}

/// Typed successful result returned by a completed workflow.
pub trait WorkflowOutput:
    Serialize + DeserializeOwned + JsonSchema + Send + Sync + 'static
{
}

/// Stability classification advertised during workflow discovery.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkflowStability {
    /// The definition or its host contract can change before stabilization.
    #[default]
    Experimental,
    /// The definition's author-facing contract has completed stabilization review.
    Stable,
}

/// Static discovery metadata for one workflow definition version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowMetadata {
    name: WorkflowName,
    version: WorkflowVersion,
    description: String,
    is_default: bool,
    stability: WorkflowStability,
    required_capabilities: Vec<String>,
}

impl WorkflowMetadata {
    /// Constructs metadata for an experimental, non-default definition version.
    pub fn new(
        name: WorkflowName,
        version: WorkflowVersion,
        description: impl Into<String>,
    ) -> Self {
        Self {
            name,
            version,
            description: description.into(),
            is_default: false,
            stability: WorkflowStability::Experimental,
            required_capabilities: Vec::new(),
        }
    }

    /// Marks whether this version is selected when a caller omits a version.
    pub fn with_default(mut self, is_default: bool) -> Self {
        self.is_default = is_default;
        self
    }

    /// Sets the discovery stability classification.
    pub fn with_stability(mut self, stability: WorkflowStability) -> Self {
        self.stability = stability;
        self
    }

    /// Declares capability names required before this workflow can run.
    pub fn with_required_capabilities(
        mut self,
        capabilities: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.required_capabilities = capabilities.into_iter().map(Into::into).collect();
        self
    }

    /// Returns the validated workflow name.
    pub fn name(&self) -> &WorkflowName {
        &self.name
    }

    /// Returns the semantic definition version.
    pub fn version(&self) -> &WorkflowVersion {
        &self.version
    }

    /// Returns the human-readable discovery description.
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Returns whether unpinned launches select this version.
    pub fn is_default(&self) -> bool {
        self.is_default
    }

    /// Returns the discovery stability classification.
    pub fn stability(&self) -> WorkflowStability {
        self.stability
    }

    /// Returns the required capability names advertised during discovery.
    pub fn required_capabilities(&self) -> &[String] {
        &self.required_capabilities
    }
}

/// A typed, replayable workflow reducer.
///
/// `step` may be called again after process failure. Mutating work must
/// therefore go through the capabilities exposed by [`WorkflowContext`] with
/// stable effect keys; implementations must not perform unjournaled external
/// effects directly.
pub trait Workflow: Send + Sync + 'static {
    /// Parsed launch argument type.
    type Arguments: WorkflowArguments;
    /// Durable checkpoint state type.
    type State: WorkflowState;
    /// Successful terminal output type.
    type Output: WorkflowOutput;

    /// Returns discovery metadata for this exact definition version.
    fn metadata(&self) -> WorkflowMetadata;

    /// Produces the first durable checkpoint from validated launch arguments.
    fn initialize(&self, args: Self::Arguments) -> Result<Self::State, WorkflowError>;

    /// Advances the reducer by one durable transition.
    fn step<'a>(
        &'a self,
        ctx: WorkflowContext<'a>,
        state: Self::State,
    ) -> impl Future<Output = Result<WorkflowTransition<Self::State, Self::Output>, WorkflowError>>
    + Send
    + 'a;
}
