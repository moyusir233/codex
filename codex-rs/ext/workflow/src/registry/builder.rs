use std::collections::BTreeMap;
use std::ffi::OsString;
use std::sync::Arc;

use clap::CommandFactory;
use schemars::schema_for;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use crate::api::ArgumentError;
use crate::api::Workflow;
use crate::api::WorkflowContext;
use crate::api::WorkflowError;
use crate::api::WorkflowMetadata;
use crate::api::WorkflowName;
use crate::api::WorkflowStability;
use crate::api::WorkflowState;
use crate::api::WorkflowVersion;
use crate::registry::erased::ConcreteWorkflow;
use crate::registry::erased::ErasedWorkflow;
use crate::registry::erased::ErasedWorkflowTransition;

/// Registry construction or lookup failure.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    /// The same name and semantic version were registered twice.
    #[error("workflow {name}@{version} is already registered")]
    DuplicateVersion {
        /// Conflicting workflow name.
        name: WorkflowName,
        /// Conflicting semantic version.
        version: WorkflowVersion,
    },
    /// More than one version of a name was marked as the default.
    #[error("workflow {name} has more than one default version")]
    DuplicateDefault {
        /// Workflow name with conflicting defaults.
        name: WorkflowName,
    },
    /// A workflow name had no explicitly selected default.
    #[error("workflow {name} has no default version")]
    MissingDefault {
        /// Workflow name missing a default version.
        name: WorkflowName,
    },
    /// Discovery text was empty or unreasonably large.
    #[error("workflow {name}@{version} has an invalid description")]
    InvalidDescription {
        /// Workflow name with invalid metadata.
        name: WorkflowName,
        /// Workflow version with invalid metadata.
        version: WorkflowVersion,
    },
    /// A generated argument or output schema was not self-contained.
    #[error("workflow {name}@{version} has an invalid {target} schema: {message}")]
    InvalidSchema {
        /// Workflow name owning the schema.
        name: WorkflowName,
        /// Workflow version owning the schema.
        version: WorkflowVersion,
        /// `arguments` or `output`.
        target: &'static str,
        /// Bounded validation detail.
        message: String,
    },
    /// Durable checkpoint schema versions are one-based.
    #[error("workflow {name}@{version} declares state schema version zero")]
    InvalidStateVersion {
        /// Workflow name owning the state.
        name: WorkflowName,
        /// Workflow version owning the state.
        version: WorkflowVersion,
    },
    /// No definition exists for the requested name.
    #[error("workflow {name} is not registered")]
    UnknownWorkflow {
        /// Requested workflow name.
        name: WorkflowName,
    },
    /// The requested exact definition version is unavailable.
    #[error("workflow {name}@{version} is not registered")]
    UnknownVersion {
        /// Requested workflow name.
        name: WorkflowName,
        /// Requested semantic version.
        version: WorkflowVersion,
    },
}

/// Serializable discovery metadata captured at registration time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowDefinitionMetadata {
    name: WorkflowName,
    version: WorkflowVersion,
    description: String,
    is_default: bool,
    stability: WorkflowStability,
    state_schema_version: u32,
    argv_help: String,
    argument_schema: Value,
    output_schema: Value,
}

impl WorkflowDefinitionMetadata {
    /// Returns the workflow name.
    pub fn name(&self) -> &WorkflowName {
        &self.name
    }

    /// Returns the exact semantic version.
    pub fn version(&self) -> &WorkflowVersion {
        &self.version
    }

    /// Returns the discovery description.
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Returns whether unpinned launches select this version.
    pub fn is_default(&self) -> bool {
        self.is_default
    }

    /// Returns the stability classification.
    pub fn stability(&self) -> WorkflowStability {
        self.stability
    }

    /// Returns the durable state schema version.
    pub fn state_schema_version(&self) -> u32 {
        self.state_schema_version
    }

    /// Returns deterministic long-form Clap help.
    pub fn argv_help(&self) -> &str {
        &self.argv_help
    }

    /// Returns the captured workflow argument JSON schema.
    pub fn argument_schema(&self) -> &Value {
        &self.argument_schema
    }

    /// Returns the captured successful output JSON schema.
    pub fn output_schema(&self) -> &Value {
        &self.output_schema
    }
}

/// Versioned JSON checkpoint stored between reducer steps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowCheckpoint {
    state_schema_version: u32,
    state: Value,
}

impl WorkflowCheckpoint {
    /// Creates a checkpoint from an already serialized state value.
    pub fn new(state_schema_version: u32, state: Value) -> Self {
        Self {
            state_schema_version,
            state,
        }
    }

    /// Returns the stored state schema version.
    pub fn state_schema_version(&self) -> u32 {
        self.state_schema_version
    }

    /// Returns the serialized state.
    pub fn state(&self) -> &Value {
        &self.state
    }

    pub(crate) fn from_state<S: WorkflowState>(state: &S) -> Result<Self, WorkflowError> {
        let state = serde_json::to_value(state)
            .map_err(|err| WorkflowError::Serialization(err.to_string()))?;
        Ok(Self::new(S::SCHEMA_VERSION, state))
    }

    pub(crate) fn into_state<S: WorkflowState>(self) -> Result<S, WorkflowError> {
        if self.state_schema_version != S::SCHEMA_VERSION {
            return Err(WorkflowError::UnsupportedStateVersion {
                expected: S::SCHEMA_VERSION,
                actual: self.state_schema_version,
            });
        }
        serde_json::from_value(self.state)
            .map_err(|err| WorkflowError::Serialization(err.to_string()))
    }
}

/// One registered, type-erased workflow definition.
#[derive(Clone)]
pub struct WorkflowDefinition {
    metadata: WorkflowDefinitionMetadata,
    workflow: Arc<dyn ErasedWorkflow>,
}

impl WorkflowDefinition {
    /// Returns immutable discovery metadata captured during registration.
    pub fn metadata(&self) -> &WorkflowDefinitionMetadata {
        &self.metadata
    }

    /// Parses and validates workflow-specific CLI arguments into JSON.
    pub fn parse_cli(&self, argv: &[OsString]) -> Result<Value, ArgumentError> {
        self.workflow.parse_cli(argv)
    }

    /// Restores JSON arguments and produces the initial durable checkpoint.
    pub fn initialize(&self, arguments: Value) -> Result<WorkflowCheckpoint, WorkflowError> {
        self.workflow.initialize(arguments)
    }

    /// Verifies that a stored checkpoint can be resumed by this exact definition.
    pub fn validate_checkpoint(
        &self,
        checkpoint: &WorkflowCheckpoint,
    ) -> Result<(), WorkflowError> {
        if checkpoint.state_schema_version != self.metadata.state_schema_version {
            return Err(WorkflowError::UnsupportedStateVersion {
                expected: self.metadata.state_schema_version,
                actual: checkpoint.state_schema_version,
            });
        }
        Ok(())
    }

    #[expect(
        dead_code,
        reason = "the durable runtime dispatches reducer steps in Milestone 2"
    )]
    pub(crate) async fn step(
        &self,
        context: WorkflowContext<'_>,
        checkpoint: WorkflowCheckpoint,
    ) -> Result<ErasedWorkflowTransition, WorkflowError> {
        self.validate_checkpoint(&checkpoint)?;
        self.workflow.step(context, checkpoint).await
    }
}

/// Mutable builder for a deterministic, versioned workflow registry.
#[derive(Default)]
pub struct WorkflowRegistryBuilder {
    definitions: BTreeMap<(WorkflowName, WorkflowVersion), WorkflowDefinition>,
}

impl WorkflowRegistryBuilder {
    /// Creates an empty registry builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Captures and registers one concrete typed workflow definition.
    pub fn register<W: Workflow>(&mut self, workflow: W) -> Result<&mut Self, RegistryError> {
        let metadata = workflow.metadata();
        let key = (metadata.name().clone(), metadata.version().clone());
        if self.definitions.contains_key(&key) {
            return Err(RegistryError::DuplicateVersion {
                name: key.0,
                version: key.1,
            });
        }
        if metadata.is_default()
            && self.definitions.values().any(|definition| {
                definition.metadata.name == *metadata.name() && definition.metadata.is_default
            })
        {
            return Err(RegistryError::DuplicateDefault {
                name: metadata.name().clone(),
            });
        }

        let captured = capture_metadata::<W>(&metadata)?;
        self.definitions.insert(
            key,
            WorkflowDefinition {
                metadata: captured,
                workflow: Arc::new(ConcreteWorkflow::new(workflow)),
            },
        );
        Ok(self)
    }

    /// Validates default selection and freezes deterministic registry ordering.
    pub fn build(self) -> Result<WorkflowRegistry, RegistryError> {
        let mut defaults = BTreeMap::new();
        for definition in self.definitions.values() {
            if definition.metadata.is_default {
                defaults.insert(
                    definition.metadata.name.clone(),
                    definition.metadata.version.clone(),
                );
            }
        }
        for (name, _) in self.definitions.keys() {
            if !defaults.contains_key(name) {
                return Err(RegistryError::MissingDefault { name: name.clone() });
            }
        }
        Ok(WorkflowRegistry {
            definitions: self.definitions,
            defaults,
        })
    }
}

/// Immutable deterministic registry of reviewed workflow definitions.
pub struct WorkflowRegistry {
    definitions: BTreeMap<(WorkflowName, WorkflowVersion), WorkflowDefinition>,
    defaults: BTreeMap<WorkflowName, WorkflowVersion>,
}

impl WorkflowRegistry {
    /// Iterates definitions in workflow-name then semantic-version order.
    pub fn definitions(&self) -> impl ExactSizeIterator<Item = &WorkflowDefinition> {
        self.definitions.values()
    }

    /// Resolves an exact version, or the explicitly registered default.
    pub fn resolve(
        &self,
        name: &WorkflowName,
        version: Option<&WorkflowVersion>,
    ) -> Result<&WorkflowDefinition, RegistryError> {
        let resolved_version = match version {
            Some(version) => version,
            None => self
                .defaults
                .get(name)
                .ok_or_else(|| RegistryError::UnknownWorkflow { name: name.clone() })?,
        };
        self.definitions
            .get(&(name.clone(), resolved_version.clone()))
            .ok_or_else(|| {
                if self.defaults.contains_key(name) {
                    RegistryError::UnknownVersion {
                        name: name.clone(),
                        version: resolved_version.clone(),
                    }
                } else {
                    RegistryError::UnknownWorkflow { name: name.clone() }
                }
            })
    }
}

fn capture_metadata<W: Workflow>(
    metadata: &WorkflowMetadata,
) -> Result<WorkflowDefinitionMetadata, RegistryError> {
    if metadata.description().trim().is_empty() || metadata.description().len() > 1_024 {
        return Err(RegistryError::InvalidDescription {
            name: metadata.name().clone(),
            version: metadata.version().clone(),
        });
    }
    if W::State::SCHEMA_VERSION == 0 {
        return Err(RegistryError::InvalidStateVersion {
            name: metadata.name().clone(),
            version: metadata.version().clone(),
        });
    }

    let argument_schema = serde_json::to_value(schema_for!(W::Arguments)).map_err(|err| {
        RegistryError::InvalidSchema {
            name: metadata.name().clone(),
            version: metadata.version().clone(),
            target: "arguments",
            message: err.to_string(),
        }
    })?;
    validate_schema(&argument_schema).map_err(|message| RegistryError::InvalidSchema {
        name: metadata.name().clone(),
        version: metadata.version().clone(),
        target: "arguments",
        message,
    })?;
    let output_schema = serde_json::to_value(schema_for!(W::Output)).map_err(|err| {
        RegistryError::InvalidSchema {
            name: metadata.name().clone(),
            version: metadata.version().clone(),
            target: "output",
            message: err.to_string(),
        }
    })?;
    validate_schema(&output_schema).map_err(|message| RegistryError::InvalidSchema {
        name: metadata.name().clone(),
        version: metadata.version().clone(),
        target: "output",
        message,
    })?;

    let mut command = <W::Arguments as CommandFactory>::command();
    let argv_help = command.render_long_help().to_string();
    Ok(WorkflowDefinitionMetadata {
        name: metadata.name().clone(),
        version: metadata.version().clone(),
        description: metadata.description().to_string(),
        is_default: metadata.is_default(),
        stability: metadata.stability(),
        state_schema_version: W::State::SCHEMA_VERSION,
        argv_help,
        argument_schema,
        output_schema,
    })
}

fn validate_schema(schema: &Value) -> Result<(), String> {
    let Some(root) = schema.as_object() else {
        return Err("root schema is not an object".to_string());
    };
    let definitions = root.get("definitions").and_then(Value::as_object);
    validate_schema_refs(schema, definitions)
}

fn validate_schema_refs(
    value: &Value,
    definitions: Option<&serde_json::Map<String, Value>>,
) -> Result<(), String> {
    match value {
        Value::Object(object) => {
            if let Some(reference) = object.get("$ref").and_then(Value::as_str)
                && let Some(name) = reference.strip_prefix("#/definitions/")
                && !definitions.is_some_and(|definitions| definitions.contains_key(name))
            {
                return Err(format!("unresolved local reference {reference}"));
            }
            for child in object.values() {
                validate_schema_refs(child, definitions)?;
            }
        }
        Value::Array(array) => {
            for child in array {
                validate_schema_refs(child, definitions)?;
            }
        }
        _ => {}
    }
    Ok(())
}
