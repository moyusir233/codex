from __future__ import annotations

import json
import os
import signal
import stat
import threading
import time
from pathlib import Path

import pytest
from fakes import FakeClient, FakeNoop
from ids import operation

from codex_fornax_trace_bridge import PROTOCOL_VERSION
from codex_fornax_trace_bridge.errors import BridgeError
from codex_fornax_trace_bridge.journal import Journal
from codex_fornax_trace_bridge.lifecycle import (
    Descriptor,
    _process_start,
    request_json,
    serve,
    status,
    stop,
)
from codex_fornax_trace_bridge.sdk import FornaxSdkSession


def test_status_rejects_token_outside_state_directory(tmp_path: Path) -> None:
    state_dir = tmp_path / "state"
    state_dir.mkdir(mode=0o700)
    token = tmp_path / "credential.json"
    token.write_text(json.dumps({"token": "x" * 48}))
    token.chmod(0o600)
    descriptor = Descriptor(
        instance_id="fbi_test",
        pid=os.getpid(),
        process_start=_process_start(os.getpid()),
        endpoint="http://127.0.0.1:12345",
        credential_file=str(token),
        protocol_version=int(PROTOCOL_VERSION),
        bridge_version="0.1.0",
        sdk_version="1.0.46",
    )
    (state_dir / "daemon.json").write_text(
        json.dumps(
            {
                "instanceId": descriptor.instance_id,
                "pid": descriptor.pid,
                "processStart": descriptor.process_start,
                "endpoint": descriptor.endpoint,
                "credentialFile": descriptor.credential_file,
                "protocolVersion": descriptor.protocol_version,
                "bridgeVersion": descriptor.bridge_version,
                "sdkVersion": descriptor.sdk_version,
            }
        )
    )
    result = status(state_dir, probe=False)
    assert result["status"] == "unhealthy"
    assert "state directory" in result["message"]


def test_status_rejects_group_readable_token(tmp_path: Path) -> None:
    state_dir = tmp_path / "state"
    state_dir.mkdir(mode=0o700)
    token = state_dir / "credential.json"
    token.write_text(json.dumps({"token": "x" * 48}))
    token.chmod(0o640)
    descriptor = Descriptor(
        instance_id="fbi_test",
        pid=os.getpid(),
        process_start=_process_start(os.getpid()),
        endpoint="http://127.0.0.1:12345",
        credential_file=str(token),
        protocol_version=int(PROTOCOL_VERSION),
        bridge_version="0.1.0",
        sdk_version="1.0.46",
    )
    (state_dir / "daemon.json").write_text(
        json.dumps(
            {
                "instanceId": descriptor.instance_id,
                "pid": descriptor.pid,
                "processStart": descriptor.process_start,
                "endpoint": descriptor.endpoint,
                "credentialFile": descriptor.credential_file,
                "protocolVersion": descriptor.protocol_version,
                "bridgeVersion": descriptor.bridge_version,
                "sdkVersion": descriptor.sdk_version,
            }
        )
    )
    result = status(state_dir, probe=False)
    assert result["status"] == "unhealthy"
    assert "permissions" in result["message"]


def test_stop_quarantines_stale_pid_identity_without_signalling(
    tmp_path: Path, monkeypatch
) -> None:
    state_dir = (tmp_path / "state").resolve()
    state_dir.mkdir(mode=0o700)
    credential = state_dir / "credential.json"
    credential.write_text(
        json.dumps(
            {
                "token": "x" * 48,
                "fingerprintSalt": "a" * 64,
                "configFingerprint": None,
            }
        )
    )
    credential.chmod(0o600)
    descriptor = {
        "instanceId": "fbi_stale",
        "pid": os.getpid(),
        "processStart": "definitely-not-current",
        "endpoint": "http://127.0.0.1:12345",
        "credentialFile": str(credential),
        "protocolVersion": int(PROTOCOL_VERSION),
        "bridgeVersion": "0.1.0",
        "sdkVersion": "1.0.46",
    }
    (state_dir / "daemon.json").write_text(json.dumps(descriptor))

    def reject_signal(*_args):
        raise AssertionError("stale PID must never be signalled")

    monkeypatch.setattr(os, "kill", reject_signal)
    assert stop(state_dir) == {"status": "stale_removed"}
    assert list(state_dir.glob("daemon.json.stale-*"))
    assert list(state_dir.glob("credential.json.stale-*"))


def test_serve_publishes_secure_descriptor_and_cleans_exact_instance(tmp_path: Path) -> None:
    state_dir = (tmp_path / "state").resolve()
    client = FakeClient()
    sdk = FornaxSdkSession(lambda: client, FakeNoop())
    observed: dict[str, object] = {}
    prior_sigterm = signal.getsignal(signal.SIGTERM)
    prior_sigint = signal.getsignal(signal.SIGINT)

    def stop_when_ready() -> None:
        expires = time.monotonic() + 5
        while time.monotonic() < expires:
            descriptor_path = state_dir / "daemon.json"
            if descriptor_path.exists():
                value = json.loads(descriptor_path.read_text())
                descriptor = Descriptor(
                    value["instanceId"],
                    value["pid"],
                    value["processStart"],
                    value["endpoint"],
                    value["credentialFile"],
                    value["protocolVersion"],
                    value["bridgeVersion"],
                    value["sdkVersion"],
                )
                token_path = Path(descriptor.credential_file)
                observed["descriptor_mode"] = stat.S_IMODE(descriptor_path.stat().st_mode)
                observed["token_mode"] = stat.S_IMODE(token_path.stat().st_mode)
                observed["state_mode"] = stat.S_IMODE(state_dir.stat().st_mode)
                token = json.loads(token_path.read_text())["token"]
                observed["health"] = request_json(
                    descriptor, token, "GET", "/v1/health", None, timeout=2
                )
                observed["shutdown"] = request_json(
                    descriptor,
                    token,
                    "POST",
                    "/v1/shutdown",
                    {"mode": "rejectIfActive"},
                    timeout=2,
                )
                return
            time.sleep(0.01)
        observed["error"] = "descriptor was not published"

    helper = threading.Thread(target=stop_when_ready)
    helper.start()
    serve(state_dir, sdk)
    helper.join(timeout=5)

    assert "error" not in observed
    assert observed["health"]["status"] == "ready"
    assert observed["shutdown"] == {"status": "stopping"}
    assert observed["state_mode"] == 0o700
    assert observed["descriptor_mode"] == 0o600
    assert observed["token_mode"] == 0o600
    assert not (state_dir / "daemon.json").exists()
    assert not (state_dir / "credential.json").exists()
    assert client.close_count == 1
    assert signal.getsignal(signal.SIGTERM) is prior_sigterm
    assert signal.getsignal(signal.SIGINT) is prior_sigint


def test_sigterm_orphans_active_handles_then_closes_trace_once(tmp_path: Path) -> None:
    state_dir = (tmp_path / "state").resolve()
    client = FakeClient()
    sdk = FornaxSdkSession(lambda: client, FakeNoop())
    observed: dict[str, object] = {}

    def terminate_with_active_span() -> None:
        expires = time.monotonic() + 5
        while time.monotonic() < expires:
            descriptor_path = state_dir / "daemon.json"
            if descriptor_path.exists():
                value = json.loads(descriptor_path.read_text())
                descriptor = Descriptor(
                    value["instanceId"],
                    value["pid"],
                    value["processStart"],
                    value["endpoint"],
                    value["credentialFile"],
                    value["protocolVersion"],
                    value["bridgeVersion"],
                    value["sdkVersion"],
                )
                token = json.loads(Path(descriptor.credential_file).read_text())["token"]
                observed["span"] = request_json(
                    descriptor,
                    token,
                    "POST",
                    "/v1/spans",
                    {
                        "operationId": operation(1),
                        "name": "active",
                        "spanType": "root",
                        "parent": {"type": "newTrace"},
                    },
                    timeout=2,
                )
                os.kill(os.getpid(), signal.SIGTERM)
                return
            time.sleep(0.01)

    helper = threading.Thread(target=terminate_with_active_span)
    helper.start()
    serve(state_dir, sdk)
    helper.join(timeout=5)
    assert client.close_count == 1
    journal = Journal(state_dir / "journal.sqlite3", "recovery")
    assert journal.span(observed["span"]["spanHandleId"])["state"] == "orphaned"
    journal.close()


@pytest.mark.parametrize("url", ["https://127.0.0.1:1", "http://localhost:1", "http://8.8.8.8:1"])
def test_request_json_rejects_non_loopback_descriptor_urls(url: str) -> None:
    descriptor = Descriptor("id", 1, "1", url, "/unused", int(PROTOCOL_VERSION), "0.1.0", "1.0.46")
    with pytest.raises(BridgeError) as invalid:
        request_json(descriptor, "secret", "GET", "/v1/health", None, timeout=0.1)
    assert invalid.value.code == "invalid_descriptor"
