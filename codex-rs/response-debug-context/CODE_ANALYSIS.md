# codex-response-debug-context

## Overview

`codex-response-debug-context` is a very small utility crate that extracts safe-to-log debugging metadata from failed API responses and normalizes error strings for telemetry.

- Manifest: [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/response-debug-context/Cargo.toml#L1-L22)
- Implementation and unit tests: [lib.rs](file:///Users/bytedance/project/codex/codex-rs/response-debug-context/src/lib.rs#L1-L165)
- Bazel target: [BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/response-debug-context/BUILD.bazel#L1-L6)

The crate exists to keep higher-level crates from duplicating the same header parsing and telemetry redaction logic. Its main value is that it preserves correlation identifiers and auth diagnostics while intentionally avoiding raw response bodies in telemetry-friendly messages.

## Structure

This crate has a deliberately flat structure:

- One public library target: [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/response-debug-context/Cargo.toml#L7-L10)
- One source module containing all production code and tests: [lib.rs](file:///Users/bytedance/project/codex/codex-rs/response-debug-context/src/lib.rs#L1-L165)
- No submodules, no feature flags, no async code, and no internal mutable state

That simplicity matches its role as a leaf utility crate.

## Concrete Responsibilities

### 1. Extract request/debug headers from HTTP transport failures

The crate defines a compact struct, [ResponseDebugContext](file:///Users/bytedance/project/codex/codex-rs/response-debug-context/src/lib.rs#L11-L17), with four fields:

- `request_id`
- `cf_ray`
- `auth_error`
- `auth_error_code`

`extract_response_debug_context(...)` reads those values from a `TransportError::Http` header map and returns `ResponseDebugContext::default()` for all non-HTTP transport failures: [lib.rs](file:///Users/bytedance/project/codex/codex-rs/response-debug-context/src/lib.rs#L19-L54)

Concrete parsing behavior:

- Prefers `x-request-id`, then falls back to `x-oai-request-id`
- Reads Cloudflare correlation from `cf-ray`
- Reads auth failure reason from `x-openai-authorization-error`
- Reads `x-error-json`, base64-decodes it, parses JSON, and extracts `error.code`

### 2. Bridge from API-layer errors to transport-layer extraction

`extract_response_debug_context_from_api_error(...)` only extracts context for `ApiError::Transport(...)` and returns an empty context for all other `ApiError` variants: [lib.rs](file:///Users/bytedance/project/codex/codex-rs/response-debug-context/src/lib.rs#L56-L61)

This is an intentional design choice because only transport HTTP errors are guaranteed to carry headers in the error model used here.

### 3. Produce telemetry-safe error summaries

`telemetry_transport_error_message(...)` and `telemetry_api_error_message(...)` map rich error enums to short strings suitable for logs/metrics: [lib.rs](file:///Users/bytedance/project/codex/codex-rs/response-debug-context/src/lib.rs#L63-L86)

Important behavior:

- `TransportError::Http { status, body, .. }` becomes `http <status>`
- `ApiError::Api { status, .. }` becomes `api error <status>`
- Non-HTTP textual details are preserved for network/build/stream cases
- Response bodies are not included in the returned message

This is the privacy/safety boundary of the crate.

## Public API Surface

The public surface is intentionally tiny and function-oriented:

- [ResponseDebugContext](file:///Users/bytedance/project/codex/codex-rs/response-debug-context/src/lib.rs#L11-L17)
- [extract_response_debug_context](file:///Users/bytedance/project/codex/codex-rs/response-debug-context/src/lib.rs#L19-L54)
- [extract_response_debug_context_from_api_error](file:///Users/bytedance/project/codex/codex-rs/response-debug-context/src/lib.rs#L56-L61)
- [telemetry_transport_error_message](file:///Users/bytedance/project/codex/codex-rs/response-debug-context/src/lib.rs#L63-L71)
- [telemetry_api_error_message](file:///Users/bytedance/project/codex/codex-rs/response-debug-context/src/lib.rs#L73-L86)

API shape observations:

- No builder or trait abstraction is needed because the crate performs pure transformation
- All functions are synchronous and allocation-light
- The struct exposes fields directly instead of forcing accessor methods
- Failure during header decoding/parsing degrades to `None` instead of returning a new error

The last point is especially important: malformed debug headers never create secondary failures in telemetry code.

## End-to-End Flow

### Extraction flow

The core extraction path in [extract_response_debug_context](file:///Users/bytedance/project/codex/codex-rs/response-debug-context/src/lib.rs#L19-L54) is:

1. Match on `TransportError::Http`; otherwise return an empty context
2. Read a header as UTF-8 text using a small local closure
3. Populate `request_id`, `cf_ray`, and `auth_error`
4. For `x-error-json`, decode base64, parse JSON, and extract `error.code`
5. Return a partially populated context if any step fails

This is a fail-soft pipeline. Each stage is optional and independently nullable.

### Telemetry flow in downstream crates

This crate is consumed by higher-level telemetry/orchestration code rather than end-user request handling directly.

#### `codex-core`

[core/src/client.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/client.rs#L123-L126) imports all four helpers.

Key usage points:

- WebSocket connect telemetry extracts API error strings and response debug metadata before recording telemetry and feedback tags: [client.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/client.rs#L773-L810)
- Unauthorized recovery logs request/debug identifiers so auth refresh attempts can be correlated to the original 401: [client.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/client.rs#L1792-L1844)
- API request telemetry records request IDs, `cf-ray`, auth error, and auth error code alongside sanitized error messages: [client.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/client.rs#L1946-L1992)

#### `codex-models-manager`

[models-manager/src/manager.rs](file:///Users/bytedance/project/codex/codex-rs/models-manager/src/manager.rs#L31-L32) uses the transport-level helpers inside its `RequestTelemetry` implementation.

On each `/models` request, it:

- Computes a sanitized error string via `telemetry_transport_error_message`
- Extracts `ResponseDebugContext`
- Emits those values into tracing events and feedback tags

See [manager.rs](file:///Users/bytedance/project/codex/codex-rs/models-manager/src/manager.rs#L57-L138).

### Why the flow matters

The crate standardizes auth failure diagnostics across the workspace:

- request correlation via request IDs and `cf-ray`
- auth diagnosis via header-derived error fields
- safer telemetry via short, body-free error summaries

Without this crate, the same parsing rules would likely be duplicated in `core`, `models-manager`, and possibly more callers.

## Dependency Analysis

Manifest dependencies are small and purpose-specific: [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/response-debug-context/Cargo.toml#L15-L22)

### Runtime dependencies

- `codex-api`
  - supplies `ApiError` and re-exported `TransportError`, which define the input error model consumed by this crate
  - related error definitions: [codex-api/src/error.rs](file:///Users/bytedance/project/codex/codex-rs/codex-api/src/error.rs#L7-L38), [codex-client/src/error.rs](file:///Users/bytedance/project/codex/codex-rs/codex-client/src/error.rs#L5-L30)
- `http`
  - used indirectly through `TransportError::Http` header handling and header value conversion
- `base64`
  - decodes the opaque `x-error-json` header payload
- `serde_json`
  - parses decoded `x-error-json` payloads and extracts `error.code`

### Dev dependency

- `pretty_assertions`
  - improves diffs for struct equality assertions in tests

### Coupling observations

- The crate is tightly coupled to the current `ApiError` and `TransportError` enum shapes
- It is also tightly coupled to specific header names and the JSON schema embedded inside `x-error-json`
- It avoids depending on tracing/otel/feedback crates directly, which keeps it reusable and prevents circular telemetry dependencies

## Testing Coverage

All tests live inline in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/response-debug-context/src/lib.rs#L88-L165). The crate currently has three unit tests, and `cargo test -p codex-response-debug-context` passes.

### Covered behaviors

- Header extraction and base64+JSON decoding of identity/auth metadata: [extract_response_debug_context_decodes_identity_headers](file:///Users/bytedance/project/codex/codex-rs/response-debug-context/src/lib.rs#L101-L131)
- HTTP telemetry message redaction, specifically omitting body content: [telemetry_error_messages_omit_http_bodies](file:///Users/bytedance/project/codex/codex-rs/response-debug-context/src/lib.rs#L133-L147)
- Preservation of textual detail for non-HTTP errors: [telemetry_error_messages_preserve_non_http_details](file:///Users/bytedance/project/codex/codex-rs/response-debug-context/src/lib.rs#L149-L164)

### Missing coverage

Notable gaps:

- no test proving `x-request-id` takes precedence over `x-oai-request-id`
- no test for malformed/non-UTF-8/missing headers returning `None` cleanly
- no test for malformed base64 in `x-error-json`
- no test for valid base64 but invalid JSON
- no test for valid JSON without `error.code`
- no test for `extract_response_debug_context_from_api_error(...)` on non-transport variants

The production code is small enough that these gaps are manageable, but they are still meaningful edge cases.

## Design Assessment

### Strengths

- Very focused scope with a clear single purpose
- No side effects beyond string/JSON transformation
- Fail-soft parsing avoids compounding failures during telemetry/reporting
- Redaction behavior is easy to reason about
- Reusable across multiple higher-level crates

### Trade-offs

- Returns `String` values instead of borrowed data, which is simpler but always allocates
- Uses untyped `serde_json::Value` parsing for `x-error-json`, which is flexible but schema-light
- Exposes raw `Option<String>` fields rather than a richer typed model for auth diagnostics
- Encodes header names as internal constants instead of centralizing them in a shared protocol/transport crate

### Rust-specific notes

- The crate uses exhaustive enum matching and a local closure to keep extraction compact and readable
- Ownership is simple: inputs are borrowed, outputs are newly owned strings
- There is no unsafe code, no lifetime complexity, and no concurrency concern

## Open Questions

1. Should `ApiError::Api { status, message }` ever carry equivalent debug context, or is transport-only extraction the intended long-term contract?
2. Should malformed `x-error-json` payloads remain silently ignored, or should there be a telemetry counter for parse failures?
3. Should header-name constants be shared with `codex-api` or `codex-client` to avoid drift if upstream header conventions change?
4. Should the crate add table-driven tests for malformed header cases to lock in the fail-soft behavior explicitly?
5. Should `ResponseDebugContext` eventually include more fields, such as URL or retry correlation data, or is the current shape intentionally minimal?

## Overall Assessment

`codex-response-debug-context` is a small but high-leverage crate. It centralizes a security-sensitive boundary: extracting just enough response metadata for debugging and auth recovery while avoiding the accidental spread of raw response bodies into telemetry.

Its implementation is intentionally conservative:

- parse only known headers
- ignore malformed metadata instead of failing
- expose a tiny API
- keep higher-level crates responsible for recording or displaying the results

For a crate of this size, the design is strong. The main improvement area is broader edge-case testing rather than major architectural change.
