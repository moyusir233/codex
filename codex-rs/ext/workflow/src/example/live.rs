use std::path::Path;
use std::sync::Arc;

use codex_state::WorkflowEffectPlan;
use codex_state::WorkflowEffectPlanOutcome;
use codex_state::WorkflowEffectState;
use codex_state::WorkflowEffectUpdate;
use serde_json::Value;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;
use tokio::task::JoinSet;

use crate::ArtifactClassification;
use crate::ArtifactId;
use crate::DependencyPolicy;
use crate::HumanInteractionOutcome;
use crate::HumanInteractionRequest;
use crate::NodeHandle;
use crate::NodeId;
use crate::NodeInput;
use crate::SkillSelector;
use crate::WorkflowArtifactStore;
use crate::WorkflowArtifactWrite;
use crate::WorkflowCancellation;
use crate::WorkflowError;
use crate::WorkflowRunId;
use crate::WorkflowService;
use crate::integrations::fornax::DurableFornaxTraceWriter;
use crate::integrations::fornax::FornaxCli;
use crate::integrations::fornax::FornaxSpanCorrelation;
use crate::integrations::fornax::FornaxTraceWriter;
use crate::integrations::fornax::FornaxWorkflowClient;
use crate::integrations::fornax::PromptDraft;
use crate::integrations::fornax::PromptLookup;
use crate::integrations::fornax::SpanParent;
use crate::integrations::fornax::SpanType;
use crate::integrations::fornax::digest_record;
use crate::integrations::fornax::render_normal_prompt;
use crate::integrations::fornax::safe_tags;
use crate::integrations::lark::ChatId;
use crate::integrations::lark::ContentSensitivity;
use crate::integrations::lark::DocumentCreateRequest;
use crate::integrations::lark::LarkIdentity;
use crate::integrations::lark::OpenId;
use crate::runtime::LarkInteractionService;

use super::PromptReviewArguments;
use super::PromptReviewCapability;
use super::PromptReviewCapabilityFuture;
use super::PromptReviewOutput;
use super::PromptReviewPrepared;
use super::PromptReviewReview;

mod capability;
mod support;
use support::*;

/// Explicit skill allow-lists used by the complete reference workflow.
#[derive(Clone)]
pub struct PromptReviewRuntimeConfig {
    pub planner_skills: Vec<SkillSelector>,
    pub reviewer_skills: Vec<Vec<SkillSelector>>,
    pub synthesizer_skills: Vec<SkillSelector>,
}

/// Already-constructed host services consumed by live preflight.
pub struct PromptReviewRuntimeDependencies {
    pub service: WorkflowService,
    pub fornax_cli: FornaxCli,
    pub trace_writer: FornaxTraceWriter,
    pub bridge_instance_id: String,
    pub lark: Arc<LarkInteractionService>,
    pub artifacts: WorkflowArtifactStore,
}

/// Concrete composition of the pinned node, artifact, Fornax, and Lark
/// capabilities consumed by `prompt-review`.
pub struct LivePromptReviewCapability {
    service: WorkflowService,
    fornax_cli: FornaxCli,
    trace_writer: FornaxTraceWriter,
    bridge_instance_id: String,
    lark: Arc<LarkInteractionService>,
    artifacts: WorkflowArtifactStore,
    config: PromptReviewRuntimeConfig,
}

impl LivePromptReviewCapability {
    /// Verifies exact CLI contracts before returning a live-capable bundle.
    ///
    /// `trace_writer` must have come from `FornaxTraceWriter::preflight`, which
    /// includes the exact bridge/protocol/SDK and explicit live-delivery gate.
    pub fn preflight(
        dependencies: PromptReviewRuntimeDependencies,
        config: PromptReviewRuntimeConfig,
        cancellation: &WorkflowCancellation,
    ) -> Result<Self, WorkflowError> {
        let PromptReviewRuntimeDependencies {
            service,
            fornax_cli,
            trace_writer,
            bridge_instance_id,
            lark,
            artifacts,
        } = dependencies;
        config.validate()?;
        fornax_cli
            .verify(cancellation.flag())
            .map_err(definition_error)?;
        lark.cli()
            .verify(cancellation.flag())
            .map_err(definition_error)?;
        if bridge_instance_id.trim().is_empty() {
            return Err(WorkflowError::definition(
                "Fornax bridge instance ID must not be empty",
            ));
        }
        Ok(Self {
            service,
            fornax_cli,
            trace_writer,
            bridge_instance_id,
            lark,
            artifacts,
            config,
        })
    }

    fn fornax(&self, run_id: WorkflowRunId) -> FornaxWorkflowClient {
        let durable = DurableFornaxTraceWriter::new(
            self.trace_writer.clone(),
            self.service.store().clone(),
            run_id.to_string(),
            self.bridge_instance_id.clone(),
        );
        FornaxWorkflowClient::new(self.fornax_cli.clone(), Some(durable))
    }

    async fn write_artifact(
        &self,
        run_id: WorkflowRunId,
        relative_path: &Path,
        media_type: &str,
        bytes: &[u8],
    ) -> Result<ArtifactId, WorkflowError> {
        if let Some(existing) = self
            .service
            .store()
            .list_artifacts(&run_id.to_string())
            .await
            .map_err(definition_error)?
            .into_iter()
            .find(|artifact| artifact.relative_path == relative_path.to_string_lossy())
        {
            return ArtifactId::parse(&existing.artifact_id).map_err(definition_error);
        }
        let artifact_id = ArtifactId::new();
        self.artifacts
            .write(WorkflowArtifactWrite {
                run_id,
                artifact_id,
                relative_path,
                classification: ArtifactClassification::Internal,
                media_type,
                bytes,
                created_at_ms: now_ms()?,
            })
            .await
            .map_err(definition_error)?;
        Ok(artifact_id)
    }

    async fn ensure_node(
        &self,
        run_id: WorkflowRunId,
        key: &str,
        skills: Vec<SkillSelector>,
    ) -> Result<NodeHandle, WorkflowError> {
        self.service
            .nodes(run_id)
            .ensure(
                effect(&format!("node.ensure.{key}"))?,
                node_spec(key, skills)?,
            )
            .await
            .map_err(definition_error)
    }

    async fn run_node_turn(
        &self,
        node: NodeHandle,
        key: &str,
        prompt: String,
    ) -> Result<(NodeId, String, String), WorkflowError> {
        let submitted = node
            .start(
                effect(&format!("node.turn.{key}"))?,
                NodeInput::text(prompt),
            )
            .await
            .map_err(definition_error)?;
        let result = node
            .await_turn(submitted.turn_id)
            .await
            .map_err(definition_error)?;
        let output = completed_output(&result)?;
        Ok((node.id(), node.thread_id().to_string(), output))
    }

    async fn save_draft(
        &self,
        run_id: WorkflowRunId,
        prompt_id: &str,
        draft: &PromptDraft,
        cancellation: &WorkflowCancellation,
    ) -> Result<(), WorkflowError> {
        let effect_key = "fornax.prompt.draft.save";
        let planned = self
            .service
            .store()
            .plan_effect(WorkflowEffectPlan {
                run_id: run_id.to_string(),
                effect_key: effect_key.to_string(),
                kind: "fornax.prompt.draft.save".to_string(),
                request: json!({
                    "prompt_id": prompt_id,
                    "draft_sha256": format!("{:x}", Sha256::digest(
                        serde_json::to_vec(draft.as_value()).map_err(definition_error)?
                    )),
                }),
                created_at_ms: now_ms()?,
            })
            .await
            .map_err(definition_error)?;
        let mut record = match planned {
            WorkflowEffectPlanOutcome::Planned(record)
            | WorkflowEffectPlanOutcome::Existing(record) => record,
        };
        if record.state == WorkflowEffectState::Applied {
            return Ok(());
        }
        if record.state == WorkflowEffectState::Ambiguous {
            return Err(WorkflowError::definition(
                "Fornax draft save is ambiguous and needs operator reconciliation",
            ));
        }
        if record.state == WorkflowEffectState::Planned {
            self.service
                .store()
                .update_effect(WorkflowEffectUpdate {
                    run_id: run_id.to_string(),
                    effect_key: effect_key.to_string(),
                    expected_state: WorkflowEffectState::Planned,
                    state: WorkflowEffectState::Dispatched,
                    response: None,
                    error_code: None,
                    updated_at_ms: now_ms()?,
                })
                .await
                .map_err(definition_error)?;
            record.state = WorkflowEffectState::Dispatched;
        }
        match self
            .fornax_cli
            .save_prompt_draft(prompt_id, draft, cancellation.flag())
        {
            Ok(response) => {
                self.service
                    .store()
                    .update_effect(WorkflowEffectUpdate {
                        run_id: run_id.to_string(),
                        effect_key: effect_key.to_string(),
                        expected_state: WorkflowEffectState::Dispatched,
                        state: WorkflowEffectState::Applied,
                        response: Some(response),
                        error_code: None,
                        updated_at_ms: now_ms()?,
                    })
                    .await
                    .map_err(definition_error)?;
                Ok(())
            }
            Err(error) => {
                self.service
                    .store()
                    .update_effect(WorkflowEffectUpdate {
                        run_id: run_id.to_string(),
                        effect_key: effect_key.to_string(),
                        expected_state: WorkflowEffectState::Dispatched,
                        state: WorkflowEffectState::Ambiguous,
                        response: None,
                        error_code: Some("fornax_draft_save_ambiguous".to_string()),
                        updated_at_ms: now_ms()?,
                    })
                    .await
                    .map_err(definition_error)?;
                Err(definition_error(error))
            }
        }
    }

    async fn create_review_document(
        &self,
        run_id: WorkflowRunId,
        synthesis: &str,
        cancellation: &WorkflowCancellation,
    ) -> Result<String, WorkflowError> {
        let effect_key = "lark.review.document.create";
        let title = format!("Codex prompt review {run_id}");
        let planned = self
            .service
            .store()
            .plan_effect(WorkflowEffectPlan {
                run_id: run_id.to_string(),
                effect_key: effect_key.to_string(),
                kind: effect_key.to_string(),
                request: json!({
                    "title": &title,
                    "markdown_sha256": format!("{:x}", Sha256::digest(synthesis.as_bytes())),
                    "classification": "non_sensitive",
                }),
                created_at_ms: now_ms()?,
            })
            .await
            .map_err(definition_error)?;
        let record = match planned {
            WorkflowEffectPlanOutcome::Planned(record)
            | WorkflowEffectPlanOutcome::Existing(record) => record,
        };
        if record.state == WorkflowEffectState::Applied {
            return record
                .response
                .as_ref()
                .and_then(|response| response.get("doc_id"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| {
                    WorkflowError::definition("applied Lark document effect has no document ID")
                });
        }
        if record.state != WorkflowEffectState::Planned {
            return Err(WorkflowError::definition(
                "Lark document create is ambiguous and needs operator reconciliation",
            ));
        }
        if !self
            .service
            .store()
            .update_effect(WorkflowEffectUpdate {
                run_id: run_id.to_string(),
                effect_key: effect_key.to_string(),
                expected_state: WorkflowEffectState::Planned,
                state: WorkflowEffectState::Dispatched,
                response: None,
                error_code: None,
                updated_at_ms: now_ms()?,
            })
            .await
            .map_err(definition_error)?
        {
            return Err(WorkflowError::definition(
                "Lark document effect changed concurrently",
            ));
        }
        match self.lark.cli().create_document(
            &DocumentCreateRequest {
                identity: LarkIdentity::Bot,
                title: Some(title),
                markdown: synthesis.to_string(),
                parent: None,
                sensitivity: ContentSensitivity::NonSensitive,
            },
            cancellation.flag(),
        ) {
            Ok(document) => {
                self.service
                    .store()
                    .update_effect(WorkflowEffectUpdate {
                        run_id: run_id.to_string(),
                        effect_key: effect_key.to_string(),
                        expected_state: WorkflowEffectState::Dispatched,
                        state: WorkflowEffectState::Applied,
                        response: Some(json!({
                            "doc_id": document.doc_id,
                            "doc_url": document.doc_url,
                        })),
                        error_code: None,
                        updated_at_ms: now_ms()?,
                    })
                    .await
                    .map_err(definition_error)?;
                Ok(document.doc_id)
            }
            Err(error) => {
                self.service
                    .store()
                    .update_effect(WorkflowEffectUpdate {
                        run_id: run_id.to_string(),
                        effect_key: effect_key.to_string(),
                        expected_state: WorkflowEffectState::Dispatched,
                        state: WorkflowEffectState::Ambiguous,
                        response: None,
                        error_code: Some("lark_document_create_ambiguous".to_string()),
                        updated_at_ms: now_ms()?,
                    })
                    .await
                    .map_err(definition_error)?;
                Err(definition_error(error))
            }
        }
    }
}
