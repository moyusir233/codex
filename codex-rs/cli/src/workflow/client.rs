use std::io;
use std::sync::Arc;

use codex_app_server_client::AppServerClient;
use codex_app_server_client::DEFAULT_IN_PROCESS_CHANNEL_CAPACITY;
use codex_app_server_client::EnvironmentManager;
use codex_app_server_client::ExecServerRuntimePaths;
use codex_app_server_client::InProcessAppServerClient;
use codex_app_server_client::InProcessClientStartArgs;
use codex_app_server_client::RemoteAppServerClient;
use codex_app_server_client::RemoteAppServerConnectArgs;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ConfigWarningNotification;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::RequestId;
use codex_config::CloudConfigBundleLoader;
use codex_core::config::ConfigBuilder;
use codex_core::config::find_codex_home;
use codex_feedback::CodexFeedback;
use codex_protocol::protocol::SessionSource;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::EXIT_FAILURE;
use super::EXIT_USAGE;
use super::WorkflowContext;
use super::output::WorkflowOutput;

pub(super) async fn start_client(context: &WorkflowContext) -> anyhow::Result<AppServerClient> {
    if let Some(endpoint) = context.remote_endpoint.clone() {
        let client = RemoteAppServerClient::connect(RemoteAppServerConnectArgs {
            endpoint,
            client_name: "codex-workflow".to_string(),
            client_version: env!("CARGO_PKG_VERSION").to_string(),
            experimental_api: true,
            mcp_server_openai_form_elicitation: false,
            opt_out_notification_methods: Vec::new(),
            channel_capacity: DEFAULT_IN_PROCESS_CHANNEL_CAPACITY,
        })
        .await?;
        return Ok(AppServerClient::Remote(client));
    }

    let mut overrides = context.config_overrides.clone();
    overrides
        .raw_overrides
        .push("features.workflows=true".to_string());
    let cli_overrides = overrides.parse_overrides().map_err(anyhow::Error::msg)?;
    let codex_home = find_codex_home()?.to_path_buf();
    let cloud_config_bundle = CloudConfigBundleLoader::default();
    let config = ConfigBuilder::default()
        .codex_home(codex_home)
        .cli_overrides(cli_overrides.clone())
        .loader_overrides(context.loader_overrides.clone())
        .strict_config(context.strict_config)
        .cloud_config_bundle(cloud_config_bundle.clone())
        .build()
        .await?;
    let config_warnings = config
        .startup_warnings
        .iter()
        .map(|warning| ConfigWarningNotification {
            summary: warning.clone(),
            details: None,
            path: None,
            range: None,
        })
        .collect();
    let runtime_paths = ExecServerRuntimePaths::from_optional_paths(
        context.arg0_paths.codex_self_exe.clone(),
        context.arg0_paths.codex_linux_sandbox_exe.clone(),
    )?;
    let environment_manager =
        EnvironmentManager::from_codex_home(config.codex_home.clone(), Some(runtime_paths)).await?;
    let state_db = codex_core::init_state_db(&config).await;
    let client = InProcessAppServerClient::start(InProcessClientStartArgs {
        arg0_paths: context.arg0_paths.clone(),
        config: Arc::new(config),
        cli_overrides,
        loader_overrides: context.loader_overrides.clone(),
        strict_config: context.strict_config,
        cloud_config_bundle,
        feedback: CodexFeedback::new(),
        log_db: None,
        state_db,
        environment_manager: Arc::new(environment_manager),
        config_warnings,
        session_source: SessionSource::Cli,
        enable_codex_api_key_env: true,
        client_name: "codex-workflow".to_string(),
        client_version: env!("CARGO_PKG_VERSION").to_string(),
        experimental_api: true,
        mcp_server_openai_form_elicitation: false,
        opt_out_notification_methods: Vec::new(),
        channel_capacity: DEFAULT_IN_PROCESS_CHANNEL_CAPACITY,
    })
    .await?;
    Ok(AppServerClient::InProcess(client))
}

pub(super) async fn request<T: DeserializeOwned>(
    client: &AppServerClient,
    request: ClientRequest,
) -> Result<T, RequestError> {
    let response = client
        .request(request)
        .await
        .map_err(|error| RequestError::Transport(error.to_string()))?
        .map_err(RequestError::Server)?;
    serde_json::from_value(response).map_err(|error| RequestError::Decode(error.to_string()))
}

pub(super) fn render_request_error(
    output: &mut WorkflowOutput<'_>,
    error: RequestError,
) -> anyhow::Error {
    let (code, message, exit_code) = match &error {
        RequestError::Server(server) => {
            let code = server
                .data
                .as_ref()
                .and_then(|data| data.get("workflowError"))
                .and_then(Value::as_str)
                .unwrap_or("server_error");
            let exit_code = if matches!(
                code,
                "invalid_arguments"
                    | "invalid_workflow_name"
                    | "invalid_workflow_version"
                    | "workflow_not_found"
            ) {
                EXIT_USAGE
            } else {
                EXIT_FAILURE
            };
            (code, server.message.as_str(), exit_code)
        }
        RequestError::Transport(_) => (
            "transport_error",
            "app-server transport failed",
            EXIT_FAILURE,
        ),
        RequestError::Decode(_) => (
            "protocol_error",
            "app-server returned an invalid response",
            EXIT_FAILURE,
        ),
        RequestError::Output(_) => (
            "output_error",
            "failed to write workflow output",
            EXIT_FAILURE,
        ),
    };
    let _ = output.error(code, message);
    anyhow::Error::new(RenderedRequestError {
        exit_code,
        source: error,
    })
}

#[derive(Default)]
pub(super) struct RequestIds {
    next: i64,
}

impl RequestIds {
    pub(super) fn next(&mut self) -> RequestId {
        self.next += 1;
        RequestId::Integer(self.next)
    }
}

#[derive(Debug, thiserror::Error)]
pub(super) enum RequestError {
    #[error("transport error: {0}")]
    Transport(String),
    #[error("server error: {0:?}")]
    Server(JSONRPCErrorError),
    #[error("response decode error: {0}")]
    Decode(String),
    #[error(transparent)]
    Output(#[from] io::Error),
}

#[derive(Debug, thiserror::Error)]
#[error("{source}")]
pub(super) struct RenderedRequestError {
    pub(super) exit_code: u8,
    source: RequestError,
}
