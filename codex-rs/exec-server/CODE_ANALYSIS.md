# CODE_ANALYSIS

## Overview

`codex-exec-server` is a reusable execution and filesystem access layer that can run in two modes:

- **Local mode**: use in-process Rust implementations for process execution and filesystem access.
- **Remote mode**: connect to a websocket JSON-RPC exec-server and proxy the same capabilities over the network.

The crate therefore does not just implement a server. It also defines the shared protocol, a Rust client, local/remote backends, session-resume logic, and the sandboxed helper path for restricted filesystem operations.

At a high level:

- `src/protocol.rs` defines the wire contract.
- `src/client.rs` and `src/client/http_client.rs` implement the client-facing JSON-RPC transport and higher-level routing.
- `src/server/*` implements the websocket server, request routing, handshake enforcement, and resumable sessions.
- `src/local_process.rs` and `src/remote_process.rs` implement the execution backend abstraction.
- `src/local_file_system.rs`, `src/remote_file_system.rs`, `src/sandboxed_file_system.rs`, `src/fs_helper.rs`, and `src/fs_sandbox.rs` implement filesystem access, including sandboxed helper execution.
- `src/environment.rs` selects local vs remote execution/filesystem behavior.

## Primary Responsibilities

### 1. Provide a stable shared protocol

The crate owns the typed request/response/notification payloads for:

- handshake: `initialize`, `initialized`
- process lifecycle: `process/start`, `process/read`, `process/write`, `process/terminate`
- process notifications: `process/output`, `process/exited`, `process/closed`
- filesystem operations: `fs/readFile`, `fs/writeFile`, `fs/createDirectory`, `fs/getMetadata`, `fs/readDirectory`, `fs/remove`, `fs/copy`
- executor HTTP requests on the client side: `http/request` and `http/request/bodyDelta`

Notable protocol design choices:

- binary payloads are base64 encoded through `ByteChunk`
- `ProcessId` is a logical protocol key, not an OS pid
- `ReadResponse` carries both retained output and terminal state
- sandbox settings are explicitly passed with filesystem requests via `FileSystemSandboxContext`

## Public Surface

`src/lib.rs` re-exports the main API so downstream crates usually depend only on the crate root.

The most important exported items are:

- `ExecServerClient` and `ExecServerError`
- `Environment` and `EnvironmentManager`
- `ExecBackend`, `ExecProcess`, `StartedExecProcess`, `ExecProcessEvent`
- `ExecutorFileSystem` and filesystem option/result types
- all protocol structs from `src/protocol.rs`
- `ExecServerRuntimePaths`, `DEFAULT_LISTEN_URL`, `run_main`

This public surface is intentionally layered:

- callers that want raw protocol access can use `ExecServerClient`
- callers that want “local or remote, decided by configuration” use `Environment`
- callers that want an embeddable server call `run_main`

## Module Responsibilities

### Core protocol and transport

- `src/protocol.rs` defines method names and all wire payloads.
- `src/process_id.rs` wraps process ids as a strongly typed transparent newtype.
- `src/connection.rs` abstracts JSON-RPC transport framing over websocket, plus stdio-only test support.
- `src/rpc.rs` provides the JSON-RPC router, client call tracking, request id matching, and notification/event fan-out.

### Environment selection

- `src/environment.rs` chooses local or remote mode from `CODEX_EXEC_SERVER_URL`.
- `EnvironmentManager` caches the chosen environment with `OnceCell`.
- `"none"` is treated specially as a disabled mode, preserving an intentional “no environment” state.

### Process execution

- `src/process.rs` defines the `ExecBackend` and `ExecProcess` traits plus the retained event log abstraction.
- `src/local_process.rs` starts and manages subprocesses with `codex-utils-pty`, retains output, emits notifications, and supports long-poll reads plus push subscriptions.
- `src/remote_process.rs` wraps `ExecServerClient` so the same trait surface works against a remote server.

### Filesystem access

- `src/file_system.rs` defines the `ExecutorFileSystem` trait and sandbox context.
- `src/local_file_system.rs` chooses direct vs sandboxed local filesystem execution and contains the raw direct filesystem implementation.
- `src/remote_file_system.rs` translates trait calls into remote JSON-RPC filesystem requests.
- `src/sandboxed_file_system.rs` delegates restricted filesystem requests to a helper process executed inside a sandbox.

### Sandboxed helper pipeline

- `src/fs_helper.rs` defines helper request/response enums and runs a request against `DirectFileSystem`.
- `src/fs_helper_main.rs` is the hidden helper entrypoint that reads one request from stdin and writes one response to stdout.
- `src/fs_sandbox.rs` builds the sandbox command, filters helper environment variables, and runs the helper process.
- `src/runtime_paths.rs` carries the absolute paths required to re-enter the top-level `codex` binary and optional Linux sandbox alias.

### Server side

- `src/server/transport.rs` parses `ws://IP:PORT`, binds a websocket listener, prints the final bound URL, and accepts connections.
- `src/server/processor.rs` owns one connection loop and converts `JsonRpcConnection` events into routed handler calls.
- `src/server/registry.rs` registers JSON-RPC routes.
- `src/server/handler.rs` enforces the initialize/initialized handshake, binds a resumable session, and exposes process/filesystem handlers.
- `src/server/session_registry.rs` keeps detached sessions alive briefly so a new websocket can resume them.
- `src/server/process_handler.rs` and `src/server/file_system_handler.rs` adapt `LocalProcess` and `LocalFileSystem` into server JSON-RPC handlers.

## Main APIs and Behavior

### Environment API

`Environment` is the main “selection” abstraction.

- `Environment::create(None)` builds local execution/filesystem behavior.
- `Environment::create(Some(ws_url))` connects a remote client and exposes remote execution/filesystem behavior.
- `Environment::get_exec_backend()` returns `Arc<dyn ExecBackend>`.
- `Environment::get_filesystem()` returns `Arc<dyn ExecutorFileSystem>`.

This design lets higher layers stay agnostic about whether execution is local or remote.

### Process API

`ExecBackend::start` returns `StartedExecProcess`, which wraps `Arc<dyn ExecProcess>`.

`ExecProcess` supports two access patterns:

- **polling** via `read(after_seq, max_bytes, wait_ms)`
- **streaming** via `subscribe_events()`

It also supports:

- `subscribe_wake()` for low-overhead change notification
- `write(chunk)` for PTY or piped stdin
- `terminate()`

The dual retained-output plus live-event design is one of the crate’s core architectural choices.

### Filesystem API

`ExecutorFileSystem` exposes:

- `read_file`
- `read_file_text`
- `write_file`
- `create_directory`
- `get_metadata`
- `read_directory`
- `remove`
- `copy`

The same trait works locally, remotely, and for sandboxed local execution.

## End-to-End Flows

### 1. Remote client connection flow

1. `ExecServerClient::connect_websocket` opens a websocket and wraps it in `JsonRpcConnection`.
2. `ExecServerClient::initialize` sends `initialize`, stores the returned `session_id`, then sends `initialized`.
3. The client starts a background reader task that consumes JSON-RPC notifications and disconnect events.
4. Process notifications are routed by `process_id` into per-process `SessionState`.
5. HTTP body delta notifications are routed by generated `request_id` into request-local channels.

### 2. Server connection flow

1. `run_main` calls `server::transport::run_transport`.
2. `run_transport` binds a websocket listener and spawns a task per accepted connection.
3. `ConnectionProcessor` creates an `ExecServerHandler`, router, and outbound notification channel.
4. Incoming requests and notifications are handled sequentially to preserve handshake ordering.
5. On disconnect or protocol failure, the handler shuts down or detaches its session.

### 3. Session resume flow

1. A connection successfully initializes and receives a generated `session_id`.
2. On disconnect, `SessionHandle::detach` removes the notification sender and marks the session detached.
3. The detached session remains resumable for a fixed TTL.
4. A new connection can call `initialize` with `resume_session_id`.
5. If resume succeeds, the new connection reuses the same `ProcessHandler`, so managed processes stay alive.

Important implications:

- long-poll reads on the old connection fail once the session is resumed elsewhere
- the notification sender is swapped so pushed events move to the new connection
- detached sessions are cleaned up after TTL expiry by shutting down remaining processes

### 4. Local process execution flow

1. `LocalProcess::start_process` validates `argv` and reserves the `ProcessId`.
2. It chooses PTY, piped-stdin, or no-stdin spawn mode from `ExecParams`.
3. Output reader tasks retain chunks in memory and emit `process/output` notifications.
4. Exit and closed events advance a per-process sequence number.
5. `process/read` long-polls by waiting on `Notify` until output or terminal state changes.

Key retained-state rules:

- output is bounded per process by `RETAINED_OUTPUT_BYTES_PER_PROCESS`
- output, exit, and close events are sequenced with `next_seq`
- exited processes remain temporarily retained before final removal

### 5. Sandboxed filesystem flow

1. A filesystem call arrives with an optional `FileSystemSandboxContext`.
2. `LocalFileSystem` chooses `SandboxedFileSystem` only for `ReadOnly` and `WorkspaceWrite` policies.
3. `FileSystemSandboxRunner` derives sandbox policy inputs and prepares a sandbox transform.
4. It launches the current `codex` executable with `--codex-run-as-fs-helper`.
5. The helper reads one JSON request from stdin, performs the operation with `DirectFileSystem`, and writes one JSON response to stdout.

This keeps the main server logic simple while pushing restricted filesystem execution into a short-lived isolated subprocess.

## Design Characteristics

### Strong points

- **Uniform local/remote abstraction**: `Environment`, `ExecBackend`, and `ExecutorFileSystem` let callers ignore transport details.
- **Resumable sessions**: the server preserves subprocess state across websocket reconnects.
- **Dual output model**: retained reads plus pushed events cover both polling and streaming consumers.
- **Ordered remote event delivery**: the client reorders out-of-sequence notifications before publishing them.
- **Sandbox isolation**: restricted filesystem work happens in a helper subprocess rather than inside the long-lived server process.
- **Typed protocol**: all payloads are concrete Rust structs rather than ad hoc JSON blobs.

### Tradeoffs

- **In-memory output retention**: bounded retention is simple, but large-output workloads can drop older history.
- **One helper process per sandboxed filesystem operation**: simple and isolated, but potentially expensive for high-volume filesystem traffic.
- **Fixed timing knobs**: detached-session TTL and exited-process retention are constants, not runtime-configurable.
- **Client-side ordering complexity**: remote event ordering is correct but requires extra buffering and state machinery.

## Dependency Analysis

### Async and transport

- `tokio`: async runtime, channels, timeouts, filesystem, processes, networking
- `futures`: websocket split/send/receive helpers
- `tokio-tungstenite`: websocket transport

### Serialization and protocol

- `serde`, `serde_json`: wire encoding/decoding
- `base64`: binary payload encoding
- `thiserror`: public error definitions

### Concurrency and state

- `arc-swap`: cheap read-mostly routing tables in the remote client
- `async-trait`: async trait methods for `ExecBackend` and `ExecutorFileSystem`
- `uuid`: session ids and connection ids

### Workspace-specific crates

- `codex-app-server-protocol`: shared JSON-RPC message envelope types
- `codex-utils-pty`: actual process spawning and PTY support
- `codex-config`: environment policy construction for child processes
- `codex-protocol`: sandbox policy and permission types
- `codex-sandboxing`: sandbox selection and command transformation
- `codex-utils-absolute-path`: absolute-path enforcement

These workspace crates do much of the heavy lifting; this crate mainly composes them into a coherent execution/filesystem service.

## Testing Strategy

The crate is well-tested across unit and integration layers.

### Unit and module tests

- `src/rpc.rs`: request/response matching and out-of-order response handling
- `src/process.rs`: retained event replay bounding
- `src/local_process.rs`: environment policy behavior
- `src/local_file_system.rs`: path resolution and symlink edge cases
- `src/remote_file_system.rs`: remote-to-io error mapping
- `src/fs_helper.rs` and `src/fs_sandbox.rs`: helper protocol shape, env filtering, permission shaping, sandbox inputs
- `src/server/handler/tests.rs`: duplicate process ids, terminate semantics, resume behavior, output retention after notification loss
- `src/server/transport_tests.rs`: listen URL parsing
- `src/client.rs`: ordered notification delivery and per-session wake isolation

### Integration tests

- `tests/initialize.rs`: initialize handshake works end to end
- `tests/process.rs`: websocket process lifecycle and session resume
- `tests/exec_process.rs`: local and remote execution backends behave the same for reads, pushed events, stdin handling, replay, and disconnects
- `tests/file_system.rs`: local and remote filesystem parity, symlink handling, recursive copy behavior, sandbox enforcement, helper PATH preservation
- `tests/http_client.rs`: streaming HTTP body routing, cancellation, drop cleanup, disconnect handling, and backpressure behavior
- `tests/websocket.rs`: malformed websocket JSON yields an error but does not kill the server

### Coverage observations

- the tests strongly emphasize behavior parity between local and remote backends
- reconnect and resume behavior receives explicit coverage
- sandbox escape attempts through symlinks and path normalization are heavily tested
- many integration tests are `#[cfg(unix)]`, so end-to-end Windows coverage appears lighter

## Notable Internal Design Details

### Event ordering

Remote process notifications can arrive out of order because output, exit, and close are emitted by different async tasks. The client compensates with `OrderedSessionEvents`, storing future events in a `BTreeMap` until all lower sequence numbers have been published.

That is a good fit for the protocol because:

- the server already emits a monotonic per-process `seq`
- consumers receive a single ordered stream
- dropped or duplicated sequence numbers can be detected cleanly

### Wake channels versus event streams

The crate uses both:

- a `watch` channel for cheap “something changed” wakeups
- a replay-plus-broadcast event stream for richer push consumption

This avoids forcing every consumer to pay event-stream costs while still supporting streaming clients.

### Session attachment model

`SessionRegistry` tracks:

- current attached connection id
- last detached connection id
- detached expiry deadline

This is a practical middle ground between:

- killing all subprocesses on disconnect
- keeping sessions alive indefinitely

### Filesystem sandboxing model

Sandboxing is isolated to filesystem operations that truly require it. `DangerFullAccess` and `ExternalSandbox` requests do not automatically force the helper path, while `ReadOnly` and `WorkspaceWrite` do.

That keeps unrestricted operations fast and keeps the sandbox boundary explicit.

## Open Questions

1. **Where is server-side `http/request` implemented?**  
   The protocol and public client support `http/request` and `http/request/bodyDelta`, but the server router only registers initialize, process, and filesystem methods. This may be intentional, but the crate currently exposes client-side HTTP helpers without matching server-side request handlers.

2. **Is the README stale around `initialize`?**  
   The README example shows an empty initialize response, but the actual protocol returns `{ "sessionId": ... }`.

3. **Should `client_name` do more?**  
   `InitializeParams` includes `client_name`, but the server currently uses it only implicitly during handshake, without validation, storage, policy selection, or logging impact beyond generic tracing.

4. **Should retention and TTL values be configurable?**  
   `EXITED_PROCESS_RETENTION` and detached-session TTL are hard-coded. That is simple, but deployments with long reconnect windows or heavy output may want tuning knobs.

5. **What is the intended Windows support level for full integration behavior?**  
   There are Windows-aware branches in the implementation, but many end-to-end tests are Unix-only.

6. **Is the PTY stream mapping intentionally duplicated?**  
   In local PTY mode, both spawned readers publish `ExecOutputStream::Pty`, and close accounting assumes two streams. This may match `codex-utils-pty` semantics, but it is worth confirming explicitly.

## Bottom Line

This crate is best understood as an **execution/filesystem abstraction layer with both server and client roles**, not only as a websocket server. Its strongest qualities are:

- clean local/remote substitution
- resumable process sessions
- careful ordering and disconnect handling
- security-conscious sandboxed filesystem execution

Its main complexity lives in three places:

- process lifecycle and retained output handling
- remote notification ordering and failure propagation
- sandbox helper orchestration for filesystem calls

Those are also the areas where future maintainers should focus first when changing behavior.
