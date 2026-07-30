use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct FornaxBridgeEnvironment {
    pub(super) values: BTreeMap<OsString, OsString>,
    pub(super) redactions: Vec<String>,
}

impl FornaxBridgeEnvironment {
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

#[derive(Clone, Debug)]
pub struct FornaxBridgeConfig {
    pub executable: PathBuf,
    pub cwd: PathBuf,
    pub state_dir: PathBuf,
    pub environment: FornaxBridgeEnvironment,
    pub lifecycle_timeout: Duration,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
}

impl FornaxBridgeConfig {
    pub fn validate(&self) -> Result<(), FornaxBridgeError> {
        if !self.executable.is_absolute() {
            return Err(FornaxBridgeError::InvalidConfiguration(
                "bridge lifecycle executable must be absolute".to_string(),
            ));
        }
        if !self.cwd.is_absolute() {
            return Err(FornaxBridgeError::InvalidConfiguration(
                "bridge working directory must be absolute".to_string(),
            ));
        }
        if !self.state_dir.is_absolute() {
            return Err(FornaxBridgeError::InvalidConfiguration(
                "bridge state directory must be absolute".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FornaxBridgeError {
    #[error("invalid Fornax trace bridge configuration: {0}")]
    InvalidConfiguration(String),
    #[error("Fornax trace bridge lifecycle failed: {0}")]
    Lifecycle(String),
    #[error("Fornax trace bridge returned an incompatible version")]
    VersionMismatch,
    #[error("Fornax trace bridge descriptor is not secure")]
    InsecureDescriptor,
    #[error("Fornax trace bridge returned an invalid response")]
    InvalidResponse,
    #[error("Fornax trace bridge transport failed")]
    Transport,
    #[error("Fornax trace mutation `{operation_id}` is ambiguous")]
    AmbiguousMutation { operation_id: uuid::Uuid },
    #[error("Fornax trace bridge local authentication failed")]
    LocalAuthentication,
    #[error("Fornax trace bridge queue is full")]
    QueueFull,
    #[error("Fornax trace span is unknown")]
    UnknownSpan,
    #[error("Fornax trace context is unknown")]
    UnknownContext,
    #[error("Fornax trace span handle was orphaned")]
    OrphanedSpan,
    #[error("Fornax SDK authentication failed")]
    SdkAuthentication,
    #[error("Fornax SDK rejected the mutation")]
    SdkRejected,
    #[error("Fornax trace bridge is shutting down")]
    ShuttingDown,
    #[error("Fornax trace writes remain disabled until live delivery is approved")]
    LiveDeliveryNotApproved,
    #[error("Fornax trace bridge rejected the request: {code}")]
    Remote {
        code: String,
        operation_id: Option<uuid::Uuid>,
        retry: super::RetryDisposition,
    },
}
