from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor

import pytest
from fakes import FakeClient, FakeNoop
from ids import operation

from codex_fornax_trace_bridge.errors import BridgeError
from codex_fornax_trace_bridge.models import RecordSpan, SpanParent, StartSpan
from codex_fornax_trace_bridge.sdk import FornaxSdkSession


def start_request(index: int, parent: SpanParent | None = None) -> StartSpan:
    return StartSpan(
        operation_id=operation(index),
        name="workflow",
        span_type="root" if parent is None else "agent",
        parent=parent or SpanParent("newTrace"),
    )


def record(index: int, kind: str, value) -> RecordSpan:
    if kind in {"tags", "baggage"}:
        return RecordSpan(operation(index), kind, value, None, None)
    return RecordSpan(operation(index), kind, None, value, None)


def test_sdk_session_maps_parentage_setters_finish_and_close_once() -> None:
    client = FakeClient()
    session = FornaxSdkSession(lambda: client, FakeNoop())

    root = session.start(start_request(1))
    child = session.start(start_request(2, SpanParent("liveSpan", root["spanHandleId"])))
    remote = session.start(
        start_request(3, SpanParent("persistedContext", operation(99))),
        {"x-flow-traceparent": f"00-{'2' * 32}-{'3' * 16}-01"},
    )

    session.record(root["spanHandleId"], record(4, "tags", {"status": "ok"}))
    session.record(
        root["spanHandleId"],
        record(5, "input", {"type": "digest", "sha256": "a" * 64}),
    )
    assert client.parents[0].trace_info["span_id"] == ""
    assert client.parents[1] is client.spans[0]
    assert client.parents[2].trace_info["span_id"] == "3" * 16
    assert client.spans[0].records == [
        ("tags", None, {"status": "ok"}),
        ("input", None, {"type": "digest", "sha256": "a" * 64}),
    ]

    for response in (child, remote, root):
        assert session.finish(response["spanHandleId"])["state"] == "finished"
    session.close()
    session.close()
    assert client.close_count == 1
    assert len(client.thread_ids) == 1


def test_sdk_session_rejects_unknown_or_finished_handles() -> None:
    client = FakeClient()
    session = FornaxSdkSession(lambda: client, FakeNoop())
    with pytest.raises(BridgeError) as missing_parent:
        session.start(start_request(1, SpanParent("liveSpan", operation(99))))
    assert missing_parent.value.code == "unknownSpan"

    span = session.start(start_request(2))
    session.finish(span["spanHandleId"])
    with pytest.raises(BridgeError) as finished:
        session.record(span["spanHandleId"], record(3, "output", "done"))
    assert finished.value.code == "unknownSpan"
    session.close()


def test_sdk_session_serializes_concurrent_callers_on_one_worker() -> None:
    client = FakeClient()
    constructor_threads: list[int] = []

    def construct() -> FakeClient:
        import threading

        constructor_threads.append(threading.get_ident())
        return client

    session = FornaxSdkSession(construct, FakeNoop(), maximum_queue=64)
    with ThreadPoolExecutor(max_workers=16) as executor:
        spans = list(executor.map(lambda index: session.start(start_request(index + 1)), range(32)))
    assert len({span["spanHandleId"] for span in spans}) == 32
    assert constructor_threads == list(client.thread_ids)
    for span in spans:
        session.finish(span["spanHandleId"])
    session.close()


def test_sdk_session_refuses_close_with_active_spans() -> None:
    client = FakeClient()
    session = FornaxSdkSession(lambda: client, FakeNoop())
    span = session.start(start_request(1))
    with pytest.raises(BridgeError) as active:
        session.close()
    assert active.value.code == "activeSpans"
    session.finish(span["spanHandleId"])
    session.close()
