use codex_extension_api::ExtensionData;

use crate::WorkflowNodeBinding;

pub(super) fn trace_lifecycle(
    phase: &'static str,
    thread_store: &ExtensionData,
    turn_id: Option<&str>,
) {
    let Some(binding) = thread_store.get::<WorkflowNodeBinding>() else {
        return;
    };
    tracing::debug!(
        workflow_run_id = %binding.run_id,
        workflow_node_id = %binding.node_id,
        thread_id = thread_store.level_id(),
        turn_id,
        phase,
        "workflow node lifecycle"
    );
}
