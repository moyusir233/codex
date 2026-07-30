use std::path::Path;
use std::time::Duration;

use reqwest::Method;
use reqwest::StatusCode;
use reqwest::redirect::Policy;
use serde::Serialize;
use serde::de::DeserializeOwned;
use url::Url;
use uuid::Uuid;

use super::BridgeErrorEnvelope;
use super::BridgeHealth;
use super::FORNAX_BRIDGE_PROTOCOL;
use super::FinishSpanRequest;
use super::FinishedSpan;
use super::FornaxBridgeDiscovery;
use super::FornaxBridgeError;
use super::OperationStatus;
use super::RecordSpanRequest;
use super::RecordedSpan;
use super::RetryDisposition;
use super::SpanStatus;
use super::StartSpanRequest;
use super::StartedSpan;

const MAX_RESPONSE_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug)]
pub struct FornaxBridgeHttpClient {
    client: reqwest::Client,
    endpoint: Url,
    bearer: String,
}

impl FornaxBridgeHttpClient {
    pub fn new(
        discovery: &FornaxBridgeDiscovery,
        state_dir: &Path,
        connect_timeout: Duration,
        request_timeout: Duration,
    ) -> Result<Self, FornaxBridgeError> {
        let endpoint = validate_endpoint(&discovery.endpoint)?;
        let bearer = read_credential(&discovery.credential_file, state_dir)?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(Policy::none())
            .connect_timeout(connect_timeout)
            .timeout(request_timeout)
            .build()
            .map_err(|_| FornaxBridgeError::Transport)?;
        Ok(Self {
            client,
            endpoint,
            bearer,
        })
    }

    pub async fn health(&self) -> Result<BridgeHealth, FornaxBridgeError> {
        self.request(Method::GET, "v1/health", Option::<&()>::None, None)
            .await
    }

    pub async fn start_span(
        &self,
        request: &StartSpanRequest,
    ) -> Result<StartedSpan, FornaxBridgeError> {
        self.request(
            Method::POST,
            "v1/spans",
            Some(request),
            Some(request.operation_id),
        )
        .await
    }

    pub async fn record_span(
        &self,
        span_handle_id: Uuid,
        request: &RecordSpanRequest,
    ) -> Result<RecordedSpan, FornaxBridgeError> {
        self.request(
            Method::POST,
            &format!("v1/spans/{span_handle_id}/records"),
            Some(request),
            Some(request.operation_id),
        )
        .await
    }

    pub async fn finish_span(
        &self,
        span_handle_id: Uuid,
        request: &FinishSpanRequest,
    ) -> Result<FinishedSpan, FornaxBridgeError> {
        self.request(
            Method::POST,
            &format!("v1/spans/{span_handle_id}/finish"),
            Some(request),
            Some(request.operation_id),
        )
        .await
    }

    pub async fn operation_status(
        &self,
        operation_id: Uuid,
    ) -> Result<OperationStatus, FornaxBridgeError> {
        self.request(
            Method::GET,
            &format!("v1/operations/{operation_id}"),
            Option::<&()>::None,
            None,
        )
        .await
    }

    pub async fn span_status(&self, span_handle_id: Uuid) -> Result<SpanStatus, FornaxBridgeError> {
        self.request(
            Method::GET,
            &format!("v1/spans/{span_handle_id}"),
            Option::<&()>::None,
            None,
        )
        .await
    }

    async fn request<T: DeserializeOwned, B: Serialize + ?Sized>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
        operation_id: Option<Uuid>,
    ) -> Result<T, FornaxBridgeError> {
        let url = self
            .endpoint
            .join(path)
            .map_err(|_| FornaxBridgeError::InvalidResponse)?;
        let mut request = self
            .client
            .request(method, url)
            .bearer_auth(&self.bearer)
            .header("X-Codex-Fornax-Protocol", FORNAX_BRIDGE_PROTOCOL);
        if let Some(operation_id) = operation_id {
            request = request.header("Idempotency-Key", operation_id.to_string());
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request
            .send()
            .await
            .map_err(|_| transport_error(operation_id))?;
        if response.content_length().is_some_and(|size| {
            usize::try_from(size).map_or(true, |size| size > MAX_RESPONSE_BYTES)
        }) {
            return Err(FornaxBridgeError::InvalidResponse);
        }
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|_| transport_error(operation_id))?;
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err(FornaxBridgeError::InvalidResponse);
        }
        if status.is_success() {
            return serde_json::from_slice(&bytes).map_err(|_| FornaxBridgeError::InvalidResponse);
        }
        classify_remote(status, &bytes, operation_id)
    }
}

fn validate_endpoint(value: &str) -> Result<Url, FornaxBridgeError> {
    let mut endpoint = Url::parse(value).map_err(|_| FornaxBridgeError::InsecureDescriptor)?;
    if endpoint.scheme() != "http"
        || endpoint.username() != ""
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
        || endpoint.path() != "/"
        || endpoint.port().is_none()
        || endpoint.host_str() != Some("127.0.0.1")
    {
        return Err(FornaxBridgeError::InsecureDescriptor);
    }
    endpoint.set_path("/");
    Ok(endpoint)
}

#[cfg(unix)]
fn read_credential(path: &Path, state_dir: &Path) -> Result<String, FornaxBridgeError> {
    use std::os::unix::fs::MetadataExt;

    let resolved_state = state_dir
        .canonicalize()
        .map_err(|_| FornaxBridgeError::InsecureDescriptor)?;
    let resolved_path = path
        .canonicalize()
        .map_err(|_| FornaxBridgeError::InsecureDescriptor)?;
    if resolved_path.parent() != Some(resolved_state.as_path())
        || resolved_path.file_name().and_then(|name| name.to_str()) != Some("credential.json")
    {
        return Err(FornaxBridgeError::InsecureDescriptor);
    }
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| FornaxBridgeError::InsecureDescriptor)?;
    let state_metadata =
        std::fs::metadata(&resolved_state).map_err(|_| FornaxBridgeError::InsecureDescriptor)?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.mode() & 0o077 != 0
        || metadata.uid() != state_metadata.uid()
    {
        return Err(FornaxBridgeError::InsecureDescriptor);
    }
    #[derive(serde::Deserialize)]
    struct Credential {
        token: String,
    }
    let bytes = std::fs::read(path).map_err(|_| FornaxBridgeError::InsecureDescriptor)?;
    let credential: Credential =
        serde_json::from_slice(&bytes).map_err(|_| FornaxBridgeError::InsecureDescriptor)?;
    if credential.token.len() < 43 {
        return Err(FornaxBridgeError::InsecureDescriptor);
    }
    Ok(credential.token)
}

#[cfg(not(unix))]
fn read_credential(_path: &Path, _state_dir: &Path) -> Result<String, FornaxBridgeError> {
    Err(FornaxBridgeError::InsecureDescriptor)
}

fn classify_remote<T>(
    status: StatusCode,
    bytes: &[u8],
    expected_operation_id: Option<Uuid>,
) -> Result<T, FornaxBridgeError> {
    let envelope: BridgeErrorEnvelope =
        serde_json::from_slice(bytes).map_err(|_| FornaxBridgeError::InvalidResponse)?;
    let operation_id = envelope.error.operation_id.or(expected_operation_id);
    if envelope.error.code == "ambiguousMutation" {
        return Err(FornaxBridgeError::AmbiguousMutation {
            operation_id: operation_id.ok_or(FornaxBridgeError::InvalidResponse)?,
        });
    }
    if envelope.error.retry == RetryDisposition::AfterStatusCheck {
        return Err(FornaxBridgeError::AmbiguousMutation {
            operation_id: operation_id.ok_or(FornaxBridgeError::InvalidResponse)?,
        });
    }
    match envelope.error.code.as_str() {
        "localAuthentication" => return Err(FornaxBridgeError::LocalAuthentication),
        "unsupportedVersion" => return Err(FornaxBridgeError::VersionMismatch),
        "queueFull" => return Err(FornaxBridgeError::QueueFull),
        "unknownSpan" => return Err(FornaxBridgeError::UnknownSpan),
        "unknownContext" => return Err(FornaxBridgeError::UnknownContext),
        "orphanedSpan" => return Err(FornaxBridgeError::OrphanedSpan),
        "sdkAuthentication" => return Err(FornaxBridgeError::SdkAuthentication),
        "sdkRejected" => return Err(FornaxBridgeError::SdkRejected),
        "shuttingDown" => return Err(FornaxBridgeError::ShuttingDown),
        _ => {}
    }
    let retry = if status == StatusCode::REQUEST_TIMEOUT {
        RetryDisposition::AfterStatusCheck
    } else {
        envelope.error.retry
    };
    Err(FornaxBridgeError::Remote {
        code: envelope.error.code,
        operation_id,
        retry,
    })
}

fn transport_error(operation_id: Option<Uuid>) -> FornaxBridgeError {
    operation_id.map_or(FornaxBridgeError::Transport, |operation_id| {
        FornaxBridgeError::AmbiguousMutation { operation_id }
    })
}
