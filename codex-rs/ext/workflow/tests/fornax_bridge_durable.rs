#![cfg(unix)]
#![allow(clippy::expect_used)]

use std::io::Write;
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use codex_state::StateRuntime;
use codex_state::WorkflowFornaxTraceState;
use codex_state::WorkflowRunCreate;
use codex_workflow_extension::integrations::fornax::DurableFornaxTraceWriter;
use codex_workflow_extension::integrations::fornax::FornaxBridgeConfig;
use codex_workflow_extension::integrations::fornax::FornaxBridgeEnvironment;
use codex_workflow_extension::integrations::fornax::FornaxDeliveryReporter;
use codex_workflow_extension::integrations::fornax::FornaxDeliveryState;
use codex_workflow_extension::integrations::fornax::FornaxSpanCorrelation;
use codex_workflow_extension::integrations::fornax::FornaxTraceWriter;
use codex_workflow_extension::integrations::fornax::SpanParent;
use codex_workflow_extension::integrations::fornax::SpanRecord;
use codex_workflow_extension::integrations::fornax::SpanType;
use serde_json::json;
use tiny_http::Header;
use tiny_http::Response;
use tiny_http::Server;
use uuid::Uuid;

fn fixture(name: &str) -> Vec<u8> {
    let relative = Path::new("tests/fixtures/fornax-bridge/v1").join(name);
    let cargo_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(&relative);
    let path = if cargo_path.exists() {
        cargo_path
    } else {
        Path::new("codex-rs/ext/workflow").join(relative)
    };
    std::fs::read(path).expect("read fixture")
}

#[tokio::test]
async fn fornax_bridge_durable_writer_persists_every_start_correlation() {
    let directory = tempfile::tempdir().expect("test directory");
    let bridge_state = directory.path().join("bridge");
    std::fs::create_dir(&bridge_state).expect("bridge state");
    std::fs::set_permissions(&bridge_state, std::fs::Permissions::from_mode(0o700))
        .expect("secure bridge state");
    let credential_file = bridge_state.join("credential.json");
    let mut credential = std::fs::File::create(&credential_file).expect("credential");
    write!(
        credential,
        "{{\"token\":\"{}\",\"fingerprintSalt\":\"{}\"}}",
        "t".repeat(48),
        "s".repeat(64)
    )
    .expect("write credential");
    std::fs::set_permissions(&credential_file, std::fs::Permissions::from_mode(0o600))
        .expect("secure credential");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("address");
    let server = Server::from_listener(listener, None).expect("server");
    let server_thread = std::thread::spawn(move || {
        for fixture_name in [
            "health-response.json",
            "start-response.json",
            "record-response.json",
            "finish-response.json",
        ] {
            let request = server.recv().expect("request");
            let mut response: serde_json::Value =
                serde_json::from_slice(&fixture(fixture_name)).expect("fixture JSON");
            response["protocolVersion"] = json!(2);
            if fixture_name == "health-response.json" {
                response["bridgeVersion"] = json!("0.2.0");
            }
            request
                .respond(
                    Response::from_data(serde_json::to_vec(&response).expect("response JSON"))
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json")
                                .expect("content type"),
                        ),
                )
                .expect("response");
        }
    });

    let lifecycle = directory.path().join("codex-fornax-trace");
    std::fs::write(
        &lifecycle,
        format!(
            "#!/bin/sh\nprintf '%s\\n' '{}'\n",
            json!({
                "status": "alreadyRunning",
                "protocolVersion": 2,
                "bridgeVersion": "0.2.0",
                "sdkVersion": "1.0.46",
                "instanceId": "fbi_fixture",
                "pid": std::process::id(),
                "endpoint": format!("http://{address}"),
                "credentialFile": credential_file,
            })
        ),
    )
    .expect("lifecycle script");
    std::fs::set_permissions(&lifecycle, std::fs::Permissions::from_mode(0o700))
        .expect("executable lifecycle");
    let config = FornaxBridgeConfig {
        executable: lifecycle,
        cwd: directory.path().to_path_buf(),
        state_dir: bridge_state,
        environment: FornaxBridgeEnvironment::default(),
        lifecycle_timeout: Duration::from_secs(2),
        connect_timeout: Duration::from_secs(1),
        request_timeout: Duration::from_secs(2),
    };
    let writer = FornaxTraceWriter::preflight(&config, &AtomicBool::new(false), true)
        .await
        .expect("preflight");

    let runtime = StateRuntime::init(
        directory.path().join("codex-home"),
        "test-provider".to_string(),
    )
    .await
    .expect("state runtime");
    runtime
        .workflows()
        .create_run(WorkflowRunCreate {
            run_id: "run-fornax-durable".to_string(),
            definition_name: "test-workflow".to_string(),
            definition_version: "1.0.0".to_string(),
            state_schema_version: 1,
            state: json!({}),
            arguments: json!({}),
            non_interactive: true,
            detached: false,
            concurrency: Some(1),
            created_at_ms: 100,
        })
        .await
        .expect("create run");
    let durable = DurableFornaxTraceWriter::new(
        writer,
        runtime.workflows().clone(),
        "run-fornax-durable",
        "fbi_fixture",
    );
    let operation_id =
        Uuid::parse_str("00000000-0000-4000-8000-000000000001").expect("operation UUID");
    let started = durable
        .start(
            "trace.root.start",
            operation_id,
            "prompt-review/planner",
            SpanType::Agent,
            SpanParent::NewTrace,
            101,
        )
        .await
        .expect("durable start");
    let correlation = runtime
        .workflows()
        .read_fornax_trace("run-fornax-durable", "trace.root.start")
        .await
        .expect("read correlation")
        .expect("correlation");
    let expected_handle = started.span_handle_id.to_string();
    assert_eq!(correlation.state, WorkflowFornaxTraceState::Live);
    assert_eq!(
        correlation.span_handle_id.as_deref(),
        Some(expected_handle.as_str())
    );
    assert_eq!(
        correlation.trace_id.as_deref(),
        Some(started.trace_id.as_str())
    );
    assert_eq!(
        correlation.span_id.as_deref(),
        Some(started.span_id.as_str())
    );
    assert_eq!(
        correlation.bridge_instance_id.as_deref(),
        Some("fbi_fixture")
    );
    let span_correlation = FornaxSpanCorrelation::from(&started);
    let record_operation =
        Uuid::parse_str("00000000-0000-4000-8000-000000000002").expect("record UUID");
    durable
        .record(
            "trace.root.tags",
            &span_correlation,
            record_operation,
            SpanRecord::Tags {
                values: [("result".to_string(), json!("ok"))].into(),
            },
            102,
        )
        .await
        .expect("durable record");
    let record = runtime
        .workflows()
        .read_fornax_trace("run-fornax-durable", "trace.root.tags")
        .await
        .expect("read record")
        .expect("record correlation");
    assert_eq!(record.state, WorkflowFornaxTraceState::Applied);
    assert_eq!(record.trace_context_id, correlation.trace_context_id);

    let finish_operation =
        Uuid::parse_str("00000000-0000-4000-8000-000000000003").expect("finish UUID");
    let finished = durable
        .finish(
            "trace.root.finish",
            &span_correlation,
            finish_operation,
            103,
        )
        .await
        .expect("durable finish");
    let finish = runtime
        .workflows()
        .read_fornax_trace("run-fornax-durable", "trace.root.finish")
        .await
        .expect("read finish")
        .expect("finish correlation");
    assert_eq!(finish.state, WorkflowFornaxTraceState::Finished);
    assert_eq!(finish.span_handle_id, correlation.span_handle_id);
    let reporter = FornaxDeliveryReporter::new(runtime.workflows().clone(), "run-fornax-durable");
    let backlog = reporter.backlog().await.expect("delivery backlog");
    assert_eq!(backlog.len(), 3);
    assert!(
        backlog
            .iter()
            .all(|report| report.state == FornaxDeliveryState::AcceptedLocal)
    );
    assert!(
        backlog
            .iter()
            .all(|report| report.state != FornaxDeliveryState::DeliveredRemote)
    );
    let proof = reporter
        .reconcile_remote(
            "trace.root.finish",
            "4bf92f3577b34da6a3ce929d0e0e4736",
            "00f067aa0ba902b7",
            "trace_get_full",
            &"a".repeat(64),
            105,
        )
        .await
        .expect("record remote reconciliation proof");
    assert_eq!(proof.operation_id, finish_operation.to_string());
    let reconciled = reporter.backlog().await.expect("reconciled backlog");
    let delivered = reconciled
        .iter()
        .find(|report| report.effect_key == "trace.root.finish")
        .expect("delivered report");
    assert_eq!(delivered.state, FornaxDeliveryState::DeliveredRemote);
    assert_eq!(
        delivered.remote_trace_id.as_deref(),
        Some("4bf92f3577b34da6a3ce929d0e0e4736")
    );
    assert!(
        reporter
            .reconcile_remote(
                "trace.root.finish",
                "different-trace",
                "00f067aa0ba902b7",
                "trace_get_full",
                &"a".repeat(64),
                106,
            )
            .await
            .is_err()
    );
    server_thread.join().expect("server thread");
    assert_eq!(
        finished,
        durable
            .finish(
                "trace.root.finish",
                &span_correlation,
                finish_operation,
                104,
            )
            .await
            .expect("replay applied finish without bridge")
    );
}
