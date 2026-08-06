use codex_state::WorkflowEffectPlan;
use codex_state::WorkflowEffectState;
use codex_state::WorkflowEffectUpdate;
use codex_state::WorkflowFornaxTracePlan;
use codex_state::WorkflowFornaxTracePlanOutcome;
use codex_state::WorkflowFornaxTraceState;
use codex_state::WorkflowFornaxTraceUpdate;
use codex_state::WorkflowStore;
use codex_state::canonical_workflow_request_hash;
use serde::Serialize;
use uuid::Uuid;

use super::FinishSpanRequest;
use super::FinishedSpan;
use super::FornaxBridgeError;
use super::FornaxTraceWriter;
use super::RecordSpanRequest;
use super::RecordedSpan;
use super::SpanParent;
use super::SpanRecord;
use super::SpanType;
use super::StartSpanRequest;
use super::StartedSpan;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FornaxSpanCorrelation {
    pub span_handle_id: Uuid,
    pub trace_context_id: Uuid,
    pub trace_id: String,
    pub span_id: String,
}

impl From<&StartedSpan> for FornaxSpanCorrelation {
    fn from(value: &StartedSpan) -> Self {
        Self {
            span_handle_id: value.span_handle_id,
            trace_context_id: value.trace_context_id,
            trace_id: value.trace_id.clone(),
            span_id: value.span_id.clone(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DurableFornaxTraceError {
    #[error(transparent)]
    Store(#[from] codex_state::WorkflowStoreError),
    #[error(transparent)]
    Bridge(#[from] FornaxBridgeError),
    #[error("durable Fornax correlation changed concurrently")]
    StaleCorrelation,
    #[error("applied durable Fornax effect has no valid response: {0}")]
    InvalidAppliedResponse(String),
}

/// Couples every bridge mutation to workflow effect and correlation journals.
#[derive(Clone)]
pub struct DurableFornaxTraceWriter {
    writer: FornaxTraceWriter,
    store: WorkflowStore,
    run_id: String,
    bridge_instance_id: String,
}

impl DurableFornaxTraceWriter {
    pub fn new(
        writer: FornaxTraceWriter,
        store: WorkflowStore,
        run_id: impl Into<String>,
        bridge_instance_id: impl Into<String>,
    ) -> Self {
        Self {
            writer,
            store,
            run_id: run_id.into(),
            bridge_instance_id: bridge_instance_id.into(),
        }
    }

    pub async fn start(
        &self,
        effect_key: &str,
        operation_id: Uuid,
        name: impl Into<String>,
        span_type: SpanType,
        parent: SpanParent,
        now_ms: i64,
    ) -> Result<StartedSpan, DurableFornaxTraceError> {
        let request = StartSpanRequest {
            operation_id,
            name: name.into(),
            span_type,
            parent,
        };
        let (existing, effect_state, response) = self
            .prepare(
                effect_key,
                "fornax.trace.start",
                &request,
                operation_id,
                now_ms,
            )
            .await?;
        if effect_state == WorkflowEffectState::Applied {
            return decode_applied_response(response);
        }
        let result = self
            .writer
            .start(
                request.operation_id,
                request.name.clone(),
                request.span_type,
                request.parent.clone(),
            )
            .await;
        match result {
            Ok(started) => {
                let correlation = FornaxSpanCorrelation::from(&started);
                self.persist_success(
                    effect_key,
                    existing,
                    WorkflowFornaxTraceState::Live,
                    &correlation,
                    &started,
                    now_ms,
                )
                .await?;
                Ok(started)
            }
            Err(error) => {
                self.persist_failure(effect_key, existing, operation_id, &error, now_ms)
                    .await?;
                Err(error.into())
            }
        }
    }

    pub async fn record(
        &self,
        effect_key: &str,
        correlation: &FornaxSpanCorrelation,
        operation_id: Uuid,
        record: SpanRecord,
        now_ms: i64,
    ) -> Result<RecordedSpan, DurableFornaxTraceError> {
        let request = RecordSpanRequest {
            operation_id,
            record,
        };
        let (existing, effect_state, response) = self
            .prepare(
                effect_key,
                "fornax.trace.record",
                &request,
                operation_id,
                now_ms,
            )
            .await?;
        if effect_state == WorkflowEffectState::Applied {
            return decode_applied_response(response);
        }
        let result = self
            .writer
            .record(
                correlation.span_handle_id,
                operation_id,
                request.record.clone(),
            )
            .await;
        match result {
            Ok(recorded) => {
                self.persist_success(
                    effect_key,
                    existing,
                    WorkflowFornaxTraceState::Applied,
                    correlation,
                    &recorded,
                    now_ms,
                )
                .await?;
                Ok(recorded)
            }
            Err(error) => {
                self.persist_failure(effect_key, existing, operation_id, &error, now_ms)
                    .await?;
                Err(error.into())
            }
        }
    }

    pub async fn finish(
        &self,
        effect_key: &str,
        correlation: &FornaxSpanCorrelation,
        operation_id: Uuid,
        now_ms: i64,
    ) -> Result<FinishedSpan, DurableFornaxTraceError> {
        let request = FinishSpanRequest { operation_id };
        let (existing, effect_state, response) = self
            .prepare(
                effect_key,
                "fornax.trace.finish",
                &request,
                operation_id,
                now_ms,
            )
            .await?;
        if effect_state == WorkflowEffectState::Applied {
            return decode_applied_response(response);
        }
        let result = self
            .writer
            .finish(correlation.span_handle_id, operation_id)
            .await;
        match result {
            Ok(finished) => {
                self.persist_success(
                    effect_key,
                    existing,
                    WorkflowFornaxTraceState::Finished,
                    correlation,
                    &finished,
                    now_ms,
                )
                .await?;
                Ok(finished)
            }
            Err(error) => {
                self.persist_failure(effect_key, existing, operation_id, &error, now_ms)
                    .await?;
                Err(error.into())
            }
        }
    }

    async fn prepare<T: Serialize>(
        &self,
        effect_key: &str,
        kind: &str,
        request: &T,
        operation_id: Uuid,
        now_ms: i64,
    ) -> Result<
        (
            WorkflowFornaxTraceState,
            WorkflowEffectState,
            Option<serde_json::Value>,
        ),
        DurableFornaxTraceError,
    > {
        let request = serde_json::to_value(request)?;
        let request_hash = canonical_workflow_request_hash(&request)?;
        let effect = self
            .store
            .plan_effect(WorkflowEffectPlan {
                run_id: self.run_id.clone(),
                effect_key: effect_key.to_string(),
                kind: kind.to_string(),
                request,
                created_at_ms: now_ms,
            })
            .await?;
        let correlation = self
            .store
            .plan_fornax_trace(WorkflowFornaxTracePlan {
                run_id: self.run_id.clone(),
                effect_key: effect_key.to_string(),
                operation_id: operation_id.to_string(),
                request_hash,
                created_at_ms: now_ms,
            })
            .await?;
        let state = match correlation {
            WorkflowFornaxTracePlanOutcome::Planned(record)
            | WorkflowFornaxTracePlanOutcome::Existing(record) => record.state,
        };
        let effect = match effect {
            codex_state::WorkflowEffectPlanOutcome::Planned(record)
            | codex_state::WorkflowEffectPlanOutcome::Existing(record) => record,
        };
        let effect_state = if effect.state == WorkflowEffectState::Planned {
            self.store
                .update_effect(WorkflowEffectUpdate {
                    run_id: self.run_id.clone(),
                    effect_key: effect_key.to_string(),
                    expected_state: WorkflowEffectState::Planned,
                    state: WorkflowEffectState::Dispatched,
                    response: None,
                    error_code: None,
                    updated_at_ms: now_ms,
                })
                .await?;
            WorkflowEffectState::Dispatched
        } else {
            effect.state
        };
        Ok((state, effect_state, effect.response))
    }

    async fn persist_success<T: Serialize>(
        &self,
        effect_key: &str,
        existing: WorkflowFornaxTraceState,
        state: WorkflowFornaxTraceState,
        correlation: &FornaxSpanCorrelation,
        response: &T,
        now_ms: i64,
    ) -> Result<(), DurableFornaxTraceError> {
        if existing != state
            && !self
                .store
                .update_fornax_trace(WorkflowFornaxTraceUpdate {
                    run_id: self.run_id.clone(),
                    effect_key: effect_key.to_string(),
                    expected_state: existing,
                    state,
                    span_handle_id: Some(correlation.span_handle_id.to_string()),
                    trace_context_id: Some(correlation.trace_context_id.to_string()),
                    trace_id: Some(correlation.trace_id.clone()),
                    span_id: Some(correlation.span_id.clone()),
                    bridge_instance_id: Some(self.bridge_instance_id.clone()),
                    error_code: None,
                    updated_at_ms: now_ms,
                })
                .await?
        {
            return Err(DurableFornaxTraceError::StaleCorrelation);
        }
        let response = serde_json::to_value(response)?;
        self.store
            .update_effect(WorkflowEffectUpdate {
                run_id: self.run_id.clone(),
                effect_key: effect_key.to_string(),
                expected_state: WorkflowEffectState::Dispatched,
                state: WorkflowEffectState::Applied,
                response: Some(response),
                error_code: None,
                updated_at_ms: now_ms,
            })
            .await?;
        Ok(())
    }

    async fn persist_failure(
        &self,
        effect_key: &str,
        existing: WorkflowFornaxTraceState,
        operation_id: Uuid,
        error: &FornaxBridgeError,
        now_ms: i64,
    ) -> Result<(), DurableFornaxTraceError> {
        if matches!(error, FornaxBridgeError::QueueFull) {
            return Ok(());
        }
        let state = if matches!(error, FornaxBridgeError::AmbiguousMutation { .. }) {
            WorkflowFornaxTraceState::Ambiguous
        } else {
            WorkflowFornaxTraceState::Failed
        };
        if existing == WorkflowFornaxTraceState::Planned {
            self.store
                .update_fornax_trace(WorkflowFornaxTraceUpdate {
                    run_id: self.run_id.clone(),
                    effect_key: effect_key.to_string(),
                    expected_state: existing,
                    state,
                    span_handle_id: None,
                    trace_context_id: None,
                    trace_id: None,
                    span_id: None,
                    bridge_instance_id: Some(self.bridge_instance_id.clone()),
                    error_code: Some(error_code(error).to_string()),
                    updated_at_ms: now_ms,
                })
                .await?;
        }
        self.store
            .update_effect(WorkflowEffectUpdate {
                run_id: self.run_id.clone(),
                effect_key: effect_key.to_string(),
                expected_state: WorkflowEffectState::Dispatched,
                state: WorkflowEffectState::Ambiguous,
                response: None,
                error_code: Some(format!("{}:{operation_id}", error_code(error))),
                updated_at_ms: now_ms,
            })
            .await?;
        Ok(())
    }
}

fn decode_applied_response<T: serde::de::DeserializeOwned>(
    response: Option<serde_json::Value>,
) -> Result<T, DurableFornaxTraceError> {
    serde_json::from_value(response.ok_or_else(|| {
        DurableFornaxTraceError::InvalidAppliedResponse("missing response".to_string())
    })?)
    .map_err(|error| DurableFornaxTraceError::InvalidAppliedResponse(error.to_string()))
}

fn error_code(error: &FornaxBridgeError) -> &'static str {
    match error {
        FornaxBridgeError::AmbiguousMutation { .. } => "ambiguous_mutation",
        FornaxBridgeError::QueueFull => "queue_full",
        FornaxBridgeError::OrphanedSpan => "orphaned_span",
        FornaxBridgeError::SdkAuthentication => "sdk_authentication",
        FornaxBridgeError::ShuttingDown => "shutting_down",
        _ => "fornax_bridge",
    }
}

impl From<serde_json::Error> for DurableFornaxTraceError {
    fn from(error: serde_json::Error) -> Self {
        Self::Store(codex_state::WorkflowStoreError::Json(error))
    }
}
