from __future__ import annotations

import threading
from typing import Any


class FakeSpan:
    def __init__(self, trace_id: str, span_id: str) -> None:
        self.trace_info = {
            "trace_id": trace_id,
            "span_id": span_id,
            "w3c": f"00-{trace_id}-{span_id}-01",
        }
        self.records: list[tuple[str, Any, Any]] = []
        self.finished = False

    def set_tag(self, values: dict[str, Any]) -> None:
        self.records.append(("tags", None, values))

    def set_baggage(self, values: dict[str, Any]) -> None:
        self.records.append(("baggage", None, values))

    def set_input(self, value: Any) -> None:
        self.records.append(("input", None, value))

    def set_output(self, value: Any) -> None:
        self.records.append(("output", None, value))

    def set_finish_time(self, value: float) -> None:
        self.records.append(("finishTime", None, value))

    def to_header(self) -> dict[str, str]:
        return {
            "x-flow-traceparent": (
                f"00-{self.trace_info['trace_id']}-{self.trace_info['span_id']}-01"
            )
        }

    def finish(self) -> None:
        self.finished = True


class FakeClient:
    def __init__(self) -> None:
        self.spans: list[FakeSpan] = []
        self.parents: list[Any] = []
        self.close_count = 0
        self.thread_ids: set[int] = set()

    def start_span(self, name: str, span_type: str, *, child_of: Any) -> FakeSpan:
        del name, span_type
        self._observe_thread()
        span = FakeSpan(f"{1:032x}", f"{len(self.spans) + 1:016x}")
        self.spans.append(span)
        self.parents.append(child_of)
        return span

    def get_span_from_header(self, header: dict[str, str]) -> FakeSpan:
        self._observe_thread()
        _, trace_id, span_id, _ = header["x-flow-traceparent"].split("-")
        return FakeSpan(trace_id, span_id)

    def close_trace(self) -> None:
        self._observe_thread()
        self.close_count += 1

    def _observe_thread(self) -> None:
        self.thread_ids.add(threading.get_ident())


class FakeNoop:
    def __init__(self) -> None:
        self.trace_info = {"trace_id": "", "span_id": "", "w3c": ""}
