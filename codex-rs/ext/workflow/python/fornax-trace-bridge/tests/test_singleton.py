from __future__ import annotations

import json
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from fakes import FakeClient, FakeNoop

from codex_fornax_trace_bridge.lifecycle import (
    Descriptor,
    ensure,
    request_json,
    serve,
)
from codex_fornax_trace_bridge.sdk import FornaxSdkSession


def test_twenty_concurrent_ensure_calls_discover_one_instance_and_client(
    tmp_path: Path,
) -> None:
    state_dir = (tmp_path / "state").resolve()
    client = FakeClient()
    sdk = FornaxSdkSession(lambda: client, FakeNoop())
    observed: dict[str, object] = {}

    def ensure_and_stop() -> None:
        expires = time.monotonic() + 5
        while time.monotonic() < expires and not (state_dir / "daemon.json").exists():
            time.sleep(0.01)
        with ThreadPoolExecutor(max_workers=20) as executor:
            results = list(executor.map(lambda _index: ensure(state_dir), range(20)))
        observed["results"] = results
        descriptor_value = json.loads((state_dir / "daemon.json").read_text())
        descriptor = Descriptor(
            descriptor_value["instanceId"],
            descriptor_value["pid"],
            descriptor_value["processStart"],
            descriptor_value["endpoint"],
            descriptor_value["credentialFile"],
            descriptor_value["protocolVersion"],
            descriptor_value["bridgeVersion"],
            descriptor_value["sdkVersion"],
        )
        token = json.loads(Path(descriptor.credential_file).read_text())["token"]
        request_json(
            descriptor,
            token,
            "POST",
            "/v1/shutdown",
            {"mode": "rejectIfActive"},
            timeout=2,
        )

    helper = threading.Thread(target=ensure_and_stop)
    helper.start()
    serve(state_dir, sdk)
    helper.join(timeout=10)

    results = observed["results"]
    assert len({result["instanceId"] for result in results}) == 1
    assert len({result["pid"] for result in results}) == 1
    assert {result["status"] for result in results} == {"alreadyRunning"}
    assert client.close_count == 1
