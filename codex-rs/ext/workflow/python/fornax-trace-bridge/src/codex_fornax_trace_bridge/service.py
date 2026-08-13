"""Journal-before-dispatch bridge service."""

import hashlib
import threading
from collections.abc import Callable
from typing import Any

from .errors import BridgeError
from .journal import Journal
from .models import FinishSpan, RecordSpan, StartSpan, validate_semantic_record
from .redaction import canonical_json
from .sdk import FornaxSdkSession


class BridgeService:
    """Maps protocol operations to one SDK session with durable idempotency."""

    def __init__(self, journal: Journal, sdk: FornaxSdkSession, instance_id: str) -> None:
        self._journal = journal
        self._sdk = sdk
        self.instance_id = instance_id
        self._admission_lock = threading.RLock()
        self._closing = False

    def start_span(
        self, body: dict[str, Any], *, protocol_version: int = 1
    ) -> dict[str, Any]:
        request = StartSpan.parse(body, protocol_version)
        persisted_header = None
        if request.parent.kind == "liveSpan":
            self._require_live_span(request.parent.identifier or "")
        elif request.parent.kind == "persistedContext":
            persisted_header = self._journal.context_header(request.parent.identifier or "")
            if persisted_header is None:
                raise BridgeError("unknownContext", "trace context was not found", 404)

        def dispatch() -> dict[str, Any]:
            def finalize(response: dict[str, Any]) -> dict[str, Any]:
                response = dict(response)
                header = response.pop("_header")
                response.update(
                    {
                        "operationId": request.operation_id,
                        "protocolVersion": protocol_version,
                    }
                )
                self._journal.add_span(
                    response,
                    request.operation_id,
                    header,
                    request.span_type,
                    protocol_version,
                )
                return response

            def late(future: Any) -> None:
                self._reconcile_late(request.operation_id, future, finalize)

            return finalize(
                self._sdk.start(
                    request,
                    persisted_header,
                    on_late_completion=late,
                )
            )

        return self._mutate(
            request.operation_id, "/v1/spans", body, dispatch, protocol_version
        )

    def record_span(
        self,
        span_handle_id: str,
        body: dict[str, Any],
        *,
        protocol_version: int = 1,
    ) -> dict[str, Any]:
        request = RecordSpan.parse(body)
        span = self._require_live_span(span_handle_id)
        contract = self._journal.span_contract(span_handle_id)
        if contract and contract["protocol_version"] >= 2:
            validate_semantic_record(contract["span_type"], request)

        def dispatch() -> dict[str, Any]:
            def finalize(response: dict[str, Any]) -> dict[str, Any]:
                response = dict(response)
                header = response.pop("_header", None)
                if header is not None:
                    self._journal.refresh_context(span["traceContextId"], header)
                self._journal.record_contract(span_handle_id, request)
                response.update(
                    {
                        "operationId": request.operation_id,
                        "protocolVersion": protocol_version,
                    }
                )
                return response

            def late(future: Any) -> None:
                self._reconcile_late(request.operation_id, future, finalize)

            return finalize(
                self._sdk.record(
                    span_handle_id,
                    request,
                    on_late_completion=late,
                )
            )

        route = f"/v1/spans/{span_handle_id}/records"
        return self._mutate(request.operation_id, route, body, dispatch, protocol_version)

    def finish_span(
        self,
        span_handle_id: str,
        body: dict[str, Any],
        *,
        protocol_version: int = 1,
    ) -> dict[str, Any]:
        request = FinishSpan.parse(body)
        span = self._require_live_span(span_handle_id)
        self._journal.verify_contract(span_handle_id)

        def dispatch() -> dict[str, Any]:
            def finalize(response: dict[str, Any]) -> dict[str, Any]:
                response = dict(response)
                header = response.pop("_header")
                self._journal.refresh_context(span["traceContextId"], header)
                self._journal.finish_span(span_handle_id)
                response.update(
                    {
                        "operationId": request.operation_id,
                        "protocolVersion": protocol_version,
                    }
                )
                return response

            def late(future: Any) -> None:
                self._reconcile_late(request.operation_id, future, finalize)

            return finalize(self._sdk.finish(span_handle_id, on_late_completion=late))

        route = f"/v1/spans/{span_handle_id}/finish"
        return self._mutate(request.operation_id, route, body, dispatch, protocol_version)

    def operation(self, operation_id: str) -> dict[str, Any] | None:
        return self._journal.operation(operation_id)

    def span(self, span_handle_id: str) -> dict[str, Any] | None:
        return self._journal.span(span_handle_id)

    def health(self) -> dict[str, Any]:
        return {
            "instanceId": self.instance_id,
            "sdkVersion": self._sdk.sdk_version,
            "status": "draining" if self._closing else "ready",
            "queueDepth": self._sdk.queue_depth(),
            "activeSpanCount": self._journal.live_span_count(),
            "orphanedSpanCount": self._journal.orphaned_span_count(),
        }

    def close(self, *, force: bool = False) -> None:
        self._sdk.close(force=force)
        self._journal.mark_clean_shutdown()
        self._journal.close()

    def prepare_shutdown(self) -> None:
        """Atomically stop admission once every live span has finished."""
        with self._admission_lock:
            count = self._journal.live_span_count()
            if count:
                raise BridgeError(
                    "activeSpans",
                    f"cannot stop the bridge while {count} span(s) remain live",
                    409,
                )
            self._closing = True

    def prepare_forced_shutdown(self) -> None:
        with self._admission_lock:
            self._closing = True
            self._journal.orphan_live_spans()

    def _require_live_span(self, span_handle_id: str) -> dict[str, Any]:
        span = self._journal.span(span_handle_id)
        if not span:
            raise BridgeError("unknownSpan", "span was not found", 404)
        if span["state"] != "live" or span["instanceId"] != self.instance_id:
            raise BridgeError("orphanedSpan", "span is not live in this instance", 409)
        return span

    def _mutate(
        self,
        operation_id: str,
        route: str,
        body: dict[str, Any],
        dispatch: Callable[[], dict[str, Any]],
        protocol_version: int,
    ) -> dict[str, Any]:
        with self._admission_lock:
            if self._closing:
                raise BridgeError("shuttingDown", "bridge is shutting down", 503)
            request_hash = hashlib.sha256(
                canonical_json({"protocolVersion": protocol_version, "body": body})
            ).hexdigest()
            cached = self._journal.begin(operation_id, route, request_hash)
            if cached is not None:
                return cached
            try:
                response = dispatch()
            except BridgeError as error:
                error.operation_id = operation_id
                if error.code == "queueFull":
                    self._journal.mark_not_dispatched(operation_id, error.code)
                    raise
                self._journal.fail(
                    operation_id,
                    error.code,
                    ambiguous=error.code
                    not in {
                        "invalidRequest",
                        "queueFull",
                        "unknownSpan",
                        "unknownContext",
                    },
                )
                raise
            except BaseException as error:
                if _sdk_error_code(error) == "600703002":
                    self._journal.fail(operation_id, "sdkAuthentication", ambiguous=False)
                    raise BridgeError(
                        "sdkAuthentication",
                        "Fornax SDK authentication rejected AK/SK/region",
                        503,
                        operation_id=operation_id,
                    ) from error
                self._journal.fail(operation_id, "sdkRejected", ambiguous=True)
                raise BridgeError(
                    "sdkRejected",
                    f"Fornax SDK rejected the operation ({type(error).__name__})",
                    500,
                    "afterStatusCheck",
                    operation_id=operation_id,
                ) from error
            self._journal.succeed(operation_id, response)
            return response

    def _reconcile_late(
        self,
        operation_id: str,
        future: Any,
        finalize: Callable[[dict[str, Any]], dict[str, Any]],
    ) -> None:
        try:
            response = finalize(future.result())
            self._journal.reconcile_late_success(operation_id, response)
        except BaseException as error:  # noqa: BLE001 - worker completion boundary.
            self._journal.reconcile_late_failure(
                operation_id,
                "sdkAuthentication" if _sdk_error_code(error) == "600703002" else "sdkRejected",
            )


def _sdk_error_code(error: BaseException) -> str | None:
    for name in ("code", "error_code", "status_code"):
        value = getattr(error, name, None)
        if isinstance(value, (str, int)):
            return str(value)
    return None
