use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use codex_state::WorkflowEffectState;
use codex_state::WorkflowEffectUpdate;
use codex_state::WorkflowNodeAttemptStatus;
use codex_state::WorkflowNodeAttemptTransition;
use codex_state::WorkflowNodeStatus;
use codex_state::WorkflowNodeTransition;
use codex_state::WorkflowRunStatus;
use serde_json::json;

use crate::NodeId;
use crate::NodeTurnStatus;
use crate::WorkflowName;
use crate::WorkflowNodeBinding;
use crate::WorkflowRegistry;
use crate::WorkflowRunId;
use crate::WorkflowVersion;
use crate::WakeCondition;
use crate::HumanInteractionOutcome;
use crate::integrations::fornax::FornaxWorkflowClient;

use super::DriveOutcome;
use super::RecoverTurnRequest;
use super::RecoveredTurnState;
use super::WorkflowDriver;
use super::WorkflowService;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    pub inspected_runs: usize,
    pub repaired_thread_bindings: usize,
    pub reconciled_terminal_attempts: usize,
    pub interrupted_attempts: usize,
    pub needs_operator_runs: usize,
    pub driven_runs: usize,
}

/// Expired-lease startup reconciliation for normal workflow node threads.
pub struct WorkflowRecovery {
    service: WorkflowService,
    registry: Arc<WorkflowRegistry>,
    owner: String,
    lease_duration_ms: i64,
    fornax: Option<Arc<FornaxWorkflowClient>>,
    lark: Option<Arc<super::LarkInteractionService>>,
}

impl WorkflowRecovery {
    pub fn new(
        service: WorkflowService,
        registry: Arc<WorkflowRegistry>,
        owner: impl Into<String>,
        lease_duration_ms: i64,
    ) -> Self {
        Self {
            service,
            registry,
            owner: owner.into(),
            lease_duration_ms: lease_duration_ms.max(1),
            fornax: None,
            lark: None,
        }
    }

    pub fn with_fornax_client(mut self, client: Arc<FornaxWorkflowClient>) -> Self {
        self.fornax = Some(client);
        self
    }

    pub fn with_lark_service(mut self, service: Arc<super::LarkInteractionService>) -> Self {
        self.lark = Some(service);
        self
    }

    pub async fn recover_nonterminal_runs(
        &self,
        now_ms: i64,
    ) -> Result<RecoveryReport, WorkflowRecoveryError> {
        let store = self.service.store();
        let runs = store.list_recoverable_runs(now_ms).await?;
        let mut report = RecoveryReport::default();
        for run in runs {
            report.inspected_runs += 1;
            let Some(lease) = store
                .acquire_lease(&run.run_id, &self.owner, now_ms, self.lease_duration_ms)
                .await?
            else {
                continue;
            };
            let run_id = WorkflowRunId::parse(&run.run_id)?;
            let outcome = self.reconcile_run(&run, now_ms, &mut report).await;
            store.release_lease(&lease, now_ms).await?;
            match outcome? {
                ReconcileOutcome::ReadyToDrive => {
                    let driver = WorkflowDriver::new_with_signals(
                        store.clone(),
                        Arc::clone(&self.registry),
                        self.owner.clone(),
                        self.lease_duration_ms,
                        self.service.cancellation_signals(),
                        self.fornax.as_ref().map(Arc::clone),
                        self.lark.as_ref().map(Arc::clone),
                    );
                    if !matches!(
                        driver.step_once(run_id, now_ms).await?,
                        DriveOutcome::Busy | DriveOutcome::NeedsOperator
                    ) {
                        report.driven_runs += 1;
                    }
                }
                ReconcileOutcome::Waiting | ReconcileOutcome::NeedsOperator => {}
            }
        }
        Ok(report)
    }

    async fn reconcile_run(
        &self,
        run: &codex_state::WorkflowRunRecord,
        now_ms: i64,
        report: &mut RecoveryReport,
    ) -> Result<ReconcileOutcome, WorkflowRecoveryError> {
        if let Some(lark) = &self.lark
            && let Some(wake) = &run.wake
            && let Ok(WakeCondition::HumanInteraction(interaction_id)) =
                serde_json::from_value::<WakeCondition>(wake.clone())
            && self
                .service
                .store()
                .read_lark_interaction(&interaction_id.to_string())
                .await?
                .is_some()
        {
            let outcome = lark
                .reconcile_waiting(interaction_id, now_ms, &AtomicBool::new(false))
                .await?;
            if !matches!(outcome, HumanInteractionOutcome::Waiting { .. }) {
                return Ok(ReconcileOutcome::ReadyToDrive);
            }
        }
        let name = WorkflowName::new(run.definition_name.clone())?;
        let version = WorkflowVersion::parse(&run.definition_version)?;
        if self.registry.resolve(&name, Some(&version)).is_err() {
            self.needs_operator(run, None, "definition_unavailable", now_ms)
                .await?;
            report.needs_operator_runs += 1;
            return Ok(ReconcileOutcome::NeedsOperator);
        }
        if self
            .service
            .store()
            .list_effects(&run.run_id)
            .await?
            .iter()
            .any(|effect| effect.state == WorkflowEffectState::Ambiguous)
        {
            self.needs_operator(run, None, "ambiguous_effect", now_ms)
                .await?;
            report.needs_operator_runs += 1;
            return Ok(ReconcileOutcome::NeedsOperator);
        }
        let host = self.service.node_host_slot().get()?;
        let effects = self.service.store().list_effects(&run.run_id).await?;
        let mut nodes = self.service.store().list_nodes(&run.run_id).await?;
        for node in &mut nodes {
            let binding = WorkflowNodeBinding {
                run_id: WorkflowRunId::parse(&run.run_id)?,
                node_id: NodeId::parse(&node.node_id)?,
            };
            let matches = host.find_materialized_nodes(binding).await?;
            let known_threads = self
                .service
                .store()
                .list_node_threads(&node.node_id)
                .await?;
            let unknown = matches
                .iter()
                .copied()
                .filter(|thread_id| {
                    !known_threads
                        .iter()
                        .any(|known| known.thread_id == thread_id.to_string())
                })
                .collect::<Vec<_>>();
            if node.thread_id.is_none() {
                match matches.as_slice() {
                    [] => {}
                    [thread_id] => {
                        *node = self
                            .service
                            .store()
                            .bind_node_thread(
                                &node.node_id,
                                node.row_version,
                                &thread_id.to_string(),
                                now_ms,
                            )
                            .await?;
                        report.repaired_thread_bindings += 1;
                    }
                    _ => {
                        self.needs_operator(
                            run,
                            Some(&node.node_id),
                            "duplicate_node_threads",
                            now_ms,
                        )
                        .await?;
                        report.needs_operator_runs += 1;
                        return Ok(ReconcileOutcome::NeedsOperator);
                    }
                }
            } else if node.thread_id.as_deref().is_some_and(|primary| {
                !matches
                    .iter()
                    .any(|thread_id| thread_id.to_string() == primary)
            }) {
                self.needs_operator(run, Some(&node.node_id), "orphan_thread_mapping", now_ms)
                    .await?;
                report.needs_operator_runs += 1;
                return Ok(ReconcileOutcome::NeedsOperator);
            } else if unknown.len() == 1 {
                let planned_fresh = effects.iter().find(|effect| {
                    effect.kind == "node.retry.thread"
                        && effect.state == WorkflowEffectState::Planned
                        && effect
                            .request
                            .get("node_id")
                            .and_then(serde_json::Value::as_str)
                            == Some(node.node_id.as_str())
                });
                let Some(effect) = planned_fresh else {
                    self.needs_operator(run, Some(&node.node_id), "orphan_node_thread", now_ms)
                        .await?;
                    report.needs_operator_runs += 1;
                    return Ok(ReconcileOutcome::NeedsOperator);
                };
                self.service
                    .store()
                    .append_node_thread(&node.node_id, &unknown[0].to_string(), now_ms)
                    .await?;
                if !self
                    .service
                    .store()
                    .update_effect(WorkflowEffectUpdate {
                        run_id: run.run_id.clone(),
                        effect_key: effect.effect_key.clone(),
                        expected_state: WorkflowEffectState::Planned,
                        state: WorkflowEffectState::Applied,
                        response: Some(json!({ "thread_id": unknown[0] })),
                        error_code: None,
                        updated_at_ms: now_ms,
                    })
                    .await?
                {
                    return Err(codex_state::WorkflowStoreError::StaleWrite.into());
                }
                report.repaired_thread_bindings += 1;
            } else if unknown.len() > 1 {
                self.needs_operator(run, Some(&node.node_id), "duplicate_node_threads", now_ms)
                    .await?;
                report.needs_operator_runs += 1;
                return Ok(ReconcileOutcome::NeedsOperator);
            }
            for attempt in self
                .service
                .store()
                .list_node_attempts(&node.node_id)
                .await?
                .into_iter()
                .filter(|attempt| !attempt.status.is_terminal())
            {
                let Some(thread_id) = attempt
                    .thread_id
                    .as_deref()
                    .or(node.thread_id.as_deref())
                    .map(codex_protocol::ThreadId::from_string)
                    .transpose()
                    .map_err(|error| WorkflowRecoveryError::InvalidThread(error.to_string()))?
                else {
                    continue;
                };
                let recovered = host
                    .recover_prepared_turn(RecoverTurnRequest {
                        thread_id,
                        submission_id: attempt.submission_id.clone(),
                        input_hash: decode_hash(&attempt.input_hash)?,
                    })
                    .await?;
                match recovered {
                    RecoveredTurnState::NoBoundary => {}
                    RecoveredTurnState::Conflict => {
                        self.needs_operator(
                            run,
                            Some(&node.node_id),
                            "prepared_turn_conflict",
                            now_ms,
                        )
                        .await?;
                        report.needs_operator_runs += 1;
                        return Ok(ReconcileOutcome::NeedsOperator);
                    }
                    RecoveredTurnState::Unterminated => {
                        self.service
                            .store()
                            .transition_node_attempt(
                                &attempt.attempt_id,
                                WorkflowNodeAttemptTransition {
                                    expected_status: attempt.status,
                                    status: WorkflowNodeAttemptStatus::Interrupted,
                                    turn_id: attempt
                                        .turn_id
                                        .clone()
                                        .or(Some(attempt.submission_id.clone())),
                                    error_code: Some("process_restart".to_string()),
                                    updated_at_ms: now_ms,
                                },
                            )
                            .await?;
                        *node = self
                            .service
                            .store()
                            .transition_node(
                                &node.node_id,
                                node.row_version,
                                WorkflowNodeTransition {
                                    status: WorkflowNodeStatus::Waiting,
                                    retry_at_ms: Some(now_ms),
                                    event_kind: "node.retry_scheduled".to_string(),
                                    event_metadata: json!({"reason": "process_restart"}),
                                    updated_at_ms: now_ms,
                                },
                            )
                            .await?;
                        report.interrupted_attempts += 1;
                    }
                    RecoveredTurnState::Terminal(result) => {
                        let status = match result.status {
                            NodeTurnStatus::Completed => WorkflowNodeAttemptStatus::Succeeded,
                            NodeTurnStatus::Interrupted => WorkflowNodeAttemptStatus::Interrupted,
                            NodeTurnStatus::Failed => WorkflowNodeAttemptStatus::Failed,
                        };
                        self.service
                            .store()
                            .transition_node_attempt(
                                &attempt.attempt_id,
                                WorkflowNodeAttemptTransition {
                                    expected_status: attempt.status,
                                    status,
                                    turn_id: Some(result.turn_id),
                                    error_code: result.error.map(|_| "turn_failed".to_string()),
                                    updated_at_ms: now_ms,
                                },
                            )
                            .await?;
                        report.reconciled_terminal_attempts += 1;
                    }
                }
            }
        }
        if run.status == WorkflowRunStatus::Waiting && !wake_is_ready(run, &nodes, now_ms)? {
            Ok(ReconcileOutcome::Waiting)
        } else {
            Ok(ReconcileOutcome::ReadyToDrive)
        }
    }

    async fn needs_operator(
        &self,
        run: &codex_state::WorkflowRunRecord,
        node_id: Option<&str>,
        error_code: &str,
        now_ms: i64,
    ) -> Result<(), WorkflowRecoveryError> {
        self.service
            .store()
            .mark_run_needs_operator(
                &run.run_id,
                node_id,
                error_code,
                json!({"error_code": error_code}),
                now_ms,
            )
            .await?;
        Ok(())
    }
}

fn wake_is_ready(
    run: &codex_state::WorkflowRunRecord,
    nodes: &[codex_state::WorkflowNodeRecord],
    now_ms: i64,
) -> Result<bool, WorkflowRecoveryError> {
    let Some(wake) = run.wake.clone() else {
        return Ok(true);
    };
    let wake: crate::WakeCondition = serde_json::from_value(wake)?;
    Ok(match wake {
        crate::WakeCondition::RetryAt(instant) => {
            let retry_at = instant
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| WorkflowRecoveryError::InvalidWake)?
                .as_millis();
            retry_at <= u128::try_from(now_ms).map_err(|_| WorkflowRecoveryError::InvalidWake)?
        }
        crate::WakeCondition::Cancellation => run.cancellation_requested_at_ms.is_some(),
        crate::WakeCondition::NodeTerminal(node_id) => nodes
            .iter()
            .find(|node| node.node_id == node_id.to_string())
            .is_some_and(|node| super::graph::is_terminal(node.status)),
        crate::WakeCondition::NodesTerminal(node_ids) => node_ids.iter().all(|node_id| {
            nodes
                .iter()
                .find(|node| node.node_id == node_id.to_string())
                .is_some_and(|node| super::graph::is_terminal(node.status))
        }),
        crate::WakeCondition::HumanInteraction(_) => false,
    })
}

fn decode_hash(value: &str) -> Result<[u8; 32], WorkflowRecoveryError> {
    if value.len() != 64 {
        return Err(WorkflowRecoveryError::InvalidHash);
    }
    let mut bytes = [0; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| WorkflowRecoveryError::InvalidHash)?;
    }
    Ok(bytes)
}

enum ReconcileOutcome {
    ReadyToDrive,
    Waiting,
    NeedsOperator,
}

#[derive(Debug, thiserror::Error)]
pub enum WorkflowRecoveryError {
    #[error(transparent)]
    Store(#[from] codex_state::WorkflowStoreError),
    #[error(transparent)]
    Identifier(#[from] crate::IdentifierError),
    #[error(transparent)]
    Registry(#[from] crate::RegistryError),
    #[error(transparent)]
    Driver(#[from] super::WorkflowDriverError),
    #[error(transparent)]
    Host(#[from] super::NodeHostError),
    #[error(transparent)]
    Serialization(#[from] serde_json::Error),
    #[error(transparent)]
    Lark(#[from] super::LarkInteractionError),
    #[error("workflow attempt input hash is invalid")]
    InvalidHash,
    #[error("workflow thread id is invalid: {0}")]
    InvalidThread(String),
    #[error("workflow wake timestamp is invalid")]
    InvalidWake,
}
