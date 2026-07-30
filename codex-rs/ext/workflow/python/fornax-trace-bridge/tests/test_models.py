from __future__ import annotations

import pytest
from ids import operation

from codex_fornax_trace_bridge.errors import BridgeError
from codex_fornax_trace_bridge.models import FinishSpan, RecordSpan, StartSpan


def test_start_span_accepts_exact_tagged_parents() -> None:
    for parent in (
        {"type": "newTrace"},
        {"type": "liveSpan", "spanHandleId": operation(2)},
        {"type": "persistedContext", "traceContextId": operation(3)},
    ):
        request = StartSpan.parse(
            {
                "operationId": operation(1),
                "name": "child",
                "spanType": "agent",
                "parent": parent,
            }
        )
        assert request.parent.kind == parent["type"]


@pytest.mark.parametrize(
    "body",
    [
        {
            "operationId": operation(1),
            "name": "n",
            "spanType": "root",
            "parent": {"type": "newTrace"},
            "extra": True,
        },
        {
            "operationId": "not-a-uuid",
            "name": "n",
            "spanType": "root",
            "parent": {"type": "newTrace"},
        },
        {
            "operationId": operation(1),
            "name": "bad\nname",
            "spanType": "root",
            "parent": {"type": "newTrace"},
        },
    ],
)
def test_start_span_rejects_unknown_unstable_or_unbounded_shapes(
    body: dict[str, object],
) -> None:
    with pytest.raises(BridgeError):
        StartSpan.parse(body)


@pytest.mark.parametrize(
    "record",
    [
        {"type": "tags", "values": {"result": "ok"}},
        {"type": "baggage", "values": {"workflow.id": "safe"}},
        {
            "type": "input",
            "value": {
                "type": "digest",
                "sha256": "a" * 64,
                "byteCount": 1,
                "classification": "sensitive",
            },
        },
        {
            "type": "output",
            "value": {
                "type": "digest",
                "sha256": "b" * 64,
                "byteCount": 1,
                "classification": "sensitive",
            },
        },
        {"type": "finishTime", "unixSeconds": 1_753_860_000.125},
    ],
)
def test_record_span_accepts_exact_tagged_shapes(record: dict[str, object]) -> None:
    assert (
        RecordSpan.parse({"operationId": operation(1), "record": record}).record_type
        == record["type"]
    )


def test_record_span_rejects_large_values_and_unsafe_baggage() -> None:
    with pytest.raises(BridgeError, match="65536"):
        RecordSpan.parse(
            {
                "operationId": operation(1),
                "record": {
                    "type": "input",
                    "value": {
                        "type": "json",
                        "value": "x" * (64 * 1024),
                        "classification": "nonSensitive",
                        "policyGrant": True,
                    },
                },
            }
        )
    with pytest.raises(BridgeError, match="policy grant"):
        RecordSpan.parse(
            {
                "operationId": operation(1),
                "record": {
                    "type": "input",
                    "value": {
                        "type": "plainText",
                        "value": "private",
                        "classification": "sensitive",
                        "policyGrant": True,
                    },
                },
            }
        )
    with pytest.raises(BridgeError, match="comma"):
        RecordSpan.parse(
            {
                "operationId": operation(1),
                "record": {"type": "baggage", "values": {"key": "bad,value"}},
            }
        )


def test_finish_span_rejects_unknown_fields() -> None:
    with pytest.raises(BridgeError, match="unexpected"):
        FinishSpan.parse({"operationId": operation(1), "force": True})
