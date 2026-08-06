"""SQLite idempotency, span-correlation, and opaque-context journal."""

import json
import os
import sqlite3
import threading
import time
from importlib.resources import files
from pathlib import Path
from typing import Any

from . import PROTOCOL_VERSION
from .errors import BridgeError, conflict

SCHEMA_VERSION = 1


class Journal:
    """Serializes operation admission and durable result publication."""

    def __init__(self, path: Path, instance_id: str, sdk_version: str = "test") -> None:
        self._path = path
        self._instance_id = instance_id
        self._lock = threading.RLock()
        self._connection = sqlite3.connect(path, check_same_thread=False, isolation_level=None)
        os.chmod(path, 0o600)
        self._connection.row_factory = sqlite3.Row
        migration = (
            files("codex_fornax_trace_bridge").joinpath("migrations", "0001.sql").read_text()
        )
        self._connection.executescript(migration)
        if self._connection.execute("PRAGMA quick_check").fetchone()[0] != "ok":
            self._connection.close()
            raise BridgeError("internal", "bridge journal integrity check failed", 500)
        now = _now_ms()
        with self._connection:
            self._connection.execute(
                """
                INSERT INTO bridge_meta (
                    singleton, schema_version, protocol_version, current_instance_id,
                    clean_shutdown, sdk_version
                ) VALUES (1, ?, ?, ?, 0, ?)
                ON CONFLICT(singleton) DO UPDATE SET
                    current_instance_id = excluded.current_instance_id,
                    clean_shutdown = 0,
                    sdk_version = excluded.sdk_version
                """,
                (SCHEMA_VERSION, PROTOCOL_VERSION, instance_id, sdk_version),
            )
            self._connection.execute(
                "UPDATE operations SET state = 'ambiguous', updated_at_ms = ? WHERE state = 'running'",
                (now,),
            )
            self._connection.execute(
                """
                UPDATE spans SET state = 'orphaned'
                WHERE state = 'live' AND instance_id != ?
                """,
                (instance_id,),
            )

    def begin(self, operation_id: str, route: str, request_hash: str) -> dict[str, Any] | None:
        """Admit a new operation or return a committed cached response."""
        now = _now_ms()
        with self._lock, self._connection:
            row = self._connection.execute(
                "SELECT * FROM operations WHERE operation_id = ?", (operation_id,)
            ).fetchone()
            if row:
                if row["request_hash"] != request_hash or row["route"] != route:
                    raise conflict("operation ID was reused with a different request")
                if row["state"] == "succeeded":
                    return json.loads(row["response_json"])
                if row["state"] == "notDispatched":
                    self._connection.execute(
                        """
                        UPDATE operations
                        SET state = 'running', error_code = NULL, updated_at_ms = ?
                        WHERE operation_id = ? AND state = 'notDispatched'
                        """,
                        (now, operation_id),
                    )
                    return None
                raise BridgeError(
                    "ambiguousMutation" if row["state"] == "ambiguous" else "operationPending",
                    f"operation is {row['state']}; automatic replay is not allowed",
                    409,
                    retry="afterStatusCheck" if row["state"] == "ambiguous" else "never",
                    operation_id=operation_id,
                )
            self._connection.execute(
                """
                INSERT INTO operations (
                    operation_id, route, request_hash, state, instance_id,
                    created_at_ms, updated_at_ms
                ) VALUES (?, ?, ?, 'running', ?, ?, ?)
                """,
                (operation_id, route, request_hash, self._instance_id, now, now),
            )
        return None

    def succeed(self, operation_id: str, response: dict[str, Any]) -> None:
        encoded = json.dumps(response, sort_keys=True, separators=(",", ":"))
        with self._lock, self._connection:
            updated = self._connection.execute(
                """
                UPDATE operations
                SET state = 'succeeded', response_json = ?, updated_at_ms = ?
                WHERE operation_id = ? AND state = 'running'
                """,
                (encoded, _now_ms(), operation_id),
            ).rowcount
            if updated != 1:
                raise BridgeError("internal", "operation journal lost ownership", 409)

    def reconcile_late_success(self, operation_id: str, response: dict[str, Any]) -> None:
        """Commit a worker result that arrived after the HTTP response deadline."""
        encoded = json.dumps(response, sort_keys=True, separators=(",", ":"))
        with self._lock, self._connection:
            updated = self._connection.execute(
                """
                UPDATE operations
                SET state = 'succeeded', response_json = ?, error_code = NULL, updated_at_ms = ?
                WHERE operation_id = ? AND state IN ('running', 'ambiguous')
                """,
                (encoded, _now_ms(), operation_id),
            ).rowcount
            if updated != 1:
                raise BridgeError("internal", "late operation journal lost ownership", 409)

    def reconcile_late_failure(self, operation_id: str, code: str) -> None:
        with self._lock, self._connection:
            self._connection.execute(
                """
                UPDATE operations SET state = 'failed', error_code = ?, updated_at_ms = ?
                WHERE operation_id = ? AND state IN ('running', 'ambiguous')
                """,
                (code, _now_ms(), operation_id),
            )

    def fail(self, operation_id: str, code: str, *, ambiguous: bool) -> None:
        state = "ambiguous" if ambiguous else "failed"
        with self._lock, self._connection:
            self._connection.execute(
                """
                UPDATE operations SET state = ?, error_code = ?, updated_at_ms = ?
                WHERE operation_id = ? AND state = 'running'
                """,
                (state, code, _now_ms(), operation_id),
            )

    def mark_not_dispatched(self, operation_id: str, code: str) -> None:
        with self._lock, self._connection:
            self._connection.execute(
                """
                UPDATE operations
                SET state = 'notDispatched', error_code = ?, updated_at_ms = ?
                WHERE operation_id = ? AND state = 'running'
                """,
                (code, _now_ms(), operation_id),
            )

    def operation(self, operation_id: str) -> dict[str, Any] | None:
        with self._lock:
            row = self._connection.execute(
                """
                SELECT operation_id, route, request_hash, state, response_json, error_code,
                       instance_id, created_at_ms, updated_at_ms
                FROM operations WHERE operation_id = ?
                """,
                (operation_id,),
            ).fetchone()
        if not row:
            return None
        result = dict(row)
        if result["response_json"]:
            result["response"] = json.loads(result.pop("response_json"))
        else:
            result.pop("response_json")
        return _camel_operation(result)

    def add_span(
        self,
        response: dict[str, Any],
        operation_id: str,
        header: dict[str, str],
    ) -> None:
        now = _now_ms()
        encoded_header = json.dumps(header, sort_keys=True, separators=(",", ":"))
        with self._lock, self._connection:
            self._connection.execute(
                """
                INSERT INTO trace_contexts (
                    trace_context_id, serialized_header_json, updated_at_ms
                ) VALUES (?, ?, ?)
                """,
                (response["traceContextId"], encoded_header, now),
            )
            self._connection.execute(
                """
                INSERT INTO spans (
                    span_handle_id, trace_context_id, trace_id, span_id, w3c,
                    creating_operation_id, instance_id, state, created_at_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, 'live', ?)
                """,
                (
                    response["spanHandleId"],
                    response["traceContextId"],
                    response["traceId"],
                    response["spanId"],
                    response["w3c"],
                    operation_id,
                    self._instance_id,
                    now,
                ),
            )

    def refresh_context(self, trace_context_id: str, header: dict[str, str]) -> None:
        encoded_header = json.dumps(header, sort_keys=True, separators=(",", ":"))
        with self._lock, self._connection:
            updated = self._connection.execute(
                """
                UPDATE trace_contexts
                SET serialized_header_json = ?, updated_at_ms = ?
                WHERE trace_context_id = ?
                """,
                (encoded_header, _now_ms(), trace_context_id),
            ).rowcount
            if updated != 1:
                raise BridgeError("unknownContext", "trace context was not found", 404)

    def context_header(self, trace_context_id: str) -> dict[str, str] | None:
        with self._lock:
            row = self._connection.execute(
                "SELECT serialized_header_json FROM trace_contexts WHERE trace_context_id = ?",
                (trace_context_id,),
            ).fetchone()
        return json.loads(row[0]) if row else None

    def finish_span(self, span_handle_id: str) -> None:
        with self._lock, self._connection:
            updated = self._connection.execute(
                """
                UPDATE spans SET state = 'finished', finished_at_ms = ?
                WHERE span_handle_id = ? AND instance_id = ? AND state = 'live'
                """,
                (_now_ms(), span_handle_id, self._instance_id),
            ).rowcount
            if updated != 1:
                raise BridgeError("orphanedSpan", "span is not live in this instance", 409)

    def span(self, span_handle_id: str) -> dict[str, Any] | None:
        with self._lock:
            row = self._connection.execute(
                """
                SELECT span_handle_id, trace_context_id, trace_id, span_id, w3c,
                       instance_id, state, created_at_ms, finished_at_ms
                FROM spans WHERE span_handle_id = ?
                """,
                (span_handle_id,),
            ).fetchone()
        return _camel_span(dict(row)) if row else None

    def live_span_count(self) -> int:
        return self._span_count("live", current_instance=True)

    def orphaned_span_count(self) -> int:
        return self._span_count("orphaned", current_instance=False)

    def orphan_live_spans(self) -> None:
        with self._lock, self._connection:
            self._connection.execute(
                """
                UPDATE spans SET state = 'orphaned'
                WHERE instance_id = ? AND state = 'live'
                """,
                (self._instance_id,),
            )

    def mark_clean_shutdown(self) -> None:
        with self._lock, self._connection:
            self._connection.execute(
                "UPDATE bridge_meta SET clean_shutdown = 1 WHERE singleton = 1"
            )

    def close(self) -> None:
        self._connection.close()

    def _span_count(self, state: str, *, current_instance: bool) -> int:
        query = "SELECT COUNT(*) AS count FROM spans WHERE state = ?"
        values: tuple[Any, ...] = (state,)
        if current_instance:
            query += " AND instance_id = ?"
            values += (self._instance_id,)
        with self._lock:
            row = self._connection.execute(query, values).fetchone()
        return int(row["count"])


def _camel_operation(value: dict[str, Any]) -> dict[str, Any]:
    return {
        "operationId": value["operation_id"],
        "route": value["route"],
        "requestHash": value["request_hash"],
        "state": value["state"],
        "errorCode": value["error_code"],
        "instanceId": value["instance_id"],
        "createdAtMs": value["created_at_ms"],
        "updatedAtMs": value["updated_at_ms"],
        **({"response": value["response"]} if "response" in value else {}),
    }


def _camel_span(value: dict[str, Any]) -> dict[str, Any]:
    return {
        "spanHandleId": value["span_handle_id"],
        "traceContextId": value["trace_context_id"],
        "traceId": value["trace_id"],
        "spanId": value["span_id"],
        "w3c": value["w3c"],
        "instanceId": value["instance_id"],
        "state": value["state"],
        "createdAtMs": value["created_at_ms"],
        "finishedAtMs": value["finished_at_ms"],
    }


def _now_ms() -> int:
    return time.time_ns() // 1_000_000
