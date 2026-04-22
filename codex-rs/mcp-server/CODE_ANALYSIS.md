# codex-mcp-server code analysis

## Scope

This document analyzes the crate at `/Users/bytedance/project/codex/codex-rs/mcp-server`.
It is based on:

- `Cargo.toml`
- all Rust sources under `src/`
- key integration and support tests under `tests/`
- a local verification run of `cargo test -p codex-mcp-server`, which passed during this analysis

## High-level purpose

`codex-mcp-server` is an MCP-facing adapter around the internal Codex runtime.
It accepts JSON-RPC/MCP messages on stdin, exposes two MCP tools (`codex` and `codex-reply`), starts or resumes Codex threads through `codex-core`, forwards Codex events back to the client as MCP notifications, and turns approval-style Codex events into MCP `elicitation/create` requests.

At a high level, the crate is responsible for:

- running a stdio JSON-RPC server loop
- performing the MCP initialize handshake
- advertising tool schemas for Codex session start and session continuation
- translating MCP tool calls into `codex-core` thread submissions
- streaming Codex runtime events back as `codex/event` notifications
- pausing for user approvals for shell execution and patch application
- correlating MCP request IDs with Codex thread IDs so replies and cancellations can target the right session

## Crate structure

### Entry points

- [`src/main.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/main.rs#L1-L10)
  - binary entrypoint
  - uses `codex_arg0::arg0_dispatch_or_else`
  - calls `run_main(...)` from the library

- [`src/lib.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/lib.rs#L58-L187)
  - owns runtime bootstrapping
  - loads config and OTEL/tracing
  - creates the three-task pipeline: stdin reader, message processor, stdout writer

### Core modules

- [`src/message_processor.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/message_processor.rs#L41-L607)
  - central MCP request/notification dispatcher
  - handles initialization, tools discovery, tool execution, response routing, and cancellation

- [`src/codex_tool_config.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/codex_tool_config.rs#L20-L283)
  - defines the request payloads for `codex` and `codex-reply`
  - builds JSON Schemas exposed through MCP `tools/list`
  - converts tool parameters into `codex_core::config::Config`

- [`src/codex_tool_runner.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/codex_tool_runner.rs#L36-L418)
  - starts or resumes Codex threads
  - submits prompts into the runtime
  - streams Codex events back to the client
  - intercepts approval events and translates them into MCP elicitation flows

- [`src/outgoing_message.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/outgoing_message.rs#L24-L227)
  - wraps outgoing JSON-RPC encoding
  - manages server-initiated request IDs and one-shot callbacks for client responses
  - defines MCP-specific notification metadata

- [`src/exec_approval.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/exec_approval.rs#L17-L147)
  - builds `elicitation/create` payloads for command approvals
  - translates client approval responses back into `Op::ExecApproval`

- [`src/patch_approval.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/patch_approval.rs#L20-L142)
  - builds `elicitation/create` payloads for patch approvals
  - translates client approval responses back into `Op::PatchApproval`

### Tests and test support

- [`tests/suite/codex_tool.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/tests/suite/codex_tool.rs#L1-L524)
  - end-to-end integration tests against a spawned MCP server process and mock Responses API

- [`tests/common/mcp_process.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/tests/common/mcp_process.rs#L35-L399)
  - process harness that speaks JSON-RPC over stdio to the real server binary

- [`tests/common/mock_model_server.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/tests/common/mock_model_server.rs#L1-L47)
  - ordered SSE mock server for `/v1/responses`

- [`tests/common/responses.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/tests/common/responses.rs#L1-L47)
  - helpers for constructing shell-command, apply-patch, and final assistant SSE streams

### Not currently wired into the crate

- [`src/tool_handlers/mod.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/tool_handlers/mod.rs#L1-L2)
  - declares `create_conversation` and `send_message`
  - no module in `src/lib.rs` references `tool_handlers`, so this appears to be leftover or future scaffolding rather than active code

## Concrete responsibilities

### 1. Boot the MCP stdio server

`run_main()` in [`lib.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/lib.rs#L58-L187) performs all startup work:

- creates an `EnvironmentManager` using runtime binary paths from `Arg0DispatchPaths`
- parses CLI config overrides and loads a resolved `codex_core::config::Config`
- configures default residency behavior for login/client creation
- builds optional OTEL providers and installs tracing layers
- creates:
  - a bounded incoming channel for parsed client messages
  - an unbounded outgoing channel for server messages
- spawns:
  - stdin reader task
  - message processor task
  - stdout writer task

This split keeps message parsing, business logic, and output serialization independent.

### 2. Perform the MCP handshake

`MessageProcessor::handle_initialize()` in [`message_processor.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/message_processor.rs#L193-L279):

- enforces single initialization
- captures client name/version and injects it into the global Codex user-agent suffix
- returns server capabilities with `tools.listChanged = true`
- preserves a non-standard `serverInfo.user_agent` field for compatibility with existing Codex clients

The integration harness asserts this exact shape in [`mcp_process.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/tests/common/mcp_process.rs#L113-L197).

### 3. Expose MCP tool schemas

`MessageProcessor::handle_list_tools()` returns two tools:

- `codex`
- `codex-reply`

The definitions are built in [`codex_tool_config.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/codex_tool_config.rs#L109-L283).

The crate uses `schemars` to generate JSON Schema from Rust structs, then strips the serialized schema down to the keys MCP clients most care about (`properties`, `required`, `type`, plus defs fallback).

### 4. Translate tool calls into Codex runtime work

The main flow lives in [`message_processor.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/message_processor.rs#L332-L523):

- `tools/call` dispatches by tool name
- `codex`:
  - parses `CodexToolCallParam`
  - converts it into a concrete Codex `Config`
  - spawns `run_codex_tool_session(...)`
- `codex-reply`:
  - parses `CodexToolCallReplyParam`
  - resolves `threadId` or legacy `conversationId`
  - fetches the existing thread from `ThreadManager`
  - spawns `run_codex_tool_session_reply(...)`

Errors at this layer are returned as normal MCP `CallToolResult` values with `is_error = true`, rather than JSON-RPC transport errors.

### 5. Bridge event streams from Codex to MCP

The runtime bridge is [`codex_tool_runner.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/codex_tool_runner.rs#L59-L418).

For a new session it:

- starts a new thread through `ThreadManager::start_thread`
- emits a synthetic `SessionConfigured` notification immediately
- stores `request_id -> thread_id` in a shared map
- submits the initial user prompt as `Op::UserInput`

For a reply it:

- reuses an existing `CodexThread`
- stores `request_id -> thread_id`
- submits another `Op::UserInput`

Then `run_codex_tool_session_inner()` loops over `thread.next_event().await` and:

- forwards every event as `codex/event`
- enriches notifications with `_meta.requestId` and `_meta.threadId`
- intercepts specific event types for control-flow:
  - `ExecApprovalRequest`
  - `ApplyPatchApprovalRequest`
  - `TurnComplete`
  - `Error`

### 6. Handle approvals through MCP elicitation

The approval bridge is one of the most concrete pieces of MCP-specific behavior.

For shell execution:

- [`exec_approval.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/exec_approval.rs#L17-L147) builds an `elicitation/create` request containing:
  - a human-readable prompt
  - `threadId`
  - Codex correlation metadata
  - the parsed command and cwd
- the server sends that request through `OutgoingMessageSender::send_request()`
- a background task waits for the client response and translates it into `Op::ExecApproval`

For patch application:

- [`patch_approval.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/patch_approval.rs#L20-L142) does the same for file-change approval
- on request failure or malformed response, it explicitly denies the patch, which is a conservative default

### 7. Correlate requests, replies, and cancellations

The shared map `running_requests_id_to_codex_uuid` in [`message_processor.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/message_processor.rs#L41-L47) and [`codex_tool_runner.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/codex_tool_runner.rs#L59-L66) exists to solve a key protocol mismatch:

- MCP cancellation targets a request ID
- Codex interruption targets a thread/submission

`handle_cancelled_notification()` looks up the request ID, resolves the `ThreadId`, fetches the thread, and submits `Op::Interrupt` to Codex, then removes the mapping.

## Public and effective APIs

### Rust-level public API

The library exports only a small surface in [`lib.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/lib.rs#L42-L47):

- `run_main(...)`
- `CodexToolCallParam`
- `CodexToolCallReplyParam`
- `ExecApprovalElicitRequestParams`
- `ExecApprovalResponse`
- `PatchApprovalElicitRequestParams`
- `PatchApprovalResponse`

This suggests the crate is primarily intended to be driven through its binary or reused by integration tests, not as a broad reusable library.

### MCP-level API

The effective protocol surface is:

- `initialize`
- `ping`
- `tools/list`
- `tools/call` for:
  - `codex`
  - `codex-reply`
- notifications from client:
  - `notifications/cancelled`
  - `notifications/initialized`
  - progress/roots changed are accepted but mostly logged
- server-initiated request:
  - `elicitation/create`
- server-initiated notification:
  - `codex/event`

### `codex` tool contract

Defined by [`CodexToolCallParam`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/codex_tool_config.rs#L20-L65).

Important fields:

- `prompt`: required
- `model`, `profile`
- `cwd`
- `approval-policy`
- `sandbox`
- `config`: freeform config overrides
- `base-instructions`
- `developer-instructions`
- `compact-prompt`

Output shape:

- `content`: standard MCP textual content array
- `structuredContent.threadId`
- `structuredContent.content`

The helper that guarantees the thread ID is present is [`create_call_tool_result_with_thread_id`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/codex_tool_runner.rs#L36-L53).

### `codex-reply` tool contract

Defined by [`CodexToolCallReplyParam`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/codex_tool_config.rs#L201-L232).

Important fields:

- `prompt`: required
- `threadId`: preferred session identifier
- `conversationId`: deprecated backward-compatibility alias

## End-to-end flow

### New session flow

1. Client sends `initialize`.
2. Server returns capabilities and server info.
3. Client sends `tools/call` with tool `codex`.
4. `MessageProcessor` parses the tool args and constructs a Codex config.
5. `codex_tool_runner` starts a new thread via `ThreadManager`.
6. Server emits `codex/event` with session metadata.
7. Server submits the initial prompt as `Op::UserInput`.
8. As Codex emits events:
   - most are forwarded as `codex/event`
   - approval events become `elicitation/create`
9. On `TurnComplete`, the server replies to the original MCP tool call with:
   - textual content
   - `structuredContent.threadId`

### Reply flow

1. Client sends `tools/call` with tool `codex-reply`.
2. Server resolves `threadId` / `conversationId`.
3. `ThreadManager` returns the existing thread.
4. Server submits another `Op::UserInput`.
5. Event streaming and completion handling are identical to the new session path.

### Approval flow

1. Codex emits `ExecApprovalRequest` or `ApplyPatchApprovalRequest`.
2. Runner forwards the raw event as `codex/event`.
3. Runner also sends `elicitation/create` to the client.
4. Client responds to the elicitation request.
5. `OutgoingMessageSender` routes the JSON-RPC response back to the waiting one-shot.
6. The approval helper converts the client decision into `Op::ExecApproval` or `Op::PatchApproval`.

### Cancellation flow

1. Client emits `notifications/cancelled` for a request ID.
2. Server resolves the request ID to the corresponding `ThreadId`.
3. Server submits `Op::Interrupt` into the Codex thread.
4. Mapping entry is removed.

## Design observations

### Strong design choices

- **Clear protocol layering**
  - `lib.rs` handles transport/runtime setup
  - `message_processor.rs` handles MCP routing
  - `codex_tool_runner.rs` handles long-running Codex execution
  - approval modules isolate protocol-specific side paths

- **Non-blocking long-running work**
  - `tools/call` handlers spawn background tasks instead of blocking the request-processing loop
  - approval response listeners also run on detached tasks

- **Explicit request correlation**
  - request-to-thread mapping makes cancellation and thread resumption practical over a single MCP connection

- **Schema-as-code**
  - tool schema generation comes from Rust types, reducing drift between runtime parsing and advertised schema

- **Pragmatic compatibility behavior**
  - preserves `serverInfo.user_agent`
  - supports deprecated `conversationId`
  - includes `threadId` in `structuredContent` to help clients resume sessions

### Trade-offs and weaker spots

- **Many MCP methods are stubbed**
  - resources, prompts, completion, logging level changes, and task APIs are mostly logged or rejected
  - this is functional for the current narrow use case, but it is not a broad MCP server implementation

- **Large event match is hard to evolve**
  - `run_codex_tool_session_inner()` has a very large `match` over `EventMsg`
  - it is explicit, but it centralizes many policy decisions in one place

- **Detached background tasks have limited lifecycle management**
  - spawned work is not tracked or joined
  - shutdown relies on channel/process teardown rather than explicit task orchestration

- **Transport semantics and tool semantics are intentionally mixed**
  - invalid tool arguments return a successful JSON-RPC response containing an error `CallToolResult`
  - this is valid MCP behavior, but clients must interpret tool-level errors correctly

## Dependency analysis

### Internal workspace dependencies

From [`Cargo.toml`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/Cargo.toml#L18-L53), the important internal crates are:

- `codex-core`
  - owns threads, runtime events, config loading, and OTEL init
- `codex-protocol`
  - shared event, operation, thread ID, and approval data model
- `codex-exec-server`
  - provides `EnvironmentManager` and runtime path support
- `codex-login`
  - auth manager and default client metadata/user-agent integration
- `codex-models-manager`
  - collaboration mode preset configuration
- `codex-features`
  - feature-flag lookup affecting thread manager setup
- `codex-arg0`
  - executable dispatch/runtime path discovery
- `codex-utils-cli`
  - CLI config overrides
- `codex-utils-json-to-toml`
  - converts JSON tool overrides into TOML-like config values
- `codex-config`
  - config-related types used in tests

### External dependencies

- `rmcp`
  - MCP/JSON-RPC types and protocol model
- `tokio`
  - async runtime, channels, stdio I/O, process handling in tests
- `serde`, `serde_json`
  - payload serialization/deserialization
- `schemars`
  - JSON Schema generation for MCP tool definitions
- `tracing`, `tracing-subscriber`
  - logs and OTEL integration
- `anyhow`
  - ergonomic error handling mostly in binary/tests
- `shlex`
  - safe-ish shell command rendering for approval prompts

## Testing strategy

### Unit tests in `src/`

The crate includes focused unit tests for:

- OTEL default and provider construction in [`lib.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/lib.rs#L189-L238)
- schema stability for `codex` and `codex-reply` in [`codex_tool_config.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/codex_tool_config.rs#L285-L434)
- `structuredContent.threadId` behavior in [`codex_tool_runner.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/codex_tool_runner.rs#L420-L440)
- outgoing request/notification serialization and `_meta` handling in [`outgoing_message.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/outgoing_message.rs#L229-L472)

These tests are good for guarding protocol-shape regressions.

### Integration tests

The integration suite in [`tests/suite/codex_tool.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/tests/suite/codex_tool.rs#L36-L442) validates the most important end-to-end behaviors:

- command approval becomes `elicitation/create`, and an approved response executes the command
- patch approval becomes `elicitation/create`, and an approved response applies the patch
- base instructions and developer instructions reach the upstream Responses request

The test harness is realistic:

- it starts the actual compiled `codex-mcp-server` binary
- it speaks real JSON-RPC over stdin/stdout
- it uses a sequential SSE mock server to mimic model responses

### What is well covered

- initialization response shape
- tool result shape
- approval request payloads
- patch and shell approval round-trips
- instruction propagation into upstream model requests

### What is lightly covered or not covered

- cancellation flow via `notifications/cancelled`
- `codex-reply` happy path
- error paths in approval response deserialization
- behavior for ignored event types and raw event forwarding
- unsupported MCP methods and custom requests
- concurrency edge cases when multiple tool calls are active at once

## Notable implementation details

### Message transport model

The server uses:

- bounded channel for inbound messages
- unbounded channel for outbound messages

That choice implies:

- inbound backpressure is intentional
- outbound messages are assumed to remain bounded in practice for an interactive client

### Notification enrichment

`OutgoingNotificationMeta` in [`outgoing_message.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/outgoing_message.rs#L194-L215) adds:

- `requestId`
- `threadId`

to `_meta`, which is useful because MCP itself does not inherently model Codex thread multiplexing.

### Conservative defaults on patch approval

`patch_approval.rs` denies patch application if:

- the client response channel fails
- the response cannot be deserialized

That is a good safety property.

### Less conservative exec approval failure handling

`exec_approval.rs` denies on malformed payloads, but if the oneshot response channel fails it logs and returns without explicitly sending a deny op.
That creates an asymmetry with patch approval and may leave the runtime waiting, depending on Codex-core behavior.

## Open questions

1. **Should exec approval mirror patch approval on client disconnect/failure?**
   - [`exec_approval.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/exec_approval.rs#L118-L146) logs and exits when the response channel fails.
   - [`patch_approval.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/patch_approval.rs#L108-L141) explicitly submits a denied approval.
   - The mismatch suggests either a bug or an intentional but undocumented difference.

2. **Should generic `EventMsg::ElicitationRequest` be forwarded to the client?**
   - [`codex_tool_runner.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/codex_tool_runner.rs#L270-L273) has an explicit TODO.
   - Today only exec and patch approval elicitation are bridged.

3. **Which streamed event types should receive richer MCP treatment?**
   - agent message deltas, reasoning deltas, and agent messages currently have TODO comments in [`codex_tool_runner.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/codex_tool_runner.rs#L320-L330).
   - Right now most events are only forwarded as raw `codex/event` notifications.

4. **Should task APIs be implemented or removed from the advertised surface expectations?**
   - task-related requests are hard rejected in [`message_processor.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/message_processor.rs#L128-L143).
   - If clients begin expecting MCP task semantics, this server will not satisfy them.

5. **How intentional is the legacy `task_complete` compatibility path?**
   - integration tests still wait for a legacy `task_complete` notification in [`codex_tool.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/tests/suite/codex_tool.rs#L148-L156) and [`mcp_process.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/tests/common/mcp_process.rs#L327-L369).
   - The current runner primarily keys on `TurnComplete`, so the long-term event contract is worth clarifying.

6. **Is `src/tool_handlers/` dead code or planned future work?**
   - [`src/tool_handlers/mod.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/tool_handlers/mod.rs#L1-L2) is not wired into the crate.

7. **Should `main.rs` expose CLI overrides explicitly?**
   - `run_main()` accepts `CliConfigOverrides`, but [`main.rs`](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/main.rs#L6-L10) always passes `CliConfigOverrides::default()`.
   - That suggests the library is more configurable than the binary currently is.

## Overall assessment

This crate is a focused MCP adapter rather than a general-purpose MCP server.
Its core path is clear and reasonably well tested:

- initialize over stdio
- advertise two tools
- start or resume a Codex thread
- forward runtime events
- pause for approvals through MCP elicitation
- return a tool result containing `threadId`

The implementation is strongest where it bridges concrete Codex product behavior into MCP.
The main gaps are breadth of MCP feature support, some unfinished event modeling decisions, and a few consistency questions around approval failure handling and legacy compatibility.
