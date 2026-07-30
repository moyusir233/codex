#![allow(clippy::expect_used)]

use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::Mutex;

use codex_protocol::ThreadId;
use codex_state::StateRuntime;
use codex_state::WorkflowNodeAttemptStatus;
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
use codex_workflow_extension::NodeTurnStatus;
use codex_workflow_extension::PreparedTurnDisposition;
use codex_workflow_extension::PreparedTurnRequest;
use codex_workflow_extension::RecoverTurnRequest;
use codex_workflow_extension::RecoveredTurnState;
use codex_workflow_extension::RetryClassification;
use codex_workflow_extension::RetryPolicy;
use codex_workflow_extension::RetryRequest;
use codex_workflow_extension::RuntimeShutdown;
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
struct RecordingHost {
    thread_id: ThreadId,
    materializations: Mutex<Vec<MaterializeNodeRequest>>,
    submissions: Mutex<Vec<PreparedTurnRequest>>,
    operations: Mutex<Vec<String>>,
}

impl RecordingHost {
    fn new(thread_id: ThreadId) -> Self {
        Self {
            thread_id,
            ..Self::default()
        }
    }

    fn record(&self, operation: impl Into<String>) {
        self.operations
            .lock()
            .expect("operation lock")
            .push(operation.into());
    }
}

impl WorkflowNodeHost for RecordingHost {
    fn materialize_node(
        &self,
        request: MaterializeNodeRequest,
    ) -> NodeHostFuture<'_, MaterializedNode> {
        Box::pin(async move {
            self.materializations
                .lock()
                .expect("materialization lock")
                .push(request);
            self.record("materialize");
            Ok(MaterializedNode {
                thread_id: self.thread_id,
            })
        })
    }

    fn find_materialized_nodes(
        &self,
        _binding: WorkflowNodeBinding,
    ) -> NodeHostFuture<'_, Vec<ThreadId>> {
        Box::pin(async move {
            Ok(
                if self
                    .materializations
                    .lock()
                    .expect("materialization lock")
                    .is_empty()
                {
                    Vec::new()
                } else {
                    vec![self.thread_id]
                },
            )
        })
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
            self.record("submit");
            Ok(SubmittedTurn {
                turn_id,
                disposition: PreparedTurnDisposition::Queued,
            })
        })
    }

    fn await_terminal_turn(&self, request: AwaitTurnRequest) -> NodeHostFuture<'_, NodeTurnResult> {
        Box::pin(async move {
            self.record("await");
            Ok(NodeTurnResult {
                turn_id: request.turn_id,
                status: NodeTurnStatus::Completed,
                final_output: Some("done".to_string()),
                error: None,
            })
        })
    }

    fn recover_prepared_turn(
        &self,
        request: RecoverTurnRequest,
    ) -> NodeHostFuture<'_, RecoveredTurnState> {
        Box::pin(async move {
            Ok(RecoveredTurnState::Terminal(NodeTurnResult {
                turn_id: request.submission_id,
                status: NodeTurnStatus::Completed,
                final_output: Some("done".to_string()),
                error: None,
            }))
        })
    }

    fn steer(&self, _request: SteerTurnRequest) -> NodeHostFuture<'_, ()> {
        Box::pin(async move {
            self.record("steer");
            Ok(())
        })
    }

    fn status(&self, _thread_id: ThreadId) -> NodeHostFuture<'_, NodeRuntimeStatus> {
        Box::pin(async move {
            self.record("status");
            Ok(NodeRuntimeStatus::Idle)
        })
    }

    fn interrupt(&self, _thread_id: ThreadId, _turn_id: String) -> NodeHostFuture<'_, ()> {
        Box::pin(async move {
            self.record("interrupt");
            Ok(())
        })
    }

    fn cancel(&self, _thread_id: ThreadId, _reason: CancellationReason) -> NodeHostFuture<'_, ()> {
        Box::pin(async move {
            self.record("cancel");
            Ok(())
        })
    }

    fn shutdown_runtime(
        &self,
        _thread_id: ThreadId,
        mode: RuntimeShutdown,
    ) -> NodeHostFuture<'_, ()> {
        Box::pin(async move {
            self.record(format!("shutdown:{mode:?}"));
            Ok(())
        })
    }

    fn detach_observer(&self, _thread_id: ThreadId) -> NodeHostFuture<'_, ()> {
        Box::pin(async move {
            self.record("detach");
            Ok(())
        })
    }

    fn archive(&self, _thread_id: ThreadId, archived: bool) -> NodeHostFuture<'_, ()> {
        Box::pin(async move {
            self.record(format!("archive:{archived}"));
            Ok(())
        })
    }

    fn delete(&self, _confirmation: ConfirmedHistoryDeletion) -> NodeHostFuture<'_, ()> {
        Box::pin(async move {
            self.record("delete");
            Ok(())
        })
    }
}

#[tokio::test]
async fn node_lifecycle_journals_idempotent_turns_and_delegates_explicit_operations() {
    let home = tempfile::tempdir().expect("create temporary Codex home");
    let runtime = StateRuntime::init(home.path().to_path_buf(), "test-provider".to_string())
        .await
        .expect("initialize state runtime");
    let run_id = WorkflowRunId::new();
    runtime
        .workflows()
        .create_run(WorkflowRunCreate {
            run_id: run_id.to_string(),
            definition_name: "node-lifecycle".to_string(),
            definition_version: "1.0.0".to_string(),
            state_schema_version: 1,
            state: json!({}),
            arguments: json!({}),
            created_at_ms: 1,
        })
        .await
        .expect("create workflow run");
    let thread_id = ThreadId::new();
    let host = Arc::new(RecordingHost::new(thread_id));
    let slot = WorkflowNodeHostSlot::new();
    slot.bind(host.clone()).expect("bind node host");
    let service = WorkflowService::new(runtime.workflows().clone(), slot);
    let spec = NodeSpec::builder(NodeKey::new("primary").expect("node key"))
        .retry(RetryPolicy {
            maximum_attempts: NonZeroU32::new(2).expect("nonzero attempts"),
            retry_on: vec![RetryClassification::Interrupted],
            ..RetryPolicy::default()
        })
        .build()
        .expect("node spec");

    let node = service
        .nodes(run_id)
        .ensure(
            EffectKey::new("ensure-primary").expect("effect key"),
            spec.clone(),
        )
        .await
        .expect("ensure node");
    let replayed = service
        .nodes(run_id)
        .ensure(
            EffectKey::new("ensure-primary").expect("effect key"),
            spec.clone(),
        )
        .await
        .expect("replay ensure");
    let second_effect = service
        .nodes(run_id)
        .ensure(
            EffectKey::new("ensure-primary-again").expect("effect key"),
            spec,
        )
        .await
        .expect("ensure existing node");
    assert_eq!(
        (node.id(), node.thread_id()),
        (replayed.id(), replayed.thread_id())
    );
    assert_eq!(second_effect.thread_id(), thread_id);
    assert_eq!(
        host.materializations
            .lock()
            .expect("materialization lock")
            .len(),
        1
    );

    let turn = node
        .start(
            EffectKey::new("initial-turn").expect("effect key"),
            NodeInput::text("run once"),
        )
        .await
        .expect("start node");
    let replayed_turn = node
        .start(
            EffectKey::new("initial-turn").expect("effect key"),
            NodeInput::text("run once"),
        )
        .await
        .expect("replay node start");
    assert_eq!(replayed_turn, turn);
    assert_eq!(host.submissions.lock().expect("submission lock").len(), 1);
    let conflict = node
        .submit(
            EffectKey::new("initial-turn").expect("effect key"),
            NodeInput::text("different input"),
        )
        .await
        .expect_err("effect key cannot be reused for different input");
    assert!(matches!(
        conflict,
        codex_workflow_extension::NodeError::Store(codex_state::WorkflowStoreError::EffectConflict)
    ));

    assert_eq!(
        node.await_turn(turn.turn_id.clone())
            .await
            .expect("await terminal turn"),
        NodeTurnResult {
            turn_id: turn.turn_id.clone(),
            status: NodeTurnStatus::Completed,
            final_output: Some("done".to_string()),
            error: None,
        }
    );
    assert_eq!(
        service
            .store()
            .read_node_attempt_by_turn_id(&node.id().to_string(), &turn.turn_id)
            .await
            .expect("read completed attempt")
            .expect("completed attempt")
            .status,
        WorkflowNodeAttemptStatus::Succeeded
    );
    let retry = node
        .retry(
            EffectKey::new("retry-turn").expect("effect key"),
            RetryRequest::same_thread(NodeInput::text("retry")),
        )
        .await
        .expect("retry node");
    assert_ne!(retry.turn_id, turn.turn_id);
    assert_eq!(
        service
            .store()
            .read_node_attempt_by_turn_id(&node.id().to_string(), &retry.turn_id)
            .await
            .expect("read retry attempt")
            .expect("retry attempt")
            .status,
        WorkflowNodeAttemptStatus::Submitted
    );
    node.cancel(CancellationReason::WorkflowRequested)
        .await
        .expect("cancel node");
    assert_eq!(
        node.status().await.expect("node status"),
        NodeRuntimeStatus::Idle
    );
    node.steer(turn.turn_id.clone(), NodeInput::text("steer"))
        .await
        .expect("steer node");
    node.interrupt(turn.turn_id.clone())
        .await
        .expect("interrupt node");
    node.shutdown_runtime(RuntimeShutdown::Graceful)
        .await
        .expect("shutdown runtime");
    node.archive_history().await.expect("archive history");
    node.unarchive_history().await.expect("unarchive history");

    let operations_before_drop = host.operations.lock().expect("operation lock").len();
    drop(node.clone());
    assert_eq!(
        host.operations.lock().expect("operation lock").len(),
        operations_before_drop
    );
    node.clone()
        .detach_observer()
        .await
        .expect("detach explicit observer");
    let confirmation = ConfirmedHistoryDeletion::new(
        WorkflowNodeBinding {
            run_id,
            node_id: node.id(),
        },
        thread_id,
    );
    node.delete_history(confirmation)
        .await
        .expect("delete confirmed history");

    assert_eq!(
        *host.operations.lock().expect("operation lock"),
        vec![
            "materialize",
            "submit",
            "await",
            "submit",
            "cancel",
            "status",
            "steer",
            "interrupt",
            "shutdown:Graceful",
            "archive:true",
            "archive:false",
            "detach",
            "delete",
        ]
    );
    runtime.close().await;
}

#[test]
fn node_host_slot_binds_once() {
    let slot = WorkflowNodeHostSlot::new();
    slot.bind(Arc::new(RecordingHost::new(ThreadId::new())))
        .expect("first bind succeeds");
    assert!(matches!(
        slot.bind(Arc::new(RecordingHost::new(ThreadId::new()))),
        Err(codex_workflow_extension::NodeHostBindError::AlreadyBound)
    ));
}
