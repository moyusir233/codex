use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::integrations::process::ProcessRequest;
use crate::integrations::process::ProcessRunner;

use super::FornaxCliCapabilities;
use super::FornaxCliError;
use super::FornaxCliVersion;
use super::error::classify_process;

/// Explicit allow-listed environment for `fornax-cli`.
#[derive(Clone, Debug, Default)]
pub struct FornaxCliEnvironment {
    values: BTreeMap<OsString, OsString>,
    redactions: Vec<String>,
}

impl FornaxCliEnvironment {
    /// Adds a non-secret environment value.
    pub fn with_value(mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.values.insert(name.into(), value.into());
        self
    }

    /// Adds a secret environment value and registers it for diagnostic redaction.
    pub fn with_secret(mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        let value = value.into();
        self.redactions.push(value.to_string_lossy().into_owned());
        self.values.insert(name.into(), value);
        self
    }
}

/// Safe process configuration for the pinned CLI.
#[derive(Clone, Debug)]
pub struct FornaxCliConfig {
    /// Absolute `fornax-cli` executable.
    pub executable: PathBuf,
    /// Absolute controlled working directory.
    pub cwd: PathBuf,
    /// Explicit child environment.
    pub environment: FornaxCliEnvironment,
    /// Hard process deadline.
    pub timeout: Duration,
    /// Maximum JSON stdout bytes.
    pub stdout_limit: usize,
    /// Maximum diagnostic stderr bytes.
    pub stderr_limit: usize,
}

/// Typed version-pinned `fornax-cli` client.
#[derive(Clone, Debug)]
pub struct FornaxCli {
    pub(super) config: FornaxCliConfig,
    pub(super) runner: ProcessRunner,
}

impl FornaxCli {
    /// Creates a client. Call [`Self::verify`] before any capability operation.
    pub fn new(config: FornaxCliConfig) -> Self {
        let runner = ProcessRunner::new(config.timeout, config.stdout_limit, config.stderr_limit);
        Self { config, runner }
    }

    /// Verifies the exact supported version and returns its fixed capabilities.
    pub fn verify(
        &self,
        cancelled: &AtomicBool,
    ) -> Result<(FornaxCliVersion, FornaxCliCapabilities), FornaxCliError> {
        let stdout = self.run_raw(["version"], cancelled)?;
        let version = FornaxCliVersion::parse(&stdout)?;
        let capabilities = FornaxCliCapabilities::for_version(&version);
        Ok((version, capabilities))
    }

    pub(super) fn run_json<T: DeserializeOwned>(
        &self,
        args: impl IntoIterator<Item = impl Into<OsString>>,
        cancelled: &AtomicBool,
    ) -> Result<T, FornaxCliError> {
        let mut all_args = vec![OsString::from("--format"), OsString::from("json")];
        all_args.extend(args.into_iter().map(Into::into));
        let stdout = self.run_raw(all_args, cancelled)?;
        decode_json(&stdout)
    }

    pub(super) fn run_raw(
        &self,
        args: impl IntoIterator<Item = impl Into<OsString>>,
        cancelled: &AtomicBool,
    ) -> Result<Vec<u8>, FornaxCliError> {
        let output = self
            .runner
            .run(
                ProcessRequest {
                    executable: self.config.executable.clone(),
                    args: args.into_iter().map(Into::into).collect(),
                    cwd: self.config.cwd.clone(),
                    env: self.config.environment.values.clone(),
                    redactions: self.config.environment.redactions.clone(),
                },
                cancelled,
            )
            .map_err(classify_process)?;
        Ok(output.stdout)
    }
}

fn decode_json<T: DeserializeOwned>(stdout: &[u8]) -> Result<T, FornaxCliError> {
    let value: Value =
        serde_json::from_slice(stdout).map_err(|error| FornaxCliError::InvalidResponse {
            message: bounded_parser_message(&error.to_string()),
        })?;
    let payload = match value {
        Value::Object(mut object) if object.contains_key("data") => {
            object.remove("data").unwrap_or(Value::Null)
        }
        value => value,
    };
    serde_json::from_value(payload).map_err(|error| FornaxCliError::InvalidResponse {
        message: bounded_parser_message(&error.to_string()),
    })
}

fn bounded_parser_message(message: &str) -> String {
    message.chars().take(256).collect()
}
