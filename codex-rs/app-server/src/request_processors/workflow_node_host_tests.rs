use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use anyhow::Result;
use app_test_support::create_mock_responses_server_repeating_assistant;
use app_test_support::write_mock_responses_config_toml;
use codex_analytics::AppServerRpcTransport;
use codex_app_server_protocol::ClientInfo;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::InitializeCapabilities;
use codex_app_server_protocol::InitializeParams;
use codex_app_server_protocol::InitializeResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::TurnStatus;
use codex_arg0::Arg0DispatchPaths;
use codex_config::CloudConfigBundleLoader;
use codex_config::LoaderOverrides;
use codex_core::config::Config;
use codex_core::config::ConfigBuilder;
use codex_exec_server::EnvironmentManager;
use codex_features::Feature;
use codex_feedback::CodexFeedback;
use codex_login::AuthManager;
use codex_protocol::protocol::SessionSource;
use codex_state::StateRuntime;
use codex_state::WorkflowRunCreate;
use codex_thread_store::ReadThreadParams;
use codex_thread_store::ThreadStoreError;
use codex_workflow_extension::ConfirmedHistoryDeletion;
use codex_workflow_extension::EffectKey;
use codex_workflow_extension::NodeApprovals;
use codex_workflow_extension::NodeInput;
use codex_workflow_extension::NodeKey;
use codex_workflow_extension::NodeSpec;
use codex_workflow_extension::NodeTurnStatus;
use codex_workflow_extension::RuntimeShutdown;
use codex_workflow_extension::SkillPolicy;
use codex_workflow_extension::WorkflowNodeBinding;
use codex_workflow_extension::WorkflowRunId;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn detached_and_non_interactive_nodes_require_fail_closed_approvals() -> Result<()> {
    let inherited = NodeSpec::builder(NodeKey::new("inherited")?).build()?;
    assert!(super::validate_runner_approval_policy(true, false, &inherited).is_err());
    assert!(super::validate_runner_approval_policy(false, true, &inherited).is_err());

    let rejected = NodeSpec::builder(NodeKey::new("rejected")?)
        .approvals(NodeApprovals::RejectWhenDetached)
        .build()?;
    super::validate_runner_approval_policy(true, false, &rejected)?;
    super::validate_runner_approval_policy(false, true, &rejected)?;

    let never = NodeSpec::builder(NodeKey::new("never")?)
        .approvals(NodeApprovals::Policy(
            codex_protocol::protocol::AskForApproval::Never,
        ))
        .build()?;
    super::validate_runner_approval_policy(true, true, &never)?;
    Ok(())
}
use tempfile::TempDir;
use tokio::sync::mpsc;

use crate::PluginStartupTasks;
use crate::analytics_utils::analytics_events_client_from_config;
use crate::config_manager::ConfigManager;
use crate::message_processor::ConnectionSessionState;
use crate::message_processor::MessageProcessor;
use crate::message_processor::MessageProcessorArgs;
use crate::outgoing_message::ConnectionId;
use crate::outgoing_message::OutgoingEnvelope;
use crate::outgoing_message::OutgoingMessage;
use crate::outgoing_message::OutgoingMessageSender;

const WORKFLOW_CONNECTION_ID: ConnectionId = ConnectionId(71);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn extension_order_restores_workflow_binding_before_skills_when_launches_are_disabled()
-> Result<()> {
    let server = create_mock_responses_server_repeating_assistant("workflow complete").await;
    let codex_home = TempDir::new()?;
    let skill_dir = codex_home.path().join("skills").join("must-stay-hidden");
    tokio::fs::create_dir_all(&skill_dir).await?;
    tokio::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: must-stay-hidden\ndescription: workflow visibility sentinel\n---\nHIDDEN_SKILL_SENTINEL\n",
    )
    .await?;
    let config = Arc::new(build_config(codex_home.path(), &server.uri()).await?);
    let state =
        StateRuntime::init(codex_home.path().to_path_buf(), "mock_provider".to_string()).await?;
    let thread_store = codex_core::thread_store_from_config(config.as_ref(), Some(state.clone()));
    let (processor, mut outgoing_rx) =
        build_processor(Arc::clone(&config), Arc::clone(&state)).await;
    let session = Arc::new(ConnectionSessionState::new());
    let outbound_initialized = AtomicBool::new(false);
    processor
        .process_client_request(
            WORKFLOW_CONNECTION_ID,
            ClientRequest::Initialize {
                request_id: RequestId::Integer(1),
                params: InitializeParams {
                    client_info: ClientInfo {
                        name: "workflow-node-test".to_string(),
                        title: None,
                        version: "0.1.0".to_string(),
                    },
                    capabilities: Some(InitializeCapabilities {
                        experimental_api: true,
                        ..Default::default()
                    }),
                },
            },
            Arc::clone(&session),
            &outbound_initialized,
        )
        .await;
    let _: InitializeResponse = read_response(&mut outgoing_rx, 1).await;

    let run_id = WorkflowRunId::new();
    state
        .workflows()
        .create_run(WorkflowRunCreate {
            run_id: run_id.to_string(),
            definition_name: "node-lifecycle".to_string(),
            definition_version: "1.0.0".to_string(),
            state_schema_version: 1,
            state: json!({}),
            arguments: json!({}),
            non_interactive: false,
            detached: false,
            concurrency: None,
            created_at_ms: 1,
        })
        .await?;
    let service = processor
        .workflow_service()
        .expect("state-backed app-server installs workflow service");
    let node = service
        .nodes(run_id)
        .ensure(
            EffectKey::new("ensure-primary")?,
            NodeSpec::builder(NodeKey::new("primary")?)
                .skills(SkillPolicy::Disabled)
                .build()?,
        )
        .await?;
    let thread_id = node.thread_id();

    processor
        .try_attach_thread_listener(thread_id, vec![WORKFLOW_CONNECTION_ID])
        .await;
    let turn = node
        .start(
            EffectKey::new("initial-turn")?,
            NodeInput::text("finish the workflow node"),
        )
        .await?;
    let result = node.await_turn(turn.turn_id.clone()).await?;
    assert_eq!(
        result.status,
        NodeTurnStatus::Completed,
        "node turn failed: {:?}",
        result.error
    );
    assert_eq!(result.final_output.as_deref(), Some("workflow complete"));

    let (client_started, client_completed) =
        read_client_turn_notifications(&mut outgoing_rx, &turn.turn_id).await;
    assert!(client_started);
    assert!(client_completed);

    node.shutdown_runtime(RuntimeShutdown::Graceful).await?;
    let stored = thread_store
        .read_thread(ReadThreadParams {
            thread_id,
            include_archived: true,
            include_history: true,
        })
        .await?;
    assert!(stored.history.is_some(), "shutdown must preserve rollout");

    processor
        .process_client_request(
            WORKFLOW_CONNECTION_ID,
            ClientRequest::ThreadResume {
                request_id: RequestId::Integer(2),
                params: ThreadResumeParams {
                    thread_id: thread_id.to_string(),
                    config: Some(HashMap::from([(
                        "features.workflows".to_string(),
                        json!(false),
                    )])),
                    ..Default::default()
                },
            },
            Arc::clone(&session),
            &outbound_initialized,
        )
        .await;
    let resumed: ThreadResumeResponse = read_response(&mut outgoing_rx, 2).await;
    assert_eq!(resumed.thread.id, thread_id.to_string());
    assert_eq!(
        resumed.thread.turns.last().map(|turn| &turn.status),
        Some(&TurnStatus::Completed)
    );
    let resumed_turn = node
        .submit(
            EffectKey::new("resumed-turn")?,
            NodeInput::text("continue after explicit resume"),
        )
        .await?;
    let resumed_result = node.await_turn(resumed_turn.turn_id).await?;
    assert_eq!(
        resumed_result.status,
        NodeTurnStatus::Completed,
        "resumed node turn failed: {:?}",
        resumed_result.error
    );
    let requests = server.received_requests().await.unwrap_or_default();
    assert_eq!(requests.len(), 2);
    for request in requests {
        let body = String::from_utf8_lossy(&request.body);
        assert!(
            !body.contains("HIDDEN_SKILL_SENTINEL"),
            "disabled workflow skill leaked into provider request"
        );
    }

    node.shutdown_runtime(RuntimeShutdown::Graceful).await?;
    node.delete_history(ConfirmedHistoryDeletion::new(
        WorkflowNodeBinding {
            run_id,
            node_id: node.id(),
        },
        thread_id,
    ))
    .await?;
    let missing = thread_store
        .read_thread(ReadThreadParams {
            thread_id,
            include_archived: true,
            include_history: true,
        })
        .await
        .expect_err("explicit delete is the only operation that removes history");
    assert!(
        matches!(&missing, ThreadStoreError::ThreadNotFound { .. })
            || matches!(
                &missing,
                ThreadStoreError::InvalidRequest { message }
                    if message == &format!("no rollout found for thread id {thread_id}")
            ),
        "unexpected read result after delete: {missing:?}"
    );

    processor.shutdown_threads().await;
    processor.drain_background_tasks().await;
    drop(processor);
    state.close().await;
    Ok(())
}

async fn build_config(codex_home: &Path, server_uri: &str) -> Result<Config> {
    write_mock_responses_config_toml(
        codex_home,
        server_uri,
        &BTreeMap::from([(Feature::Workflows, true)]),
        /*auto_compact_limit*/ 8_192,
        Some(false),
        "mock_provider",
        "compact",
    )?;
    Ok(ConfigBuilder::default()
        .codex_home(codex_home.to_path_buf())
        .build()
        .await?)
}

async fn build_processor(
    config: Arc<Config>,
    state: Arc<StateRuntime>,
) -> (Arc<MessageProcessor>, mpsc::Receiver<OutgoingEnvelope>) {
    let (outgoing_tx, outgoing_rx) = mpsc::channel(64);
    let auth_manager =
        AuthManager::shared_from_config(config.as_ref(), /*enable_codex_api_key_env*/ false).await;
    let config_manager = ConfigManager::new(
        config.codex_home.to_path_buf(),
        Vec::new(),
        LoaderOverrides::default(),
        /*strict_config*/ false,
        CloudConfigBundleLoader::default(),
        Arg0DispatchPaths::default(),
        Arc::new(codex_config::NoopThreadConfigLoader),
    );
    let analytics_events_client =
        analytics_events_client_from_config(Arc::clone(&auth_manager), config.as_ref());
    let outgoing = Arc::new(OutgoingMessageSender::new(
        outgoing_tx,
        analytics_events_client.clone(),
    ));
    (
        Arc::new(MessageProcessor::new(MessageProcessorArgs {
            outgoing,
            analytics_events_client,
            arg0_paths: Arg0DispatchPaths::default(),
            config,
            config_manager,
            environment_manager: Arc::new(EnvironmentManager::default_for_tests()),
            feedback: CodexFeedback::new(),
            log_db: None,
            state_db: Some(state),
            config_warnings: Vec::new(),
            session_source: SessionSource::VSCode,
            auth_manager,
            installation_id: "11111111-1111-4111-8111-111111111111".to_string(),
            rpc_transport: AppServerRpcTransport::Stdio,
            remote_control_handle: None,
            plugin_startup_tasks: PluginStartupTasks::Skip,
        })),
        outgoing_rx,
    )
}

async fn read_response<T: serde::de::DeserializeOwned>(
    outgoing_rx: &mut mpsc::Receiver<OutgoingEnvelope>,
    request_id: i64,
) -> T {
    loop {
        let envelope = tokio::time::timeout(Duration::from_secs(10), outgoing_rx.recv())
            .await
            .expect("timed out waiting for app-server response")
            .expect("outgoing channel closed");
        let OutgoingEnvelope::ToConnection {
            connection_id,
            message,
            ..
        } = envelope
        else {
            continue;
        };
        if connection_id != WORKFLOW_CONNECTION_ID {
            continue;
        }
        let OutgoingMessage::Response(response) = message else {
            continue;
        };
        if response.id == RequestId::Integer(request_id) {
            return serde_json::from_value(response.result)
                .expect("response payload should deserialize");
        }
    }
}

async fn read_client_turn_notifications(
    outgoing_rx: &mut mpsc::Receiver<OutgoingEnvelope>,
    expected_turn_id: &str,
) -> (bool, bool) {
    let mut started = false;
    let mut completed = false;
    while !completed {
        let envelope = tokio::time::timeout(Duration::from_secs(10), outgoing_rx.recv())
            .await
            .expect("timed out waiting for normal turn notifications")
            .expect("outgoing channel closed");
        let OutgoingEnvelope::ToConnection {
            connection_id,
            message: OutgoingMessage::AppServerNotification(notification),
            ..
        } = envelope
        else {
            continue;
        };
        if connection_id != WORKFLOW_CONNECTION_ID {
            continue;
        }
        match notification {
            ServerNotification::TurnStarted(notification)
                if notification.turn.id == expected_turn_id =>
            {
                started = true;
            }
            ServerNotification::TurnCompleted(notification)
                if notification.turn.id == expected_turn_id =>
            {
                completed = true;
            }
            _ => {}
        }
    }
    (started, completed)
}
