# codex-api Code Analysis

## Scope

This document analyzes the crate at `codex-api/`, focusing on:

- `Cargo.toml`
- `src/lib.rs`
- the main source modules under `src/`
- representative unit tests embedded in source files
- integration tests under `tests/`

`codex-api` is the wire-level API layer between higher-level Codex business logic and lower-level HTTP/WebSocket transports. It owns request shaping, provider configuration, authentication header injection, retry/telemetry handoff, stream parsing, and compatibility behavior for several backend variants.

## High-Level Responsibilities

The crate owns seven concrete responsibilities:

1. Define the canonical request and response types used to call Codex/OpenAI-style APIs.
2. Build typed clients for the major HTTP, SSE, WebSocket, and realtime endpoints.
3. Encapsulate provider details such as base URLs, query params, default headers, retry policy, and stream idle timeout.
4. Attach auth headers through a minimal synchronous `AuthProvider` abstraction.
5. Convert raw SSE and WebSocket frames into typed `ResponseEvent` or realtime protocol events.
6. Bridge transport-level failures into the workspace-wide `codex_protocol::error::CodexErr` model.
7. Preserve compatibility with backend-specific quirks such as Azure Responses behavior, legacy memory paths, and multiple realtime wire formats.

In practice, this crate is not where product logic lives. It is where Codex standardizes transport behavior so callers can work with typed Rust values instead of raw HTTP details.

## Crate Layout

The crate is organized around protocol concerns rather than feature domains:

- `Cargo.toml`
  - declares `codex-api`
  - depends primarily on `codex-client` for transport and `codex-protocol` for shared wire/domain types
- `src/lib.rs`
  - exposes the small public surface used by other crates
- `src/common.rs`
  - defines shared request/response contracts and the unified `ResponseStream`
- `src/provider.rs`
  - defines provider configuration and retry translation
- `src/auth.rs`
  - defines the auth header abstraction
- `src/endpoint/`
  - contains typed endpoint clients for Responses, Models, Compact, Memories, realtime calls, and realtime websockets
- `src/sse/`
  - parses Responses SSE streams into `ResponseEvent`
- `src/requests/`
  - contains small request/header helpers
- `src/files.rs`
  - handles OpenAI-style local file upload using a presigned upload flow
- `src/rate_limits.rs`
  - parses header-based and event-based rate-limit information
- `src/api_bridge.rs`
  - maps crate-local `ApiError` into workspace `CodexErr`
- `tests/`
  - covers end-to-end streaming and realtime websocket behavior

The internal split is deliberate: request-building, transport execution, and event parsing stay in separate modules so the same semantic event model can be reused across SSE and WebSocket transports.

## Cargo and Dependencies

`codex-api` depends on a small set of foundational crates with clear roles:

- `codex-client`
  - provides `HttpTransport`, `ReqwestTransport`, request/response structs, retry helpers, TLS helper functions, and transport error types
- `codex-protocol`
  - provides shared model types such as `ResponseItem`, `RealtimeEvent`, `ModelInfo`, `RateLimitSnapshot`, and workspace error types
- `reqwest`
  - used directly for file upload flows that need presigned PUT uploads and direct polling
- `tokio`, `futures`, `async-channel`, `tokio-util`
  - power async request execution, stream parsing, reader conversion, and channel-based event delivery
- `tokio-tungstenite`, `tungstenite`
  - implement Responses and realtime websocket connections
- `eventsource-stream`
  - parses HTTP streaming bytes into SSE events
- `serde`, `serde_json`
  - encode requests and decode responses/events
- `http`, `url`
  - manage headers, methods, status codes, and URL shaping
- `regex-lite`, `chrono`, `base64`
  - support retry-after parsing, timestamp handling, and error-header decoding

The important architectural point is that this crate does not own the transport abstraction or the canonical protocol schema. It sits between them.

## Public API Surface

The public surface in `src/lib.rs` is intentionally flattened and re-export heavy so callers do not need to know the internal module structure.

### Core shared types

- `ResponsesApiRequest`
  - canonical HTTP request body for the Responses API
- `ResponsesWsRequest`
  - websocket request envelope for Responses
- `ResponseCreateWsRequest`
  - websocket `response.create` payload
- `ResponseEvent`
  - normalized stream event enum shared by SSE and Responses websocket transports
- `ResponseStream`
  - channel-backed async stream of `Result<ResponseEvent, ApiError>`
- `CompactionInput`
  - typed body for `responses/compact`
- `MemorySummarizeInput` and `MemorySummarizeOutput`
  - typed request/response for trace summarization
- `Reasoning`, `TextControls`, `OpenAiVerbosity`
  - request-shaping helpers for reasoning and text output controls

### Provider and auth

- `Provider`
  - carries base URL, query params, headers, retry settings, and stream idle timeout
- `RetryConfig`
  - high-level retry settings that convert into `codex-client::RetryPolicy`
- `AuthProvider`
  - small trait that mutates request headers in place
- `SharedAuthProvider`
  - `Arc<dyn AuthProvider>` used by most endpoint clients

### Endpoint clients

- `ResponsesClient`
  - HTTP + SSE client for `POST /responses`
- `ResponsesWebsocketClient`
  - websocket transport for response streaming
- `CompactClient`
  - typed client for `POST /responses/compact`
- `MemoriesClient`
  - typed client for `POST /memories/trace_summarize`
- `ModelsClient`
  - typed client for `GET /models`
- `RealtimeCallClient`
  - creates WebRTC realtime calls and returns SDP + `call_id`
- `RealtimeWebsocketClient`
  - manages realtime sideband websocket connections

### Supporting utilities

- `map_api_error`
  - converts `ApiError` into `CodexErr`
- `upload_local_file`
  - uploads a local file through the OpenAI-compatible file workflow
- `build_conversation_headers`
  - creates conversation/session headers
- `response_create_client_metadata`
  - injects trace context into websocket client metadata

## Key Module Responsibilities

### `src/common.rs`

This file defines the crate-wide contracts.

- `ResponsesApiRequest` is the central Responses payload and includes model, instructions, input history, tools, tool-choice behavior, reasoning, text controls, and client metadata.
- `ResponseCreateWsRequest` mirrors the same semantic payload for websocket `response.create`.
- `ResponseEvent` is the normalization layer that callers consume regardless of whether bytes came from SSE or websocket transport.
- `ResponseStream` wraps a Tokio channel receiver and implements `futures::Stream`, which keeps stream producers transport-specific while the consumer API stays uniform.

This is the most important file for understanding the crate’s external contract.

### `src/provider.rs`

This module owns endpoint configuration rather than request execution.

- `Provider::url_for_path()` appends endpoint paths and optional query params.
- `Provider::build_request()` creates a baseline `codex_client::Request`.
- `Provider::websocket_url_for_path()` maps `http/https` URLs to `ws/wss`.
- `is_azure_responses_provider()` and `is_azure_responses_endpoint()` detect Azure deployments so Responses requests can apply compatibility behavior.

The provider object is passed by value into clients, so each client instance is effectively bound to one backend deployment shape.

### `src/auth.rs`

Auth is intentionally tiny:

- `AuthProvider` only exposes `add_auth_headers(&mut HeaderMap)`.
- The trait is synchronous by design; the crate assumes token refresh and I/O happen in higher layers.
- `auth_header_telemetry()` lets callers record whether authorization-like headers were attached without exposing secrets.

This keeps `codex-api` focused on transport execution rather than auth lifecycle management.

### `src/endpoint/session.rs`

`EndpointSession<T: HttpTransport>` is the shared execution core for HTTP-based endpoint clients.

- It merges provider defaults, extra headers, auth headers, and optional JSON bodies.
- It routes requests through `run_with_request_telemetry()`.
- It uses the provider’s retry policy for both unary and streaming calls.

This module is the main reason the endpoint clients stay small: most of them only define a path, payload shape, and response decoder.

### `src/endpoint/responses.rs`

This is the main HTTP Responses client.

- `stream_request()` takes a typed `ResponsesApiRequest` plus `ResponsesOptions`.
- It serializes the request body, adds conversation and subagent headers, and applies Azure-specific item-id preservation when `store = true`.
- It delegates transport work to `EndpointSession::stream_with()`.
- It wraps the returned byte stream with `spawn_response_stream()` so callers receive a typed `ResponseStream`.

`ResponsesOptions` separates transport concerns from the main JSON body:

- `conversation_id`
- `session_source`
- `extra_headers`
- `compression`
- `turn_state`

That split is a good design choice because the request body remains an API contract while options remain transport/session metadata.

### `src/sse/responses.rs`

This module converts raw SSE bytes into typed events.

- `spawn_response_stream()` emits synthetic header-derived events before parsing bytes:
  - `ServerModel`
  - `RateLimits`
  - `ModelsEtag`
  - `ServerReasoningIncluded`
- `process_sse()` runs the event loop with idle timeout handling and optional telemetry.
- `ResponsesStreamEvent` captures the raw event envelope from the wire.
- `process_responses_event()` maps event kinds such as:
  - `response.output_item.done`
  - `response.output_item.added`
  - `response.output_text.delta`
  - `response.completed`
  - `response.failed`
  - reasoning and tool-call delta events

This module also normalizes several server failure modes into semantic `ApiError` variants such as:

- `ContextWindowExceeded`
- `QuotaExceeded`
- `UsageNotIncluded`
- `InvalidRequest`
- `ServerOverloaded`
- `Retryable`

One subtle design detail matters here: parser errors for intermediate events do not necessarily terminate the stream, but terminal server-side failures do.

### `src/endpoint/responses_websocket.rs`

This is the websocket counterpart to the SSE Responses client.

- `ResponsesWebsocketClient::connect()` creates a websocket connection using provider headers plus auth headers.
- `ResponsesWebsocketConnection::stream_request()` sends a websocket request and returns the same `ResponseStream` type used by SSE.
- `run_websocket_response_stream()` receives websocket messages, maps wrapped websocket error payloads, parses rate-limit events, and reuses `process_responses_event()` for semantic event mapping.

The important design decision is reuse: SSE and websocket transports have different framing, but they converge onto the same `ResponsesStreamEvent -> ResponseEvent` conversion path.

### `src/endpoint/models.rs`

`ModelsClient` is a straightforward unary HTTP client with two specific behaviors:

- appends `client_version` to the request URL
- returns both parsed models and the response `ETag`

The returned tuple `(Vec<ModelInfo>, Option<String>)` is intentionally richer than a plain model list because the caller can cache model metadata using the ETag.

### `src/endpoint/compact.rs`

`CompactClient` sends a typed compaction request to `responses/compact` and decodes a simple `{ output: Vec<ResponseItem> }` response.

This endpoint is thin because most of the interesting structure already lives in `CompactionInput`.

### `src/endpoint/memories.rs`

`MemoriesClient` is similar to `CompactClient`, but it preserves an older wire path and schema:

- path: `memories/trace_summarize`
- request field alias: `raw_memories` serializes as `traces`

This module is clearly carrying a compatibility contract, not just a clean-sheet API.

### `src/endpoint/realtime_call.rs`

`RealtimeCallClient` creates a WebRTC realtime call and returns:

- `sdp`
- `call_id`

It supports two request shapes:

- a backend-specific JSON body for `/backend-api`
- a multipart form body for the newer API route

It also supports embedding an initial realtime session payload so inference can start immediately after peer connection setup.

### `src/endpoint/realtime_websocket/`

This directory implements realtime websocket support.

- `protocol.rs`
  - defines parser version selection and outbound message schema
- `methods.rs`
  - owns connection setup, message sending, event receiving, sideband retry logic, and transcript state
- `protocol_v1.rs` and `protocol_v2.rs`
  - parse distinct wire formats into shared `RealtimeEvent` values
- `methods_common.rs`
  - builds shared session update / message payload helpers

Key capabilities:

- `RealtimeWebsocketClient::connect()`
  - starts a standalone realtime websocket session
- `RealtimeWebsocketClient::connect_webrtc_sideband()`
  - joins the control socket for an already-created WebRTC call and retries join attempts
- `RealtimeWebsocketConnection`
  - exposes `send_audio_frame`, `send_conversation_item_create`, `send_conversation_function_call_output`, `next_event`, and `close`

This part of the crate is the most stateful and protocol-heavy area.

### `src/files.rs`

`upload_local_file()` bypasses `EndpointSession` and uses direct `reqwest` because the flow is different from ordinary API calls:

1. create a file resource via authenticated POST
2. upload bytes to a presigned URL via PUT
3. poll finalize status until the file becomes available

The function returns a rich `UploadedOpenAiFile` struct containing file id, canonical `sediment://` URI, download URL, metadata, and original path.

### `src/rate_limits.rs`

This module parses two kinds of rate-limit data:

- header families such as `x-codex-primary-used-percent`
- websocket/SSE event payloads such as `codex.rate_limits`

The output is always `RateLimitSnapshot`, which keeps downstream rate-limit handling transport-agnostic.

### `src/api_bridge.rs`

This module is the error boundary between `codex-api` and the rest of the workspace.

- It maps crate-local `ApiError` variants into `codex_protocol::error::CodexErr`.
- It enriches 429 handling with parsed rate-limit snapshots, plan type, reset timestamps, and promo messages.
- It extracts request-tracking and identity-debugging metadata from headers.

This is where the crate stops being “transport-only” and starts aligning with workspace-specific error semantics.

## Core Request and Event Flows

### 1. Responses HTTP + SSE flow

The main happy path is:

1. Caller builds `ResponsesApiRequest` and `ResponsesOptions`.
2. `ResponsesClient::stream_request()` serializes the body and attaches transport/session headers.
3. `EndpointSession::stream_with()` builds the request, injects auth headers, and executes it with retry and request telemetry.
4. `spawn_response_stream()` emits header-derived events immediately.
5. `process_sse()` parses SSE frames into `ResponsesStreamEvent`.
6. `process_responses_event()` maps raw event kinds into typed `ResponseEvent`.
7. Caller consumes a unified `ResponseStream`.

The flow is layered cleanly: request shaping, transport execution, and semantic event parsing are all separate.

### 2. Responses websocket flow

The websocket variant works similarly but with different framing:

1. `ResponsesWebsocketClient::connect()` builds the websocket URL and handshake headers.
2. The resulting `ResponsesWebsocketConnection` stores a synchronized websocket stream handle.
3. `stream_request()` sends a serialized `ResponsesWsRequest`.
4. Incoming text frames are decoded as:
   - wrapped websocket error payloads
   - rate-limit events
   - standard response stream events
5. Standard events reuse `process_responses_event()`.

The notable design point is that HTTP/SSE and websocket converge into the same semantic event enum.

### 3. Realtime websocket flow

Realtime is structurally different from Responses:

1. `RealtimeWebsocketClient::connect()` or `connect_webrtc_sideband()` computes a websocket URL based on parser version, session mode, and sometimes call id.
2. On connect, the client immediately sends a `session.update` message.
3. Writer methods send audio frames or conversation items.
4. Reader methods parse protocol-v1 or protocol-v2 text frames into `RealtimeEvent`.
5. The connection tracks transcript state so handoff requests can include recent context.

This is a bidirectional, stateful protocol rather than a unidirectional response stream.

### 4. Realtime call creation flow

`RealtimeCallClient` handles a separate setup step:

1. create the WebRTC call with an SDP offer
2. optionally include initial session config
3. decode answer SDP from the response body
4. extract `call_id` from the `Location` header
5. use the returned `call_id` to join the sideband websocket if needed

### 5. File upload flow

`upload_local_file()` has a three-stage network flow:

1. validate the local file and size limit
2. create the upload resource
3. upload file bytes to a presigned URL
4. poll the finalize endpoint until the file becomes ready

This is separate from the endpoint-client pattern because the second step targets a different upload URL and transport shape.

## Design Observations

### 1. Strong separation between transport and protocol

The crate does not redefine protocol types that already belong in `codex-protocol`, and it does not own the generic transport implementation from `codex-client`. That keeps responsibilities narrow and makes the codebase easier to reason about.

### 2. One shared HTTP execution core

`EndpointSession` removes repeated boilerplate across unary and streaming clients:

- request construction
- auth header injection
- retry handling
- request telemetry

This is a good use of a small generic helper rather than trait-heavy abstraction.

### 3. Uniform semantic stream model

`ResponseStream` and `ResponseEvent` are the core ergonomic win of the crate. Higher layers do not need to care whether the wire transport is SSE or websocket.

### 4. Compatibility logic is kept near the wire

Examples:

- Azure item-id injection for stored Responses requests
- `memories/trace_summarize` legacy path and `traces` alias
- dual request shapes for realtime call creation
- protocol version split for realtime websocket parsing

This is the correct layer for these concerns because callers should not need to know transport quirks.

### 5. Intentional synchronous auth boundary

The auth abstraction is intentionally narrow and synchronous. That choice reduces latency and keeps `codex-api` deterministic, but it also means any refresh workflow must happen before requests reach this crate.

### 6. Realtime and Responses are deliberately separate stacks

Although both use websockets, the code does not force them into one abstraction. That is a good tradeoff because:

- Responses websocket is request/response-stream oriented
- realtime websocket is long-lived, bidirectional, and transcript-aware

Trying to unify those too early would likely obscure important protocol differences.

## Testing Strategy

The crate has good coverage across three levels.

### Unit tests in source modules

Representative unit coverage includes:

- `provider.rs`
  - Azure provider detection
- `rate_limits.rs`
  - header and event parsing across multiple limit families
- `files.rs`
  - successful upload flow and auth-header propagation
- `models.rs`
  - URL shaping, ETag extraction, and response parsing
- `memories.rs`
  - request-body encoding and response parsing
- `compact.rs`
  - endpoint path sanity check
- `responses_websocket.rs`
  - wrapped websocket error mapping and header precedence
- `sse/responses.rs`
  - event parsing, stream completion behavior, tool-call delta handling, and terminal error behavior
- `api_bridge.rs`
  - workspace error translation and header-derived diagnostics

### Integration tests under `tests/`

- `tests/clients.rs`
  - verifies Responses path selection, auth header injection, retry behavior, and Azure-specific item-id/header behavior
- `tests/models_integration.rs`
  - verifies `ModelsClient` against a mock HTTP server
- `tests/sse_end_to_end.rs`
  - verifies end-to-end SSE parsing from raw streamed bytes into `ResponseEvent`
- `tests/realtime_websocket_e2e.rs`
  - verifies realtime websocket session setup, concurrent send/receive behavior, sideband retry, disconnect semantics, ignored unknown events, and protocol-v2 handoff behavior

### Test design quality

The tests are pragmatic:

- source-level unit tests isolate parsing and request shaping
- mock transports validate generic client behavior without a real network
- wiremock integration tests cover real HTTP serialization/deserialization
- websocket end-to-end tests validate the most stateful code path

The biggest strength is that event parsing is tested both as isolated logic and as transport-driven end-to-end behavior.

## Notable Design Constraints

Several constraints are visible in the implementation:

- auth is expected to be pre-resolved before entering this crate
- retries are transport-level, not semantic replays managed by higher-level state
- stream idle timeouts are provider-configured, not request-specific
- websocket event channels are bounded for Responses and unbounded/async-channel based for realtime internals
- realtime parsing supports multiple protocol versions at runtime

These constraints all fit the crate’s role as a thin but opinionated transport façade.

## Open Questions

The code is solid, but a few design questions remain visible:

1. `ResponsesWebsocketConnection` stores the idle timeout on the connection object and even carries an inline TODO asking whether that is the right location. If per-request websocket timeout policy ever differs from per-connection policy, this design may need to move.
2. `RealtimeCallClient` still branches between a backend-specific JSON body and the newer multipart API body. The inline TODO suggests the team wants to remove that divergence once the backend route aligns.
3. `RealtimeWebsocketClient` does not take a `SharedAuthProvider`, unlike most other endpoint clients. That may be intentional because realtime auth is encoded in provider headers/query params, but if realtime ever needs dynamic auth refresh, the current constructor shape may become limiting.
4. Header merging logic exists in both Responses websocket and realtime websocket code paths. If those policies must stay identical long-term, consolidating them could reduce drift risk.
5. The crate already handles several backend-compatibility branches. If more provider-specific behavior accumulates, it may eventually justify a more explicit provider-capability model instead of scattered conditionals.

## Bottom Line

`codex-api` is a well-factored transport adaptation crate.

Its main strengths are:

- a compact public API
- strong reuse of shared protocol types
- clean separation between request execution and event parsing
- good compatibility handling at the correct layer
- meaningful tests around the most failure-prone paths

The crate’s complexity is concentrated in exactly the places where it should be:

- stream parsing
- websocket lifecycle management
- backend compatibility
- error translation

That makes it a strong foundation for higher-level crates such as `codex-core`, which can stay focused on business logic rather than wire details.
