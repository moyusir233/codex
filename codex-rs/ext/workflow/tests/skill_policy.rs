#![allow(clippy::expect_used)]

use std::sync::Arc;
use std::sync::Mutex;

use codex_protocol::ThreadId;
use codex_protocol::user_input::UserInput;
use codex_state::StateRuntime;
use codex_state::WorkflowRunCreate;
use codex_workflow_extension::AwaitTurnRequest;
use codex_workflow_extension::CancellationReason;
use codex_workflow_extension::ConfirmedHistoryDeletion;
use codex_workflow_extension::EffectKey;
use codex_workflow_extension::MaterializeNodeRequest;
use codex_workflow_extension::MaterializedNode;
use codex_workflow_extension::NodeHostFuture;
use codex_workflow_extension::NodeInput;
use codex_workflow_extension::NodeKey;
use codex_workflow_extension::NodeRuntimeStatus;
use codex_workflow_extension::NodeSpec;
use codex_workflow_extension::NodeTurnResult;
use codex_workflow_extension::PreparedTurnDisposition;
use codex_workflow_extension::PreparedTurnRequest;
use codex_workflow_extension::RecoverTurnRequest;
use codex_workflow_extension::RecoveredTurnState;
use codex_workflow_extension::ResolvedSkillSelection;
use codex_workflow_extension::RuntimeShutdown;
use codex_workflow_extension::SkillAuthoritySelector;
use codex_workflow_extension::SkillInitialInvocation;
use codex_workflow_extension::SkillPackageSelector;
use codex_workflow_extension::SkillPolicy;
use codex_workflow_extension::SkillSelector;
use codex_workflow_extension::SteerTurnRequest;
use codex_workflow_extension::SubmittedTurn;
use codex_workflow_extension::WorkflowNodeBinding;
use codex_workflow_extension::WorkflowNodeHost;
use codex_workflow_extension::WorkflowNodeHostSlot;
use codex_workflow_extension::WorkflowRunId;
use codex_workflow_extension::WorkflowService;
use pretty_assertions::assert_eq;
use serde_json::json;

#[derive(Default)]
struct SkillPolicyHost {
    thread_id: ThreadId,
    submissions: Mutex<Vec<PreparedTurnRequest>>,
}

impl SkillPolicyHost {
    fn resolved(spec: NodeSpec) -> NodeSpec {
        let SkillPolicy::AllowOnly(selectors) = spec.skills() else {
            return spec.with_resolved_skills(Vec::new());
        };
        spec.clone().with_resolved_skills(
            selectors
                .iter()
                .map(|selector| ResolvedSkillSelection {
                    authority: selector.authority.clone(),
                    package: selector.package.clone(),
                    name: format!("{}-skill", selector.authority.kind),
                    invocation_path: format!("skill://{}/SKILL.md", selector.package.0),
                    initial_invocation: selector.initial_invocation,
                })
                .collect(),
        )
    }
}

impl WorkflowNodeHost for SkillPolicyHost {
    fn resolve_node_spec(&self, spec: NodeSpec) -> NodeHostFuture<'_, NodeSpec> {
        Box::pin(async move { Ok(Self::resolved(spec)) })
    }

    fn materialize_node(
        &self,
        _request: MaterializeNodeRequest,
    ) -> NodeHostFuture<'_, MaterializedNode> {
        Box::pin(async move {
            Ok(MaterializedNode {
                thread_id: self.thread_id,
            })
        })
    }

    fn find_materialized_nodes(
        &self,
        _binding: WorkflowNodeBinding,
    ) -> NodeHostFuture<'_, Vec<ThreadId>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn submit_prepared_turn(
        &self,
        request: PreparedTurnRequest,
    ) -> NodeHostFuture<'_, SubmittedTurn> {
        Box::pin(async move {
            let turn_id = request.submission_id.clone();
            self.submissions
                .lock()
                .expect("submission lock")
                .push(request);
            Ok(SubmittedTurn {
                turn_id,
                disposition: PreparedTurnDisposition::Queued,
            })
        })
    }

    fn await_terminal_turn(
        &self,
        _request: AwaitTurnRequest,
    ) -> NodeHostFuture<'_, NodeTurnResult> {
        Box::pin(async { unreachable!("turn completion is not needed") })
    }

    fn recover_prepared_turn(
        &self,
        _request: RecoverTurnRequest,
    ) -> NodeHostFuture<'_, RecoveredTurnState> {
        Box::pin(async { Ok(RecoveredTurnState::NoBoundary) })
    }

    fn steer(&self, _request: SteerTurnRequest) -> NodeHostFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn status(&self, _thread_id: ThreadId) -> NodeHostFuture<'_, NodeRuntimeStatus> {
        Box::pin(async { Ok(NodeRuntimeStatus::Idle) })
    }

    fn interrupt(&self, _thread_id: ThreadId, _turn_id: String) -> NodeHostFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn cancel(&self, _thread_id: ThreadId, _reason: CancellationReason) -> NodeHostFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn shutdown_runtime(
        &self,
        _thread_id: ThreadId,
        _mode: RuntimeShutdown,
    ) -> NodeHostFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn detach_observer(&self, _thread_id: ThreadId) -> NodeHostFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn archive(&self, _thread_id: ThreadId, _archived: bool) -> NodeHostFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn delete(&self, _confirmation: ConfirmedHistoryDeletion) -> NodeHostFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn skill_policy_persists_all_authority_resolution_and_uses_normal_initial_invocations() {
    let home = tempfile::tempdir().expect("temporary Codex home");
    let runtime = StateRuntime::init(home.path().to_path_buf(), "test-provider".to_string())
        .await
        .expect("initialize state runtime");
    let run_id = WorkflowRunId::new();
    runtime
        .workflows()
        .create_run(WorkflowRunCreate {
            run_id: run_id.to_string(),
            definition_name: "skill-policy".to_string(),
            definition_version: "1.0.0".to_string(),
            state_schema_version: 1,
            state: json!({}),
            arguments: json!({}),
            non_interactive: false,
            detached: false,
            concurrency: None,
            created_at_ms: 1,
        })
        .await
        .expect("create workflow run");
    let host = Arc::new(SkillPolicyHost {
        thread_id: ThreadId::new(),
        ..SkillPolicyHost::default()
    });
    let slot = WorkflowNodeHostSlot::new();
    slot.bind(host.clone()).expect("bind node host");
    let service = WorkflowService::new(runtime.workflows().clone(), slot);
    let selectors = ["host", "executor", "orchestrator"]
        .into_iter()
        .map(selector)
        .collect::<Vec<_>>();
    let spec = NodeSpec::builder(NodeKey::new("skills").expect("node key"))
        .skills(SkillPolicy::AllowOnly(selectors.clone()))
        .build()
        .expect("node spec");

    let node = service
        .nodes(run_id)
        .ensure(EffectKey::new("ensure").expect("effect key"), spec)
        .await
        .expect("ensure node");
    let stored = runtime
        .workflows()
        .read_node(&node.id().to_string())
        .await
        .expect("read stored node")
        .expect("stored node");
    let stored_spec =
        serde_json::from_value::<NodeSpec>(stored.spec).expect("decode persisted node spec");
    assert_eq!(stored_spec.resolved_skills().len(), 3);
    assert_eq!(
        stored_spec
            .resolved_skills()
            .iter()
            .map(|skill| skill.authority.kind.as_str())
            .collect::<Vec<_>>(),
        vec!["host", "executor", "orchestrator"]
    );

    let effect = EffectKey::new("initial").expect("effect key");
    node.start(effect.clone(), NodeInput::text("begin"))
        .await
        .expect("submit initial turn");
    node.start(effect, NodeInput::text("begin"))
        .await
        .expect("replay initial turn");
    node.submit(
        EffectKey::new("follow-up").expect("effect key"),
        NodeInput::text("continue"),
    )
    .await
    .expect("submit follow-up turn");

    let submissions = host.submissions.lock().expect("submission lock");
    assert_eq!(submissions.len(), 2, "effect replay must not queue twice");
    let initial_skill_count = submissions[0]
        .input
        .items
        .iter()
        .filter(|item| matches!(item, UserInput::Skill { .. }))
        .count();
    assert_eq!(initial_skill_count, 3);
    assert!(
        submissions[1]
            .input
            .items
            .iter()
            .all(|item| !matches!(item, UserInput::Skill { .. }))
    );
}

fn selector(kind: &str) -> SkillSelector {
    SkillSelector {
        authority: SkillAuthoritySelector {
            kind: kind.to_string(),
            id: format!("{kind}-authority"),
        },
        package: SkillPackageSelector(format!("{kind}-package")),
        initial_invocation: SkillInitialInvocation::InvokeOnInitialTurn,
    }
}
