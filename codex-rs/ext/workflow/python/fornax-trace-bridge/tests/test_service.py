import time
from pathlib import Path

import pytest
from fakes import FakeClient, FakeNoop
from ids import operation

from codex_fornax_trace_bridge.errors import BridgeError
from codex_fornax_trace_bridge.journal import Journal
from codex_fornax_trace_bridge.sdk import FornaxSdkSession
from codex_fornax_trace_bridge.service import BridgeService


def make_service(tmp_path: Path) -> tuple[BridgeService, FakeClient]:
    client = FakeClient()
    sdk = FornaxSdkSession(lambda: client, FakeNoop())
    journal = Journal(tmp_path / "journal.sqlite3", "instance")
    return BridgeService(journal, sdk, "instance"), client


def start_body(index: int = 1) -> dict[str, object]:
    return {
        "operationId": operation(index),
        "name": "workflow",
        "spanType": "root",
        "parent": {"type": "newTrace"},
    }


def finish_body(index: int) -> dict[str, object]:
    return {"operationId": operation(index)}


def test_service_replays_applied_operation_without_redispatch(tmp_path: Path) -> None:
    service, client = make_service(tmp_path)
    first = service.start_span(start_body())
    replay = service.start_span(start_body())
    assert replay == first
    assert len(client.spans) == 1

    service.finish_span(first["spanHandleId"], finish_body(2))
    service.prepare_shutdown()
    service.close()


def test_service_rejects_operation_id_conflict(tmp_path: Path) -> None:
    service, _client = make_service(tmp_path)
    first = service.start_span(start_body())
    with pytest.raises(BridgeError) as conflict:
        service.start_span({**start_body(), "name": "different"})
    assert conflict.value.code == "idempotencyConflict"
    service.finish_span(first["spanHandleId"], finish_body(2))
    service.close()


def test_prepare_shutdown_rejects_live_spans_then_stops_admission(tmp_path: Path) -> None:
    service, _client = make_service(tmp_path)
    span = service.start_span(start_body())
    with pytest.raises(BridgeError) as active:
        service.prepare_shutdown()
    assert active.value.code == "activeSpans"
    service.finish_span(span["spanHandleId"], finish_body(2))
    service.prepare_shutdown()
    with pytest.raises(BridgeError) as closing:
        service.start_span(start_body(3))
    assert closing.value.code == "shuttingDown"
    service.close()


def test_persisted_context_parents_recovery_child_after_restart(tmp_path: Path) -> None:
    path = tmp_path / "journal.sqlite3"
    first_client = FakeClient()
    first_sdk = FornaxSdkSession(lambda: first_client, FakeNoop())
    first = BridgeService(Journal(path, "first"), first_sdk, "first")
    old = first.start_span(start_body())
    first._journal.close()

    second_client = FakeClient()
    second_sdk = FornaxSdkSession(lambda: second_client, FakeNoop())
    second = BridgeService(Journal(path, "second"), second_sdk, "second")
    child = second.start_span(
        {
            "operationId": operation(2),
            "name": "recovery",
            "spanType": "agent",
            "parent": {
                "type": "persistedContext",
                "traceContextId": old["traceContextId"],
            },
        }
    )
    assert child["traceId"] == old["traceId"]
    with pytest.raises(BridgeError) as orphaned:
        second.finish_span(old["spanHandleId"], finish_body(3))
    assert orphaned.value.code == "orphanedSpan"
    second.finish_span(child["spanHandleId"], finish_body(4))
    second.close()


def test_queue_rejection_is_safely_replayed_with_same_operation(
    tmp_path: Path, monkeypatch
) -> None:
    client = FakeClient()
    sdk = FornaxSdkSession(lambda: client, FakeNoop())
    journal = Journal(tmp_path / "journal.sqlite3", "instance")
    service = BridgeService(journal, sdk, "instance")
    original = sdk.start
    calls = 0

    def reject_once(request, persisted_header=None, on_late_completion=None):
        nonlocal calls
        calls += 1
        if calls == 1:
            raise BridgeError("queueFull", "queue is full", 429, "safe")
        return original(request, persisted_header, on_late_completion)

    monkeypatch.setattr(sdk, "start", reject_once)
    with pytest.raises(BridgeError) as full:
        service.start_span(start_body())
    assert full.value.code == "queueFull"
    assert journal.operation(operation(1))["state"] == "notDispatched"
    span = service.start_span(start_body())
    assert len(client.spans) == 1
    service.finish_span(span["spanHandleId"], finish_body(2))
    service.close()


def test_sdk_auth_code_is_sanitized_and_classified(tmp_path: Path) -> None:
    class SdkAuthError(Exception):
        code = 600703002

    client = FakeClient()

    def reject(*_args, **_kwargs):
        raise SdkAuthError("AK-CANARY SK-CANARY")

    client.start_span = reject
    service = BridgeService(
        Journal(tmp_path / "journal.sqlite3", "instance"),
        FornaxSdkSession(lambda: client, FakeNoop()),
        "instance",
    )
    with pytest.raises(BridgeError) as rejected:
        service.start_span(start_body())
    assert rejected.value.code == "sdkAuthentication"
    assert "CANARY" not in rejected.value.message


def test_late_sdk_completion_reconciles_operation_and_span(tmp_path: Path) -> None:
    client = FakeClient()
    original_start = client.start_span

    def slow_start(*args, **kwargs):
        time.sleep(0.05)
        return original_start(*args, **kwargs)

    client.start_span = slow_start
    journal = Journal(tmp_path / "journal.sqlite3", "instance")
    service = BridgeService(
        journal,
        FornaxSdkSession(
            lambda: client,
            FakeNoop(),
            operation_timeout=0.005,
        ),
        "instance",
    )
    with pytest.raises(BridgeError) as timed_out:
        service.start_span(start_body())
    assert timed_out.value.code == "ambiguousMutation"

    deadline = time.monotonic() + 1
    while journal.operation(operation(1))["state"] != "succeeded":
        assert time.monotonic() < deadline
        time.sleep(0.005)
    response = journal.operation(operation(1))["response"]
    assert journal.span(response["spanHandleId"])["state"] == "live"
    service.finish_span(response["spanHandleId"], finish_body(2))
    service.close()
