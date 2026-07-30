use std::sync::Arc;
use std::sync::Weak;

use codex_core::ThreadManager;
use codex_extension_api::ConfigContributor;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::SkillInvocationContributor;
use codex_extension_api::SkillInvocationInput;
use codex_extension_api::ThreadIdleInput;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadResumeInput;
use codex_extension_api::ThreadStartInput;
use codex_extension_api::ThreadStopInput;
use codex_extension_api::TokenUsageContributor;
use codex_extension_api::ToolFinishInput;
use codex_extension_api::ToolLifecycleContributor;
use codex_extension_api::ToolLifecycleFuture;
use codex_extension_api::ToolStartInput;
use codex_extension_api::TurnAbortInput;
use codex_extension_api::TurnErrorInput;
use codex_extension_api::TurnLifecycleContributor;
use codex_extension_api::TurnStartInput;
use codex_extension_api::TurnStopInput;
use codex_protocol::protocol::ThreadSource;
use codex_protocol::protocol::TokenUsageInfo;
use codex_thread_store::ThreadStore;

use crate::NodeSpec;
use crate::WorkflowNodeBinding;
use crate::WorkflowNodeLaunch;
use crate::WorkflowService;

use super::binding::WorkflowExtensionConfig;
use super::binding::attach_binding;
use super::binding::attach_fault;
use super::telemetry::trace_lifecycle;

#[derive(Clone)]
pub struct WorkflowExtension<C> {
    state: Arc<codex_state::StateRuntime>,
    service: Arc<WorkflowService>,
    thread_manager: Weak<ThreadManager>,
    thread_store: Arc<dyn ThreadStore>,
    workflows_enabled: Arc<dyn Fn(&C) -> bool + Send + Sync>,
}

impl<C> std::fmt::Debug for WorkflowExtension<C> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorkflowExtension")
            .finish_non_exhaustive()
    }
}

impl<C> WorkflowExtension<C> {
    pub(crate) fn new(
        state: Arc<codex_state::StateRuntime>,
        service: Arc<WorkflowService>,
        thread_manager: Weak<ThreadManager>,
        thread_store: Arc<dyn ThreadStore>,
        workflows_enabled: impl Fn(&C) -> bool + Send + Sync + 'static,
    ) -> Self {
        Self {
            state,
            service,
            thread_manager,
            thread_store,
            workflows_enabled: Arc::new(workflows_enabled),
        }
    }

    async fn restore_binding(
        &self,
        input: &ThreadStartInput<'_, C>,
        expected: WorkflowNodeBinding,
    ) {
        if !input.persistent_thread_state_available {
            attach_fault(
                input.thread_store,
                "persistent workflow state is unavailable",
            );
            return;
        }
        let node = match self
            .state
            .workflows()
            .read_node_by_thread_id(input.thread_store.level_id())
            .await
        {
            Ok(Some(node)) => node,
            Ok(None) => {
                attach_fault(input.thread_store, "workflow node binding was not found");
                return;
            }
            Err(error) => {
                tracing::warn!(
                    thread_id = input.thread_store.level_id(),
                    "failed to restore workflow node binding: {error}"
                );
                attach_fault(
                    input.thread_store,
                    "workflow node binding could not be read",
                );
                return;
            }
        };
        if node.run_id != expected.run_id.to_string()
            || node.node_id != expected.node_id.to_string()
        {
            attach_fault(
                input.thread_store,
                "workflow thread source does not match durable node",
            );
            return;
        }
        let spec = match serde_json::from_value::<NodeSpec>(node.spec) {
            Ok(spec) => spec,
            Err(error) => {
                tracing::warn!(
                    workflow_run_id = %expected.run_id,
                    workflow_node_id = %expected.node_id,
                    "failed to decode workflow node specification: {error}"
                );
                attach_fault(input.thread_store, "workflow node specification is invalid");
                return;
            }
        };
        attach_binding(input.thread_store, expected, spec);
    }
}

impl<C> ThreadLifecycleContributor<C> for WorkflowExtension<C>
where
    C: Send + Sync + 'static,
{
    fn on_thread_start<'a>(&'a self, input: ThreadStartInput<'a, C>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            input.thread_store.insert(WorkflowExtensionConfig {
                execution_enabled: (self.workflows_enabled)(input.config),
            });
            // Retain the process capabilities on the extension object without
            // exposing them through WorkflowContext.
            let _manager_is_live = self.thread_manager.strong_count() > 0;
            let _thread_store = Arc::clone(&self.thread_store);
            let _service = Arc::clone(&self.service);

            if let Some(launch) = input.thread_store.get::<WorkflowNodeLaunch>() {
                attach_binding(
                    input.thread_store,
                    launch.binding.clone(),
                    launch.spec.clone(),
                );
                trace_lifecycle("thread_start", input.thread_store, None);
                return;
            }
            let Some(ThreadSource::Feature(source)) = input.thread_source else {
                return;
            };
            if !source.starts_with(WorkflowNodeBinding::THREAD_SOURCE_PREFIX) {
                return;
            }
            let Some(binding) = WorkflowNodeBinding::from_thread_source(source) else {
                attach_fault(input.thread_store, "workflow thread source is malformed");
                return;
            };
            self.restore_binding(&input, binding).await;
            trace_lifecycle("thread_restore", input.thread_store, None);
        })
    }

    fn on_thread_resume<'a>(&'a self, input: ThreadResumeInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            trace_lifecycle("thread_resume", input.thread_store, None);
        })
    }

    fn on_thread_idle<'a>(&'a self, input: ThreadIdleInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            trace_lifecycle("thread_idle", input.thread_store, None);
        })
    }

    fn on_thread_stop<'a>(&'a self, input: ThreadStopInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            trace_lifecycle("thread_stop", input.thread_store, None);
        })
    }
}

impl<C> ConfigContributor<C> for WorkflowExtension<C>
where
    C: Send + Sync + 'static,
{
    fn on_config_changed(
        &self,
        _session_store: &ExtensionData,
        thread_store: &ExtensionData,
        _previous_config: &C,
        new_config: &C,
    ) {
        thread_store.insert(WorkflowExtensionConfig {
            execution_enabled: (self.workflows_enabled)(new_config),
        });
    }
}

impl<C> TurnLifecycleContributor for WorkflowExtension<C>
where
    C: Send + Sync + 'static,
{
    fn on_turn_start<'a>(&'a self, input: TurnStartInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if let Some(binding) = input.thread_store.get::<WorkflowNodeBinding>()
                && let Err(error) = self
                    .service
                    .observe_turn_start(&binding, input.turn_id)
                    .await
            {
                tracing::warn!(
                    workflow_run_id = %binding.run_id,
                    workflow_node_id = %binding.node_id,
                    "failed to classify workflow turn ownership: {error}"
                );
            }
            trace_lifecycle("turn_start", input.thread_store, Some(input.turn_id));
        })
    }

    fn on_turn_stop<'a>(&'a self, input: TurnStopInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            trace_lifecycle(
                "turn_stop",
                input.thread_store,
                Some(input.turn_store.level_id()),
            );
        })
    }

    fn on_turn_abort<'a>(&'a self, input: TurnAbortInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            trace_lifecycle(
                "turn_abort",
                input.thread_store,
                Some(input.turn_store.level_id()),
            );
        })
    }

    fn on_turn_error<'a>(&'a self, input: TurnErrorInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            trace_lifecycle("turn_error", input.thread_store, Some(input.turn_id));
        })
    }
}

impl<C> TokenUsageContributor for WorkflowExtension<C>
where
    C: Send + Sync + 'static,
{
    fn on_token_usage<'a>(
        &'a self,
        _session_store: &'a ExtensionData,
        thread_store: &'a ExtensionData,
        turn_store: &'a ExtensionData,
        _token_usage: &'a TokenUsageInfo,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            trace_lifecycle("token_usage", thread_store, Some(turn_store.level_id()));
        })
    }
}

impl<C> ToolLifecycleContributor for WorkflowExtension<C>
where
    C: Send + Sync + 'static,
{
    fn on_tool_start<'a>(&'a self, input: ToolStartInput<'a>) -> ToolLifecycleFuture<'a> {
        Box::pin(async move {
            trace_lifecycle("tool_start", input.thread_store, Some(input.turn_id));
        })
    }

    fn on_tool_finish<'a>(&'a self, input: ToolFinishInput<'a>) -> ToolLifecycleFuture<'a> {
        Box::pin(async move {
            trace_lifecycle("tool_finish", input.thread_store, Some(input.turn_id));
        })
    }
}

impl<C> SkillInvocationContributor for WorkflowExtension<C>
where
    C: Send + Sync + 'static,
{
    fn on_skill_invocation<'a>(
        &'a self,
        input: SkillInvocationInput<'a>,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            trace_lifecycle("skill_invocation", input.thread_store, Some(input.turn_id));
        })
    }
}
