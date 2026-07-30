use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use codex_state::WorkflowEffectPlan;
use codex_state::WorkflowEffectPlanOutcome;
use codex_state::WorkflowEffectState;
use codex_state::WorkflowEffectUpdate;
use serde_json::Value;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;
use tokio::task::JoinSet;
use uuid::Uuid;

use crate::ArtifactClassification;
use crate::ArtifactId;
use crate::DependencyPolicy;
use crate::EffectKey;
use crate::HumanInteractionOutcome;
use crate::HumanInteractionRequest;
use crate::NodeApprovals;
use crate::NodeId;
use crate::NodeInput;
use crate::NodeKey;
use crate::NodeOutputClassification;
use crate::NodeSpec;
use crate::NodeTurnResult;
use crate::NodeTurnStatus;
use crate::SkillPolicy;
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

impl PromptReviewRuntimeConfig {
    fn validate(&self) -> Result<(), WorkflowError> {
        if self.planner_skills.is_empty()
            || self.synthesizer_skills.is_empty()
            || self.reviewer_skills.len() != 3
            || self.reviewer_skills.iter().any(Vec::is_empty)
            || self.reviewer_skills[0] == self.reviewer_skills[1]
            || self.reviewer_skills[0] == self.reviewer_skills[2]
            || self.reviewer_skills[1] == self.reviewer_skills[2]
        {
            return Err(WorkflowError::definition(
                "prompt-review requires non-empty planner/synthesizer skills and three distinct reviewer allow-lists",
            ));
        }
        Ok(())
    }
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

    async fn run_turn(
        &self,
        run_id: WorkflowRunId,
        key: &str,
        skills: Vec<SkillSelector>,
        prompt: String,
    ) -> Result<(NodeId, String, String), WorkflowError> {
        let spec = node_spec(key, skills)?;
        let node = self
            .service
            .nodes(run_id)
            .ensure(effect(&format!("node.ensure.{key}"))?, spec)
            .await
            .map_err(definition_error)?;
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

impl PromptReviewCapability for LivePromptReviewCapability {
    fn prepare<'a>(
        &'a self,
        run_id: WorkflowRunId,
        args: &'a PromptReviewArguments,
        cancellation: &'a WorkflowCancellation,
    ) -> PromptReviewCapabilityFuture<'a, PromptReviewPrepared> {
        Box::pin(async move {
            let fornax = self.fornax(run_id);
            let prompt = fornax
                .prompts()
                .get_prompt(
                    &PromptLookup::Key {
                        key: args.prompt_key.clone(),
                        version: args.prompt_version.clone(),
                        with_draft: true,
                        commit_version: args.prompt_version.clone(),
                    },
                    cancellation.flag(),
                )
                .map_err(definition_error)?;
            let draft_value = prompt
                .draft
                .ok_or_else(|| WorkflowError::definition("Fornax prompt has no complete draft"))?;
            let draft = PromptDraft::new(draft_value.clone()).map_err(definition_error)?;
            let variables = render_variables(&draft_value, &args.prompt_key);
            let messages = render_normal_prompt(&draft, &variables).map_err(definition_error)?;
            let rendered = messages
                .into_iter()
                .map(|message| format!("{}: {}", message.role, message.content))
                .collect::<Vec<_>>()
                .join("\n\n");
            let draft_bytes = serde_json::to_vec(&draft_value).map_err(definition_error)?;
            let prompt_draft_artifact_id = self
                .write_artifact(
                    run_id,
                    Path::new("prompt/source-draft.json"),
                    "application/json",
                    &draft_bytes,
                )
                .await?;
            let rendered_prompt_artifact_id = self
                .write_artifact(
                    run_id,
                    Path::new("prompt/rendered.txt"),
                    "text/plain; charset=utf-8",
                    rendered.as_bytes(),
                )
                .await?;
            let writer = fornax.trace_writer().map_err(definition_error)?;
            let started = writer
                .start(
                    "fornax.trace.root.start",
                    operation_uuid(run_id, "root.start"),
                    "codex.prompt-review",
                    SpanType::Root,
                    SpanParent::NewTrace,
                    now_ms()?,
                )
                .await
                .map_err(definition_error)?;
            let correlation = FornaxSpanCorrelation::from(&started);
            writer
                .record(
                    "fornax.trace.root.prompt-digest",
                    &correlation,
                    operation_uuid(run_id, "root.prompt-digest"),
                    digest_record(rendered.as_bytes(), "internal"),
                    now_ms()?,
                )
                .await
                .map_err(definition_error)?;
            Ok(PromptReviewPrepared {
                prompt_id: prompt.prompt_id,
                prompt_version: prompt.version,
                prompt_draft_artifact_id,
                rendered_prompt_artifact_id,
                span_handle_id: started.span_handle_id.to_string(),
                trace_context_id: started.trace_context_id.to_string(),
                trace_id: started.trace_id,
                root_span_id: started.span_id,
            })
        })
    }

    fn review<'a>(
        &'a self,
        run_id: WorkflowRunId,
        args: &'a PromptReviewArguments,
        prepared: &'a PromptReviewPrepared,
        cancellation: &'a WorkflowCancellation,
    ) -> PromptReviewCapabilityFuture<'a, PromptReviewReview> {
        Box::pin(async move {
            let rendered = self
                .artifacts
                .read(run_id, prepared.rendered_prompt_artifact_id)
                .await
                .map_err(definition_error)?;
            let rendered = String::from_utf8(rendered).map_err(definition_error)?;
            let (planner_node_id, planner_thread_id, plan) = self
                .run_turn(
                    run_id,
                    "planner",
                    self.config.planner_skills.clone(),
                    format!("Plan a rigorous review of this rendered prompt:\n\n{rendered}"),
                )
                .await?;

            let mut pending = JoinSet::new();
            for index in 0..usize::from(args.reviewers) {
                let capability = self.clone_for_task();
                let reviewer_prompt = format!(
                    "Reviewer {}: independently critique the prompt using this plan.\n\nPLAN:\n{}\n\nPROMPT:\n{}",
                    index + 1,
                    plan,
                    rendered
                );
                let skills = self.config.reviewer_skills[index].clone();
                pending.spawn(async move {
                    capability
                        .run_turn(
                            run_id,
                            &format!("reviewer-{}", index + 1),
                            skills,
                            reviewer_prompt,
                        )
                        .await
                });
            }
            let mut reviewers = Vec::new();
            while let Some(result) = pending.join_next().await {
                reviewers.push(result.map_err(|error| {
                    WorkflowError::definition(format!("reviewer task failed to join: {error}"))
                })??);
            }
            reviewers.sort_by(|left, right| left.1.cmp(&right.1));
            let review_text = reviewers
                .iter()
                .enumerate()
                .map(|(index, (_, _, output))| format!("REVIEW {}:\n{}", index + 1, output))
                .collect::<Vec<_>>()
                .join("\n\n");
            let (synthesizer_node_id, synthesizer_thread_id, synthesis) = self
                .run_turn(
                    run_id,
                    "synthesizer",
                    self.config.synthesizer_skills.clone(),
                    format!(
                        "Synthesize these independent reviews into a proposed final prompt and rationale.\n\n{review_text}"
                    ),
                )
                .await?;
            let nodes = self.service.nodes(run_id);
            for (index, (reviewer_id, _, _)) in reviewers.iter().enumerate() {
                nodes
                    .add_dependency(
                        effect(&format!("dependency.planner.reviewer-{}", index + 1))?,
                        planner_node_id,
                        *reviewer_id,
                        DependencyPolicy::AllSucceeded,
                    )
                    .await
                    .map_err(definition_error)?;
                nodes
                    .add_dependency(
                        effect(&format!("dependency.reviewer-{}.synthesizer", index + 1))?,
                        *reviewer_id,
                        synthesizer_node_id,
                        DependencyPolicy::AllSucceeded,
                    )
                    .await
                    .map_err(definition_error)?;
            }
            let synthesis_artifact_id = self
                .write_artifact(
                    run_id,
                    Path::new("review/synthesis.txt"),
                    "text/plain; charset=utf-8",
                    synthesis.as_bytes(),
                )
                .await?;
            let lark_document_id = self
                .create_review_document(run_id, &synthesis, cancellation)
                .await?;
            let fornax = self.fornax(run_id);
            let writer = fornax.trace_writer().map_err(definition_error)?;
            let correlation = correlation(prepared)?;
            writer
                .record(
                    "fornax.trace.root.review-tags",
                    &correlation,
                    operation_uuid(run_id, "root.review-tags"),
                    safe_tags([
                        ("reviewerCount".to_string(), Value::from(args.reviewers)),
                        (
                            "synthesisArtifactSha256".to_string(),
                            Value::String(format!("{:x}", Sha256::digest(synthesis.as_bytes()))),
                        ),
                    ]),
                    now_ms()?,
                )
                .await
                .map_err(definition_error)?;
            Ok(PromptReviewReview {
                planner_node_id: planner_node_id.to_string(),
                planner_thread_id,
                reviewer_node_ids: reviewers.iter().map(|(id, _, _)| id.to_string()).collect(),
                reviewer_thread_ids: reviewers
                    .iter()
                    .map(|(_, thread, _)| thread.clone())
                    .collect(),
                synthesizer_node_id: synthesizer_node_id.to_string(),
                synthesizer_thread_id,
                synthesis_artifact_id,
                lark_document_id: Some(lark_document_id),
            })
        })
    }

    fn request_human<'a>(
        &'a self,
        run_id: WorkflowRunId,
        args: &'a PromptReviewArguments,
        review: &'a PromptReviewReview,
        cancellation: &'a WorkflowCancellation,
    ) -> PromptReviewCapabilityFuture<'a, HumanInteractionOutcome> {
        Box::pin(async move {
            let deadline_delta = i64::try_from(args.reply_timeout_ms)
                .map_err(|_| WorkflowError::definition("reply timeout exceeds i64"))?;
            self.lark
                .poll_or_request(
                    run_id,
                    HumanInteractionRequest {
                        effect_key: effect("lark.review.request")?,
                        prompt: format!(
                            "Review the synthesized prompt in artifact {} and reply with approval or corrections.",
                            review.synthesis_artifact_id
                        ),
                        chat_id: ChatId::parse(&args.lark_chat_id).map_err(definition_error)?,
                        thread_id: None,
                        allowed_senders: args
                            .lark_users
                            .iter()
                            .map(|user| OpenId::parse(user).map_err(definition_error))
                            .collect::<Result<Vec<_>, _>>()?,
                        deadline_ms: now_ms()?.saturating_add(deadline_delta),
                        sensitivity: ContentSensitivity::NonSensitive,
                    },
                    now_ms()?,
                    cancellation.flag(),
                )
                .await
                .map_err(definition_error)
        })
    }

    fn follow_up_and_save<'a>(
        &'a self,
        run_id: WorkflowRunId,
        _args: &'a PromptReviewArguments,
        prepared: &'a PromptReviewPrepared,
        review: &'a PromptReviewReview,
        reply_artifact_id: ArtifactId,
        cancellation: &'a WorkflowCancellation,
    ) -> PromptReviewCapabilityFuture<'a, PromptReviewOutput> {
        Box::pin(async move {
            let reply = self
                .artifacts
                .read(run_id, reply_artifact_id)
                .await
                .map_err(definition_error)?;
            let reply = String::from_utf8(reply).map_err(definition_error)?;
            let synthesizer = self
                .service
                .nodes(run_id)
                .get(NodeId::parse(&review.synthesizer_node_id).map_err(definition_error)?)
                .await
                .map_err(definition_error)?;
            let submitted = synthesizer
                .start(
                    effect("node.turn.synthesizer-human-follow-up")?,
                    NodeInput::text(format!(
                        "Apply this correlated human review and return the final prompt:\n\n{reply}"
                    )),
                )
                .await
                .map_err(definition_error)?;
            let final_result = synthesizer
                .await_turn(submitted.turn_id)
                .await
                .map_err(definition_error)?;
            let final_output = completed_output(&final_result)?;
            let final_artifact_id = self
                .write_artifact(
                    run_id,
                    Path::new("review/final.txt"),
                    "text/plain; charset=utf-8",
                    final_output.as_bytes(),
                )
                .await?;
            let draft_bytes = self
                .artifacts
                .read(run_id, prepared.prompt_draft_artifact_id)
                .await
                .map_err(definition_error)?;
            let draft_value = serde_json::from_slice(&draft_bytes).map_err(definition_error)?;
            let draft = PromptDraft::new(draft_value).map_err(definition_error)?;
            self.save_draft(run_id, &prepared.prompt_id, &draft, cancellation)
                .await?;
            let fornax = self.fornax(run_id);
            let writer = fornax.trace_writer().map_err(definition_error)?;
            let correlation = correlation(prepared)?;
            writer
                .record(
                    "fornax.trace.root.final-digest",
                    &correlation,
                    operation_uuid(run_id, "root.final-digest"),
                    digest_record(final_output.as_bytes(), "internal"),
                    now_ms()?,
                )
                .await
                .map_err(definition_error)?;
            writer
                .finish(
                    "fornax.trace.root.finish",
                    &correlation,
                    operation_uuid(run_id, "root.finish"),
                    now_ms()?,
                )
                .await
                .map_err(definition_error)?;
            let resume_commands = std::iter::once(&review.planner_thread_id)
                .chain(review.reviewer_thread_ids.iter())
                .chain(std::iter::once(&review.synthesizer_thread_id))
                .map(|thread| format!("codex resume {thread}"))
                .collect();
            Ok(PromptReviewOutput {
                prompt_id: prepared.prompt_id.clone(),
                prompt_version: prepared.prompt_version.clone(),
                planner_node_id: review.planner_node_id.clone(),
                planner_thread_id: review.planner_thread_id.clone(),
                reviewer_node_ids: review.reviewer_node_ids.clone(),
                reviewer_thread_ids: review.reviewer_thread_ids.clone(),
                synthesizer_node_id: review.synthesizer_node_id.clone(),
                synthesizer_thread_id: review.synthesizer_thread_id.clone(),
                trace_id: prepared.trace_id.clone(),
                root_span_id: prepared.root_span_id.clone(),
                lark_document_id: review.lark_document_id.clone(),
                human_reply_artifact_id: reply_artifact_id.to_string(),
                draft_saved: true,
                final_artifact_ids: vec![final_artifact_id.to_string()],
                resume_commands,
            })
        })
    }
}

impl LivePromptReviewCapability {
    fn clone_for_task(&self) -> Arc<Self> {
        Arc::new(Self {
            service: self.service.clone(),
            fornax_cli: self.fornax_cli.clone(),
            trace_writer: self.trace_writer.clone(),
            bridge_instance_id: self.bridge_instance_id.clone(),
            lark: Arc::clone(&self.lark),
            artifacts: self.artifacts.clone(),
            config: self.config.clone(),
        })
    }
}

fn node_spec(key: &str, skills: Vec<SkillSelector>) -> Result<NodeSpec, WorkflowError> {
    NodeSpec::builder(NodeKey::new(key).map_err(definition_error)?)
        .skills(SkillPolicy::AllowOnly(skills))
        .approvals(NodeApprovals::RejectWhenDetached)
        .output_classification(NodeOutputClassification::Internal)
        .build()
        .map_err(definition_error)
}

fn completed_output(result: &NodeTurnResult) -> Result<String, WorkflowError> {
    if result.status != NodeTurnStatus::Completed {
        return Err(WorkflowError::definition(format!(
            "node turn {} ended as {:?}: {}",
            result.turn_id,
            result.status,
            result.error.as_deref().unwrap_or("no diagnostic")
        )));
    }
    result
        .final_output
        .clone()
        .ok_or_else(|| WorkflowError::definition("completed node turn returned no final output"))
}

fn render_variables(draft: &Value, prompt_key: &str) -> BTreeMap<String, String> {
    draft["detail"]["prompt_template"]["variable_defs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|definition| definition.get("key").and_then(Value::as_str))
        .map(|key| {
            let value = match key {
                "name" => "Codex prompt reviewer".to_string(),
                "topic" => prompt_key.to_string(),
                _ => String::new(),
            };
            (key.to_string(), value)
        })
        .collect()
}

fn correlation(prepared: &PromptReviewPrepared) -> Result<FornaxSpanCorrelation, WorkflowError> {
    Ok(FornaxSpanCorrelation {
        span_handle_id: Uuid::parse_str(&prepared.span_handle_id).map_err(definition_error)?,
        trace_context_id: Uuid::parse_str(&prepared.trace_context_id).map_err(definition_error)?,
        trace_id: prepared.trace_id.clone(),
        span_id: prepared.root_span_id.clone(),
    })
}

fn operation_uuid(run_id: WorkflowRunId, key: &str) -> Uuid {
    let digest = Sha256::digest(format!("{run_id}:{key}").as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn effect(value: &str) -> Result<EffectKey, WorkflowError> {
    EffectKey::new(value).map_err(definition_error)
}

fn now_ms() -> Result<i64, WorkflowError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(definition_error)?
        .as_millis();
    i64::try_from(millis).map_err(definition_error)
}

fn definition_error(error: impl std::fmt::Display) -> WorkflowError {
    WorkflowError::definition(error.to_string())
}
