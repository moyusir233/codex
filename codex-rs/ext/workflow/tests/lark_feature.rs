#![allow(clippy::expect_used)]

use std::path::PathBuf;

use codex_workflow_extension::ArtifactId;
use codex_workflow_extension::LarkFeatureArguments;
use codex_workflow_extension::LarkFeatureStageId;
use codex_workflow_extension::NodeApprovals;
use codex_workflow_extension::NodeSandbox;
use codex_workflow_extension::NodeWorkingDirectory;
use codex_workflow_extension::SkillInitialInvocation;
use codex_workflow_extension::SkillPolicy;
use codex_workflow_extension::WorkflowArguments;
use codex_workflow_extension::WorkflowName;
use codex_workflow_extension::default_registry;
use codex_workflow_extension::lark_feature_node_spec;
use codex_workflow_extension::lark_feature_prompt_asset;
use codex_workflow_extension::validate_lark_feature_stage_result_json;
use pretty_assertions::assert_eq;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;

#[test]
fn lark_feature_registry_assets_and_arguments_are_versioned() {
    let registry = default_registry().expect("registry");
    let name = WorkflowName::new("lark-rust-sdk-feature-development").expect("name");
    let definition = registry.resolve(&name, None).expect("definition");
    assert_eq!(definition.metadata().version().to_string(), "1.0.0");
    assert_eq!(definition.metadata().state_schema_version(), 1);
    assert!(
        definition
            .metadata()
            .argv_help()
            .contains("--worktree-path")
    );
    assert!(
        definition
            .metadata()
            .required_capabilities()
            .iter()
            .any(|capability| { capability == "lark.documents.single-writer-revision-aware.v1" })
    );

    let hashes = LarkFeatureStageId::ALL
        .into_iter()
        .map(|stage| {
            let asset = lark_feature_prompt_asset(stage);
            assert_eq!(asset.version, "1.0.0");
            assert_eq!(asset.content_sha256.len(), 64);
            assert!(asset.body.contains("Authoritative runtime input"));
            assert!(asset.body.contains("Final output contract"));
            let schema: serde_json::Value =
                serde_json::from_str(asset.output_schema).expect("schema");
            assert_eq!(schema["additionalProperties"], false);
            asset.content_sha256
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        hashes.len(),
        4,
        "each committed prompt must have its own hash"
    );

    valid_args().validate().expect("valid arguments");
    let mut invalid = valid_args();
    invalid.worktree_path = invalid.repository_path.clone();
    assert!(invalid.validate().is_err());
}

#[test]
fn lark_feature_release_manifest_binds_local_assets_and_disables_live_rollout() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest_path = crate_root.join("eval/lark_feature/release-manifest.json");
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&manifest_path).expect("read lark feature release manifest"),
    )
    .expect("parse lark feature release manifest");
    assert_eq!(manifest["release_decision"]["live_workflow"], "disabled");
    assert_eq!(
        manifest["release_decision"]["canary"],
        "not_run_not_authorized"
    );
    assert_eq!(manifest["validation"]["external_mutations"], false);
    assert_eq!(manifest["evaluation"]["results"]["false_completions"], 0);

    for group in [
        &manifest["source"]["implementation_files"],
        &manifest["prompts"]["files"],
        &manifest["schemas"]["files"],
        &manifest["evaluation"]["files"],
    ] {
        for (relative_path, expected_sha256) in group.as_object().expect("manifest file hash map") {
            assert!(!relative_path.starts_with('/'));
            assert!(!relative_path.contains(".."));
            let bytes = std::fs::read(crate_root.join(relative_path)).expect("read bound asset");
            assert_eq!(
                digest(&bytes),
                expected_sha256.as_str().expect("sha256 string"),
                "release manifest drift for {relative_path}"
            );
        }
    }
}

#[test]
fn lark_feature_grilling_contract_invokes_skill_and_requires_transport_adapter() {
    let args = valid_args();
    let requirements =
        lark_feature_node_spec(LarkFeatureStageId::Requirements, &args).expect("requirements spec");
    let SkillPolicy::AllowOnly(skills) = requirements.skills() else {
        panic!("requirements skills must be allow-listed");
    };
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].package.0, "grill-me");
    assert_eq!(
        skills[0].initial_invocation,
        SkillInitialInvocation::InvokeOnInitialTurn
    );
    assert!(matches!(
        requirements.approvals(),
        NodeApprovals::RejectWhenDetached
    ));
    assert!(matches!(requirements.sandbox(), NodeSandbox::Policy(_)));
    let registry = default_registry().expect("registry");
    let definition = registry
        .resolve(
            &WorkflowName::new("lark-rust-sdk-feature-development").expect("name"),
            None,
        )
        .expect("definition");
    assert!(
        definition
            .metadata()
            .required_capabilities()
            .iter()
            .any(|capability| capability == "lark.grilling-transport.v1")
    );

    let execution =
        lark_feature_node_spec(LarkFeatureStageId::Execution, &args).expect("execution spec");
    assert!(matches!(
        execution.working_directory(),
        NodeWorkingDirectory::Exact(path) if path.as_path() == args.worktree_path
    ));
    assert!(matches!(
        execution.approvals(),
        NodeApprovals::ExistingClient
    ));
    assert!(matches!(execution.sandbox(), NodeSandbox::Policy(_)));
    assert!(matches!(execution.skills(), SkillPolicy::Disabled));
}

#[test]
fn lark_feature_stage_contracts_fail_closed_for_stale_and_incomplete_results() {
    let subject = ArtifactId::new();
    let sha = "a".repeat(64);
    let valid = json!({
        "schema_version": 1,
        "stage_id": "requirements",
        "stage_attempt_id": "v1/1/stage-1-requirements/1",
        "requirement_generation": 1,
        "disposition": "ready_for_gate",
        "output": {"approval_subject": {"artifact_id": subject, "sha256": sha}},
        "produced_artifacts": [{
            "logical_name": "requirements-baseline.json",
            "artifact_id": subject,
            "sha256": sha,
            "classification": "internal"
        }],
        "questions": [],
        "risks": [],
        "decisions": [],
        "validations": [],
        "trace_context": trace_context("requirements"),
        "requested_transition": {"kind": "request_requirements_gate"}
    });
    validate_lark_feature_stage_result_json(
        &valid.to_string(),
        LarkFeatureStageId::Requirements,
        "v1/1/stage-1-requirements/1",
        1,
    )
    .expect("valid requirements result");
    let mut invalid_back_edge = valid.clone();
    invalid_back_edge["disposition"] = json!("return_to_prior_stage");
    invalid_back_edge["requested_transition"] = json!({"kind":"return_to_technical_design"});
    assert!(
        validate_lark_feature_stage_result_json(
            &invalid_back_edge.to_string(),
            LarkFeatureStageId::Requirements,
            "v1/1/stage-1-requirements/1",
            1,
        )
        .is_err(),
        "Stage 1 must not accept a forward or prior-stage bypass"
    );
    assert!(
        validate_lark_feature_stage_result_json(
            &valid.to_string(),
            LarkFeatureStageId::Requirements,
            "v1/2/stage-1-requirements/1",
            2,
        )
        .is_err()
    );

    let validation = ArtifactId::new();
    let acceptance = ArtifactId::new();
    let report = ArtifactId::new();
    let execution = execution_result(validation, acceptance, report, "complete", true);
    validate_lark_feature_stage_result_json(
        &execution.to_string(),
        LarkFeatureStageId::Execution,
        "v1/1/stage-4-execution/1",
        1,
    )
    .expect("evidence-complete execution");
    let incomplete = execution_result(validation, acceptance, report, "active", true);
    assert!(
        validate_lark_feature_stage_result_json(
            &incomplete.to_string(),
            LarkFeatureStageId::Execution,
            "v1/1/stage-4-execution/1",
            1,
        )
        .is_err()
    );
    let same_evidence = execution_result(validation, validation, report, "complete", true);
    assert!(
        validate_lark_feature_stage_result_json(
            &same_evidence.to_string(),
            LarkFeatureStageId::Execution,
            "v1/1/stage-4-execution/1",
            1,
        )
        .is_err()
    );
    let mut omitted_validation = execution_result(validation, acceptance, report, "complete", true);
    omitted_validation["output"]["validations"] = json!([]);
    assert!(
        validate_lark_feature_stage_result_json(
            &omitted_validation.to_string(),
            LarkFeatureStageId::Execution,
            "v1/1/stage-4-execution/1",
            1,
        )
        .is_err(),
        "a complete goal cannot substitute for omitted validation evidence"
    );
}

fn execution_result(
    validation: ArtifactId,
    acceptance: ArtifactId,
    report: ArtifactId,
    goal_status: &str,
    passed: bool,
) -> serde_json::Value {
    let validation_sha = digest(b"validation");
    let acceptance_sha = digest(b"acceptance");
    json!({
        "schema_version": 1,
        "stage_id": "execution",
        "stage_attempt_id": "v1/1/stage-4-execution/1",
        "requirement_generation": 1,
        "disposition": "completed",
        "output": {
            "goal": {
                "goal_id": "goal-1",
                "thread_id": "11111111-1111-4111-8111-111111111111",
                "objective_sha256": "c".repeat(64),
                "status": goal_status
            },
            "validations": [{
                "result": if passed { "passed" } else { "failed" },
                "exit_code": if passed { 0 } else { 1 },
                "stdout_artifact_id": validation
            }],
            "acceptance_results": [{
                "result": if passed { "passed" } else { "failed" },
                "evidence_refs": [acceptance]
            }],
            "code_artifacts": [{"kind": "diff", "locator": "artifact"}],
            "completion_report_artifact_id": report,
            "observability_delivery_state": "accepted_local"
        },
        "produced_artifacts": [
            {"logical_name":"validation", "artifact_id":validation, "sha256":validation_sha, "classification":"internal"},
            {"logical_name":"acceptance", "artifact_id":acceptance, "sha256":acceptance_sha, "classification":"internal"},
            {"logical_name":"report", "artifact_id":report, "sha256":"d".repeat(64), "classification":"internal"}
        ],
        "questions": [], "risks": [], "decisions": [], "validations": [],
        "trace_context": trace_context("execution"),
        "requested_transition": {"kind":"validate_delivery"}
    })
}

fn valid_args() -> LarkFeatureArguments {
    LarkFeatureArguments {
        requirement_id: "REQ-123".to_string(),
        requirement_title: "Add an SDK feature".to_string(),
        requirement: "Provide one observable synthetic SDK capability.".to_string(),
        requester: "ou_requester".to_string(),
        developers: vec!["ou_developer".to_string()],
        approvers: vec!["ou_approver".to_string()],
        approval_quorum: 1,
        repository_path: PathBuf::from("/sdk/repository"),
        worktree_path: PathBuf::from("/sdk/worktree"),
        branch: "feature/req-123".to_string(),
        base_commit: "a".repeat(40),
        design_template: "https://example.test/docx/template?revision=7".to_string(),
        lark_tenant: "tenant-test".to_string(),
        lark_credential_ref: "lark-test-bot".to_string(),
        lark_chat_id: Some("oc_test".to_string()),
        fornax_workspace: "workflow-test".to_string(),
        goal_objective: "Implement and validate the approved plan".to_string(),
        goal_token_budget: None,
        approval_timeout_ms: 60_000,
    }
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

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
