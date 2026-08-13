use serde_json::Value;

use crate::ArtifactId;
use crate::WorkflowError;

use super::LarkFeatureStageId;
use super::StageResultV1;

pub struct ValidatedApprovalSubject {
    pub artifact_id: ArtifactId,
    pub sha256: String,
    pub document_id: Option<String>,
    pub document_revision: Option<String>,
}

pub struct ValidatedExecution {
    pub goal_id: String,
    pub goal_thread_id: String,
    pub objective_sha256: String,
    pub validation_artifact_id: ArtifactId,
    pub validation_sha256: String,
    pub acceptance_artifact_id: ArtifactId,
    pub acceptance_sha256: String,
    pub completion_report_artifact_id: ArtifactId,
    pub completion_report_sha256: String,
    pub observability_delivery_state: String,
}

pub fn validate_envelope(
    result: &StageResultV1,
    expected_stage: LarkFeatureStageId,
    expected_attempt: &str,
    expected_generation: u32,
) -> Result<(), WorkflowError> {
    if result.schema_version != 1
        || result.stage_id != expected_stage
        || result.stage_attempt_id != expected_attempt
        || result.requirement_generation != expected_generation
    {
        return Err(invalid("stage result correlation mismatch"));
    }
    let allowed = match expected_stage {
        LarkFeatureStageId::Execution => [
            "completed",
            "needs_input",
            "return_to_prior_stage",
            "blocked",
        ]
        .as_slice(),
        _ => [
            "ready_for_gate",
            "needs_input",
            "return_to_prior_stage",
            "blocked",
        ]
        .as_slice(),
    };
    if !allowed.contains(&result.disposition.as_str()) {
        return Err(invalid("stage result disposition is invalid"));
    }
    for artifact in &result.produced_artifacts {
        if artifact.logical_name.trim().is_empty()
            || !valid_sha256(&artifact.sha256)
            || !matches!(
                artifact.classification.as_str(),
                "public" | "internal" | "sensitive"
            )
        {
            return Err(invalid("produced artifact contract is invalid"));
        }
    }
    if result
        .questions
        .iter()
        .any(|question| question.question_id.trim().is_empty() || question.text.trim().is_empty())
    {
        return Err(invalid("question contract is invalid"));
    }
    validate_trace_context(&result.trace_context)?;
    Ok(())
}

fn validate_trace_context(value: &Value) -> Result<(), WorkflowError> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid("stage trace context is not an object"))?;
    if object.len() != 4
        || !["trace_id", "span_id", "trace_context_id", "w3c"]
            .iter()
            .all(|key| object.contains_key(*key))
    {
        return Err(invalid("stage trace context contains unapproved fields"));
    }
    let trace_id = string(value, "trace_id")?;
    let span_id = string(value, "span_id")?;
    let trace_context_id = string(value, "trace_context_id")?;
    let w3c = string(value, "w3c")?;
    if !lower_hex(trace_id, 32)
        || !lower_hex(span_id, 16)
        || uuid::Uuid::parse_str(trace_context_id).is_err()
        || w3c != format!("00-{trace_id}-{span_id}-01")
    {
        return Err(invalid("stage trace context is invalid or disconnected"));
    }
    Ok(())
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub fn validate_requested_transition(
    stage: LarkFeatureStageId,
    result: &StageResultV1,
) -> Result<(), WorkflowError> {
    let kind = transition_kind(result)?;
    let valid = match result.disposition.as_str() {
        "ready_for_gate" => matches!(
            (stage, kind),
            (
                LarkFeatureStageId::Requirements,
                "request_requirements_gate"
            ) | (LarkFeatureStageId::TechnicalDesign, "request_design_gate")
                | (LarkFeatureStageId::ExecPlanDesign, "request_exec_plan_gate")
        ),
        "completed" => stage == LarkFeatureStageId::Execution && kind == "validate_delivery",
        "needs_input" => matches!(
            (stage, kind),
            (LarkFeatureStageId::Requirements, "wait_for_input")
                | (LarkFeatureStageId::TechnicalDesign, "revise_design")
                | (LarkFeatureStageId::ExecPlanDesign, "revise_exec_plan")
                | (LarkFeatureStageId::Execution, "continue_execution")
        ),
        "return_to_prior_stage" => requested_back_edge(stage, result).is_ok(),
        "blocked" => kind == "needs_operator",
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(invalid(
            "requested transition is not valid for the stage disposition",
        ))
    }
}

pub fn requested_back_edge(
    stage: LarkFeatureStageId,
    result: &StageResultV1,
) -> Result<LarkFeatureStageId, WorkflowError> {
    match (stage, transition_kind(result)?) {
        (LarkFeatureStageId::TechnicalDesign, "return_to_requirements")
        | (LarkFeatureStageId::ExecPlanDesign, "return_to_requirements")
        | (LarkFeatureStageId::Execution, "return_to_requirements") => {
            Ok(LarkFeatureStageId::Requirements)
        }
        (LarkFeatureStageId::ExecPlanDesign, "return_to_technical_design")
        | (LarkFeatureStageId::Execution, "return_to_technical_design") => {
            Ok(LarkFeatureStageId::TechnicalDesign)
        }
        (LarkFeatureStageId::Execution, "return_to_exec_plan_design") => {
            Ok(LarkFeatureStageId::ExecPlanDesign)
        }
        _ => Err(invalid("invalid or forward requested back-edge")),
    }
}

fn transition_kind(result: &StageResultV1) -> Result<&str, WorkflowError> {
    result
        .requested_transition
        .get("kind")
        .and_then(Value::as_str)
        .filter(|kind| !kind.is_empty())
        .ok_or_else(|| invalid("requested transition kind is missing"))
}

pub fn validate_stage_result_json(
    raw: &str,
    expected_stage: LarkFeatureStageId,
    expected_attempt: &str,
    expected_generation: u32,
) -> Result<(), WorkflowError> {
    let result: StageResultV1 = serde_json::from_str(raw)
        .map_err(|error| invalid(format!("invalid stage result JSON: {error}")))?;
    validate_envelope(
        &result,
        expected_stage,
        expected_attempt,
        expected_generation,
    )?;
    validate_requested_transition(expected_stage, &result)?;
    if expected_stage == LarkFeatureStageId::Execution && result.disposition == "completed" {
        execution_evidence(&result)?;
    } else if result.disposition == "ready_for_gate" {
        approval_subject(expected_stage, &result)?;
    }
    Ok(())
}

pub fn approval_subject(
    stage: LarkFeatureStageId,
    result: &StageResultV1,
) -> Result<ValidatedApprovalSubject, WorkflowError> {
    if result.disposition != "ready_for_gate" {
        return Err(invalid("non-ready stage cannot request approval"));
    }
    let subject = result
        .output
        .get("approval_subject")
        .ok_or_else(|| invalid("stage output has no approval subject"))?;
    let artifact_id = string(subject, "artifact_id")?;
    let artifact_id = ArtifactId::parse(artifact_id)
        .map_err(|_| invalid("approval subject artifact ID is invalid"))?;
    let sha256 = string(subject, "sha256")?.to_string();
    if !valid_sha256(&sha256) {
        return Err(invalid("approval subject digest is invalid"));
    }
    let (document_id, document_revision) = match stage {
        LarkFeatureStageId::Requirements => (None, None),
        LarkFeatureStageId::TechnicalDesign | LarkFeatureStageId::ExecPlanDesign => (
            Some(string(subject, "document_id")?.to_string()),
            Some(string(subject, "document_revision")?.to_string()),
        ),
        LarkFeatureStageId::Execution => {
            return Err(invalid("execution does not have an approval subject"));
        }
    };
    if !result
        .produced_artifacts
        .iter()
        .any(|artifact| artifact.artifact_id == artifact_id && artifact.sha256 == sha256)
    {
        return Err(invalid(
            "approval subject is not present in produced artifacts",
        ));
    }
    Ok(ValidatedApprovalSubject {
        artifact_id,
        sha256,
        document_id,
        document_revision,
    })
}

pub fn execution_evidence(result: &StageResultV1) -> Result<ValidatedExecution, WorkflowError> {
    if result.stage_id != LarkFeatureStageId::Execution || result.disposition != "completed" {
        return Err(invalid("execution is not completed"));
    }
    let output = &result.output;
    let goal = output
        .get("goal")
        .ok_or_else(|| invalid("execution output has no goal"))?;
    if string(goal, "status")? != "complete" {
        return Err(invalid("execution goal is not complete"));
    }
    let objective_sha256 = string(goal, "objective_sha256")?.to_string();
    if !valid_sha256(&objective_sha256) {
        return Err(invalid("goal objective digest is invalid"));
    }
    let validations = output
        .get("validations")
        .and_then(Value::as_array)
        .filter(|values| !values.is_empty())
        .ok_or_else(|| invalid("execution has no validation evidence"))?;
    if validations.iter().any(|validation| {
        validation.get("result").and_then(Value::as_str) != Some("passed")
            || validation.get("exit_code").and_then(Value::as_i64) != Some(0)
    }) {
        return Err(invalid("a required validation did not pass"));
    }
    let validation_artifact_id = ArtifactId::parse(string(&validations[0], "stdout_artifact_id")?)
        .map_err(|_| invalid("validation artifact ID is invalid"))?;
    let validation_sha256 = artifact_digest(result, validation_artifact_id)?;
    let acceptance = output
        .get("acceptance_results")
        .and_then(Value::as_array)
        .filter(|values| !values.is_empty())
        .ok_or_else(|| invalid("execution has no acceptance evidence"))?;
    if acceptance
        .iter()
        .any(|criterion| criterion.get("result").and_then(Value::as_str) != Some("passed"))
    {
        return Err(invalid("an acceptance criterion did not pass"));
    }
    let acceptance_artifact_id = acceptance[0]
        .get("evidence_refs")
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("acceptance result has no evidence artifact"))?;
    let acceptance_artifact_id = ArtifactId::parse(acceptance_artifact_id)
        .map_err(|_| invalid("acceptance artifact ID is invalid"))?;
    if acceptance_artifact_id == validation_artifact_id {
        return Err(invalid(
            "validation and acceptance evidence must be independent artifacts",
        ));
    }
    let acceptance_sha256 = artifact_digest(result, acceptance_artifact_id)?;
    let completion_report_artifact_id =
        ArtifactId::parse(string(output, "completion_report_artifact_id")?)
            .map_err(|_| invalid("completion report artifact ID is invalid"))?;
    let completion_report_sha256 = artifact_digest(result, completion_report_artifact_id)?;
    if output
        .get("code_artifacts")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
    {
        return Err(invalid("execution has no code evidence"));
    }
    Ok(ValidatedExecution {
        goal_id: string(goal, "goal_id")?.to_string(),
        goal_thread_id: string(goal, "thread_id")?.to_string(),
        objective_sha256,
        validation_artifact_id,
        validation_sha256,
        acceptance_artifact_id,
        acceptance_sha256,
        completion_report_artifact_id,
        completion_report_sha256,
        observability_delivery_state: string(output, "observability_delivery_state")?.to_string(),
    })
}

fn artifact_digest(
    result: &StageResultV1,
    artifact_id: ArtifactId,
) -> Result<String, WorkflowError> {
    result
        .produced_artifacts
        .iter()
        .find(|artifact| artifact.artifact_id == artifact_id)
        .map(|artifact| artifact.sha256.clone())
        .filter(|sha256| valid_sha256(sha256))
        .ok_or_else(|| invalid("evidence artifact is absent from produced artifacts"))
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, WorkflowError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid(format!("missing required string `{key}`")))
}

pub fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn invalid(message: impl Into<String>) -> WorkflowError {
    WorkflowError::definition(message.into())
}
