from __future__ import annotations

import json
import os

import pytest
from ids import operation

from codex_fornax_trace_bridge.models import RecordSpan, SpanParent, StartSpan
from codex_fornax_trace_bridge.sdk import FornaxSdkSession

pytestmark = pytest.mark.skipif(
    os.environ.get("FORNAX_LIVE_TEST") != "1",
    reason="requires explicit disposable-workspace approval and SDK credentials",
)


def test_finished_and_flushed_trace_is_ready_for_read_reconciliation(capsys) -> None:
    session = FornaxSdkSession.from_environment()
    root = session.start(
        StartSpan(operation(1), "codex-live-contract", "root", SpanParent("newTrace"))
    )
    session.record(
        root["spanHandleId"],
        RecordSpan(operation(2), "tags", {"contract": "codex-m8b"}, None, None),
    )
    session.record(
        root["spanHandleId"],
        RecordSpan(
            operation(3),
            "input",
            None,
            {"type": "digest", "sha256": "a" * 64, "byteCount": 0},
            None,
        ),
    )
    child = session.start(
        StartSpan(
            operation(4),
            "codex-live-child",
            "agent",
            SpanParent("liveSpan", root["spanHandleId"]),
        )
    )
    session.record(
        child["spanHandleId"],
        RecordSpan(
            operation(5),
            "output",
            None,
            {"type": "digest", "sha256": "b" * 64, "byteCount": 0},
            None,
        ),
    )
    session.finish(child["spanHandleId"])
    session.finish(root["spanHandleId"])
    session.close()
    print(
        json.dumps(
            {
                "traceId": root["traceId"],
                "summary": "one root and one child finished; close_trace completed",
            },
            sort_keys=True,
        )
    )
    assert root["traceId"] in capsys.readouterr().out
