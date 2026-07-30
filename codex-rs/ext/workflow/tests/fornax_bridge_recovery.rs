#![cfg(unix)]
#![allow(clippy::expect_used)]

use std::io::Write;
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

use codex_workflow_extension::integrations::fornax::DiscoveryStatus;
use codex_workflow_extension::integrations::fornax::FORNAX_BRIDGE_PROTOCOL;
use codex_workflow_extension::integrations::fornax::FORNAX_BRIDGE_VERSION;
use codex_workflow_extension::integrations::fornax::FORNAX_SDK_VERSION;
use codex_workflow_extension::integrations::fornax::FornaxBridgeDiscovery;
use codex_workflow_extension::integrations::fornax::FornaxBridgeError;
use codex_workflow_extension::integrations::fornax::FornaxBridgeHttpClient;
use codex_workflow_extension::integrations::fornax::OperationState;
use codex_workflow_extension::integrations::fornax::SpanParent;
use codex_workflow_extension::integrations::fornax::SpanType;
use codex_workflow_extension::integrations::fornax::StartSpanRequest;
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

fn discovery(endpoint: String, credential_file: std::path::PathBuf) -> FornaxBridgeDiscovery {
    FornaxBridgeDiscovery {
        status: DiscoveryStatus::AlreadyRunning,
        protocol_version: FORNAX_BRIDGE_PROTOCOL,
        bridge_version: FORNAX_BRIDGE_VERSION.to_string(),
        sdk_version: FORNAX_SDK_VERSION.to_string(),
        instance_id: "fbi_fixture".to_string(),
        pid: std::process::id(),
        endpoint,
        credential_file,
    }
}

#[tokio::test]
async fn fornax_bridge_ambiguous_mutation_is_reconciled_by_explicit_status_check() {
    let directory = tempfile::tempdir().expect("state dir");
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
        .expect("secure state dir");
    let credential_file = directory.path().join("credential.json");
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
        let first = server.recv().expect("first request");
        assert_eq!(first.url(), "/v1/spans");
        assert!(first.headers().iter().any(|header| {
            header.field.equiv("Idempotency-Key")
                && header.value.as_str() == "00000000-0000-4000-8000-000000000001"
        }));
        first
            .respond(
                Response::from_data(fixture("ambiguous-error.json"))
                    .with_status_code(409)
                    .with_header(
                        Header::from_bytes("Content-Type", "application/json")
                            .expect("content type"),
                    ),
            )
            .expect("ambiguous response");
        let second = server.recv().expect("status request");
        assert_eq!(
            second.url(),
            "/v1/operations/00000000-0000-4000-8000-000000000001"
        );
        second
            .respond(
                Response::from_data(fixture("operation-ambiguous.json")).with_header(
                    Header::from_bytes("Content-Type", "application/json").expect("content type"),
                ),
            )
            .expect("status response");
    });

    let client = FornaxBridgeHttpClient::new(
        &discovery(format!("http://{address}"), credential_file),
        directory.path(),
        Duration::from_secs(1),
        Duration::from_secs(2),
    )
    .expect("client");
    let operation_id = Uuid::parse_str("00000000-0000-4000-8000-000000000001").expect("uuid");
    let error = client
        .start_span(&StartSpanRequest {
            operation_id,
            name: "planner".to_string(),
            span_type: SpanType::Agent,
            parent: SpanParent::NewTrace,
        })
        .await
        .expect_err("ambiguous mutation");
    assert!(matches!(
        error,
        FornaxBridgeError::AmbiguousMutation {
            operation_id: observed
        } if observed == operation_id
    ));
    let status = client.operation_status(operation_id).await.expect("status");
    assert_eq!(status.state, OperationState::Ambiguous);
    server_thread.join().expect("server thread");
}

#[test]
fn fornax_bridge_credential_outside_state_directory_is_rejected_before_network() {
    let state = tempfile::tempdir().expect("state");
    let outside = tempfile::NamedTempFile::new().expect("credential");
    let result = FornaxBridgeHttpClient::new(
        &discovery(
            "http://127.0.0.1:1".to_string(),
            outside.path().to_path_buf(),
        ),
        state.path(),
        Duration::from_millis(10),
        Duration::from_millis(10),
    );
    assert!(matches!(result, Err(FornaxBridgeError::InsecureDescriptor)));
}
