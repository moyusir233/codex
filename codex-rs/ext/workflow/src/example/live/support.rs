use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use serde_json::Value;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;
use uuid::Uuid;

use crate::EffectKey;
use crate::NodeApprovals;
use crate::NodeKey;
use crate::NodeOutputClassification;
use crate::NodeSpec;
use crate::NodeTurnResult;
use crate::NodeTurnStatus;
use crate::SkillPolicy;
use crate::SkillSelector;
use crate::WorkflowError;
use crate::WorkflowRunId;
use crate::integrations::fornax::FornaxSpanCorrelation;
use crate::integrations::fornax::PromptDraft;

use super::super::PromptReviewPrepared;
use super::LivePromptReviewCapability;
use super::PromptReviewRuntimeConfig;

impl LivePromptReviewCapability {
    pub(super) fn clone_for_task(&self) -> Arc<Self> {
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

impl PromptReviewRuntimeConfig {
    pub(super) fn validate(&self) -> Result<(), WorkflowError> {
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

pub(super) fn node_spec(key: &str, skills: Vec<SkillSelector>) -> Result<NodeSpec, WorkflowError> {
    NodeSpec::builder(NodeKey::new(key).map_err(definition_error)?)
        .skills(SkillPolicy::AllowOnly(skills))
        .approvals(NodeApprovals::RejectWhenDetached)
        .output_classification(NodeOutputClassification::Internal)
        .build()
        .map_err(definition_error)
}

pub(super) fn completed_output(result: &NodeTurnResult) -> Result<String, WorkflowError> {
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

pub(super) fn reviewed_draft(
    bytes: &[u8],
    final_output: &str,
) -> Result<PromptDraft, WorkflowError> {
    let mut value: Value = serde_json::from_slice(bytes).map_err(definition_error)?;
    value["detail"]["prompt_template"]["messages"] = json!([{
        "role": "system",
        "content": final_output,
    }]);
    value["detail"]["prompt_template"]["variable_defs"] = json!([]);
    PromptDraft::new(value).map_err(definition_error)
}

pub(super) fn render_variables(draft: &Value, prompt_key: &str) -> BTreeMap<String, String> {
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

pub(super) fn correlation(
    prepared: &PromptReviewPrepared,
) -> Result<FornaxSpanCorrelation, WorkflowError> {
    Ok(FornaxSpanCorrelation {
        span_handle_id: Uuid::parse_str(&prepared.span_handle_id).map_err(definition_error)?,
        trace_context_id: Uuid::parse_str(&prepared.trace_context_id).map_err(definition_error)?,
        trace_id: prepared.trace_id.clone(),
        span_id: prepared.root_span_id.clone(),
    })
}

pub(super) fn operation_uuid(run_id: WorkflowRunId, key: &str) -> Uuid {
    let digest = Sha256::digest(format!("{run_id}:{key}").as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

pub(super) fn effect(value: &str) -> Result<EffectKey, WorkflowError> {
    EffectKey::new(value).map_err(definition_error)
}

pub(super) fn now_ms() -> Result<i64, WorkflowError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(definition_error)?
        .as_millis();
    i64::try_from(millis).map_err(definition_error)
}

pub(super) fn definition_error(error: impl std::fmt::Display) -> WorkflowError {
    WorkflowError::definition(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reviewed_draft_saves_the_final_output() {
        let draft = reviewed_draft(
            br#"{"detail":{"prompt_template":{"template_type":"normal","messages":[{"role":"user","content":"old"}],"variable_defs":[{"key":"topic","type":"string"}]}}}"#,
            "reviewed final prompt",
        )
        .expect("build reviewed draft");
        assert_eq!(
            draft.as_value()["detail"]["prompt_template"]["messages"],
            json!([{"role": "system", "content": "reviewed final prompt"}])
        );
        assert_eq!(
            draft.as_value()["detail"]["prompt_template"]["variable_defs"],
            json!([])
        );
    }
}
