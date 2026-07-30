from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from fakes import FakeClient, FakeNoop
from ids import operation

from codex_fornax_trace_bridge.journal import Journal
from codex_fornax_trace_bridge.sdk import FornaxSdkSession
from codex_fornax_trace_bridge.service import BridgeService


def test_concurrent_bridge_mutations_are_serialized_without_crossed_handles(
    tmp_path: Path,
) -> None:
    client = FakeClient()
    sdk = FornaxSdkSession(lambda: client, FakeNoop(), maximum_queue=64)
    service = BridgeService(Journal(tmp_path / "journal.sqlite3", "instance"), sdk, "instance")

    def lifecycle(index: int) -> tuple[str, str]:
        span = service.start_span(
            {
                "operationId": operation(index * 3 + 1),
                "name": f"root-{index}",
                "spanType": "root",
                "parent": {"type": "newTrace"},
            }
        )
        handle = span["spanHandleId"]
        service.record_span(
            handle,
            {
                "operationId": operation(index * 3 + 2),
                "record": {"type": "tags", "values": {"index": index}},
            },
        )
        service.finish_span(handle, {"operationId": operation(index * 3 + 3)})
        return handle, span["spanId"]

    with ThreadPoolExecutor(max_workers=20) as executor:
        results = list(executor.map(lifecycle, range(20)))
    assert len({handle for handle, _span_id in results}) == 20
    assert len({span_id for _handle, span_id in results}) == 20
    assert len(client.thread_ids) == 1
    service.close()
