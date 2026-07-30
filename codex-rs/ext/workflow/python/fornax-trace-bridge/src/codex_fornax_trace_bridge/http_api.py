"""Authenticated loopback-only HTTP protocol v1."""

from __future__ import annotations

import hmac
import ipaddress
import json
import threading
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any
from urllib.parse import unquote, urlparse

from . import PROTOCOL_VERSION, __version__
from .errors import BridgeError, invalid
from .service import BridgeService

MAX_BODY = 256 * 1024


class BridgeHttpServer(ThreadingHTTPServer):
    """HTTP server carrying bridge-owned dependencies."""

    daemon_threads = True

    def __init__(
        self,
        address: tuple[str, int],
        service: BridgeService,
        token: str,
    ) -> None:
        super().__init__(address, BridgeRequestHandler)
        bound = ipaddress.ip_address(self.server_address[0])
        if not bound.is_loopback:
            self.server_close()
            raise ValueError("bridge HTTP server must bind to loopback")
        self.service = service
        self.token = token


class BridgeRequestHandler(BaseHTTPRequestHandler):
    """Protocol router with no payload logging."""

    server: BridgeHttpServer

    def do_GET(self) -> None:
        try:
            self._authorize()
            path = urlparse(self.path).path
            if path == "/v1/health":
                health = self.server.service.health()
                self._write_json(
                    HTTPStatus.OK,
                    {
                        "bridgeVersion": __version__,
                        "protocolVersion": int(PROTOCOL_VERSION),
                        **health,
                    },
                )
                return
            prefix = "/v1/operations/"
            if path.startswith(prefix):
                operation_id = unquote(path[len(prefix) :])
                operation = self.server.service.operation(operation_id)
                if operation is None:
                    raise BridgeError("operation_not_found", "operation was not found", 404)
                self._write_json(HTTPStatus.OK, operation)
                return
            prefix = "/v1/spans/"
            if path.startswith(prefix) and "/" not in path[len(prefix) :]:
                span_handle_id = unquote(path[len(prefix) :])
                span = self.server.service.span(span_handle_id)
                if span is None:
                    raise BridgeError("unknownSpan", "span was not found", 404)
                self._write_json(HTTPStatus.OK, span)
                return
            raise BridgeError("not_found", "endpoint was not found", 404)
        except BridgeError as error:
            self._write_json(error.status, error.envelope())

    def do_POST(self) -> None:
        try:
            self._authorize()
            path = urlparse(self.path).path
            body = self._read_json()
            if path == "/v1/spans":
                self._authorize_mutation(body)
                self._write_json(HTTPStatus.OK, self.server.service.start_span(body))
                return
            prefix = "/v1/spans/"
            if path.startswith(prefix):
                suffix = path[len(prefix) :].split("/")
                if len(suffix) == 2 and suffix[0]:
                    self._authorize_mutation(body)
                    span_handle_id = unquote(suffix[0])
                    if suffix[1] == "records":
                        result = self.server.service.record_span(span_handle_id, body)
                        self._write_json(HTTPStatus.OK, result)
                        return
                    if suffix[1] == "finish":
                        result = self.server.service.finish_span(span_handle_id, body)
                        self._write_json(HTTPStatus.OK, result)
                        return
            if path == "/v1/shutdown":
                mode = body.get("mode")
                if mode == "rejectIfActive" and set(body) == {"mode"}:
                    self.server.service.prepare_shutdown()
                elif mode == "drain" and set(body) == {"mode", "deadlineMs"}:
                    deadline = body.get("deadlineMs")
                    if not isinstance(deadline, int) or deadline <= 0 or deadline > 300_000:
                        raise invalid("shutdown deadline must be a bounded positive integer")
                    self.server.service.prepare_shutdown()
                else:
                    raise invalid("unsupported shutdown request")
                self._write_json(HTTPStatus.OK, {"status": "stopping"})
                threading.Thread(target=self.server.shutdown, daemon=True).start()
                return
            raise BridgeError("not_found", "endpoint was not found", 404)
        except BridgeError as error:
            self._write_json(error.status, error.envelope())

    def log_message(self, _format: str, *_args: object) -> None:
        """Suppress stdlib request logs because they may contain identifiers."""

    def _authorize(self) -> None:
        address = ipaddress.ip_address(self.client_address[0])
        if not address.is_loopback:
            raise BridgeError("forbidden", "bridge accepts loopback clients only", 403)
        protocol = self.headers.get("X-Codex-Fornax-Protocol")
        if protocol != PROTOCOL_VERSION:
            raise BridgeError("protocol_mismatch", "unsupported bridge protocol", 426)
        authorization = self.headers.get("Authorization", "")
        expected = f"Bearer {self.server.token}"
        if not hmac.compare_digest(authorization, expected):
            raise BridgeError("localAuthentication", "invalid bridge credential", 401)

    def _authorize_mutation(self, body: dict[str, Any]) -> None:
        operation_id = body.get("operationId")
        idempotency_key = self.headers.get("Idempotency-Key")
        if not isinstance(operation_id, str) or idempotency_key != operation_id:
            raise invalid("Idempotency-Key must equal operationId")

    def _read_json(self) -> dict[str, Any]:
        try:
            length = int(self.headers.get("Content-Length", "0"))
        except ValueError as error:
            raise invalid("invalid content length") from error
        if length > MAX_BODY:
            raise BridgeError("invalidRequest", "request body exceeds the size limit", 413)
        if length <= 0:
            raise invalid("request body must be a bounded JSON object")
        raw = self.rfile.read(length)
        try:
            value = json.loads(raw)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise invalid("request body must be valid JSON") from error
        if not isinstance(value, dict):
            raise invalid("request body must be a JSON object")
        return value

    def _write_json(self, status: int, body: dict[str, Any]) -> None:
        encoded = json.dumps(body, sort_keys=True, separators=(",", ":")).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(encoded)
