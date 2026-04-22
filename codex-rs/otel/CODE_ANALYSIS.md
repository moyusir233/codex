# codex-otel crate analysis

## Executive summary

`codex-otel` is the Codex workspace’s OpenTelemetry integration crate. It centralizes three concerns that would otherwise be duplicated across binaries and subsystems:

1. Provider/bootstrap wiring for OTEL logs, traces, and metrics.
2. Session-scoped event emission for product/business telemetry.
3. A small metrics abstraction with validation, tagging, runtime snapshots, and in-memory testing support.

At the API surface, the crate is intentionally narrow: callers mainly interact with `OtelSettings`/`OtelProvider`, `SessionTelemetry`, `MetricsClient`, and trace-context helpers re-exported from the crate root. See [lib.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/lib.rs#L14-L66).

## Top-level responsibilities

### 1) Exporter and provider wiring

- `OtelSettings` defines the runtime OTEL configuration: service metadata, exporter selection for logs/traces/metrics, and whether runtime snapshots should be enabled. See [config.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/config.rs#L35-L80).
- `OtelExporter` supports:
  - `None`
  - `Statsig` shorthand for Codex-internal OTLP/HTTP JSON metric export
  - `OtlpGrpc`
  - `OtlpHttp`
- `resolve_exporter()` expands `Statsig` into a concrete OTLP/HTTP JSON exporter, but disables it in debug builds to avoid local/test accidental egress. See [config.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/config.rs#L6-L33).
- `OtelProvider::from()` is the main assembly function. It:
  - decides whether logs, traces, and metrics are enabled
  - builds a `MetricsClient` first
  - installs that metrics client into a global `OnceLock`
  - constructs OTEL logger and tracer providers
  - installs the global tracer provider and W3C propagator
  - returns `None` when every signal is disabled
  See [provider.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/provider.rs#L46-L120).

### 2) Session/business telemetry

- `SessionTelemetry` is the highest-level product-facing API. It captures stable per-session metadata such as conversation id, auth mode, originator, model, slug, app version, terminal type, and whether prompts may be logged. See [session_telemetry.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/events/session_telemetry.rs#L66-L290).
- It emits business events through `tracing::event!` with two privacy lanes:
  - `codex_otel.log_only` for richer logs
  - `codex_otel.trace_safe` for trace-safe events
  See [shared.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/events/shared.rs#L4-L60) and [targets.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/targets.rs#L1-L10).
- It also forwards operational metrics through `MetricsClient`, with optional automatic session metadata tags. See [session_telemetry.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/events/session_telemetry.rs#L141-L257).

### 3) Metrics abstraction

- `MetricsClient` wraps OpenTelemetry metrics with:
  - name/tag validation
  - default tag merging
  - cached instruments
  - duration recording as histograms in milliseconds
  - optional runtime snapshot collection
  See [client.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/metrics/client.rs#L81-L291).
- `MetricsConfig` supports OTLP exporters and `InMemoryMetricExporter` for tests. See [config.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/metrics/config.rs#L9-L82).
- `RuntimeMetricsSummary` converts snapshot data into higher-level Codex totals such as tool call counts, API call durations, SSE/WebSocket activity, and response timing metrics. See [runtime_metrics.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/metrics/runtime_metrics.rs#L24-L216).

### 4) Trace-context propagation

- `trace_context.rs` implements W3C trace-context extraction/injection helpers:
  - current span → `W3cTraceContext`
  - trace headers → OTEL `Context`
  - set a tracing span’s parent from that context
  - lazily load `TRACEPARENT`/`TRACESTATE` from the environment once
  See [trace_context.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/trace_context.rs#L19-L104).

### 5) OTLP transport/TLS glue

- `otlp.rs` contains the low-level transport helpers shared by logs, traces, and metrics:
  - HTTP header sanitization
  - gRPC TLS setup
  - blocking/asynchronous HTTP client construction
  - timeout resolution from OTEL environment variables
  See [otlp.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/otlp.rs#L22-L227).

## Module map

- `src/lib.rs`
  - re-exports the public API
  - defines `ToolDecisionSource`, `TelemetryAuthMode`, and `start_global_timer()`
- `src/config.rs`
  - exporter enums and provider settings
- `src/provider.rs`
  - builds logger/tracer/meter providers and tracing layers
- `src/otlp.rs`
  - shared OTLP HTTP/gRPC transport and TLS helpers
- `src/targets.rs`
  - target naming and routing predicates for log-safe vs trace-safe events
- `src/trace_context.rs`
  - W3C propagation helpers
- `src/events/shared.rs`
  - event macros that stamp common metadata and timestamps
- `src/events/session_telemetry.rs`
  - session-aware business telemetry APIs and metric recording helpers
- `src/metrics/*`
  - `client.rs`: primary metrics client
  - `config.rs`: client configuration
  - `error.rs`: validation/export/shutdown errors
  - `names.rs`: canonical Codex metric names
  - `runtime_metrics.rs`: runtime snapshot aggregation
  - `tags.rs`: standard session metadata tags
  - `timer.rs`: RAII duration helper
  - `validation.rs`: metric/tag character validation

## Public API surface

### Provider/bootstrap API

- `OtelSettings`
- `OtelExporter`
- `OtelHttpProtocol`
- `OtelTlsConfig`
- `OtelProvider`

Primary call pattern:

1. Construct `OtelSettings`.
2. Call `OtelProvider::from(&settings)`.
3. If present, attach `logger_layer()` and/or `tracing_layer()` to a `tracing_subscriber`.
4. Call `shutdown()` for deterministic flushing if needed.

Reference: [provider.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/provider.rs#L67-L160).

### Session telemetry API

Important methods on `SessionTelemetry`:

- construction and enrichment
  - `new()`
  - `with_auth_env()`
  - `with_model()`
  - `with_metrics_service_name()`
  - `with_metrics()`
  - `with_metrics_without_metadata_tags()`
  - `with_metrics_config()`
  - `with_provider_metrics()`
- metric helpers
  - `counter()`
  - `histogram()`
  - `record_duration()`
  - `start_timer()`
  - `shutdown_metrics()`
  - `snapshot_metrics()`
  - `reset_runtime_metrics()`
  - `runtime_metrics_summary()`
- business/transport events
  - `conversation_starts()`
  - `record_api_request()`
  - `record_websocket_connect()`
  - `record_websocket_request()`
  - `record_websocket_event()`
  - `log_sse_event()`
  - `sse_event_completed()`
  - `user_prompt()`
  - `tool_decision()`
  - `tool_result_with_tags()`
  - `log_tool_result_with_tags()`
  - `record_auth_recovery()`

Reference: [session_telemetry.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/events/session_telemetry.rs#L100-L1098).

### Metrics API

- `MetricsConfig::otlp(...)`
- `MetricsConfig::in_memory(...)`
- `MetricsConfig::with_export_interval(...)`
- `MetricsConfig::with_runtime_reader()`
- `MetricsConfig::with_tag(...)`
- `MetricsClient::new(...)`
- `MetricsClient::counter(...)`
- `MetricsClient::histogram(...)`
- `MetricsClient::record_duration(...)`
- `MetricsClient::start_timer(...)`
- `MetricsClient::snapshot(...)`
- `MetricsClient::shutdown(...)`
- `Timer::record(...)`

References:
- [config.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/metrics/config.rs#L26-L82)
- [client.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/metrics/client.rs#L186-L291)
- [timer.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/metrics/timer.rs#L5-L40)

### Trace-context API

- `current_span_w3c_trace_context()`
- `span_w3c_trace_context()`
- `current_span_trace_id()`
- `context_from_w3c_trace_context()`
- `set_parent_from_w3c_trace_context()`
- `set_parent_from_context()`
- `traceparent_context_from_env()`

Reference: [trace_context.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/trace_context.rs#L19-L88).

## Control flow

### Provider initialization flow

1. The caller prepares `OtelSettings`.
2. `OtelProvider::from()` checks each signal’s exporter.
3. Metrics are initialized first through `MetricsConfig::otlp(...)` and `MetricsClient::new(...)`.
4. If metrics exist, the client is stored in the crate-global `GLOBAL_METRICS` `OnceLock`, enabling root-level helpers like `start_global_timer()`. See [metrics/mod.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/metrics/mod.rs#L19-L26) and [lib.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/lib.rs#L60-L66).
5. Log and trace `Resource`s are built, including service metadata and selective `host.name` for logs only. See [provider.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/provider.rs#L178-L217).
6. Exporters are built:
  - gRPC: tonic transport + metadata + optional TLS
  - HTTP: protocol selection + headers + optional reqwest client
7. The provider exposes `logger_layer()` and `tracing_layer()` to plug into `tracing_subscriber`.
8. If tracing is enabled, it sets the global tracer provider and `TraceContextPropagator`.

### Event emission flow

1. A subsystem creates or receives a `SessionTelemetry`.
2. A business event method emits one or two `tracing` events through the macros in `events/shared.rs`.
3. The macros attach:
  - event timestamp
  - conversation id
  - app version
  - auth mode
  - originator
  - terminal type
  - model and slug
  - optionally user account fields in the log-only lane
4. `OtelProvider` filters targets:
  - log pipeline accepts `codex_otel.*` except trace-safe targets
  - trace pipeline accepts actual spans plus `codex_otel.trace_safe*`
5. Sensitive fields therefore stay in logs, while traces get a reduced attribute set.

References:
- [shared.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/events/shared.rs#L4-L60)
- [targets.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/targets.rs#L1-L10)
- [provider.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/provider.rs#L122-L156)

### Metrics recording flow

1. `SessionTelemetry` or direct callers invoke `MetricsClient`.
2. Names and tags are validated. See [validation.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/metrics/validation.rs#L5-L55).
3. Default tags and per-call tags are merged, with per-call values overriding defaults. See [client.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/metrics/client.rs#L148-L168).
4. Instruments are cached by name in mutex-protected maps.
5. Duration metrics are recorded as `f64` histograms in milliseconds with consistent unit/description.
6. Periodic OTEL readers push exports; optional manual readers support on-demand snapshots.

### Runtime metrics summary flow

1. A `MetricsClient` must be created with `with_runtime_reader()`.
2. `snapshot()` collects without shutting down the provider.
3. `RuntimeMetricsSummary::from_snapshot()` scans known metric names and converts counters/histograms into higher-level totals.
4. `SessionTelemetry::runtime_metrics_summary()` suppresses empty summaries.

References:
- [client.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/metrics/client.rs#L214-L291)
- [runtime_metrics.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/metrics/runtime_metrics.rs#L58-L216)

## Key implementation details

### Privacy split between logs and traces

This is the strongest design pattern in the crate.

- `user_prompt()` logs prompt content only to the log target, while the trace-safe event records counts and prompt length instead of raw content. See [session_telemetry.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/events/session_telemetry.rs#L821-L862).
- `tool_result_with_tags()` logs arguments/output to logs, but only lengths/counts/origin flags to traces. See [session_telemetry.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/events/session_telemetry.rs#L946-L992).
- The target filter implementation in `provider.rs` is what enforces this separation. See [provider.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/provider.rs#L146-L156).

### Runtime-aware HTTP exporter construction

`otlp.rs` explicitly handles three runtime situations when building a blocking reqwest client:

- multi-thread tokio runtime: use `block_in_place`
- current-thread tokio runtime: spawn a separate OS thread
- no tokio runtime: build directly

This avoids blocking the wrong executor context while still satisfying OTEL components that require blocking clients. See [otlp.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/otlp.rs#L70-L99).

Related nuance: HTTP trace exporting uses an async reqwest client when a multi-thread tokio runtime is present, and falls back to the blocking exporter path otherwise. See [provider.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/provider.rs#L320-L383).

### RAII timer behavior

- `Timer` records on drop, which is ergonomic for scope timing.
- `record(additional_tags)` also allows explicit recording with extra tags.
- Because `Drop` always records, callers need to avoid double-recording if they call `record()` manually and still let the timer fall out of scope.

Reference: [timer.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/metrics/timer.rs#L13-L40).

### Session metadata tags

`SessionTelemetry` can decorate all metrics with stable metadata tags:

- `auth_mode`
- `session_source`
- `originator`
- optional `service_name`
- `model`
- `app.version`

This is centrally encoded in `SessionMetricTagValues`, which keeps the ordering and validation logic consistent. See [tags.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/metrics/tags.rs#L5-L46).

### Responses timing extraction

`record_websocket_event()` recognizes a special `responsesapi.websocket_timing` message and converts JSON timing fields into dedicated duration metrics. This gives the crate some product-specific intelligence beyond being a generic OTEL wrapper. See [session_telemetry.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/events/session_telemetry.rs#L576-L670) and [session_telemetry.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/events/session_telemetry.rs#L994-L1098).

## Dependencies and why they matter

### OpenTelemetry stack

- `opentelemetry`, `opentelemetry_sdk`
  - core tracing/logging/metrics data model and SDK
- `opentelemetry-otlp`
  - OTLP exporters for logs, metrics, and traces over HTTP/gRPC
- `opentelemetry-appender-tracing`
  - bridges `tracing` events into OTEL logs
- `tracing-opentelemetry`
  - bridges `tracing` spans/events into OTEL traces
- `opentelemetry-semantic-conventions`
  - service/version semantic attributes

Reference: [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/otel/Cargo.toml#L15-L66).

### Runtime and transport

- `tokio`
  - runtime detection and async batch trace processor
- `reqwest`
  - OTLP HTTP transport, both blocking and async
- `tokio-tungstenite`
  - WebSocket event typing in session telemetry
- `eventsource-stream`
  - SSE event typing in session telemetry
- `http`
  - URI parsing for TLS host derivation

### Codex workspace crates

- `codex-protocol`
  - conversation/thread ids, session source, user input, review decisions, W3C trace context model
- `codex-api`
  - response event types and API errors
- `codex-app-server-protocol`
  - auth-mode conversion without a `codex-core` dependency
- `codex-utils-string`
  - metric tag sanitization
- `codex-utils-absolute-path`
  - validated certificate/key paths for OTLP TLS config

These workspace dependencies show that the crate is not just telemetry plumbing; it encodes Codex product telemetry semantics.

## Testing strategy

The test suite is broad and maps well to the crate’s responsibilities.

### Harness

- Shared helpers build in-memory exporters, inspect `ResourceMetrics`, and convert attributes for assertions. See [harness/mod.rs](file:///Users/bytedance/project/codex/codex-rs/otel/tests/harness/mod.rs#L12-L81).

### Metrics semantics and validation

- `send.rs`
  - verifies counter/histogram export, tag merging, queue flush, and shutdown behavior
  - confirms per-call tags override defaults
  See [send.rs](file:///Users/bytedance/project/codex/codex-rs/otel/tests/suite/send.rs#L10-L217).
- `timing.rs`
  - verifies duration histograms and RAII timer behavior
  See [timing.rs](file:///Users/bytedance/project/codex/codex-rs/otel/tests/suite/timing.rs#L9-L77).
- `validation.rs`
  - verifies invalid metric names, invalid tag keys/values, and negative counter increments
  See [validation.rs](file:///Users/bytedance/project/codex/codex-rs/otel/tests/suite/validation.rs#L13-L91).

### Snapshot/runtime-reader behavior

- `snapshot.rs`
  - proves snapshots can be collected without shutting down the exporter
  - verifies metadata-tagging behavior through `SessionTelemetry`
  See [snapshot.rs](file:///Users/bytedance/project/codex/codex-rs/otel/tests/suite/snapshot.rs#L16-L125).
- `runtime_summary.rs`
  - verifies aggregation of tool/API/SSE/WebSocket/timing metrics into a structured runtime summary
  See [runtime_summary.rs](file:///Users/bytedance/project/codex/codex-rs/otel/tests/suite/runtime_summary.rs#L16-L143).

### Session metric-tag policy

- `manager_metrics.rs`
  - verifies automatic session metadata tags
  - verifies metadata-tag suppression
  - verifies optional `service_name` tagging
  See [manager_metrics.rs](file:///Users/bytedance/project/codex/codex-rs/otel/tests/suite/manager_metrics.rs#L15-L163).

### Privacy/routing policy

- `otel_export_routing_policy.rs`
  - validates that sensitive fields appear in OTEL logs but not in OTEL traces
  - covers prompt logging, tool results, auth recovery, API request observability, and WebSocket observability
  This is the most important behavioral test file because it encodes the privacy contract of the crate.
  See [otel_export_routing_policy.rs](file:///Users/bytedance/project/codex/codex-rs/otel/tests/suite/otel_export_routing_policy.rs#L94-L852).

### Real OTLP HTTP loopback

- `otlp_http_loopback.rs`
  - spins up a local TCP listener
  - captures real OTLP/HTTP JSON payloads for metrics and traces
  - tests three runtime contexts for trace export:
    - no tokio runtime
    - multi-thread tokio runtime
    - current-thread tokio runtime
  See [otlp_http_loopback.rs](file:///Users/bytedance/project/codex/codex-rs/otel/tests/suite/otlp_http_loopback.rs#L135-L560).

### Unit tests inside modules

- `config.rs` checks the Statsig-debug-build resolution behavior.
- `provider.rs` checks resource attributes and target-filter logic.
- `otlp.rs` checks runtime flavor detection and blocking-client construction.
- `trace_context.rs` checks W3C parsing and current-span trace-id extraction.
- `tags.rs` checks session tag ordering and omission rules.

## Design assessment

### Strengths

- Clear layering:
  - low-level OTLP/TLS helpers
  - generic metrics client
  - provider/bootstrap layer
  - product-specific session telemetry layer
- Strong privacy model via separate tracing targets and exporter filters.
- Good testability through in-memory exporters and runtime snapshots.
- Thoughtful runtime handling around blocking vs async OTLP HTTP export.
- Codified metric names/tags reduce drift across call sites.

### Trade-offs

- The crate mixes generic telemetry infrastructure with strongly product-specific event semantics inside `SessionTelemetry`.
  - This is pragmatic, but it makes `session_telemetry.rs` large and domain-heavy.
- `SessionTelemetry` currently owns a very broad surface area: prompts, tools, auth, API requests, SSE, WebSockets, and response timing.
  - That centralizes consistency, but it also creates a large “god object” for telemetry.
- Global metrics installation is intentionally simple, but `OnceLock` means only the first installed global metrics client wins.
  - That is fine for process-wide single-provider setups, but it limits reconfiguration.

### Non-obvious invariants

- `Statsig` must be resolved before exporter construction; matching on raw `Statsig` during provider/client build is treated as unreachable. See [provider.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/provider.rs#L225-L283) and [client.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/metrics/client.rs#L336-L405).
- Trace-safe events must use the `codex_otel.trace_safe` target prefix, or they will not land in traces. See [targets.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/targets.rs#L1-L10).
- Runtime summaries require the manual reader to be enabled up front. Otherwise snapshots are unavailable. See [error.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/metrics/error.rs#L38-L45).

## Notable observations

- `codex_home` is part of `OtelSettings` but is not consumed anywhere in the current crate implementation. It appears present for future expansion or API compatibility rather than current behavior. See [config.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/config.rs#L36-L45) and the `grep` result showing only field construction sites.
- `OtelProvider::shutdown()` and `Drop` both attempt best-effort shutdown of the same providers. That is probably tolerated by the OTEL SDK, but the type itself does not track whether shutdown already occurred. See [provider.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/provider.rs#L53-L65) and [provider.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/provider.rs#L163-L175).
- `Timer::record()` does not consume the timer, so manual recording plus drop will record twice unless the caller manages scope carefully. See [timer.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/metrics/timer.rs#L13-L40).
- `traceparent_context_from_env()` caches the first parsed result in a `OnceLock<Option<Context>>`; later environment changes in the same process are ignored. See [trace_context.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/trace_context.rs#L15-L18) and [trace_context.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/trace_context.rs#L66-L104).

## Open questions

1. Is `codex_home` intentionally reserved for future local spool/cache behavior, or is it leftover API surface that can be removed?
2. Should `Timer::record()` consume `self` or disable drop-recording after explicit record to prevent accidental duplicate metrics?
3. Should `OtelProvider` guard against double shutdown internally, especially because `shutdown()` and `Drop` do the same work?
4. Is a process-global metrics singleton sufficient long-term, or will tests/multi-session embeddings eventually need a resettable or scoped global?
5. Should `SessionTelemetry` be split into smaller domains such as auth, transport, tooling, and prompt telemetry to keep the file maintainable as event volume grows?
6. Should privacy-sensitive log/trace routing be encoded more declaratively than target prefixes, for example via typed helpers or dedicated wrapper APIs that make misrouting harder?

## Bottom line

This crate is well-structured around a clear operational goal: give the Codex workspace a single place to configure OTEL exporters, emit domain telemetry safely, and test telemetry behavior without external collectors. The most important architectural idea is the deliberate split between rich logs and trace-safe events. The biggest maintenance risk is not correctness in the current code, but continued growth of `SessionTelemetry` and the subtle global-state behaviors around metrics and trace-parent caching.
