#![allow(clippy::expect_used)]

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use codex_workflow_extension::integrations::fornax::BridgeErrorEnvelope;
use codex_workflow_extension::integrations::fornax::BridgeHealth;
use codex_workflow_extension::integrations::fornax::FinishSpanRequest;
use codex_workflow_extension::integrations::fornax::FinishedSpan;
use codex_workflow_extension::integrations::fornax::FornaxBridgeConfig;
use codex_workflow_extension::integrations::fornax::FornaxBridgeEnvironment;
use codex_workflow_extension::integrations::fornax::FornaxBridgeError;
use codex_workflow_extension::integrations::fornax::FornaxSpanInput;
use codex_workflow_extension::integrations::fornax::FornaxSpanOutput;
use codex_workflow_extension::integrations::fornax::FornaxTraceWriter;
use codex_workflow_extension::integrations::fornax::RecordSpanRequest;
use codex_workflow_extension::integrations::fornax::RecordedSpan;
use codex_workflow_extension::integrations::fornax::RetryDisposition;
use codex_workflow_extension::integrations::fornax::SpanParent;
use codex_workflow_extension::integrations::fornax::SpanRecord;
use codex_workflow_extension::integrations::fornax::SpanType;
use codex_workflow_extension::integrations::fornax::StartSpanRequest;
use codex_workflow_extension::integrations::fornax::StartedSpan;
use codex_workflow_extension::integrations::fornax::TraceContent;
use codex_workflow_extension::integrations::fornax::TraceMessage;
use codex_workflow_extension::integrations::fornax::TracePromptArgument;
use codex_workflow_extension::integrations::fornax::TracePromptArgumentSource;
use pretty_assertions::assert_eq;
use serde::de::DeserializeOwned;

fn fixture<T: DeserializeOwned>(name: &str) -> T {
    let relative = Path::new("tests/fixtures/fornax-bridge/v1").join(name);
    let cargo_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(&relative);
    let path = if cargo_path.exists() {
        cargo_path
    } else {
        Path::new("codex-rs/ext/workflow").join(relative)
    };
    serde_json::from_slice(&std::fs::read(path).expect("read fixture")).expect("decode fixture")
}

#[test]
fn fornax_bridge_v1_golden_requests_round_trip_without_drift() {
    let start: StartSpanRequest = fixture("start-request.json");
    assert!(matches!(start.parent, SpanParent::PersistedContext { .. }));
    assert_eq!(
        serde_json::to_value(&start).expect("serialize start"),
        fixture::<serde_json::Value>("start-request.json")
    );

    let record: RecordSpanRequest = fixture("record-request.json");
    assert!(matches!(record.record, SpanRecord::Input { .. }));
    assert_eq!(
        serde_json::to_value(&record).expect("serialize record"),
        fixture::<serde_json::Value>("record-request.json")
    );

    let finish: FinishSpanRequest = fixture("finish-request.json");
    assert_eq!(
        serde_json::to_value(&finish).expect("serialize finish"),
        fixture::<serde_json::Value>("finish-request.json")
    );
}

#[test]
fn fornax_bridge_v1_golden_responses_decode_exact_required_fields() {
    let started: StartedSpan = fixture("start-response.json");
    assert_eq!(started.protocol_version, 1);
    assert_eq!(started.trace_id.len(), 32);
    let health: BridgeHealth = fixture("health-response.json");
    assert_eq!(health.sdk_version, "1.0.46");
    let recorded: RecordedSpan = fixture("record-response.json");
    assert_eq!(recorded.protocol_version, 1);
    let finished: FinishedSpan = fixture("finish-response.json");
    assert_eq!(finished.trace_id, started.trace_id);
    let error: BridgeErrorEnvelope = fixture("ambiguous-error.json");
    assert_eq!(error.error.retry, RetryDisposition::AfterStatusCheck);
}

#[test]
fn fornax_bridge_v1_rejects_unknown_tagged_union_variants() {
    let mut request: serde_json::Value = fixture("start-request.json");
    request["parent"]["type"] = serde_json::Value::String("futureVariant".to_string());
    assert!(serde_json::from_value::<StartSpanRequest>(request).is_err());
}

#[test]
fn fornax_bridge_v2_adds_all_pinned_sdk_span_types() {
    let relative = Path::new("tests/fixtures/fornax-bridge/v2/span-types.json");
    let cargo_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    let path = if cargo_path.exists() {
        cargo_path
    } else {
        Path::new("codex-rs/ext/workflow").join(relative)
    };
    let requests: Vec<StartSpanRequest> =
        serde_json::from_slice(&std::fs::read(path).expect("read fixture"))
            .expect("decode fixture");
    assert_eq!(
        requests
            .into_iter()
            .map(|request| request.span_type)
            .collect::<Vec<_>>(),
        vec![
            SpanType::Root,
            SpanType::Prompt,
            SpanType::Model,
            SpanType::Tool,
            SpanType::Agent,
            SpanType::Retriever,
        ]
    );
}

#[test]
fn fornax_bridge_v2_semantic_builders_cover_mandatory_six_type_fields() {
    let message = || TraceMessage {
        role: "user".to_string(),
        content: "[REDACTED:internal]".to_string(),
    };
    let inputs = vec![
        FornaxSpanInput::Root {
            contents: vec![TraceContent {
                content_type: "text".to_string(),
                text: "safe summary".to_string(),
            }],
        },
        FornaxSpanInput::Prompt {
            prompt_provider: "fornax".to_string(),
            prompt_key: "lark_sdk.feature.requirements".to_string(),
            prompt_version: "1.0.0".to_string(),
            templates: vec![message()],
            arguments: vec![TracePromptArgument {
                key: "requirement".to_string(),
                value: "[REDACTED:internal]".to_string(),
                source: TracePromptArgumentSource::Input,
            }],
        },
        FornaxSpanInput::Model {
            model_provider: "fornax".to_string(),
            model_name: "fixture-model".to_string(),
            messages: vec![message()],
        },
        FornaxSpanInput::Tool {
            tool_name: "lark.docs.fetch".to_string(),
            input: "document_id=hashed".to_string(),
        },
        FornaxSpanInput::Agent {
            agent_name: "technical_design".to_string(),
            agent_run_id: "attempt-1".to_string(),
            input: "handoff_sha256=fixture".to_string(),
        },
        FornaxSpanInput::Retriever {
            retriever_provider: "repository".to_string(),
            query: "symbol digest".to_string(),
        },
    ];
    assert_eq!(
        inputs
            .iter()
            .map(FornaxSpanInput::span_type)
            .collect::<Vec<_>>(),
        vec![
            SpanType::Root,
            SpanType::Prompt,
            SpanType::Model,
            SpanType::Tool,
            SpanType::Agent,
            SpanType::Retriever,
        ]
    );
    let records = inputs
        .into_iter()
        .flat_map(|input| input.into_records().expect("valid semantic input"))
        .map(|record| serde_json::to_value(record).expect("serialize record"))
        .collect::<Vec<_>>();
    let encoded = serde_json::to_string(&records).expect("encode records");
    for mandatory in [
        "prompt_provider",
        "prompt_key",
        "prompt_version",
        "model_provider",
        "model_name",
        "tool_name",
        "agent_name",
        "agent_run_id",
        "retriever_provider",
        "messages",
    ] {
        assert!(encoded.contains(mandatory), "missing {mandatory}");
    }
    assert!(!encoded.contains("messsages"));

    let outputs = vec![
        FornaxSpanOutput::Root {
            contents: vec![TraceContent {
                content_type: "text".to_string(),
                text: "complete".to_string(),
            }],
        },
        FornaxSpanOutput::Prompt {
            prompts: vec![message()],
        },
        FornaxSpanOutput::Model {
            choices: vec![TraceMessage {
                role: "assistant".to_string(),
                content: "[REDACTED:internal]".to_string(),
            }],
        },
        FornaxSpanOutput::Tool {
            output: serde_json::json!({ "status": "ok" }),
        },
        FornaxSpanOutput::Agent {
            output: "artifact_sha256=fixture".to_string(),
        },
        FornaxSpanOutput::Retriever {
            documents: vec![serde_json::json!({ "id": "digest-only" })],
        },
    ];
    for output in outputs {
        let records = output.into_records(0, None).expect("valid semantic output");
        assert!(matches!(records[0], SpanRecord::Output { .. }));
        assert!(matches!(
            &records[1],
            SpanRecord::Tags { values }
                if values.get("_status_code") == Some(&serde_json::json!(0))
        ));
    }
    assert!(
        FornaxSpanOutput::Agent {
            output: "failed".to_string(),
        }
        .into_records(1, None)
        .is_err()
    );
}

#[tokio::test]
async fn fornax_bridge_writer_stays_disabled_without_live_delivery_approval() {
    let config = FornaxBridgeConfig {
        executable: Path::new("/does/not/run").to_path_buf(),
        cwd: Path::new("/tmp").to_path_buf(),
        state_dir: Path::new("/tmp/fornax-bridge-does-not-run").to_path_buf(),
        environment: FornaxBridgeEnvironment::default(),
        lifecycle_timeout: Duration::from_secs(1),
        connect_timeout: Duration::from_secs(1),
        request_timeout: Duration::from_secs(1),
    };
    let result = FornaxTraceWriter::preflight(&config, &AtomicBool::new(false), false).await;
    assert!(matches!(
        result,
        Err(FornaxBridgeError::LiveDeliveryNotApproved)
    ));
}
