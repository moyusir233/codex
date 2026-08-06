from pathlib import Path

from fakes import FakeClient, FakeNoop
from ids import operation

from codex_fornax_trace_bridge.errors import BridgeError
from codex_fornax_trace_bridge.journal import Journal
from codex_fornax_trace_bridge.sdk import FornaxSdkSession
from codex_fornax_trace_bridge.service import BridgeService


def test_crash_after_sdk_dispatch_before_commit_becomes_ambiguous(
    tmp_path: Path, monkeypatch
) -> None:
    path = tmp_path / "journal.sqlite3"
    journal = Journal(path, "first")
    client = FakeClient()
    service = BridgeService(
        journal,
        FornaxSdkSession(lambda: client, FakeNoop()),
        "first",
    )

    def crash_instead_of_commit(_operation_id: str, _response: dict[str, object]) -> None:
        raise BridgeError("internal", "injected commit crash", 500)

    monkeypatch.setattr(journal, "succeed", crash_instead_of_commit)
    try:
        service.start_span(
            {
                "operationId": operation(1),
                "name": "root",
                "spanType": "root",
                "parent": {"type": "newTrace"},
            }
        )
    except BridgeError:
        pass
    assert len(client.spans) == 1
    journal.close()

    recovered = Journal(path, "second")
    assert recovered.operation(operation(1))["state"] == "ambiguous"
    recovered.close()


def test_sensitive_record_content_is_absent_from_journal_bytes(tmp_path: Path) -> None:
    path = tmp_path / "journal.sqlite3"
    client = FakeClient()
    service = BridgeService(
        Journal(path, "instance"),
        FornaxSdkSession(lambda: client, FakeNoop()),
        "instance",
    )
    span = service.start_span(
        {
            "operationId": operation(1),
            "name": "root",
            "spanType": "root",
            "parent": {"type": "newTrace"},
        }
    )
    canary = "CANARY-PRIVATE-PROMPT-92b129"
    service.record_span(
        span["spanHandleId"],
        {
            "operationId": operation(2),
            "record": {
                "type": "tags",
                "values": {"safe.note": canary},
            },
        },
    )
    service.finish_span(span["spanHandleId"], {"operationId": operation(3)})
    service.close()
    assert canary.encode() not in path.read_bytes()
