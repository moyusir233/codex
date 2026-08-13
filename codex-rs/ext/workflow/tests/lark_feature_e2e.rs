#![cfg(unix)]
#![allow(clippy::expect_used)]

mod support;

use std::collections::BTreeSet;
use std::num::NonZeroUsize;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use codex_protocol::ThreadId;
use codex_protocol::user_input::UserInput;
use codex_state::StateRuntime;
use codex_state::WorkflowRunCreate;
use codex_workflow_extension::ApprovalId;
use codex_workflow_extension::ApprovalOutcome;
use codex_workflow_extension::ArtifactClassification;
use codex_workflow_extension::ArtifactId;
use codex_workflow_extension::DriveOutcome;
use codex_workflow_extension::EffectKey;
use codex_workflow_extension::HumanInteractionOutcome;
use codex_workflow_extension::HumanInteractionRequest;
use codex_workflow_extension::LarkFeatureArguments;
use codex_workflow_extension::LarkFeatureBootstrap;
use codex_workflow_extension::LarkFeatureCapability;
use codex_workflow_extension::LarkFeatureFuture;
use codex_workflow_extension::LarkFeatureStageId;
use codex_workflow_extension::StageQuestion;
use codex_workflow_extension::WorkflowApprovalService;
use codex_workflow_extension::WorkflowArtifactStore;
use codex_workflow_extension::WorkflowArtifactWrite;
use codex_workflow_extension::WorkflowDriver;
use codex_workflow_extension::WorkflowError;
use codex_workflow_extension::WorkflowGoalCapability;
use codex_workflow_extension::WorkflowGoalEnsureRequest;
use codex_workflow_extension::WorkflowGoalError;
use codex_workflow_extension::WorkflowGoalFuture;
use codex_workflow_extension::WorkflowGoalSnapshot;
use codex_workflow_extension::WorkflowGoalStatus;
use codex_workflow_extension::WorkflowName;
use codex_workflow_extension::WorkflowRunId;
use codex_workflow_extension::WorkflowService;
use codex_workflow_extension::default_registry;
use codex_workflow_extension::integrations::lark::ChatId;
use codex_workflow_extension::integrations::lark::ContentSensitivity;
use codex_workflow_extension::integrations::lark::LarkCli;
use codex_workflow_extension::integrations::lark::LarkCliConfig;
use codex_workflow_extension::integrations::lark::LarkCliEnvironment;
use codex_workflow_extension::integrations::lark::LarkEvent;
use codex_workflow_extension::integrations::lark::MessageId;
use codex_workflow_extension::integrations::lark::OpenId;
use codex_workflow_extension::runtime::LarkInteractionService;
use pretty_assertions::assert_eq;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;

struct FakeFeatureCapability {
    bootstrap: LarkFeatureBootstrap,
    grilling_calls: Mutex<Vec<(LarkFeatureStageId, String)>>,
}

impl LarkFeatureCapability for FakeFeatureCapability {
    fn preflight_and_group<'a>(
        &'a self,
        _run_id: WorkflowRunId,
        _args: &'a LarkFeatureArguments,
        _cancellation: &'a codex_workflow_extension::WorkflowCancellation,
    ) -> LarkFeatureFuture<'a, LarkFeatureBootstrap> {
        Box::pin(async move { Ok(self.bootstrap.clone()) })
    }

    fn poll_stage_questions<'a>(
        &'a self,
        _run_id: WorkflowRunId,
        stage: LarkFeatureStageId,
        owning_thread_id: &'a str,
        _questions: &'a [StageQuestion],
        _cancellation: &'a codex_workflow_extension::WorkflowCancellation,
    ) -> LarkFeatureFuture<'a, HumanInteractionOutcome> {
        self.grilling_calls
            .lock()
            .expect("grilling calls")
            .push((stage, owning_thread_id.to_string()));
        Box::pin(async {
            Err(codex_workflow_extension::WorkflowError::definition(
                "unexpected question",
            ))
        })
    }
}

struct FakeGoalCapability {
    run_id: WorkflowRunId,
    thread_id: ThreadId,
    snapshot: WorkflowGoalSnapshot,
}

struct LarkQuestionFeatureCapability {
    bootstrap: LarkFeatureBootstrap,
    lark: Arc<LarkInteractionService>,
    grilling_calls: Mutex<Vec<(LarkFeatureStageId, String)>>,
}

impl LarkFeatureCapability for LarkQuestionFeatureCapability {
    fn preflight_and_group<'a>(
        &'a self,
        _run_id: WorkflowRunId,
        _args: &'a LarkFeatureArguments,
        _cancellation: &'a codex_workflow_extension::WorkflowCancellation,
    ) -> LarkFeatureFuture<'a, LarkFeatureBootstrap> {
        Box::pin(async move { Ok(self.bootstrap.clone()) })
    }

    fn poll_stage_questions<'a>(
        &'a self,
        run_id: WorkflowRunId,
        stage: LarkFeatureStageId,
        owning_thread_id: &'a str,
        questions: &'a [StageQuestion],
        cancellation: &'a codex_workflow_extension::WorkflowCancellation,
    ) -> LarkFeatureFuture<'a, HumanInteractionOutcome> {
        self.grilling_calls
            .lock()
            .expect("grilling calls")
            .push((stage, owning_thread_id.to_string()));
        Box::pin(async move {
            if stage != LarkFeatureStageId::Requirements
                || questions.is_empty()
                || questions
                    .iter()
                    .all(|question| !question.required_for_advance)
            {
                return Err(WorkflowError::definition("invalid grilling question batch"));
            }
            self.lark
                .poll_or_request(
                    run_id,
                    HumanInteractionRequest {
                        effect_key: EffectKey::new(format!(
                            "lark-feature/questions/{owning_thread_id}"
                        ))
                        .map_err(|error| WorkflowError::definition(error.to_string()))?,
                        prompt: questions
                            .iter()
                            .map(|question| format!("{}: {}", question.question_id, question.text))
                            .collect::<Vec<_>>()
                            .join("\n"),
                        chat_id: ChatId::parse("oc_approval_01")
                            .map_err(|error| WorkflowError::definition(error.to_string()))?,
                        thread_id: None,
                        allowed_senders: vec![
                            OpenId::parse("ou_developer")
                                .map_err(|error| WorkflowError::definition(error.to_string()))?,
                        ],
                        deadline_ms: 4_000_000_000_000,
                        sensitivity: ContentSensitivity::NonSensitive,
                    },
                    now_ms(),
                    cancellation.flag(),
                )
                .await
                .map_err(|error| WorkflowError::definition(error.to_string()))
        })
    }
}

impl WorkflowGoalCapability for FakeGoalCapability {
    fn ensure<'a>(
        &'a self,
        _run_id: WorkflowRunId,
        _request: &'a WorkflowGoalEnsureRequest,
    ) -> WorkflowGoalFuture<'a, WorkflowGoalSnapshot> {
        Box::pin(async {
            Err(WorkflowGoalError::InvalidRequest(
                "agent owns goal creation".to_string(),
            ))
        })
    }

    fn get<'a>(
        &'a self,
        run_id: WorkflowRunId,
        thread_id: ThreadId,
    ) -> WorkflowGoalFuture<'a, Option<WorkflowGoalSnapshot>> {
        Box::pin(async move {
            if run_id == self.run_id && thread_id == self.thread_id {
                Ok(Some(self.snapshot.clone()))
            } else {
                Ok(None)
            }
        })
    }
}

#[tokio::test]
async fn lark_feature_fake_happy_path_has_four_threads_three_gates_and_goal_evidence()
-> Result<(), Box<dyn std::error::Error>> {
    let fake_lark = FakeLark::new()?;
    let home = tempfile::tempdir()?;
    let runtime =
        Arc::new(StateRuntime::init(home.path().to_path_buf(), "test-provider".to_string()).await?);
    let registry = Arc::new(default_registry()?);
    let name = WorkflowName::new("lark-rust-sdk-feature-development")?;
    let definition = registry.resolve(&name, None)?;
    let args = arguments();
    let arguments_json = serde_json::to_value(&args)?;
    let checkpoint = definition.initialize(arguments_json.clone())?;
    let run_id = WorkflowRunId::new();
    runtime
        .workflows()
        .create_run(WorkflowRunCreate {
            run_id: run_id.to_string(),
            definition_name: name.to_string(),
            definition_version: "1.0.0".to_string(),
            state_schema_version: checkpoint.state_schema_version(),
            state: checkpoint.state().clone(),
            arguments: arguments_json,
            non_interactive: false,
            detached: false,
            concurrency: Some(1),
            created_at_ms: now_ms(),
        })
        .await?;
    let artifacts = WorkflowArtifactStore::new(home.path(), runtime.workflows().clone());
    let source = write_artifact(&artifacts, run_id, "bootstrap/source.json", b"source").await?;
    let instructions = write_artifact(
        &artifacts,
        run_id,
        "bootstrap/instructions.md",
        b"instructions",
    )
    .await?;
    let requirements = write_artifact(
        &artifacts,
        run_id,
        "subjects/requirements.json",
        b"requirements",
    )
    .await?;
    let design = write_artifact(&artifacts, run_id, "subjects/design.md", b"design").await?;
    let plan = write_artifact(&artifacts, run_id, "subjects/plan.md", b"plan").await?;
    let validation =
        write_artifact(&artifacts, run_id, "delivery/validation.log", b"validation").await?;
    let acceptance = write_artifact(
        &artifacts,
        run_id,
        "delivery/acceptance.json",
        b"acceptance",
    )
    .await?;
    let report = write_artifact(&artifacts, run_id, "delivery/report.md", b"report").await?;
    let stage_threads = (0..4).map(|_| ThreadId::new()).collect::<Vec<_>>();
    let goal_objective_sha256 = digest(args.goal_objective.as_bytes());
    let outputs = vec![
        approval_stage_result(
            "requirements",
            "v1/1/stage-1-requirements/1",
            requirements,
            digest(b"requirements"),
            None,
        ),
        approval_stage_result(
            "technical_design",
            "v1/1/stage-2-technical-design/1",
            design,
            digest(b"design"),
            Some(("doc-design", "7")),
        ),
        approval_stage_result(
            "exec_plan_design",
            "v1/1/stage-3-exec-plan-design/1",
            plan,
            digest(b"plan"),
            Some(("doc-plan", "11")),
        ),
        execution_stage_result(validation, acceptance, report, &goal_objective_sha256),
    ];
    let host = Arc::new(support::TestHost::with_plan(stage_threads.clone(), outputs));
    let slot = codex_workflow_extension::WorkflowNodeHostSlot::new();
    slot.bind(host.clone())?;
    let lark = Arc::new(LarkInteractionService::new(
        fake_lark.client(),
        runtime.workflows().clone(),
        artifacts.clone(),
    ));
    let approvals = Arc::new(WorkflowApprovalService::new(
        runtime.workflows().clone(),
        Arc::clone(&lark),
        artifacts.clone(),
    ));
    let feature = Arc::new(FakeFeatureCapability {
        bootstrap: LarkFeatureBootstrap {
            chat_id: "oc_approval_01".to_string(),
            group_owner_marker_sha256: "a".repeat(64),
            required_member_ids: vec!["ou_developer".to_string(), "ou_approver".to_string()],
            repository_identity_sha256: "b".repeat(64),
            source_manifest_artifact_id: source,
            instruction_ledger_artifact_id: instructions,
            trace_id: "1".repeat(32),
            root_span_id: "2".repeat(16),
            trace_context_id: "00000000-0000-4000-8000-000000000001".to_string(),
            observability_delivery_state: "accepted_local".to_string(),
        },
        grilling_calls: Mutex::new(Vec::new()),
    });
    let goal = Arc::new(FakeGoalCapability {
        run_id,
        thread_id: stage_threads[3],
        snapshot: WorkflowGoalSnapshot {
            goal_id: "goal-stage-4".to_string(),
            thread_id: stage_threads[3],
            objective_sha256: goal_objective_sha256,
            status: WorkflowGoalStatus::Complete,
            token_budget: None,
            tokens_used: 12_345,
            time_used_seconds: 90,
        },
    });
    let service =
        WorkflowService::new_with_registry(runtime.workflows().clone(), slot, registry.clone())
            .with_artifact_store(artifacts)
            .with_approval_service(approvals)
            .with_goal_capability(goal)
            .with_lark_feature_capability(feature.clone());
    let driver = WorkflowDriver::new(
        runtime.workflows().clone(),
        registry.clone(),
        "lark-feature-test",
        30_000,
    )
    .with_lark_service(Arc::clone(&lark))
    .with_runtime_facets(service);

    for gate_index in 0..3 {
        assert_eq!(
            driver
                .drive_until_blocked(run_id, now_ms(), NonZeroUsize::new(20).expect("steps"))
                .await?,
            DriveOutcome::Waiting
        );
        approve_current_gate(&runtime, &lark, run_id, gate_index).await?;
    }
    assert_eq!(
        driver
            .drive_until_blocked(run_id, now_ms(), NonZeroUsize::new(20).expect("steps"))
            .await?,
        DriveOutcome::Completed
    );
    let run = runtime
        .workflows()
        .read_run(&run_id.to_string())
        .await?
        .expect("run");
    let output = run.output.expect("workflow output");
    assert_eq!(output["stage_thread_ids"].as_array().map(Vec::len), Some(4));
    assert_eq!(output["approval_ids"].as_array().map(Vec::len), Some(3));
    assert_eq!(output["goal_id"], "goal-stage-4");
    assert_eq!(output["technical_design_revision"], "7");
    assert_eq!(output["exec_plan_revision"], "11");
    assert_eq!(
        host.materializations
            .load(std::sync::atomic::Ordering::Acquire),
        4
    );
    assert_eq!(host.threads(), stage_threads);
    let submitted_inputs = host.submitted_inputs();
    assert_eq!(submitted_inputs.len(), 4);
    for (index, input) in submitted_inputs.into_iter().enumerate() {
        assert!(input.final_output_json_schema.is_some());
        let metadata = input
            .responsesapi_client_metadata
            .as_ref()
            .expect("trace metadata");
        for key in [
            "workflow.run_id",
            "workflow.requirement_id",
            "workflow.requirement_generation",
            "workflow.stage_id",
            "workflow.stage_attempt_id",
            "workflow.prompt_sha256",
            "workflow.parent_trace_id",
            "workflow.parent_span_id",
            "workflow.trace_context_id",
        ] {
            assert!(
                metadata.contains_key(key),
                "missing node trace metadata {key}"
            );
        }
        let prompt = input
            .items
            .iter()
            .find_map(|item| match item {
                UserInput::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .expect("standalone prompt text");
        assert!(prompt.contains("## Canonical StageHandoffV1"));
        assert!(prompt.contains("Do not rely on prior chat history"));
        assert!(!prompt.contains("{{"), "prompt has unresolved host input");
        if index == 0 {
            assert!(input.items.iter().any(|item| {
                matches!(item, UserInput::Skill { name, .. } if name == "grill-me")
            }));
        }
    }
    assert!(
        feature
            .grilling_calls
            .lock()
            .expect("grilling calls")
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn lark_feature_grilling_wait_resumes_the_same_stage_one_thread()
-> Result<(), Box<dyn std::error::Error>> {
    let fake_lark = FakeLark::new()?;
    let home = tempfile::tempdir()?;
    let runtime =
        Arc::new(StateRuntime::init(home.path().to_path_buf(), "test-provider".to_string()).await?);
    let registry = Arc::new(default_registry()?);
    let name = WorkflowName::new("lark-rust-sdk-feature-development")?;
    let definition = registry.resolve(&name, None)?;
    let args = arguments();
    let arguments_json = serde_json::to_value(&args)?;
    let checkpoint = definition.initialize(arguments_json.clone())?;
    let run_id = WorkflowRunId::new();
    runtime
        .workflows()
        .create_run(WorkflowRunCreate {
            run_id: run_id.to_string(),
            definition_name: name.to_string(),
            definition_version: "1.0.0".to_string(),
            state_schema_version: checkpoint.state_schema_version(),
            state: checkpoint.state().clone(),
            arguments: arguments_json,
            non_interactive: false,
            detached: false,
            concurrency: Some(1),
            created_at_ms: now_ms(),
        })
        .await?;
    let artifacts = WorkflowArtifactStore::new(home.path(), runtime.workflows().clone());
    let source = write_artifact(&artifacts, run_id, "bootstrap/source.json", b"source").await?;
    let instructions = write_artifact(
        &artifacts,
        run_id,
        "bootstrap/instructions.md",
        b"instructions",
    )
    .await?;
    let requirements = write_artifact(
        &artifacts,
        run_id,
        "subjects/requirements.json",
        b"requirements",
    )
    .await?;
    let stage_thread = ThreadId::new();
    let outputs = vec![
        json!({
            "schema_version":1,"stage_id":"requirements",
            "stage_attempt_id":"v1/1/stage-1-requirements/1",
            "requirement_generation":1,"disposition":"needs_input","output":{},
            "produced_artifacts":[],
            "questions":[{
                "question_id":"Q-1","text":"Which compatibility boundary is required?",
                "required_for_advance":true,"asked_to":["ou_developer"]
            }],
            "risks":[],"decisions":[],"validations":[],
            "trace_context":trace_context("questions"),
            "requested_transition":{"kind":"wait_for_input"}
        })
        .to_string(),
        approval_stage_result(
            "requirements",
            "v1/1/stage-1-requirements/2",
            requirements,
            digest(b"requirements"),
            None,
        ),
    ];
    let host = Arc::new(support::TestHost::with_plan(vec![stage_thread], outputs));
    let slot = codex_workflow_extension::WorkflowNodeHostSlot::new();
    slot.bind(host.clone())?;
    let lark = Arc::new(LarkInteractionService::new(
        fake_lark.client(),
        runtime.workflows().clone(),
        artifacts.clone(),
    ));
    let approvals = Arc::new(WorkflowApprovalService::new(
        runtime.workflows().clone(),
        Arc::clone(&lark),
        artifacts.clone(),
    ));
    let feature = Arc::new(LarkQuestionFeatureCapability {
        bootstrap: LarkFeatureBootstrap {
            chat_id: "oc_approval_01".to_string(),
            group_owner_marker_sha256: "a".repeat(64),
            required_member_ids: vec!["ou_developer".to_string(), "ou_approver".to_string()],
            repository_identity_sha256: "b".repeat(64),
            source_manifest_artifact_id: source,
            instruction_ledger_artifact_id: instructions,
            trace_id: "1".repeat(32),
            root_span_id: "2".repeat(16),
            trace_context_id: "00000000-0000-4000-8000-000000000001".to_string(),
            observability_delivery_state: "accepted_local".to_string(),
        },
        lark: Arc::clone(&lark),
        grilling_calls: Mutex::new(Vec::new()),
    });
    let goal = Arc::new(FakeGoalCapability {
        run_id,
        thread_id: stage_thread,
        snapshot: WorkflowGoalSnapshot {
            goal_id: "unused-goal".to_string(),
            thread_id: stage_thread,
            objective_sha256: digest(args.goal_objective.as_bytes()),
            status: WorkflowGoalStatus::Active,
            token_budget: None,
            tokens_used: 0,
            time_used_seconds: 0,
        },
    });
    let service =
        WorkflowService::new_with_registry(runtime.workflows().clone(), slot, registry.clone())
            .with_artifact_store(artifacts)
            .with_approval_service(approvals)
            .with_goal_capability(goal)
            .with_lark_feature_capability(feature.clone());
    let driver = WorkflowDriver::new(
        runtime.workflows().clone(),
        registry.clone(),
        "lark-feature-question-test",
        30_000,
    )
    .with_lark_service(Arc::clone(&lark))
    .with_runtime_facets(service.clone());

    let resumed_outcome = driver
        .drive_until_blocked(run_id, now_ms(), NonZeroUsize::new(20).expect("steps"))
        .await?;
    if resumed_outcome != DriveOutcome::Waiting {
        let run = runtime
            .workflows()
            .read_run(&run_id.to_string())
            .await?
            .expect("failed run");
        panic!(
            "resumed grilling run did not reach approval wait: {resumed_outcome:?}, error={:?}, state={}",
            run.error_code, run.state
        );
    }
    let correlation = runtime
        .workflows()
        .list_waiting_lark_interactions("oc_approval_01", 10)
        .await?
        .into_iter()
        .find(|record| record.allowed_senders == ["ou_developer"])
        .expect("durable grilling interaction");
    let outcomes = lark
        .ingest_event(
            &LarkEvent {
                event_id: "evt-question-answer".to_string(),
                event_type: "im.message.receive_v1".to_string(),
                created_at_ms: now_ms(),
                message_id: MessageId::parse("om_question_answer")?,
                chat_id: ChatId::parse("oc_approval_01")?,
                thread_id: None,
                sender_id: OpenId::parse("ou_developer")?,
                message_type: "text".to_string(),
                text: format!(
                    "Preserve compatibility with the current stable surface. [wf:{}]",
                    correlation.correlation_token
                ),
            },
            "test",
        )
        .await?;
    assert!(
        outcomes.iter().any(|outcome| {
            matches!(outcome, codex_state::WorkflowLarkResolveOutcome::Resolved)
        })
    );
    let restarted_driver = WorkflowDriver::new(
        runtime.workflows().clone(),
        registry,
        "lark-feature-question-restarted",
        30_000,
    )
    .with_lark_service(Arc::clone(&lark))
    .with_runtime_facets(service);
    let resumed_outcome = restarted_driver
        .drive_until_blocked(run_id, now_ms(), NonZeroUsize::new(20).expect("steps"))
        .await?;
    if resumed_outcome != DriveOutcome::Waiting {
        let run = runtime
            .workflows()
            .read_run(&run_id.to_string())
            .await?
            .expect("failed run");
        panic!(
            "resumed grilling run did not reach approval wait: {resumed_outcome:?}, error={:?}, state={}",
            run.error_code, run.state
        );
    }
    assert_eq!(
        host.materializations
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert_eq!(host.threads(), vec![stage_thread]);
    let inputs = host.submitted_inputs();
    assert_eq!(inputs.len(), 2);
    assert!(
        inputs[0]
            .items
            .iter()
            .any(|item| matches!(item, UserInput::Skill { name, .. } if name == "grill-me"))
    );
    assert!(
        !inputs[1]
            .items
            .iter()
            .any(|item| matches!(item, UserInput::Skill { .. }))
    );
    assert_eq!(
        feature
            .grilling_calls
            .lock()
            .expect("grilling calls")
            .as_slice(),
        &[
            (LarkFeatureStageId::Requirements, stage_thread.to_string()),
            (LarkFeatureStageId::Requirements, stage_thread.to_string()),
        ]
    );
    Ok(())
}

#[test]
fn lark_feature_e2e_evaluation_bundle_covers_release_scenarios_and_thresholds() {
    let scenarios: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "fixtures/lark_feature/scenarios/scenario-matrix.json"
    ))
    .expect("scenario matrix");
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/lark_feature/expected/evaluation-bundle.json"
    ))
    .expect("expected evaluation bundle");
    let thresholds: serde_json::Value =
        serde_json::from_str(include_str!("../eval/lark_feature/release-thresholds.json"))
            .expect("release thresholds");
    let item_manifest: serde_json::Value =
        serde_json::from_str(include_str!("../eval/lark_feature/items.json"))
            .expect("item manifest");
    assert_eq!(scenarios.len(), 16);
    assert_eq!(item_manifest["item_count"], 16);
    assert_eq!(expected["total_cases"], 16);

    let ids = scenarios
        .iter()
        .map(|case| case["case_id"].as_str().expect("case ID"))
        .collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), scenarios.len());
    let tags = scenarios
        .iter()
        .flat_map(|case| case["scenario_tags"].as_array().expect("tags"))
        .map(|tag| tag.as_str().expect("tag"))
        .collect::<BTreeSet<_>>();
    for required in [
        "normal",
        "ambiguity",
        "conflicting_stakeholders",
        "inaccessible_document",
        "stale_template",
        "permission_identity_failure",
        "idle_reviewer",
        "post_approval_requirement_change",
        "omitted_validation",
        "execution_failure_resume",
        "trace_failure",
        "approval_bypass",
        "prompt_injection",
        "revocation",
        "source_drift",
        "partial_goal",
    ] {
        assert!(
            tags.contains(required),
            "missing scenario family {required}"
        );
    }
    let normal_holdout = scenarios
        .iter()
        .filter(|case| {
            case["split"] == "holdout"
                && case["scenario_tags"]
                    .as_array()
                    .expect("tags")
                    .iter()
                    .any(|tag| tag == "normal")
        })
        .count();
    assert_eq!(normal_holdout, 1);
    let adverse = scenarios.len() - normal_holdout;
    assert_eq!(adverse, 15);
    let mut minimum_score = 5.0_f64;
    let mut minimum_mean = 5.0_f64;
    for case in &scenarios {
        assert!(
            !case["required_actions"]
                .as_array()
                .expect("required")
                .is_empty()
        );
        assert!(
            !case["forbidden_actions"]
                .as_array()
                .expect("forbidden")
                .is_empty()
        );
        assert_eq!(case["expected_policy"]["hard_veto"], false);
        let scores = case["quality_scores"]
            .as_array()
            .expect("scores")
            .iter()
            .map(|score| score.as_f64().expect("numeric score"))
            .collect::<Vec<_>>();
        minimum_score = minimum_score.min(scores.iter().copied().fold(5.0, f64::min));
        minimum_mean = minimum_mean.min(
            scores.iter().sum::<f64>()
                / f64::from(u32::try_from(scores.len()).expect("bounded score count")),
        );
    }
    assert!(
        minimum_score
            >= thresholds["minimum_dimension_score"]
                .as_f64()
                .expect("threshold")
    );
    assert!(
        minimum_mean
            >= thresholds["minimum_stage_mean"]
                .as_f64()
                .expect("threshold")
    );
    assert_eq!(
        minimum_score,
        expected["minimum_dimension_score"]
            .as_f64()
            .expect("expected minimum score")
    );
    assert!(
        minimum_mean
            >= expected["stage_mean_minimum"]
                .as_f64()
                .expect("expected mean")
    );
    assert_eq!(expected["normal_holdout_success_percent"], 100.0);
    assert_eq!(expected["adverse_safe_disposition_percent"], 100.0);
    assert_eq!(expected["hard_veto_failures"], 0);
    assert_eq!(expected["release_decision"], "local_m6_pass_live_disabled");
}

#[tokio::test]
async fn lark_feature_e2e_revoked_design_routes_delivery_back_and_prunes_plan()
-> Result<(), Box<dyn std::error::Error>> {
    let harness = planned_harness(HarnessPlan::Happy).await?;
    for gate_index in 0..3 {
        assert_eq!(
            harness
                .driver
                .drive_until_blocked(
                    harness.run_id,
                    now_ms(),
                    NonZeroUsize::new(20).expect("steps"),
                )
                .await?,
            DriveOutcome::Waiting
        );
        approve_current_gate(
            harness.runtime.as_ref(),
            harness.lark.as_ref(),
            harness.run_id,
            gate_index,
        )
        .await?;
    }
    let waiting = harness
        .runtime
        .workflows()
        .read_run(&harness.run_id.to_string())
        .await?
        .expect("run");
    let design_approval = ApprovalId::parse(
        waiting.state["progress"]["approvals"][1]["approval_id"]
            .as_str()
            .expect("design approval ID"),
    )?;
    assert!(matches!(
        harness
            .approvals
            .revoke(
                harness.run_id,
                design_approval,
                OpenId::parse("ou_approver")?,
                "Architecture must be revised".to_string(),
                now_ms(),
            )
            .await?,
        ApprovalOutcome::Revoked { .. }
    ));
    for _ in 0..3 {
        assert_eq!(
            harness.driver.step_once(harness.run_id, now_ms()).await?,
            DriveOutcome::Continued
        );
    }
    let rewound = harness
        .runtime
        .workflows()
        .read_run(&harness.run_id.to_string())
        .await?
        .expect("rewound run");
    assert_eq!(rewound.state["phase"], "stage_ready");
    assert_eq!(rewound.state["stage"], "technical_design");
    assert_eq!(rewound.state["attempt"], 2);
    assert_eq!(rewound.state["progress"]["requirement_generation"], 1);
    assert_eq!(
        rewound.state["progress"]["stages"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        rewound.state["progress"]["approvals"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert!(rewound.output.is_none());
    Ok(())
}

#[tokio::test]
async fn lark_feature_e2e_requirement_back_edge_increments_generation_and_reuses_thread()
-> Result<(), Box<dyn std::error::Error>> {
    let harness = planned_harness(HarnessPlan::RequirementsBackEdge).await?;
    assert_eq!(
        harness
            .driver
            .drive_until_blocked(
                harness.run_id,
                now_ms(),
                NonZeroUsize::new(20).expect("steps"),
            )
            .await?,
        DriveOutcome::Waiting
    );
    approve_current_gate(
        harness.runtime.as_ref(),
        harness.lark.as_ref(),
        harness.run_id,
        0,
    )
    .await?;
    assert_eq!(
        harness
            .driver
            .drive_until_blocked(
                harness.run_id,
                now_ms(),
                NonZeroUsize::new(20).expect("steps"),
            )
            .await?,
        DriveOutcome::Waiting
    );
    let run = harness
        .runtime
        .workflows()
        .read_run(&harness.run_id.to_string())
        .await?
        .expect("run");
    assert_eq!(run.state["phase"], "approval");
    assert_eq!(run.state["stage"], "requirements");
    assert_eq!(run.state["progress"]["requirement_generation"], 2);
    assert_eq!(
        run.state["progress"]["stages"].as_array().map(Vec::len),
        Some(0)
    );
    assert_eq!(
        run.state["progress"]["approvals"].as_array().map(Vec::len),
        Some(0)
    );
    assert_eq!(
        run.state["stage_record"]["thread_id"],
        harness.stage_threads[0].to_string()
    );
    assert_eq!(
        harness
            .host
            .materializations
            .load(std::sync::atomic::Ordering::Acquire),
        2
    );
    let inputs = harness.host.submitted_inputs();
    assert_eq!(inputs.len(), 3);
    assert!(
        inputs[2]
            .items
            .iter()
            .any(|item| matches!(item, UserInput::Skill { name, .. } if name == "grill-me"))
    );
    assert_eq!(
        inputs[2]
            .responsesapi_client_metadata
            .as_ref()
            .and_then(|metadata| metadata.get("workflow.requirement_generation"))
            .map(String::as_str),
        Some("2")
    );
    Ok(())
}

#[tokio::test]
async fn lark_feature_e2e_blocker_persists_sensitive_evidence_for_operator_resume()
-> Result<(), Box<dyn std::error::Error>> {
    let harness = planned_harness(HarnessPlan::BlockedRequirements).await?;
    assert_eq!(
        harness
            .driver
            .drive_until_blocked(
                harness.run_id,
                now_ms(),
                NonZeroUsize::new(20).expect("steps"),
            )
            .await?,
        DriveOutcome::NeedsOperator
    );
    let run = harness
        .runtime
        .workflows()
        .read_run(&harness.run_id.to_string())
        .await?
        .expect("run");
    assert_eq!(
        run.error_code.as_deref(),
        Some("lark_feature_stage_blocked")
    );
    assert_eq!(run.state["phase"], "stage_ready");
    assert_eq!(run.state["stage"], "requirements");
    assert_eq!(run.state["attempt"], 2);
    let artifacts = harness
        .runtime
        .workflows()
        .list_artifacts(&harness.run_id.to_string())
        .await?;
    let blocker = artifacts
        .iter()
        .find(|artifact| {
            artifact
                .relative_path
                .contains("blocked-stage-result-1.json")
        })
        .expect("blocked result artifact");
    assert_eq!(blocker.classification.as_str(), "sensitive");
    Ok(())
}

#[tokio::test]
async fn lark_feature_e2e_trace_outage_finishes_only_as_degraded_pending_backfill()
-> Result<(), Box<dyn std::error::Error>> {
    let harness = planned_harness(HarnessPlan::TraceDegraded).await?;
    for gate_index in 0..3 {
        assert_eq!(
            harness
                .driver
                .drive_until_blocked(
                    harness.run_id,
                    now_ms(),
                    NonZeroUsize::new(20).expect("steps"),
                )
                .await?,
            DriveOutcome::Waiting
        );
        approve_current_gate(
            harness.runtime.as_ref(),
            harness.lark.as_ref(),
            harness.run_id,
            gate_index,
        )
        .await?;
    }
    assert_eq!(
        harness
            .driver
            .drive_until_blocked(
                harness.run_id,
                now_ms(),
                NonZeroUsize::new(20).expect("steps"),
            )
            .await?,
        DriveOutcome::Completed
    );
    let output = harness
        .runtime
        .workflows()
        .read_run(&harness.run_id.to_string())
        .await?
        .expect("run")
        .output
        .expect("output");
    assert_eq!(
        output["observability_delivery_state"],
        "degraded_pending_backfill"
    );
    Ok(())
}

#[tokio::test]
async fn lark_feature_e2e_partial_goal_never_produces_workflow_success()
-> Result<(), Box<dyn std::error::Error>> {
    let harness = planned_harness(HarnessPlan::PartialGoal).await?;
    for gate_index in 0..3 {
        assert_eq!(
            harness
                .driver
                .drive_until_blocked(
                    harness.run_id,
                    now_ms(),
                    NonZeroUsize::new(20).expect("steps"),
                )
                .await?,
            DriveOutcome::Waiting
        );
        approve_current_gate(
            harness.runtime.as_ref(),
            harness.lark.as_ref(),
            harness.run_id,
            gate_index,
        )
        .await?;
    }
    assert_eq!(
        harness
            .driver
            .drive_until_blocked(
                harness.run_id,
                now_ms(),
                NonZeroUsize::new(20).expect("steps"),
            )
            .await?,
        DriveOutcome::NeedsOperator
    );
    let run = harness
        .runtime
        .workflows()
        .read_run(&harness.run_id.to_string())
        .await?
        .expect("run");
    assert!(run.output.is_none());
    assert_eq!(
        run.error_code.as_deref(),
        Some("lark_feature_goal_or_evidence_incomplete")
    );
    assert_eq!(run.state["phase"], "delivery_validation");
    Ok(())
}

#[derive(Clone, Copy)]
enum HarnessPlan {
    Happy,
    RequirementsBackEdge,
    BlockedRequirements,
    TraceDegraded,
    PartialGoal,
}

struct StandardHappyHarness {
    _fake_lark: FakeLark,
    _home: tempfile::TempDir,
    runtime: Arc<StateRuntime>,
    run_id: WorkflowRunId,
    lark: Arc<LarkInteractionService>,
    approvals: Arc<WorkflowApprovalService>,
    driver: WorkflowDriver,
    host: Arc<support::TestHost>,
    stage_threads: Vec<ThreadId>,
}

async fn planned_harness(
    plan_kind: HarnessPlan,
) -> Result<StandardHappyHarness, Box<dyn std::error::Error>> {
    let fake_lark = FakeLark::new()?;
    let home = tempfile::tempdir()?;
    let runtime =
        StateRuntime::init(home.path().to_path_buf(), "test-provider".to_string()).await?;
    let registry = Arc::new(default_registry()?);
    let name = WorkflowName::new("lark-rust-sdk-feature-development")?;
    let definition = registry.resolve(&name, None)?;
    let args = arguments();
    let arguments_json = serde_json::to_value(&args)?;
    let checkpoint = definition.initialize(arguments_json.clone())?;
    let run_id = WorkflowRunId::new();
    runtime
        .workflows()
        .create_run(WorkflowRunCreate {
            run_id: run_id.to_string(),
            definition_name: name.to_string(),
            definition_version: "1.0.0".to_string(),
            state_schema_version: checkpoint.state_schema_version(),
            state: checkpoint.state().clone(),
            arguments: arguments_json,
            non_interactive: false,
            detached: false,
            concurrency: Some(1),
            created_at_ms: now_ms(),
        })
        .await?;
    let artifacts = WorkflowArtifactStore::new(home.path(), runtime.workflows().clone());
    let source = write_artifact(&artifacts, run_id, "bootstrap/source.json", b"source").await?;
    let instructions = write_artifact(
        &artifacts,
        run_id,
        "bootstrap/instructions.md",
        b"instructions",
    )
    .await?;
    let requirements = write_artifact(
        &artifacts,
        run_id,
        "subjects/requirements.json",
        b"requirements",
    )
    .await?;
    let requirements_v2 = write_artifact(
        &artifacts,
        run_id,
        "subjects/requirements-v2.json",
        b"requirements-v2",
    )
    .await?;
    let design = write_artifact(&artifacts, run_id, "subjects/design.md", b"design").await?;
    let plan = write_artifact(&artifacts, run_id, "subjects/plan.md", b"plan").await?;
    let validation =
        write_artifact(&artifacts, run_id, "delivery/validation.log", b"validation").await?;
    let acceptance = write_artifact(
        &artifacts,
        run_id,
        "delivery/acceptance.json",
        b"acceptance",
    )
    .await?;
    let report = write_artifact(&artifacts, run_id, "delivery/report.md", b"report").await?;
    let stage_threads = (0..4).map(|_| ThreadId::new()).collect::<Vec<_>>();
    let outputs = match plan_kind {
        HarnessPlan::Happy | HarnessPlan::TraceDegraded | HarnessPlan::PartialGoal => vec![
            approval_stage_result(
                "requirements",
                "v1/1/stage-1-requirements/1",
                requirements,
                digest(b"requirements"),
                None,
            ),
            approval_stage_result(
                "technical_design",
                "v1/1/stage-2-technical-design/1",
                design,
                digest(b"design"),
                Some(("doc-design", "7")),
            ),
            approval_stage_result(
                "exec_plan_design",
                "v1/1/stage-3-exec-plan-design/1",
                plan,
                digest(b"plan"),
                Some(("doc-plan", "11")),
            ),
            execution_stage_result(
                validation,
                acceptance,
                report,
                &digest(args.goal_objective.as_bytes()),
            ),
        ],
        HarnessPlan::RequirementsBackEdge => vec![
            approval_stage_result(
                "requirements",
                "v1/1/stage-1-requirements/1",
                requirements,
                digest(b"requirements"),
                None,
            ),
            back_edge_stage_result(
                "technical_design",
                "v1/1/stage-2-technical-design/1",
                1,
                "return_to_requirements",
            ),
            approval_stage_result(
                "requirements",
                "v1/2/stage-1-requirements/1",
                requirements_v2,
                digest(b"requirements-v2"),
                None,
            ),
        ],
        HarnessPlan::BlockedRequirements => vec![blocked_stage_result(
            "requirements",
            "v1/1/stage-1-requirements/1",
            1,
        )],
    };
    let host = Arc::new(support::TestHost::with_plan(stage_threads.clone(), outputs));
    let slot = codex_workflow_extension::WorkflowNodeHostSlot::new();
    slot.bind(host.clone())?;
    let lark = Arc::new(LarkInteractionService::new(
        fake_lark.client(),
        runtime.workflows().clone(),
        artifacts.clone(),
    ));
    let approvals = Arc::new(WorkflowApprovalService::new(
        runtime.workflows().clone(),
        Arc::clone(&lark),
        artifacts.clone(),
    ));
    let feature = Arc::new(FakeFeatureCapability {
        bootstrap: LarkFeatureBootstrap {
            chat_id: "oc_approval_01".to_string(),
            group_owner_marker_sha256: "a".repeat(64),
            required_member_ids: vec!["ou_developer".to_string(), "ou_approver".to_string()],
            repository_identity_sha256: "b".repeat(64),
            source_manifest_artifact_id: source,
            instruction_ledger_artifact_id: instructions,
            trace_id: "1".repeat(32),
            root_span_id: "2".repeat(16),
            trace_context_id: "00000000-0000-4000-8000-000000000001".to_string(),
            observability_delivery_state: if matches!(plan_kind, HarnessPlan::TraceDegraded) {
                "degraded_pending_backfill".to_string()
            } else {
                "accepted_local".to_string()
            },
        },
        grilling_calls: Mutex::new(Vec::new()),
    });
    let goal = Arc::new(FakeGoalCapability {
        run_id,
        thread_id: stage_threads[3],
        snapshot: WorkflowGoalSnapshot {
            goal_id: "goal-stage-4".to_string(),
            thread_id: stage_threads[3],
            objective_sha256: digest(args.goal_objective.as_bytes()),
            status: if matches!(plan_kind, HarnessPlan::PartialGoal) {
                WorkflowGoalStatus::Active
            } else {
                WorkflowGoalStatus::Complete
            },
            token_budget: None,
            tokens_used: 12_345,
            time_used_seconds: 90,
        },
    });
    let service =
        WorkflowService::new_with_registry(runtime.workflows().clone(), slot, registry.clone())
            .with_artifact_store(artifacts)
            .with_approval_service(approvals.clone())
            .with_goal_capability(goal)
            .with_lark_feature_capability(feature);
    let driver = WorkflowDriver::new(
        runtime.workflows().clone(),
        registry,
        "lark-feature-revocation-test",
        30_000,
    )
    .with_lark_service(Arc::clone(&lark))
    .with_runtime_facets(service);
    Ok(StandardHappyHarness {
        _fake_lark: fake_lark,
        _home: home,
        runtime,
        run_id,
        lark,
        approvals,
        driver,
        host,
        stage_threads,
    })
}

async fn approve_current_gate(
    runtime: &StateRuntime,
    lark: &LarkInteractionService,
    run_id: WorkflowRunId,
    ordinal: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let run = runtime
        .workflows()
        .read_run(&run_id.to_string())
        .await?
        .expect("run");
    let approval_id = run.state["approval_id"]
        .as_str()
        .expect("approval ID in waiting checkpoint");
    let approval = runtime
        .workflows()
        .read_approval(&run_id.to_string(), approval_id)
        .await?
        .expect("approval");
    let correlation = runtime
        .workflows()
        .list_waiting_lark_interactions("oc_approval_01", 10)
        .await?
        .into_iter()
        .find(|record| record.allowed_senders == ["ou_approver"])
        .expect("waiting correlation");
    let nonce = &approval.request_hash[..24];
    let sha256 = approval.subject["sha256"].as_str().expect("subject digest");
    let event = LarkEvent {
        event_id: format!("evt-approve-{ordinal}"),
        event_type: "im.message.receive_v1".to_string(),
        created_at_ms: now_ms(),
        message_id: MessageId::parse(format!("om_approve_{ordinal}"))?,
        chat_id: ChatId::parse("oc_approval_01")?,
        thread_id: None,
        sender_id: OpenId::parse("ou_approver")?,
        message_type: "text".to_string(),
        text: format!(
            "APPROVE {nonce} {sha256} [wf:{}]",
            correlation.correlation_token
        ),
    };
    let outcomes = lark.ingest_event(&event, "test").await?;
    assert!(
        outcomes.iter().any(|outcome| {
            matches!(outcome, codex_state::WorkflowLarkResolveOutcome::Resolved)
        })
    );
    Ok(())
}

fn approval_stage_result(
    stage: &str,
    attempt: &str,
    artifact_id: ArtifactId,
    sha256: String,
    document: Option<(&str, &str)>,
) -> String {
    let generation = attempt
        .split('/')
        .nth(1)
        .and_then(|value| value.parse::<u32>().ok())
        .expect("stage attempt generation");
    let mut subject = json!({"artifact_id": artifact_id, "sha256": sha256});
    if let Some((document_id, revision)) = document {
        subject["document_id"] = json!(document_id);
        subject["document_revision"] = json!(revision);
    }
    json!({
        "schema_version":1,"stage_id":stage,"stage_attempt_id":attempt,
        "requirement_generation":generation,"disposition":"ready_for_gate",
        "output":{"approval_subject":subject},
        "produced_artifacts":[{"logical_name":format!("{stage}-subject"),"artifact_id":artifact_id,"sha256":sha256,"classification":"internal"}],
        "questions":[],"risks":[],"decisions":[],"validations":[],
        "trace_context":trace_context(stage),"requested_transition":{"kind":match stage {
            "requirements" => "request_requirements_gate",
            "technical_design" => "request_design_gate",
            "exec_plan_design" => "request_exec_plan_gate",
            _ => "invalid_stage",
        }}
    }).to_string()
}

fn back_edge_stage_result(stage: &str, attempt: &str, generation: u32, transition: &str) -> String {
    json!({
        "schema_version":1,"stage_id":stage,"stage_attempt_id":attempt,
        "requirement_generation":generation,"disposition":"return_to_prior_stage",
        "output":{},"produced_artifacts":[],"questions":[],"risks":[],
        "decisions":[],"validations":[],"trace_context":trace_context(stage),
        "requested_transition":{"kind":transition,"reason":"upstream decision required"}
    })
    .to_string()
}

fn blocked_stage_result(stage: &str, attempt: &str, generation: u32) -> String {
    json!({
        "schema_version":1,"stage_id":stage,"stage_attempt_id":attempt,
        "requirement_generation":generation,"disposition":"blocked",
        "output":{"blocker":{"code":"required_source_unavailable"}},
        "produced_artifacts":[],"questions":[],"risks":[],"decisions":[],
        "validations":[],"trace_context":trace_context(stage),
        "requested_transition":{"kind":"needs_operator","reason":"required source unavailable"},
        "error":{"code":"required_source_unavailable"}
    })
    .to_string()
}

fn execution_stage_result(
    validation: ArtifactId,
    acceptance: ArtifactId,
    report: ArtifactId,
    objective_sha256: &str,
) -> String {
    json!({
        "schema_version":1,"stage_id":"execution","stage_attempt_id":"v1/1/stage-4-execution/1",
        "requirement_generation":1,"disposition":"completed",
        "output":{
            "goal":{"goal_id":"goal-stage-4","thread_id":"{{thread_id}}","objective_sha256":objective_sha256,"status":"complete"},
            "validations":[{"result":"passed","exit_code":0,"stdout_artifact_id":validation}],
            "acceptance_results":[{"result":"passed","evidence_refs":[acceptance]}],
            "code_artifacts":[{"kind":"diff","locator":"artifact:code"}],
            "completion_report_artifact_id":report,
            "observability_delivery_state":"accepted_local"
        },
        "produced_artifacts":[
            {"logical_name":"validation","artifact_id":validation,"sha256":digest(b"validation"),"classification":"internal"},
            {"logical_name":"acceptance","artifact_id":acceptance,"sha256":digest(b"acceptance"),"classification":"internal"},
            {"logical_name":"report","artifact_id":report,"sha256":digest(b"report"),"classification":"internal"}
        ],
        "questions":[],"risks":[],"decisions":[],"validations":[],
        "trace_context":trace_context("execution"),
        "requested_transition":{"kind":"validate_delivery"}
    }).to_string()
}

async fn write_artifact(
    artifacts: &WorkflowArtifactStore,
    run_id: WorkflowRunId,
    relative_path: &str,
    bytes: &[u8],
) -> Result<ArtifactId, Box<dyn std::error::Error>> {
    let artifact_id = ArtifactId::new();
    artifacts
        .write(WorkflowArtifactWrite {
            run_id,
            artifact_id,
            relative_path: &PathBuf::from(relative_path),
            classification: ArtifactClassification::Internal,
            media_type: "application/octet-stream",
            bytes,
            created_at_ms: now_ms(),
        })
        .await?;
    Ok(artifact_id)
}

fn arguments() -> LarkFeatureArguments {
    LarkFeatureArguments {
        requirement_id: "REQ-E2E".to_string(),
        requirement_title: "Synthetic SDK feature".to_string(),
        requirement: "Implement one synthetic, locally verifiable SDK feature.".to_string(),
        requester: "ou_requester".to_string(),
        developers: vec!["ou_developer".to_string()],
        approvers: vec!["ou_approver".to_string()],
        approval_quorum: 1,
        repository_path: PathBuf::from("/sdk/repository"),
        worktree_path: PathBuf::from("/sdk/worktree"),
        branch: "feature/req-e2e".to_string(),
        base_commit: "a".repeat(40),
        design_template: "docx://template/revision/1".to_string(),
        lark_tenant: "tenant-test".to_string(),
        lark_credential_ref: "lark-test".to_string(),
        lark_chat_id: Some("oc_approval_01".to_string()),
        fornax_workspace: "fornax-test".to_string(),
        goal_objective: "Implement and validate the approved synthetic feature".to_string(),
        goal_token_budget: None,
        approval_timeout_ms: 60_000,
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn trace_context(seed: &str) -> serde_json::Value {
    let trace_id = "1".repeat(32);
    let span_id = digest(seed.as_bytes())[..16].to_string();
    json!({
        "trace_id": trace_id,
        "span_id": span_id,
        "trace_context_id": "00000000-0000-4000-8000-000000000001",
        "w3c": format!("00-{trace_id}-{span_id}-01"),
    })
}

fn now_ms() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_millis(),
    )
    .expect("timestamp")
}

struct FakeLark {
    _home: tempfile::TempDir,
    executable: PathBuf,
    empty_page: PathBuf,
}

impl FakeLark {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let home = tempfile::tempdir()?;
        let executable = home.path().join("lark-cli");
        let empty_page = home.path().join("empty-page.json");
        std::fs::write(&executable, FAKE_CLI)?;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
        std::fs::write(
            &empty_page,
            serde_json::to_vec(
                &json!({"data":{"messages":[],"total":0,"has_more":false,"page_token":null}}),
            )?,
        )?;
        Ok(Self {
            _home: home,
            executable,
            empty_page,
        })
    }

    fn client(&self) -> Arc<LarkCli> {
        Arc::new(LarkCli::new(LarkCliConfig {
            executable: self.executable.clone(),
            cwd: self.executable.parent().expect("parent").to_path_buf(),
            environment: LarkCliEnvironment::default()
                .with_value("PATH", "/usr/bin:/bin")
                .with_value("EMPTY_PAGE", self.empty_page.as_os_str()),
            timeout: Duration::from_secs(1),
            stdout_limit: 64 * 1024,
            stderr_limit: 4096,
        }))
    }
}

const FAKE_CLI: &str = r#"#!/bin/sh
case "$*" in
  *"im +chat-messages-list"*) /bin/cat "$EMPTY_PAGE" ;;
  *"im +messages-send"*) printf '{"data":{"message_id":"om_request_shared","chat_id":"oc_approval_01","create_time":"100"}}\n' ;;
  *) printf 'unsupported fake command\n' >&2; exit 2 ;;
esac
"#;
