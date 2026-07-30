use std::sync::Arc;
use std::sync::Weak;

use codex_core::ThreadManager;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_thread_store::ThreadStore;

use crate::WorkflowService;

use super::WorkflowExtension;

pub fn install_with_backend<C>(
    registry: &mut ExtensionRegistryBuilder<C>,
    state: Arc<codex_state::StateRuntime>,
    service: Arc<WorkflowService>,
    thread_manager: Weak<ThreadManager>,
    thread_store: Arc<dyn ThreadStore>,
    workflows_enabled: impl Fn(&C) -> bool + Send + Sync + 'static,
) where
    C: Send + Sync + 'static,
{
    let extension = Arc::new(WorkflowExtension::new(
        state,
        service,
        thread_manager,
        thread_store,
        workflows_enabled,
    ));
    registry.thread_lifecycle_contributor(extension.clone());
    registry.config_contributor(extension.clone());
    registry.turn_lifecycle_contributor(extension.clone());
    registry.token_usage_contributor(extension.clone());
    registry.tool_lifecycle_contributor(extension.clone());
    registry.skill_invocation_contributor(extension);
}
