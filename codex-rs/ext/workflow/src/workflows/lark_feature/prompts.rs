use std::collections::BTreeMap;

use serde_json::Value;
use sha2::Digest;
use sha2::Sha256;

use super::LarkFeatureStageId;
use super::PromptSnapshot;

pub struct BundledStagePrompt {
    pub name: &'static str,
    pub version: &'static str,
    pub fornax_key: &'static str,
    pub body: &'static str,
    pub output_schema: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LarkFeaturePromptAsset {
    pub stage: LarkFeatureStageId,
    pub name: &'static str,
    pub version: &'static str,
    pub fornax_key: &'static str,
    pub content_sha256: String,
    pub body: &'static str,
    pub output_schema: &'static str,
}

impl BundledStagePrompt {
    pub fn snapshot(&self) -> PromptSnapshot {
        PromptSnapshot {
            name: self.name.to_string(),
            version: self.version.to_string(),
            content_sha256: digest(self.body.as_bytes()),
            fornax_key: self.fornax_key.to_string(),
        }
    }
}

pub fn bundled_prompt(stage: LarkFeatureStageId) -> BundledStagePrompt {
    match stage {
        LarkFeatureStageId::Requirements => BundledStagePrompt {
            name: "lark-sdk-feature-requirements-clarification",
            version: "1.0.0",
            fornax_key: "lark_sdk.feature.requirements",
            body: include_str!("../../../prompts/lark_feature/01-requirements-clarification.md"),
            output_schema: include_str!("../../../schemas/lark_feature/requirements_result.json"),
        },
        LarkFeatureStageId::TechnicalDesign => BundledStagePrompt {
            name: "lark-sdk-feature-technical-design",
            version: "1.0.0",
            fornax_key: "lark_sdk.feature.technical_design",
            body: include_str!("../../../prompts/lark_feature/02-technical-design.md"),
            output_schema: include_str!(
                "../../../schemas/lark_feature/technical_design_result.json"
            ),
        },
        LarkFeatureStageId::ExecPlanDesign => BundledStagePrompt {
            name: "lark-sdk-feature-coding-exec-plan-design",
            version: "1.0.0",
            fornax_key: "lark_sdk.feature.exec_plan_design",
            body: include_str!("../../../prompts/lark_feature/03-coding-exec-plan-design.md"),
            output_schema: include_str!("../../../schemas/lark_feature/exec_plan_result.json"),
        },
        LarkFeatureStageId::Execution => BundledStagePrompt {
            name: "lark-sdk-feature-exec-plan-execution",
            version: "1.0.0",
            fornax_key: "lark_sdk.feature.execution",
            body: include_str!("../../../prompts/lark_feature/04-exec-plan-execution.md"),
            output_schema: include_str!("../../../schemas/lark_feature/execution_result.json"),
        },
    }
}

pub fn prompt_asset(stage: LarkFeatureStageId) -> LarkFeaturePromptAsset {
    let prompt = bundled_prompt(stage);
    LarkFeaturePromptAsset {
        stage,
        name: prompt.name,
        version: prompt.version,
        fornax_key: prompt.fornax_key,
        content_sha256: digest(prompt.body.as_bytes()),
        body: prompt.body,
        output_schema: prompt.output_schema,
    }
}

pub fn stage_handoff_schema() -> &'static str {
    include_str!("../../../schemas/lark_feature/stage_handoff.json")
}

pub fn workflow_output_schema() -> &'static str {
    include_str!("../../../schemas/lark_feature/workflow_output.json")
}

pub fn render_prompt(
    prompt: &BundledStagePrompt,
    variables: &BTreeMap<&'static str, Value>,
    handoff_json: &str,
) -> Result<String, String> {
    let mut rendered = prompt.body.to_string();
    for (name, value) in variables {
        let placeholder = format!("{{{{{name}}}}}");
        let value = serde_json::to_string_pretty(value)
            .map_err(|error| format!("could not serialize prompt variable {name}: {error}"))?;
        rendered = rendered.replace(&placeholder, &value);
    }
    if rendered.contains("{{") || rendered.contains("}}") {
        return Err("bundled prompt contains an unresolved host placeholder".to_string());
    }
    Ok(format!(
        "{rendered}\n\n## Canonical StageHandoffV1\n\n```json\n{handoff_json}\n```\n\nThe handoff above is complete and authoritative. Do not rely on prior chat history."
    ))
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
