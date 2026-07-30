# Fornax Python SDK contract status

On 2026-07-30 PRC, `uv 0.12.0` resolved and locked private distribution
`bytedance.fornax 1.0.46` from `https://bytedpypi.byted.org/simple/` under
CPython 3.10.15. `pyproject.toml` now pins that exact version and `uv.lock`
records its complete dependency graph.

The documentation inputs were:

- “[SDK]使用 Fornax SDK 上报 Trace（新版）” at
  `https://bytedance.larkoffice.com/wiki/Q2ExwcSDqiAt2EkVYLLczlBPnvg`,
  resolved Docx `LW4CdYIYKoxMVnxT4aRcsiqKnJc`;
- “使用 Fornax Python SDK 上报 Trace” at
  `https://bytedance.larkoffice.com/wiki/Cp4LwbLrYiMWS0k1222ck1hPntI`,
  resolved Docx `Wsobd2fAToxCJNxUeMYcW3sHnVc`.

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
`set_finish_time`, `to_header`, `finish`, and `trace_info`;
`NoopFornaxSpan` is an instance of that type. The pinned signatures show that
`set_tag` and `set_baggage` each receive one mapping. `to_header()` returns an
opaque string map and `trace_info` supplies `trace_id`, `span_id`, and W3C.

The optional `codex-fornax-trace-bridge 0.1.0` wheel now implements only that
verified generic custom-span subset. One explicit client is constructed on one
worker thread. Roots use a generated log ID and `NoopFornaxSpan`; live children
use the Python handle; recovery children use `get_span_from_header`. The bridge
refreshes and privately journals the header after start, baggage, and before
finish, then calls `close_trace()` once after admission stops. Shared protocol
fixtures bind bridge protocol 1 to SDK 1.0.46.

Delivery acknowledgement, timeouts during export, exceptions, retries,
idempotency, and live reconciliation remain unverified. The writer therefore
remains experimental even though deterministic fake-backed tests pass. The
live test is skipped unless `FORNAX_LIVE_TEST=1`; no disposable SDK workspace
or approved AK/SK/region was supplied on 2026-07-30, so no trace was written and
finish/flush was not remotely reconciled. Rust preflight keeps trace writes
disabled until that delivery approval is explicitly recorded.
