use codex_protocol::ThreadId;
use codex_state::WorkflowEffectPlan;
use codex_state::WorkflowEffectState;
use codex_state::WorkflowEffectUpdate;
use codex_state::WorkflowNodeCreate;
use codex_state::WorkflowNodeRecord;
use codex_state::WorkflowNodeStatus;
use serde_json::json;

use crate::DependencyPolicy;
use crate::EffectKey;
use crate::NodeId;
use crate::NodeSpec;
use crate::WorkflowNodeBinding;
use crate::WorkflowRunId;

use super::FailurePoint;
use super::MaterializeNodeRequest;
use super::NodeError;
use super::NodeHandle;
use super::WorkflowService;
use super::service::now_ms;

/// Typed node effects scoped to one workflow run.
pub struct NodeClient<'a> {
    pub(super) service: &'a WorkflowService,
    pub(super) run_id: WorkflowRunId,
}

impl NodeClient<'_> {
    pub async fn ensure(&self, effect: EffectKey, spec: NodeSpec) -> Result<NodeHandle, NodeError> {
        let now = now_ms()?;
        self.service
            .failure_injector()
            .checkpoint(FailurePoint::BeforeDbWrite)?;
        let request = serde_json::to_value(&spec)?;
        let effect_outcome = self
            .service
            .store()
            .plan_effect(WorkflowEffectPlan {
                run_id: self.run_id.to_string(),
                effect_key: effect.to_string(),
                kind: "node.ensure".to_string(),
                request,
                created_at_ms: now,
            })
            .await?;
        self.service
            .failure_injector()
            .checkpoint(FailurePoint::AfterDbWrite)?;
        let effect_record = match effect_outcome {
            codex_state::WorkflowEffectPlanOutcome::Planned(record)
            | codex_state::WorkflowEffectPlanOutcome::Existing(record) => record,
        };
        if effect_record.state == WorkflowEffectState::Applied {
            let node_id = effect_record
                .response
                .as_ref()
                .and_then(|response| response.get("node_id"))
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| NodeError::InvalidState("node effect has no node_id".to_string()))?;
            return self.get(NodeId::parse(node_id)?).await;
        }

        let (node, spec) = match self
            .service
            .store()
            .list_nodes(&self.run_id.to_string())
            .await?
            .into_iter()
            .find(|node| node.node_key == spec.key().as_str())
        {
            Some(node) => {
                let persisted_spec = serde_json::from_value(node.spec.clone())?;
                (node, persisted_spec)
            }
            None => {
                let resolved_spec = self
                    .service
                    .inner
                    .node_host
                    .get()?
                    .resolve_node_spec(spec)
                    .await?;
                let node_id = NodeId::new();
                let node = self
                    .service
                    .store()
                    .create_node(
                        &self.run_id.to_string(),
                        WorkflowNodeCreate {
                            node_id: node_id.to_string(),
                            node_key: resolved_spec.key().to_string(),
                            spec: serde_json::to_value(&resolved_spec)?,
                            status: WorkflowNodeStatus::Ready,
                            failure_policy: failure_policy_name(resolved_spec.failure_policy())
                                .to_string(),
                            created_at_ms: now,
                        },
                        &[],
                    )
                    .await?;
                (node, resolved_spec)
            }
        };
        let node = self.materialize(node, spec.clone(), now).await?;
        let updated = self
            .service
            .store()
            .update_effect(WorkflowEffectUpdate {
                run_id: self.run_id.to_string(),
                effect_key: effect.to_string(),
                expected_state: WorkflowEffectState::Planned,
                state: WorkflowEffectState::Applied,
                response: Some(json!({
                    "node_id": node.node_id,
                    "thread_id": node.thread_id,
                })),
                error_code: None,
                updated_at_ms: now,
            })
            .await?;
        if !updated {
            let reconciled = self
                .service
                .store()
                .read_effect(&self.run_id.to_string(), &effect.to_string())
                .await?
                .is_some_and(|record| record.state == WorkflowEffectState::Applied);
            if !reconciled {
                return Err(NodeError::InvalidState(
                    "node ensure effect lost its planned state".to_string(),
                ));
            }
        }
        self.handle_from_record(node, spec)
    }

    pub async fn get(&self, id: NodeId) -> Result<NodeHandle, NodeError> {
        let node = self
            .service
            .store()
            .read_node(&id.to_string())
            .await?
            .ok_or(NodeError::NotFound)?;
        if node.run_id != self.run_id.to_string() {
            return Err(NodeError::NotFound);
        }
        let spec = serde_json::from_value(node.spec.clone())?;
        self.handle_from_record(node, spec)
    }

    pub async fn add_dependency(
        &self,
        effect: EffectKey,
        upstream: NodeId,
        downstream: NodeId,
        policy: DependencyPolicy,
    ) -> Result<(), NodeError> {
        let now = now_ms()?;
        let request = json!({
            "upstream": upstream,
            "downstream": downstream,
            "policy": policy,
        });
        let planned = self
            .service
            .store()
            .plan_effect(WorkflowEffectPlan {
                run_id: self.run_id.to_string(),
                effect_key: effect.to_string(),
                kind: "node.add_dependency".to_string(),
                request,
                created_at_ms: now,
            })
            .await?;
        let record = match planned {
            codex_state::WorkflowEffectPlanOutcome::Planned(record)
            | codex_state::WorkflowEffectPlanOutcome::Existing(record) => record,
        };
        if record.state == WorkflowEffectState::Applied {
            return Ok(());
        }
        let (policy_name, at_least) = dependency_policy_parts(&policy)?;
        let add_result = self
            .service
            .store()
            .add_node_dependency(
                &self.run_id.to_string(),
                &downstream.to_string(),
                &upstream.to_string(),
                policy_name,
                at_least,
                now,
            )
            .await;
        if let Err(codex_state::WorkflowStoreError::DuplicateDependency) = add_result {
            let reconciled = self
                .service
                .store()
                .list_node_dependencies(&self.run_id.to_string())
                .await?
                .into_iter()
                .any(|dependency| {
                    dependency.node_id == downstream.to_string()
                        && dependency.depends_on_node_id == upstream.to_string()
                        && dependency.policy == policy_name
                        && dependency.at_least == at_least
                });
            if !reconciled {
                return Err(codex_state::WorkflowStoreError::DuplicateDependency.into());
            }
        } else {
            add_result?;
        }
        if !self
            .service
            .store()
            .update_effect(WorkflowEffectUpdate {
                run_id: self.run_id.to_string(),
                effect_key: effect.to_string(),
                expected_state: WorkflowEffectState::Planned,
                state: WorkflowEffectState::Applied,
                response: Some(json!({})),
                error_code: None,
                updated_at_ms: now,
            })
            .await?
        {
            return Err(NodeError::InvalidState(
                "dependency effect lost its planned state".to_string(),
            ));
        }
        Ok(())
    }

    async fn materialize(
        &self,
        node: WorkflowNodeRecord,
        spec: NodeSpec,
        now: i64,
    ) -> Result<WorkflowNodeRecord, NodeError> {
        if node.thread_id.is_some() {
            return Ok(node);
        }
        let node_id = NodeId::parse(&node.node_id)?;
        let binding = WorkflowNodeBinding {
            run_id: self.run_id,
            node_id,
        };
        self.service
            .failure_injector()
            .checkpoint(FailurePoint::BeforeThreadCreate)?;
        let host = self.service.inner.node_host.get()?;
        let existing = host.find_materialized_nodes(binding.clone()).await?;
        let materialized = match existing.as_slice() {
            [] => {
                host.materialize_node(MaterializeNodeRequest { binding, spec })
                    .await?
            }
            [thread_id] => super::MaterializedNode {
                thread_id: *thread_id,
            },
            _ => {
                return Err(NodeError::InvalidState(
                    "multiple persisted threads match an unbound workflow node".to_string(),
                ));
            }
        };
        self.service
            .failure_injector()
            .checkpoint(FailurePoint::AfterThreadCreate)?;
        self.service
            .store()
            .bind_node_thread(
                &node.node_id,
                node.row_version,
                &materialized.thread_id.to_string(),
                now,
            )
            .await
            .map_err(Into::into)
    }

    fn handle_from_record(
        &self,
        node: WorkflowNodeRecord,
        spec: NodeSpec,
    ) -> Result<NodeHandle, NodeError> {
        let thread_id = node
            .thread_id
            .as_deref()
            .ok_or_else(|| NodeError::InvalidState("node has no materialized thread".to_string()))
            .and_then(|value| {
                ThreadId::from_string(value)
                    .map_err(|error| NodeError::InvalidThreadId(error.to_string()))
            })?;
        Ok(NodeHandle {
            service: self.service.clone(),
            binding: WorkflowNodeBinding {
                run_id: self.run_id,
                node_id: NodeId::parse(&node.node_id)?,
            },
            thread_id,
            spec,
        })
    }
}

fn failure_policy_name(policy: crate::FailurePolicy) -> &'static str {
    match policy {
        crate::FailurePolicy::FailFast => "fail_fast",
        crate::FailurePolicy::ContinueIndependentBranches => "continue_independent",
        crate::FailurePolicy::SkipDependents => "skip_dependents",
    }
}

fn dependency_policy_parts(
    policy: &DependencyPolicy,
) -> Result<(&'static str, Option<u32>), NodeError> {
    match policy {
        DependencyPolicy::AllSucceeded => Ok(("all_succeeded", None)),
        DependencyPolicy::AllTerminal => Ok(("all_terminal", None)),
        DependencyPolicy::AtLeast(count) => Ok((
            "at_least",
            Some(u32::try_from(count.get()).map_err(|_| {
                NodeError::InvalidState("dependency threshold exceeds u32".to_string())
            })?),
        )),
    }
}
