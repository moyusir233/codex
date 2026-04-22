# codex-feedback crate analysis

## Scope

`codex-feedback` is a small infrastructure crate that captures runtime logs and structured request metadata, snapshots them for a specific Codex thread, and uploads the result to Sentry as feedback with optional attachments.

The crate has two source modules:

- `src/lib.rs`: in-memory log capture, structured metadata capture, feedback snapshot construction, and Sentry upload.
- `src/feedback_diagnostics.rs`: environment-derived connectivity diagnostics that can be attached to feedback uploads.

It is intentionally standalone: it depends on `codex-login` only for auth-environment telemetry fields and on `codex-protocol` for thread/session identifiers.

## Responsibilities

### 1. Capture full-fidelity logs for later feedback upload

`CodexFeedback` owns a shared `FeedbackInner` that contains:

- a mutex-protected byte ring buffer for log text
- a mutex-protected `BTreeMap<String, String>` for structured feedback tags

The log side is exposed through:

- `CodexFeedback::make_writer()`
- `CodexFeedback::logger_layer()`

`logger_layer()` builds a `tracing_subscriber::fmt::Layer` configured to:

- write into the in-memory ring buffer
- use `SystemTime`
- disable ANSI formatting
- disable log target rendering
- capture everything at `TRACE` regardless of caller `RUST_LOG`

This makes the feedback buffer an always-on diagnostic sink rather than a user-facing logger.

### 2. Capture structured metadata as upload tags

The crate defines a dedicated tracing target:

- `FEEDBACK_TAGS_TARGET = "feedback_tags"`

Any tracing event emitted with this target is interpreted as structured key/value metadata for later upload. There are two main entry points:

- `emit_feedback_request_tags()`
- `emit_feedback_request_tags_with_auth_env()`

Both functions normalize optional fields into a `FeedbackRequestSnapshot` so the tracing event always carries stringifiable values instead of `Option<T>`.

`CodexFeedback::metadata_layer()` installs `FeedbackMetadataLayer`, which:

- filters to the `feedback_tags` target only
- visits event fields using `FeedbackTagsVisitor`
- converts values into strings
- stores them in the shared tag map
- caps unique stored keys at `MAX_FEEDBACK_TAGS = 64`

This is the bridge from request/auth telemetry emitted elsewhere in the application into feedback-specific Sentry tags.

### 3. Build thread-scoped feedback snapshots

`CodexFeedback::snapshot(session_id)` materializes a `FeedbackSnapshot` containing:

- current log bytes from the ring buffer
- accumulated structured tags
- `FeedbackDiagnostics::collect_from_env()`
- a `thread_id`

If `session_id` is absent, it synthesizes a fallback value:

- `no-active-thread-<generated-thread-id>`

That ensures every upload can still be correlated even outside an active session.

### 4. Persist or upload captured feedback

`FeedbackSnapshot` supports:

- `save_to_temp_file()`: writes captured logs to a temp file named `codex-feedback-<thread_id>.log`
- `upload_feedback()`: builds a Sentry envelope with event tags and attachments, then flushes

Uploads include:

- a Sentry event whose level depends on `classification`
- optional `reason` encoded as an exception payload
- optional log attachment `codex-logs.log`
- optional connectivity diagnostics attachment
- optional extra attachments loaded from caller-provided paths

### 5. Collect environment-based connectivity diagnostics

`feedback_diagnostics.rs` looks only for proxy-related environment variables:

- `HTTP_PROXY`, `http_proxy`
- `HTTPS_PROXY`, `https_proxy`
- `ALL_PROXY`, `all_proxy`

If any are present, it records a single diagnostic with the raw key/value pairs and can render them into an attachment named:

- `codex-connectivity-diagnostics.txt`

This is meant to help explain request failures caused by proxy configuration.

## Public API

### Core types

- `CodexFeedback`
  - `new()`
  - `make_writer()`
  - `logger_layer()`
  - `metadata_layer()`
  - `snapshot(session_id: Option<ThreadId>)`

- `FeedbackSnapshot`
  - `feedback_diagnostics()`
  - `with_feedback_diagnostics(...)`
  - `feedback_diagnostics_attachment_text(include_logs)`
  - `save_to_temp_file()`
  - `upload_feedback(options)`
  - public field: `thread_id`

- `FeedbackUploadOptions`
  - `classification`
  - `reason`
  - `tags`
  - `include_logs`
  - `extra_attachment_paths`
  - `session_source`
  - `logs_override`

- `FeedbackDiagnostics`
  - `new(...)`
  - `collect_from_env()`
  - `is_empty()`
  - `diagnostics()`
  - `attachment_text()`

- `FeedbackDiagnostic`
  - `headline`
  - `details`

- request telemetry helpers
  - `FeedbackRequestTags`
  - `emit_feedback_request_tags(...)`
  - `emit_feedback_request_tags_with_auth_env(...)`

### API shape observations

- The crate exposes high-level integration points rather than low-level primitives.
- `logger_layer()` and `metadata_layer()` are the expected wiring points for applications.
- Snapshot/upload is explicitly a two-step flow, which keeps collection decoupled from transport.
- `with_capacity()` and `as_bytes()` are crate-private, which keeps the buffer implementation hidden while still allowing unit tests to validate it.

## End-to-end flow

### Runtime wiring

At process startup, callers create `CodexFeedback` and install both layers into the tracing subscriber:

1. `logger_layer()` captures rendered log lines into the ring buffer.
2. `metadata_layer()` captures structured `feedback_tags` events into a tag map.

This happens in higher-level crates such as:

- `tui/src/lib.rs`
- `app-server/src/lib.rs`

### Telemetry emission

During network/auth operations, other crates emit request metadata through:

- `emit_feedback_request_tags_with_auth_env(...)`
- `emit_feedback_request_tags(...)`

Typical fields include:

- endpoint
- auth header presence/name
- auth mode
- retry-after-401 state
- recovery mode/phase
- connection reuse
- request id / cf-ray
- auth error / auth error code
- auth environment presence booleans

These are logged as tracing fields on the `feedback_tags` target and later captured by `FeedbackMetadataLayer`.

### Snapshot and upload

When a user submits feedback:

1. caller requests `feedback.snapshot(session_id)`
2. snapshot clones current logs and tags and collects environment diagnostics
3. caller optionally overrides diagnostics or log bytes
4. `upload_feedback()` constructs Sentry tags and attachments
5. Sentry envelope is sent and flushed with a 10-second timeout

## Internal design

### Ring buffer

The ring buffer is byte-oriented, not line-oriented:

- storage: `VecDeque<u8>`
- limit: `DEFAULT_MAX_BYTES = 4 MiB`
- overflow behavior:
  - if an incoming chunk is larger than capacity, keep only the trailing `max` bytes
  - otherwise evict from the front until the new chunk fits

Implications:

- simple and allocation-friendly
- agnostic to UTF-8 and log line boundaries
- may cut through the middle of a log line or multi-byte UTF-8 sequence

That tradeoff is acceptable because uploads treat logs as opaque diagnostic text.

### Concurrency model

Shared state sits behind `Arc<FeedbackInner>` with separate mutexes for:

- ring buffer
- structured tags

This keeps the design straightforward and avoids having log writes block tag writes and vice versa. The crate accepts poisoned mutexes as fatal in snapshot/tag-read paths via `expect("mutex poisoned")`, while writer code maps lock failure to an I/O error.

### Metadata normalization

Request-tag emission deliberately snapshots borrowed optional data into owned or defaulted forms before logging. This solves two practical problems:

- tracing fields must outlive the macro call
- uploads should receive stable string values rather than nested option syntax

### Tag merge semantics

`FeedbackSnapshot::upload_tags()` merges tag sources in this order:

1. reserved built-in tags:
   - `thread_id`
   - `classification`
   - `cli_version`
   - `session_source`
   - `reason`
2. caller-supplied `client_tags`
3. tags captured by `metadata_layer()`

Collision behavior:

- reserved keys cannot be overridden
- client tags win over internally captured metadata for non-reserved keys
- internally captured tags fill remaining gaps only

This is a deliberate priority rule: explicit upload-time tags override passive runtime collection.

### Diagnostics attachment gating

Connectivity diagnostics are only attached when `include_logs` is `true`.

This is slightly stricter than “attach diagnostics whenever available”; the crate treats diagnostics as part of the broader diagnostic/log bundle rather than as standalone metadata.

## Dependencies

### Direct dependencies from `Cargo.toml`

- `anyhow`
  - used for fallible upload setup and error shaping

- `codex-login`
  - provides `AuthEnvTelemetry`, whose fields are copied into feedback tag events

- `codex-protocol`
  - provides `ThreadId` and `SessionSource`
  - `ThreadId` is used for session correlation and fallback id generation
  - `SessionSource` is converted to an upload tag

- `sentry`
  - builds the envelope, event, exceptions, and attachments
  - sends feedback to the hard-coded DSN

- `tracing`
  - emits metadata events and warning logs

- `tracing-subscriber`
  - provides `Layer`, formatting layer construction, filtering, and writer plumbing

### Build system

- `Cargo.toml` defines the Rust crate as `codex-feedback`
- `BUILD.bazel` exposes the same crate under Bazel via `codex_rust_crate(name = "feedback", crate_name = "codex_feedback")`

## Testing

The crate uses inline unit tests only. There is no dedicated `tests/` directory.

### Tests in `src/lib.rs`

- `ring_buffer_drops_front_when_full`
  - verifies the byte ring buffer retains the newest bytes

- `metadata_layer_records_tags_from_feedback_target`
  - verifies only `feedback_tags` events populate the snapshot tag map

- `feedback_attachments_gate_connectivity_diagnostics`
  - verifies attachment ordering and that diagnostics are only included when present
  - verifies `logs_override` takes precedence over captured bytes
  - verifies extra attachment file contents and filename preservation

- `upload_tags_include_client_tags_and_preserve_reserved_fields`
  - verifies reserved upload tags cannot be overridden
  - verifies client tags beat internally captured tags for non-reserved keys

### Tests in `src/feedback_diagnostics.rs`

- `collect_from_pairs_reports_raw_values_and_attachment`
  - verifies proxy env vars are preserved verbatim and rendered into the attachment

- `collect_from_pairs_ignores_absent_values`
  - verifies empty env input yields no diagnostics

- `collect_from_pairs_preserves_whitespace_and_empty_values`
  - verifies values are not trimmed or normalized

- `collect_from_pairs_reports_values_verbatim`
  - verifies malformed proxy strings are still reported as-is

### What the tests emphasize

- deterministic string output
- conservative merge semantics
- attachment composition
- preserving raw values rather than sanitizing or interpreting them

### What is not directly tested here

- actual Sentry transport behavior
- `save_to_temp_file()` behavior
- mutex poisoning behavior
- large-volume concurrent logging
- correctness of startup subscriber wiring in integration crates

## Design strengths

- Small surface area with clear responsibility boundaries.
- Good integration with `tracing_subscriber`; callers install layers instead of reimplementing formatting/writer glue.
- Snapshot/upload separation keeps feedback capture independent from transport timing.
- Explicit reserved-tag handling prevents callers from corrupting core identifiers.
- Diagnostics module is isolated and easy to extend with additional checks.
- Byte ring buffer is simple and robust for bounded-memory capture.

## Design tradeoffs and risks

### Hard-coded Sentry DSN

The DSN is compiled into the crate. That simplifies deployment but reduces flexibility for:

- testing against alternate Sentry projects
- self-hosted deployments
- per-environment routing

### Raw proxy value capture

`FeedbackDiagnostics` intentionally stores raw proxy env values, including credentials, query parameters, and malformed strings. That is useful for debugging, but it creates a privacy/security tradeoff because sensitive proxy credentials could be uploaded.

### Logs and diagnostics coupling

Diagnostics are only attached when `include_logs` is enabled. If callers want connectivity diagnostics without full logs, the current API does not support that.

### Silent extra-attachment failures

Unreadable extra attachments are skipped with a warning log instead of surfaced to the caller. That keeps uploads resilient but makes it harder for the UI to explain incomplete uploads.

### No delivery confirmation

`upload_feedback()` sends the envelope and flushes, but it does not inspect a success/failure result from Sentry transport. The function returning `Ok(())` means setup completed, not necessarily that the feedback was accepted remotely.

### Byte-oriented truncation

The ring buffer truncates arbitrary bytes, so the saved/uploaded log may start in the middle of a line or invalid UTF-8 sequence.

## Integration notes

This crate is used as shared infrastructure across multiple top-level binaries and services. In particular:

- UI and app-server startup code create one `CodexFeedback` instance and install both tracing layers.
- request/transport telemetry code in `core` and `models-manager` emits structured feedback tags during auth/network activity.
- later UI or service code can snapshot the accumulated state and upload it as user feedback.

That architecture makes `codex-feedback` a passive collector during runtime and an active uploader only at the explicit feedback boundary.

## Open questions

1. Should proxy diagnostics redact credentials and secrets before attachment generation?
2. Should diagnostics be attachable independently of `include_logs`?
3. Should the DSN and ring-buffer capacity be configurable by caller or environment?
4. Should `upload_feedback()` surface transport success/failure more explicitly?
5. Should extra-attachment read failures be returned to callers instead of only logged?
6. Should the metadata tag cap expose observability for dropped keys when more than 64 unique fields are emitted?
7. Should the ring buffer preserve line boundaries, or is byte-level truncation intentionally the desired behavior long-term?
