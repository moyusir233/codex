# codex-code-mode crate analysis

## Overview

`codex-code-mode` is a focused runtime crate that executes model-authored JavaScript inside an embedded V8 isolate and lets that JavaScript orchestrate other tools through a controlled host bridge.

At a high level, the crate splits into four concerns:

1. **Prompt and tool description generation** in [description.rs](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/description.rs)
2. **V8 runtime setup and JS helper plumbing** in [runtime/mod.rs](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/runtime/mod.rs) and its helper modules
3. **Session lifecycle, polling, and termination orchestration** in [service.rs](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/service.rs)
4. **Output item modeling** in [response.rs](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/response.rs)

The crate does not try to be a full JavaScript environment. It intentionally provides:

- a fresh isolate per `exec`
- no Node APIs
- no filesystem or network primitives
- no `console` global
- no arbitrary imports
- a narrow set of host callbacks: `tools.*`, `text`, `image`, `store`, `load`, `notify`, `setTimeout`, `clearTimeout`, `yield_control`, `exit`

## Crate shape

- Manifest: [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/code-mode/Cargo.toml)
- Public re-exports: [lib.rs](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/lib.rs)
- Main runtime/session surface:
  - [CodeModeService](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/service.rs#L52-L208)
  - [ExecuteRequest / WaitRequest / RuntimeResponse](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/runtime/mod.rs#L26-L59)
- Prompt-description helpers:
  - [parse_exec_source](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/description.rs#L164-L246)
  - [build_exec_tool_description](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/description.rs#L252-L322)
  - [build_wait_tool_description](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/description.rs#L324-L326)

## Concrete responsibilities

### 1. Public API and types

[lib.rs](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/lib.rs#L1-L32) exposes the crate as a small integration surface:

- `CodeModeService` manages execution sessions and `wait`
- `CodeModeTurnHost` abstracts the outer host that can run nested tools and handle notifications
- `ExecuteRequest`, `WaitRequest`, and `RuntimeResponse` define the protocol between the outer app and the runtime
- description helpers generate the `exec`/`wait` tool descriptions and normalize nested tool metadata
- response types define output items and image detail levels

The crate intentionally re-exports almost everything a higher-level integration layer needs, which keeps `core` from touching runtime internals directly.

### 2. Description and prompt construction

[description.rs](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/description.rs) is responsible for all user/model-facing tool guidance.

Key responsibilities:

- Parse the optional first-line pragma `// @exec: {...}` with validation in [parse_exec_source](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/description.rs#L164-L246)
- Render the base `exec` contract and helper inventory in [EXEC_DESCRIPTION_TEMPLATE](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/description.rs#L14-L36)
- Render the `wait` contract in [WAIT_DESCRIPTION_TEMPLATE](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/description.rs#L37-L44)
- Turn nested tool schemas into TypeScript declarations through [augment_tool_definition](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/description.rs#L352-L357), [render_code_mode_sample](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/description.rs#L376-L388), and [render_json_schema_to_typescript](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/description.rs#L446-L707)
- Group namespace guidance once per namespace in [build_exec_tool_description](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/description.rs#L252-L322)
- Detect MCP-style `CallToolResult` schemas and emit a shared TypeScript preamble via [mcp_structured_content_schema](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/description.rs#L450-L489)

This module is more than string templating. It encodes the contract that the model sees, so it directly influences how safely and effectively `exec` is used.

### 3. Runtime execution engine

[runtime/mod.rs](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/runtime/mod.rs) owns the isolate lifecycle and runtime event loop.

Key responsibilities:

- Initialize V8 and ICU once in [initialize_v8](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/runtime/mod.rs#L170-L184)
- Spawn one OS thread per `exec` in [spawn_runtime](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/runtime/mod.rs#L104-L139)
- Install globals and runtime state in [run_runtime](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/runtime/mod.rs#L186-L294)
- Receive host commands (`ToolResponse`, `ToolError`, `TimeoutFired`, `Terminate`) and drive microtask checkpoints
- Emit `RuntimeEvent`s back to the async service layer
- Collect final `stored_values` and any terminal error into a `RuntimeEvent::Result`

Important runtime design choices:

- The isolate lives on a dedicated thread, while the service runs on Tokio
- Cross-thread commands use `std::sync::mpsc`, while service coordination uses Tokio channels
- The runtime treats top-level JS as an async module, so `await` works naturally
- Termination uses both `RuntimeCommand::Terminate` and `v8::IsolateHandle::terminate_execution()`

### 4. Global helpers and host bridge

The runtime helper modules provide the JS-visible API:

- [globals.rs](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/runtime/globals.rs) installs globals, removes `console`, builds `tools`, and publishes `ALL_TOOLS`
- [callbacks.rs](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/runtime/callbacks.rs) implements the callable bridge functions
- [module_loader.rs](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/runtime/module_loader.rs) compiles/evaluates the main module, forbids imports, resolves tool promises, and interprets `exit()`
- [timers.rs](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/runtime/timers.rs) implements `setTimeout` / `clearTimeout`
- [value.rs](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/runtime/value.rs) handles V8/JSON conversion, text serialization, error extraction, and image normalization

The most important callback paths are:

- `tool_callback`: converts the first JS argument to JSON, creates a promise, stores its resolver, and emits `RuntimeEvent::ToolCall` ([callbacks.rs:L13-L75](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/runtime/callbacks.rs#L13-L75))
- `text_callback` / `image_callback`: append output items into the event stream ([callbacks.rs:L77-L133](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/runtime/callbacks.rs#L77-L133))
- `store_callback` / `load_callback`: mutate or read serialized cross-exec values ([callbacks.rs:L135-L192](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/runtime/callbacks.rs#L135-L192))
- `notify_callback`: emits an out-of-band notify event to the host ([callbacks.rs:L194-L222](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/runtime/callbacks.rs#L194-L222))
- `yield_control_callback`: asks the service to flush output without ending execution ([callbacks.rs:L253-L261](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/runtime/callbacks.rs#L253-L261))
- `exit_callback`: marks `exit_requested` and throws a sentinel exception handled as a successful exit ([callbacks.rs:L263-L274](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/runtime/callbacks.rs#L263-L274))

### 5. Session control and wait semantics

[service.rs](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/service.rs) is the async orchestration layer on top of the runtime thread.

Key responsibilities:

- Maintain active cells in `sessions` ([service.rs:L44-L54](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/service.rs#L44-L54))
- Create a new cell id and runtime on `execute` ([service.rs:L79-L114](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/service.rs#L79-L114))
- Route `wait` calls to the correct active session or return a synthetic “cell not found” result ([service.rs:L116-L144](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/service.rs#L116-L144))
- Start a turn worker that dispatches tool calls and notifications to the outer host ([service.rs:L146-L207](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/service.rs#L146-L207))
- Run `run_session_control`, which merges runtime events, wait requests, yield timers, buffering, and termination ([service.rs:L283-L462](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/service.rs#L283-L462))

`run_session_control` is the core control-plane state machine. It:

- accumulates `content_items`
- starts a yield timer after `RuntimeEvent::Started`
- flushes output on timer expiry or `YieldRequested`
- forwards tool calls and notifications into the `turn_message` queue
- buffers final results if they arrive before the next `wait`
- delays the termination response until the runtime is actually closed
- always removes the cell from `sessions` on exit

## Main APIs

### Execute path

The outer layer calls [CodeModeService::execute](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/service.rs#L79-L114) with:

- `tool_call_id`: used by `notify`
- `enabled_tools`: nested tools exposed on `globalThis.tools`
- `source`: raw JavaScript
- `stored_values`: serialized session values passed into the new isolate
- `yield_time_ms`: optional early-yield timeout
- `max_output_tokens`: accepted by the request type, but not enforced inside this crate

It returns one of:

- `RuntimeResponse::Yielded`
- `RuntimeResponse::Terminated`
- `RuntimeResponse::Result`

### Wait path

The outer layer calls [CodeModeService::wait](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/service.rs#L116-L144) with:

- `cell_id`
- `yield_time_ms`
- `terminate`

`wait` does not itself interpret runtime events; it sends a control command into the per-cell session controller and awaits a one-shot response.

### Host path

The outer application must implement [CodeModeTurnHost](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/service.rs#L26-L36):

- `invoke_tool(...) -> Result<JsonValue, String>`
- `notify(...) -> Result<(), String>`

This is the only host-specific abstraction inside the crate. Everything else is generic runtime/session machinery.

## End-to-end execution flow

### `exec`

1. Outer layer parses the user/model code via [parse_exec_source](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/description.rs#L164-L246).
2. Outer layer builds enabled nested tool definitions and passes them in `ExecuteRequest`.
3. [CodeModeService::execute](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/service.rs#L79-L114) allocates a `cell_id`, spawns the runtime, registers the session, and spawns `run_session_control`.
4. [spawn_runtime](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/runtime/mod.rs#L104-L139) creates a V8 thread and returns a command sender plus an isolate termination handle.
5. [run_runtime](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/runtime/mod.rs#L186-L294) installs globals and evaluates the JS module.
6. JS helper calls emit `RuntimeEvent`s:
   - `text` / `image`
   - `ToolCall`
   - `Notify`
   - `YieldRequested`
   - final `Result`
7. [run_session_control](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/service.rs#L283-L462) either:
   - returns a `Yielded` response immediately, or
   - returns a final `Result`, or
   - waits for a later `wait`

### Nested tool calls

1. JS calls `await tools.some_tool(args)`.
2. [tool_callback](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/runtime/callbacks.rs#L13-L75) creates a pending promise resolver and emits `RuntimeEvent::ToolCall`.
3. The session controller forwards that to the turn worker through `TurnMessage::ToolCall`.
4. [start_turn_worker](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/service.rs#L146-L207) invokes the host implementation asynchronously.
5. The worker sends `RuntimeCommand::ToolResponse` or `RuntimeCommand::ToolError` back into the runtime thread.
6. [resolve_tool_response](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/runtime/module_loader.rs#L66-L101) resolves or rejects the original JS promise.

### `wait`

1. Outer layer calls `wait(cell_id, ...)`.
2. [CodeModeService::wait](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/service.rs#L116-L144) finds the session and sends either `Poll` or `Terminate`.
3. `run_session_control`:
   - returns a buffered final result immediately if one already exists
   - resets the yield timer for polling
   - or initiates shutdown for termination
4. If termination is requested, the controller waits until the runtime actually closes before replying with `RuntimeResponse::Terminated`.

## Dependencies and why they exist

From [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/code-mode/Cargo.toml#L15-L28):

- `v8`: embedded JavaScript engine and isolate API
- `deno_core_icudata`: ICU data so locale/date APIs behave correctly in V8
- `tokio`: async coordination, channels, timers, tests
- `tokio-util`: `CancellationToken` in the host API
- `async-channel`: queue between session control and turn worker
- `async-trait`: async trait methods on `CodeModeTurnHost`
- `serde` / `serde_json`: JSON schema handling, request parsing, JS value interchange
- `tracing`: warnings on host notification failures
- `codex-protocol`: shared `ToolName` type and tool metadata identity
- `pretty_assertions` (dev): readable test diffs

## Design observations

### Clear separations

- `description.rs` is pure contract generation and schema rendering
- `runtime/*` is V8-specific execution machinery
- `service.rs` is async orchestration and session control
- `response.rs` is a tiny serialization boundary for output items

That separation is clean and keeps most unsafe/FFI-adjacent concerns localized to the runtime modules.

### Strongly constrained JS environment

The environment is intentionally narrow:

- `console` is explicitly deleted in [install_globals](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/runtime/globals.rs#L13-L19)
- imports are always rejected in [resolve_module](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/runtime/module_loader.rs#L223-L235)
- tool interaction is promise-based and routed through host mediation

This reduces surface area and makes the runtime more like a deterministic orchestration sandbox than a general JS shell.

### Event-driven bridging

The crate models cross-boundary interaction as:

- runtime emits `RuntimeEvent`
- host/service emits `RuntimeCommand`

That makes the async control flow explicit and debuggable. It also lets `wait` work naturally without suspending the runtime thread itself.

### Store/load is isolate-local per exec but session-persistent at the integration boundary

Inside this crate, `stored_values` are simply injected into `ExecuteRequest` and returned in `RuntimeResponse::Result`.

The actual persistence across `exec` calls is handled outside the crate in the core integration:

- read before `exec`: [execute_handler.rs:L27-L45](file:///Users/bytedance/project/codex/codex-rs/core/src/tools/code_mode/execute_handler.rs#L27-L45)
- write after final result: [core code_mode mod.rs:L177-L203](file:///Users/bytedance/project/codex/codex-rs/core/src/tools/code_mode/mod.rs#L177-L203)

This is an important architectural boundary: the crate transports state, but the outer layer owns persistence policy.

### Output truncation is not enforced here

`ExecuteRequest.max_output_tokens` exists in the crate API, and the description advertises output budgeting, but truncation is applied in the outer core layer rather than inside this crate:

- result truncation: [core code_mode mod.rs:L236-L252](file:///Users/bytedance/project/codex/codex-rs/core/src/tools/code_mode/mod.rs#L236-L252)
- `wait` max token parsing: [wait_handler.rs:L17-L30](file:///Users/bytedance/project/codex/codex-rs/core/src/tools/code_mode/wait_handler.rs#L17-L30)

So the crate is a raw runtime producer, not the final presentation layer.

## Testing coverage

All current tests passed locally with:

```bash
cargo test -p codex-code-mode
```

The crate currently has 22 unit tests across three modules.

### Description tests

[description.rs tests](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/description.rs#L711-L1101) cover:

- pragma parsing
- identifier normalization
- typed declaration rendering
- schema property descriptions as comments
- nested-tool inclusion in generated prompt text
- timeout helper mention in docs
- namespace grouping behavior
- shared MCP type preamble deduplication
- deferred-tool guidance text

### Runtime test

[runtime/mod.rs tests](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/runtime/mod.rs#L320-L378) cover:

- termination of a CPU-bound infinite loop using the V8 isolate termination handle

This is an important safety test because it validates that runaway JS can be stopped.

### Service tests

[service.rs tests](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/service.rs#L464-L898) cover:

- `exit()` as a successful early return
- `console` removal
- ICU-backed locale formatting for `Date` and `Intl.DateTimeFormat`
- helper return values being `undefined`
- image helper acceptance and validation paths
- termination waiting for actual runtime shutdown before replying

These tests are practical and behavior-focused rather than purely structural, which is a good fit for this crate.

## Notable strengths

- Narrow public surface and clear internal layering
- Good behavioral tests around user-visible semantics
- Clean host/runtime handshake for nested tool calls
- Strongly constrained sandbox model
- Proper ICU initialization, which avoids subtle locale regressions
- Robust termination path for both cooperative and CPU-bound code

## Open questions and follow-ups

1. **Should `max_output_tokens` move out of this crate API or be enforced here?**  
   The crate accepts it in `ExecuteRequest`, and docs promise budgeting, but enforcement happens in `core`, not in this crate. That is workable, but the contract is split across layers.

2. **Should `CodeModeService` own stored-value persistence directly?**  
   `service.rs` keeps an `inner.stored_values` map with getters/setters, but `execute` uses `request.stored_values` rather than that internal map. The current design is consistent with outer-layer ownership, but the duplicated state surface can be confusing.

3. **Should nested-tool cancellation be wired through real session termination?**  
   `start_turn_worker` currently passes `CancellationToken::new()` for each host tool call in [service.rs:L180-L183](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/service.rs#L180-L183). That means terminating the JS runtime does not obviously cancel already-running host tool invocations.

4. **Do timer threads need tighter lifecycle control?**  
   [timers.rs](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/runtime/timers.rs#L39-L42) spawns one OS thread per timeout. For expected usage this may be fine, but heavy timer use could become more expensive than necessary.

5. **Should there be more tests around `store/load`, nested tool resolution, and `yield_control()`?**  
   Those are central features, but current tests emphasize images, termination, and description generation more heavily than multi-step orchestration scenarios.

6. **Is `notify` delivery failure policy correct?**  
   Notification failures are logged and dropped in [service.rs:L166-L170](file:///Users/bytedance/project/codex/codex-rs/code-mode/src/service.rs#L166-L170). That is likely intentional, but it means notifications are best-effort rather than part of the script success contract.

## Bottom line

`codex-code-mode` is best understood as a **sandboxed JS orchestration engine plus session controller**, not as a full end-user tool by itself. It executes JS, exposes a controlled set of helpers, bridges nested tool calls to an abstract host, and supports incremental `wait`/terminate behavior. The outer `core` layer supplies persistence policy, final output truncation, nested tool discovery, and protocol-specific adaptation.
