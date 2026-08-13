use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::num::NonZeroU32;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use codex_protocol::ThreadId;
use codex_protocol::protocol::SandboxPolicy;
use codex_utils_absolute_path::AbsolutePathBuf;
use serde::Serialize;
use serde_json::Value;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;

use crate::ApprovalOutcome;
use crate::ApprovalRequest;
use crate::ApprovalSubject;
use crate::ArtifactClassification;
use crate::ArtifactId;
use crate::BackoffPolicy;
use crate::EffectKey;
use crate::FailurePolicy;
use crate::HumanInteractionOutcome;
use crate::NodeApprovals;
use crate::NodeCollaborationMode;
use crate::NodeInput;
use crate::NodeKey;
use crate::NodeOutputClassification;
use crate::NodeSandbox;
use crate::NodeSpec;
use crate::NodeWorkingDirectory;
use crate::RetryClassification;
use crate::RetryPolicy;
use crate::RetrySession;
use crate::SkillAuthoritySelector;
use crate::SkillInitialInvocation;
use crate::SkillPackageSelector;
use crate::SkillPolicy;
use crate::SkillSelector;
use crate::WakeCondition;
use crate::Workflow;
use crate::WorkflowArtifactClientWrite;
use crate::WorkflowContext;
use crate::WorkflowError;
use crate::WorkflowGoalDeliveryEvidence;
use crate::WorkflowMetadata;
use crate::WorkflowName;
use crate::WorkflowTransition;
use crate::WorkflowVersion;
use crate::integrations::lark::ChatId;
use crate::integrations::lark::OpenId;

use super::ApprovalEvidenceRef;
use super::LarkFeatureArguments;
use super::LarkFeatureProgress;
use super::LarkFeatureStageId;
use super::LarkFeatureState;
use super::LarkFeatureWorkflowOutput;
use super::StageHandoffV1;
use super::StageResultV1;
use super::StageRunRecord;
use super::prompts::bundled_prompt;
use super::prompts::digest;
use super::prompts::render_prompt;
use super::validation::approval_subject;
use super::validation::execution_evidence;
use super::validation::requested_back_edge;
use super::validation::validate_envelope;
use super::validation::validate_requested_transition;

pub struct LarkFeatureWorkflow;

impl Workflow for LarkFeatureWorkflow {
    type Arguments = LarkFeatureArguments;
    type State = LarkFeatureState;
    type Output = LarkFeatureWorkflowOutput;

    fn metadata(&self) -> WorkflowMetadata {
        WorkflowMetadata::new(
            WorkflowName::new("lark-rust-sdk-feature-development")
                .unwrap_or_else(|error| panic!("static workflow name is invalid: {error}")),
            WorkflowVersion::parse("1.0.0")
                .unwrap_or_else(|error| panic!("static workflow version is invalid: {error}")),
            "Develops one Lark Rust SDK feature through independent requirements, design, plan and execution threads with three revision-bound gates.",
        )
        .with_default(true)
        .with_required_capabilities([
            "codex.nodes",
            "codex.skills.explicit",
            "workflow.artifacts",
            "workflow.approvals.revision-bound",
            "workflow.goals.run-bound",
            "lark.feature.preflight-group.v1",
            "lark.grilling-transport.v1",
            "lark.documents.single-writer-revision-aware.v1",
            "fornax.spans.v2",
        ])
    }

    fn initialize(&self, args: Self::Arguments) -> Result<Self::State, WorkflowError> {
        Ok(LarkFeatureState::Preflight { args })
    }

    async fn step(
        &self,
        ctx: WorkflowContext<'_>,
        state: Self::State,
    ) -> Result<WorkflowTransition<Self::State, Self::Output>, WorkflowError> {
        match state {
            LarkFeatureState::Preflight { args } => preflight(ctx, args).await,
            LarkFeatureState::StageReady {
                progress,
                stage,
                attempt,
                reply_artifact_id,
            } => run_stage(ctx, progress, stage, attempt, reply_artifact_id).await,
            LarkFeatureState::StageQuestions {
                progress,
                stage,
                attempt,
                node_id,
                thread_id,
                handoff_artifact_id,
                handoff_sha256,
                questions,
                interaction_id: _,
            } => {
                resume_stage_questions(
                    ctx,
                    progress,
                    stage,
                    attempt,
                    node_id,
                    thread_id,
                    handoff_artifact_id,
                    handoff_sha256,
                    questions,
                )
                .await
            }
            LarkFeatureState::Approval {
                progress,
                stage,
                attempt,
                stage_record,
                approval_id: _,
                deadline_ms,
            } => poll_approval(ctx, progress, stage, attempt, stage_record, deadline_ms).await,
            LarkFeatureState::DeliveryValidation {
                progress,
                execution,
            } => validate_delivery(ctx, progress, execution).await,
        }
    }
}

async fn preflight(
    ctx: WorkflowContext<'_>,
    args: LarkFeatureArguments,
) -> Result<WorkflowTransition<LarkFeatureState, LarkFeatureWorkflowOutput>, WorkflowError> {
    require_runtime_facets(&ctx)?;
    let capability = ctx
        .lark_feature()
        .ok_or_else(|| unavailable("Lark feature adapter"))?;
    let bootstrap = capability
        .preflight_and_group(ctx.run_id(), &args, ctx.cancellation())
        .await?;
    if !bootstrap.chat_id.starts_with("oc_")
        || !valid_sha256(&bootstrap.group_owner_marker_sha256)
        || bootstrap.required_member_ids.is_empty()
        || bootstrap
            .required_member_ids
            .iter()
            .any(|member| !member.starts_with("ou_"))
        || !valid_lower_hex(&bootstrap.trace_id, 32)
        || !valid_lower_hex(&bootstrap.root_span_id, 16)
        || uuid::Uuid::parse_str(&bootstrap.trace_context_id).is_err()
        || !matches!(
            bootstrap.observability_delivery_state.as_str(),
            "delivered_remote" | "accepted_local" | "degraded_pending_backfill"
        )
    {
        return Err(WorkflowError::definition(
            "preflight/group adapter returned an invalid reconciled group",
        ));
    }
    Ok(WorkflowTransition::Continue {
        state: LarkFeatureState::StageReady {
            progress: LarkFeatureProgress {
                args,
                requirement_generation: 1,
                bootstrap,
                stages: Vec::new(),
                approvals: Vec::new(),
            },
            stage: LarkFeatureStageId::Requirements,
            attempt: 1,
            reply_artifact_id: None,
        },
    })
}

async fn run_stage(
    ctx: WorkflowContext<'_>,
    progress: LarkFeatureProgress,
    stage: LarkFeatureStageId,
    attempt: u32,
    reply_artifact_id: Option<ArtifactId>,
) -> Result<WorkflowTransition<LarkFeatureState, LarkFeatureWorkflowOutput>, WorkflowError> {
    let nodes = ctx.nodes().ok_or_else(|| unavailable("node client"))?;
    let artifacts = ctx
        .artifacts()
        .ok_or_else(|| unavailable("artifact client"))?;
    let spec = lark_feature_node_spec(stage, &progress.args)?;
    let node = nodes
        .ensure(
            effect(
                progress.requirement_generation,
                stage,
                attempt,
                "node-ensure",
            )?,
            spec,
        )
        .await
        .map_err(definition_error)?;
    let stage_attempt_id = stage_attempt_id(progress.requirement_generation, stage, attempt);
    let prompt = bundled_prompt(stage);
    let handoff = StageHandoffV1 {
        schema_version: 1,
        workflow_run_id: ctx.run_id(),
        requirement_id: progress.args.requirement_id.clone(),
        requirement_generation: progress.requirement_generation,
        stage_id: stage,
        stage_attempt_id: stage_attempt_id.clone(),
        prompt: prompt.snapshot(),
        owning_thread_id: node.thread_id().to_string(),
        participant_ids: participants(&progress),
        source_artifact_ids: vec![
            progress.bootstrap.source_manifest_artifact_id,
            progress.bootstrap.instruction_ledger_artifact_id,
        ],
        approved_inputs: progress.approvals.clone(),
        repository_path: progress.args.repository_path.display().to_string(),
        worktree_path: progress.args.worktree_path.display().to_string(),
        branch: progress.args.branch.clone(),
        base_commit: progress.args.base_commit.clone(),
        parent_trace_id: progress.bootstrap.trace_id.clone(),
        parent_span_id: progress.bootstrap.root_span_id.clone(),
        trace_context_id: progress.bootstrap.trace_context_id.clone(),
        approval_policy_id: "lark-feature-explicit-v1".to_string(),
        security_profile_id: "lark-feature-redacted-v1".to_string(),
        retry_profile_id: "lark-feature-bounded-v1".to_string(),
        tool_allowlist_id: format!("lark-feature-{}-v1", stage.as_str()),
    };
    let handoff_bytes = serde_json::to_vec_pretty(&handoff)
        .map_err(|error| WorkflowError::Serialization(error.to_string()))?;
    let handoff_artifact_id =
        deterministic_artifact(ctx.run_id(), &format!("{stage_attempt_id}/input-handoff"))?;
    let handoff_path = PathBuf::from(format!(
        "lark-feature/{}/{}/input-handoff-{attempt}.json",
        progress.requirement_generation,
        stage.node_key()
    ));
    let handoff_meta = artifacts
        .write(WorkflowArtifactClientWrite {
            artifact_id: handoff_artifact_id,
            relative_path: &handoff_path,
            classification: ArtifactClassification::Sensitive,
            media_type: "application/json",
            bytes: &handoff_bytes,
            created_at_ms: now_ms()?,
        })
        .await
        .map_err(definition_error)?;
    let handoff_json = String::from_utf8(handoff_bytes)
        .map_err(|error| WorkflowError::Serialization(error.to_string()))?;
    let prompt_variables = prompt_variables(&progress, stage, &handoff, prompt.output_schema)?;
    let is_initial_turn = attempt == 1;
    let text = match (reply_artifact_id, is_initial_turn) {
        (None, _) => render_prompt(&prompt, &prompt_variables, &handoff_json)
            .map_err(WorkflowError::definition)?,
        (Some(artifact_id), true) => format!(
            "{}\n\nA durable approval/change artifact triggered this new generation: {artifact_id}. Read it through the allowed artifact capability before acting.",
            render_prompt(&prompt, &prompt_variables, &handoff_json)
                .map_err(WorkflowError::definition)?
        ),
        (Some(artifact_id), false) => format!(
            "Resume the same stage thread. The durable Lark answer or gate feedback is artifact {artifact_id}; read it through the allowed artifact capability, update the question/feedback ledger, and return the complete structured stage result."
        ),
    };
    let schema = serde_json::from_str::<Value>(prompt.output_schema)
        .map_err(|error| WorkflowError::Serialization(error.to_string()))?;
    let input = NodeInput {
        items: NodeInput::text(text).items,
        final_output_json_schema: Some(schema),
        responsesapi_client_metadata: Some(BTreeMap::from([
            ("workflow.run_id".to_string(), ctx.run_id().to_string()),
            (
                "workflow.requirement_id".to_string(),
                progress.args.requirement_id.clone(),
            ),
            (
                "workflow.requirement_generation".to_string(),
                progress.requirement_generation.to_string(),
            ),
            ("workflow.stage_id".to_string(), stage.as_str().to_string()),
            (
                "workflow.stage_attempt_id".to_string(),
                stage_attempt_id.clone(),
            ),
            (
                "workflow.prompt_sha256".to_string(),
                handoff.prompt.content_sha256.clone(),
            ),
            (
                "workflow.parent_trace_id".to_string(),
                progress.bootstrap.trace_id.clone(),
            ),
            (
                "workflow.parent_span_id".to_string(),
                progress.bootstrap.root_span_id.clone(),
            ),
            (
                "workflow.trace_context_id".to_string(),
                progress.bootstrap.trace_context_id.clone(),
            ),
        ])),
    };
    let submitted = if is_initial_turn {
        node.start(
            effect(
                progress.requirement_generation,
                stage,
                attempt,
                "turn-start",
            )?,
            input,
        )
        .await
    } else {
        node.submit(
            effect(
                progress.requirement_generation,
                stage,
                attempt,
                "turn-submit",
            )?,
            input,
        )
        .await
    }
    .map_err(definition_error)?;
    let turn = node
        .await_turn(submitted.turn_id)
        .await
        .map_err(definition_error)?;
    let final_output = turn
        .final_output
        .ok_or_else(|| WorkflowError::definition("stage turn has no structured final output"))?;
    let result: StageResultV1 = serde_json::from_str(&final_output).map_err(|error| {
        WorkflowError::definition(format!("invalid stage result JSON: {error}"))
    })?;
    validate_envelope(
        &result,
        stage,
        &stage_attempt_id,
        progress.requirement_generation,
    )?;
    validate_requested_transition(stage, &result)?;
    if result.trace_context.get("trace_id").and_then(Value::as_str)
        != Some(progress.bootstrap.trace_id.as_str())
    {
        return Err(WorkflowError::definition(
            "stage trace context is disconnected from the workflow root",
        ));
    }
    if result.disposition == "needs_input" {
        if result.questions.is_empty()
            || result
                .questions
                .iter()
                .all(|question| !question.required_for_advance)
        {
            return Err(WorkflowError::definition(
                "needs_input result has no mandatory question",
            ));
        }
        return poll_questions(
            ctx,
            progress,
            stage,
            attempt + 1,
            node.id(),
            node.thread_id().to_string(),
            handoff_artifact_id,
            handoff_meta.sha256,
            result.questions,
        )
        .await;
    }
    if result.disposition == "return_to_prior_stage" {
        let target = requested_back_edge(stage, &result)?;
        let target_attempt = if target == LarkFeatureStageId::Requirements {
            1
        } else {
            next_recorded_attempt(&progress, target)?
        };
        let progress = rewind_progress(progress, target)?;
        return Ok(WorkflowTransition::Continue {
            state: LarkFeatureState::StageReady {
                progress,
                stage: target,
                attempt: target_attempt,
                reply_artifact_id: None,
            },
        });
    }
    if result.disposition == "blocked" {
        return persist_blocked_result(ctx, progress, stage, attempt, result).await;
    }
    persist_stage_result(
        ctx,
        progress,
        stage,
        attempt,
        node.id(),
        node.thread_id().to_string(),
        handoff_artifact_id,
        handoff_meta.sha256,
        result,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn poll_questions(
    ctx: WorkflowContext<'_>,
    progress: LarkFeatureProgress,
    stage: LarkFeatureStageId,
    attempt: u32,
    node_id: crate::NodeId,
    thread_id: String,
    handoff_artifact_id: ArtifactId,
    handoff_sha256: String,
    questions: Vec<super::StageQuestion>,
) -> Result<WorkflowTransition<LarkFeatureState, LarkFeatureWorkflowOutput>, WorkflowError> {
    let capability = ctx
        .lark_feature()
        .ok_or_else(|| unavailable("Lark feature adapter"))?;
    match capability
        .poll_stage_questions(
            ctx.run_id(),
            stage,
            &thread_id,
            &questions,
            ctx.cancellation(),
        )
        .await?
    {
        HumanInteractionOutcome::Waiting { interaction_id } => Ok(WorkflowTransition::Wait {
            state: LarkFeatureState::StageQuestions {
                progress,
                stage,
                attempt,
                node_id,
                thread_id,
                handoff_artifact_id,
                handoff_sha256,
                questions,
                interaction_id: Some(interaction_id),
            },
            wake: WakeCondition::HumanInteraction(interaction_id),
        }),
        HumanInteractionOutcome::Resolved { artifact_id, .. } => Ok(WorkflowTransition::Continue {
            state: LarkFeatureState::StageReady {
                progress,
                stage,
                attempt,
                reply_artifact_id: Some(artifact_id),
            },
        }),
        HumanInteractionOutcome::TimedOut { interaction_id } => {
            Ok(WorkflowTransition::NeedsOperator {
                state: LarkFeatureState::StageQuestions {
                    progress,
                    stage,
                    attempt,
                    node_id,
                    thread_id,
                    handoff_artifact_id,
                    handoff_sha256,
                    questions,
                    interaction_id: Some(interaction_id),
                },
                error_code: "lark_feature_question_timed_out".to_string(),
                metadata: json!({"stage": stage, "interaction_id": interaction_id}),
            })
        }
        HumanInteractionOutcome::Cancelled { interaction_id } => {
            Ok(WorkflowTransition::NeedsOperator {
                state: LarkFeatureState::StageQuestions {
                    progress,
                    stage,
                    attempt,
                    node_id,
                    thread_id,
                    handoff_artifact_id,
                    handoff_sha256,
                    questions,
                    interaction_id: Some(interaction_id),
                },
                error_code: "lark_feature_question_cancelled".to_string(),
                metadata: json!({"stage": stage, "interaction_id": interaction_id}),
            })
        }
        HumanInteractionOutcome::NeedsOperator {
            interaction_id,
            reason,
        } => Ok(WorkflowTransition::NeedsOperator {
            state: LarkFeatureState::StageQuestions {
                progress,
                stage,
                attempt,
                node_id,
                thread_id,
                handoff_artifact_id,
                handoff_sha256,
                questions,
                interaction_id: Some(interaction_id),
            },
            error_code: "lark_feature_question_needs_operator".to_string(),
            metadata: json!({
                "stage": stage,
                "interaction_id": interaction_id,
                "reason": reason,
            }),
        }),
    }
}

#[allow(clippy::too_many_arguments)]
async fn resume_stage_questions(
    ctx: WorkflowContext<'_>,
    progress: LarkFeatureProgress,
    stage: LarkFeatureStageId,
    attempt: u32,
    node_id: crate::NodeId,
    thread_id: String,
    handoff_artifact_id: ArtifactId,
    handoff_sha256: String,
    questions: Vec<super::StageQuestion>,
) -> Result<WorkflowTransition<LarkFeatureState, LarkFeatureWorkflowOutput>, WorkflowError> {
    poll_questions(
        ctx,
        progress,
        stage,
        attempt,
        node_id,
        thread_id,
        handoff_artifact_id,
        handoff_sha256,
        questions,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn persist_stage_result(
    ctx: WorkflowContext<'_>,
    progress: LarkFeatureProgress,
    stage: LarkFeatureStageId,
    attempt: u32,
    node_id: crate::NodeId,
    thread_id: String,
    handoff_artifact_id: ArtifactId,
    handoff_sha256: String,
    result: StageResultV1,
) -> Result<WorkflowTransition<LarkFeatureState, LarkFeatureWorkflowOutput>, WorkflowError> {
    let artifacts = ctx
        .artifacts()
        .ok_or_else(|| unavailable("artifact client"))?;
    let result_bytes = serde_json::to_vec_pretty(&result)
        .map_err(|error| WorkflowError::Serialization(error.to_string()))?;
    let stage_attempt_id = stage_attempt_id(progress.requirement_generation, stage, attempt);
    let result_artifact_id =
        deterministic_artifact(ctx.run_id(), &format!("{stage_attempt_id}/stage-result"))?;
    let result_path = PathBuf::from(format!(
        "lark-feature/{}/{}/stage-result-{attempt}.json",
        progress.requirement_generation,
        stage.node_key()
    ));
    let result_meta = artifacts
        .write(WorkflowArtifactClientWrite {
            artifact_id: result_artifact_id,
            relative_path: &result_path,
            classification: if matches!(
                stage,
                LarkFeatureStageId::Requirements | LarkFeatureStageId::TechnicalDesign
            ) {
                ArtifactClassification::Sensitive
            } else {
                ArtifactClassification::Internal
            },
            media_type: "application/json",
            bytes: &result_bytes,
            created_at_ms: now_ms()?,
        })
        .await
        .map_err(definition_error)?;
    if stage == LarkFeatureStageId::Execution {
        let evidence = execution_evidence(&result)?;
        let record = StageRunRecord {
            stage_id: stage,
            stage_attempt_id,
            node_id,
            thread_id,
            handoff_artifact_id,
            handoff_sha256,
            result_artifact_id,
            result_sha256: result_meta.sha256.clone(),
            subject_artifact_id: evidence.completion_report_artifact_id,
            subject_sha256: result_meta.sha256,
            document_id: None,
            document_revision: None,
        };
        return Ok(WorkflowTransition::Continue {
            state: LarkFeatureState::DeliveryValidation {
                progress,
                execution: record,
            },
        });
    }
    let subject = approval_subject(stage, &result)?;
    let record = StageRunRecord {
        stage_id: stage,
        stage_attempt_id,
        node_id,
        thread_id,
        handoff_artifact_id,
        handoff_sha256,
        result_artifact_id,
        result_sha256: result_meta.sha256,
        subject_artifact_id: subject.artifact_id,
        subject_sha256: subject.sha256,
        document_id: subject.document_id,
        document_revision: subject.document_revision,
    };
    let deadline_ms = now_ms()?
        .checked_add(
            i64::try_from(progress.args.approval_timeout_ms)
                .map_err(|error| WorkflowError::definition(error.to_string()))?,
        )
        .ok_or_else(|| WorkflowError::definition("approval deadline overflow"))?;
    Ok(WorkflowTransition::Continue {
        state: LarkFeatureState::Approval {
            progress,
            stage,
            attempt,
            stage_record: record,
            approval_id: None,
            deadline_ms,
        },
    })
}

async fn persist_blocked_result(
    ctx: WorkflowContext<'_>,
    progress: LarkFeatureProgress,
    stage: LarkFeatureStageId,
    attempt: u32,
    result: StageResultV1,
) -> Result<WorkflowTransition<LarkFeatureState, LarkFeatureWorkflowOutput>, WorkflowError> {
    let artifacts = ctx
        .artifacts()
        .ok_or_else(|| unavailable("artifact client"))?;
    let result_bytes = serde_json::to_vec_pretty(&result)
        .map_err(|error| WorkflowError::Serialization(error.to_string()))?;
    let stage_attempt_id = stage_attempt_id(progress.requirement_generation, stage, attempt);
    let artifact_id = deterministic_artifact(
        ctx.run_id(),
        &format!("{stage_attempt_id}/blocked-stage-result"),
    )?;
    let artifact = artifacts
        .write(WorkflowArtifactClientWrite {
            artifact_id,
            relative_path: &PathBuf::from(format!(
                "lark-feature/{}/{}/blocked-stage-result-{attempt}.json",
                progress.requirement_generation,
                stage.node_key()
            )),
            classification: if matches!(
                stage,
                LarkFeatureStageId::Requirements | LarkFeatureStageId::TechnicalDesign
            ) {
                ArtifactClassification::Sensitive
            } else {
                ArtifactClassification::Internal
            },
            media_type: "application/json",
            bytes: &result_bytes,
            created_at_ms: now_ms()?,
        })
        .await
        .map_err(definition_error)?;
    let next_attempt = attempt
        .checked_add(1)
        .ok_or_else(|| WorkflowError::definition("stage attempt overflow"))?;
    Ok(WorkflowTransition::NeedsOperator {
        state: LarkFeatureState::StageReady {
            progress,
            stage,
            attempt: next_attempt,
            reply_artifact_id: None,
        },
        error_code: "lark_feature_stage_blocked".to_string(),
        metadata: json!({
            "stage": stage,
            "stage_attempt_id": stage_attempt_id,
            "result_artifact_id": artifact_id,
            "result_sha256": artifact.sha256,
        }),
    })
}

async fn poll_approval(
    ctx: WorkflowContext<'_>,
    mut progress: LarkFeatureProgress,
    stage: LarkFeatureStageId,
    attempt: u32,
    stage_record: StageRunRecord,
    deadline_ms: i64,
) -> Result<WorkflowTransition<LarkFeatureState, LarkFeatureWorkflowOutput>, WorkflowError> {
    let approvals = ctx
        .approvals()
        .ok_or_else(|| unavailable("approval service"))?;
    let request = approval_request(&progress, stage, attempt, deadline_ms, &stage_record)?;
    match approvals
        .poll_or_request(ctx.run_id(), request, now_ms()?, ctx.cancellation().flag())
        .await
        .map_err(definition_error)?
    {
        ApprovalOutcome::Waiting {
            approval_id,
            interaction_id,
        } => Ok(WorkflowTransition::Wait {
            state: LarkFeatureState::Approval {
                progress,
                stage,
                attempt,
                stage_record,
                approval_id: Some(approval_id),
                deadline_ms,
            },
            wake: WakeCondition::HumanInteraction(interaction_id),
        }),
        ApprovalOutcome::Approved {
            approval_id,
            decisions: _,
        } => {
            progress.approvals.push(ApprovalEvidenceRef {
                gate: gate(stage)?.to_string(),
                approval_id,
                artifact_id: stage_record.subject_artifact_id,
                sha256: stage_record.subject_sha256.clone(),
                document_id: stage_record.document_id.clone(),
                document_revision: stage_record.document_revision.clone(),
            });
            progress.stages.push(stage_record);
            Ok(WorkflowTransition::Continue {
                state: LarkFeatureState::StageReady {
                    progress,
                    stage: next_stage(stage)?,
                    attempt: 1,
                    reply_artifact_id: None,
                },
            })
        }
        ApprovalOutcome::ChangesRequested { decisions, .. }
        | ApprovalOutcome::Revoked { decisions, .. } => {
            let reply_artifact_id = decisions
                .iter()
                .rev()
                .find_map(|decision| decision.response_artifact_id);
            Ok(WorkflowTransition::Continue {
                state: LarkFeatureState::StageReady {
                    progress,
                    stage,
                    attempt: attempt + 1,
                    reply_artifact_id,
                },
            })
        }
        ApprovalOutcome::TimedOut { approval_id } => Ok(WorkflowTransition::NeedsOperator {
            state: LarkFeatureState::Approval {
                progress,
                stage,
                attempt,
                stage_record,
                approval_id: Some(approval_id),
                deadline_ms,
            },
            error_code: "lark_feature_approval_timed_out".to_string(),
            metadata: json!({"stage": stage, "approval_id": approval_id}),
        }),
        ApprovalOutcome::Cancelled { approval_id } => Ok(WorkflowTransition::NeedsOperator {
            state: LarkFeatureState::Approval {
                progress,
                stage,
                attempt,
                stage_record,
                approval_id: Some(approval_id),
                deadline_ms,
            },
            error_code: "lark_feature_approval_cancelled".to_string(),
            metadata: json!({"stage": stage, "approval_id": approval_id}),
        }),
        ApprovalOutcome::NeedsOperator {
            approval_id,
            reason,
        } => Ok(WorkflowTransition::NeedsOperator {
            state: LarkFeatureState::Approval {
                progress,
                stage,
                attempt,
                stage_record,
                approval_id: Some(approval_id),
                deadline_ms,
            },
            error_code: "lark_feature_approval_needs_operator".to_string(),
            metadata: json!({
                "stage": stage,
                "approval_id": approval_id,
                "reason": reason,
            }),
        }),
    }
}

async fn validate_delivery(
    ctx: WorkflowContext<'_>,
    mut progress: LarkFeatureProgress,
    execution: StageRunRecord,
) -> Result<WorkflowTransition<LarkFeatureState, LarkFeatureWorkflowOutput>, WorkflowError> {
    if progress.approvals.len() != 3 || progress.stages.len() != 3 {
        return Err(WorkflowError::definition(
            "delivery requires three current approval subjects",
        ));
    }
    let approvals = ctx
        .approvals()
        .ok_or_else(|| unavailable("approval service"))?;
    for approval in progress.approvals.clone() {
        match approvals
            .current_outcome(ctx.run_id(), approval.approval_id, now_ms()?)
            .await
            .map_err(definition_error)?
        {
            ApprovalOutcome::Approved { approval_id, .. }
                if approval_id == approval.approval_id => {}
            ApprovalOutcome::ChangesRequested { decisions, .. }
            | ApprovalOutcome::Revoked { decisions, .. } => {
                let target = approval_gate_stage(&approval.gate)
                    .ok_or_else(|| WorkflowError::definition("stored approval gate is invalid"))?;
                let target_attempt = if target == LarkFeatureStageId::Requirements {
                    1
                } else {
                    next_recorded_attempt(&progress, target)?
                };
                let reply_artifact_id = decisions
                    .iter()
                    .rev()
                    .find_map(|decision| decision.response_artifact_id);
                let progress = rewind_progress(progress, target)?;
                return Ok(WorkflowTransition::Continue {
                    state: LarkFeatureState::StageReady {
                        progress,
                        stage: target,
                        attempt: target_attempt,
                        reply_artifact_id,
                    },
                });
            }
            ApprovalOutcome::TimedOut { approval_id }
            | ApprovalOutcome::Cancelled { approval_id } => {
                return Ok(WorkflowTransition::NeedsOperator {
                    state: LarkFeatureState::DeliveryValidation {
                        progress,
                        execution,
                    },
                    error_code: "lark_feature_delivery_approval_unavailable".to_string(),
                    metadata: json!({"approval_id": approval_id}),
                });
            }
            ApprovalOutcome::NeedsOperator {
                approval_id,
                reason,
            } => {
                return Ok(WorkflowTransition::NeedsOperator {
                    state: LarkFeatureState::DeliveryValidation {
                        progress,
                        execution,
                    },
                    error_code: "lark_feature_delivery_approval_needs_operator".to_string(),
                    metadata: json!({"approval_id": approval_id, "reason": reason}),
                });
            }
            ApprovalOutcome::Waiting {
                approval_id,
                interaction_id,
            } => {
                return Ok(WorkflowTransition::NeedsOperator {
                    state: LarkFeatureState::DeliveryValidation {
                        progress,
                        execution,
                    },
                    error_code: "lark_feature_delivery_approval_incomplete".to_string(),
                    metadata: json!({
                        "approval_id": approval_id,
                        "interaction_id": interaction_id,
                    }),
                });
            }
            ApprovalOutcome::Approved { approval_id, .. } => {
                return Err(WorkflowError::definition(format!(
                    "approval identity changed during delivery validation: {approval_id}"
                )));
            }
        }
    }
    let artifacts = ctx
        .artifacts()
        .ok_or_else(|| unavailable("artifact client"))?;
    let result_bytes = artifacts
        .read(execution.result_artifact_id)
        .await
        .map_err(definition_error)?;
    if digest(&result_bytes) != execution.result_sha256 {
        return Err(WorkflowError::definition(
            "execution result artifact digest changed",
        ));
    }
    let result: StageResultV1 = serde_json::from_slice(&result_bytes)
        .map_err(|error| WorkflowError::Serialization(error.to_string()))?;
    let evidence = execution_evidence(&result)?;
    for (artifact_id, expected_sha256) in [
        (
            evidence.validation_artifact_id,
            evidence.validation_sha256.as_str(),
        ),
        (
            evidence.acceptance_artifact_id,
            evidence.acceptance_sha256.as_str(),
        ),
        (
            evidence.completion_report_artifact_id,
            evidence.completion_report_sha256.as_str(),
        ),
    ] {
        let bytes = artifacts
            .read(artifact_id)
            .await
            .map_err(definition_error)?;
        if digest(&bytes) != expected_sha256 {
            return Err(WorkflowError::definition(
                "delivery evidence artifact digest changed",
            ));
        }
    }
    if evidence.goal_thread_id != execution.thread_id {
        return Err(WorkflowError::definition(
            "goal is not owned by the Stage 4 thread",
        ));
    }
    let thread_id = ThreadId::from_string(&execution.thread_id)
        .map_err(|error| WorkflowError::definition(error.to_string()))?;
    let goal = ctx
        .goals()
        .ok_or_else(|| unavailable("goal client"))?
        .get(thread_id)
        .await
        .map_err(definition_error)?
        .ok_or_else(|| WorkflowError::definition("Stage 4 goal is missing"))?;
    if goal.goal_id != evidence.goal_id || goal.objective_sha256 != evidence.objective_sha256 {
        return Err(WorkflowError::definition(
            "Stage 4 goal correlation changed",
        ));
    }
    let delivery_evidence = WorkflowGoalDeliveryEvidence {
        goal_id: evidence.goal_id.clone(),
        objective_sha256: evidence.objective_sha256.clone(),
        validation_artifact_id: evidence.validation_artifact_id,
        validation_sha256: evidence.validation_sha256,
        acceptance_artifact_id: evidence.acceptance_artifact_id,
        acceptance_sha256: evidence.acceptance_sha256,
    };
    if !goal.permits_delivery(&delivery_evidence) {
        return Ok(WorkflowTransition::NeedsOperator {
            state: LarkFeatureState::DeliveryValidation {
                progress,
                execution,
            },
            error_code: "lark_feature_goal_or_evidence_incomplete".to_string(),
            metadata: json!({
                "goal_id": goal.goal_id,
                "goal_status": goal.status,
                "reason": "goal is not complete with independent validation and acceptance evidence",
            }),
        });
    }
    progress.stages.push(execution);
    let requirements = &progress.stages[0];
    let design = &progress.stages[1];
    let plan = &progress.stages[2];
    Ok(WorkflowTransition::Complete {
        output: LarkFeatureWorkflowOutput {
            workflow_run_id: ctx.run_id().to_string(),
            requirement_id: progress.args.requirement_id,
            requirement_generation: progress.requirement_generation,
            chat_id: progress.bootstrap.chat_id,
            stage_node_ids: progress
                .stages
                .iter()
                .map(|stage| stage.node_id.to_string())
                .collect(),
            stage_thread_ids: progress
                .stages
                .iter()
                .map(|stage| stage.thread_id.clone())
                .collect(),
            requirements_artifact_id: requirements.subject_artifact_id.to_string(),
            technical_design_document_id: design
                .document_id
                .clone()
                .ok_or_else(|| WorkflowError::definition("design document is missing"))?,
            technical_design_revision: design
                .document_revision
                .clone()
                .ok_or_else(|| WorkflowError::definition("design revision is missing"))?,
            exec_plan_document_id: plan
                .document_id
                .clone()
                .ok_or_else(|| WorkflowError::definition("plan document is missing"))?,
            exec_plan_revision: plan
                .document_revision
                .clone()
                .ok_or_else(|| WorkflowError::definition("plan revision is missing"))?,
            approval_ids: progress
                .approvals
                .iter()
                .map(|approval| approval.approval_id.to_string())
                .collect(),
            goal_id: evidence.goal_id,
            validation_artifact_id: evidence.validation_artifact_id.to_string(),
            acceptance_artifact_id: evidence.acceptance_artifact_id.to_string(),
            completion_report_artifact_id: evidence.completion_report_artifact_id.to_string(),
            trace_id: progress.bootstrap.trace_id,
            observability_delivery_state: canonical_delivery_state(
                &progress.bootstrap.observability_delivery_state,
                &evidence.observability_delivery_state,
            )
            .to_string(),
        },
    })
}

fn require_runtime_facets(ctx: &WorkflowContext<'_>) -> Result<(), WorkflowError> {
    if ctx.nodes().is_none()
        || ctx.artifacts().is_none()
        || ctx.approvals().is_none()
        || ctx.goals().is_none()
        || ctx.lark_feature().is_none()
    {
        return Err(WorkflowError::definition(
            "lark feature workflow capabilities are unavailable before preflight",
        ));
    }
    Ok(())
}

pub fn lark_feature_node_spec(
    stage: LarkFeatureStageId,
    args: &LarkFeatureArguments,
) -> Result<NodeSpec, WorkflowError> {
    let repository = AbsolutePathBuf::from_absolute_path(&args.repository_path)
        .map_err(|error| WorkflowError::definition(error.to_string()))?;
    let worktree = AbsolutePathBuf::from_absolute_path(&args.worktree_path)
        .map_err(|error| WorkflowError::definition(error.to_string()))?;
    let (working_directory, sandbox, approvals, skills, classification) = match stage {
        LarkFeatureStageId::Requirements => (
            NodeWorkingDirectory::WorkflowDefault,
            NodeSandbox::Policy(SandboxPolicy::ReadOnly {
                network_access: false,
            }),
            NodeApprovals::RejectWhenDetached,
            SkillPolicy::AllowOnly(vec![skill("grill-me", true)]),
            NodeOutputClassification::Sensitive,
        ),
        LarkFeatureStageId::TechnicalDesign => (
            NodeWorkingDirectory::Exact(repository),
            NodeSandbox::Policy(SandboxPolicy::ReadOnly {
                network_access: false,
            }),
            NodeApprovals::RejectWhenDetached,
            SkillPolicy::AllowOnly(vec![skill("lark-doc", true)]),
            NodeOutputClassification::Sensitive,
        ),
        LarkFeatureStageId::ExecPlanDesign => (
            NodeWorkingDirectory::Exact(repository),
            NodeSandbox::Policy(SandboxPolicy::ReadOnly {
                network_access: false,
            }),
            NodeApprovals::RejectWhenDetached,
            SkillPolicy::AllowOnly(vec![
                skill("workflow-exec-plan-designer", true),
                skill("lark-doc", false),
            ]),
            NodeOutputClassification::Internal,
        ),
        LarkFeatureStageId::Execution => (
            NodeWorkingDirectory::Exact(worktree),
            NodeSandbox::Policy(SandboxPolicy::WorkspaceWrite {
                writable_roots: Vec::new(),
                network_access: false,
                exclude_tmpdir_env_var: false,
                exclude_slash_tmp: false,
            }),
            NodeApprovals::ExistingClient,
            SkillPolicy::Disabled,
            NodeOutputClassification::Internal,
        ),
    };
    NodeSpec::builder(NodeKey::new(stage.node_key()).map_err(definition_error)?)
        .working_directory(working_directory)
        .sandbox(sandbox)
        .approvals(approvals)
        .collaboration_mode(NodeCollaborationMode::WorkflowDefault)
        .skills(skills)
        .retry(RetryPolicy {
            maximum_attempts: NonZeroU32::new(3)
                .ok_or_else(|| WorkflowError::definition("retry count must be nonzero"))?,
            backoff: BackoffPolicy::Exponential {
                initial_delay_ms: 1_000,
                maximum_delay_ms: 30_000,
                jitter_percent: 10,
            },
            session: RetrySession::NewTurnOnSameThread,
            retry_on: vec![
                RetryClassification::ModelTransient,
                RetryClassification::ToolTransient,
                RetryClassification::RateLimited,
                RetryClassification::Interrupted,
            ],
        })
        .output_classification(classification)
        .failure_policy(FailurePolicy::FailFast)
        .build()
        .map_err(definition_error)
}

fn skill(package: &str, invoke: bool) -> SkillSelector {
    SkillSelector {
        authority: SkillAuthoritySelector {
            kind: "executor".to_string(),
            id: "codex-skills".to_string(),
        },
        package: SkillPackageSelector(package.to_string()),
        initial_invocation: if invoke {
            SkillInitialInvocation::InvokeOnInitialTurn
        } else {
            SkillInitialInvocation::Available
        },
    }
}

fn approval_request(
    progress: &LarkFeatureProgress,
    stage: LarkFeatureStageId,
    attempt: u32,
    deadline_ms: i64,
    record: &StageRunRecord,
) -> Result<ApprovalRequest, WorkflowError> {
    let allowed_approvers = progress
        .args
        .approvers
        .iter()
        .map(|approver| OpenId::parse(approver).map_err(definition_error))
        .collect::<Result<BTreeSet<_>, _>>()?;
    Ok(ApprovalRequest {
        effect_key: effect(progress.requirement_generation, stage, attempt, "approval")?,
        gate: gate(stage)?.to_string(),
        subject: ApprovalSubject {
            requirement_generation: progress.requirement_generation,
            artifact_id: record.subject_artifact_id,
            sha256: record.subject_sha256.clone(),
            document_id: record.document_id.clone(),
            document_revision: record.document_revision.clone(),
        },
        allowed_approvers,
        quorum: NonZeroUsize::new(progress.args.approval_quorum)
            .ok_or_else(|| WorkflowError::definition("approval quorum is zero"))?,
        chat_id: ChatId::parse(&progress.bootstrap.chat_id).map_err(definition_error)?,
        thread_id: None,
        deadline_ms,
        prompt: format!(
            "Review {} for requirement {}. The request is bound to the exact artifact digest and optional document revision shown below.",
            gate(stage)?,
            progress.args.requirement_id
        ),
    })
}

fn participants(progress: &LarkFeatureProgress) -> Vec<String> {
    let mut participants = vec![progress.args.requester.clone()];
    participants.extend(progress.args.developers.clone());
    participants.extend(progress.args.approvers.clone());
    participants.sort();
    participants.dedup();
    participants
}

fn prompt_variables(
    progress: &LarkFeatureProgress,
    stage: LarkFeatureStageId,
    handoff: &StageHandoffV1,
    output_schema: &str,
) -> Result<BTreeMap<&'static str, Value>, WorkflowError> {
    let participant_registry = progress
        .args
        .developers
        .iter()
        .map(|principal_id| (principal_id, "developer", true))
        .chain(
            progress
                .args
                .approvers
                .iter()
                .map(|principal_id| (principal_id, "approver", true)),
        )
        .chain(std::iter::once((
            &progress.args.requester,
            "requester",
            true,
        )))
        .map(|(principal_id, role, required)| {
            json!({
                "principal_id": principal_id,
                "role": role,
                "required": required,
                "group_membership_verified": progress
                    .bootstrap
                    .required_member_ids
                    .contains(principal_id),
            })
        })
        .collect::<Vec<_>>();
    let repository = json!({
        "repository_path": progress.args.repository_path,
        "worktree_path": progress.args.worktree_path,
        "branch": progress.args.branch,
        "base_commit": progress.args.base_commit,
        "repository_identity_sha256": progress.bootstrap.repository_identity_sha256,
        "instruction_ledger_artifact_id": progress.bootstrap.instruction_ledger_artifact_id,
    });
    let source_manifest = json!({
        "content_delivery": "immutable_artifact_reference",
        "source_manifest_artifact_id": progress.bootstrap.source_manifest_artifact_id,
        "instruction_ledger_artifact_id": progress.bootstrap.instruction_ledger_artifact_id,
    });
    let policies = json!({
        "policy_id": "lark-feature-explicit-v1",
        "quorum": progress.args.approval_quorum,
        "approver_ids": progress.args.approvers,
        "timeout_ms": progress.args.approval_timeout_ms,
        "silence_is_approval": false,
        "revision_and_digest_bound": true,
    });
    let redaction = json!({
        "policy_id": "lark-feature-redacted-v1",
        "raw_sensitive_content": "sensitive_artifacts_only",
        "trace_content": "identifiers_digests_and_redacted_summaries_only",
    });
    let tool_manifest = json!({
        "allowlist_id": handoff.tool_allowlist_id,
        "stage": stage,
        "skills": match stage {
            LarkFeatureStageId::Requirements => vec!["grill-me"],
            LarkFeatureStageId::TechnicalDesign => vec!["lark-doc"],
            LarkFeatureStageId::ExecPlanDesign => {
                vec!["workflow-exec-plan-designer", "lark-doc"]
            }
            LarkFeatureStageId::Execution => Vec::new(),
        },
        "goal_tools": stage == LarkFeatureStageId::Execution,
        "network_access": false,
    });
    let approved = |approved_stage| approved_stage_bundle(progress, approved_stage);
    let prior = to_json_value(&progress.stages)?;
    let mut variables = BTreeMap::new();
    variables.insert("stage_handoff_json", to_json_value(handoff)?);
    variables.insert(
        "output_schema_json",
        serde_json::from_str(output_schema)
            .map_err(|error| WorkflowError::Serialization(error.to_string()))?,
    );
    variables.insert(
        "initial_requirement_json",
        json!({
            "requirement_id": progress.args.requirement_id,
            "title": progress.args.requirement_title,
            "description": progress.args.requirement,
            "requester": progress.args.requester,
            "requirement_generation": progress.requirement_generation,
        }),
    );
    variables.insert("source_manifest_json", source_manifest);
    variables.insert("participant_registry_json", json!(participant_registry));
    variables.insert("prior_requirements_state_json", prior.clone());
    variables.insert("prior_design_state_json", prior.clone());
    variables.insert("prior_exec_plan_state_json", prior.clone());
    variables.insert("prior_execution_state_json", prior);
    variables.insert("tool_manifest_json", tool_manifest);
    variables.insert("approval_policy_json", policies);
    variables.insert("redaction_policy_json", redaction);
    variables.insert(
        "idle_policy_json",
        json!({"bounded_reminders": true, "silence_is_confirmation": false}),
    );
    variables.insert(
        "approved_requirements_json",
        approved(LarkFeatureStageId::Requirements),
    );
    variables.insert(
        "approved_technical_design_json",
        approved(LarkFeatureStageId::TechnicalDesign),
    );
    variables.insert(
        "approved_exec_plan_json",
        approved(LarkFeatureStageId::ExecPlanDesign),
    );
    variables.insert(
        "template_document_json",
        json!({
            "template_locator": progress.args.design_template,
            "content_delivery": "source_manifest_artifact_reference",
            "source_manifest_artifact_id": progress.bootstrap.source_manifest_artifact_id,
        }),
    );
    variables.insert("repository_snapshot_json", repository.clone());
    variables.insert("repository_context_json", repository);
    variables.insert(
        "repository_command_evidence_json",
        json!({
            "content_delivery": "source_manifest_artifact_reference",
            "source_manifest_artifact_id": progress.bootstrap.source_manifest_artifact_id,
        }),
    );
    variables.insert("feedback_state_json", json!({"current": []}));
    variables.insert(
        "instruction_ledger_json",
        json!({
            "artifact_id": progress.bootstrap.instruction_ledger_artifact_id,
            "content_delivery": "immutable_artifact_reference",
        }),
    );
    variables.insert(
        "goal_contract_json",
        json!({
            "objective": progress.args.goal_objective,
            "objective_sha256": digest(progress.args.goal_objective.as_bytes()),
            "token_budget": progress.args.goal_token_budget,
            "owning_stage": "execution",
            "completion_requires_independent_validation_and_acceptance": true,
        }),
    );
    variables.insert(
        "validation_inventory_json",
        json!({
            "content_delivery": "approved_exec_plan_artifact_reference",
            "approved_exec_plan": approved(LarkFeatureStageId::ExecPlanDesign),
        }),
    );
    Ok(variables)
}

fn to_json_value(value: &impl Serialize) -> Result<Value, WorkflowError> {
    serde_json::to_value(value).map_err(|error| WorkflowError::Serialization(error.to_string()))
}

fn approved_stage_bundle(progress: &LarkFeatureProgress, stage: LarkFeatureStageId) -> Value {
    let record = progress
        .stages
        .iter()
        .find(|record| record.stage_id == stage);
    let approval = progress
        .approvals
        .iter()
        .find(|approval| approval.gate == gate_name(stage));
    json!({
        "content_delivery": "immutable_artifact_reference",
        "stage": stage,
        "stage_record": record,
        "approval": approval,
    })
}

fn next_recorded_attempt(
    progress: &LarkFeatureProgress,
    stage: LarkFeatureStageId,
) -> Result<u32, WorkflowError> {
    let previous = progress
        .stages
        .iter()
        .filter(|record| record.stage_id == stage)
        .filter_map(|record| record.stage_attempt_id.rsplit('/').next())
        .map(|attempt| {
            attempt
                .parse::<u32>()
                .map_err(|_| WorkflowError::definition("stored stage attempt ID is invalid"))
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .max()
        .unwrap_or(0);
    previous
        .checked_add(1)
        .ok_or_else(|| WorkflowError::definition("stage attempt overflow"))
}

fn rewind_progress(
    mut progress: LarkFeatureProgress,
    target: LarkFeatureStageId,
) -> Result<LarkFeatureProgress, WorkflowError> {
    if target == LarkFeatureStageId::Requirements {
        progress.requirement_generation = progress
            .requirement_generation
            .checked_add(1)
            .ok_or_else(|| WorkflowError::definition("requirement generation overflow"))?;
        progress.stages.clear();
        progress.approvals.clear();
        return Ok(progress);
    }
    progress.stages.retain(|record| record.stage_id < target);
    progress
        .approvals
        .retain(|approval| approval_gate_stage(&approval.gate).is_some_and(|stage| stage < target));
    Ok(progress)
}

fn gate_name(stage: LarkFeatureStageId) -> &'static str {
    match stage {
        LarkFeatureStageId::Requirements => "requirements_baseline",
        LarkFeatureStageId::TechnicalDesign => "technical_design",
        LarkFeatureStageId::ExecPlanDesign => "coding_exec_plan",
        LarkFeatureStageId::Execution => "",
    }
}

fn approval_gate_stage(gate: &str) -> Option<LarkFeatureStageId> {
    match gate {
        "requirements_baseline" => Some(LarkFeatureStageId::Requirements),
        "technical_design" => Some(LarkFeatureStageId::TechnicalDesign),
        "coding_exec_plan" => Some(LarkFeatureStageId::ExecPlanDesign),
        _ => None,
    }
}

fn gate(stage: LarkFeatureStageId) -> Result<&'static str, WorkflowError> {
    match stage {
        LarkFeatureStageId::Requirements => Ok("requirements_baseline"),
        LarkFeatureStageId::TechnicalDesign => Ok("technical_design"),
        LarkFeatureStageId::ExecPlanDesign => Ok("coding_exec_plan"),
        LarkFeatureStageId::Execution => Err(WorkflowError::definition(
            "execution has no content approval gate",
        )),
    }
}

fn next_stage(stage: LarkFeatureStageId) -> Result<LarkFeatureStageId, WorkflowError> {
    match stage {
        LarkFeatureStageId::Requirements => Ok(LarkFeatureStageId::TechnicalDesign),
        LarkFeatureStageId::TechnicalDesign => Ok(LarkFeatureStageId::ExecPlanDesign),
        LarkFeatureStageId::ExecPlanDesign => Ok(LarkFeatureStageId::Execution),
        LarkFeatureStageId::Execution => Err(WorkflowError::definition(
            "execution has no next agent stage",
        )),
    }
}

fn effect(
    generation: u32,
    stage: LarkFeatureStageId,
    attempt: u32,
    operation: &str,
) -> Result<EffectKey, WorkflowError> {
    EffectKey::new(format!(
        "v1/{generation}/{}/{attempt}/{operation}",
        stage.node_key()
    ))
    .map_err(definition_error)
}

fn stage_attempt_id(generation: u32, stage: LarkFeatureStageId, attempt: u32) -> String {
    format!("v1/{generation}/{}/{attempt}", stage.node_key())
}

fn deterministic_artifact(
    run_id: crate::WorkflowRunId,
    logical_key: &str,
) -> Result<ArtifactId, WorkflowError> {
    let digest = Sha256::digest(format!("{run_id}\0{logical_key}"));
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    ArtifactId::try_from(uuid::Uuid::from_bytes(bytes)).map_err(definition_error)
}

fn valid_sha256(value: &str) -> bool {
    valid_lower_hex(value, 64)
}

fn valid_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn canonical_delivery_state(preflight: &str, execution: &str) -> &'static str {
    if preflight == "degraded_pending_backfill" || execution == "degraded_pending_backfill" {
        "degraded_pending_backfill"
    } else if preflight == "delivered_remote" && execution == "delivered_remote" {
        "delivered_remote"
    } else {
        "accepted_local"
    }
}

fn now_ms() -> Result<i64, WorkflowError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| WorkflowError::definition(error.to_string()))?
        .as_millis();
    i64::try_from(millis).map_err(|error| WorkflowError::definition(error.to_string()))
}

fn unavailable(name: &str) -> WorkflowError {
    WorkflowError::definition(format!("{name} is unavailable"))
}

fn definition_error(error: impl std::fmt::Display) -> WorkflowError {
    WorkflowError::definition(error.to_string())
}
