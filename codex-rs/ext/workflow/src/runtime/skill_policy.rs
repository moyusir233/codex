use std::path::PathBuf;

use codex_protocol::user_input::UserInput;

use crate::NodeInput;
use crate::NodeSpec;
use crate::SkillInitialInvocation;

pub(super) fn apply_initial_skill_invocations(mut input: NodeInput, spec: &NodeSpec) -> NodeInput {
    for skill in spec
        .resolved_skills()
        .iter()
        .filter(|skill| skill.initial_invocation == SkillInitialInvocation::InvokeOnInitialTurn)
    {
        let path = PathBuf::from(&skill.invocation_path);
        let already_present = input.items.iter().any(|item| {
            matches!(
                item,
                UserInput::Skill {
                    name,
                    path: existing_path,
                } if name == &skill.name && existing_path == &path
            )
        });
        if !already_present {
            input.items.push(UserInput::Skill {
                name: skill.name.clone(),
                path,
            });
        }
    }
    input
}
