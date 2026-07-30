use codex_extension_api::ExtensionData;
use codex_skills_extension::SkillIdentity;
use codex_skills_extension::SkillVisibilityPolicy;
use codex_skills_extension::catalog::SkillAuthority;
use codex_skills_extension::catalog::SkillPackageId;
use codex_skills_extension::catalog::SkillSourceKind;

use crate::NodeSpec;
use crate::SkillPolicy;
use crate::WorkflowNodeBinding;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkflowExtensionConfig {
    pub execution_enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowBindingFault {
    pub reason: &'static str,
}

pub(super) fn attach_binding(
    thread_store: &ExtensionData,
    binding: WorkflowNodeBinding,
    spec: NodeSpec,
) {
    let visibility = skill_visibility(&spec);
    thread_store.insert(binding);
    thread_store.insert(spec);
    thread_store.insert(visibility);
    thread_store.remove::<WorkflowBindingFault>();
}

pub(super) fn attach_fault(thread_store: &ExtensionData, reason: &'static str) {
    thread_store.insert(WorkflowBindingFault { reason });
    // A workflow-marked thread with an unrecoverable binding must never fall
    // back to the ambient skill catalog.
    thread_store.insert(SkillVisibilityPolicy::DisableAll);
    thread_store.remove::<WorkflowNodeBinding>();
    thread_store.remove::<NodeSpec>();
}

fn skill_visibility(spec: &NodeSpec) -> SkillVisibilityPolicy {
    match spec.skills() {
        SkillPolicy::Inherit => SkillVisibilityPolicy::AllowAll,
        SkillPolicy::Disabled => SkillVisibilityPolicy::DisableAll,
        SkillPolicy::AllowOnly(_) => {
            SkillVisibilityPolicy::allow_only(spec.resolved_skills().iter().map(|skill| {
                SkillIdentity::new(
                    SkillAuthority::new(
                        skill_source_kind(&skill.authority.kind),
                        skill.authority.id.clone(),
                    ),
                    SkillPackageId(skill.package.0.clone()),
                )
            }))
        }
    }
}

fn skill_source_kind(value: &str) -> SkillSourceKind {
    match value {
        "host" => SkillSourceKind::Host,
        "executor" => SkillSourceKind::Executor,
        "orchestrator" => SkillSourceKind::Orchestrator,
        value => SkillSourceKind::custom(value),
    }
}
