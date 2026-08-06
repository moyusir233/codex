#![allow(dead_code)]
#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use codex_protocol::ThreadId;
use codex_state::StateRuntime;
use codex_state::WorkflowRunCreate;
use codex_workflow_extension::AwaitTurnRequest;
use codex_workflow_extension::CancellationReason;
use codex_workflow_extension::ConfirmedHistoryDeletion;
use codex_workflow_extension::MaterializeNodeRequest;
use codex_workflow_extension::MaterializedNode;
use codex_workflow_extension::NodeHostError;
use codex_workflow_extension::NodeHostFuture;
use codex_workflow_extension::NodeRuntimeStatus;
use codex_workflow_extension::NodeTurnResult;
use codex_workflow_extension::NodeTurnStatus;
use codex_workflow_extension::PreparedTurnDisposition;
use codex_workflow_extension::PreparedTurnRequest;
use codex_workflow_extension::RecoverTurnRequest;
use codex_workflow_extension::RecoveredTurnState;
use codex_workflow_extension::RuntimeShutdown;
use codex_workflow_extension::SteerTurnRequest;
use codex_workflow_extension::SubmittedTurn;
use codex_workflow_extension::WorkflowNodeBinding;
use codex_workflow_extension::WorkflowNodeHost;
use codex_workflow_extension::WorkflowNodeHostSlot;
use codex_workflow_extension::WorkflowRegistry;
use codex_workflow_extension::WorkflowRunId;
use codex_workflow_extension::WorkflowService;
use serde_json::Value;
use serde_json::json;

pub struct TestHost {
    threads: Mutex<Vec<ThreadId>>,
    bindings: Mutex<BTreeMap<String, ThreadId>>,
    submissions: Mutex<BTreeMap<String, [u8; 32]>>,
    recoveries: Mutex<BTreeMap<String, RecoveredTurnState>>,
    cancelled: AtomicBool,
    await_delay_ms: AtomicUsize,
    pub materializations: AtomicUsize,
    pub unique_queues: AtomicUsize,
    pub cancels: AtomicUsize,
    pub detaches: AtomicUsize,
    pub deletes: AtomicUsize,
}

impl Default for TestHost {
    fn default() -> Self {
        Self {
            threads: Mutex::new(Vec::new()),
            bindings: Mutex::new(BTreeMap::new()),
            submissions: Mutex::new(BTreeMap::new()),
            recoveries: Mutex::new(BTreeMap::new()),
            cancelled: AtomicBool::new(false),
            await_delay_ms: AtomicUsize::new(0),
            materializations: AtomicUsize::new(0),
            unique_queues: AtomicUsize::new(0),
            cancels: AtomicUsize::new(0),
            detaches: AtomicUsize::new(0),
            deletes: AtomicUsize::new(0),
        }
    }
}

impl TestHost {
    pub fn with_thread(thread_id: ThreadId) -> Self {
        Self {
            threads: Mutex::new(vec![thread_id]),
            ..Self::default()
        }
    }

    pub fn threads(&self) -> Vec<ThreadId> {
        self.threads.lock().expect("threads lock").clone()
    }

    pub fn set_recovery(&self, submission_id: impl Into<String>, state: RecoveredTurnState) {
        self.recoveries
            .lock()
            .expect("recoveries lock")
            .insert(submission_id.into(), state);
    }

    pub fn set_await_delay(&self, delay: Duration) {
        self.await_delay_ms.store(
            usize::try_from(delay.as_millis()).unwrap_or(usize::MAX),
            Ordering::Release,
        );
    }
}

impl WorkflowNodeHost for TestHost {
    fn materialize_node(
        &self,
        request: MaterializeNodeRequest,
    ) -> NodeHostFuture<'_, MaterializedNode> {
        Box::pin(async move {
            let thread_id = ThreadId::new();
            self.threads.lock().expect("threads lock").push(thread_id);
            self.bindings
                .lock()
                .expect("bindings lock")
                .insert(binding_key(&request.binding), thread_id);
            self.materializations.fetch_add(1, Ordering::AcqRel);
            Ok(MaterializedNode { thread_id })
        })
    }

    fn find_materialized_nodes(
        &self,
        binding: WorkflowNodeBinding,
    ) -> NodeHostFuture<'_, Vec<ThreadId>> {
        Box::pin(async move {
            let bindings = self.bindings.lock().expect("bindings lock");
            if let Some(thread_id) = bindings.get(&binding_key(&binding)) {
                return Ok(vec![*thread_id]);
            }
            if bindings.is_empty() {
                return Ok(self.threads());
            }
            Ok(Vec::new())
        })
    }

    fn submit_prepared_turn(
        &self,
        request: PreparedTurnRequest,
    ) -> NodeHostFuture<'_, SubmittedTurn> {
        Box::pin(async move {
            let mut submissions = self.submissions.lock().expect("submissions lock");
            let disposition = match submissions.get(&request.submission_id) {
                Some(hash) if hash == &request.input_hash => {
                    PreparedTurnDisposition::BoundaryAlreadyPersisted
                }
                Some(_) => {
                    return Err(NodeHostError::InvalidRequest(
                        "prepared input hash conflict".to_string(),
                    ));
                }
                None => {
                    submissions.insert(request.submission_id.clone(), request.input_hash);
                    self.unique_queues.fetch_add(1, Ordering::AcqRel);
                    PreparedTurnDisposition::Queued
                }
            };
            Ok(SubmittedTurn {
                turn_id: request.submission_id,
                disposition,
            })
        })
    }

    fn await_terminal_turn(&self, request: AwaitTurnRequest) -> NodeHostFuture<'_, NodeTurnResult> {
        Box::pin(async move {
            let delay_ms = self.await_delay_ms.load(Ordering::Acquire);
            if delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(
                    u64::try_from(delay_ms).unwrap_or(u64::MAX),
                ))
                .await;
            }
            let interrupted = self.cancelled.load(Ordering::Acquire);
            Ok(NodeTurnResult {
                turn_id: request.turn_id,
                status: if interrupted {
                    NodeTurnStatus::Interrupted
                } else {
                    NodeTurnStatus::Completed
                },
                final_output: (!interrupted).then(|| "done".to_string()),
                error: None,
            })
        })
    }

    fn recover_prepared_turn(
        &self,
        request: RecoverTurnRequest,
    ) -> NodeHostFuture<'_, RecoveredTurnState> {
        Box::pin(async move {
            Ok(self
                .recoveries
                .lock()
                .expect("recoveries lock")
                .get(&request.submission_id)
                .cloned()
                .unwrap_or(RecoveredTurnState::NoBoundary))
        })
    }

    fn steer(&self, _request: SteerTurnRequest) -> NodeHostFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn status(&self, _thread_id: ThreadId) -> NodeHostFuture<'_, NodeRuntimeStatus> {
        Box::pin(async { Ok(NodeRuntimeStatus::Idle) })
    }

    fn interrupt(&self, _thread_id: ThreadId, _turn_id: String) -> NodeHostFuture<'_, ()> {
        Box::pin(async move {
            self.cancelled.store(true, Ordering::Release);
            Ok(())
        })
    }

    fn cancel(&self, _thread_id: ThreadId, _reason: CancellationReason) -> NodeHostFuture<'_, ()> {
        Box::pin(async move {
            self.cancelled.store(true, Ordering::Release);
            self.cancels.fetch_add(1, Ordering::AcqRel);
            Ok(())
        })
    }

    fn shutdown_runtime(
        &self,
        _thread_id: ThreadId,
        _mode: RuntimeShutdown,
    ) -> NodeHostFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn detach_observer(&self, _thread_id: ThreadId) -> NodeHostFuture<'_, ()> {
        Box::pin(async move {
            self.detaches.fetch_add(1, Ordering::AcqRel);
            Ok(())
        })
    }

    fn archive(&self, _thread_id: ThreadId, _archived: bool) -> NodeHostFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn delete(&self, _confirmation: ConfirmedHistoryDeletion) -> NodeHostFuture<'_, ()> {
        Box::pin(async move {
            self.deletes.fetch_add(1, Ordering::AcqRel);
            Ok(())
        })
    }
}

fn binding_key(binding: &WorkflowNodeBinding) -> String {
    format!("{}/{}", binding.run_id, binding.node_id)
}

pub async fn runtime() -> (tempfile::TempDir, Arc<StateRuntime>) {
    let home = tempfile::tempdir().expect("temporary Codex home");
    let runtime = StateRuntime::init(home.path().to_path_buf(), "test-provider".to_string())
        .await
        .expect("initialize state runtime");
    (home, runtime)
}

pub async fn create_run(
    runtime: &StateRuntime,
    run_id: WorkflowRunId,
    definition_name: &str,
    definition_version: &str,
    state_schema_version: u32,
    state: Value,
) {
    runtime
        .workflows()
        .create_run(WorkflowRunCreate {
            run_id: run_id.to_string(),
            definition_name: definition_name.to_string(),
            definition_version: definition_version.to_string(),
            state_schema_version,
            state,
            arguments: json!({}),
            non_interactive: false,
            detached: false,
            concurrency: None,
            created_at_ms: 100,
        })
        .await
        .expect("create workflow run");
}

pub fn service(
    runtime: &StateRuntime,
    host: Arc<TestHost>,
    registry: Arc<WorkflowRegistry>,
) -> WorkflowService {
    let slot = WorkflowNodeHostSlot::new();
    slot.bind(host).expect("bind test host");
    WorkflowService::new_with_registry(runtime.workflows().clone(), slot, registry)
}
