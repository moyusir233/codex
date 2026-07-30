use std::ffi::OsString;
use std::sync::atomic::AtomicBool;

use serde::Deserialize;

use crate::integrations::process::ProcessRequest;
use crate::integrations::process::ProcessRunner;

use super::FORNAX_BRIDGE_PROTOCOL;
use super::FORNAX_BRIDGE_VERSION;
use super::FORNAX_SDK_VERSION;
use super::FornaxBridgeConfig;
use super::FornaxBridgeError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FornaxBridgeDiscovery {
    pub status: DiscoveryStatus,
    pub protocol_version: u32,
    pub bridge_version: String,
    pub sdk_version: String,
    pub instance_id: String,
    pub pid: u32,
    pub endpoint: String,
    pub credential_file: std::path::PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscoveryStatus {
    Started,
    AlreadyRunning,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnsureResponse {
    status: EnsureStatus,
    protocol_version: u32,
    bridge_version: String,
    sdk_version: String,
    instance_id: String,
    pid: u32,
    endpoint: String,
    credential_file: std::path::PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
enum EnsureStatus {
    Started,
    AlreadyRunning,
}

pub fn ensure_bridge(
    config: &FornaxBridgeConfig,
    cancelled: &AtomicBool,
) -> Result<FornaxBridgeDiscovery, FornaxBridgeError> {
    config.validate()?;
    #[cfg(not(unix))]
    {
        let _ = cancelled;
        return Err(FornaxBridgeError::InvalidConfiguration(
            "Fornax trace bridge lifecycle is Unix-only".to_string(),
        ));
    }
    #[cfg(unix)]
    {
        let runner = ProcessRunner::new(config.lifecycle_timeout, 64 * 1024, 16 * 1024);
        let output = runner
            .run(
                ProcessRequest {
                    executable: config.executable.clone(),
                    args: vec![
                        OsString::from("daemon"),
                        OsString::from("ensure"),
                        OsString::from("--state-dir"),
                        config.state_dir.clone().into_os_string(),
                        OsString::from("--format"),
                        OsString::from("json"),
                    ],
                    cwd: config.cwd.clone(),
                    env: config.environment.values.clone(),
                    redactions: config.environment.redactions.clone(),
                },
                cancelled,
            )
            .map_err(|error| FornaxBridgeError::Lifecycle(error.to_string()))?;
        let response: EnsureResponse = serde_json::from_slice(&output.stdout)
            .map_err(|_| FornaxBridgeError::InvalidResponse)?;
        if response.protocol_version != FORNAX_BRIDGE_PROTOCOL
            || response.bridge_version != FORNAX_BRIDGE_VERSION
            || response.sdk_version != FORNAX_SDK_VERSION
        {
            return Err(FornaxBridgeError::VersionMismatch);
        }
        if !response.credential_file.is_absolute() {
            return Err(FornaxBridgeError::InsecureDescriptor);
        }
        Ok(FornaxBridgeDiscovery {
            status: match response.status {
                EnsureStatus::Started => DiscoveryStatus::Started,
                EnsureStatus::AlreadyRunning => DiscoveryStatus::AlreadyRunning,
            },
            protocol_version: response.protocol_version,
            bridge_version: response.bridge_version,
            sdk_version: response.sdk_version,
            instance_id: response.instance_id,
            pid: response.pid,
            endpoint: response.endpoint,
            credential_file: response.credential_file,
        })
    }
}
