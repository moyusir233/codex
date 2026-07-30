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
use codex_workflow_extension::integrations::fornax::FornaxTraceWriter;
use codex_workflow_extension::integrations::fornax::RecordSpanRequest;
use codex_workflow_extension::integrations::fornax::RecordedSpan;
use codex_workflow_extension::integrations::fornax::RetryDisposition;
use codex_workflow_extension::integrations::fornax::SpanParent;
use codex_workflow_extension::integrations::fornax::SpanRecord;
use codex_workflow_extension::integrations::fornax::StartSpanRequest;
use codex_workflow_extension::integrations::fornax::StartedSpan;
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
