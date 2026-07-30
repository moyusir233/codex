# Fornax Python SDK contract status

On 2026-07-30 PRC, `uv 0.12.0` resolved and locked private distribution
`bytedance-fornax 1.0.46` from `https://bytedpypi.byted.org/simple/` under
CPython 3.13.5. `pyproject.toml` now pins that exact version and `uv.lock`
records its complete dependency graph.

The first import probe found an important documentation/import-path mismatch:
`bytedance.fornax` itself exports integrations, not the low-level client.
The installed package and its own examples export `FornaxClient` from
`bytedance.fornax.infra`, while `FornaxSpan` and the `NoopFornaxSpan` instance
live in `bytedance.fornax.infra.trace.span`.

The import-only surface probe records these exact signatures:

- `FornaxClient(ak, sk, *, conn_timeout=1, read_timeout=10,
  http_timeout=None, fornax_custom_region="", skip_tcc_validation=False)`;
- `start_span(span_name, span_type, child_of=None, start_time=None)`;
- `get_span_from_header(span_header)`;
- `close_trace()`.

`FornaxSpan` exposes `set_tag`, `set_baggage`, `set_input`, `set_output`,
`to_header`, `finish`, and `trace_info`; `NoopFornaxSpan` is an instance of
that type. The probe does not construct a client, start a span, or contact the
service.

Delivery acknowledgement, timeouts during export, exceptions, retries,
idempotency, and live reconciliation remain unverified. The writer therefore
remains experimental even though the required import surface is present.
