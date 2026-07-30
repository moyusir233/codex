#![allow(clippy::expect_used)]

mod support;

use std::ffi::OsString;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::Ordering;

use codex_state::WorkflowRunStatus;
use codex_workflow_extension::ArtifactId;
use codex_workflow_extension::DriveOutcome;
use codex_workflow_extension::DependencyPolicy;
use codex_workflow_extension::EffectKey;
use codex_workflow_extension::HumanInteractionOutcome;
use codex_workflow_extension::InteractionId;
use codex_workflow_extension::PromptReviewArguments;
use codex_workflow_extension::PromptReviewCapability;
use codex_workflow_extension::PromptReviewCapabilityFuture;
use codex_workflow_extension::PromptReviewOutput;
use codex_workflow_extension::PromptReviewPrepared;
use codex_workflow_extension::PromptReviewReview;
use codex_workflow_extension::NodeInput;
use codex_workflow_extension::NodeKey;
use codex_workflow_extension::NodeSpec;
use codex_workflow_extension::SkillAuthoritySelector;
use codex_workflow_extension::SkillInitialInvocation;
use codex_workflow_extension::SkillPackageSelector;
use codex_workflow_extension::SkillPolicy;
use codex_workflow_extension::SkillSelector;
use codex_workflow_extension::WorkflowDriver;
use codex_workflow_extension::WorkflowName;
use codex_workflow_extension::WorkflowRunId;
use codex_workflow_extension::WorkflowService;
use codex_workflow_extension::default_registry;
use pretty_assertions::assert_eq;

struct FakePromptReview {
    calls: Mutex<Vec<&'static str>>,
    reply: HumanInteractionOutcome,
    service: WorkflowService,
}

impl FakePromptReview {
    fn waiting(service: WorkflowService, interaction_id: InteractionId) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            reply: HumanInteractionOutcome::Waiting { interaction_id },
            service,
        }
    }

    fn resolved(
        service: WorkflowService,
        interaction_id: InteractionId,
        artifact_id: ArtifactId,
    ) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            reply: HumanInteractionOutcome::Resolved {
                interaction_id,
                artifact_id,
            },
            service,
        }
    }

    fn calls(&self) -> Vec<&'static str> {
        self.calls.lock().expect("calls lock").clone()
    }

    fn record(&self, call: &'static str) {
        self.calls.lock().expect("calls lock").push(call);
    }
}

impl PromptReviewCapability for FakePromptReview {
    fn prepare<'a>(
        &'a self,
        _run_id: WorkflowRunId,
        args: &'a PromptReviewArguments,
        _cancellation: &'a codex_workflow_extension::WorkflowCancellation,
    ) -> PromptReviewCapabilityFuture<'a, PromptReviewPrepared> {
        self.record("prepare");
        Box::pin(async move {
            assert_eq!(args.reviewers, 3);
            Ok(PromptReviewPrepared {
                prompt_id: "prompt-01".to_string(),
                prompt_version: Some("7".to_string()),
                prompt_draft_artifact_id: ArtifactId::new(),
                rendered_prompt_artifact_id: ArtifactId::new(),
                span_handle_id: uuid::Uuid::now_v7().to_string(),
                trace_context_id: uuid::Uuid::now_v7().to_string(),
                trace_id: "trace-01".to_string(),
                root_span_id: "span-root".to_string(),
            })
        })
    }

    fn review<'a>(
        &'a self,
        run_id: WorkflowRunId,
        args: &'a PromptReviewArguments,
        _prepared: &'a PromptReviewPrepared,
        _cancellation: &'a codex_workflow_extension::WorkflowCancellation,
    ) -> PromptReviewCapabilityFuture<'a, PromptReviewReview> {
        self.record("review");
        Box::pin(async move {
            let planner = ensure_node(&self.service, run_id, "planner", 0).await?;
            let planner_turn = planner
                .start(effect("turn.planner")?, NodeInput::text("plan"))
                .await
                .map_err(workflow_error)?;
            planner
                .await_turn(planner_turn.turn_id)
                .await
                .map_err(workflow_error)?;

            let mut reviewers = Vec::new();
            for index in 0..args.reviewers {
                let reviewer = ensure_node(
                    &self.service,
                    run_id,
                    &format!("reviewer-{}", index + 1),
                    index + 1,
                )
                .await?;
                let turn = reviewer
                    .start(
                        effect(&format!("turn.reviewer-{}", index + 1))?,
                        NodeInput::text("review"),
                    )
                    .await
                    .map_err(workflow_error)?;
                reviewers.push((reviewer, turn.turn_id));
            }
            for (reviewer, turn_id) in &reviewers {
                reviewer
                    .await_turn(turn_id)
                    .await
                    .map_err(workflow_error)?;
            }

            let synthesizer = ensure_node(&self.service, run_id, "synthesizer", 4).await?;
            let nodes = self.service.nodes(run_id);
            for (index, (reviewer, _)) in reviewers.iter().enumerate() {
                nodes
                    .add_dependency(
                        effect(&format!("dependency.planner.reviewer-{}", index + 1))?,
                        planner.id(),
                        reviewer.id(),
                        DependencyPolicy::AllSucceeded,
                    )
                    .await
                    .map_err(workflow_error)?;
                nodes
                    .add_dependency(
                        effect(&format!("dependency.reviewer-{}.synthesizer", index + 1))?,
                        reviewer.id(),
                        synthesizer.id(),
                        DependencyPolicy::AllSucceeded,
                    )
                    .await
                    .map_err(workflow_error)?;
            }
            let synth_turn = synthesizer
                .start(effect("turn.synthesizer")?, NodeInput::text("synthesize"))
                .await
                .map_err(workflow_error)?;
            synthesizer
                .await_turn(synth_turn.turn_id)
                .await
                .map_err(workflow_error)?;
            Ok(PromptReviewReview {
                planner_node_id: planner.id().to_string(),
                planner_thread_id: planner.thread_id().to_string(),
                reviewer_node_ids: reviewers
                    .iter()
                    .map(|(reviewer, _)| reviewer.id().to_string())
                    .collect(),
                reviewer_thread_ids: reviewers
                    .iter()
                    .map(|(reviewer, _)| reviewer.thread_id().to_string())
                    .collect(),
                synthesizer_node_id: synthesizer.id().to_string(),
                synthesizer_thread_id: synthesizer.thread_id().to_string(),
                synthesis_artifact_id: ArtifactId::new(),
                lark_document_id: Some("doc-01".to_string()),
            })
        })
    }

    fn request_human<'a>(
        &'a self,
        _run_id: WorkflowRunId,
        _args: &'a PromptReviewArguments,
        _review: &'a PromptReviewReview,
        _cancellation: &'a codex_workflow_extension::WorkflowCancellation,
    ) -> PromptReviewCapabilityFuture<'a, HumanInteractionOutcome> {
        self.record("request_human");
        let reply = self.reply.clone();
        Box::pin(async move { Ok(reply) })
    }

    fn follow_up_and_save<'a>(
        &'a self,
        _run_id: WorkflowRunId,
        _args: &'a PromptReviewArguments,
        prepared: &'a PromptReviewPrepared,
        review: &'a PromptReviewReview,
        reply_artifact_id: ArtifactId,
        _cancellation: &'a codex_workflow_extension::WorkflowCancellation,
    ) -> PromptReviewCapabilityFuture<'a, PromptReviewOutput> {
        self.record("follow_up_and_save");
        Box::pin(async move {
            let synthesizer = self
                .service
                .nodes(_run_id)
                .get(
                    codex_workflow_extension::NodeId::parse(&review.synthesizer_node_id)
                        .map_err(workflow_error)?,
                )
                .await
                .map_err(workflow_error)?;
            let turn = synthesizer
                .start(effect("turn.synthesizer.follow-up")?, NodeInput::text("human reply"))
                .await
                .map_err(workflow_error)?;
            synthesizer
                .await_turn(turn.turn_id)
                .await
                .map_err(workflow_error)?;
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
                final_artifact_ids: vec![review.synthesis_artifact_id.to_string()],
                resume_commands: std::iter::once(&review.planner_thread_id)
                    .chain(review.reviewer_thread_ids.iter())
                    .chain(std::iter::once(&review.synthesizer_thread_id))
                    .map(|thread| format!("codex resume {thread}"))
                    .collect(),
            })
        })
    }
}

async fn ensure_node(
    service: &WorkflowService,
    run_id: WorkflowRunId,
    key: &str,
    skill_index: u8,
) -> Result<codex_workflow_extension::NodeHandle, codex_workflow_extension::WorkflowError> {
    let spec = NodeSpec::builder(NodeKey::new(key).map_err(workflow_error)?)
        .skills(SkillPolicy::AllowOnly(vec![SkillSelector {
            authority: SkillAuthoritySelector {
                kind: "test".to_string(),
                id: format!("authority-{skill_index}"),
            },
            package: SkillPackageSelector(format!("package-{skill_index}")),
            initial_invocation: SkillInitialInvocation::Available,
        }]))
        .build()
        .map_err(workflow_error)?;
    service
        .nodes(run_id)
        .ensure(effect(&format!("ensure.{key}"))?, spec)
        .await
        .map_err(workflow_error)
}

fn effect(value: &str) -> Result<EffectKey, codex_workflow_extension::WorkflowError> {
    EffectKey::new(value).map_err(workflow_error)
}

fn workflow_error(error: impl std::fmt::Display) -> codex_workflow_extension::WorkflowError {
    codex_workflow_extension::WorkflowError::definition(error.to_string())
}

#[tokio::test]
async fn prompt_review_e2e_restarts_at_human_wait_and_resolves_once() {
    let (_home, runtime) = support::runtime().await;
    let registry = Arc::new(default_registry().expect("default registry"));
    let definition = registry
        .resolve(
            &WorkflowName::new("prompt-review").expect("workflow name"),
            None,
        )
        .expect("prompt-review definition");
    let arguments = definition
        .parse_cli(
            &[
                "--prompt-key",
                "demo.prompt",
                "--prompt-version",
                "7",
                "--reviewers",
                "3",
                "--lark-users",
                "ou_a,ou_b",
                "--lark-chat-id",
                "oc_review",
            ]
            .map(OsString::from),
        )
        .expect("parse arguments");
    let checkpoint = definition
        .initialize(arguments.clone())
        .expect("initialize");
    let run_id = WorkflowRunId::new();
    support::create_run(
        runtime.as_ref(),
        run_id,
        "prompt-review",
        "1.0.0",
        checkpoint.state_schema_version(),
        checkpoint.state().clone(),
    )
    .await;

    let interaction_id = InteractionId::new();
    let host = Arc::new(support::TestHost::default());
    let node_service = support::service(
        runtime.as_ref(),
        Arc::clone(&host),
        Arc::clone(&registry),
    );
    let before_restart = Arc::new(FakePromptReview::waiting(
        node_service.clone(),
        interaction_id,
    ));
    let driver = WorkflowDriver::new(
        runtime.workflows().clone(),
        Arc::clone(&registry),
        "before-restart",
        1_000,
    )
    .with_prompt_review_capability(before_restart.clone());
    assert_eq!(
        driver
            .drive_until_blocked(run_id, 200, NonZeroUsize::new(10).expect("nonzero"))
            .await
            .expect("drive to wait"),
        DriveOutcome::Waiting
    );
    assert_eq!(
        before_restart.calls(),
        ["prepare", "review", "request_human"]
    );
    let waiting = runtime
        .workflows()
        .read_run(&run_id.to_string())
        .await
        .expect("read waiting run")
        .expect("waiting run");
    assert_eq!(waiting.status, WorkflowRunStatus::Waiting);
    assert_eq!(
        waiting.wake,
        Some(
            serde_json::to_value(codex_workflow_extension::WakeCondition::HumanInteraction(
                interaction_id
            ))
            .expect("serialize wake")
        )
    );

    let reply_artifact_id = ArtifactId::new();
    let after_restart = Arc::new(FakePromptReview::resolved(
        node_service,
        interaction_id,
        reply_artifact_id,
    ));
    let restarted_driver = WorkflowDriver::new(
        runtime.workflows().clone(),
        Arc::clone(&registry),
        "after-restart",
        1_000,
    )
    .with_prompt_review_capability(after_restart.clone());
    assert_eq!(
        restarted_driver
            .drive_until_blocked(run_id, 300, NonZeroUsize::new(10).expect("nonzero"))
            .await
            .expect("drive after restart"),
        DriveOutcome::Completed
    );
    assert_eq!(
        after_restart.calls(),
        ["request_human", "follow_up_and_save"]
    );
    assert_eq!(host.materializations.load(Ordering::Acquire), 5);
    assert_eq!(host.unique_queues.load(Ordering::Acquire), 6);
    let completed = runtime
        .workflows()
        .read_run(&run_id.to_string())
        .await
        .expect("read completed run")
        .expect("completed run");
    assert_eq!(completed.status, WorkflowRunStatus::Succeeded);
    assert_eq!(
        completed
            .output
            .as_ref()
            .and_then(|value| value.get("human_reply_artifact_id"))
            .and_then(serde_json::Value::as_str),
        Some(reply_artifact_id.to_string().as_str())
    );
    let output = completed.output.as_ref().expect("workflow output");
    let planner_thread = output["planner_thread_id"]
        .as_str()
        .expect("planner thread");
    let resume_commands = output["resume_commands"]
        .as_array()
        .expect("resume commands");
    assert_eq!(
        resume_commands.first().and_then(serde_json::Value::as_str),
        Some(format!("codex resume {planner_thread}").as_str())
    );
    assert_eq!(resume_commands.len(), 5);
}

#[tokio::test]
async fn prompt_review_fails_closed_before_capability_side_effects() {
    let (_home, runtime) = support::runtime().await;
    let registry = Arc::new(default_registry().expect("default registry"));
    let definition = registry
        .resolve(
            &WorkflowName::new("prompt-review").expect("workflow name"),
            None,
        )
        .expect("prompt-review definition");
    let arguments = definition
        .parse_cli(
            &[
                "--prompt-key",
                "demo.prompt",
                "--lark-users",
                "ou_a",
                "--lark-chat-id",
                "oc_review",
            ]
            .map(OsString::from),
        )
        .expect("parse arguments");
    let checkpoint = definition.initialize(arguments).expect("initialize");
    let run_id = WorkflowRunId::new();
    support::create_run(
        runtime.as_ref(),
        run_id,
        "prompt-review",
        "1.0.0",
        checkpoint.state_schema_version(),
        checkpoint.state().clone(),
    )
    .await;

    assert_eq!(
        WorkflowDriver::new(
            runtime.workflows().clone(),
            registry,
            "missing-capability",
            1_000,
        )
        .step_once(run_id, 200)
        .await
        .expect("drive missing capability"),
        DriveOutcome::Failed
    );
    assert_eq!(
        runtime
            .workflows()
            .read_run(&run_id.to_string())
            .await
            .expect("read failed run")
            .expect("failed run")
            .status,
        WorkflowRunStatus::Failed
    );
    assert!(
        runtime
            .workflows()
            .list_effects(&run_id.to_string())
            .await
            .expect("list effects")
            .is_empty()
    );
}
