use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;

use codex_app_server_protocol::ThreadHistoryBuilder;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::TurnStatus;
use codex_core::PreparedUserTurn;
use codex_core::PreparedUserTurnSubmission;
use codex_core::StartThreadOptions;
use codex_core::ThreadManager;
use codex_core::config::Config;
use codex_extension_api::ExtensionDataInit;
use codex_protocol::ThreadId;
use codex_protocol::models::AgentMessageInputContent;
use codex_protocol::models::ContentItem;
use codex_protocol::models::MessagePhase;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::AgentStatus;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::InitialHistory;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::RolloutItem;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::ThreadSettingsOverrides;
use codex_protocol::protocol::ThreadSource;
use codex_thread_store::ArchiveThreadParams;
use codex_thread_store::DeleteThreadParams;
use codex_thread_store::ThreadStore;
use codex_workflow_extension::AwaitTurnRequest;
use codex_workflow_extension::CancellationReason;
use codex_workflow_extension::ConfirmedHistoryDeletion;
use codex_workflow_extension::MaterializeNodeRequest;
use codex_workflow_extension::MaterializedNode;
use codex_workflow_extension::NodeApprovals;
use codex_workflow_extension::NodeCollaborationMode;
use codex_workflow_extension::NodeHostError;
use codex_workflow_extension::NodeHostFuture;
use codex_workflow_extension::NodeModel;
use codex_workflow_extension::NodeReasoningEffort;
use codex_workflow_extension::NodeRuntimeStatus;
use codex_workflow_extension::NodeSandbox;
use codex_workflow_extension::NodeTurnResult;
use codex_workflow_extension::NodeTurnStatus;
use codex_workflow_extension::NodeWorkingDirectory;
use codex_workflow_extension::PreparedTurnDisposition;
use codex_workflow_extension::PreparedTurnRequest;
use codex_workflow_extension::RuntimeShutdown;
use codex_workflow_extension::SteerTurnRequest;
use codex_workflow_extension::SubmittedTurn;
use codex_workflow_extension::WorkflowNodeHost;
use codex_workflow_extension::WorkflowNodeLaunch;

use crate::outgoing_message::OutgoingMessageSender;
use crate::skills_watcher::SkillsWatcher;
use crate::thread_state::ThreadStateManager;
use crate::thread_state::wait_for_terminal_turn;
use crate::thread_status::ThreadWatchManager;

use super::ListenerTaskContext;
use super::ensure_listener_task_running;

pub(crate) struct AppServerWorkflowNodeHost {
    thread_manager: Arc<ThreadManager>,
    thread_store: Arc<dyn ThreadStore>,
    config: Arc<Config>,
    session_source: SessionSource,
    listener_task_context: ListenerTaskContext,
}

pub(crate) struct AppServerWorkflowNodeHostArgs {
    pub thread_manager: Arc<ThreadManager>,
    pub thread_store: Arc<dyn ThreadStore>,
    pub config: Arc<Config>,
    pub session_source: SessionSource,
    pub outgoing: Arc<OutgoingMessageSender>,
    pub pending_thread_unloads: Arc<tokio::sync::Mutex<std::collections::HashSet<ThreadId>>>,
    pub thread_state_manager: ThreadStateManager,
    pub thread_watch_manager: ThreadWatchManager,
    pub thread_list_state_permit: Arc<tokio::sync::Semaphore>,
    pub skills_watcher: Arc<SkillsWatcher>,
}

impl AppServerWorkflowNodeHost {
    pub(crate) fn new(args: AppServerWorkflowNodeHostArgs) -> Self {
        let listener_task_context = ListenerTaskContext {
            thread_manager: Arc::clone(&args.thread_manager),
            thread_state_manager: args.thread_state_manager,
            outgoing: args.outgoing,
            pending_thread_unloads: args.pending_thread_unloads,
            thread_watch_manager: args.thread_watch_manager,
            thread_list_state_permit: args.thread_list_state_permit,
            fallback_model_provider: args.config.model_provider_id.clone(),
            codex_home: args.config.codex_home.to_path_buf(),
            skills_watcher: args.skills_watcher,
        };
        Self {
            thread_manager: args.thread_manager,
            thread_store: args.thread_store,
            config: args.config,
            session_source: args.session_source,
            listener_task_context,
        }
    }

    async fn materialize(
        &self,
        request: MaterializeNodeRequest,
    ) -> Result<MaterializedNode, NodeHostError> {
        let mut config = self.config.as_ref().clone();
        apply_node_spec(&mut config, &request.spec)?;
        config.ephemeral = false;
        let environments = self
            .thread_manager
            .default_environment_selections(&config.cwd);
        let mut thread_extension_init = ExtensionDataInit::new();
        let thread_source = request.binding.thread_source();
        thread_extension_init.insert(WorkflowNodeLaunch {
            binding: request.binding,
            spec: request.spec,
        });
        let new_thread = self
            .thread_manager
            .start_thread_with_options(StartThreadOptions {
                config,
                allow_provider_model_fallback: false,
                initial_history: InitialHistory::New,
                history_mode: None,
                session_source: Some(self.session_source.clone()),
                thread_source: Some(ThreadSource::Feature(thread_source)),
                dynamic_tools: Vec::new(),
                metrics_service_name: Some("codex_workflow".to_string()),
                parent_trace: None,
                environments,
                thread_extension_init,
                supports_openai_form_elicitation: false,
            })
            .await
            .map_err(|error| NodeHostError::Host(error.to_string()))?;
        let thread_id = new_thread.thread_id;
        self.listener_task_context
            .thread_state_manager
            .retain_internal_observer(thread_id)
            .await;
        self.listener_task_context
            .thread_watch_manager
            .note_thread_loaded(&thread_id.to_string())
            .await;
        let thread_state = self
            .listener_task_context
            .thread_state_manager
            .thread_state(thread_id)
            .await;
        if let Err(error) = ensure_listener_task_running(
            self.listener_task_context.clone(),
            thread_id,
            Arc::clone(&new_thread.thread),
            thread_state,
        )
        .await
        {
            self.listener_task_context
                .thread_state_manager
                .release_internal_observer(thread_id)
                .await;
            return Err(NodeHostError::Host(error.message));
        }
        Ok(MaterializedNode { thread_id })
    }

    async fn submit_turn(
        &self,
        request: PreparedTurnRequest,
    ) -> Result<SubmittedTurn, NodeHostError> {
        let thread = self
            .thread_manager
            .get_thread(request.thread_id)
            .await
            .map_err(|_| NodeHostError::ThreadNotFound)?;
        let collaboration_mode = match request.spec.collaboration_mode() {
            NodeCollaborationMode::WorkflowDefault => None,
            NodeCollaborationMode::Exact(mode) => Some(mode.clone()),
        };
        let metadata: Option<HashMap<String, String>> = request
            .input
            .responsesapi_client_metadata
            .map(|metadata| metadata.into_iter().collect());
        let op = Op::UserInput {
            items: request.input.items,
            final_output_json_schema: request.input.final_output_json_schema,
            responsesapi_client_metadata: metadata,
            additional_context: BTreeMap::new(),
            thread_settings: ThreadSettingsOverrides {
                collaboration_mode,
                ..Default::default()
            },
        };
        let prepared = PreparedUserTurn::new(&request.submission_id, request.input_hash)
            .map_err(|error| NodeHostError::InvalidRequest(error.to_string()))?;
        let disposition = thread
            .submit_prepared_user_turn(op, None, prepared)
            .await
            .map_err(|error| NodeHostError::Host(error.to_string()))?;
        Ok(SubmittedTurn {
            turn_id: request.submission_id,
            disposition: match disposition {
                PreparedUserTurnSubmission::Queue => PreparedTurnDisposition::Queued,
                PreparedUserTurnSubmission::AlreadyQueued => PreparedTurnDisposition::AlreadyQueued,
                PreparedUserTurnSubmission::BoundaryAlreadyPersisted => {
                    PreparedTurnDisposition::BoundaryAlreadyPersisted
                }
            },
        })
    }

    async fn await_turn(&self, request: AwaitTurnRequest) -> Result<NodeTurnResult, NodeHostError> {
        let thread = self
            .thread_manager
            .get_thread(request.thread_id)
            .await
            .map_err(|_| NodeHostError::ThreadNotFound)?;
        let thread_state = self
            .listener_task_context
            .thread_state_manager
            .thread_state(request.thread_id)
            .await;
        let observation = wait_for_terminal_turn(&thread_state, request.turn_id.clone())
            .await
            .ok_or_else(|| NodeHostError::Host("thread listener is not running".to_string()))?;
        thread
            .flush_rollout()
            .await
            .map_err(|error| NodeHostError::Host(error.to_string()))?;
        let history = thread
            .load_history(/*include_archived*/ true)
            .await
            .map_err(|error| NodeHostError::Host(error.to_string()))?;
        let mut history_builder = ThreadHistoryBuilder::new();
        for item in &history.items {
            history_builder.handle_rollout_item(item);
        }
        let turn = history_builder
            .finish()
            .into_iter()
            .find(|turn| turn.id == request.turn_id)
            .ok_or_else(|| {
                NodeHostError::Host(
                    "terminal turn was not present in the flushed rollout".to_string(),
                )
            })?;
        let final_output = turn
            .items
            .iter()
            .rev()
            .find_map(|item| match item {
                ThreadItem::AgentMessage { text, .. } => Some(text.clone()),
                _ => None,
            })
            .or_else(|| final_output_from_rollout(&history.items, &request.turn_id));
        let error = turn
            .error
            .map(|error| error.message)
            .or_else(|| terminal_error_from_rollout(&history.items, &request.turn_id));
        let status = match turn.status {
            TurnStatus::Completed => NodeTurnStatus::Completed,
            TurnStatus::Interrupted => NodeTurnStatus::Interrupted,
            TurnStatus::Failed => NodeTurnStatus::Failed,
            TurnStatus::InProgress => {
                return Err(NodeHostError::Host(
                    "terminal turn remained in progress after rollout flush".to_string(),
                ));
            }
        };
        let status = if error.is_some() {
            NodeTurnStatus::Failed
        } else {
            status
        };
        let status = match observation.status {
            crate::thread_state::ThreadTerminalStatus::Completed => status,
            crate::thread_state::ThreadTerminalStatus::Aborted
                if status != NodeTurnStatus::Failed =>
            {
                NodeTurnStatus::Interrupted
            }
            crate::thread_state::ThreadTerminalStatus::Aborted => status,
        };
        Ok(NodeTurnResult {
            turn_id: observation.turn_id,
            status,
            final_output,
            error,
        })
    }

    async fn shutdown(
        &self,
        thread_id: ThreadId,
        mode: RuntimeShutdown,
    ) -> Result<(), NodeHostError> {
        let Ok(thread) = self.thread_manager.get_thread(thread_id).await else {
            return Ok(());
        };
        if matches!(mode, RuntimeShutdown::InterruptThenShutdown) {
            thread
                .submit(Op::Interrupt)
                .await
                .map_err(|error| NodeHostError::Host(format!("{error:?}")))?;
        }
        thread
            .flush_rollout()
            .await
            .map_err(|error| NodeHostError::Host(error.to_string()))?;
        thread
            .shutdown_and_wait()
            .await
            .map_err(|error| NodeHostError::Host(error.to_string()))?;
        self.thread_manager.remove_thread(&thread_id).await;
        self.listener_task_context
            .thread_state_manager
            .remove_thread_state(thread_id)
            .await;
        self.listener_task_context
            .thread_watch_manager
            .remove_thread(&thread_id.to_string())
            .await;
        Ok(())
    }
}

impl WorkflowNodeHost for AppServerWorkflowNodeHost {
    fn materialize_node(
        &self,
        request: MaterializeNodeRequest,
    ) -> NodeHostFuture<'_, MaterializedNode> {
        Box::pin(self.materialize(request))
    }

    fn submit_prepared_turn(
        &self,
        request: PreparedTurnRequest,
    ) -> NodeHostFuture<'_, SubmittedTurn> {
        Box::pin(self.submit_turn(request))
    }

    fn await_terminal_turn(&self, request: AwaitTurnRequest) -> NodeHostFuture<'_, NodeTurnResult> {
        Box::pin(self.await_turn(request))
    }

    fn steer(&self, request: SteerTurnRequest) -> NodeHostFuture<'_, ()> {
        Box::pin(async move {
            let thread = self
                .thread_manager
                .get_thread(request.thread_id)
                .await
                .map_err(|_| NodeHostError::ThreadNotFound)?;
            let metadata = request
                .input
                .responsesapi_client_metadata
                .map(|metadata| metadata.into_iter().collect());
            thread
                .steer_input(
                    request.input.items,
                    BTreeMap::new(),
                    Some(&request.turn_id),
                    None,
                    metadata,
                )
                .await
                .map_err(|error| NodeHostError::Host(format!("{error:?}")))?;
            Ok(())
        })
    }

    fn status(&self, thread_id: ThreadId) -> NodeHostFuture<'_, NodeRuntimeStatus> {
        Box::pin(async move {
            let Ok(thread) = self.thread_manager.get_thread(thread_id).await else {
                return Ok(NodeRuntimeStatus::Shutdown);
            };
            Ok(match thread.agent_status().await {
                AgentStatus::PendingInit | AgentStatus::Running => NodeRuntimeStatus::Running,
                AgentStatus::Completed(_) | AgentStatus::Errored(_) | AgentStatus::Interrupted => {
                    NodeRuntimeStatus::Idle
                }
                AgentStatus::Shutdown | AgentStatus::NotFound => NodeRuntimeStatus::Shutdown,
            })
        })
    }

    fn interrupt(&self, thread_id: ThreadId, _turn_id: String) -> NodeHostFuture<'_, ()> {
        Box::pin(async move {
            self.thread_manager
                .get_thread(thread_id)
                .await
                .map_err(|_| NodeHostError::ThreadNotFound)?
                .submit(Op::Interrupt)
                .await
                .map_err(|error| NodeHostError::Host(error.to_string()))?;
            Ok(())
        })
    }

    fn cancel(&self, thread_id: ThreadId, _reason: CancellationReason) -> NodeHostFuture<'_, ()> {
        Box::pin(async move {
            self.thread_manager
                .get_thread(thread_id)
                .await
                .map_err(|_| NodeHostError::ThreadNotFound)?
                .submit(Op::Interrupt)
                .await
                .map_err(|error| NodeHostError::Host(error.to_string()))?;
            Ok(())
        })
    }

    fn shutdown_runtime(
        &self,
        thread_id: ThreadId,
        mode: RuntimeShutdown,
    ) -> NodeHostFuture<'_, ()> {
        Box::pin(self.shutdown(thread_id, mode))
    }

    fn detach_observer(&self, thread_id: ThreadId) -> NodeHostFuture<'_, ()> {
        Box::pin(async move {
            if self
                .listener_task_context
                .thread_state_manager
                .release_internal_observer(thread_id)
                .await
            {
                Ok(())
            } else {
                Err(NodeHostError::InvalidRequest(
                    "workflow node observer was not retained".to_string(),
                ))
            }
        })
    }

    fn archive(&self, thread_id: ThreadId, archived: bool) -> NodeHostFuture<'_, ()> {
        Box::pin(async move {
            self.shutdown(thread_id, RuntimeShutdown::Graceful).await?;
            if archived {
                self.thread_store
                    .archive_thread(ArchiveThreadParams { thread_id })
                    .await
                    .map_err(|error| NodeHostError::Host(error.to_string()))
            } else {
                self.thread_store
                    .unarchive_thread(ArchiveThreadParams { thread_id })
                    .await
                    .map(|_| ())
                    .map_err(|error| NodeHostError::Host(error.to_string()))
            }
        })
    }

    fn delete(&self, confirmation: ConfirmedHistoryDeletion) -> NodeHostFuture<'_, ()> {
        Box::pin(async move {
            self.shutdown(confirmation.thread_id, RuntimeShutdown::Graceful)
                .await?;
            self.thread_store
                .delete_thread(DeleteThreadParams {
                    thread_id: confirmation.thread_id,
                })
                .await
                .map_err(|error| NodeHostError::Host(error.to_string()))
        })
    }
}

fn apply_node_spec(
    config: &mut Config,
    spec: &codex_workflow_extension::NodeSpec,
) -> Result<(), NodeHostError> {
    if let NodeWorkingDirectory::Exact(cwd) = spec.working_directory() {
        config.cwd = cwd.clone();
    }
    if let NodeModel::Named(model) = spec.model() {
        config.model = Some(model.clone());
    }
    if let NodeReasoningEffort::Exact(effort) = spec.reasoning_effort() {
        config.model_reasoning_effort = effort.clone();
    }
    if let NodeSandbox::Policy(policy) = spec.sandbox() {
        config
            .set_legacy_sandbox_policy(policy.clone())
            .map_err(|error| NodeHostError::InvalidRequest(error.to_string()))?;
    }
    let approvals = match spec.approvals() {
        NodeApprovals::WorkflowDefault | NodeApprovals::ExistingClient => None,
        NodeApprovals::RejectWhenDetached => Some(codex_protocol::protocol::AskForApproval::Never),
        NodeApprovals::Policy(policy) => Some(*policy),
    };
    if let Some(approvals) = approvals {
        config
            .permissions
            .approval_policy
            .set(approvals)
            .map_err(|error| NodeHostError::InvalidRequest(error.to_string()))?;
    }
    Ok(())
}

fn final_output_from_rollout(items: &[RolloutItem], turn_id: &str) -> Option<String> {
    let mut inside_turn = false;
    for item in items.iter().rev() {
        match item {
            RolloutItem::EventMsg(EventMsg::TurnComplete(event)) => {
                inside_turn = event.turn_id == turn_id;
                if inside_turn && let Some(message) = event.last_agent_message.as_ref() {
                    return Some(message.clone());
                }
            }
            RolloutItem::EventMsg(EventMsg::TurnAborted(event)) => {
                inside_turn = event.turn_id.as_deref() == Some(turn_id);
            }
            RolloutItem::EventMsg(EventMsg::TurnStarted(event))
                if inside_turn && event.turn_id == turn_id =>
            {
                break;
            }
            RolloutItem::EventMsg(EventMsg::AgentMessage(event))
                if inside_turn
                    && !matches!(event.phase, Some(MessagePhase::Commentary))
                    && !event.message.trim().is_empty() =>
            {
                return Some(event.message.clone());
            }
            RolloutItem::ResponseItem(ResponseItem::Message {
                role,
                content,
                phase,
                ..
            }) if inside_turn
                && role == "assistant"
                && !matches!(phase, Some(MessagePhase::Commentary)) =>
            {
                let text = content
                    .iter()
                    .filter_map(|content| match content {
                        ContentItem::OutputText { text } => Some(text.as_str()),
                        ContentItem::InputText { .. } | ContentItem::InputImage { .. } => None,
                    })
                    .collect::<String>();
                if !text.trim().is_empty() {
                    return Some(text);
                }
            }
            RolloutItem::ResponseItem(ResponseItem::AgentMessage { content, .. })
                if inside_turn =>
            {
                let mut text = String::new();
                for content in content {
                    match content {
                        AgentMessageInputContent::InputText { text: part } => text.push_str(part),
                        AgentMessageInputContent::EncryptedContent { .. } => return None,
                    }
                }
                if !text.trim().is_empty() {
                    return Some(text);
                }
            }
            _ => {}
        }
    }
    None
}

fn terminal_error_from_rollout(items: &[RolloutItem], turn_id: &str) -> Option<String> {
    items.iter().rev().find_map(|item| match item {
        RolloutItem::EventMsg(EventMsg::TurnComplete(event)) if event.turn_id == turn_id => {
            event.error.as_ref().map(|error| error.message.clone())
        }
        _ => None,
    })
}

#[cfg(test)]
#[path = "workflow_node_host_tests.rs"]
mod tests;
