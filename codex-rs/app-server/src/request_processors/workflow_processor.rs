use std::ffi::OsString;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::NodeThreadSubscription;
use codex_app_server_protocol::WorkflowArgumentsInput;
use codex_app_server_protocol::WorkflowArtifactSummary;
use codex_app_server_protocol::WorkflowDefinitionStability;
use codex_app_server_protocol::WorkflowDefinitionSummary;
use codex_app_server_protocol::WorkflowInteractionRespondParams;
use codex_app_server_protocol::WorkflowInteractionRespondResponse;
use codex_app_server_protocol::WorkflowListResponse;
use codex_app_server_protocol::WorkflowNodeStatus;
use codex_app_server_protocol::WorkflowNodeSummary;
use codex_app_server_protocol::WorkflowRun;
use codex_app_server_protocol::WorkflowRunCancelParams;
use codex_app_server_protocol::WorkflowRunCancelResponse;
use codex_app_server_protocol::WorkflowRunListParams;
use codex_app_server_protocol::WorkflowRunListResponse;
use codex_app_server_protocol::WorkflowRunParams;
use codex_app_server_protocol::WorkflowRunReadParams;
use codex_app_server_protocol::WorkflowRunReadResponse;
use codex_app_server_protocol::WorkflowRunResponse;
use codex_app_server_protocol::WorkflowRunResumeParams;
use codex_app_server_protocol::WorkflowRunResumeResponse;
use codex_app_server_protocol::WorkflowRunStatus;
use codex_app_server_protocol::WorkflowRunSubscribeParams;
use codex_app_server_protocol::WorkflowRunSubscribeResponse;
use codex_app_server_protocol::WorkflowRunUnsubscribeParams;
use codex_app_server_protocol::WorkflowRunUnsubscribeResponse;
use codex_core::config::Config;
use codex_features::Feature;
use codex_state::WorkflowEffectPlan;
use codex_state::WorkflowEffectPlanOutcome;
use codex_state::WorkflowEffectState;
use codex_state::WorkflowEffectUpdate;
use codex_state::WorkflowInteractionState;
use codex_state::WorkflowRunCreate;
use codex_state::WorkflowRunRecord;
use codex_workflow_extension::ArtifactClassification;
use codex_workflow_extension::ArtifactId;
use codex_workflow_extension::CancellationReason;
use codex_workflow_extension::DriveOutcome;
use codex_workflow_extension::InteractionId;
use codex_workflow_extension::WorkflowArtifactStore;
use codex_workflow_extension::WorkflowArtifactWrite;
use codex_workflow_extension::WorkflowCancellationController;
use codex_workflow_extension::WorkflowDriver;
use codex_workflow_extension::WorkflowName;
use codex_workflow_extension::WorkflowRunId;
use codex_workflow_extension::WorkflowService;
use codex_workflow_extension::WorkflowStability;
use codex_workflow_extension::WorkflowVersion;
use serde_json::json;

use crate::error_code::INTERNAL_ERROR_CODE;
use crate::error_code::INVALID_PARAMS_ERROR_CODE;
use crate::error_code::invalid_request;
use crate::outgoing_message::ConnectionId;
use crate::workflow_subscriptions::WorkflowSubscriptions;

mod support;
use support::*;

const DEFAULT_PAGE_SIZE: u32 = 50;
const MAX_PAGE_SIZE: u32 = 200;
const DRIVER_STEP_LIMIT: usize = 1_000;
const WORKFLOW_ERROR_FIELD: &str = "workflowError";

#[derive(Clone)]
pub(crate) struct WorkflowRequestProcessor {
    config: Arc<Config>,
    service: Option<Arc<WorkflowService>>,
    subscriptions: Option<WorkflowSubscriptions>,
    artifact_store: Option<WorkflowArtifactStore>,
}

impl WorkflowRequestProcessor {
    pub(crate) fn new(
        config: Arc<Config>,
        service: Option<Arc<WorkflowService>>,
        subscriptions: Option<WorkflowSubscriptions>,
    ) -> Self {
        let artifact_store = service
            .as_ref()
            .map(|service| WorkflowArtifactStore::new(&config.codex_home, service.store().clone()));
        Self {
            config,
            service,
            subscriptions,
            artifact_store,
        }
    }

    pub(crate) async fn list(&self) -> Result<WorkflowListResponse, JSONRPCErrorError> {
        let service = self.enabled_service()?;
        let data = service
            .registry()
            .definitions()
            .map(|definition| {
                let metadata = definition.metadata();
                WorkflowDefinitionSummary {
                    name: metadata.name().to_string(),
                    version: metadata.version().to_string(),
                    description: metadata.description().to_string(),
                    is_default: metadata.is_default(),
                    stability: match metadata.stability() {
                        WorkflowStability::Experimental => {
                            WorkflowDefinitionStability::Experimental
                        }
                        WorkflowStability::Stable => WorkflowDefinitionStability::Stable,
                    },
                    state_schema_version: metadata.state_schema_version(),
                    argv_help: metadata.argv_help().to_string(),
                    argument_schema: metadata.argument_schema().clone(),
                    output_schema: metadata.output_schema().clone(),
                    required_capabilities: metadata.required_capabilities().to_vec(),
                }
            })
            .collect();
        Ok(WorkflowListResponse { data })
    }

    pub(crate) async fn run(
        &self,
        connection_id: ConnectionId,
        params: WorkflowRunParams,
    ) -> Result<WorkflowRunResponse, JSONRPCErrorError> {
        let service = self.enabled_service()?;
        if params
            .concurrency
            .is_some_and(|concurrency| concurrency == 0)
        {
            return Err(workflow_invalid(
                "invalid_concurrency",
                "workflow concurrency must be greater than zero",
            ));
        }
        let name = WorkflowName::new(params.workflow_name)
            .map_err(|error| workflow_invalid("invalid_workflow_name", error.to_string()))?;
        let version = params
            .workflow_version
            .map(|version| WorkflowVersion::parse(&version))
            .transpose()
            .map_err(|error| workflow_invalid("invalid_workflow_version", error.to_string()))?;
        let registry = service.registry();
        let definition = registry
            .resolve(&name, version.as_ref())
            .map_err(|error| workflow_invalid("workflow_not_found", error.to_string()))?;
        if !definition.metadata().required_capabilities().is_empty()
            && service.prompt_review_capability().is_none()
        {
            return Err(workflow_invalid(
                "capability_unavailable",
                format!(
                    "workflow {}@{} requires unavailable capabilities: {}",
                    definition.metadata().name(),
                    definition.metadata().version(),
                    definition.metadata().required_capabilities().join(", ")
                ),
            ));
        }
        let arguments = match params.arguments {
            WorkflowArgumentsInput::Argv { argv } => definition
                .parse_cli(&argv.into_iter().map(OsString::from).collect::<Vec<_>>())
                .map_err(|error| workflow_invalid("invalid_arguments", error.to_string()))?,
            WorkflowArgumentsInput::Json { value } => value,
        };
        let checkpoint = definition
            .initialize(arguments.clone())
            .map_err(|error| workflow_invalid("invalid_arguments", error.to_string()))?;
        let run_id = WorkflowRunId::new();
        let now_ms = now_ms();
        let record = service
            .store()
            .create_run(WorkflowRunCreate {
                run_id: run_id.to_string(),
                definition_name: definition.metadata().name().to_string(),
                definition_version: definition.metadata().version().to_string(),
                state_schema_version: checkpoint.state_schema_version(),
                state: checkpoint.state().clone(),
                arguments,
                non_interactive: params.non_interactive,
                detached: params.detached,
                concurrency: params.concurrency,
                created_at_ms: now_ms,
            })
            .await
            .map_err(workflow_store_error)?;
        let run = self.snapshot(record).await?;
        if params.subscribe {
            self.subscribe_at_latest(connection_id, &run, params.node_threads)
                .await?;
        }
        self.spawn_driver(run_id);
        Ok(WorkflowRunResponse { run })
    }

    pub(crate) async fn list_runs(
        &self,
        params: WorkflowRunListParams,
    ) -> Result<WorkflowRunListResponse, JSONRPCErrorError> {
        let service = self.enabled_service()?;
        let limit = params
            .limit
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_PAGE_SIZE);
        let records = service
            .store()
            .list_runs(params.cursor.as_deref(), limit.saturating_add(1))
            .await
            .map_err(workflow_store_error)?;
        let has_more = records.len() > usize::try_from(limit).unwrap_or(usize::MAX);
        let records = records
            .into_iter()
            .take(usize::try_from(limit).unwrap_or(usize::MAX))
            .collect::<Vec<_>>();
        let next_cursor = has_more
            .then(|| records.last().map(|run| run.run_id.clone()))
            .flatten();
        let mut data = Vec::with_capacity(records.len());
        for record in records {
            data.push(self.snapshot(record).await?);
        }
        Ok(WorkflowRunListResponse { data, next_cursor })
    }

    pub(crate) async fn read(
        &self,
        params: WorkflowRunReadParams,
    ) -> Result<WorkflowRunReadResponse, JSONRPCErrorError> {
        Ok(WorkflowRunReadResponse {
            run: self.read_snapshot(&params.run_id).await?,
        })
    }

    pub(crate) async fn resume(
        &self,
        params: WorkflowRunResumeParams,
    ) -> Result<WorkflowRunResumeResponse, JSONRPCErrorError> {
        let service = self.enabled_service()?;
        let run_id = parse_run_id(&params.run_id)?;
        let current = service
            .store()
            .read_run(&params.run_id)
            .await
            .map_err(workflow_store_error)?
            .ok_or_else(|| workflow_invalid("run_not_found", "workflow run was not found"))?;
        if params.detached && !current.detached {
            return Err(workflow_conflict(
                "run_not_detachable",
                "workflow run was not launched with detached-safe node policies",
            ));
        }
        if !matches!(
            current.status,
            codex_state::WorkflowRunStatus::Waiting | codex_state::WorkflowRunStatus::NeedsOperator
        ) {
            return Err(workflow_conflict(
                "run_not_resumable",
                "workflow run is not waiting for explicit resume",
            ));
        }
        let resumed = service
            .store()
            .resume_run(&params.run_id, now_ms())
            .await
            .map_err(workflow_store_error)?;
        let run = self.snapshot(resumed).await?;
        self.spawn_driver(run_id);
        self.publish(&params.run_id);
        Ok(WorkflowRunResumeResponse { run })
    }

    pub(crate) async fn cancel(
        &self,
        params: WorkflowRunCancelParams,
    ) -> Result<WorkflowRunCancelResponse, JSONRPCErrorError> {
        let service = self.enabled_service()?;
        let run_id = parse_run_id(&params.run_id)?;
        let accepted = service
            .store()
            .request_run_cancellation(&params.run_id, now_ms())
            .await
            .map_err(workflow_store_error)?;
        let run = self.snapshot(accepted).await?;
        let subscriptions = self.subscriptions.clone();
        let run_id_text = params.run_id;
        tokio::spawn(async move {
            let controller = WorkflowCancellationController::new(service.as_ref().clone());
            if let Err(error) = controller
                .cancel_run(
                    run_id,
                    CancellationReason::WorkflowRequested,
                    now_ms(),
                    Duration::from_secs(5),
                )
                .await
            {
                tracing::warn!(%error, workflow_run_id = run_id_text, "workflow cancellation failed");
            }
            if let Some(subscriptions) = subscriptions {
                let _ = subscriptions.publish_run(&run_id_text).await;
            }
        });
        Ok(WorkflowRunCancelResponse { run })
    }

    pub(crate) async fn subscribe(
        &self,
        connection_id: ConnectionId,
        params: WorkflowRunSubscribeParams,
    ) -> Result<WorkflowRunSubscribeResponse, JSONRPCErrorError> {
        let run = self.read_snapshot(&params.run_id).await?;
        let replay = self
            .subscriptions()?
            .subscribe(
                connection_id,
                &params.run_id,
                params.after_sequence.unwrap_or(0),
                params.node_threads,
            )
            .await
            .map_err(workflow_store_error)?;
        Ok(WorkflowRunSubscribeResponse {
            run,
            events: replay.events,
            snapshot_required: replay.snapshot_required,
        })
    }

    pub(crate) async fn unsubscribe(
        &self,
        connection_id: ConnectionId,
        params: WorkflowRunUnsubscribeParams,
    ) -> Result<WorkflowRunUnsubscribeResponse, JSONRPCErrorError> {
        self.enabled_service()?;
        Ok(WorkflowRunUnsubscribeResponse {
            removed: self
                .subscriptions()?
                .unsubscribe(connection_id, &params.run_id)
                .await,
        })
    }

    pub(crate) async fn respond(
        &self,
        params: WorkflowInteractionRespondParams,
    ) -> Result<WorkflowInteractionRespondResponse, JSONRPCErrorError> {
        let service = self.enabled_service()?;
        if params.idempotency_key.trim().is_empty() {
            return Err(workflow_invalid(
                "invalid_idempotency_key",
                "idempotency key must not be empty",
            ));
        }
        let interaction_id = InteractionId::parse(&params.interaction_id)
            .map_err(|error| workflow_invalid("invalid_interaction_id", error.to_string()))?;
        let interaction = service
            .store()
            .read_interaction(&params.interaction_id)
            .await
            .map_err(workflow_store_error)?
            .ok_or_else(|| {
                workflow_invalid("interaction_not_found", "interaction was not found")
            })?;
        let run_id = parse_run_id(&interaction.run_id)?;
        let effect_key = format!(
            "interaction.respond:{interaction_id}:{}",
            params.idempotency_key
        );
        let planned = service
            .store()
            .plan_effect(WorkflowEffectPlan {
                run_id: interaction.run_id.clone(),
                effect_key: effect_key.clone(),
                kind: "interaction.respond".to_string(),
                request: json!({
                    "interaction_id": params.interaction_id,
                    "idempotency_key": params.idempotency_key,
                    "response": params.response,
                }),
                created_at_ms: now_ms(),
            })
            .await
            .map_err(workflow_store_error)?;
        let mut effect = match planned {
            WorkflowEffectPlanOutcome::Planned(effect)
            | WorkflowEffectPlanOutcome::Existing(effect) => effect,
        };
        if effect.state == WorkflowEffectState::Applied {
            return Ok(WorkflowInteractionRespondResponse {
                run: self.read_snapshot(&interaction.run_id).await?,
                accepted: false,
            });
        }
        if effect.state == WorkflowEffectState::Planned {
            let artifact_id = ArtifactId::new();
            service
                .store()
                .update_effect(WorkflowEffectUpdate {
                    run_id: interaction.run_id.clone(),
                    effect_key: effect_key.clone(),
                    expected_state: WorkflowEffectState::Planned,
                    state: WorkflowEffectState::Dispatched,
                    response: Some(json!({"artifact_id": artifact_id})),
                    error_code: None,
                    updated_at_ms: now_ms(),
                })
                .await
                .map_err(workflow_store_error)?;
            effect = service
                .store()
                .read_effect(&interaction.run_id, &effect_key)
                .await
                .map_err(workflow_store_error)?
                .ok_or_else(|| workflow_internal("interaction effect disappeared"))?;
        }
        let artifact_id = effect
            .response
            .as_ref()
            .and_then(|response| response.get("artifact_id"))
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| workflow_internal("interaction effect has no artifact identity"))
            .and_then(|value| {
                ArtifactId::parse(value).map_err(|error| {
                    workflow_internal(format!("invalid artifact identity: {error}"))
                })
            })?;
        let request = effect
            .request
            .get("response")
            .cloned()
            .ok_or_else(|| workflow_internal("interaction effect has no response payload"))?;
        let bytes =
            serde_json::to_vec(&request).map_err(|error| workflow_internal(error.to_string()))?;
        let relative_path = PathBuf::from("interactions")
            .join(interaction_id.to_string())
            .join(format!("{artifact_id}.json"));
        match self
            .artifact_store()?
            .write(WorkflowArtifactWrite {
                run_id,
                artifact_id,
                relative_path: &relative_path,
                classification: ArtifactClassification::Internal,
                media_type: "application/json",
                bytes: &bytes,
                created_at_ms: now_ms(),
            })
            .await
        {
            Ok(_) | Err(codex_workflow_extension::WorkflowArtifactStoreError::AlreadyExists) => {}
            Err(error) => return Err(workflow_internal(error.to_string())),
        }
        let artifact_id_string = artifact_id.to_string();
        let (accepted, resumed) = if interaction.state == WorkflowInteractionState::Waiting {
            let resolution = service
                .store()
                .resolve_interaction(&interaction.interaction_id, &artifact_id_string, now_ms())
                .await
                .map_err(workflow_store_error)?;
            if resolution.0 {
                resolution
            } else {
                let current = service
                    .store()
                    .read_interaction(&interaction.interaction_id)
                    .await
                    .map_err(workflow_store_error)?
                    .ok_or_else(|| workflow_internal("interaction disappeared during response"))?;
                if current.state == WorkflowInteractionState::Resolved
                    && current.response_artifact_id.as_deref() == Some(artifact_id_string.as_str())
                {
                    (false, false)
                } else {
                    return Err(workflow_conflict(
                        "interaction_not_waiting",
                        "interaction is no longer waiting for this response",
                    ));
                }
            }
        } else if interaction.state == WorkflowInteractionState::Resolved
            && interaction.response_artifact_id.as_deref() == Some(artifact_id_string.as_str())
        {
            (false, false)
        } else {
            return Err(workflow_conflict(
                "interaction_not_waiting",
                "interaction is no longer waiting for this response",
            ));
        };
        service
            .store()
            .update_effect(WorkflowEffectUpdate {
                run_id: interaction.run_id.clone(),
                effect_key,
                expected_state: WorkflowEffectState::Dispatched,
                state: WorkflowEffectState::Applied,
                response: Some(json!({"artifact_id": artifact_id})),
                error_code: None,
                updated_at_ms: now_ms(),
            })
            .await
            .map_err(workflow_store_error)?;
        if resumed {
            self.spawn_driver(run_id);
        }
        self.publish(&interaction.run_id);
        Ok(WorkflowInteractionRespondResponse {
            run: self.read_snapshot(&interaction.run_id).await?,
            accepted,
        })
    }

    pub(crate) async fn connection_closed(&self, connection_id: ConnectionId) {
        if let Some(subscriptions) = &self.subscriptions {
            subscriptions.connection_closed(connection_id).await;
        }
    }
}
