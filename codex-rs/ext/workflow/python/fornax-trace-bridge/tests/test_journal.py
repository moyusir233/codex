import sqlite3
from pathlib import Path

import pytest

from codex_fornax_trace_bridge.errors import BridgeError
from codex_fornax_trace_bridge.journal import Journal


def test_journal_replays_only_matching_applied_operation(tmp_path: Path) -> None:
    journal = Journal(tmp_path / "journal.sqlite3", "instance-1")
    assert journal.begin("op-1", "/route", "hash-1") is None
    journal.succeed("op-1", {"result": "ok"})
    assert journal.begin("op-1", "/route", "hash-1") == {"result": "ok"}
    with pytest.raises(BridgeError, match="different request") as conflict:
        journal.begin("op-1", "/route", "hash-2")
    assert conflict.value.code == "idempotencyConflict"
    journal.close()


def test_restart_marks_inflight_operations_ambiguous(tmp_path: Path) -> None:
    path = tmp_path / "journal.sqlite3"
    first = Journal(path, "instance-1")
    first.begin("op-running", "/route", "hash")
    first.close()

    second = Journal(path, "instance-2")
    operation = second.operation("op-running")
    assert operation is not None
    assert operation["state"] == "ambiguous"
    with pytest.raises(BridgeError) as ambiguous:
        second.begin("op-running", "/route", "hash")
    assert ambiguous.value.code == "ambiguousMutation"
    second.close()


def test_restart_orphans_live_handles_from_old_instance(tmp_path: Path) -> None:
    path = tmp_path / "journal.sqlite3"
    first = Journal(path, "instance-1")
    first.begin("creating-op", "/v1/spans", "hash")
    first.add_span(
        {
            "spanHandleId": "handle-1",
            "traceContextId": "context-1",
            "traceId": "trace",
            "spanId": "span",
            "w3c": "w3c",
        },
        "creating-op",
        {"x-flow-traceparent": "opaque"},
        "root",
        1,
    )
    assert first.live_span_count() == 1
    first.close()

    second = Journal(path, "instance-2")
    assert second.span("handle-1")["state"] == "orphaned"
    assert second.context_header("context-1") == {"x-flow-traceparent": "opaque"}
    assert second.live_span_count() == 0
    second.close()


def test_sqlite_journal_uses_wal_and_full_synchronous(tmp_path: Path) -> None:
    path = tmp_path / "journal.sqlite3"
    journal = Journal(path, "instance")
    with sqlite3.connect(path) as connection:
        assert connection.execute("PRAGMA journal_mode").fetchone()[0] == "wal"
        assert connection.execute("PRAGMA synchronous").fetchone()[0] == 2
    journal.close()
