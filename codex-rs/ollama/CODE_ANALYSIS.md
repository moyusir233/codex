# codex-ollama crate analysis

## Overview

`codex-ollama` is a small integration crate that prepares and talks to a local Ollama instance for Codex's OSS mode.

Its scope is intentionally narrow:

- resolve the built-in `ollama` provider from merged Codex config
- verify that an Ollama server is reachable
- check whether the running Ollama version is new enough for the Responses API
- list locally available models
- pull a missing model while streaming progress events
- render pull progress through a pluggable reporter abstraction

The crate is operational glue more than domain logic. It does not implement a general OpenAI-compatible provider client; instead, it bridges Codex's provider metadata into Ollama-native endpoints like `/api/tags`, `/api/version`, and `/api/pull`.

## Crate surface

From [lib.rs](file:///Users/bytedance/project/codex/codex-rs/ollama/src/lib.rs#L1-L97), the public surface is:

- `OllamaClient`
- `PullEvent`
- `PullProgressReporter`
- `CliProgressReporter`
- `TuiProgressReporter`
- `DEFAULT_OSS_MODEL`
- `ensure_oss_ready(config: &Config) -> io::Result<()>`

There is also a crate-visible entrypoint:

- `ensure_responses_supported(provider: &ModelProviderInfo) -> io::Result<()>`

This makes `src/lib.rs` a façade over four focused modules:

- [client.rs](file:///Users/bytedance/project/codex/codex-rs/ollama/src/client.rs)
- [parser.rs](file:///Users/bytedance/project/codex/codex-rs/ollama/src/parser.rs)
- [pull.rs](file:///Users/bytedance/project/codex/codex-rs/ollama/src/pull.rs)
- [url.rs](file:///Users/bytedance/project/codex/codex-rs/ollama/src/url.rs)

## Primary responsibilities

### 1. Bootstrap Ollama-backed OSS mode

[ensure_oss_ready](file:///Users/bytedance/project/codex/codex-rs/ollama/src/lib.rs#L18-L49) is the crate's main orchestration function.

It:

1. picks the configured model or falls back to `DEFAULT_OSS_MODEL`
2. creates an `OllamaClient` by resolving the built-in provider from `Config`
3. asks Ollama for its local model list
4. pulls the model if it is missing

Important behavior:

- connectivity failure is fatal because `try_from_oss_provider` probes the server up front
- model-list lookup failure is non-fatal; the crate only logs a warning and returns `Ok(())`
- model download only happens for the resolved model, not for every configured model

### 2. Enforce minimum version for Responses support

[ensure_responses_supported](file:///Users/bytedance/project/codex/codex-rs/ollama/src/lib.rs#L59-L76) validates Ollama's version before higher-level code relies on the Responses API.

The policy is intentionally soft:

- version `0.0.0` is treated as a development build and accepted
- versions before `0.13.4` are rejected
- missing `/api/version` or unparsable versions are tolerated and return `Ok(())`

This behavior is implemented through [min_responses_version](file:///Users/bytedance/project/codex/codex-rs/ollama/src/lib.rs#L51-L57) and the `supports_responses` helper.

### 3. Translate Codex provider config into Ollama-native requests

[OllamaClient::try_from_oss_provider](file:///Users/bytedance/project/codex/codex-rs/ollama/src/client.rs#L31-L50) looks up the built-in provider ID `ollama` from `Config.model_providers`, so user-configured base URL overrides are honored.

[OllamaClient::try_from_provider](file:///Users/bytedance/project/codex/codex-rs/ollama/src/client.rs#L58-L78) then:

- requires `base_url` to exist
- detects whether the configured base URL ends with `/v1`
- strips `/v1` to get the native Ollama host root
- builds a `reqwest::Client` with a 5-second connect timeout
- probes server reachability immediately

The URL normalization logic is isolated in [url.rs](file:///Users/bytedance/project/codex/codex-rs/ollama/src/url.rs#L1-L18).

### 4. Stream and decode model-pull progress

[pull_model_stream](file:///Users/bytedance/project/codex/codex-rs/ollama/src/client.rs#L155-L212) starts `POST /api/pull` with `"stream": true` and converts the newline-delimited JSON response into a `BoxStream<PullEvent>`.

This path has three layers:

- raw bytes are received from `reqwest::Response::bytes_stream`
- lines are accumulated with `BytesMut`
- each JSON object is converted into higher-level events by [pull_events_from_value](file:///Users/bytedance/project/codex/codex-rs/ollama/src/parser.rs#L5-L29)

`PullEvent` in [pull.rs](file:///Users/bytedance/project/codex/codex-rs/ollama/src/pull.rs#L5-L21) models four outcomes:

- `Status(String)`
- `ChunkProgress { digest, total, completed }`
- `Success`
- `Error(String)`

### 5. Separate transport from presentation

[PullProgressReporter](file:///Users/bytedance/project/codex/codex-rs/ollama/src/pull.rs#L23-L27) is the crate's main extension point.

[pull_with_reporter](file:///Users/bytedance/project/codex/codex-rs/ollama/src/client.rs#L214-L246) drives the stream and delegates event rendering to any reporter implementation. The built-in reporters are:

- [CliProgressReporter](file:///Users/bytedance/project/codex/codex-rs/ollama/src/pull.rs#L29-L136), which writes aggregate progress to stderr
- [TuiProgressReporter](file:///Users/bytedance/project/codex/codex-rs/ollama/src/pull.rs#L138-L146), which currently just wraps `CliProgressReporter`

This keeps the HTTP/pull logic decoupled from presentation concerns.

## Module-by-module analysis

### `src/lib.rs`

Role:

- crate façade
- simple orchestration entrypoints
- version-gating policy

Notable design choices:

- almost all behavior is delegated into `client`, `parser`, `pull`, and `url`
- the crate root exposes only the abstractions needed by upstream OSS bootstrap code
- `DEFAULT_OSS_MODEL` is hard-coded here as `gpt-oss:20b`

### `src/client.rs`

Role:

- core integration logic
- provider resolution
- reachability probing
- HTTP calls for tags, version, and pull
- stream decoding orchestration
- most integration-like unit tests

Important behaviors:

- [probe_server](file:///Users/bytedance/project/codex/codex-rs/ollama/src/client.rs#L80-L101) uses `/v1/models` for OpenAI-compatible base URLs and `/api/tags` otherwise
- [fetch_models](file:///Users/bytedance/project/codex/codex-rs/ollama/src/client.rs#L103-L127) always talks to `/api/tags` after normalizing to the host root
- [fetch_version](file:///Users/bytedance/project/codex/codex-rs/ollama/src/client.rs#L129-L153) trims an optional leading `v` before semver parsing
- [pull_model_stream](file:///Users/bytedance/project/codex/codex-rs/ollama/src/client.rs#L155-L212) treats stream-embedded `"error"` payloads as terminal errors
- [pull_with_reporter](file:///Users/bytedance/project/codex/codex-rs/ollama/src/client.rs#L214-L246) returns an error if the stream ends before a `Success` event arrives

Observations:

- a failed HTTP status from `fetch_models` is downgraded to `Ok(Vec::new())`, which is permissive and makes "server reachable but tags endpoint unhappy" look like "no models installed"
- malformed JSON lines in the pull stream are silently ignored rather than surfaced
- `let _pending: VecDeque<PullEvent> = VecDeque::new();` in `pull_model_stream` is currently unused and looks like leftover scaffolding

### `src/parser.rs`

Role:

- small decoder from raw Ollama JSON objects into semantic `PullEvent`s

Important behaviors:

- status events always become `PullEvent::Status`
- status `"success"` becomes both `Status("success")` and `Success`
- partial progress is supported when only `total` or only `completed` is present
- missing `digest` becomes an empty string rather than `Option<String>`

This parser is intentionally permissive and stateless.

### `src/pull.rs`

Role:

- progress event schema
- reporter trait
- default CLI/TUI reporter implementations

Important behaviors:

- `CliProgressReporter` aggregates per-digest totals inside a `HashMap<String, (u64, u64)>`
- displayed progress is global, not per-layer
- the first known total prints a one-time header with estimated GB size
- transfer speed is computed from delta bytes over wall-clock time between events
- `"pulling manifest"` is intentionally suppressed as noisy
- `Error` events are ignored in the reporter because `pull_with_reporter` already returns them to the caller

Design trade-off:

- `TuiProgressReporter` is not a real TUI adapter yet; it preserves behavior consistency by delegating to the CLI implementation

### `src/url.rs`

Role:

- normalize base URLs
- detect whether the configured provider points at an OpenAI-compatible `/v1` root

This module is tiny but important because the rest of the crate assumes it can convert a configured OpenAI-style base URL like `http://localhost:11434/v1` into native Ollama endpoints under `http://localhost:11434/api/...`.

## End-to-end flow

### Configuration origin

The built-in provider catalog lives upstream in [model-provider-info/src/lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L392-L493). For Ollama, the built-in provider:

- has ID `ollama`
- defaults to `http://localhost:11434/v1`
- uses `WireApi::Responses`
- can be affected by experimental environment variables like `CODEX_OSS_PORT` and `CODEX_OSS_BASE_URL`

`codex-ollama` itself does not define provider defaults; it consumes the merged provider map supplied by `codex-core`.

### Bootstrap path

When higher-level code wants Ollama OSS mode ready, the flow is:

1. upstream OSS selection chooses provider ID `ollama`
2. [utils/oss/src/lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/oss/src/lib.rs#L16-L38) calls `codex_ollama::ensure_responses_supported(&config.model_provider)` first
3. the same utility then calls `codex_ollama::ensure_oss_ready(config)`
4. `ensure_oss_ready` resolves the effective model
5. `OllamaClient::try_from_oss_provider` resolves provider metadata and probes the server
6. `fetch_models` queries installed models
7. if the target model is missing, `pull_with_reporter` streams and renders download progress

### Pull path

The model-pull execution path is:

1. send `POST /api/pull` with `{"model": "...", "stream": true}`
2. receive newline-delimited JSON objects
3. decode each object into `PullEvent`s
4. feed those events to the reporter
5. stop on explicit stream success or error
6. fail if the stream closes without either

## Dependency analysis

### Workspace dependencies

From [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/ollama/Cargo.toml#L14-L35):

- `codex-core`
  - used for `Config`
  - provides the merged `model_providers` map and selected model override
- `codex-model-provider-info`
  - provides `ModelProviderInfo`
  - provides provider IDs and test helpers
  - defines the built-in Ollama provider shape and default base URL

### External dependencies

- `reqwest`
  - all HTTP transport
- `futures`
  - `StreamExt` and boxed stream support
- `async-stream`
  - ergonomic async stream generation for pull events
- `bytes`
  - line buffering for chunked streaming responses
- `serde_json`
  - JSON decoding for tags/version/pull messages
- `semver`
  - version parsing and comparison
- `tokio`
  - async test runtime and crate async context
- `tracing`
  - warning logs for degraded or failed operations
- `wiremock`
  - used by tests to simulate local server behavior

One unusual detail is that `wiremock` is listed under normal `[dependencies]`, even though production code uses it only behind `#[cfg(test)]`.

## Testing assessment

The crate keeps all tests inline with the modules they validate.

### `src/lib.rs` tests

[lib.rs tests](file:///Users/bytedance/project/codex/codex-rs/ollama/src/lib.rs#L78-L97) cover version-gating policy:

- accepts `0.0.0`
- rejects `0.13.3`
- accepts `0.13.4` and newer

### `src/url.rs` tests

[url.rs tests](file:///Users/bytedance/project/codex/codex-rs/ollama/src/url.rs#L20-L39) cover `/v1` stripping and trailing-slash normalization.

### `src/parser.rs` tests

[parser.rs tests](file:///Users/bytedance/project/codex/codex-rs/ollama/src/parser.rs#L31-L75) cover:

- ordinary status decoding
- success status producing both status and success events
- progress decoding with either `total` or `completed`

### `src/client.rs` tests

[client.rs tests](file:///Users/bytedance/project/codex/codex-rs/ollama/src/client.rs#L263-L414) are the most important because they exercise the HTTP-facing integration logic with `wiremock`.

They cover:

- fetching models from `/api/tags`
- fetching and parsing `/api/version`
- probing both native and `/v1` OpenAI-compatible endpoints
- successful client construction when the probe succeeds
- user-facing connection failure when the server is absent

These tests skip themselves when `CODEX_SANDBOX_NETWORK_DISABLED_ENV_VAR` is set, even though they only use local mock servers.

### Observed test run

Running `cargo test -p codex-ollama` in the workspace passed successfully with 11 unit tests.

## Design characteristics

### Cohesive but pragmatic

The crate is cohesive around one use case: "make local Ollama usable for Codex OSS mode." It avoids generic abstractions that the rest of the workspace does not need.

### Thin public API

Most consumers only need:

- `DEFAULT_OSS_MODEL`
- `ensure_responses_supported`
- `ensure_oss_ready`
- optionally the progress types if they want custom rendering

That keeps upstream integration simple.

### Soft-failure policy for non-essential checks

The crate distinguishes between "cannot use Ollama at all" and "some helpful introspection failed":

- fatal: provider missing, base URL missing, server probe fails, pull fails, old version explicitly detected
- non-fatal: version endpoint unavailable, version unparsable, model listing request fails inside `ensure_oss_ready`

This favors startup robustness but can hide operational detail.

### Native-Ollama assumption behind an OpenAI-looking config

The provider catalog advertises an OpenAI-style base URL ending in `/v1`, but the crate still relies on native Ollama endpoints for real work.

That means the integration model is:

- use `/v1/models` only as a compatibility/readiness probe
- use `/api/*` for version, model tags, and pull

This is practical for Ollama, but it is not a pure OpenAI-compatible client abstraction.

### Presentation abstraction is intentionally lightweight

The reporter trait is a simple callback:

```rust
pub trait PullProgressReporter {
    fn on_event(&mut self, event: &PullEvent) -> io::Result<()>;
}
```

This keeps progress rendering easy to swap out without introducing channels, background tasks, or UI state machines.

## Gaps and trade-offs

- `fetch_models` conflates HTTP failure with an empty model list
- the pull parser ignores malformed or non-UTF-8 lines instead of surfacing diagnostic errors
- `probe_server` can succeed on `/v1/models` even if required native `/api/*` endpoints later fail
- `TuiProgressReporter` is currently a naming convenience rather than a true TUI integration
- progress aggregation assumes summing layer totals is the right UX, which may overcount or mislead if Ollama changes event semantics
- there are no tests for `pull_model_stream` or `pull_with_reporter`, so the most stateful path is only indirectly validated through parser logic

## Open questions

1. Should `fetch_models` return an error on non-success HTTP status instead of `Ok(Vec::new())`, so callers can distinguish "no models installed" from "tags endpoint failed"?
2. Should `probe_server` verify a native Ollama endpoint as well when the configured base URL ends in `/v1`, since later operations still require `/api/tags`, `/api/version`, and `/api/pull`?
3. Should `ensure_responses_supported` treat a missing or unparsable version as a warning-only success forever, or should the policy become stricter once Ollama version reporting is stable?
4. Should the crate expose a richer readiness result type instead of `io::Result<()>`, so callers can observe degraded-but-usable states?
5. Should `pull_model_stream` surface malformed stream lines as explicit `PullEvent::Error` or structured diagnostics instead of silently dropping them?
6. Should `wiremock` move to `dev-dependencies`, since production code does not appear to require it?
7. Is the unused `_pending` queue in `pull_model_stream` leftover code that should be removed, or is it placeholder state for future buffering/retry behavior?

## Summary

`codex-ollama` is a compact, focused adapter crate for Codex's local Ollama integration. Its architecture is straightforward:

- `lib.rs` exposes a small OSS-bootstrap API
- `client.rs` performs the real network work
- `parser.rs` decodes streaming pull payloads
- `pull.rs` defines progress events and rendering hooks
- `url.rs` reconciles OpenAI-style configuration with native Ollama endpoints

The implementation is clear and easy to follow. The main trade-offs are permissive error handling in a few places, limited validation of the streaming pull path, and a somewhat mixed abstraction boundary between OpenAI-compatible configuration and Ollama-native HTTP behavior.
