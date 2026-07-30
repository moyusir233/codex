use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;

use codex_app_server_protocol::ThreadHistoryBuilder;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::TurnStatus;
use codex_config::ConfigLayerEntry;
use codex_config::ConfigLayerSource;
use codex_config::ConfigLayerStack;
use codex_config::ConfigLayerStackOrdering;
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
use codex_thread_store::ListThreadsParams;
use codex_thread_store::LoadThreadHistoryParams;
use codex_thread_store::SortDirection;
use codex_thread_store::ThreadSortKey;
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
        let host_skills = self.host_skills_snapshot(&config).await;
        apply_host_skill_restrictions(&mut config, &request.spec, &host_skills)?;
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

    async fn find_materialized(
        &self,
        binding: WorkflowNodeBinding,
    ) -> Result<Vec<ThreadId>, NodeHostError> {
        let expected = ThreadSource::Feature(binding.thread_source());
        let mut matches = Vec::new();
        for archived in [false, true] {
            let mut cursor = None;
            loop {
                let page = self
                    .thread_store
                    .list_threads(ListThreadsParams {
                        page_size: 100,
                        cursor,
                        sort_key: ThreadSortKey::CreatedAt,
                        sort_direction: SortDirection::Asc,
                        allowed_sources: Vec::new(),
                        model_providers: Some(Vec::new()),
                        cwd_filters: None,
                        archived,
                        search_term: None,
                        relation_filter: None,
                        use_state_db_only: false,
                    })
                    .await
                    .map_err(|error| NodeHostError::Host(error.to_string()))?;
                matches.extend(
                    page.items
                        .into_iter()
                        .filter(|thread| thread.thread_source.as_ref() == Some(&expected))
                        .map(|thread| thread.thread_id),
                );
                let Some(next) = page.next_cursor else {
                    break;
                };
                cursor = Some(next);
            }
        }
        Ok(matches)
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

impl WorkflowNodeHost for AppServerWorkflowNodeHost {
    fn resolve_node_spec(
        &self,
        spec: codex_workflow_extension::NodeSpec,
    ) -> NodeHostFuture<'_, codex_workflow_extension::NodeSpec> {
        Box::pin(self.resolve_spec(spec))
    }

    fn materialize_node(
        &self,
        request: MaterializeNodeRequest,
    ) -> NodeHostFuture<'_, MaterializedNode> {
        Box::pin(self.materialize(request))
    }

    fn find_materialized_nodes(
        &self,
        binding: WorkflowNodeBinding,
    ) -> NodeHostFuture<'_, Vec<ThreadId>> {
        Box::pin(self.find_materialized(binding))
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

    fn recover_prepared_turn(
        &self,
        request: RecoverTurnRequest,
    ) -> NodeHostFuture<'_, RecoveredTurnState> {
        Box::pin(self.recover_turn(request))
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

fn apply_host_skill_restrictions(
    config: &mut Config,
    spec: &codex_workflow_extension::NodeSpec,
    snapshot: &HostSkillsSnapshot,
) -> Result<(), NodeHostError> {
    let allowed = match spec.skills() {
        SkillPolicy::Inherit => return Ok(()),
        SkillPolicy::Disabled => std::collections::HashSet::new(),
        SkillPolicy::AllowOnly(_) => spec
            .resolved_skills()
            .iter()
            .filter(|skill| skill.authority.kind == "host" && skill.authority.id == "host")
            .map(|skill| skill.package.0.as_str())
            .collect(),
    };
    let mut rules = Vec::new();
    for skill in &snapshot.outcome().skills {
        let path = skill.path_to_skills_md.to_string_lossy().into_owned();
        let mut rule = toml::map::Map::new();
        rule.insert("path".to_string(), toml::Value::String(path.clone()));
        rule.insert(
            "enabled".to_string(),
            toml::Value::Boolean(allowed.contains(path.as_str())),
        );
        rules.push(toml::Value::Table(rule));
    }
    let mut skills = toml::map::Map::new();
    skills.insert("config".to_string(), toml::Value::Array(rules));
    let mut root = toml::map::Map::new();
    root.insert("skills".to_string(), toml::Value::Table(skills));
    let workflow_layer =
        ConfigLayerEntry::new(ConfigLayerSource::SessionFlags, toml::Value::Table(root));
    let mut layers = config
        .config_layer_stack
        .get_layers(
            ConfigLayerStackOrdering::LowestPrecedenceFirst,
            /*include_disabled*/ true,
        )
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    let insertion = layers
        .iter()
        .position(|layer| layer.name.precedence() > ConfigLayerSource::SessionFlags.precedence())
        .unwrap_or(layers.len());
    layers.insert(insertion, workflow_layer);
    config.config_layer_stack = ConfigLayerStack::new(
        layers,
        config.config_layer_stack.requirements().clone(),
        config.config_layer_stack.requirements_toml().clone(),
    )
    .map_err(|error| NodeHostError::InvalidRequest(error.to_string()))?;
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

fn recovered_turn_result(items: &[RolloutItem], turn_id: &str) -> Option<NodeTurnResult> {
    let terminal = items.iter().rev().find_map(|item| match item {
        RolloutItem::EventMsg(EventMsg::TurnComplete(event)) if event.turn_id == turn_id => Some((
            if event.error.is_some() {
                NodeTurnStatus::Failed
            } else {
                NodeTurnStatus::Completed
            },
            event.error.as_ref().map(|error| error.message.clone()),
        )),
        RolloutItem::EventMsg(EventMsg::TurnAborted(event))
            if event.turn_id.as_deref() == Some(turn_id) =>
        {
            Some((NodeTurnStatus::Interrupted, None))
        }
        _ => None,
    })?;
    Some(NodeTurnResult {
        turn_id: turn_id.to_string(),
        status: terminal.0,
        final_output: final_output_from_rollout(items, turn_id),
        error: terminal.1,
    })
}

#[cfg(test)]
#[path = "workflow_node_host_tests.rs"]
mod tests;
