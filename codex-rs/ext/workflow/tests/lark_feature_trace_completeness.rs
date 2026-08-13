#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use codex_workflow_extension::integrations::fornax::FornaxSpanInput;
use codex_workflow_extension::integrations::fornax::FornaxSpanOutput;
use codex_workflow_extension::integrations::fornax::SpanRecord;
use codex_workflow_extension::integrations::fornax::SpanType;
use codex_workflow_extension::integrations::fornax::TraceContent;
use codex_workflow_extension::integrations::fornax::TraceMessage;
use codex_workflow_extension::integrations::fornax::TracePromptArgument;
use codex_workflow_extension::integrations::fornax::TracePromptArgumentSource;
use serde_json::Value;
use serde_json::json;

struct CapturedSpan {
    name: String,
    span_type: SpanType,
    parent: Option<String>,
    records: Vec<SpanRecord>,
}

#[test]
fn lark_feature_trace_completeness_has_one_root_four_agents_and_all_six_types() {
    let run_id = "00000000-0000-4000-8000-000000000010";
    let requirement_id = "REQ-TRACE";
    let root_name = "lark_feature_workflow.run";
    let common_tags = || SpanRecord::Tags {
        values: BTreeMap::from([
            ("message_id".to_string(), json!(run_id)),
            ("thread_id".to_string(), json!(requirement_id)),
            ("workflow.run_id".to_string(), json!(run_id)),
            ("workflow.requirement_id".to_string(), json!(requirement_id)),
        ]),
    };
    let mut spans = Vec::new();
    spans.push(completed_span(
        root_name,
        None,
        FornaxSpanInput::Root {
            contents: vec![TraceContent {
                content_type: "text".to_string(),
                text: "REQ-TRACE synthetic feature".to_string(),
            }],
        },
        FornaxSpanOutput::Root {
            contents: vec![TraceContent {
                content_type: "text".to_string(),
                text: "artifact-only completion".to_string(),
            }],
        },
        common_tags(),
    ));
    let stages = [
        ("requirements", "lark_sdk.feature.requirements"),
        ("technical_design", "lark_sdk.feature.technical_design"),
        ("exec_plan_design", "lark_sdk.feature.exec_plan_design"),
        ("execution", "lark_sdk.feature.execution"),
    ];
    for (ordinal, (stage, prompt_key)) in stages.into_iter().enumerate() {
        let agent_name = format!("agent.{stage}");
        spans.push(completed_span(
            &agent_name,
            Some(root_name),
            FornaxSpanInput::Agent {
                agent_name: stage.to_string(),
                agent_run_id: format!("v1/1/{stage}/1"),
                input: "handoff_sha256=redacted-digest".to_string(),
            },
            FornaxSpanOutput::Agent {
                output: "result_sha256=redacted-digest".to_string(),
            },
            common_tags(),
        ));
        spans.push(completed_span(
            &format!("prompt.{stage}"),
            Some(&agent_name),
            FornaxSpanInput::Prompt {
                prompt_provider: "fornax".to_string(),
                prompt_key: prompt_key.to_string(),
                prompt_version: "1.0.0".to_string(),
                templates: vec![redacted_message("user")],
                arguments: vec![TracePromptArgument {
                    key: "stage_handoff".to_string(),
                    value: "[REDACTED:sensitive]".to_string(),
                    source: TracePromptArgumentSource::Input,
                }],
            },
            FornaxSpanOutput::Prompt {
                prompts: vec![redacted_message("user")],
            },
            common_tags(),
        ));
        spans.push(completed_span(
            &format!("model.{stage}"),
            Some(&agent_name),
            FornaxSpanInput::Model {
                model_provider: "fixture".to_string(),
                model_name: "fixture-model-v1".to_string(),
                messages: vec![redacted_message("user")],
            },
            FornaxSpanOutput::Model {
                choices: vec![redacted_message("assistant")],
            },
            common_tags(),
        ));
        spans.push(completed_span(
            &format!("workflow.stage.{}.artifact", ordinal + 1),
            Some(&agent_name),
            FornaxSpanInput::Tool {
                tool_name: "workflow.artifact.write".to_string(),
                input: "artifact_id=uuid;body=[REDACTED]".to_string(),
            },
            FornaxSpanOutput::Tool {
                output: json!({"status":"ok","sha256":"a".repeat(64)}),
            },
            common_tags(),
        ));
    }
    spans.push(completed_span(
        "repository.source.retrieve",
        Some("agent.technical_design"),
        FornaxSpanInput::Retriever {
            retriever_provider: "workflow_artifact_store".to_string(),
            query: "source_manifest_sha256".to_string(),
        },
        FornaxSpanOutput::Retriever {
            documents: vec![json!({"artifact_id":"redacted","sha256":"b".repeat(64)})],
        },
        common_tags(),
    ));

    assert_eq!(
        spans
            .iter()
            .filter(|span| span.span_type == SpanType::Root)
            .count(),
        1
    );
    let agents = spans
        .iter()
        .filter(|span| span.span_type == SpanType::Agent)
        .collect::<Vec<_>>();
    assert_eq!(agents.len(), 4);
    assert!(
        agents
            .iter()
            .all(|span| span.parent.as_deref() == Some(root_name))
    );
    assert_eq!(
        spans
            .iter()
            .map(|span| format!("{:?}", span.span_type))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "Root".to_string(),
            "Prompt".to_string(),
            "Model".to_string(),
            "Tool".to_string(),
            "Agent".to_string(),
            "Retriever".to_string(),
        ])
    );
    for span in &spans {
        assert!(
            span.records
                .iter()
                .any(|record| matches!(record, SpanRecord::Input { .. })),
            "{} has no input",
            span.name
        );
        assert!(
            span.records
                .iter()
                .any(|record| matches!(record, SpanRecord::Output { .. })),
            "{} has no output",
            span.name
        );
        assert!(span.records.iter().any(|record| {
            matches!(record, SpanRecord::Tags { values }
                if values.get("_status_code") == Some(&json!(0)))
        }));
        assert!(span.records.iter().any(|record| {
            matches!(record, SpanRecord::Tags { values }
                if values.get("message_id") == Some(&json!(run_id))
                    && values.get("thread_id") == Some(&json!(requirement_id)))
        }));
    }
    let encoded = serde_json::to_string(
        &spans
            .iter()
            .flat_map(|span| &span.records)
            .collect::<Vec<_>>(),
    )
    .expect("captured trace JSON");
    for forbidden in [
        "sk-live-secret-canary",
        "Authorization: Bearer",
        "person@example.com",
        "/Users/private-user",
    ] {
        assert!(!encoded.contains(forbidden));
    }
}

#[test]
fn lark_feature_trace_failure_fixture_requires_degraded_backlog_not_remote_claim() {
    let scenarios: Vec<Value> = serde_json::from_str(include_str!(
        "fixtures/lark_feature/scenarios/scenario-matrix.json"
    ))
    .expect("scenario matrix");
    let case = scenarios
        .iter()
        .find(|case| case["case_id"] == "LF-TRACE-FAIL-01")
        .expect("trace failure case");
    assert_eq!(
        case["expected_policy"]["terminal_class"],
        "degraded_continue"
    );
    assert!(
        case["required_actions"]
            .as_array()
            .expect("required actions")
            .iter()
            .any(|action| action == "durable_backlog")
    );
    assert!(
        case["forbidden_actions"]
            .as_array()
            .expect("forbidden actions")
            .iter()
            .any(|action| action == "claim_remote_delivery")
    );
}

fn completed_span(
    name: &str,
    parent: Option<&str>,
    input: FornaxSpanInput,
    output: FornaxSpanOutput,
    common_tags: SpanRecord,
) -> CapturedSpan {
    assert_eq!(input.span_type(), output.span_type());
    let span_type = input.span_type();
    let mut records = input.into_records().expect("semantic input");
    records.push(common_tags);
    records.extend(output.into_records(0, None).expect("semantic output"));
    CapturedSpan {
        name: name.to_string(),
        span_type,
        parent: parent.map(str::to_string),
        records,
    }
}

fn redacted_message(role: &str) -> TraceMessage {
    TraceMessage {
        role: role.to_string(),
        content: "[REDACTED:internal]".to_string(),
    }
}
