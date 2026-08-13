import json
from pathlib import Path

from codex_fornax_trace_bridge.models import FinishSpan, RecordSpan, StartSpan

FIXTURE_ROOT = Path(__file__).parents[3] / "tests" / "fixtures" / "fornax-bridge"
FIXTURES = FIXTURE_ROOT / "v1"


def fixture(name: str) -> dict[str, object]:
    return json.loads((FIXTURES / name).read_text())


def test_rust_shared_request_fixtures_match_python_protocol() -> None:
    start = StartSpan.parse(fixture("start-request.json"), protocol_version=1)
    assert start.parent.kind == "persistedContext"
    assert RecordSpan.parse(fixture("record-request.json")).record_type == "input"
    FinishSpan.parse(fixture("finish-request.json"))


def test_shared_responses_never_expose_opaque_header_context() -> None:
    for name in (
        "start-response.json",
        "record-response.json",
        "finish-response.json",
        "health-response.json",
        "ambiguous-error.json",
    ):
        encoded = (FIXTURES / name).read_text()
        assert "x-flow-" not in encoded
        assert "authorization" not in encoded.lower()


def test_protocol_v2_shared_span_types_are_additive() -> None:
    values = json.loads((FIXTURE_ROOT / "v2" / "span-types.json").read_text())
    assert [StartSpan.parse(value, protocol_version=2).span_type for value in values] == [
        "root",
        "prompt",
        "model",
        "tool",
        "agent",
        "retriever",
    ]
