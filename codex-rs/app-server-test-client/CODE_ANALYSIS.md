# CODE_ANALYSIS

## Overview

`codex-app-server-test-client` is a small Rust CLI crate that exercises `codex app-server` through either:

- a private stdio child process spawned from a `codex` binary, or
- a shared websocket endpoint, usually `ws://127.0.0.1:4222`.

The crate is not a production SDK. It is a developer-facing harness for:

- bootstrapping the app-server,
- sending JSON-RPC requests for both legacy and V2 thread/turn flows,
- observing notifications and streaming output,
- auto-answering approval callbacks,
- testing login, model, thread-list, and elicitation behaviors,
- and capturing trace context for debugging.

The crate is intentionally concentrated in a single source file, with `src/main.rs` only creating a single-threaded Tokio runtime and calling `run()`.

## Package and Build Role

### Cargo

`Cargo.toml` defines the crate as `codex-app-server-test-client` and keeps version, edition, license, and lint policy aligned with the workspace.

Key direct dependencies:

- `clap`: CLI parsing for a large subcommand surface.
- `tokio`: only the runtime is used; most I/O is synchronous/blocking.
- `tungstenite` and `url`: websocket transport.
- `serde` / `serde_json`: JSON-RPC serialization.
- `uuid`: request id generation.
- `anyhow`: error propagation with context.
- `tracing` and `tracing-subscriber`: client-side request spans.
- `codex-app-server-protocol`: the core protocol contract for requests, responses, notifications, approval schemas, thread/turn APIs, and JSON-RPC wrappers.
- `codex-core`, `codex-otel`, `codex-protocol`, `codex-utils-cli`: config loading, OpenTelemetry initialization, reasoning effort types, trace propagation, and CLI config overrides.

### Bazel

`BUILD.bazel` wraps the crate as `app-server-test-client` through the repo’s `codex_rust_crate` macro. There is no crate-local custom Bazel logic.

## File Structure

- `src/main.rs`: minimal runtime bootstrap.
- `src/lib.rs`: essentially the whole implementation.
- `scripts/live_elicitation_hold.sh`: helper used by the elicitation timeout harness.
- `README.md`: manual quickstart for common flows.
- `CODE_ANALYSIS.md`: this analysis document.

There are no crate-local `tests/`, `benches/`, or additional modules under `src/`.

## Primary Responsibilities

### 1. CLI entrypoint and command dispatch

The `Cli` and `CliCommand` types define a test-oriented command surface that covers:

- app-server lifecycle (`serve`),
- raw observation (`watch`, `thread-resume`),
- chat flows (`send-message`, `send-message-v2`, `resume-message-v2`, `send-follow-up-v2`),
- approval scenarios (`trigger-cmd-approval`, `trigger-patch-approval`, `no-trigger-cmd-approval`, `trigger-zsh-fork-multi-cmd-approval`),
- account and discovery operations (`test-login`, `get-account-rate-limits`, `model-list`, `thread-list`),
- explicit elicitation control (`thread-increment-elicitation`, `thread-decrement-elicitation`),
- and a full timeout regression harness (`live-elicitation-timeout-pause`).

`run()` parses CLI arguments, decodes optional dynamic tool specs, resolves the transport mode, and dispatches to the appropriate flow.

### 2. Transport abstraction over stdio and websocket

`Endpoint` chooses whether a command talks to:

- `SpawnCodex(PathBuf)`: launch `codex app-server` as a private child process using stdio, or
- `ConnectWs(String)`: connect to an already running websocket server.

`ClientTransport` then implements the concrete transport:

- stdio uses line-oriented JSON payloads over child stdin/stdout,
- websocket uses `tungstenite` text frames.

This lets most request/notification logic stay transport-agnostic.

### 3. JSON-RPC client behavior

`CodexClient` is the real center of the crate. It:

- serializes typed protocol requests into JSON-RPC,
- injects W3C trace context into outgoing requests,
- waits for matching responses by request id,
- buffers notifications that arrive while a request is pending,
- handles server-initiated approval requests,
- and continuously streams turn notifications for human-readable test output.

The crate leans heavily on `codex-app-server-protocol` types rather than manually shaping JSON.

### 4. Approval automation and harnessing

The client automatically handles two server request categories:

- command execution approval requests,
- file change approval requests.

Command approvals are programmable through `CommandApprovalBehavior`:

- `AlwaysAccept`,
- `AbortOn(index)`, which cancels a chosen approval callback to test partial rejection flows.

This is specifically used by the multi-approval zsh-fork scenario to assert that repeated approval callbacks occur and that mixed accept/cancel flows produce the expected completion state.

### 5. Elicitation timeout regression testing

The most specialized behavior in the crate is `live_elicitation_timeout_pause()`. It creates a real end-to-end scenario proving that a paused thread can keep a long-running command alive beyond a nominal 10-second unified execution timeout.

That scenario depends on:

- starting or attaching to an app-server,
- starting a V2 thread and turn,
- instructing the model to execute a very precise shell command,
- using `scripts/live_elicitation_hold.sh` to increment/decrement the thread’s elicitation pause counter,
- and validating from streamed events and elapsed wall-clock time that the command completed only after the helper script finished.

## Public and Practical APIs

### Public Rust API

The crate exposes two practical library entrypoints:

- `pub async fn run() -> Result<()>`
- `pub async fn send_message_v2(...) -> Result<()>`

`send_message_v2()` is also called from the main `codex` CLI’s debug command path, so this crate is not only runnable as a binary; it also serves as a reusable debug helper inside the workspace.

### Important internal methods

The rest of the useful surface is internal but structurally important:

- connection setup: `resolve_endpoint()`, `resolve_shared_websocket_url()`, `BackgroundAppServer::spawn()`
- command execution flows: `send_message_v2_with_policies()`, `resume_message_v2()`, `send_follow_up_v2()`
- inspection flows: `watch()`, `thread_resume_follow()`
- account/discovery flows: `test_login()`, `get_account_rate_limits()`, `model_list()`, `thread_list()`
- elicitation helpers: `thread_increment_elicitation()`, `thread_decrement_elicitation()`, `live_elicitation_timeout_pause()`
- client internals: `initialize_with_experimental_api()`, `send_request()`, `wait_for_response()`, `stream_turn()`, `handle_server_request()`

## End-to-End Flow

### Standard V2 message flow

The main V2 path is:

1. parse CLI options,
2. resolve transport via `--codex-bin` or `--url`,
3. initialize tracing and connect a `CodexClient`,
4. send `initialize`,
5. send `initialized` notification,
6. send `thread/start`,
7. send `turn/start`,
8. loop on notifications until `turn/completed`,
9. print a Datadog trace summary.

`send_message()` is now effectively a thin wrapper over the V2 flow with default policies and no dynamic tools.

### Response/notification handling

The client has a clean split between request/response synchronization and streaming:

- `wait_for_response()` blocks until it sees a matching response id.
- Notifications arriving during that wait are queued in `pending_notifications`.
- Later streaming calls consume those queued notifications first via `next_notification()`.

That avoids losing early turn events that arrive before the request/response phase is fully complete.

### Server request handling

When the server sends a JSON-RPC request instead of a notification or response, `handle_server_request()` dispatches it into typed approval handlers. Unsupported request kinds are treated as fatal. This is sensible for a narrow test harness because it fails loudly when the protocol expands unexpectedly.

## Command Groups and What They Validate

### Lifecycle and inspection

- `serve`: launches a websocket app-server in the background via `nohup`, writes logs to `/tmp/codex-app-server-test-client/app-server.log`, and can kill an existing listener on the same port first.
- `watch`: initializes and then prints inbound traffic indefinitely.
- `thread-resume`: rejoins an existing thread and streams notifications without starting a new turn.

### Chat and thread/turn flows

- `send-message`: compatibility wrapper that still routes through the V2 machinery.
- `send-message-v2`: starts a new thread/turn and optionally includes dynamic tools when experimental API is enabled.
- `resume-message-v2`: resumes an existing thread before starting a new turn.
- `send-follow-up-v2`: sends two sequential turns in the same thread to verify follow-up behavior.

### Approval behavior

- `trigger-cmd-approval`: chooses policies that should force a command execution approval.
- `trigger-patch-approval`: chooses policies that should force an apply-patch/file-change approval.
- `no-trigger-cmd-approval`: sanity-checks that a similar command does not require approval under default policies.
- `trigger-zsh-fork-multi-cmd-approval`: validates repeated approval requests against one command item and optionally aborts a chosen approval index.

### Account, discovery, and metadata

- `test-login`: exercises either browser or device-code ChatGPT login.
- `get-account-rate-limits`: fetches current rate limits.
- `model-list`: lists available models.
- `thread-list`: lists stored threads.

### Explicit elicitation control

- `thread-increment-elicitation` / `thread-decrement-elicitation`: send direct out-of-band requests against an existing websocket app-server.
- `live-elicitation-timeout-pause`: full regression harness around timeout pausing.

## Dynamic Tools Handling

Dynamic tools are intentionally narrow in scope:

- `--dynamic-tools` accepts inline JSON or `@file` content.
- the JSON may be either a single tool object or an array of tool objects.
- dynamic tools are only allowed for V2 thread-start flows.
- `send-message-v2` additionally requires `--experimental-api` when dynamic tools are present.

This is enforced centrally through `parse_dynamic_tools_arg()` and `ensure_dynamic_tools_unused()`.

## Tracing and Observability Design

The crate has stronger observability than a typical test client:

- `with_client()` initializes OpenTelemetry based on workspace config overrides.
- each top-level command gets its own span,
- each RPC call gets its own request span,
- outgoing requests include current W3C trace context,
- and the tool prints a Datadog trace URL when tracing is enabled.

That makes the crate useful not just for protocol testing but also for correlating client actions with backend traces.

One important implementation detail: tracing setup uses `tracing_subscriber::registry().try_init()`. This means repeated initialization attempts are intentionally tolerated, which fits a reusable helper inside a larger process.

## Testing Strategy Present in This Crate

There are no Rust unit tests or integration tests inside this crate.

Instead, testing is embedded in command behavior:

- commands assert expectations at runtime and fail with `bail!` when invariants are not met,
- approval flows count callbacks and validate final statuses,
- the elicitation timeout harness validates both event ordering and elapsed time,
- the README documents manual test procedures for background-server, model-list, thread-list, resume, and watch flows.

So this crate is best understood as an executable test harness rather than a conventionally test-covered library.

## Notable Implementation Details

### Single-file architecture

Almost everything lives in `src/lib.rs`. That keeps ad hoc debugging easy, but it also means:

- protocol logic,
- CLI parsing,
- transport details,
- validation logic,
- tracing,
- and scenario-specific assertions

all evolve in one file. This is efficient for an internal tool, but the file is already large enough that future maintenance will get harder.

### Mostly synchronous I/O inside an async shell

Although the binary runs inside Tokio, the core I/O is synchronous:

- stdio uses blocking `BufRead::read_line`,
- websocket reads use blocking `tungstenite`,
- child process and port-killing operations use blocking `std::process::Command`.

This is acceptable because the tool is essentially single-session and CLI-bound, but it is not designed for high concurrency or embedding in a latency-sensitive async service.

### Strong protocol typing

The crate uses protocol enums and structs from `codex-app-server-protocol` almost everywhere. That reduces JSON drift and keeps changes aligned with the main app-server contract.

### Defensive cleanup

Two `Drop` implementations matter:

- `BackgroundAppServer` kills the spawned websocket app-server when the harness exits.
- `CodexClient` closes stdio and waits up to five seconds for the child app-server to exit gracefully before force-killing it.

The shell helper script also traps exit signals and tries to decrement elicitation on cleanup.

## External Dependency Roles

### Workspace crates

- `codex-app-server-protocol`: request/response/notification contract and approval schemas.
- `codex-core`: config loading and OTEL initialization helpers.
- `codex-otel`: current trace context extraction and OTEL provider wrapper.
- `codex-protocol`: shared protocol types such as reasoning effort and W3C trace context.
- `codex-utils-cli`: parsing of repeated `--config key=value` overrides.

### Third-party crates

- `clap`: declarative CLI shape.
- `anyhow`: error layering.
- `serde`, `serde_json`: typed JSON encoding/decoding.
- `tungstenite`, `url`: websocket transport and URL parsing.
- `uuid`: unique JSON-RPC request ids.
- `tracing`, `tracing-subscriber`: structured spans and export.
- `tokio`: runtime bootstrap only.

## Operational Assumptions and Constraints

- Default websocket target is `ws://127.0.0.1:4222`.
- Several flows assume POSIX tooling such as `nohup`, `sh`, `lsof`, `kill`, and `tail`.
- `live-elicitation-timeout-pause` explicitly rejects Windows.
- `serve --kill` is macOS/Linux oriented because it uses `lsof` and Unix signals.
- stdio-spawned app-servers are private per invocation; explicit elicitation increment/decrement commands require a shared websocket server instead.

## Design Assessment

### Strengths

- Strongly typed protocol integration.
- Dual transport support without much duplicated business logic.
- Excellent observability for a test harness.
- Rich scenario coverage for approvals, thread resumption, login, and elicitation pause semantics.
- Loud runtime assertions that make regressions obvious during manual or scripted use.

### Tradeoffs

- Minimal separation of concerns because almost all code is in one file.
- Limited portability due to shell- and Unix-specific utilities.
- No formal automated test suite despite the crate’s testing purpose.
- Heavy reliance on `println!` for output, which is perfect for humans but awkward for machine parsing.
- Blocking transport operations inside an async entrypoint.

## Open Questions

1. Should the crate stay a monolithic harness, or should `CodexClient`, approval handlers, and scenario runners split into modules as the protocol surface grows?
2. Is `send-message` intentionally a V2 wrapper now, or is it carrying legacy naming that may mislead users about the API path?
3. Should unsupported server request kinds remain fatal, or should the client log-and-ignore unknown request types to stay forward-compatible with protocol expansion?
4. Is there value in promoting the strongest scenario checks, especially the elicitation timeout harness, into automated workspace integration tests rather than manual CLI commands?
5. Should websocket and stdio I/O eventually move to async implementations if this crate gets embedded more deeply into other debug tooling?
6. Are dynamic tools intentionally unsupported for resumed threads and watcher-style commands, or is that just the current minimal scope?
7. Should the Datadog trace URL format remain hardcoded as `go/trace/<trace_id>`, or should it be derived from config to support multiple telemetry backends?

## Most Important Source Anchors

- `src/main.rs`: Tokio bootstrap only.
- `src/lib.rs`: all CLI, transport, protocol, tracing, and harness logic.
- `scripts/live_elicitation_hold.sh`: helper script that manipulates elicitation pause state during the timeout regression test.

For a new reader, the most useful reading order is:

1. `run()` and `CliCommand`,
2. `send_message_v2_with_policies()`,
3. `CodexClient`,
4. `stream_turn()`,
5. `live_elicitation_timeout_pause()`,
6. `scripts/live_elicitation_hold.sh`.
