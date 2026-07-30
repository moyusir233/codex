use std::ffi::OsString;

use clap::CommandFactory;
use clap::Parser;
use schemars::JsonSchema;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::api::ArgumentError;

/// Typed, serializable launch arguments for a workflow definition.
///
/// Implementations normally derive [`clap::Parser`], [`serde::Serialize`],
/// [`serde::Deserialize`], and [`schemars::JsonSchema`], then implement only
/// [`WorkflowArguments::validate`]. The default parser treats `argv` as the
/// arguments after the workflow name.
pub trait WorkflowArguments:
    Parser + CommandFactory + Serialize + DeserializeOwned + JsonSchema + Send + Sync + 'static
{
    /// Parses a workflow-specific argument vector.
    fn parse_cli(argv: &[OsString]) -> Result<Self, ArgumentError> {
        let argv = std::iter::once(OsString::from("workflow")).chain(argv.iter().cloned());
        let parsed =
            Self::try_parse_from(argv).map_err(|err| ArgumentError::Parse(err.to_string()))?;
        parsed.validate()?;
        Ok(parsed)
    }

    /// Performs definition-specific semantic validation after parsing or JSON decoding.
    fn validate(&self) -> Result<(), ArgumentError>;
}
