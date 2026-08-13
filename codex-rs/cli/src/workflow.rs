mod client;
mod output;

use std::io;
use std::num::NonZeroU32;

use clap::Args;
use codex_app_server_client::AppServerClient;
use codex_app_server_client::AppServerEvent;
use codex_app_server_client::RemoteAppServerEndpoint;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::NodeThreadSubscription;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::WorkflowArgumentsInput;
use codex_app_server_protocol::WorkflowListParams;
use codex_app_server_protocol::WorkflowListResponse;
use codex_app_server_protocol::WorkflowRun;
use codex_app_server_protocol::WorkflowRunCancelParams;
use codex_app_server_protocol::WorkflowRunCancelResponse;
use codex_app_server_protocol::WorkflowRunParams;
use codex_app_server_protocol::WorkflowRunReadParams;
use codex_app_server_protocol::WorkflowRunReadResponse;
use codex_app_server_protocol::WorkflowRunResponse;
use codex_app_server_protocol::WorkflowRunResumeParams;
use codex_app_server_protocol::WorkflowRunResumeResponse;
use codex_app_server_protocol::WorkflowRunStatus;
use codex_app_server_protocol::WorkflowRunSubscribeParams;
use codex_app_server_protocol::WorkflowRunSubscribeResponse;
use codex_arg0::Arg0DispatchPaths;
use codex_config::LoaderOverrides;
use codex_utils_cli::CliConfigOverrides;
use tokio::sync::mpsc;

use self::client::RenderedRequestError;
use self::client::RequestError;
use self::client::RequestIds;
use self::client::render_request_error;
use self::client::request;
use self::client::start_client;
use self::output::WorkflowOutput;

const EXIT_SUCCESS: u8 = 0;
const EXIT_FAILURE: u8 = 1;
const EXIT_USAGE: u8 = 2;
const EXIT_INTERRUPT: u8 = 130;

/// Run a registered durable workflow.
#[derive(Debug, Args)]
#[command(
    trailing_var_arg = true,
    override_usage = "codex workflow [runner-options...] <workflow-name> [workflow-args...]",
    long_about = "Run a registered durable workflow through the app server.\n\n\
Use --list to discover exact versions, schemas, help, and required capabilities. \
The built-in prompt-review workflow remains unavailable until its exact Fornax \
and Lark versions, auth/scopes, live trace approval, node/skill host, and artifact \
store are configured. Launch never installs tools, starts login, or grants live \
writes.\n\n\
Example:\n  codex workflow prompt-review --prompt-key KEY --reviewers 3 \
--lark-users ou_a,ou_b --lark-chat-id oc_review\n\n\
Use --json for NDJSON automation. Use --detach only with a persistent remote app \
server. Resume a waiting/operator run with --resume-run RUN_ID; resume returned \
node sessions with `codex resume THREAD_ID`."
)]
pub(crate) struct WorkflowCli {
    /// List registered workflow definitions.
    #[arg(long, conflicts_with_all = ["resume_run", "workflow"])]
    list: bool,

    /// Resume an operator-waiting workflow run.
    #[arg(long, value_name = "run-id", conflicts_with_all = ["list", "workflow"])]
    resume_run: Option<String>,

    /// Emit stable newline-delimited JSON on stdout.
    #[arg(long)]
    json: bool,

    /// Select an exact registered semantic version.
    #[arg(long, value_name = "semver")]
    version: Option<String>,

    /// Reject interactive approval/input requests instead of auto-approving.
    #[arg(long)]
    non_interactive: bool,

    /// Return after a remote app-server accepts the run.
    #[arg(long)]
    detach: bool,

    /// Use an already-running app-server endpoint.
    #[arg(long, value_name = "endpoint")]
    app_server: Option<String>,

    /// Bound workflow node concurrency.
    #[arg(long, value_name = "n")]
    concurrency: Option<NonZeroU32>,

    /// Registered workflow name followed by arguments forwarded unchanged.
    #[arg(
        value_name = "workflow-name",
        required_unless_present_any = ["list", "resume_run"],
        conflicts_with_all = ["list", "resume_run"],
        allow_hyphen_values = true,
        trailing_var_arg = true
    )]
    workflow: Vec<String>,
}

pub(crate) struct WorkflowContext {
    pub(crate) arg0_paths: Arg0DispatchPaths,
    pub(crate) config_overrides: CliConfigOverrides,
    pub(crate) loader_overrides: LoaderOverrides,
    pub(crate) strict_config: bool,
    pub(crate) remote_endpoint: Option<RemoteAppServerEndpoint>,
}

impl WorkflowCli {
    pub(crate) fn app_server_endpoint(&self) -> Option<&str> {
        self.app_server.as_deref()
    }

    fn workflow_name(&self) -> Option<&str> {
        self.workflow.first().map(String::as_str)
    }

    fn workflow_args(&self) -> &[String] {
        let args = self.workflow.get(1..).unwrap_or_default();
        if args.first().is_some_and(|arg| arg == "--") {
            &args[1..]
        } else {
            args
        }
    }
}

pub(crate) async fn run(cli: WorkflowCli, context: WorkflowContext) -> anyhow::Result<u8> {
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    let mut output = WorkflowOutput::new(cli.json, &mut stdout, &mut stderr);
    if let Some(message) = validate_runner_options(&cli, context.remote_endpoint.is_some()) {
        output.error("invalid_arguments", message)?;
        return Ok(EXIT_USAGE);
    }

    let mut client = match start_client(&context).await {
        Ok(client) => client,
        Err(error) => {
            output.error("client_start_failed", &error.to_string())?;
            return Ok(EXIT_FAILURE);
        }
    };
    let (signal_tx, signal_rx) = mpsc::channel(2);
    tokio::spawn(forward_interrupts(signal_tx));
    let result = run_connected(&cli, &mut client, &mut output, signal_rx).await;
    if let Err(error) = client.shutdown().await {
        output.error("client_shutdown_failed", &error.to_string())?;
        return Ok(EXIT_FAILURE);
    }
    match result {
        Ok(exit_code) => Ok(exit_code),
        Err(error) => Ok(error
            .downcast_ref::<RenderedRequestError>()
            .map_or(EXIT_FAILURE, |error| error.exit_code)),
    }
}

fn validate_runner_options(cli: &WorkflowCli, remote: bool) -> Option<&'static str> {
    if cli.detach && !remote {
        return Some(
            "`--detach` requires a persistent remote app-server; an in-process host stops when this command exits",
        );
    }
    if cli.list
        && (cli.detach
            || cli.non_interactive
            || cli.version.is_some()
            || cli.concurrency.is_some()
            || !cli.workflow.is_empty())
    {
        return Some("run-only options and workflow arguments cannot be used with `--list`");
    }
    if cli.resume_run.is_some()
        && (cli.version.is_some() || cli.concurrency.is_some() || !cli.workflow.is_empty())
    {
        return Some(
            "`--version`, `--concurrency`, and workflow arguments cannot be used with `--resume-run`",
        );
    }
    None
}

async fn run_connected(
    cli: &WorkflowCli,
    client: &mut AppServerClient,
    output: &mut WorkflowOutput<'_>,
    mut signals: mpsc::Receiver<()>,
) -> anyhow::Result<u8> {
    let mut request_ids = RequestIds::default();
    if cli.list {
        let response: WorkflowListResponse = request(
            client,
            ClientRequest::WorkflowList {
                request_id: request_ids.next(),
                params: WorkflowListParams {},
            },
        )
        .await
        .map_err(|error| render_request_error(output, error))?;
        output.definitions(&response.data)?;
        return Ok(EXIT_SUCCESS);
    }
    if matches!(cli.workflow_args(), [argument] if argument == "--help" || argument == "-h") {
        let response: WorkflowListResponse = request(
            client,
            ClientRequest::WorkflowList {
                request_id: request_ids.next(),
                params: WorkflowListParams {},
            },
        )
        .await
        .map_err(|error| render_request_error(output, error))?;
        let definition = response.data.iter().find(|definition| {
            definition.name == cli.workflow_name().unwrap_or_default()
                && cli
                    .version
                    .as_ref()
                    .map_or(definition.is_default, |version| {
                        &definition.version == version
                    })
        });
        let Some(definition) = definition else {
            output.error(
                "workflow_not_found",
                "workflow name/version was not found in the registry",
            )?;
            return Ok(EXIT_USAGE);
        };
        output.workflow_help(definition)?;
        return Ok(EXIT_SUCCESS);
    }

    let mut run = if let Some(run_id) = cli.resume_run.as_ref() {
        let response: WorkflowRunResumeResponse = request(
            client,
            ClientRequest::WorkflowRunResume {
                request_id: request_ids.next(),
                params: WorkflowRunResumeParams {
                    run_id: run_id.clone(),
                    detached: cli.detach,
                },
            },
        )
        .await
        .map_err(|error| render_request_error(output, error))?;
        response.run
    } else {
        let response: WorkflowRunResponse = request(
            client,
            ClientRequest::WorkflowRun {
                request_id: request_ids.next(),
                params: WorkflowRunParams {
                    workflow_name: cli.workflow_name().unwrap_or_default().to_string(),
                    workflow_version: cli.version.clone(),
                    arguments: WorkflowArgumentsInput::Argv {
                        argv: cli.workflow_args().to_vec(),
                    },
                    non_interactive: cli.non_interactive,
                    detached: cli.detach,
                    concurrency: cli.concurrency.map(NonZeroU32::get),
                    subscribe: !cli.detach,
                    node_threads: NodeThreadSubscription::Include,
                },
            },
        )
        .await
        .map_err(|error| render_request_error(output, error))?;
        response.run
    };

    if cli.detach {
        output.result(&run, true)?;
        return Ok(EXIT_SUCCESS);
    }

    let mut last_sequence = run.next_sequence.saturating_sub(1);
    if cli.resume_run.is_some() {
        run = subscribe(
            client,
            &mut request_ids,
            &run.run_id,
            0,
            output,
            &mut last_sequence,
        )
        .await
        .map_err(|error| render_request_error(output, error))?;
    }
    if terminal(run.status) {
        output.result(&run, false)?;
        return Ok(exit_for_status(run.status, false));
    }
    output.progress(&format!("Workflow run {} accepted.", run.run_id))?;

    let mut interrupted = false;
    loop {
        tokio::select! {
            signal = signals.recv() => {
                if signal.is_none() {
                    continue;
                }
                if next_interrupt(&mut interrupted) == InterruptAction::Exit {
                    output.error("interrupted", "received a second interrupt")?;
                    return Ok(EXIT_INTERRUPT);
                }
                output.progress("Interrupt received; requesting workflow cancellation.")?;
                let _: WorkflowRunCancelResponse = request(
                    client,
                    ClientRequest::WorkflowRunCancel {
                        request_id: request_ids.next(),
                        params: WorkflowRunCancelParams {
                            run_id: run.run_id.clone(),
                        },
                    },
                )
                .await
                .map_err(|error| render_request_error(output, error))?;
            }
            event = client.next_event() => {
                match event {
                    Some(AppServerEvent::Lagged { .. }) => {
                        run = subscribe(
                            client,
                            &mut request_ids,
                            &run.run_id,
                            last_sequence,
                            output,
                            &mut last_sequence,
                        )
                        .await
                        .map_err(|error| render_request_error(output, error))?;
                    }
                    Some(AppServerEvent::ServerNotification(notification)) => {
                        if let Some(sequence) = workflow_sequence(&notification) {
                            let mut refreshed = false;
                            if sequence > last_sequence.saturating_add(1) {
                                run = subscribe(
                                    client,
                                    &mut request_ids,
                                    &run.run_id,
                                    last_sequence,
                                    output,
                                    &mut last_sequence,
                                )
                                .await
                                .map_err(|error| render_request_error(output, error))?;
                                refreshed = true;
                            }
                            if !refreshed && sequence > last_sequence {
                                output.event("workflowNotification", &notification)?;
                                last_sequence = sequence;
                                if let ServerNotification::WorkflowRunUpdated(update) = notification {
                                    run.status = update.status;
                                }
                            }
                        }
                    }
                    Some(AppServerEvent::ServerRequest(server_request)) => {
                        let message = if cli.non_interactive {
                            "workflow CLI is non-interactive and does not approve requests"
                        } else {
                            "workflow CLI does not auto-approve requests; use an interactive Codex client"
                        };
                        output.error("interactive_request_rejected", message)?;
                        client
                            .reject_server_request(
                                server_request.id().clone(),
                                JSONRPCErrorError {
                                    code: -32002,
                                    message: message.to_string(),
                                    data: None,
                                },
                            )
                            .await?;
                    }
                    Some(AppServerEvent::Disconnected { message }) => {
                        output.error("app_server_disconnected", &message)?;
                        return Ok(if interrupted { EXIT_INTERRUPT } else { EXIT_FAILURE });
                    }
                    None => {
                        output.error("app_server_disconnected", "app-server event stream closed")?;
                        return Ok(if interrupted { EXIT_INTERRUPT } else { EXIT_FAILURE });
                    }
                }
            }
        }

        if terminal(run.status) {
            let response: WorkflowRunReadResponse = request(
                client,
                ClientRequest::WorkflowRunRead {
                    request_id: request_ids.next(),
                    params: WorkflowRunReadParams {
                        run_id: run.run_id.clone(),
                    },
                },
            )
            .await
            .map_err(|error| render_request_error(output, error))?;
            run = response.run;
            output.result(&run, false)?;
            return Ok(exit_for_status(run.status, interrupted));
        }
    }
}

async fn subscribe(
    client: &AppServerClient,
    request_ids: &mut RequestIds,
    run_id: &str,
    after_sequence: u64,
    output: &mut WorkflowOutput<'_>,
    last_sequence: &mut u64,
) -> Result<WorkflowRun, RequestError> {
    let response: WorkflowRunSubscribeResponse = request(
        client,
        ClientRequest::WorkflowRunSubscribe {
            request_id: request_ids.next(),
            params: WorkflowRunSubscribeParams {
                run_id: run_id.to_string(),
                after_sequence: Some(after_sequence),
                node_threads: NodeThreadSubscription::Include,
            },
        },
    )
    .await?;
    if response.snapshot_required {
        output
            .progress("Workflow event history changed; refreshed the durable run snapshot.")
            .map_err(RequestError::Output)?;
        *last_sequence = response.run.next_sequence.saturating_sub(1);
    } else {
        for event in &response.events {
            if event.sequence > *last_sequence {
                output.replay_event(event).map_err(RequestError::Output)?;
                *last_sequence = event.sequence;
            }
        }
    }
    Ok(response.run)
}

fn workflow_sequence(notification: &ServerNotification) -> Option<u64> {
    match notification {
        ServerNotification::WorkflowRunUpdated(notification) => Some(notification.sequence),
        ServerNotification::WorkflowNodeUpdated(notification) => Some(notification.sequence),
        ServerNotification::WorkflowInteractionRequested(notification) => {
            Some(notification.sequence)
        }
        ServerNotification::WorkflowInteractionResolved(notification) => {
            Some(notification.sequence)
        }
        ServerNotification::WorkflowArtifactCreated(notification) => Some(notification.sequence),
        _ => None,
    }
}

fn terminal(status: WorkflowRunStatus) -> bool {
    matches!(
        status,
        WorkflowRunStatus::Succeeded
            | WorkflowRunStatus::Failed
            | WorkflowRunStatus::Cancelled
            | WorkflowRunStatus::NeedsOperator
    )
}

fn exit_for_status(status: WorkflowRunStatus, interrupted: bool) -> u8 {
    if interrupted {
        return EXIT_INTERRUPT;
    }
    match status {
        WorkflowRunStatus::Succeeded => EXIT_SUCCESS,
        _ => EXIT_FAILURE,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InterruptAction {
    Cancel,
    Exit,
}

fn next_interrupt(interrupted: &mut bool) -> InterruptAction {
    if *interrupted {
        InterruptAction::Exit
    } else {
        *interrupted = true;
        InterruptAction::Cancel
    }
}

async fn forward_interrupts(signal_tx: mpsc::Sender<()>) {
    loop {
        if tokio::signal::ctrl_c().await.is_err() || signal_tx.send(()).await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests;
