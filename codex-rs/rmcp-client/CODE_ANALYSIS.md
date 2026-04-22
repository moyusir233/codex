# codex-rmcp-client crate analysis

## Overview

`codex-rmcp-client` is a transport-and-auth wrapper around the upstream `rmcp` Rust SDK. It gives the rest of Codex a single client abstraction that can talk to MCP servers over:

- local stdio child processes,
- executor-managed stdio processes, and
- streamable HTTP, with optional bearer-token or OAuth authentication.

The crate is not just a thin SDK re-export. It adds Codex-specific behavior around process launching, environment shaping, HTTP header injection, OAuth credential persistence, recovery from expired streamable-HTTP sessions, and elicitation/UI bridging.

## Top-level responsibilities

### 1. Unified MCP client API

The main entry point is `RmcpClient` in `src/rmcp_client.rs`.

It owns:

- transport selection and construction,
- the MCP initialize handshake,
- steady-state request execution (`tools/list`, `tools/call`, `resources/list`, `resources/read`),
- custom notifications and custom requests,
- timeout handling, and
- limited streamable-HTTP session recovery.

This makes the rest of the application largely transport-agnostic once a client has been constructed.

### 2. Local and remote stdio launching

`src/stdio_server_launcher.rs` separates “how to start the process” from “how to speak MCP once it is running”.

- `LocalStdioServerLauncher` spawns a normal child process with stdin/stdout pipes.
- `ExecutorStdioServerLauncher` starts the process through `codex_exec_server::ExecBackend`.
- Both return `StdioServerTransport`, which implements rmcp’s `Transport<RoleClient>` and hides whether the process is local or executor-backed.

This boundary is a clean design choice: `RmcpClient` does not need to know where the server process lives.

### 3. Executor byte-stream adaptation

`src/executor_process_transport.rs` exists only for executor-backed stdio.

Its job is to adapt executor process events into rmcp’s line-delimited JSON transport contract:

- serialize outgoing JSON-RPC messages and append `\n`,
- write bytes through executor `process/write`,
- buffer stdout until full newline-delimited messages are available,
- log stderr separately, and
- recover from broadcast lag by replaying retained process output via `process/read`.

This module is intentionally lower level than the rest of the crate.

### 4. OAuth discovery, login, storage, and refresh

The OAuth-related logic is split across three modules:

- `src/auth_status.rs`: determines whether a streamable HTTP server uses bearer auth, saved OAuth, discoverable OAuth, or no supported auth.
- `src/perform_oauth_login.rs`: performs the interactive OAuth login flow with a local callback server.
- `src/oauth.rs`: persists, loads, deletes, and refreshes OAuth credentials.

Together, these modules turn `rmcp`’s auth support into a user-facing Codex authentication workflow.

### 5. Elicitation/UI bridging

`src/elicitation_client_service.rs` wraps the rmcp client service implementation and intercepts `CreateElicitationRequest` so the UI can prompt the user and return a response.

The crate also pauses operation timeouts while an elicitation is pending, which is important because interactive user prompts should not count against normal MCP request time budgets.

### 6. Diagnostics and operational hardening

The crate adds several reliability features on top of `rmcp`:

- process-group cleanup for local stdio children on Unix,
- stderr logging for MCP subprocesses,
- session-expiry recovery for streamable HTTP 404s,
- credential persistence after initialize and after tool/resource operations,
- environment filtering for remote execution, and
- fallback credential storage when keyring access is unavailable.

## Public API surface

The crate’s public exports are defined in `src/lib.rs`.

### Core client API

- `RmcpClient`
- `Elicitation`
- `ElicitationResponse`
- `SendElicitation`
- `ToolWithConnectorId`
- `ListToolsWithConnectorIdResult`
- `ElicitationAction`

### Launchers

- `StdioServerLauncher`
- `LocalStdioServerLauncher`
- `ExecutorStdioServerLauncher`

### Authentication and OAuth helpers

- `determine_streamable_http_auth_status`
- `discover_streamable_http_oauth`
- `supports_oauth_login`
- `StreamableHttpOAuthDiscovery`
- `perform_oauth_login`
- `perform_oauth_login_silent`
- `perform_oauth_login_return_url`
- `OauthLoginHandle`
- `OAuthProviderError`
- `StoredOAuthTokens`
- `WrappedOAuthTokenResponse`
- `save_oauth_tokens`
- `delete_oauth_tokens`
- `McpAuthStatus`

### Important `RmcpClient` methods

The primary steady-state API is:

- `new_stdio_client(...)`
- `new_streamable_http_client(...)`
- `initialize(...)`
- `list_tools(...)`
- `list_tools_with_connector_ids(...)`
- `list_resources(...)`
- `list_resource_templates(...)`
- `read_resource(...)`
- `call_tool(...)`
- `send_custom_notification(...)`
- `send_custom_request(...)`

### API observations

- The API is intentionally explicit rather than builder-based.
- Constructor signatures are long, especially for streamable HTTP and OAuth flows.
- The crate exposes only high-value public types; transport internals stay private.
- `list_tools_with_connector_ids` is a Codex convenience API layered over standard MCP tool metadata.

## Module-by-module analysis

### `src/rmcp_client.rs`

This is the core orchestration module.

Key internal ideas:

- `TransportRecipe` stores enough information to recreate the transport later.
- `ClientState` is a small state machine with `Connecting` and `Ready`.
- `PendingTransport` captures whether the transport is stdio, plain HTTP, or HTTP with OAuth runtime support.
- `InitializeContext` stores enough information to reinitialize after a session-expiry failure.

Notable behavior:

- `new_stdio_client` and `new_streamable_http_client` eagerly create a pending transport.
- `initialize` consumes the pending transport, creates an `ElicitationClientService`, calls `rmcp::service::serve_client`, and transitions the client into `Ready`.
- Every request path calls `refresh_oauth_if_needed` before the operation and `persist_oauth_tokens` after the operation.
- `call_tool` validates that `arguments` and request `_meta` are JSON objects before sending them over rmcp.
- `list_tools_with_connector_ids` scrapes connector metadata from tool `_meta` and normalizes a few naming variants.

The most important resilience logic is `run_service_operation(...)`:

- it runs the operation once,
- detects a specific “session expired” shape for streamable HTTP 404 failures,
- reinitializes the client once using the stored `TransportRecipe` and `InitializeContext`,
- then retries the operation once.

This recovery strategy is narrow by design. It does not attempt generic replay for arbitrary transport failures.

### `src/stdio_server_launcher.rs`

This module is well-factored and intentionally documents its architectural role.

Key design choices:

- `StdioServerLauncher` is sealed, so outside code can use but not implement the trait.
- `StdioServerCommand` carries program, args, env, env-var forwarding config, and optional cwd.
- `StdioServerTransport` hides whether the process is local or executor-backed.

Local launcher details:

- builds a controlled environment with `create_env_for_mcp_server`,
- resolves the program with `program_resolver`,
- clears inherited environment and then injects only the computed env,
- logs stderr lines asynchronously, and
- on Unix, assigns a process group and later kills the full group on drop.

Executor launcher details:

- converts `OsString` values into UTF-8 strings because the process RPC API is string-based,
- forwards only an overlay of explicitly requested variables,
- uses an explicit cwd because executor `process/start` requires one,
- builds an `ExecEnvPolicy` that can expose remote-source env vars without passing through the entire environment.

This is one of the crate’s strongest modules from a design perspective because the responsibility split is very clear.

### `src/executor_process_transport.rs`

This module translates executor process RPC into rmcp transport behavior.

Its core responsibilities are:

- outgoing message framing,
- incoming stdout buffering,
- stderr logging,
- sequence-based duplicate suppression,
- lag recovery via retained reads, and
- best-effort process termination on close/drop.

The line-oriented buffering logic is important:

- rmcp stdio expects one JSON-RPC message per line,
- stdout chunks may arrive split arbitrarily across executor events,
- the transport must buffer until a newline appears,
- after closure it accepts one final unterminated line, mirroring EOF-friendly behavior.

This code is subtle but coherent. The sequence tracking via `last_seq` is especially important because executor event delivery can lag or replay.

### `src/elicitation_client_service.rs`

This module decorates the normal client service behavior with special handling for server-side elicitation requests.

Important details:

- `CreateElicitationRequest` is intercepted and sent to the UI callback.
- request context `_meta` is restored into the elicitation payload because rmcp lifts JSON-RPC `_meta` into `RequestContext`.
- `progressToken` is removed from the reattached metadata.
- result-level `_meta` is manually serialized because rmcp’s typed `CreateElicitationResult` does not model it.

The crate’s timeout pausing depends on this module entering and dropping an `ElicitationPauseGuard`.

### `src/logging_client_handler.rs`

This is mostly a logging adapter over rmcp’s client handler trait.

It:

- forwards elicitation through `SendElicitation`,
- logs cancellation/progress/resource-change/prompt-change/tool-change events,
- maps server log levels onto `tracing` levels.

It is operationally useful, but intentionally simple.

### `src/auth_status.rs`

This module answers a practical question for the rest of the system: “How should Codex authenticate to this streamable HTTP MCP server?”

Decision order:

1. explicit bearer-token env var means bearer auth,
2. explicit `Authorization` header means bearer auth,
3. saved OAuth tokens mean OAuth,
4. otherwise, try RFC 8414 discovery for OAuth metadata,
5. if discovery fails or is unsupported, treat auth as unsupported.

Notable design detail:

- discovery failures are deliberately downgraded to `Unsupported` rather than surfaced as hard errors.

That makes the UX more tolerant, but it also means configuration or connectivity problems can be masked as “no OAuth support”.

### `src/perform_oauth_login.rs`

This module runs the interactive login flow.

Main flow:

- build a local callback server with `tiny_http`,
- resolve callback bind host, callback port, and redirect URI,
- build an `OAuthState`,
- start authorization with requested scopes,
- optionally launch the browser,
- wait for the callback,
- hand the callback code and state back to `OAuthState`,
- extract resulting credentials and save them.

The `perform_oauth_login_return_url` variant is useful for non-blocking or externally managed browser flows because it returns both the authorization URL and a completion future.

Notable strengths:

- callback parsing handles success and provider-reported errors,
- browser launch failure still leaves the printed URL available,
- timeout is configurable and clamped to at least one second.

### `src/oauth.rs`

This module is more than persistence glue; it is effectively the credential runtime for streamable HTTP OAuth sessions.

Important pieces:

- `StoredOAuthTokens` stores server identity, client id, serialized token response, and a computed `expires_at`.
- storage mode can be `Auto`, `File`, or `Keyring`.
- `Auto` prefers keyring and falls back to `CODEX_HOME/.credentials.json`.
- `OAuthPersistor` talks to rmcp’s `AuthorizationManager`, persists credentials only when they change, and refreshes them when expiry is near.

The `expires_at` design is pragmatic:

- `oauth2::TokenResponse` contains relative expiry (`expires_in`),
- relative durations are hard to persist meaningfully across process restarts,
- so the crate stores an absolute millisecond timestamp and reconstructs `expires_in` on load.

The module is careful to clean up fallback-file entries when a keyring write succeeds, which avoids duplicated storage over time.

### `src/utils.rs`

This module centralizes:

- local stdio environment construction,
- remote stdio environment overlays,
- extraction of remote-source env var names,
- HTTP default header construction from static config and env-backed config,
- reqwest builder header injection.

The environment functions are important because they encode a security boundary:

- local stdio gets a curated base environment plus requested variables,
- remote stdio does not inherit the orchestrator environment except where explicitly intended.

### `src/program_resolver.rs`

This module solves a Windows-only ergonomics issue:

- Unix leaves the program unchanged.
- Windows resolves executables using `which`, including script extensions like `.cmd`.

This is a small module, but it prevents a class of confusing cross-platform launch failures.

## End-to-end flows

### Flow A: stdio MCP client

1. Caller constructs `RmcpClient::new_stdio_client(...)`.
2. The client builds a `TransportRecipe::Stdio`.
3. The selected launcher starts the process and returns `StdioServerTransport`.
4. `initialize(...)` creates `ElicitationClientService`.
5. `rmcp::service::serve_client(...)` performs the handshake and returns `RunningService`.
6. Subsequent tool/resource requests run through `run_service_operation(...)`.

### Flow B: streamable HTTP with bearer auth

1. Caller constructs `RmcpClient::new_streamable_http_client(...)` with a bearer token or `Authorization` header.
2. The client builds default headers and creates a plain `StreamableHttpClientTransport`.
3. `initialize(...)` performs the rmcp handshake over streamable HTTP.
4. Later requests are sent through the same service, with no OAuth runtime attached.

### Flow C: streamable HTTP with stored OAuth

1. Caller constructs `RmcpClient::new_streamable_http_client(...)` without explicit bearer auth.
2. The client loads persisted OAuth tokens, if any.
3. If tokens exist, it builds an `AuthClient` plus `OAuthPersistor`.
4. Requests refresh tokens before use when necessary.
5. After initialize and after operations, new/rotated credentials are persisted.

### Flow D: interactive OAuth login

1. Caller checks auth status or directly starts `perform_oauth_login(...)`.
2. The login flow spins up a callback server and starts rmcp OAuth authorization.
3. The browser is opened or the URL is returned.
4. The callback is received and validated.
5. Credentials are extracted from `OAuthState` and persisted.
6. A later `RmcpClient::new_streamable_http_client(...)` can pick up those saved credentials automatically.

### Flow E: streamable HTTP session recovery

1. A request hits a streamable HTTP session that the server has invalidated.
2. The transport surfaces a synthetic `SessionExpired404`.
3. `run_service_operation(...)` recognizes the error shape.
4. The client serializes recovery via a semaphore.
5. It rebuilds the transport from `TransportRecipe`, reconnects, and reuses the saved initialize context.
6. It retries the failed request once.

This is a targeted recovery mechanism, not a full reconnect framework.

## Dependencies and why they matter

### Core protocol/runtime dependencies

- `rmcp`: the actual MCP protocol/runtime implementation.
- `tokio`: async runtime, process spawning, synchronization primitives.
- `futures`: boxed futures/streams used to erase transport and callback details.
- `serde` / `serde_json`: JSON request/response handling and token persistence.
- `anyhow` / `thiserror`: error propagation and typed local errors.

### HTTP and OAuth

- `reqwest`: streamable HTTP transport and OAuth discovery requests.
- `oauth2`: OAuth login and token handling.
- `sse-stream`: streamable HTTP SSE decoding.
- `webbrowser`: best-effort browser launch for interactive login.
- `tiny_http`: simple local callback server for OAuth redirect handling.

### Codex-specific integration

- `codex-client`: builds reqwest clients with custom CA support.
- `codex-config`: MCP env var and OAuth storage configuration types.
- `codex-exec-server`: remote/executor process launching and process events.
- `codex-keyring-store`: OS keyring abstraction.
- `codex-protocol`: shared auth-status enum type.
- `codex-utils-pty`: Unix process-group cleanup.
- `codex-utils-home-dir`: `CODEX_HOME` discovery for fallback credential storage.

### Test-only dependencies

- `axum`: in-process HTTP test servers for auth discovery and streamable HTTP server binaries.
- `serial_test`: protects env-var-based tests from racing.
- `tempfile`: isolated filesystem state.
- `pretty_assertions`: more readable failures.
- `codex-utils-cargo-bin`: finds compiled test binaries for integration tests.

## Testing strategy

The test coverage is broad and layered.

### Unit tests inside modules

- `src/auth_status.rs`
  - verifies bearer-token detection from static and env-backed headers,
  - verifies OAuth discovery and scope normalization.
- `src/oauth.rs`
  - exercises keyring reads/writes/deletes,
  - validates fallback-file behavior,
  - validates expiry restoration logic.
- `src/utils.rs`
  - verifies local env construction, remote env overlays, and invalid remote-source usage.
- `src/program_resolver.rs`
  - validates platform-specific executable resolution behavior.
- `src/perform_oauth_login.rs`
  - validates callback parsing and authorization URL mutation.
- `src/stdio_server_launcher.rs`
  - validates remote executor env policy construction.
- `src/rmcp_client.rs`
  - validates the active-time timeout behavior around elicitation pauses.
- `src/elicitation_client_service.rs`
  - validates meta restoration and result `_meta` serialization.

### Integration tests in `tests/`

- `tests/resources.rs`
  - verifies end-to-end stdio initialize, list-resources, list-resource-templates, and read-resource flows against a real test MCP server binary.
- `tests/process_group_cleanup.rs`
  - verifies dropping the client kills the full Unix process group, not just the direct child.
- `tests/streamable_http_recovery.rs`
  - verifies targeted recovery semantics for session-expired 404s,
  - verifies 401 and 500 responses do not trigger recovery,
  - verifies recovery retries only once per failed operation.

### Test binaries under `src/bin/`

- `test_stdio_server.rs`: rich stdio MCP server used by integration/manual tests.
- `test_streamable_http_server.rs`: streamable HTTP MCP server with injectable failure behavior and minimal OAuth discovery endpoint.
- `rmcp_test_server.rs`: simpler stdio MCP server variant.

These binaries materially improve confidence because the integration tests exercise real transport and protocol behavior rather than only mocking the `rmcp` layer.

## Design assessment

### What is strong

- Clear separation between lifecycle/orchestration and transport placement.
- Good encapsulation of executor-specific complexity.
- Thoughtful OAuth persistence model with keyring-first fallback.
- Practical timeout-pausing model for interactive elicitation.
- Narrow but useful recovery logic for expired streamable HTTP sessions.
- Strong test strategy with both unit and process-level integration coverage.

### What is intentionally opinionated

- Constructors take many explicit parameters instead of using builders/config structs.
- Recovery is scoped to a specific transport error shape instead of general reconnection logic.
- Auth-status discovery is permissive and user-experience oriented rather than fail-fast.
- Credential persistence is tightly coupled to the rmcp authorization manager lifecycle.

### Potential weaknesses or trade-offs

- `RmcpClient` centralizes many responsibilities, so it is the largest and most complex module.
- Silent warning-based handling of token persistence/refresh failures may hide operational issues.
- The public API is serviceable, but not especially ergonomic for callers that must supply many repeated parameters.
- Recovery logic depends on preserving an `InitializeContext`; if future MCP state becomes more complex, reinitialize-and-retry may be insufficient.

## Open questions

These are not necessarily bugs, but they are useful follow-up questions for maintainers:

1. Should `RmcpClient` expose an explicit `close()` or `shutdown()` API?
   - Today cleanup is mostly drop-driven.
   - That is fine for local ownership, but explicit shutdown could make lifecycle behavior clearer, especially for long-lived streamable-HTTP sessions.

2. Should streamable HTTP recovery handle more cases than 404?
   - The current implementation treats only a very specific 404-with-session case as recoverable.
   - Depending on MCP server behavior, other transport failures may deserve reconnect-and-retry semantics.

3. Should auth discovery distinguish “unsupported” from “temporarily unreachable”?
   - `auth_status` currently collapses discovery errors into `Unsupported`.
   - That keeps UX simple, but it can hide network misconfiguration or proxy issues.

4. Should the constructor/login APIs be refactored into option structs?
   - Several functions already need `#[allow(clippy::too_many_arguments)]`.
   - A small config type would likely improve readability and future extensibility.

5. Is the fallback credential file format intended to be shared long-term across tools?
   - The file is intentionally “consistent with other coding CLI agents”.
   - If multiple tools evolve independently, compatibility and migration policy may need to be made explicit.

6. Is `tiny_http` the desired long-term callback server implementation?
   - It is simple and adequate, but the rest of the crate already uses async infrastructure heavily.
   - A future async callback server might simplify integration and cancellation behavior.

7. Should prompt-related MCP functionality eventually become first-class here?
   - The crate logs prompt-list changes, but the main public API focuses on tools/resources/custom requests and elicitation.
   - That may be enough today, but it leaves prompts as a less visible part of the client surface.

## File map

- `Cargo.toml`: dependency and feature definition
- `src/lib.rs`: public exports
- `src/rmcp_client.rs`: core orchestration and recovery
- `src/stdio_server_launcher.rs`: local/executor stdio launchers
- `src/executor_process_transport.rs`: executor event-stream adapter
- `src/elicitation_client_service.rs`: elicitation request handling
- `src/logging_client_handler.rs`: client-side notification/log handling
- `src/auth_status.rs`: auth capability/status discovery
- `src/perform_oauth_login.rs`: interactive OAuth login flow
- `src/oauth.rs`: credential storage and refresh
- `src/utils.rs`: env/header helpers
- `src/program_resolver.rs`: platform-specific executable resolution
- `tests/resources.rs`: stdio resource integration test
- `tests/process_group_cleanup.rs`: Unix cleanup integration test
- `tests/streamable_http_recovery.rs`: streamable HTTP recovery integration test

## Bottom line

This crate is best understood as a production-oriented adapter around `rmcp`, not merely a protocol client. Its main value is the operational policy it adds:

- launch servers safely,
- keep transport details private,
- bridge MCP elicitation into Codex UI callbacks,
- make streamable HTTP auth usable in practice,
- persist and refresh credentials,
- recover from one important class of session failure,
- and provide enough integration testing to make those policies credible.

If the crate evolves further, the most likely pressure points are API ergonomics, further separation of concerns inside `RmcpClient`, and more explicit policies around recovery and auth error reporting.
