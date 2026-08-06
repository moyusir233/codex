import json
from pathlib import Path

from codex_fornax_trace_bridge.models import FinishSpan, RecordSpan, StartSpan

FIXTURES = Path(__file__).parents[3] / "tests" / "fixtures" / "fornax-bridge" / "v1"


def fixture(name: str) -> dict[str, object]:
    return json.loads((FIXTURES / name).read_text())


def test_rust_shared_request_fixtures_match_python_protocol() -> None:
    start = StartSpan.parse(fixture("start-request.json"))
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
