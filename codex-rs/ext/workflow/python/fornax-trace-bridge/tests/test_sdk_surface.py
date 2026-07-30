from importlib.metadata import version
from inspect import signature

from bytedance.fornax.infra import FornaxClient
from bytedance.fornax.infra.trace.span import FornaxSpan, NoopFornaxSpan


def test_pinned_sdk_exposes_documented_client_surface() -> None:
    assert version("bytedance.fornax") == "1.0.46"
    assert set(signature(FornaxClient).parameters) == {
        "ak",
        "sk",
        "conn_timeout",
        "read_timeout",
        "http_timeout",
        "fornax_custom_region",
        "skip_tcc_validation",
    }

    for method in (
        "start_span",
        "get_span_from_header",
        "close_trace",
    ):
        assert callable(getattr(FornaxClient, method))

    assert set(signature(FornaxClient.start_span).parameters) == {
        "span_name",
        "span_type",
        "child_of",
        "start_time",
    }
    assert set(signature(FornaxClient.get_span_from_header).parameters) == {"span_header"}
    assert not signature(FornaxClient.close_trace).parameters


def test_pinned_sdk_exposes_documented_span_surface() -> None:
    for member in (
        "set_tag",
        "set_baggage",
        "set_input",
        "set_output",
        "to_header",
        "finish",
        "trace_info",
    ):
        assert hasattr(FornaxSpan, member)
    assert isinstance(NoopFornaxSpan, FornaxSpan)

    assert set(signature(FornaxSpan.set_tag).parameters) == {"self", "tagKV"}
    assert set(signature(FornaxSpan.set_baggage).parameters) == {"self", "baggageKV"}
    assert set(signature(FornaxSpan.set_input).parameters) == {"self", "_input"}
    assert set(signature(FornaxSpan.set_output).parameters) == {"self", "output"}
    assert set(signature(FornaxSpan.set_finish_time).parameters) == {
        "self",
        "finish_time_stamp",
    }
