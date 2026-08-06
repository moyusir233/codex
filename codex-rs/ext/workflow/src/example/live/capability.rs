use super::*;

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
            let planner = self
                .ensure_node(run_id, "planner", self.config.planner_skills.clone())
                .await?;
            let mut reviewer_nodes = Vec::new();
            for index in 0..usize::from(args.reviewers) {
                reviewer_nodes.push(
                    self.ensure_node(
                        run_id,
                        &format!("reviewer-{}", index + 1),
                        self.config.reviewer_skills[index].clone(),
                    )
                    .await?,
                );
            }
            let synthesizer = self
                .ensure_node(
                    run_id,
                    "synthesizer",
                    self.config.synthesizer_skills.clone(),
                )
                .await?;
            let nodes = self.service.nodes(run_id);
            for (index, reviewer) in reviewer_nodes.iter().enumerate() {
                nodes
                    .add_dependency(
                        effect(&format!("dependency.planner.reviewer-{}", index + 1))?,
                        planner.id(),
                        reviewer.id(),
                        DependencyPolicy::AllSucceeded,
                    )
                    .await
                    .map_err(definition_error)?;
                nodes
                    .add_dependency(
                        effect(&format!("dependency.reviewer-{}.synthesizer", index + 1))?,
                        reviewer.id(),
                        synthesizer.id(),
                        DependencyPolicy::AllSucceeded,
                    )
                    .await
                    .map_err(definition_error)?;
            }
            let (planner_node_id, planner_thread_id, plan) = self
                .run_node_turn(
                    planner,
                    "planner",
                    format!("Plan a rigorous review of this rendered prompt:\n\n{rendered}"),
                )
                .await?;

            let mut pending = JoinSet::new();
            for (index, reviewer) in reviewer_nodes.into_iter().enumerate() {
                let capability = self.clone_for_task();
                let reviewer_prompt = format!(
                    "Reviewer {}: independently critique the prompt using this plan.\n\nPLAN:\n{}\n\nPROMPT:\n{}",
                    index + 1,
                    plan,
                    rendered
                );
                pending.spawn(async move {
                    capability
                        .run_node_turn(
                            reviewer,
                            &format!("reviewer-{}", index + 1),
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
                .run_node_turn(
                    synthesizer,
                    "synthesizer",
                    format!(
                        "Synthesize these independent reviews into a proposed final prompt and rationale.\n\n{review_text}"
                    ),
                )
                .await?;
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
            let draft = reviewed_draft(&draft_bytes, &final_output)?;
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
