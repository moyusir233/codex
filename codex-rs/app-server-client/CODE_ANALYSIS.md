# codex-app-server-client Code Analysis

## Scope

This document analyzes the crate at `app-server-client/`, focusing on:

- `Cargo.toml`
- `src/lib.rs`
- `src/remote.rs`
- inline tests in both modules

The crate exposes a unified async client surface for talking to Codex app-server through either:

- an embedded in-process runtime, or
- a websocket-based remote connection

The core architectural theme is transport unification: higher-level callers get a consistent request / notification / event API regardless of whether the app-server lives inside the same process or behind a websocket.

## High-Level Responsibilities

The crate is responsible for six concrete jobs:

1. Start and initialize an embedded app-server runtime for CLI/TUI surfaces.
2. Connect and initialize a remote websocket app-server.
3. Expose a common async API for requests, notifications, server-request replies, event consumption, and shutdown.
4. Buffer transport activity through bounded Tokio channels so callers are isolated from low-level runtime/socket details.
5. Preserve lossless delivery for transcript-critical notifications while degrading less important events under backpressure.
6. Translate transport, JSON-RPC, and deserialization failures into caller-usable error shapes.

In practice, this makes the crate the transport adapter layer between UI surfaces such as `codex-exec` and `codex-tui` and the app-server protocol/runtime.

## Module Layout

### `src/lib.rs`

Owns the embedded/in-process implementation and most of the public surface:

- public enums and shared types (`AppServerEvent`, `AppServerClient`, `AppServerRequestHandle`, `TypedRequestError`)
- embedded startup arguments (`InProcessClientStartArgs`)
- embedded worker loop (`InProcessAppServerClient`)
- common typed request helpers (`request_typed`)
- common notification classification (`server_notification_requires_delivery`)
- transitional `legacy_core` re-exports for callers that still need selected `codex-core` items

### `src/remote.rs`

Owns the websocket transport:

- connection argument type (`RemoteAppServerConnectArgs`)
- websocket auth-token transport policy
- initialize / initialized handshake
- remote worker loop
- JSON-RPC encoding/decoding glue
- remote event backpressure handling

## Public API Surface

### Shared user-facing types

- `AppServerClient`
  - enum over `InProcess(InProcessAppServerClient)` and `Remote(RemoteAppServerClient)`
  - gives callers a single transport-agnostic object
- `AppServerRequestHandle`
  - cloneable request-only handle for concurrent request issuing
- `AppServerEvent`
  - normalized event stream with:
    - `Lagged { skipped }`
    - `ServerNotification(ServerNotification)`
    - `ServerRequest(ServerRequest)`
    - `Disconnected { message }`
- `RequestResult`
  - preserves the app-server JSON-RPC result envelope even on the in-process path
- `TypedRequestError`
  - separates transport, server-side JSON-RPC, and response decode errors

### Embedded startup

- `InProcessClientStartArgs`
  - bundles runtime config, startup identity, feedback/logging hooks, environment manager, initialize capabilities, and queue sizing
  - `initialize_params()` builds protocol-level initialize parameters
  - `into_runtime_start_args()` converts facade arguments into `codex_app_server::in_process::InProcessStartArgs`

- `InProcessAppServerClient`
  - `start(args)` boots the runtime and spawns the worker
  - `request()` sends a raw typed request and returns JSON-RPC result
  - `request_typed<T>()` decodes a successful result body into `T`
  - `notify()` sends a client notification
  - `resolve_server_request()` and `reject_server_request()` answer server-initiated requests
  - `next_event()` yields the next embedded event
  - `shutdown()` performs bounded graceful shutdown with abort fallback
  - `request_handle()` returns a cloneable request handle

### Remote startup

- `RemoteAppServerConnectArgs`
  - carries websocket URL, optional auth token, initialize identity/capabilities, and channel capacity
- `RemoteAppServerClient`
  - mirrors the embedded client methods, but starts from `connect(args)`
  - `connect()` validates the URL, applies auth policy, performs websocket connect, runs initialize/initialized, and spawns the worker

## Core Flows

### 1. Embedded flow

1. The caller constructs `InProcessClientStartArgs`.
2. `start()` clamps `channel_capacity` to at least 1 and starts `codex_app_server::in_process`.
3. The crate creates two bounded Tokio channels:
   - command channel: caller -> worker
   - event channel: worker -> caller
4. A worker task bridges public commands onto `InProcessClientHandle`.
5. Requests are delegated through detached tasks so the worker can keep draining events while a request blocks.
6. Runtime events are classified as must-deliver or best-effort.
7. Must-deliver events block on `send`; best-effort events use `try_send`.
8. If best-effort delivery fails, the crate increments `skipped_events` and later emits `Lagged`.
9. If a dropped event is a server request, the worker actively rejects it so the server does not hang waiting for a response.
10. `shutdown()` drops the caller event receiver first, requests graceful shutdown, then aborts the worker if the timeout is exceeded.

### 2. Remote websocket flow

1. The caller constructs `RemoteAppServerConnectArgs`.
2. `connect()` parses the websocket URL and rejects unsafe auth-token transport combinations.
3. If `auth_token` is set, the websocket handshake includes `Authorization: Bearer ...`.
4. The crate ensures a rustls crypto provider exists before connecting.
5. `initialize_remote_connection()` sends `initialize`, waits for its response with timeout, queues any early server notifications/requests, then sends `initialized`.
6. A worker task multiplexes:
   - outgoing commands from callers
   - incoming websocket frames from the remote server
7. Pending requests are tracked in a `HashMap<RequestId, oneshot::Sender<...>>`.
8. Responses/errors resolve the matching waiter.
9. Notifications and server requests are converted into `AppServerEvent`s and delivered through the same bounded-channel policy as the embedded path.
10. Disconnects, invalid JSON-RPC payloads, and transport failures surface as `AppServerEvent::Disconnected`.

## Backpressure Model

The most important design choice in this crate is the two-tier event delivery model.

### Must-deliver notifications

`server_notification_requires_delivery()` classifies these as lossless:

- `TurnCompleted`
- `ItemCompleted`
- `AgentMessageDelta`
- `PlanDelta`
- `ReasoningSummaryTextDelta`
- `ReasoningTextDelta`

Rationale:

- dropping transcript deltas corrupts visible assistant output
- dropping completion markers can leave the UI waiting forever for completion

### Best-effort events

Everything else is best-effort and may be dropped under pressure, including progress-style and command-output notifications.

### Lag signaling

When best-effort events are dropped:

- the crate increments `skipped_events`
- later emits `Lagged { skipped }`
- flushes that marker before the next must-deliver event when necessary

This is a practical compromise:

- it bounds memory usage
- it preserves critical correctness signals
- it still tells callers that they missed non-critical events

### Server-request overload behavior

If a dropped event is actually a `ServerRequest`, the crate rejects it instead of silently losing it:

- in-process path: error `-32001`, message `"in-process app-server event queue is full"`
- remote path: error `-32001`, message `"remote app-server event queue is full"`

That avoids deadlocking approval or input flows behind a saturated queue.

## Error Handling Model

### `TypedRequestError`

The layered error type is well designed because it preserves distinct failure semantics:

- `Transport`
  - the request never completed due to channel/socket/runtime problems
- `Server`
  - the request reached app-server, but app-server replied with a JSON-RPC error
- `Deserialize`
  - the request succeeded, but the caller chose the wrong expected response type or the implementation returned an unexpected shape

This is especially useful for surfaces that need different user messaging or retry policy.

### Shutdown behavior

Both transports use the same bounded shutdown pattern:

- try graceful shutdown first
- wait up to `SHUTDOWN_TIMEOUT` (5 seconds)
- abort the worker if it does not finish

This favors process hygiene over perfect graceful completion.

## Dependencies and Their Roles

`Cargo.toml` shows that this crate is mostly an orchestration layer on top of existing workspace crates.

### Core workspace dependencies

- `codex-app-server`
  - embedded runtime startup and in-process event handle
- `codex-app-server-protocol`
  - request/notification/event enums and JSON-RPC envelope types
- `codex-core`
  - config types, cloud requirement loaders, and transitional `legacy_core` re-exports
- `codex-config`
  - `NoopThreadConfigLoader` used when constructing embedded runtime args
- `codex-arg0`
  - resolved helper dispatch paths
- `codex-exec-server`
  - `EnvironmentManager` and runtime-path types exposed to callers
- `codex-feedback`
  - feedback sink passed into embedded runtime startup
- `codex-protocol`
  - `SessionSource`
- `codex-utils-rustls-provider`
  - remote TLS provider initialization before websocket connect

### Async / serialization dependencies

- `tokio`
  - channels, timeouts, tasks, runtime integration
- `futures`
  - websocket stream/sink helpers
- `serde`, `serde_json`, `toml`
  - protocol encoding/decoding and caller override transport
- `tokio-tungstenite`
  - websocket transport implementation
- `url`
  - remote URL parsing and validation
- `tracing`
  - overload / protocol warnings

### Dev dependencies

- `tokio` with macros + multi-thread runtime
- `serde_json`
- `pretty_assertions`

## Test Coverage

The crate keeps tests inline inside `src/lib.rs` and `src/remote.rs`. There is no separate `tests/` directory.

### Embedded behavior covered

- typed request round-trip works
- JSON-RPC errors map into `TypedRequestError::Server`
- caller-provided `SessionSource` reaches thread metadata
- threads started via app-server are readable through subsequent requests
- tiny channel capacity still allows round-trip requests
- transcript-critical notifications survive backpressure
- lagged markers surface through `next_event()`
- delivery classification behaves as intended
- environment manager is forwarded into runtime start args
- shutdown completes promptly

### Remote behavior covered

- initialize + request round-trip over websocket
- optional auth header injection
- unsafe auth-token URL combinations are rejected
- duplicate request IDs are rejected locally while original waiter remains intact
- server notifications arrive as `AppServerEvent`
- transcript-critical remote notifications survive backpressure
- server-request resolution round-trip works
- server requests arriving during initialize are queued and later delivered
- unknown server requests are rejected with JSON-RPC `-32601`
- disconnects surface as `AppServerEvent::Disconnected`

### Test style observations

The tests are concrete and behavior-driven rather than purely unit-level:

- embedded tests exercise real startup paths against the in-process runtime
- remote tests spin up an ad hoc websocket server and validate wire behavior

That gives this small crate unusually strong transport-level confidence for its size.

## Design Strengths

### 1. Transport-agnostic caller experience

`AppServerClient` and `AppServerRequestHandle` let upper layers write mostly transport-independent code. This is the right abstraction boundary for UI surfaces that may operate embedded or remotely.

### 2. Explicit overload strategy

The code does not pretend channels are infinite. Instead, it:

- bounds queues
- marks lag explicitly
- preserves critical events
- rejects server requests that cannot be delivered

This is much safer than silently accumulating memory or silently dropping completion signals.

### 3. Good shutdown hygiene

Dropping the event receiver before shutdown is subtle but important. It prevents a must-deliver event send from blocking shutdown forever.

### 4. Strong typed error boundaries

`TypedRequestError` makes it clear whether the fault is:

- transport-level
- app-server-level
- local decode-level

That separation is valuable in callers.

### 5. Early-connection event buffering on remote initialize

`initialize_remote_connection()` buffers notifications and server requests that arrive before initialize completes. This avoids a race where the remote side talks early and the client loses those messages.

## Design Tradeoffs and Risks

### 1. API duplication

`request`, `request_typed`, and some command-send boilerplate are duplicated across:

- `InProcessAppServerClient`
- `InProcessAppServerRequestHandle`
- `RemoteAppServerClient`
- `RemoteAppServerRequestHandle`
- transport-erased enums

This is readable, but it increases maintenance cost and the risk of future semantic drift.

### 2. Serialization-based method-name extraction

`request_method_name()` serializes a `ClientRequest` into JSON and then reads `"method"` back out. This avoids adding helpers to the protocol crate, but it is indirect and fragile if request encoding changes.

### 3. Transitional `legacy_core` surface

The `legacy_core` module is explicitly transitional. It helps reduce direct `codex-core` coupling in callers, but it also means this crate currently mixes:

- transport abstraction
- embedded runtime startup
- migration shim exports

That broadens its responsibility.

### 4. Potentially silent dropping of non-critical remote notifications

The backpressure policy is intentional, but callers that care about command output or progress events must tolerate missing intermediate updates and rely on `Lagged` as a hint rather than a replay mechanism.

## Integration Notes

The crate is already consumed as a shared client layer by at least:

- `codex-exec`, which starts `InProcessAppServerClient` and drives thread start/resume through app-server requests
- `codex-tui`, which can choose either embedded or remote app-server mode and wraps both in `AppServerClient`

This confirms the crate is meant to sit at the boundary between UI/application surfaces and the lower-level app-server runtime/protocol.

## Open Questions

1. Can the duplicated request/notify/reply boilerplate be factored behind a trait or internal helper without obscuring transport-specific behavior?
2. When `legacy_core` migration finishes, should those re-exports leave this crate entirely so it becomes a pure transport facade?
3. Is there a need for a richer recovery story after `Lagged`, such as explicit replay/snapshot APIs for best-effort event classes?
4. Should request method names be exposed directly by `codex-app-server-protocol` instead of recovered via JSON serialization?
5. Should the remote transport eventually expose stronger ordering or durability guarantees for non-transcript notifications, or is best-effort sufficient for all current consumers?

## Bottom Line

`codex-app-server-client` is a focused transport/orchestration crate that turns two very different app-server access modes into one consistent async client API.

Its most notable design idea is not just “embedded vs remote”; it is the explicit event-delivery policy:

- preserve correctness-critical transcript/completion events
- bound memory through finite queues
- surface overload through `Lagged`
- fail fast on undeliverable server requests

That makes the crate a reliability layer for UI consumers, not just a thin wrapper around app-server handles.
