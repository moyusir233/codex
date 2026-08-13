use std::sync::Arc;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use codex_goal_extension::GoalService;
use codex_state::StateRuntime;
use codex_state::WorkflowEffectPlan;
use codex_state::WorkflowEffectPlanOutcome;
use codex_state::WorkflowEffectState;
use codex_state::WorkflowEffectUpdate;
use codex_state::WorkflowStore;
use codex_state::WorkflowStoreError;
use codex_workflow_extension::WorkflowGoalCapability;
use codex_workflow_extension::WorkflowGoalEnsureRequest;
use codex_workflow_extension::WorkflowGoalError;
use codex_workflow_extension::WorkflowGoalFuture;
use codex_workflow_extension::WorkflowGoalSnapshot;
use codex_workflow_extension::WorkflowGoalStatus;
use codex_workflow_extension::WorkflowRunId;
use sha2::Digest;
use sha2::Sha256;

pub(crate) struct AppServerWorkflowGoalAdapter {
    state: Arc<StateRuntime>,
    store: WorkflowStore,
    goals: Arc<GoalService>,
}

impl AppServerWorkflowGoalAdapter {
    pub(crate) fn new(
        state: Arc<StateRuntime>,
        store: WorkflowStore,
        goals: Arc<GoalService>,
    ) -> Self {
        Self {
            state,
            store,
            goals,
        }
    }

    async fn ensure_inner(
        &self,
        run_id: WorkflowRunId,
        request: &WorkflowGoalEnsureRequest,
    ) -> Result<WorkflowGoalSnapshot, WorkflowGoalError> {
        self.validate_thread_binding(run_id, request.thread_id)
            .await?;
        let objective = request.objective.trim();
        if objective.is_empty() {
            return Err(WorkflowGoalError::InvalidRequest(
                "objective must not be empty".to_string(),
            ));
        }
        let objective_sha256 = digest(objective);
        let now_ms = now_ms()?;
        let planned = self
            .store
            .plan_effect(WorkflowEffectPlan {
                run_id: run_id.to_string(),
                effect_key: request.effect_key.to_string(),
                kind: "goal.ensure".to_string(),
                request: serde_json::json!({
                    "thread_id": request.thread_id.to_string(),
                    "objective_sha256": objective_sha256,
                    "token_budget": request.token_budget,
                }),
                created_at_ms: now_ms,
            })
            .await
            .map_err(map_store)?;
        let effect = match planned {
            WorkflowEffectPlanOutcome::Planned(effect)
            | WorkflowEffectPlanOutcome::Existing(effect) => effect,
        };
        if effect.state == WorkflowEffectState::Applied {
            return serde_json::from_value(effect.response.ok_or_else(|| {
                WorkflowGoalError::Internal("applied goal effect has no response".to_string())
            })?)
            .map_err(|error| WorkflowGoalError::Internal(error.to_string()));
        }
        if effect.state == WorkflowEffectState::Planned {
            let updated = self
                .store
                .update_effect(WorkflowEffectUpdate {
                    run_id: run_id.to_string(),
                    effect_key: request.effect_key.to_string(),
                    expected_state: WorkflowEffectState::Planned,
                    state: WorkflowEffectState::Dispatched,
                    response: None,
                    error_code: None,
                    updated_at_ms: now_ms,
                })
                .await
                .map_err(map_store)?;
            if !updated {
                return Err(WorkflowGoalError::Internal(
                    "goal effect dispatch lost ownership".to_string(),
                ));
            }
        }

        let existing = self
            .state
            .thread_goals()
            .get_thread_goal(request.thread_id)
            .await
            .map_err(|error| WorkflowGoalError::Internal(error.to_string()))?;
        let should_create = if let Some(goal) = existing.as_ref() {
            if digest(goal.objective.trim()) != objective_sha256 {
                if goal.status != codex_state::ThreadGoalStatus::Complete {
                    return Err(WorkflowGoalError::UnrelatedUnfinished);
                }
                true
            } else {
                if goal.token_budget != request.token_budget {
                    return Err(WorkflowGoalError::ObjectiveMismatch);
                }
                false
            }
        } else {
            true
        };
        if should_create {
            let outcome = self
                .goals
                .create_thread_goal(
                    &self.state,
                    request.thread_id,
                    objective,
                    request.token_budget,
                )
                .await
                .map_err(|error| WorkflowGoalError::InvalidRequest(error.to_string()))?;
            outcome.apply_runtime_effects(&self.goals).await;
        }
        let goal = self
            .state
            .thread_goals()
            .get_thread_goal(request.thread_id)
            .await
            .map_err(|error| WorkflowGoalError::Internal(error.to_string()))?
            .ok_or_else(|| {
                WorkflowGoalError::Internal("goal disappeared after ensure".to_string())
            })?;
        if digest(goal.objective.trim()) != objective_sha256 {
            return Err(WorkflowGoalError::ObjectiveMismatch);
        }
        let snapshot = snapshot(goal);
        let response = serde_json::to_value(&snapshot)
            .map_err(|error| WorkflowGoalError::Internal(error.to_string()))?;
        let updated = self
            .store
            .update_effect(WorkflowEffectUpdate {
                run_id: run_id.to_string(),
                effect_key: request.effect_key.to_string(),
                expected_state: WorkflowEffectState::Dispatched,
                state: WorkflowEffectState::Applied,
                response: Some(response),
                error_code: None,
                updated_at_ms: now_ms,
            })
            .await
            .map_err(map_store)?;
        if !updated {
            let effect = self
                .store
                .read_effect(&run_id.to_string(), request.effect_key.as_str())
                .await
                .map_err(map_store)?
                .ok_or_else(|| {
                    WorkflowGoalError::Internal("goal effect disappeared".to_string())
                })?;
            if effect.state != WorkflowEffectState::Applied {
                return Err(WorkflowGoalError::Internal(
                    "goal effect completion lost ownership".to_string(),
                ));
            }
        }
        Ok(snapshot)
    }

    async fn validate_thread_binding(
        &self,
        run_id: WorkflowRunId,
        thread_id: codex_protocol::ThreadId,
    ) -> Result<(), WorkflowGoalError> {
        let node = self
            .store
            .read_node_by_thread_id(&thread_id.to_string())
            .await
            .map_err(map_store)?;
        if node
            .as_ref()
            .is_some_and(|node| node.run_id == run_id.to_string())
        {
            Ok(())
        } else {
            Err(WorkflowGoalError::InvalidRequest(
                "goal thread is not bound to this workflow run".to_string(),
            ))
        }
    }
}

impl WorkflowGoalCapability for AppServerWorkflowGoalAdapter {
    fn ensure<'a>(
        &'a self,
        run_id: WorkflowRunId,
        request: &'a WorkflowGoalEnsureRequest,
    ) -> WorkflowGoalFuture<'a, WorkflowGoalSnapshot> {
        Box::pin(async move { self.ensure_inner(run_id, request).await })
    }

    fn get<'a>(
        &'a self,
        run_id: WorkflowRunId,
        thread_id: codex_protocol::ThreadId,
    ) -> WorkflowGoalFuture<'a, Option<WorkflowGoalSnapshot>> {
        Box::pin(async move {
            self.validate_thread_binding(run_id, thread_id).await?;
            self.state
                .thread_goals()
                .get_thread_goal(thread_id)
                .await
                .map(|goal| goal.map(snapshot))
                .map_err(|error| WorkflowGoalError::Internal(error.to_string()))
        })
    }
}

fn snapshot(goal: codex_state::ThreadGoal) -> WorkflowGoalSnapshot {
    WorkflowGoalSnapshot {
        goal_id: goal.goal_id,
        thread_id: goal.thread_id,
        objective_sha256: digest(goal.objective.trim()),
        status: match goal.status {
            codex_state::ThreadGoalStatus::Active => WorkflowGoalStatus::Active,
            codex_state::ThreadGoalStatus::Paused => WorkflowGoalStatus::Paused,
            codex_state::ThreadGoalStatus::Blocked => WorkflowGoalStatus::Blocked,
            codex_state::ThreadGoalStatus::UsageLimited => WorkflowGoalStatus::UsageLimited,
            codex_state::ThreadGoalStatus::BudgetLimited => WorkflowGoalStatus::BudgetLimited,
            codex_state::ThreadGoalStatus::Complete => WorkflowGoalStatus::Complete,
        },
        token_budget: goal.token_budget,
        tokens_used: goal.tokens_used,
        time_used_seconds: goal.time_used_seconds,
    }
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn map_store(error: WorkflowStoreError) -> WorkflowGoalError {
    match error {
        WorkflowStoreError::EffectConflict => WorkflowGoalError::EffectConflict,
        error => WorkflowGoalError::Internal(error.to_string()),
    }
}

fn now_ms() -> Result<i64, WorkflowGoalError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| WorkflowGoalError::Internal(error.to_string()))?
        .as_millis();
    i64::try_from(millis).map_err(|error| WorkflowGoalError::Internal(error.to_string()))
}

#[cfg(test)]
mod tests {
    use codex_protocol::ThreadId;
    use codex_protocol::protocol::SessionSource;
    use codex_state::GoalUpdate;
    use codex_state::ThreadGoalStatus;
    use codex_state::ThreadMetadataBuilder;
    use codex_state::WorkflowNodeCreate;
    use codex_state::WorkflowNodeStatus;
    use codex_state::WorkflowRunCreate;
    use codex_workflow_extension::ArtifactId;
    use codex_workflow_extension::EffectKey;
    use codex_workflow_extension::WorkflowGoalDeliveryEvidence;
    use codex_workflow_extension::WorkflowGoalEnsureRequest;
    use codex_workflow_extension::WorkflowGoalError;
    use codex_workflow_extension::WorkflowGoalStatus;
    use codex_workflow_extension::WorkflowRunId;

    use super::*;

    #[tokio::test]
    async fn workflow_goal_adapter_is_restart_idempotent_and_evidence_gated() -> anyhow::Result<()>
    {
        let home = tempfile::tempdir()?;
        let state = Arc::new(
            StateRuntime::init(home.path().to_path_buf(), "test-provider".to_string()).await?,
        );
        let thread_id = ThreadId::from_string("11111111-1111-4111-8111-111111111111")?;
        seed_thread(&state, thread_id).await?;
        let run_id = WorkflowRunId::new();
        create_run_and_bind(&state, run_id, thread_id).await?;
        let goals = Arc::new(GoalService::new());
        let request = WorkflowGoalEnsureRequest {
            effect_key: EffectKey::new("stage4.goal.ensure")?,
            thread_id,
            objective: "execute the approved coding plan".to_string(),
            token_budget: None,
        };
        let first = AppServerWorkflowGoalAdapter::new(
            Arc::clone(&state),
            state.workflows().clone(),
            Arc::clone(&goals),
        )
        .ensure(run_id, &request)
        .await?;
        assert_eq!(first.status, WorkflowGoalStatus::Active);

        let restarted =
            AppServerWorkflowGoalAdapter::new(Arc::clone(&state), state.workflows().clone(), goals);
        let replay = restarted.ensure(run_id, &request).await?;
        assert_eq!(first.goal_id, replay.goal_id);
        assert_eq!(first.objective_sha256, replay.objective_sha256);

        for (state_status, expected) in [
            (ThreadGoalStatus::Paused, WorkflowGoalStatus::Paused),
            (ThreadGoalStatus::Blocked, WorkflowGoalStatus::Blocked),
            (
                ThreadGoalStatus::UsageLimited,
                WorkflowGoalStatus::UsageLimited,
            ),
            (
                ThreadGoalStatus::BudgetLimited,
                WorkflowGoalStatus::BudgetLimited,
            ),
            (ThreadGoalStatus::Complete, WorkflowGoalStatus::Complete),
        ] {
            state
                .thread_goals()
                .update_thread_goal(
                    thread_id,
                    GoalUpdate {
                        objective: None,
                        status: Some(state_status),
                        token_budget: None,
                        expected_goal_id: Some(first.goal_id.clone()),
                    },
                )
                .await?
                .expect("goal update");
            assert_eq!(
                restarted
                    .get(run_id, thread_id)
                    .await?
                    .expect("goal")
                    .status,
                expected
            );
        }
        let complete = restarted
            .get(run_id, thread_id)
            .await?
            .expect("complete goal");
        let evidence = WorkflowGoalDeliveryEvidence {
            goal_id: complete.goal_id.clone(),
            objective_sha256: complete.objective_sha256.clone(),
            validation_artifact_id: ArtifactId::new(),
            validation_sha256: "a".repeat(64),
            acceptance_artifact_id: ArtifactId::new(),
            acceptance_sha256: "b".repeat(64),
        };
        assert!(complete.permits_delivery(&evidence));
        let mut wrong = evidence.clone();
        wrong.goal_id = "unrelated".to_string();
        assert!(!complete.permits_delivery(&wrong));

        let replacement = restarted
            .ensure(
                run_id,
                &WorkflowGoalEnsureRequest {
                    effect_key: EffectKey::new("stage4.goal.replacement")?,
                    thread_id,
                    objective: "execute a later approved coding plan".to_string(),
                    token_budget: None,
                },
            )
            .await?;
        assert_ne!(replacement.goal_id, complete.goal_id);
        assert_eq!(replacement.status, WorkflowGoalStatus::Active);
        Ok(())
    }

    #[tokio::test]
    async fn workflow_goal_adapter_rejects_unrelated_unfinished_goal() -> anyhow::Result<()> {
        let home = tempfile::tempdir()?;
        let state = Arc::new(
            StateRuntime::init(home.path().to_path_buf(), "test-provider".to_string()).await?,
        );
        let thread_id = ThreadId::from_string("22222222-2222-4222-8222-222222222222")?;
        seed_thread(&state, thread_id).await?;
        state
            .thread_goals()
            .insert_thread_goal(thread_id, "unrelated", ThreadGoalStatus::Active, None)
            .await?
            .expect("seed unrelated goal");
        let run_id = WorkflowRunId::new();
        create_run_and_bind(&state, run_id, thread_id).await?;
        let adapter = AppServerWorkflowGoalAdapter::new(
            Arc::clone(&state),
            state.workflows().clone(),
            Arc::new(GoalService::new()),
        );
        let error = adapter
            .ensure(
                run_id,
                &WorkflowGoalEnsureRequest {
                    effect_key: EffectKey::new("stage4.goal.ensure")?,
                    thread_id,
                    objective: "expected".to_string(),
                    token_budget: None,
                },
            )
            .await
            .expect_err("unrelated goal must fail closed");
        assert_eq!(error, WorkflowGoalError::UnrelatedUnfinished);
        Ok(())
    }

    #[tokio::test]
    async fn workflow_goal_adapter_recovers_dispatched_goal_after_restart() -> anyhow::Result<()> {
        let home = tempfile::tempdir()?;
        let state = Arc::new(
            StateRuntime::init(home.path().to_path_buf(), "test-provider".to_string()).await?,
        );
        let thread_id = ThreadId::from_string("33333333-3333-4333-8333-333333333333")?;
        seed_thread(&state, thread_id).await?;
        let run_id = WorkflowRunId::new();
        create_run_and_bind(&state, run_id, thread_id).await?;
        let goals = Arc::new(GoalService::new());
        let request = WorkflowGoalEnsureRequest {
            effect_key: EffectKey::new("stage4.goal.ensure")?,
            thread_id,
            objective: "execute the approved coding plan".to_string(),
            token_budget: Some(100_000),
        };
        let store = state.workflows();
        store
            .plan_effect(WorkflowEffectPlan {
                run_id: run_id.to_string(),
                effect_key: request.effect_key.to_string(),
                kind: "goal.ensure".to_string(),
                request: serde_json::json!({
                    "thread_id": thread_id.to_string(),
                    "objective_sha256": digest(&request.objective),
                    "token_budget": request.token_budget,
                }),
                created_at_ms: 4,
            })
            .await?;
        assert!(
            store
                .update_effect(WorkflowEffectUpdate {
                    run_id: run_id.to_string(),
                    effect_key: request.effect_key.to_string(),
                    expected_state: WorkflowEffectState::Planned,
                    state: WorkflowEffectState::Dispatched,
                    response: None,
                    error_code: None,
                    updated_at_ms: 5,
                })
                .await?
        );
        let created = goals
            .create_thread_goal(&state, thread_id, &request.objective, request.token_budget)
            .await?;
        created.apply_runtime_effects(&goals).await;
        let goal_id = state
            .thread_goals()
            .get_thread_goal(thread_id)
            .await?
            .expect("goal created before crash")
            .goal_id;

        let recovered = AppServerWorkflowGoalAdapter::new(Arc::clone(&state), store.clone(), goals)
            .ensure(run_id, &request)
            .await?;
        assert_eq!(recovered.goal_id, goal_id);
        assert_eq!(
            store
                .read_effect(&run_id.to_string(), request.effect_key.as_str())
                .await?
                .expect("effect")
                .state,
            WorkflowEffectState::Applied
        );
        Ok(())
    }

    async fn seed_thread(state: &StateRuntime, thread_id: ThreadId) -> anyhow::Result<()> {
        let builder = ThreadMetadataBuilder::new(
            thread_id,
            state
                .codex_home()
                .join(format!("rollout-{thread_id}.jsonl")),
            chrono::Utc::now(),
            SessionSource::Cli,
        );
        state.upsert_thread(&builder.build("test-provider")).await
    }

    async fn create_run_and_bind(
        state: &StateRuntime,
        run_id: WorkflowRunId,
        thread_id: ThreadId,
    ) -> anyhow::Result<()> {
        let store = state.workflows();
        store
            .create_run(WorkflowRunCreate {
                run_id: run_id.to_string(),
                definition_name: "goal-test".to_string(),
                definition_version: "1.0.0".to_string(),
                state_schema_version: 1,
                state: serde_json::json!({}),
                arguments: serde_json::json!({}),
                non_interactive: true,
                detached: false,
                concurrency: Some(1),
                created_at_ms: 1,
            })
            .await?;
        let node = store
            .create_node(
                &run_id.to_string(),
                WorkflowNodeCreate {
                    node_id: format!("node-{run_id}"),
                    node_key: "stage4".to_string(),
                    spec: serde_json::json!({}),
                    status: WorkflowNodeStatus::Running,
                    failure_policy: "fail_fast".to_string(),
                    created_at_ms: 2,
                },
                &[],
            )
            .await?;
        store
            .bind_node_thread(&node.node_id, node.row_version, &thread_id.to_string(), 3)
            .await?;
        Ok(())
    }
}
