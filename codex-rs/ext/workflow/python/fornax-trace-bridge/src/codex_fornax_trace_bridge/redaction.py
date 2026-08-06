"""Small deterministic secret/payload redaction helpers."""

import hashlib
import json
from collections.abc import Mapping
from typing import Any

SENSITIVE_KEYS = {
    "ak",
    "sk",
    "authorization",
    "token",
    "input",
    "output",
    "value",
    "payload",
}


def digest_payload(value: Any) -> dict[str, object]:
    """Return classification-safe metadata instead of a payload."""
    encoded = canonical_json(value)
    return {
        "sha256": hashlib.sha256(encoded).hexdigest(),
        "size": len(encoded),
    }


def redact_mapping(value: Mapping[str, Any]) -> dict[str, Any]:
    """Recursively redact credential and content-bearing keys."""
    result: dict[str, Any] = {}
    for key, item in value.items():
        if key.lower() in SENSITIVE_KEYS:
            result[key] = "[REDACTED]"
        elif isinstance(item, Mapping):
            result[key] = redact_mapping(item)
        elif isinstance(item, list):
            result[key] = [
                redact_mapping(entry) if isinstance(entry, Mapping) else entry for entry in item
            ]
        else:
            result[key] = item
    return result


def canonical_json(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
