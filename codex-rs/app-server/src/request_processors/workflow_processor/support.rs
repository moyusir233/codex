use super::*;

impl WorkflowRequestProcessor {
    pub(super) async fn read_snapshot(
        &self,
        run_id: &str,
    ) -> Result<WorkflowRun, JSONRPCErrorError> {
        let service = self.enabled_service()?;
        let record = service
            .store()
            .read_run(run_id)
            .await
            .map_err(workflow_store_error)?
            .ok_or_else(|| workflow_invalid("run_not_found", "workflow run was not found"))?;
        self.snapshot(record).await
    }

    pub(super) async fn snapshot(
        &self,
        record: WorkflowRunRecord,
    ) -> Result<WorkflowRun, JSONRPCErrorError> {
        let service = self.enabled_service()?;
        let node_records = service
            .store()
            .list_nodes(&record.run_id)
            .await
            .map_err(workflow_store_error)?;
        let mut nodes = Vec::with_capacity(node_records.len());
        for node in node_records {
            let attempts = service
                .store()
                .list_node_attempts(&node.node_id)
                .await
                .map_err(workflow_store_error)?;
            let mut thread_ids = Vec::new();
            for thread_id in attempts.into_iter().filter_map(|attempt| attempt.thread_id) {
                if !thread_ids.contains(&thread_id) {
                    thread_ids.push(thread_id);
                }
            }
            if let Some(thread_id) = node.thread_id.as_ref()
                && !thread_ids.contains(thread_id)
            {
                thread_ids.push(thread_id.clone());
            }
            nodes.push(WorkflowNodeSummary {
                node_id: node.node_id,
                node_key: node.node_key,
                thread_id: node.thread_id,
                thread_ids,
                status: node_status(node.status),
                retry_at_ms: node.retry_at_ms,
                created_at_ms: node.created_at_ms,
                updated_at_ms: node.updated_at_ms,
            });
        }
        let artifacts = service
            .store()
            .list_artifacts(&record.run_id)
            .await
            .map_err(workflow_store_error)?
            .into_iter()
            .map(|artifact| WorkflowArtifactSummary {
                artifact_id: artifact.artifact_id,
                relative_path: artifact.relative_path,
                classification: artifact.classification.as_str().to_string(),
                media_type: artifact.media_type,
                size_bytes: artifact.byte_count,
                sha256: artifact.sha256,
                created_at_ms: artifact.created_at_ms,
            })
            .collect();
        Ok(WorkflowRun {
            run_id: record.run_id,
            workflow_name: record.definition_name,
            workflow_version: record.definition_version,
            status: run_status(record.status),
            arguments: record.arguments,
            non_interactive: record.non_interactive,
            detached: record.detached,
            concurrency: record.concurrency,
            output: record.output,
            error_code: record.error_code,
            wake: record.wake,
            next_sequence: record.next_sequence,
            nodes,
            artifacts,
            created_at_ms: record.created_at_ms,
            updated_at_ms: record.updated_at_ms,
        })
    }

    pub(super) async fn subscribe_at_latest(
        &self,
        connection_id: ConnectionId,
        run: &WorkflowRun,
        node_threads: NodeThreadSubscription,
    ) -> Result<(), JSONRPCErrorError> {
        self.subscriptions()?
            .subscribe(
                connection_id,
                &run.run_id,
                run.next_sequence.saturating_sub(1),
                node_threads,
            )
            .await
            .map_err(workflow_store_error)?;
        Ok(())
    }

    pub(super) fn spawn_driver(&self, run_id: WorkflowRunId) {
        let Some(service) = self.service.clone() else {
            return;
        };
        let subscriptions = self.subscriptions.clone();
        tokio::spawn(async move {
            let mut driver = WorkflowDriver::new(
                service.store().clone(),
                service.registry(),
                format!("app-server-{}", uuid::Uuid::now_v7()),
                30_000,
            )
            .with_runtime_facets(service.as_ref().clone());
            if let Some(capability) = service.prompt_review_capability() {
                driver = driver.with_prompt_review_capability(capability);
            }
            let mut busy_backoff = Duration::from_millis(10);
            loop {
                match driver
                    .drive_until_blocked(
                        run_id,
                        now_ms(),
                        NonZeroUsize::new(DRIVER_STEP_LIMIT).unwrap_or(NonZeroUsize::MIN),
                    )
                    .await
                {
                    Ok(DriveOutcome::StepLimitReached | DriveOutcome::Continued) => {
                        tokio::task::yield_now().await;
                    }
                    Ok(DriveOutcome::Busy) => {
                        tokio::time::sleep(busy_backoff).await;
                        busy_backoff = (busy_backoff * 2).min(Duration::from_secs(1));
                    }
                    Ok(DriveOutcome::Completed | DriveOutcome::Failed) => {
                        if let Err(error) = service.detach_run_observers(run_id).await {
                            tracing::warn!(%error, %run_id, "failed to release workflow observers");
                        }
                        break;
                    }
                    Ok(
                        DriveOutcome::Waiting
                        | DriveOutcome::Cancelling
                        | DriveOutcome::NeedsOperator,
                    ) => break,
                    Err(error) => {
                        tracing::warn!(%error, %run_id, "workflow drive failed");
                        break;
                    }
                }
            }
            if let Some(subscriptions) = subscriptions {
                let _ = subscriptions.publish_run(&run_id.to_string()).await;
            }
        });
    }

    pub(super) fn publish(&self, run_id: &str) {
        let Some(subscriptions) = self.subscriptions.clone() else {
            return;
        };
        let run_id = run_id.to_string();
        tokio::spawn(async move {
            let _ = subscriptions.publish_run(&run_id).await;
        });
    }

    pub(super) fn enabled_service(&self) -> Result<Arc<WorkflowService>, JSONRPCErrorError> {
        if !self.config.features.enabled(Feature::Workflows) {
            return Err(invalid_request("workflows feature is disabled"));
        }
        self.service
            .clone()
            .ok_or_else(|| workflow_internal("workflow state is unavailable"))
    }

    pub(super) fn subscriptions(&self) -> Result<&WorkflowSubscriptions, JSONRPCErrorError> {
        self.subscriptions
            .as_ref()
            .ok_or_else(|| workflow_internal("workflow subscriptions are unavailable"))
    }

    pub(super) fn artifact_store(&self) -> Result<&WorkflowArtifactStore, JSONRPCErrorError> {
        self.artifact_store
            .as_ref()
            .ok_or_else(|| workflow_internal("workflow artifact store is unavailable"))
    }
}

pub(super) fn parse_run_id(value: &str) -> Result<WorkflowRunId, JSONRPCErrorError> {
    WorkflowRunId::parse(value)
        .map_err(|error| workflow_invalid("invalid_run_id", error.to_string()))
}

fn run_status(status: codex_state::WorkflowRunStatus) -> WorkflowRunStatus {
    match status {
        codex_state::WorkflowRunStatus::Pending => WorkflowRunStatus::Pending,
        codex_state::WorkflowRunStatus::Running => WorkflowRunStatus::Running,
        codex_state::WorkflowRunStatus::Waiting => WorkflowRunStatus::Waiting,
        codex_state::WorkflowRunStatus::Cancelling => WorkflowRunStatus::Cancelling,
        codex_state::WorkflowRunStatus::Succeeded => WorkflowRunStatus::Succeeded,
        codex_state::WorkflowRunStatus::Failed => WorkflowRunStatus::Failed,
        codex_state::WorkflowRunStatus::Cancelled => WorkflowRunStatus::Cancelled,
        codex_state::WorkflowRunStatus::NeedsOperator => WorkflowRunStatus::NeedsOperator,
    }
}

fn node_status(status: codex_state::WorkflowNodeStatus) -> WorkflowNodeStatus {
    match status {
        codex_state::WorkflowNodeStatus::Pending => WorkflowNodeStatus::Pending,
        codex_state::WorkflowNodeStatus::Ready => WorkflowNodeStatus::Ready,
        codex_state::WorkflowNodeStatus::Running => WorkflowNodeStatus::Running,
        codex_state::WorkflowNodeStatus::Waiting => WorkflowNodeStatus::Waiting,
        codex_state::WorkflowNodeStatus::Succeeded => WorkflowNodeStatus::Succeeded,
        codex_state::WorkflowNodeStatus::Failed => WorkflowNodeStatus::Failed,
        codex_state::WorkflowNodeStatus::Cancelled => WorkflowNodeStatus::Cancelled,
        codex_state::WorkflowNodeStatus::Blocked => WorkflowNodeStatus::Blocked,
    }
}

pub(super) fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub(super) fn workflow_store_error(error: codex_state::WorkflowStoreError) -> JSONRPCErrorError {
    use codex_state::WorkflowStoreError;
    match error {
        WorkflowStoreError::RunNotFound => {
            workflow_invalid("run_not_found", "workflow run was not found")
        }
        WorkflowStoreError::DuplicateRun
        | WorkflowStoreError::EffectConflict
        | WorkflowStoreError::InteractionConflict
        | WorkflowStoreError::StaleWrite => {
            workflow_conflict("workflow_conflict", error.to_string())
        }
        _ => workflow_internal(error.to_string()),
    }
}

pub(super) fn workflow_invalid(
    code: &'static str,
    message: impl Into<String>,
) -> JSONRPCErrorError {
    workflow_error(INVALID_PARAMS_ERROR_CODE, code, message)
}

pub(super) fn workflow_conflict(
    code: &'static str,
    message: impl Into<String>,
) -> JSONRPCErrorError {
    workflow_error(-32009, code, message)
}

pub(super) fn workflow_internal(message: impl Into<String>) -> JSONRPCErrorError {
    workflow_error(INTERNAL_ERROR_CODE, "internal", message)
}

fn workflow_error(
    jsonrpc_code: i64,
    workflow_code: &'static str,
    message: impl Into<String>,
) -> JSONRPCErrorError {
    JSONRPCErrorError {
        code: jsonrpc_code,
        message: message.into(),
        data: Some(json!({WORKFLOW_ERROR_FIELD: workflow_code})),
    }
}
