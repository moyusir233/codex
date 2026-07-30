use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::integrations::process::ProcessRequest;
use crate::integrations::process::ProcessRunner;

use super::LarkCliCapabilities;
use super::LarkCliError;
use super::LarkCliVersion;
use super::error::classify_process;

/// Explicit allow-listed environment for `lark-cli`.
#[derive(Clone, Debug, Default)]
pub struct LarkCliEnvironment {
    pub(super) values: BTreeMap<OsString, OsString>,
    pub(super) redactions: Vec<String>,
}

impl LarkCliEnvironment {
    pub fn with_value(mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.values.insert(name.into(), value.into());
        self
    }

    pub fn with_secret(mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        let value = value.into();
        self.redactions.push(value.to_string_lossy().into_owned());
        self.values.insert(name.into(), value);
        self
    }
}

/// Safe process configuration for the pinned Lark CLI.
#[derive(Clone, Debug)]
pub struct LarkCliConfig {
    pub executable: PathBuf,
    pub cwd: PathBuf,
    pub environment: LarkCliEnvironment,
    pub timeout: Duration,
    pub stdout_limit: usize,
    pub stderr_limit: usize,
}

/// Typed version-pinned `lark-cli` client.
#[derive(Clone, Debug)]
pub struct LarkCli {
    pub(super) config: LarkCliConfig,
    runner: ProcessRunner,
}

impl LarkCli {
    pub fn new(config: LarkCliConfig) -> Self {
        let runner = ProcessRunner::new(config.timeout, config.stdout_limit, config.stderr_limit);
        Self { config, runner }
    }

    pub fn verify(
        &self,
        cancelled: &AtomicBool,
    ) -> Result<(LarkCliVersion, LarkCliCapabilities), LarkCliError> {
        let stdout = self.run_raw(["--version"], &[], cancelled)?;
        let version = LarkCliVersion::parse(&stdout)?;
        Ok((version, LarkCliCapabilities::verified()))
    }

    pub(super) fn run_json<T: DeserializeOwned>(
        &self,
        args: impl IntoIterator<Item = impl Into<OsString>>,
        content_redactions: &[String],
        cancelled: &AtomicBool,
    ) -> Result<T, LarkCliError> {
        let stdout = self.run_raw(args, content_redactions, cancelled)?;
        let value: Value =
            serde_json::from_slice(&stdout).map_err(|error| LarkCliError::InvalidResponse {
                message: bounded(&error.to_string()),
            })?;
        let payload = match value {
            Value::Object(mut object) if object.contains_key("data") => {
                object.remove("data").unwrap_or(Value::Null)
            }
            value => value,
        };
        serde_json::from_value(payload).map_err(|error| LarkCliError::InvalidResponse {
            message: bounded(&error.to_string()),
        })
    }

    pub(super) fn run_raw(
        &self,
        args: impl IntoIterator<Item = impl Into<OsString>>,
        content_redactions: &[String],
        cancelled: &AtomicBool,
    ) -> Result<Vec<u8>, LarkCliError> {
        let mut redactions = self.config.environment.redactions.clone();
        redactions.extend_from_slice(content_redactions);
        let output = self
            .runner
            .run(
                ProcessRequest {
                    executable: self.config.executable.clone(),
                    args: args.into_iter().map(Into::into).collect(),
                    cwd: self.config.cwd.clone(),
                    env: self.config.environment.values.clone(),
                    redactions,
                },
                cancelled,
            )
            .map_err(classify_process)?;
        Ok(output.stdout)
    }
}

fn bounded(message: &str) -> String {
    message.chars().take(256).collect()
}
