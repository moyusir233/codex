"""One-client, one-worker mapping to the pinned Fornax SDK."""

from __future__ import annotations

import contextvars
import queue
import threading
import uuid
from collections.abc import Callable
from concurrent.futures import Future, TimeoutError
from dataclasses import dataclass
from typing import Any

from .config import PINNED_SDK_VERSION, resolve_sdk_version
from .errors import BridgeError, unavailable
from .models import RecordSpan, StartSpan, validated_trace_info


@dataclass(slots=True)
class _Task:
    action: Callable[[], Any]
    future: Future[Any]


@dataclass(slots=True)
class _SpanState:
    span: Any
    context: contextvars.Context
    trace_context_id: str


class SdkWorker:
    """Bounded serialized owner for every SDK object and ContextVar."""

    def __init__(self, maximum_queue: int = 1024) -> None:
        self._queue: queue.Queue[_Task | None] = queue.Queue(maxsize=maximum_queue)
        self._thread = threading.Thread(target=self._run, name="fornax-sdk-worker", daemon=True)
        self._thread.start()

    def call(self, action: Callable[[], Any], timeout: float = 30.0) -> Any:
        future: Future[Any] = Future()
        try:
            self._queue.put_nowait(_Task(action=action, future=future))
        except queue.Full as error:
            raise BridgeError(
                "queueFull", "Fornax SDK worker queue is full", 429, "safe"
            ) from error
        try:
            return future.result(timeout=timeout)
        except TimeoutError as error:
            raise BridgeError(
                "ambiguousMutation",
                "Fornax SDK operation exceeded its response deadline",
                409,
                "afterStatusCheck",
            ) from error

    def close(self) -> None:
        self._queue.put(None)
        self._thread.join(timeout=30)
        if self._thread.is_alive():
            raise unavailable("Fornax SDK worker did not stop")

    def queue_depth(self) -> int:
        return self._queue.qsize()

    def _run(self) -> None:
        while (task := self._queue.get()) is not None:
            try:
                result = task.action()
            except BaseException as error:  # noqa: BLE001 - transferred into the Future.
                task.future.set_exception(error)
            else:
                task.future.set_result(result)


class FornaxSdkSession:
    """Owns one explicit client and all live span handles."""

    def __init__(
        self,
        client_factory: Callable[[], Any],
        noop_span: Any,
        root_context_initializer: Callable[[], None] | None = None,
        maximum_queue: int = 1024,
        sdk_version: str = PINNED_SDK_VERSION,
    ) -> None:
        self._worker = SdkWorker(maximum_queue)
        self._client = self._worker.call(client_factory)
        self._noop_span = noop_span
        self._initialize_root = root_context_initializer or (lambda: None)
        self._handles: dict[str, _SpanState] = {}
        self._closed = False
        self.sdk_version = sdk_version

    @classmethod
    def from_environment(cls) -> FornaxSdkSession:
        import os

        ak = os.environ.get("FORNAX_AK")
        sk = os.environ.get("FORNAX_SK")
        if not ak or not sk:
            raise BridgeError("sdkAuthentication", "FORNAX_AK and FORNAX_SK are required", 503)
        region = os.environ.get("FORNAX_CUSTOM_REGION")
        from bytedance.fornax.infra import FornaxClient
        from bytedance.fornax.infra.trace.span import NoopFornaxSpan

        def construct_client() -> Any:
            if region:
                return FornaxClient(ak, sk, fornax_custom_region=region)
            return FornaxClient(ak, sk)

        def initialize_root() -> None:
            import bytedance.context
            import logid

            bytedance.context.set("logid", logid.generate())

        try:
            return cls(
                construct_client,
                NoopFornaxSpan,
                initialize_root,
                sdk_version=resolve_sdk_version(),
            )
        except BridgeError:
            raise
        except BaseException as error:
            code = next(
                (
                    str(value)
                    for name in ("code", "error_code", "status_code")
                    if isinstance((value := getattr(error, name, None)), (str, int))
                ),
                None,
            )
            if code == "600703002":
                raise BridgeError(
                    "sdkAuthentication",
                    "Fornax SDK authentication rejected AK/SK/region",
                    503,
                ) from error
            raise BridgeError(
                "sdkUnavailable",
                f"Fornax SDK client construction failed ({type(error).__name__})",
                503,
            ) from error

    def start(
        self,
        request: StartSpan,
        persisted_header: dict[str, str] | None = None,
    ) -> dict[str, Any]:
        if request.parent.kind == "liveSpan":
            parent_state = self._handles.get(request.parent.identifier or "")
            if parent_state is None:
                raise BridgeError("unknownSpan", "parent span is not live in this instance", 404)
            context = parent_state.context
        else:
            context = contextvars.Context()

        def in_context() -> dict[str, Any]:
            if request.parent.kind == "newTrace":
                self._initialize_root()
                parent = self._noop_span
            elif request.parent.kind == "liveSpan":
                parent = self._handles[request.parent.identifier or ""].span
            else:
                if persisted_header is None:
                    raise BridgeError("unknownContext", "persisted context was not found", 404)
                self._initialize_root()
                parent = self._client.get_span_from_header(persisted_header)
            span = self._client.start_span(request.name, request.span_type, child_of=parent)
            handle_id = str(uuid.uuid4())
            trace_context_id = str(uuid.uuid4())
            trace_id, span_id, w3c = validated_trace_info(span)
            header = _validated_header(span.to_header())
            self._handles[handle_id] = _SpanState(span, context, trace_context_id)
            return {
                "spanHandleId": handle_id,
                "traceContextId": trace_context_id,
                "traceId": trace_id,
                "spanId": span_id,
                "w3c": w3c,
                "_header": header,
            }

        return self._worker.call(lambda: context.run(in_context))

    def record(self, handle_id: str, request: RecordSpan) -> dict[str, Any]:
        state = self._handles.get(handle_id)
        if state is None:
            raise BridgeError("unknownSpan", "span is not live in this instance", 404)

        def in_context() -> dict[str, Any]:
            if request.record_type == "tags":
                state.span.set_tag(dict(request.values or {}))
            elif request.record_type == "baggage":
                state.span.set_baggage(dict(request.values or {}))
            elif request.record_type == "input":
                state.span.set_input(request.value)
            elif request.record_type == "output":
                state.span.set_output(request.value)
            elif request.record_type == "finishTime":
                state.span.set_finish_time(request.finish_time)
            response: dict[str, Any] = {
                "spanHandleId": handle_id,
                "recorded": request.record_type,
            }
            if request.record_type == "baggage":
                response["_header"] = _validated_header(state.span.to_header())
                response["traceContextId"] = state.trace_context_id
            return response

        return self._worker.call(lambda: state.context.run(in_context))

    def finish(self, handle_id: str) -> dict[str, Any]:
        state = self._handles.get(handle_id)
        if state is None:
            raise BridgeError("unknownSpan", "span is not live in this instance", 404)

        def in_context() -> dict[str, Any]:
            header = _validated_header(state.span.to_header())
            trace_id, span_id, w3c = validated_trace_info(state.span)
            state.span.finish()
            self._handles.pop(handle_id)
            return {
                "spanHandleId": handle_id,
                "traceContextId": state.trace_context_id,
                "traceId": trace_id,
                "spanId": span_id,
                "w3c": w3c,
                "state": "finished",
                "_header": header,
            }

        return self._worker.call(lambda: state.context.run(in_context))

    def close(self, *, force: bool = False) -> None:
        if self._closed:
            return

        def action() -> None:
            if self._handles and not force:
                raise BridgeError(
                    "activeSpans", "cannot close the SDK while spans remain live", 409
                )
            self._handles.clear()
            self._client.close_trace()

        self._worker.call(action)
        self._worker.close()
        self._closed = True

    def queue_depth(self) -> int:
        return self._worker.queue_depth()


def _validated_header(value: Any) -> dict[str, str]:
    if (
        not isinstance(value, dict)
        or not value
        or len(value) > 16
        or any(
            not isinstance(key, str)
            or not key
            or len(key) > 128
            or not isinstance(item, str)
            or not item
            or len(item) > 8192
            for key, item in value.items()
        )
    ):
        raise BridgeError("sdkRejected", "SDK returned an invalid trace header", 500)
    return dict(value)
