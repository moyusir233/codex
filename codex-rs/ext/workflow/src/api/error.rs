/// Failure while parsing or validating workflow arguments.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ArgumentError {
    /// Clap rejected the supplied argument vector.
    #[error("{0}")]
    Parse(String),
    /// Definition-specific validation rejected the parsed value.
    #[error("{0}")]
    Validation(String),
    /// The typed value could not be converted to or from JSON.
    #[error("workflow argument serialization failed: {0}")]
    Serialization(String),
}

impl ArgumentError {
    /// Creates a definition-specific validation error.
    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation(message.into())
    }
}

/// Durable workflow reducer failure.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum WorkflowError {
    /// A checkpoint does not match the registered state schema version.
    #[error("unsupported workflow state schema version {actual}; expected {expected}")]
    UnsupportedStateVersion {
        /// Version required by the registered definition.
        expected: u32,
        /// Version stored in the checkpoint.
        actual: u32,
    },
    /// A typed argument, state, or output could not be converted to or from JSON.
    #[error("workflow serialization failed: {0}")]
    Serialization(String),
    /// Definition-specific reducer logic rejected the operation.
    #[error("{0}")]
    Definition(String),
}

impl WorkflowError {
    /// Creates a definition-specific workflow error.
    pub fn definition(message: impl Into<String>) -> Self {
        Self::Definition(message.into())
    }
}
