use codex_config::ConfigLayerEntry;
use codex_config::ConfigLayerSource;
use codex_config::ConfigLayerStack;
use codex_config::ConfigLayerStackOrdering;
use codex_core::config::Config;
use codex_core::skills::HostSkillsSnapshot;
use codex_protocol::models::AgentMessageInputContent;
use codex_protocol::models::ContentItem;
use codex_protocol::models::MessagePhase;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::RolloutItem;
use codex_workflow_extension::NodeApprovals;
use codex_workflow_extension::NodeHostError;
use codex_workflow_extension::NodeModel;
use codex_workflow_extension::NodeReasoningEffort;
use codex_workflow_extension::NodeSandbox;
use codex_workflow_extension::NodeTurnResult;
use codex_workflow_extension::NodeTurnStatus;
use codex_workflow_extension::NodeWorkingDirectory;
use codex_workflow_extension::SkillPolicy;

pub(super) fn apply_node_spec(
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

pub(super) fn validate_runner_approval_policy(
    non_interactive: bool,
    detached: bool,
    spec: &codex_workflow_extension::NodeSpec,
) -> Result<(), NodeHostError> {
    if !(non_interactive || detached)
        || matches!(
            spec.approvals(),
            NodeApprovals::RejectWhenDetached
                | NodeApprovals::Policy(codex_protocol::protocol::AskForApproval::Never)
        )
    {
        return Ok(());
    }
    Err(NodeHostError::InvalidRequest(
        "non-interactive or detached workflow nodes must reject approvals".to_string(),
    ))
}

pub(super) fn apply_host_skill_restrictions(
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
        .get_layers(ConfigLayerStackOrdering::LowestPrecedenceFirst, true)
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

pub(super) fn final_output_from_rollout(items: &[RolloutItem], turn_id: &str) -> Option<String> {
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

pub(super) fn terminal_error_from_rollout(items: &[RolloutItem], turn_id: &str) -> Option<String> {
    items.iter().rev().find_map(|item| match item {
        RolloutItem::EventMsg(EventMsg::TurnComplete(event)) if event.turn_id == turn_id => {
            event.error.as_ref().map(|error| error.message.clone())
        }
        _ => None,
    })
}

pub(super) fn recovered_turn_result(
    items: &[RolloutItem],
    turn_id: &str,
) -> Option<NodeTurnResult> {
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
