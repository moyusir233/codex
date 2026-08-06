use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;

use codex_app_server_protocol::ThreadHistoryBuilder;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::TurnStatus;
use codex_core::PreparedUserTurn;
use codex_core::PreparedUserTurnHistory;
use codex_core::PreparedUserTurnSubmission;
use codex_core::StartThreadOptions;
use codex_core::ThreadManager;
use codex_core::config::Config;
use codex_core::inspect_prepared_user_turn_history;
use codex_core::skills::HostSkillsSnapshot;
use codex_core::skills::SkillsLoadInput;
use codex_extension_api::ExtensionDataInit;
use codex_protocol::ThreadId;
use codex_protocol::protocol::AgentStatus;
use codex_protocol::protocol::InitialHistory;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::ResumedHistory;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::ThreadSettingsOverrides;
use codex_protocol::protocol::ThreadSource;
use codex_state::WorkflowStore;
use codex_thread_store::ArchiveThreadParams;
use codex_thread_store::DeleteThreadParams;
use codex_thread_store::ListThreadsParams;
use codex_thread_store::LoadThreadHistoryParams;
use codex_thread_store::ReadThreadParams;
use codex_thread_store::SortDirection;
use codex_thread_store::ThreadSortKey;
use codex_thread_store::ThreadStore;
use codex_workflow_extension::AwaitTurnRequest;
use codex_workflow_extension::CancellationReason;
use codex_workflow_extension::ConfirmedHistoryDeletion;
use codex_workflow_extension::MaterializeNodeRequest;
use codex_workflow_extension::MaterializedNode;
use codex_workflow_extension::NodeCollaborationMode;
use codex_workflow_extension::NodeHostError;
use codex_workflow_extension::NodeHostFuture;
use codex_workflow_extension::NodeRuntimeStatus;
use codex_workflow_extension::NodeTurnResult;
use codex_workflow_extension::NodeTurnStatus;
use codex_workflow_extension::PreparedTurnDisposition;
use codex_workflow_extension::PreparedTurnRequest;
use codex_workflow_extension::RecoverTurnRequest;
use codex_workflow_extension::RecoveredTurnState;
use codex_workflow_extension::ResolvedSkillSelection;
use codex_workflow_extension::RuntimeShutdown;
use codex_workflow_extension::SkillPolicy;
use codex_workflow_extension::SteerTurnRequest;
use codex_workflow_extension::SubmittedTurn;
use codex_workflow_extension::WorkflowNodeBinding;
use codex_workflow_extension::WorkflowNodeHost;
use codex_workflow_extension::WorkflowNodeLaunch;

use crate::outgoing_message::OutgoingMessageSender;
use crate::skills_watcher::SkillsWatcher;
use crate::thread_state::ThreadStateManager;
use crate::thread_state::wait_for_terminal_turn;
use crate::thread_status::ThreadWatchManager;
use crate::workflow_subscriptions::WorkflowSubscriptions;

use super::ListenerTaskContext;
use super::ensure_listener_task_running;

mod recovery;
mod support;
mod trait_impl;
use support::*;

pub(crate) struct AppServerWorkflowNodeHost {
    thread_manager: Arc<ThreadManager>,
    thread_store: Arc<dyn ThreadStore>,
    config: Arc<Config>,
    session_source: SessionSource,
    listener_task_context: ListenerTaskContext,
    workflow_subscriptions: Option<WorkflowSubscriptions>,
    workflow_store: WorkflowStore,
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
    pub workflow_subscriptions: Option<WorkflowSubscriptions>,
    pub workflow_store: WorkflowStore,
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
            workflow_subscriptions: args.workflow_subscriptions,
            workflow_store: args.workflow_store,
        }
    }

    async fn materialize(
        &self,
        request: MaterializeNodeRequest,
    ) -> Result<MaterializedNode, NodeHostError> {
        let run = self
            .workflow_store
            .read_run(&request.binding.run_id.to_string())
            .await
            .map_err(|error| NodeHostError::Host(error.to_string()))?
            .ok_or_else(|| {
                NodeHostError::InvalidRequest("workflow run was not found".to_string())
            })?;
        validate_runner_approval_policy(run.non_interactive, run.detached, &request.spec)?;
        let mut config = self.config.as_ref().clone();
        apply_node_spec(&mut config, &request.spec)?;
        let host_skills = self.host_skills_snapshot(&config).await;
        apply_host_skill_restrictions(&mut config, &request.spec, &host_skills)?;
        config.ephemeral = false;
        let environments = self
            .thread_manager
            .default_environment_selections(&config.cwd);
        let mut thread_extension_init = ExtensionDataInit::new();
        let thread_source = request.binding.thread_source();
        let run_id = request.binding.run_id.to_string();
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
        if let Some(subscriptions) = &self.workflow_subscriptions {
            for connection_id in subscriptions.connections_including_nodes(&run_id).await {
                self.listener_task_context
                    .thread_state_manager
                    .try_ensure_connection_subscribed(
                        thread_id,
                        connection_id,
                        /*experimental_raw_events*/ false,
                    )
                    .await;
            }
        }
        Ok(MaterializedNode { thread_id })
    }

    async fn resolve_spec(
        &self,
        spec: codex_workflow_extension::NodeSpec,
    ) -> Result<codex_workflow_extension::NodeSpec, NodeHostError> {
        if !matches!(spec.skills(), SkillPolicy::AllowOnly(_)) {
            return Ok(spec.with_resolved_skills(Vec::new()));
        }
        let mut config = self.config.as_ref().clone();
        apply_node_spec(&mut config, &spec)?;
        let snapshot = self.host_skills_snapshot(&config).await;
        let mut resolved = Vec::new();
        let SkillPolicy::AllowOnly(selectors) = spec.skills() else {
            unreachable!();
        };
        for selector in selectors {
            let skill = if selector.authority.kind == "host" {
                if selector.authority.id != "host" {
                    return Err(NodeHostError::InvalidRequest(
                        "host skill selectors must use authority id `host`".to_string(),
                    ));
                }
                let matches = snapshot
                    .outcome()
                    .skills
                    .iter()
                    .filter(|skill| {
                        skill.path_to_skills_md.to_string_lossy() == selector.package.0
                            && snapshot.outcome().is_skill_enabled(skill)
                    })
                    .collect::<Vec<_>>();
                match matches.as_slice() {
                    [] => {
                        return Err(NodeHostError::InvalidRequest(format!(
                            "workflow host skill is unknown or disabled: {}",
                            selector.package.0
                        )));
                    }
                    [skill] => ResolvedSkillSelection {
                        authority: selector.authority.clone(),
                        package: selector.package.clone(),
                        name: skill.name.clone(),
                        invocation_path: skill.path_to_skills_md.to_string_lossy().into_owned(),
                        initial_invocation: selector.initial_invocation,
                    },
                    _ => {
                        return Err(NodeHostError::InvalidRequest(format!(
                            "workflow host skill identity is ambiguous: {}",
                            selector.package.0
                        )));
                    }
                }
            } else {
                // Executor and orchestrator selectors are already opaque exact
                // provider identities. Their provider remains authoritative at
                // list/read time and fails closed when the identity is absent.
                ResolvedSkillSelection {
                    authority: selector.authority.clone(),
                    package: selector.package.clone(),
                    name: selector.package.0.clone(),
                    invocation_path: selector.package.0.clone(),
                    initial_invocation: selector.initial_invocation,
                }
            };
            resolved.push(skill);
        }
        Ok(spec.with_resolved_skills(resolved))
    }

    async fn host_skills_snapshot(&self, config: &Config) -> HostSkillsSnapshot {
        let plugins_input = config.plugins_config_input();
        let plugins_manager = self.thread_manager.plugins_manager();
        let plugin_outcome = plugins_manager.plugins_for_config(&plugins_input).await;
        let plugin_skill_snapshots =
            plugins_manager.plugin_skill_snapshots_for_config(&plugins_input);
        let input = SkillsLoadInput::new(
            config.cwd.clone(),
            plugin_outcome.effective_plugin_skill_roots(),
            config.config_layer_stack.clone(),
            config.bundled_skills_enabled(),
        )
        .with_plugin_skill_snapshots(plugin_skill_snapshots);
        self.thread_manager
            .skills_service()
            .snapshot_for_config(&input, None)
            .await
    }

    async fn recover_turn(
        &self,
        request: RecoverTurnRequest,
    ) -> Result<RecoveredTurnState, NodeHostError> {
        let history = self
            .thread_store
            .load_history(LoadThreadHistoryParams {
                thread_id: request.thread_id,
                include_archived: true,
            })
            .await
            .map_err(|error| NodeHostError::Host(error.to_string()))?;
        let prepared = PreparedUserTurn::new(&request.submission_id, request.input_hash)
            .map_err(|error| NodeHostError::InvalidRequest(error.to_string()))?;
        match inspect_prepared_user_turn_history(&history.items, &prepared) {
            PreparedUserTurnHistory::Missing => Ok(RecoveredTurnState::NoBoundary),
            PreparedUserTurnHistory::Conflict => Ok(RecoveredTurnState::Conflict),
            PreparedUserTurnHistory::BoundaryPersisted => Ok(recovered_turn_result(
                &history.items,
                &request.submission_id,
            )
            .map(RecoveredTurnState::Terminal)
            .unwrap_or(RecoveredTurnState::Unterminated)),
        }
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

#[cfg(test)]
#[path = "workflow_node_host_tests.rs"]
mod tests;
