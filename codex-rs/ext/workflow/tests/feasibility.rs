use std::ffi::OsString;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use codex_workflow_extension::feasibility::FeasibilityProcessRunner;
use codex_workflow_extension::feasibility::FornaxCliCapabilities;
use codex_workflow_extension::feasibility::LarkCliCapabilities;
use codex_workflow_extension::feasibility::ProcessFailure;
use pretty_assertions::assert_eq;

#[test]
fn installed_cli_help_detects_only_verified_capabilities() {
    let fornax = FornaxCliCapabilities::detect(
        "fornax-cli v0.0.51\n",
        "\
Available Commands:
  get         Get a single trace
  list        List traces
      --format string   Output format: raw, json (indented JSON), pretty
      --timeout duration HTTP request timeout
",
    );
    assert_eq!(
        FornaxCliCapabilities {
            version: "fornax-cli v0.0.51".to_string(),
            trace_get: true,
            trace_list: true,
            trace_write: false,
            json_output: true,
            timeout: true,
        },
        fornax
    );

    let lark = LarkCliCapabilities::detect(
        "lark-cli version 1.0.0\n",
        "Flags:\n  --idempotency-key string\n",
        "Subscribe to Lark events via WebSocket (NDJSON output)\n  --force UNSAFE\n",
    );
    assert_eq!(
        LarkCliCapabilities {
            version: "lark-cli version 1.0.0".to_string(),
            message_idempotency: true,
            event_ndjson: true,
            event_force_is_unsafe: true,
        },
        lark
    );
}

#[cfg(unix)]
#[test]
fn fake_executable_covers_json_timeout_and_cancellation() -> Result<(), Box<dyn std::error::Error>>
{
    let temp = tempfile::tempdir()?;
    let script = temp.path().join("fake-cli.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\ncase \"$1\" in\njson) printf '{\"ok\":true}\\n';;\nwait) while :; do :; done;;\n*) printf 'unknown\\n' >&2; exit 7;;\nesac\n",
    )?;
    let shell = std::path::Path::new("/bin/sh");

    let output = FeasibilityProcessRunner::new(Duration::from_secs(1)).run(
        shell,
        &[script.clone().into_os_string(), OsString::from("json")],
        temp.path(),
        &AtomicBool::new(false),
    )?;
    assert_eq!(Some(0), output.status_code);
    let json: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(serde_json::json!({"ok": true}), json);

    let timeout = FeasibilityProcessRunner::new(Duration::from_millis(20)).run(
        shell,
        &[script.clone().into_os_string(), OsString::from("wait")],
        temp.path(),
        &AtomicBool::new(false),
    );
    assert!(matches!(timeout, Err(ProcessFailure::Timeout(_))));

    let cancelled = Arc::new(AtomicBool::new(false));
    let cancel_signal = Arc::clone(&cancelled);
    let cancellation_thread = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        cancel_signal.store(true, Ordering::Release);
    });
    let result = FeasibilityProcessRunner::new(Duration::from_secs(1)).run(
        shell,
        &[script.into_os_string(), OsString::from("wait")],
        temp.path(),
        &cancelled,
    );
    cancellation_thread.join().expect("cancellation thread");
    assert!(matches!(result, Err(ProcessFailure::Cancelled(_))));

    Ok(())
}
