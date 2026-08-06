use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use codex_state::WorkflowEffectPlan;
use codex_state::WorkflowEffectPlanOutcome;
use codex_state::WorkflowEffectState;
use codex_state::WorkflowEffectUpdate;
use codex_state::WorkflowInteractionPlan;
use codex_state::WorkflowInteractionPlanOutcome;
use codex_state::WorkflowInteractionState;
use codex_state::WorkflowLarkInteractionPlan;
use codex_state::WorkflowLarkResolve;
use codex_state::WorkflowLarkResolveOutcome;
use codex_state::WorkflowStore;
use serde_json::Value;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;

use crate::ArtifactClassification;
use crate::ArtifactId;
use crate::HumanInteractionOutcome;
use crate::HumanInteractionRequest;
use crate::InteractionId;
use crate::WorkflowRunId;
use crate::integrations::lark::ContentSensitivity;
use crate::integrations::lark::LarkCli;
use crate::integrations::lark::LarkCliError;
use crate::integrations::lark::LarkEvent;
use crate::integrations::lark::MessageBody;
use crate::integrations::lark::MessageSendRequest;
use crate::integrations::lark::MessageTarget;
use crate::integrations::lark::OpenId;
use crate::integrations::lark::ThreadId;

use super::WorkflowArtifactStore;
use super::WorkflowArtifactStoreError;
use super::WorkflowArtifactWrite;

/// Durable Lark send, reconciliation, reply, and artifact service.
#[derive(Clone)]
pub struct LarkInteractionService {
    pub(super) cli: Arc<LarkCli>,
    pub(super) store: WorkflowStore,
    pub(super) artifacts: WorkflowArtifactStore,
}

impl LarkInteractionService {
    pub fn new(cli: Arc<LarkCli>, store: WorkflowStore, artifacts: WorkflowArtifactStore) -> Self {
        Self {
            cli,
            store,
            artifacts,
        }
    }

    pub fn cli(&self) -> &Arc<LarkCli> {
        &self.cli
    }

    pub async fn poll_or_request(
        &self,
        run_id: WorkflowRunId,
        request: HumanInteractionRequest,
        now_ms: i64,
        cancelled: &AtomicBool,
    ) -> Result<HumanInteractionOutcome, LarkInteractionError> {
        if request.sensitivity == ContentSensitivity::Sensitive {
            return Err(LarkCliError::SensitiveArgv.into());
        }
        if request.allowed_senders.is_empty() || request.deadline_ms <= 0 {
            return Err(LarkInteractionError::InvalidRequest);
        }
        let run_id_text = run_id.to_string();
        let effect_key = request.effect_key.to_string();
        let token = correlation_token(&run_id_text, &effect_key);
        let idempotency_key = message_idempotency_key(&run_id_text, &effect_key);
        let effect_request = json!({
            "chat_id": request.chat_id.as_str(),
            "thread_id": request.thread_id.as_ref().map(ThreadId::as_str),
            "allowed_senders": request
                .allowed_senders
                .iter()
                .map(OpenId::as_str)
                .collect::<Vec<_>>(),
            "deadline_ms": request.deadline_ms,
            "correlation_token": &token,
            "prompt": &request.prompt,
        });
        let planned = self
            .store
            .plan_effect(WorkflowEffectPlan {
                run_id: run_id_text.clone(),
                effect_key: effect_key.clone(),
                kind: "lark.interaction.request".to_string(),
                request: effect_request,
                created_at_ms: now_ms,
            })
            .await?;
        let mut effect = match planned {
            WorkflowEffectPlanOutcome::Planned(effect)
            | WorkflowEffectPlanOutcome::Existing(effect) => effect,
        };
        if effect.state == WorkflowEffectState::Planned {
            let interaction_id = InteractionId::new();
            if !self
                .store
                .update_effect(WorkflowEffectUpdate {
                    run_id: run_id_text.clone(),
                    effect_key: effect_key.clone(),
                    expected_state: WorkflowEffectState::Planned,
                    state: WorkflowEffectState::Dispatched,
                    response: Some(json!({"interaction_id": interaction_id})),
                    error_code: None,
                    updated_at_ms: now_ms,
                })
                .await?
            {
                return Err(LarkInteractionError::ConcurrentMutation);
            }
            effect = self
                .store
                .read_effect(&run_id_text, &effect_key)
                .await?
                .ok_or(LarkInteractionError::MissingEffect)?;
        }
        let interaction_id = effect_interaction_id(&effect.response)?;
        self.ensure_interaction_records(
            run_id,
            interaction_id,
            &effect_key,
            &request,
            &token,
            now_ms,
        )
        .await?;
        if effect.state == WorkflowEffectState::Applied {
            return self
                .refresh_outcome(interaction_id, now_ms, cancelled)
                .await;
        }
        if effect.state != WorkflowEffectState::Dispatched {
            return Ok(HumanInteractionOutcome::NeedsOperator {
                interaction_id,
                reason: "lark_send_requires_reconciliation".to_string(),
            });
        }

        if let Some(message_id) = self
            .poll_for_request_marker(interaction_id, cancelled)
            .await?
        {
            self.complete_send(
                &run_id_text,
                &effect_key,
                interaction_id,
                &message_id,
                now_ms,
            )
            .await?;
            return self
                .refresh_outcome(interaction_id, now_ms, cancelled)
                .await;
        }

        let prompt = format!("{}\n\n[wf:{}]", request.prompt, token);
        match self.cli.send_message(
            &MessageSendRequest {
                target: MessageTarget::Chat(request.chat_id),
                body: MessageBody::Text(prompt),
                idempotency_key,
                sensitivity: ContentSensitivity::NonSensitive,
            },
            cancelled,
        ) {
            Ok(sent) => {
                self.complete_send(
                    &run_id_text,
                    &effect_key,
                    interaction_id,
                    sent.message_id.as_str(),
                    now_ms,
                )
                .await?;
            }
            Err(error) => {
                if let Some(message_id) = self
                    .poll_for_request_marker(interaction_id, cancelled)
                    .await?
                {
                    self.complete_send(
                        &run_id_text,
                        &effect_key,
                        interaction_id,
                        &message_id,
                        now_ms,
                    )
                    .await?;
                } else {
                    let _ = self
                        .store
                        .update_effect(WorkflowEffectUpdate {
                            run_id: run_id_text,
                            effect_key,
                            expected_state: WorkflowEffectState::Dispatched,
                            state: WorkflowEffectState::Ambiguous,
                            response: effect.response,
                            error_code: Some("lark_send_ambiguous".to_string()),
                            updated_at_ms: now_ms,
                        })
                        .await?;
                    tracing::warn!(%interaction_id, "Lark send requires reconciliation: {error}");
                    return Ok(HumanInteractionOutcome::NeedsOperator {
                        interaction_id,
                        reason: "lark_send_ambiguous".to_string(),
                    });
                }
            }
        }
        self.refresh_outcome(interaction_id, now_ms, cancelled)
            .await
    }

    pub async fn ingest_event(
        &self,
        event: &LarkEvent,
        source: &str,
    ) -> Result<Vec<WorkflowLarkResolveOutcome>, LarkInteractionError> {
        let candidates = self
            .store
            .list_waiting_lark_interactions(event.chat_id.as_str(), 100)
            .await?;
        let mut outcomes = Vec::new();
        for candidate in candidates {
            if !event
                .text
                .contains(&format!("[wf:{}]", candidate.correlation_token))
            {
                continue;
            }
            if event.created_at_ms < candidate.watermark_ms
                || !candidate
                    .allowed_senders
                    .iter()
                    .any(|sender| sender == event.sender_id.as_str())
                || candidate
                    .thread_id
                    .as_ref()
                    .is_some_and(|thread| event.thread_id.as_ref() != Some(thread))
            {
                continue;
            }
            let interaction = self
                .store
                .read_interaction(&candidate.interaction_id)
                .await?
                .ok_or(LarkInteractionError::MissingInteraction)?;
            if interaction
                .deadline_ms
                .is_some_and(|deadline| event.created_at_ms > deadline)
            {
                continue;
            }
            let artifact_id = self
                .write_reply_artifact(&candidate.run_id, &candidate.interaction_id, event)
                .await?;
            outcomes.push(
                self.store
                    .resolve_lark_interaction(WorkflowLarkResolve {
                        interaction_id: candidate.interaction_id,
                        source: source.to_string(),
                        event_id: event.event_id.clone(),
                        message_id: event.message_id.as_str().to_string(),
                        chat_id: event.chat_id.as_str().to_string(),
                        thread_id: event.thread_id.clone(),
                        sender_id: event.sender_id.as_str().to_string(),
                        response_artifact_id: artifact_id.to_string(),
                        received_at_ms: event.created_at_ms,
                    })
                    .await?,
            );
        }
        Ok(outcomes)
    }

    pub async fn reconcile_waiting(
        &self,
        interaction_id: InteractionId,
        now_ms: i64,
        cancelled: &AtomicBool,
    ) -> Result<HumanInteractionOutcome, LarkInteractionError> {
        self.refresh_outcome(interaction_id, now_ms, cancelled)
            .await
    }

    async fn ensure_interaction_records(
        &self,
        run_id: WorkflowRunId,
        interaction_id: InteractionId,
        effect_key: &str,
        request: &HumanInteractionRequest,
        token: &str,
        now_ms: i64,
    ) -> Result<(), LarkInteractionError> {
        let planned = self
            .store
            .plan_interaction(WorkflowInteractionPlan {
                interaction_id: interaction_id.to_string(),
                run_id: run_id.to_string(),
                dedupe_key: effect_key.to_string(),
                kind: "lark.reply".to_string(),
                request: json!({
                    "chat_id": request.chat_id.as_str(),
                    "thread_id": request.thread_id.as_ref().map(ThreadId::as_str),
                    "allowed_senders": request
                        .allowed_senders
                        .iter()
                        .map(OpenId::as_str)
                        .collect::<Vec<_>>(),
                    "correlation_token": token,
                }),
                deadline_ms: Some(request.deadline_ms),
                created_at_ms: now_ms,
            })
            .await?;
        let interaction = match planned {
            WorkflowInteractionPlanOutcome::Planned(record)
            | WorkflowInteractionPlanOutcome::Existing(record) => record,
        };
        self.store
            .plan_lark_interaction(WorkflowLarkInteractionPlan {
                interaction_id: interaction_id.to_string(),
                run_id: run_id.to_string(),
                effect_key: effect_key.to_string(),
                chat_id: request.chat_id.as_str().to_string(),
                thread_id: request
                    .thread_id
                    .as_ref()
                    .map(|thread| thread.as_str().to_string()),
                correlation_token: token.to_string(),
                allowed_senders: request
                    .allowed_senders
                    .iter()
                    .map(|sender| sender.as_str().to_string())
                    .collect(),
                watermark_ms: interaction.created_at_ms,
                created_at_ms: interaction.created_at_ms,
            })
            .await?;
        if interaction.state == WorkflowInteractionState::Planned {
            let _ = self
                .store
                .update_interaction(
                    &interaction.interaction_id,
                    WorkflowInteractionState::Planned,
                    WorkflowInteractionState::Waiting,
                    None,
                    now_ms,
                )
                .await?;
        }
        Ok(())
    }

    async fn complete_send(
        &self,
        run_id: &str,
        effect_key: &str,
        interaction_id: InteractionId,
        message_id: &str,
        now_ms: i64,
    ) -> Result<(), LarkInteractionError> {
        if !self
            .store
            .mark_lark_message_sent(&interaction_id.to_string(), message_id, now_ms)
            .await?
        {
            return Err(LarkInteractionError::ConcurrentMutation);
        }
        let _ = self
            .store
            .update_effect(WorkflowEffectUpdate {
                run_id: run_id.to_string(),
                effect_key: effect_key.to_string(),
                expected_state: WorkflowEffectState::Dispatched,
                state: WorkflowEffectState::Applied,
                response: Some(json!({
                    "interaction_id": interaction_id,
                    "message_id": message_id,
                })),
                error_code: None,
                updated_at_ms: now_ms,
            })
            .await?;
        Ok(())
    }

    async fn refresh_outcome(
        &self,
        interaction_id: InteractionId,
        now_ms: i64,
        cancelled: &AtomicBool,
    ) -> Result<HumanInteractionOutcome, LarkInteractionError> {
        let durable = self
            .store
            .read_interaction(&interaction_id.to_string())
            .await?
            .ok_or(LarkInteractionError::MissingInteraction)?;
        if matches!(
            durable.state,
            WorkflowInteractionState::Resolved
                | WorkflowInteractionState::TimedOut
                | WorkflowInteractionState::Cancelled
        ) {
            return interaction_outcome(interaction_id, &durable);
        }
        self.poll_replies(interaction_id, cancelled).await?;
        let _ = self
            .store
            .timeout_lark_interaction(&interaction_id.to_string(), now_ms)
            .await?;
        let interaction = self
            .store
            .read_interaction(&interaction_id.to_string())
            .await?
            .ok_or(LarkInteractionError::MissingInteraction)?;
        interaction_outcome(interaction_id, &interaction)
    }

    async fn write_reply_artifact(
        &self,
        run_id: &str,
        interaction_id: &str,
        event: &LarkEvent,
    ) -> Result<ArtifactId, LarkInteractionError> {
        let relative_path = PathBuf::from("interactions")
            .join(interaction_id)
            .join(format!("{}.json", event.message_id.as_str()));
        if let Some(existing) = self
            .store
            .list_artifacts(run_id)
            .await?
            .into_iter()
            .find(|artifact| artifact.relative_path == relative_path.to_string_lossy())
        {
            return Ok(ArtifactId::parse(&existing.artifact_id)?);
        }
        let run_id = WorkflowRunId::parse(run_id)?;
        let artifact_id = ArtifactId::new();
        let bytes = serde_json::to_vec(&json!({
            "message_id": event.message_id.as_str(),
            "chat_id": event.chat_id.as_str(),
            "thread_id": &event.thread_id,
            "sender_id": event.sender_id.as_str(),
            "created_at_ms": event.created_at_ms,
            "text": &event.text,
        }))?;
        self.artifacts
            .write(WorkflowArtifactWrite {
                run_id,
                artifact_id,
                relative_path: &relative_path,
                classification: ArtifactClassification::Internal,
                media_type: "application/json",
                bytes: &bytes,
                created_at_ms: event.created_at_ms,
            })
            .await?;
        Ok(artifact_id)
    }
}

fn interaction_outcome(
    interaction_id: InteractionId,
    interaction: &codex_state::WorkflowInteractionRecord,
) -> Result<HumanInteractionOutcome, LarkInteractionError> {
    Ok(match interaction.state {
        WorkflowInteractionState::Planned | WorkflowInteractionState::Waiting => {
            HumanInteractionOutcome::Waiting { interaction_id }
        }
        WorkflowInteractionState::Resolved => HumanInteractionOutcome::Resolved {
            interaction_id,
            artifact_id: ArtifactId::parse(
                interaction
                    .response_artifact_id
                    .as_deref()
                    .ok_or(LarkInteractionError::MissingArtifact)?,
            )?,
        },
        WorkflowInteractionState::TimedOut => HumanInteractionOutcome::TimedOut { interaction_id },
        WorkflowInteractionState::Cancelled => {
            HumanInteractionOutcome::Cancelled { interaction_id }
        }
    })
}

fn effect_interaction_id(response: &Option<Value>) -> Result<InteractionId, LarkInteractionError> {
    let value = response
        .as_ref()
        .and_then(|response| response.get("interaction_id"))
        .and_then(Value::as_str)
        .ok_or(LarkInteractionError::MissingInteraction)?;
    Ok(InteractionId::parse(value)?)
}

fn correlation_token(run_id: &str, effect_key: &str) -> String {
    let digest = Sha256::digest(format!("{run_id}\0{effect_key}\0lark-correlation"));
    format!("{digest:x}")[..24].to_string()
}

fn message_idempotency_key(run_id: &str, effect_key: &str) -> String {
    let digest = Sha256::digest(format!("{run_id}\0{effect_key}\0lark-send"));
    format!("wf-{}", &format!("{digest:x}")[..48])
}

#[derive(Debug, thiserror::Error)]
pub enum LarkInteractionError {
    #[error("invalid human interaction request")]
    InvalidRequest,
    #[error("durable Lark effect changed concurrently")]
    ConcurrentMutation,
    #[error("durable Lark effect was not found")]
    MissingEffect,
    #[error("durable Lark interaction was not found")]
    MissingInteraction,
    #[error("resolved Lark interaction has no response artifact")]
    MissingArtifact,
    #[error(transparent)]
    Lark(#[from] LarkCliError),
    #[error(transparent)]
    State(#[from] codex_state::WorkflowStoreError),
    #[error(transparent)]
    Artifact(#[from] WorkflowArtifactStoreError),
    #[error(transparent)]
    Identifier(#[from] crate::IdentifierError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
