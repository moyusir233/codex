import json
import threading
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path
from urllib.error import HTTPError
from urllib.request import ProxyHandler, Request, build_opener

import pytest
from fakes import FakeClient, FakeNoop
from ids import operation

from codex_fornax_trace_bridge import PROTOCOL_VERSION
from codex_fornax_trace_bridge.errors import BridgeError
from codex_fornax_trace_bridge.http_api import MAX_BODY, BridgeHttpServer
from codex_fornax_trace_bridge.journal import Journal
from codex_fornax_trace_bridge.lifecycle import Descriptor, request_json
from codex_fornax_trace_bridge.sdk import FornaxSdkSession
from codex_fornax_trace_bridge.service import BridgeService


def direct_open(request: Request):
    return build_opener(ProxyHandler({})).open(request, timeout=2)


@contextmanager
def running_server(tmp_path: Path) -> Iterator[tuple[BridgeHttpServer, Descriptor, str]]:
    journal = Journal(tmp_path / "journal.sqlite3", "instance")
    sdk = FornaxSdkSession(lambda: FakeClient(), FakeNoop())
    service = BridgeService(journal, sdk, "instance")
    server = BridgeHttpServer(("127.0.0.1", 0), service, "secret-token")
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    descriptor = Descriptor(
        instance_id="instance",
        pid=1,
        process_start="1",
        endpoint=f"http://127.0.0.1:{server.server_address[1]}",
        credential_file="/unused",
        protocol_version=int(PROTOCOL_VERSION),
        bridge_version="0.1.0",
        sdk_version="1.0.46",
    )
    try:
        yield server, descriptor, "secret-token"
    finally:
        server.shutdown()
        thread.join(timeout=5)
        server.server_close()
        if journal.live_span_count() == 0:
            service.close()


def start_body(index: int = 1) -> dict[str, object]:
    return {
        "operationId": operation(index),
        "name": "workflow",
        "spanType": "root",
        "parent": {"type": "newTrace"},
    }


def test_http_span_lifecycle_and_status_lookup(tmp_path: Path) -> None:
    with running_server(tmp_path) as (_server, descriptor, token):
        start = request_json(descriptor, token, "POST", "/v1/spans", start_body(), timeout=2)
        replay = request_json(descriptor, token, "POST", "/v1/spans", start_body(), timeout=2)
        assert replay == start
        handle = start["spanHandleId"]
        record = request_json(
            descriptor,
            token,
            "POST",
            f"/v1/spans/{handle}/records",
            {
                "operationId": operation(2),
                "record": {"type": "tags", "values": {"result": "ok"}},
            },
            timeout=2,
        )
        assert record["recorded"] == "tags"
        operation_status = request_json(
            descriptor,
            token,
            "GET",
            f"/v1/operations/{operation(2)}",
            None,
            timeout=2,
        )
        assert operation_status["state"] == "succeeded"
        span_status = request_json(descriptor, token, "GET", f"/v1/spans/{handle}", None, timeout=2)
        assert span_status["state"] == "live"
        assert "_header" not in repr(span_status)
        request_json(
            descriptor,
            token,
            "POST",
            f"/v1/spans/{handle}/finish",
            {"operationId": operation(3)},
            timeout=2,
        )


@pytest.mark.parametrize(
    ("headers", "code"),
    [
        ({"X-Codex-Fornax-Protocol": PROTOCOL_VERSION}, 401),
        ({"Authorization": "Bearer secret-token"}, 426),
    ],
)
def test_http_requires_bearer_and_protocol(
    tmp_path: Path, headers: dict[str, str], code: int
) -> None:
    with running_server(tmp_path) as (_server, descriptor, _token):
        request = Request(descriptor.endpoint + "/v1/health", headers=headers)
        with pytest.raises(HTTPError) as rejected:
            direct_open(request)
        assert rejected.value.code == code


def test_http_rejects_wrong_token_and_ignores_proxy_environment(
    tmp_path: Path, monkeypatch
) -> None:
    monkeypatch.setenv("HTTP_PROXY", "http://127.0.0.1:1")
    monkeypatch.setenv("HTTPS_PROXY", "http://127.0.0.1:1")
    with running_server(tmp_path) as (_server, descriptor, token):
        health = request_json(descriptor, token, "GET", "/v1/health", None, timeout=2)
        assert health["status"] == "ready"
        with pytest.raises(BridgeError) as unauthorized:
            request_json(descriptor, "wrong-token" * 8, "GET", "/v1/health", None, timeout=2)
        assert unauthorized.value.code == "localAuthentication"


def test_http_rejects_oversized_body_before_json_parse(tmp_path: Path) -> None:
    with running_server(tmp_path) as (_server, descriptor, token):
        request = Request(
            descriptor.endpoint + "/v1/spans",
            data=b"{}",
            method="POST",
            headers={
                "Authorization": f"Bearer {token}",
                "X-Codex-Fornax-Protocol": PROTOCOL_VERSION,
                "Content-Length": str(MAX_BODY + 1),
            },
        )
        with pytest.raises(HTTPError) as rejected:
            direct_open(request)
        assert rejected.value.code == 413
        assert json.load(rejected.value)["error"]["code"] == "invalidRequest"


def test_http_mutation_requires_matching_idempotency_header(tmp_path: Path) -> None:
    with running_server(tmp_path) as (_server, descriptor, token):
        body = json.dumps(start_body()).encode()
        request = Request(
            descriptor.endpoint + "/v1/spans",
            data=body,
            method="POST",
            headers={
                "Authorization": f"Bearer {token}",
                "X-Codex-Fornax-Protocol": PROTOCOL_VERSION,
                "Idempotency-Key": operation(99),
                "Content-Type": "application/json",
            },
        )
        with pytest.raises(HTTPError) as rejected:
            direct_open(request)
        assert json.load(rejected.value)["error"]["code"] == "invalidRequest"


def test_http_shutdown_refuses_active_spans(tmp_path: Path) -> None:
    with running_server(tmp_path) as (server, descriptor, token):
        start = request_json(descriptor, token, "POST", "/v1/spans", start_body(), timeout=2)
        with pytest.raises(BridgeError) as active:
            request_json(
                descriptor,
                token,
                "POST",
                "/v1/shutdown",
                {"mode": "rejectIfActive"},
                timeout=2,
            )
        assert active.value.code == "activeSpans"
        assert server._BaseServer__shutdown_request is False
        request_json(
            descriptor,
            token,
            "POST",
            f"/v1/spans/{start['spanHandleId']}/finish",
            {"operationId": operation(2)},
            timeout=2,
        )
