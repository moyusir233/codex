#![allow(clippy::expect_used)]

use std::future::Future;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;

use anyhow::Result;
use futures::SinkExt;
use futures::StreamExt;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use predicates::str::is_empty;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

fn codex_command(codex_home: &Path) -> Result<assert_cmd::Command> {
    let mut cmd = assert_cmd::Command::new(codex_utils_cargo_bin::cargo_bin("codex")?);
    cmd.env("CODEX_HOME", codex_home);
    Ok(cmd)
}

async fn start_fake_app_server<F, Fut>(handler: F) -> Result<(String, tokio::task::JoinHandle<()>)>
where
    F: FnOnce(WebSocketStream<tokio::net::TcpStream>) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept fake app-server");
        let websocket = accept_async(stream).await.expect("upgrade websocket");
        handler(websocket).await;
    });
    Ok((format!("ws://{address}"), task))
}

async fn read_json(websocket: &mut WebSocketStream<tokio::net::TcpStream>) -> Value {
    loop {
        let message = websocket
            .next()
            .await
            .expect("websocket message")
            .expect("valid websocket frame");
        match message {
            Message::Text(text) => {
                return serde_json::from_str(&text).expect("valid JSON-RPC message");
            }
            Message::Ping(payload) => websocket
                .send(Message::Pong(payload))
                .await
                .expect("send pong"),
            Message::Close(_) => panic!("websocket closed before expected message"),
            _ => {}
        }
    }
}

async fn write_json(websocket: &mut WebSocketStream<tokio::net::TcpStream>, value: Value) {
    websocket
        .send(Message::Text(value.to_string().into()))
        .await
        .expect("write JSON-RPC message");
}

async fn initialize_fake_app_server(websocket: &mut WebSocketStream<tokio::net::TcpStream>) {
    let initialize = read_json(websocket).await;
    assert_eq!(initialize["method"], "initialize");
    write_json(
        websocket,
        json!({
            "id": initialize["id"],
            "result": {
                "userAgent": "codex_cli_rs/test",
                "codexHome": "/server/.codex"
            }
        }),
    )
    .await;
    let initialized = read_json(websocket).await;
    assert_eq!(initialized["method"], "initialized");
}

fn run_snapshot(status: &str, next_sequence: u64) -> Value {
    json!({
        "runId": "wfr_01",
        "workflowName": "release",
        "workflowVersion": "1.0.0",
        "status": status,
        "arguments": {"topic": "launch"},
        "nonInteractive": false,
        "detached": false,
        "concurrency": Value::Null,
        "output": if status == "succeeded" { json!({"ok": true}) } else { Value::Null },
        "errorCode": Value::Null,
        "wake": Value::Null,
        "nextSequence": next_sequence,
        "nodes": [{
            "nodeId": "wfn_01",
            "nodeKey": "build",
            "threadId": "thr_02",
            "threadIds": ["thr_01", "thr_02"],
            "status": if status == "succeeded" { "succeeded" } else { "running" },
            "retryAtMs": Value::Null,
            "createdAtMs": 1,
            "updatedAtMs": 2
        }],
        "artifacts": if status == "succeeded" {
            json!([{
                "artifactId": "wfa_01",
                "relativePath": "reports/final.json",
                "classification": "public",
                "mediaType": "application/json",
                "sizeBytes": 12,
                "sha256": "abc",
                "createdAtMs": 2
            }])
        } else {
            json!([])
        },
        "createdAtMs": 1,
        "updatedAtMs": 2
    })
}

#[test]
fn workflow_help_publishes_the_generic_command_shape() -> Result<()> {
    let codex_home = TempDir::new()?;
    let mut cmd = codex_command(codex_home.path())?;
    cmd.args(["workflow", "--help"])
        .assert()
        .success()
        .stdout(contains(
            "codex workflow [runner-options...] <workflow-name> [workflow-args...]",
        ))
        .stdout(contains("--resume-run <run-id>"))
        .stdout(contains("--non-interactive"))
        .stdout(contains("--concurrency <n>"));
    Ok(())
}

#[test]
fn workflow_list_uses_the_embedded_registry() -> Result<()> {
    let codex_home = TempDir::new()?;
    let mut cmd = codex_command(codex_home.path())?;
    cmd.args(["workflow", "--list"])
        .assert()
        .success()
        .stdout(contains("prompt-review 1.0.0 (default)"));
    Ok(())
}

#[test]
fn workflow_json_list_is_clean_ndjson() -> Result<()> {
    let codex_home = TempDir::new()?;
    let mut cmd = codex_command(codex_home.path())?;
    let output = cmd
        .args(["workflow", "--json", "--list"])
        .assert()
        .success()
        .stderr(is_empty())
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output)?;
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    let definition: Value = serde_json::from_str(lines[0])?;
    assert_eq!(definition["type"], "definition");
    assert_eq!(definition["definition"]["name"], "prompt-review");
    let result: Value = serde_json::from_str(lines[1])?;
    assert_eq!(result["type"], "result");
    assert_eq!(result["definitionCount"], 1);
    Ok(())
}

#[test]
fn workflow_detach_rejects_an_in_process_host_with_usage_exit() -> Result<()> {
    let codex_home = TempDir::new()?;
    let mut cmd = codex_command(codex_home.path())?;
    cmd.args(["workflow", "--detach", "release"])
        .assert()
        .code(2)
        .stdout(is_empty())
        .stderr(contains("requires a persistent remote app-server"));
    Ok(())
}

#[test]
fn workflow_runner_options_stop_at_the_workflow_name() -> Result<()> {
    let codex_home = TempDir::new()?;

    let mut before_name = codex_command(codex_home.path())?;
    before_name
        .args(["workflow", "--json", "missing", "--topic", "release"])
        .assert()
        .code(2)
        .stdout(contains(r#""type":"error""#))
        .stderr(is_empty());

    let mut after_name = codex_command(codex_home.path())?;
    after_name
        .args(["workflow", "missing", "--json"])
        .assert()
        .code(2)
        .stdout(is_empty())
        .stderr(
            contains("workflow missing is not registered")
                .or(contains("workflow was not found"))
                .or(contains("no registered versions")),
        );
    Ok(())
}

#[test]
fn workflow_resume_rejects_a_positional_name() -> Result<()> {
    let codex_home = TempDir::new()?;
    let mut cmd = codex_command(codex_home.path())?;
    cmd.args(["workflow", "--resume-run", "wfr_01", "release"])
        .assert()
        .code(2)
        .stderr(contains("cannot be used with"));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workflow_remote_run_streams_clean_ndjson_and_resume_commands() -> Result<()> {
    let (endpoint, server) = start_fake_app_server(|mut websocket| async move {
        initialize_fake_app_server(&mut websocket).await;
        let run = read_json(&mut websocket).await;
        assert_eq!(run["method"], "workflow/run");
        assert_eq!(run["params"]["workflowName"], "prompt-review");
        assert_eq!(
            run["params"]["arguments"]["argv"],
            json!([
                "--prompt-key",
                "demo.prompt",
                "--reviewers",
                "3",
                "--lark-users",
                "ou_a,ou_b",
                "--lark-chat-id",
                "oc_review"
            ])
        );
        assert_eq!(run["params"]["subscribe"], true);
        write_json(
            &mut websocket,
            json!({
                "id": run["id"],
                "result": {"run": run_snapshot("running", 2)}
            }),
        )
        .await;
        write_json(
            &mut websocket,
            json!({
                "method": "workflowRun/updated",
                "params": {
                    "runId": "wfr_01",
                    "sequence": 2,
                    "createdAtMs": 2,
                    "status": "succeeded",
                    "metadata": {}
                }
            }),
        )
        .await;
        let read = read_json(&mut websocket).await;
        assert_eq!(read["method"], "workflowRun/read");
        write_json(
            &mut websocket,
            json!({
                "id": read["id"],
                "result": {"run": run_snapshot("succeeded", 3)}
            }),
        )
        .await;
    })
    .await?;

    let codex_home = TempDir::new()?;
    let codex = codex_utils_cargo_bin::cargo_bin("codex")?;
    let output = tokio::task::spawn_blocking(move || {
        Command::new(codex)
            .env("CODEX_HOME", codex_home.path())
            .env("no_proxy", "127.0.0.1,localhost")
            .env("NO_PROXY", "127.0.0.1,localhost")
            .args([
                "workflow",
                "--json",
                "--app-server",
                &endpoint,
                "prompt-review",
                "--prompt-key",
                "demo.prompt",
                "--reviewers",
                "3",
                "--lark-users",
                "ou_a,ou_b",
                "--lark-chat-id",
                "oc_review",
            ])
            .output()
    })
    .await??;
    server.await?;

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let lines = String::from_utf8(output.stdout)?;
    let values = lines
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(values.last().expect("result")["type"], "result");
    assert_eq!(
        values.last().expect("result")["resumeCommands"],
        json!(["codex resume thr_01", "codex resume thr_02"])
    );
    assert_eq!(
        values.last().expect("result")["run"]["artifacts"][0]["artifactId"],
        "wfa_01"
    );
    Ok(())
}

#[test]
fn workflow_example_embedded_launch_fails_closed_without_live_capabilities() -> Result<()> {
    let codex_home = TempDir::new()?;
    let mut cmd = codex_command(codex_home.path())?;
    cmd.args([
        "workflow",
        "prompt-review",
        "--prompt-key",
        "demo.prompt",
        "--lark-users",
        "ou_a",
        "--lark-chat-id",
        "oc_review",
    ])
    .assert()
    .code(1)
    .stderr(contains("requires unavailable capabilities"));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workflow_detach_forwards_typed_runner_policy_to_remote_server() -> Result<()> {
    let (endpoint, server) = start_fake_app_server(|mut websocket| async move {
        initialize_fake_app_server(&mut websocket).await;
        let run = read_json(&mut websocket).await;
        assert_eq!(run["method"], "workflow/run");
        assert_eq!(run["params"]["nonInteractive"], true);
        assert_eq!(run["params"]["detached"], true);
        assert_eq!(run["params"]["concurrency"], 3);
        assert_eq!(run["params"]["subscribe"], false);
        write_json(
            &mut websocket,
            json!({
                "id": run["id"],
                "result": {"run": run_snapshot("running", 2)}
            }),
        )
        .await;
    })
    .await?;

    let codex_home = TempDir::new()?;
    let codex = codex_utils_cargo_bin::cargo_bin("codex")?;
    let output = tokio::task::spawn_blocking(move || {
        Command::new(codex)
            .env("CODEX_HOME", codex_home.path())
            .env("no_proxy", "127.0.0.1,localhost")
            .env("NO_PROXY", "127.0.0.1,localhost")
            .args([
                "workflow",
                "--json",
                "--detach",
                "--non-interactive",
                "--concurrency",
                "3",
                "--app-server",
                &endpoint,
                "release",
            ])
            .output()
    })
    .await??;
    server.await?;

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(value["type"], "result");
    assert_eq!(value["detached"], true);
    assert_eq!(value["run"]["status"], "running");
    Ok(())
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workflow_first_sigint_cancels_and_exits_130() -> Result<()> {
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let (endpoint, server) = start_fake_app_server(|mut websocket| async move {
        initialize_fake_app_server(&mut websocket).await;
        let run = read_json(&mut websocket).await;
        write_json(
            &mut websocket,
            json!({
                "id": run["id"],
                "result": {"run": run_snapshot("running", 2)}
            }),
        )
        .await;
        ready_tx.send(()).expect("signal test is waiting");

        let cancel = read_json(&mut websocket).await;
        assert_eq!(cancel["method"], "workflowRun/cancel");
        write_json(
            &mut websocket,
            json!({
                "id": cancel["id"],
                "result": {"run": run_snapshot("cancelling", 2)}
            }),
        )
        .await;
        write_json(
            &mut websocket,
            json!({
                "method": "workflowRun/updated",
                "params": {
                    "runId": "wfr_01",
                    "sequence": 2,
                    "createdAtMs": 2,
                    "status": "cancelled",
                    "metadata": {}
                }
            }),
        )
        .await;
        let read = read_json(&mut websocket).await;
        assert_eq!(read["method"], "workflowRun/read");
        write_json(
            &mut websocket,
            json!({
                "id": read["id"],
                "result": {"run": run_snapshot("cancelled", 3)}
            }),
        )
        .await;
    })
    .await?;

    let codex_home = TempDir::new()?;
    let codex = codex_utils_cargo_bin::cargo_bin("codex")?;
    let child = Command::new(codex)
        .env("CODEX_HOME", codex_home.path())
        .env("no_proxy", "127.0.0.1,localhost")
        .env("NO_PROXY", "127.0.0.1,localhost")
        .args(["workflow", "--app-server", &endpoint, "release"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    ready_rx.await?;
    // SAFETY: `child.id()` is the exact live child created above and SIGINT is
    // the public cancellation contract under test.
    let kill_result = unsafe { libc::kill(child.id().cast_signed(), libc::SIGINT) };
    assert_eq!(kill_result, 0);
    let output = tokio::task::spawn_blocking(move || child.wait_with_output()).await??;
    server.await?;

    assert_eq!(output.status.code(), Some(130));
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("Workflow run wfr_01: Cancelled"));
    Ok(())
}
