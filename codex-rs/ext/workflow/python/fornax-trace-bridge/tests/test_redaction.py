from codex_fornax_trace_bridge.redaction import digest_payload, redact_mapping


def test_redaction_removes_nested_credentials_and_payloads() -> None:
    value = {
        "authorization": "Bearer secret",
        "nested": {"sk": "secret", "ordinary": "visible"},
        "items": [{"token": "secret"}],
    }
    assert redact_mapping(value) == {
        "authorization": "[REDACTED]",
        "nested": {"sk": "[REDACTED]", "ordinary": "visible"},
        "items": [{"token": "[REDACTED]"}],
    }


def test_payload_digest_is_deterministic_without_retaining_content() -> None:
    first = digest_payload({"b": 2, "a": "private"})
    second = digest_payload({"a": "private", "b": 2})
    assert first == second
    assert "private" not in repr(first)
