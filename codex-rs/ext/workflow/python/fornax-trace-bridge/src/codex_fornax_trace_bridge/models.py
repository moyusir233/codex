"""Protocol-v1 request validation and response helpers."""

from __future__ import annotations

import math
import re
import uuid
from dataclasses import dataclass
from typing import Any

from .errors import invalid
from .redaction import canonical_json

MAX_NAME = 256
MAX_KEY = 128
MAX_VALUE_BYTES = 64 * 1024
W3C_PATTERN = re.compile(r"^[0-9a-f]{2}-([0-9a-f]{32})-([0-9a-f]{16})-[0-9a-f]{2}$")
SHA256_PATTERN = re.compile(r"^[0-9a-f]{64}$")
SENSITIVE_KEYS = {
    "authorization",
    "cookie",
    "password",
    "token",
    "secret",
    "ak",
    "sk",
}


def require_fields(body: dict[str, Any], allowed: set[str], required: set[str]) -> None:
    unexpected = sorted(set(body) - allowed)
    if unexpected:
        raise invalid(f"unexpected request field: `{unexpected[0]}`")
    missing = sorted(required - set(body))
    if missing:
        raise invalid(f"missing request field: `{missing[0]}`")


def require_string(body: dict[str, Any], key: str, *, maximum: int = MAX_NAME) -> str:
    value = body.get(key)
    if (
        not isinstance(value, str)
        or not value
        or len(value) > maximum
        or any(ord(character) < 32 for character in value)
    ):
        raise invalid(f"`{key}` must be a non-empty bounded string")
    return value


def require_uuid(body: dict[str, Any], key: str) -> str:
    value = require_string(body, key, maximum=36)
    try:
        parsed = uuid.UUID(value)
    except ValueError as error:
        raise invalid(f"`{key}` must be a UUID") from error
    if str(parsed) != value.lower():
        raise invalid(f"`{key}` must be a canonical UUID")
    return str(parsed)


def require_json_value(value: Any) -> Any:
    try:
        encoded = canonical_json(value)
    except (TypeError, ValueError) as error:
        raise invalid("record value must be JSON-compatible") from error
    if len(encoded) > MAX_VALUE_BYTES:
        raise invalid("record value exceeds the 65536-byte limit")
    return value


@dataclass(frozen=True, slots=True)
class SpanParent:
    kind: str
    identifier: str | None = None

    @classmethod
    def parse(cls, value: Any) -> SpanParent:
        if not isinstance(value, dict):
            raise invalid("`parent` must be a tagged object")
        kind = require_string(value, "type", maximum=32)
        if kind == "newTrace":
            require_fields(value, {"type"}, {"type"})
            return cls(kind)
        if kind == "liveSpan":
            require_fields(value, {"type", "spanHandleId"}, {"type", "spanHandleId"})
            return cls(kind, require_uuid(value, "spanHandleId"))
        if kind == "persistedContext":
            require_fields(value, {"type", "traceContextId"}, {"type", "traceContextId"})
            return cls(kind, require_uuid(value, "traceContextId"))
        raise invalid("unsupported parent type")


@dataclass(frozen=True, slots=True)
class StartSpan:
    operation_id: str
    name: str
    span_type: str
    parent: SpanParent

    @classmethod
    def parse(cls, body: dict[str, Any]) -> StartSpan:
        require_fields(
            body,
            {"operationId", "name", "spanType", "parent"},
            {"operationId", "name", "spanType", "parent"},
        )
        span_type = require_string(body, "spanType", maximum=32)
        if span_type not in {"root", "agent", "tool"}:
            raise invalid("unsupported span type")
        return cls(
            operation_id=require_uuid(body, "operationId"),
            name=require_string(body, "name"),
            span_type=span_type,
            parent=SpanParent.parse(body["parent"]),
        )


@dataclass(frozen=True, slots=True)
class RecordSpan:
    operation_id: str
    record_type: str
    values: dict[str, Any] | None
    value: Any
    finish_time: float | None

    @classmethod
    def parse(cls, body: dict[str, Any]) -> RecordSpan:
        require_fields(body, {"operationId", "record"}, {"operationId", "record"})
        operation_id = require_uuid(body, "operationId")
        record = body["record"]
        if not isinstance(record, dict):
            raise invalid("`record` must be a tagged object")
        record_type = require_string(record, "type", maximum=32)
        if record_type in {"tags", "baggage"}:
            require_fields(record, {"type", "values"}, {"type", "values"})
            values = record["values"]
            if not isinstance(values, dict) or not values or len(values) > 64:
                raise invalid("record values must be a non-empty bounded object")
            checked: dict[str, Any] = {}
            for key, value in values.items():
                if not isinstance(key, str) or not key or len(key) > MAX_KEY:
                    raise invalid("record keys must be non-empty bounded strings")
                if _normalized_key(key) in SENSITIVE_KEYS:
                    raise invalid("sensitive metadata keys are not accepted")
                checked[key] = require_json_value(value)
                if record_type == "baggage" and (
                    not isinstance(value, str) or "," in value or "=" in value
                ):
                    raise invalid("baggage values must be strings without comma or equals")
            return cls(operation_id, record_type, checked, None, None)
        if record_type in {"input", "output"}:
            require_fields(record, {"type", "value"}, {"type", "value"})
            return cls(
                operation_id,
                record_type,
                None,
                require_trace_value(record["value"]),
                None,
            )
        if record_type == "finishTime":
            require_fields(record, {"type", "unixSeconds"}, {"type", "unixSeconds"})
            value = record["unixSeconds"]
            if not isinstance(value, (int, float)) or not math.isfinite(value) or value <= 0:
                raise invalid("finish time must be a positive finite Unix timestamp")
            return cls(operation_id, record_type, None, None, float(value))
        raise invalid("unsupported record type")


@dataclass(frozen=True, slots=True)
class FinishSpan:
    operation_id: str

    @classmethod
    def parse(cls, body: dict[str, Any]) -> FinishSpan:
        require_fields(body, {"operationId"}, {"operationId"})
        return cls(operation_id=require_uuid(body, "operationId"))


def validated_trace_info(span: Any) -> tuple[str, str, str]:
    info = span.trace_info
    if callable(info):
        info = info()
    trace_id = _info_field(info, "trace_id")
    span_id = _info_field(info, "span_id")
    w3c = _info_field(info, "w3c")
    match = W3C_PATTERN.fullmatch(w3c)
    if not match or match.group(1) != trace_id or match.group(2) != span_id:
        raise invalid("SDK returned inconsistent W3C trace information")
    return trace_id, span_id, w3c


def require_trace_value(value: Any) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise invalid("input and output require a tagged TraceValue object")
    kind = require_string(value, "type", maximum=32)
    if kind == "digest":
        require_fields(
            value,
            {"type", "sha256", "byteCount", "classification"},
            {"type", "sha256", "byteCount", "classification"},
        )
        sha256 = value.get("sha256")
        byte_count = value.get("byteCount")
        classification = value.get("classification")
        if (
            not isinstance(sha256, str)
            or not SHA256_PATTERN.fullmatch(sha256)
            or not isinstance(byte_count, int)
            or isinstance(byte_count, bool)
            or byte_count < 0
            or not isinstance(classification, str)
            or not classification
        ):
            raise invalid("digest TraceValue fields are invalid")
    elif kind in {"plainText", "json"}:
        require_fields(
            value,
            {"type", "value", "classification", "policyGrant"},
            {"type", "value", "classification", "policyGrant"},
        )
        if value.get("classification") != "nonSensitive" or value.get("policyGrant") is not True:
            raise invalid("content TraceValue requires an explicit non-sensitive policy grant")
        if kind == "plainText" and not isinstance(value.get("value"), str):
            raise invalid("plainText TraceValue requires a string")
    else:
        raise invalid("unsupported TraceValue type")
    require_json_value(value)
    return dict(value)


def _info_field(info: Any, name: str) -> str:
    value = info.get(name) if isinstance(info, dict) else getattr(info, name, None)
    if not isinstance(value, str) or not value:
        raise invalid(f"SDK trace information omitted `{name}`")
    return value


def _normalized_key(value: str) -> str:
    return re.sub(r"[^a-z0-9]", "", value.lower())
