# Interactive Codex CLI conversation flow

This page traces one interactive conversation through the current Rust implementation at commit `4254f76393805034cfd254779c80c9638057d070`. It is an implementation map, not a protocol promise. Source and focused tests outrank older prose where they disagree.

Source citations beginning with `codex-rs/` are relative to the workspace's `codex/` repository root. For example, `codex-rs/core/src/session/turn.rs` resolves to `../../../codex/codex-rs/core/src/session/turn.rs` from this document.

Companions: [symbol and call-chain code map](codex-cli-conversation-code-map.md) and [ownership/type map](codex-cli-conversation-type-map.md).

Evidence labels used below:

- **Confirmed** — executable code or a focused test establishes the behavior.
- **Documented intent** — a comment or repository document states the intent.
- **Interpretation** — an architectural explanation inferred from confirmed structure.
- **Uncertain** — the evidence does not justify one universal answer.

## The durable mental model

**Confirmed:** the interactive TUI always crosses the app-server protocol boundary. “Embedded” means the app-server runs in-process; it does not mean the TUI calls `codex-core` directly. A normal conversation has four nested lifetimes:

```text
durable thread (ThreadId; rollout/history and metadata)
└── active runtime (CodexThread → Codex → Arc<Session>)
    └── user-visible turn (submission ID; RegularTask; TurnContext)
        └── one or more run_turn segments (local ModelClientSession)
            └── logical sampling step(s) (StepContext; Responses stream plus retries)
                └── zero or more tool calls (call ID; execution future; paired output)
```

The word *turn* is overloaded in older code and documents. On this page:

- **thread** is the durable conversation identity;
- **runtime session** is the loaded core engine for that thread;
- **session task** is the one active `SessionTask` owned by `Session`;
- **user-visible turn** is the `TurnStarted … TurnComplete/TurnAborted` interval correlated by the submission ID;
- **run segment** is this page's label for one `run_turn` invocation, which owns a local `ModelClientSession`;
- **sampling step** is one logical model request/stream built from one `StepContext`;
- **transport retry** repeats the wire/stream attempt without creating a new turn or step snapshot.

One tool call therefore causes a follow-up **sampling step inside the same `run_turn` segment and user-visible turn**, not a new public turn. `RegularTask` can also call `run_turn` again for queued input while retaining its public `TurnContext`; that later invocation creates a new `ModelClientSession` (`codex-rs/core/src/tasks/regular.rs:37-89`; `codex-rs/core/src/session/turn.rs:143-460`).

## Compact sequence

```mermaid
sequenceDiagram
    actor U as User
    participant CLI as codex CLI
    participant TUI as TUI App/ChatWidget
    participant ASC as AppServerSession/client
    participant AS as app-server
    participant TM as ThreadManager
    participant CT as CodexThread/Codex
    participant S as Session/RegularTask
    participant M as run_turn / ModelClientSession
    participant TR as ToolRouter/runtime
    participant P as LiveThread/ThreadStore

    U->>CLI: codex (no subcommand)
    CLI->>TUI: run_main(TuiCli)
    TUI->>ASC: select Embedded, LocalDaemon, or Remote
    ASC->>AS: thread/start
    AS->>TM: start_thread_with_options
    TM->>CT: Codex::spawn
    CT->>S: Session::new + submission_loop
    S-->>TM: SessionConfigured (first event)
    TM-->>AS: Arc<CodexThread>
    AS-->>TUI: thread/start response + thread/started

    U->>TUI: submit text
    TUI->>ASC: turn/start (or turn/steer if active)
    ASC->>AS: v2 TurnStartParams
    AS->>CT: Op::UserInput
    CT->>S: Submission through bounded channel
    S->>S: TurnContext + RegularTask + StepContext
    S->>M: stream(Prompt)
    M-->>S: OutputItemDone(tool call)
    S->>P: persist model tool-call item
    S->>TR: build, validate, approve, sandbox, execute
    TR-->>S: paired tool output
    S->>P: persist tool output in call order
    S->>M: follow-up stream(new StepContext, call + output)
    M-->>S: assistant deltas + completed item
    S->>P: persist response/events; flush
    S-->>AS: core Event stream
    AS-->>ASC: v2 item/delta/turn notifications
    ASC-->>TUI: per-thread event route
    TUI-->>U: stream, consolidate, render; clear running state
```

## 1. Bootstrap: process, arguments, runtime, configuration, and logging

1. `cli::main` enters `arg0_dispatch_or_else` (`codex-rs/cli/src/main.rs:956-962`). Before async execution, `codex-arg0` handles `.env`/PATH and helper-name dispatch, starts an OS thread named `codex-main` with a 16 MiB stack, builds a multithread Tokio runtime with all drivers enabled, and `block_on`s the async CLI (`codex-rs/arg0/src/lib.rs:208-237`, `:279-284`). This is both an OS-thread and async-runtime boundary.
2. The private Clap types `MultitoolCli` and `Subcommand` parse the multitool surface (`codex-rs/cli/src/main.rs:106-202`). With no subcommand, root options are folded into `TuiCli`, then `run_interactive_tui` calls `codex_tui::run_main` (`codex-rs/cli/src/main.rs:964-1000`, `:2236-2313`).
3. TUI startup loads bootstrap and final configuration, applies auth restrictions, initializes state/telemetry, and installs tracing (`codex-rs/tui/src/lib.rs:848-1234`). Plaintext TUI logging is opened only when an effective `log_dir` is explicitly present; on Unix it is mode `0600` (`codex-rs/tui/src/lib.rs:1071-1213`).

Errors before terminal initialization return normally. The CLI has one targeted recovery path for a corrupt state database; it does not indiscriminately retry startup (`codex-rs/cli/src/main.rs:2236-2313`).

## 2. The TUI/app-server boundary and target selection

`AppServerTarget` is a crate-private TUI enum (`codex-rs/tui/src/lib.rs:261-279`):

| Target | Transport | Thread parameter semantics | Selection |
| --- | --- | --- | --- |
| `Embedded` | typed in-process client/server channels | local/embedded workspace | fallback/default |
| `LocalDaemon { endpoint }` | remote client using WebSocket framing over the default Unix socket | local/embedded workspace | live default socket **and** replayable launch |
| `Remote { endpoint }` | WebSocket or WebSocket-over-Unix-socket | remote workspace | explicit remote endpoint; wins |

The reuse predicate rejects unreplayable command-line config overrides, non-default loader overrides, strict config, and hook-trust bypass. Unix probing attempts a short connection; non-Unix platforms do not implicitly reuse a daemon (`codex-rs/tui/src/lib.rs:411-478`, `:799-846`). **Interpretation:** transport location and workspace ownership are intentionally separate axes.

`run_ratatui_app` initializes the terminal, starts the selected `AppServerClient`, wraps it in an `AppServerSession`, performs bootstrap/model/account work, and passes the session beside the `App` into `App::run` (`codex-rs/tui/src/lib.rs:1237-1325`, `:1680-1768`). `AppServerSession` owns the client, request counter, parameter mode, and bootstrap defaults; `App` owns UI state, transcript, active/primary thread IDs, per-thread event routing, and the `ChatWidget` (`codex-rs/tui/src/app_server_session.rs:175-245`; `codex-rs/tui/src/app.rs:503-587`).

Concurrency differs by transport:

- the in-process client has bounded command/event channels and a worker task; transcript-critical events block under backpressure, while cosmetic events can become `Lagged` (`codex-rs/app-server-client/src/lib.rs:96-205`, `:431-575`);
- the remote client has a bounded command queue, unbounded event queue, response map, and WebSocket worker (`codex-rs/app-server-client/src/remote.rs:151-470`).

## 3. New thread and runtime-session creation

For a fresh launch, `App::run` starts `thread/start` concurrently and lets the first `ChatWidget` exist while submissions remain queued (`codex-rs/tui/src/app.rs:629-647`, `:885-925`). The request path is:

```text
App::spawn_startup_thread_start
→ AppServerSession::start_thread_with_request_handle
→ ClientRequest::ThreadStart
→ MessageProcessor
→ ThreadRequestProcessor::thread_start_inner / thread_start_task
→ ThreadManager::start_thread_with_options
→ ThreadManagerState::spawn_thread_with_source
→ Codex::spawn
→ Session::new
→ ThreadManagerState::finalize_thread_spawn
```

Important effects in order:

1. Startup uses a string request ID `startup-thread-start-{UUID}`; ordinary TUI requests use integers starting at one (`codex-rs/tui/src/app_server_session.rs:1184-1210`). The app-server scopes client IDs with a connection identity, preventing collisions between clients (`codex-rs/app-server/src/outgoing_message.rs:43-48`).
2. `thread_start_inner` validates configuration, trust, environment, tools, and history mode, then spawns tracked background work (`codex-rs/app-server/src/request_processors/thread_processor.rs:930-1040`).
3. `thread_start_task` calls `ThreadManager::start_thread_with_options` with initial history, source, dynamic tools, environment, trace, and extensions (`codex-rs/app-server/src/request_processors/thread_processor.rs:1089-1255`; `codex-rs/core/src/thread_manager.rs:647-720`).
4. `ThreadManagerState::spawn_thread_with_source` resolves new/resume/inheritance inputs and calls crate-private `Codex::spawn` (`codex-rs/core/src/thread_manager.rs:1519-1629`).
5. `Codex::spawn_internal` creates a capacity-512 submission channel, an unbounded event channel, status watch state, `Arc<Session>`, and a background `submission_loop` (`codex-rs/core/src/session/mod.rs:500-742`).
6. `Session::new` chooses/restores `ThreadId` and `SessionId`, initializes auth/MCP/state/persistence concurrently, creates or resumes `LiveThread`, assembles `SessionServices` and `SessionState`, then emits `SessionConfigured` as the first event (`codex-rs/core/src/session/session.rs:514-647`, `:1051-1199`). A `LiveThreadInitGuard` discards a half-created writer if initialization fails.
7. `finalize_thread_spawn` requires that first event and the empty initial correlation ID, wraps the queue facade in `Arc<CodexThread>`, and inserts it into `ThreadManagerState`'s `RwLock<HashMap<ThreadId, Arc<CodexThread>>>` (`codex-rs/core/src/thread_manager.rs:1636-1677`). Duplicate live IDs are shut down rather than silently replacing the resident handle.
8. The app-server attaches its listener, returns the RPC response, then broadcasts `thread/started`. TUI session lifecycle code installs the per-thread channel/state, activates the widget, replays history, and drains queued input (`codex-rs/app-server/src/request_processors/thread_processor.rs:1260-1366`; `codex-rs/tui/src/app/session_lifecycle.rs:492-527`; `codex-rs/tui/src/app/thread_routing.rs:1109-1163`).

**New versus resume/fork.** New/fork/cleared history generates a new UUIDv7 `ThreadId`; resume reuses the historical ID. Root `SessionId` normally derives from `ThreadId`, but resumed and non-root agent sessions can differ (`codex-rs/core/src/session/session.rs:514-558`; `codex-rs/protocol/src/thread_id.rs:16-29`; `codex-rs/protocol/src/session_id.rs:15-28`). Resume rebuilds model history from rollout items; fork flushes/snapshots the source and starts a fresh thread identity (`codex-rs/core/src/thread_manager.rs:760-817`, `:949-1045`).

## 4. Text submission: UI state to core `Op`

The composer returns `InputResult::Submitted`; `ChatWidget` either sends it or queues it until session/turn state allows submission (`codex-rs/tui/src/bottom_pane/chat_composer.rs:2681-2838`; `codex-rs/tui/src/chatwidget/input_flow.rs:9-166`). Submission then crosses three distinct representations:

1. `ChatWidget::submit_user_message_with_history_and_shell_escape_policy` converts composer content into app-server `UserInput` values, adds the local user cell/history, and emits TUI-private `AppCommand::UserTurn` (`codex-rs/tui/src/chatwidget/input_submission.rs:98-420`).
2. `AppEvent::CodexOp` reaches `App::submit_active_thread_op`. If the thread has an active turn, the TUI first tries `turn/steer` and handles the stale-turn race; otherwise it calls `AppServerSession::turn_start` (`codex-rs/tui/src/app/event_dispatch.rs:340-343`; `codex-rs/tui/src/app/thread_routing.rs:418-677`).
3. `AppServerSession::turn_start` allocates an app-server request ID and sends v2 `TurnStartParams` (`codex-rs/tui/src/app_server_session.rs:787-835`; `codex-rs/app-server-protocol/src/protocol/v2/turn.rs:68-171`). `TurnRequestProcessor::turn_start_inner` validates/maps the request, constructs the **current core** `Op::UserInput`, and calls `CodexThread::submit_user_input_with_client_user_message_id` (`codex-rs/app-server/src/request_processors/turn_processor.rs:443-579`).

`CodexThread::submit` delegates to `Codex::submit`; the latter generates a UUIDv7 submission ID, sends `Submission { id, op }` on the bounded channel, and returns that ID (`codex-rs/core/src/codex_thread.rs:202-204`; `codex-rs/core/src/session/mod.rs:745-795`, `:904-910`). App-server returns the same string as `TurnStartResponse.turn.id`. It is the user-visible turn/event correlation ID—not the app-server JSON-RPC request ID and not a model response ID.

## 5. Submission loop, task, public turn, and active state

`submission_loop` receives operations sequentially (`codex-rs/core/src/session/handlers.rs:714-872`). Keeping it separate from the running task is what permits approval, steering, interrupt, and shutdown operations to be processed while model/tool work awaits.

For `Op::UserInput`, `user_input_or_turn_inner`:

1. applies per-turn settings;
2. builds `Arc<TurnContext>` using the submission ID;
3. first attempts to steer an active regular turn;
4. if idle, constructs `TurnInput` and calls `Session::spawn_task(RegularTask)` (`codex-rs/core/src/session/handlers.rs:194-285`).

`Session::spawn_task` aborts an existing task only when directly asked to replace it; normal new input first uses steering/queueing. `start_task` creates the root `CancellationToken`, `TurnState`, `RunningTask`, and `ActiveTurn`, then spawns the task on Tokio (`codex-rs/core/src/tasks/mod.rs:313-451`; `codex-rs/core/src/state/turn.rs:30-100`). The session invariant is one active task. Pending approval/input/dynamic-tool oneshots and queued input are turn-scoped in `TurnState`.

`RegularTask::run` emits exactly one `TurnStarted` and calls `run_turn` (`codex-rs/core/src/tasks/regular.rs:37-89`). It passes the optional startup-prewarmed `ModelClientSession` only to the first call; if pending input remains after that call returns, the same task and `TurnContext` can invoke `run_turn` again. `TurnContext` is the user-turn-stable snapshot: model/provider, effective configuration, environment, approval/permission state, identifiers, telemetry, and timing (`codex-rs/core/src/session/turn_context.rs:104-147`).

## 6. Context, tools, prompt, and one sampling step

`run_turn` creates or consumes one local `ModelClientSession`, performs pre-sampling compaction, records input/context, and enters its sampling loop (`codex-rs/core/src/session/turn.rs:143-460`). Before each new logical sampling step it captures a fresh `Arc<StepContext>` (`codex-rs/core/src/session/mod.rs:2879-2917`). This request-scoped snapshot retains the `Arc<TurnContext>` plus exact environment/capability roots, MCP snapshot/tool list, and loaded `AGENTS.md` state (`codex-rs/core/src/session/step_context.rs:13-48`). Transport retries inside that step do not recapture it.

The prompt is assembled from:

- normalized `ContextManager` history, oldest first; call/output pairing is repaired and unsupported images removed (`codex-rs/core/src/context_manager/history.rs:38-144`, `:355-368`);
- model base instructions and typed contextual fragments implementing `ContextualUserFragment` (`codex-rs/context-fragments/src/fragment.rs:46-114`);
- world/environment/skills/hooks context captured for the turn/step;
- `ToolSpec`s and executable registrations produced from the **same** step plan (`codex-rs/core/src/tools/spec_plan.rs:159-272`);
- optional output schema and parallel-tool flag in `Prompt` (`codex-rs/core/src/client_common.rs:18-48`).

`run_sampling_request` creates one `ToolRouter`, one `ToolCallRuntime`, base-instruction snapshot, and retry loop for the step (`codex-rs/core/src/session/turn.rs:1085-1207`). The same `StepContext` and router survive transport retries, enforcing the invariant that the tools advertised to the model are the tools used to execute its calls.

## 7. Responses API request and stream

`ModelClient` is session-scoped client configuration and fallback state. In the current caller structure, `ModelClientSession` is scoped to one `run_turn` invocation (the client comments call this a Codex turn): it carries reusable WebSocket/incremental and sticky-routing state across that invocation's samples and retries, but must not cross into another model-turn invocation (`codex-rs/core/src/client.rs:252-300`, `:412-479`; `codex-rs/core/src/tasks/regular.rs:68-88`). `ModelProvider` is the provider/auth/capability boundary, while `ModelProviderInfo` is provider configuration (`codex-rs/model-provider/src/provider.rs:101-227`; `codex-rs/model-provider-info/src/lib.rs:89-167`).

With no explicit provider, configuration selects the built-in provider ID `openai`; an unknown configured provider is an initialization error (`codex-rs/core/src/config/mod.rs:3454-3468`). The exact model is then resolved through the current model manager/configuration rather than hard-coded in this flow.

`ModelClientSession::stream` builds `ResponsesApiRequest` with the selected model, instructions, normalized input, tools, automatic tool choice, parallel flag, reasoning controls, stream flag, include fields, cache key, service tier, output schema/text options, and client metadata (`codex-rs/core/src/client.rs:824-907`; `codex-rs/codex-api/src/common.rs:216-270`). The current wire API is Responses; configured legacy chat mode is rejected rather than silently selected.

Transport behavior is capability-driven:

- WebSocket is preferred only when the provider supports it and the session has not fallen back;
- otherwise HTTP Responses streaming is used;
- retryable stream failures consume the provider retry budget with backoff;
- exhausted WebSocket retries can produce a sticky session fallback to HTTP;
- premature EOF before `response.completed` is a retryable error (`codex-rs/core/src/client.rs:926-937`, `:1765-1825`; `codex-rs/core/src/responses_retry.rs:20-79`; `codex-rs/core/src/session/turn.rs:2018-2036`).

`try_run_sampling_request` consumes public `ResponseEvent`s (`codex-rs/codex-api/src/common.rs:74-121`) and emits deltas/events while retaining completed response items. The model `response_id` is transport/telemetry state and can support incremental WebSocket requests; it is not the public turn ID and is not present in current `TurnCompleteEvent`.

## 8. Tool-call feedback loop: registration, approval, sandbox, execution

On `ResponseEvent::OutputItemDone`, `handle_output_item_done` asks `ToolRouter` to parse a tool call. It records the model call item **before** execution, starts a child-cancelled tool future, and sets `needs_follow_up = true` (`codex-rs/core/src/stream_events_utils.rs:319-420`). Malformed but recoverable calls become model-visible error outputs; fatal failures end the task.

The execution layers are deliberately different:

| Layer | Responsibility | Does not decide |
| --- | --- | --- |
| `ToolSpec` / tool plan | model-visible schema and exposure | execution |
| `ToolRouter` / `ToolRegistry` | call parsing, name/payload validation, hook/lifecycle dispatch | sandbox policy |
| `ToolCallRuntime` | bind exact session/step, parallelism, cancellation, failure normalization | tool catalog construction |
| handler/runtime | tool-specific request and output | universal approval order |
| `ToolOrchestrator` | approval → sandbox → optional escalation retry | registration/history |

`ToolCallRuntime` spawns each dispatch. Parallel-capable tools share a read lock; a serial tool holds the write lock and excludes all other calls from that sampling response. Cancellation either waits for declared teardown or aborts the spawned task, but still produces one normalized aborted outcome (`codex-rs/core/src/tools/parallel.rs:42-259`). `ToolRegistry` increments active tool counts, validates runtime/payload, emits lifecycle, runs pre-hooks, calls the handler, runs post-hooks, and claims terminal outcome exactly once (`codex-rs/core/src/tools/registry.rs:322-710`).

For the representative local shell path:

1. the shell handler normalizes permissions, detects special `apply_patch`, asks exec policy to classify the command, builds `ShellRequest`, and invokes `ToolOrchestrator` (`codex-rs/core/src/tools/handlers/shell.rs:63-245`; `codex-rs/core/src/exec_policy.rs:234-373`);
2. permission hooks, configured automatic reviewer/Guardian, and user approval are resolved in that order as applicable (`codex-rs/core/src/tools/orchestrator.rs:137-217`; `codex-rs/core/src/tools/approvals.rs:180-263`);
3. `Session::request_command_approval` stores a oneshot sender in `TurnState` **before** emitting `ExecApprovalRequest`, then awaits it (`codex-rs/core/src/session/mod.rs:2156-2244`);
4. app-server projects that event into a server request with its own request ID; TUI presents an overlay and answers; app-server maps the answer to `ReviewDecision` and submits `Op::ExecApproval`; the submission loop resolves the oneshot (`codex-rs/app-server/src/bespoke_event_handling.rs:529-680`, `:1908-2031`; `codex-rs/core/src/session/handlers.rs:373-415`);
5. the orchestrator selects a sandbox and executes. A sandbox-denial path may request fresh approval and make at most one escalated retry; denied-read constraints prevent an unsafe unsandboxed bypass (`codex-rs/core/src/tools/orchestrator.rs:219-505`; `codex-rs/core/src/tools/sandboxing.rs:157-465`).

Approval answers **whether** execution may proceed; exec policy classifies/amends a request; sandboxing controls **how** it runs. They are not aliases. Defaults are resolved from trust, managed requirements, platform, and frontend config: trusted projects resolve `OnRequest`, untrusted projects `UnlessTrusted`, and the default reviewer is the user unless constrained. With no explicit legacy sandbox mode, any recorded project trust decision currently resolves to workspace-write (except unsandboxed Windows, which is forced read-only); no trust decision falls back to `SandboxMode::default`, and requirements can replace a disallowed default (`codex-rs/core/src/config/mod.rs:3395-3430`; `codex-rs/config/src/config_toml.rs:742-815`).

## 9. Tool output, ordered recording, and follow-up sampling

Tool futures live in `FuturesOrdered`. Execution can overlap, but `drain_in_flight` converts and records outputs in the model's call order (`codex-rs/core/src/session/turn.rs:1893-1917`, `:2455-2483`). This preserves call/output pairing and deterministic next-prompt ordering. The drain happens after the response stream outcome and before cancellation is returned, so completed tool outputs are not silently lost.

`ResponseEvent::Completed` contributes usage and `end_turn`; either a tool call or explicit `end_turn = false` forces follow-up (`codex-rs/core/src/session/turn.rs:2278-2305`). The outer loop then:

1. records all paired tool outputs;
2. checks pending input, stop hooks, token/window thresholds, and compaction;
3. captures a new `StepContext`;
4. rebuilds normalized history containing call + output;
5. sends another Responses request with the same public turn ID (`codex-rs/core/src/session/turn.rs:248-418`).

The loop exits only when no follow-up, hook continuation, queued input, or required compaction remains.

## 10. Events and persistence ordering

`Session::record_conversation_items` fills missing turn/item IDs, mutates `ContextManager` under the session-state mutex, persists `RolloutItem::ResponseItem`, then emits raw item events (`codex-rs/core/src/session/mod.rs:2767-2836`). `Session::send_event` wraps the message in `Event { id: TurnContext.sub_id, msg }`; `send_event_raw_with_persistence` attempts to append `RolloutItem::EventMsg` before tracing/delivering the event (`codex-rs/core/src/session/mod.rs:1768-1810`, `:1947-1985`). Persistence errors are logged and swallowed rather than failing an otherwise live turn (`:3493-3499`).

Persistence layers:

```text
Session ContextManager           active model-visible history
        │
        ├─ RolloutItem stream    durable replay representation
        ▼
LiveThread                      active per-thread lifecycle + metadata ordering
        ▼
ThreadStore trait               storage-neutral create/resume/append/flush API
        ├─ Local store → RolloutRecorder → canonical JSONL history
        └─ metadata sync → StateRuntime/SQLite query projection
```

`LiveThread` delegates append to `ThreadStore`, then applies derived metadata only after append succeeds (`codex-rs/thread-store/src/live_thread.rs:91-243`). The local writer filters by shared rollout policy and flushes JSONL before metadata projection can advance, so SQLite does not get ahead of accepted local history (`codex-rs/thread-store/src/local/live_writer.rs:109-184`). `RolloutRecorder` is the bounded-channel background JSONL writer (`codex-rs/rollout/src/recorder.rs:84-130`, `:884-1047`). `StateRuntime` is a query/metadata and auxiliary-domain SQLite service, not a duplicate authoritative prompt transcript (`codex-rs/state/src/runtime.rs:166-323`).

On task exit, core flushes ordinary work, emits `TurnComplete` or `TurnAborted`, clears the active turn, updates idle lifecycle, then flushes again so the terminal event crosses a durability barrier (`codex-rs/core/src/tasks/mod.rs:563-809`).

## 11. App-server projection and TUI rendering

An app-server listener owns `Arc<CodexThread>` and selects between `next_event()`, cancellation, listener commands, and idle unload (`codex-rs/app-server/src/request_processors/thread_lifecycle.rs:138-385`). `apply_bespoke_event_handling` projects core events to v2 notifications:

- `TurnStarted`/`TurnComplete` → `turn/started`/`turn/completed`;
- item lifecycle → `item/started`/`item/completed`;
- text/reasoning deltas → delta notifications;
- approval events → app-server requests that await client response in detached tasks (`codex-rs/app-server/src/bespoke_event_handling.rs:135-197`, `:529-680`, `:849-994`, `:1224-1245`).

The TUI routes `AppServerEvent` through the active thread's store/channel, then `ChatWidget::handle_server_notification` updates stream controllers, running state, and history (`codex-rs/tui/src/app/app_server_events.rs:31-222`; `codex-rs/tui/src/app/thread_events.rs:41-150`; `codex-rs/tui/src/app/thread_routing.rs:1464-1579`; `codex-rs/tui/src/chatwidget/protocol.rs:4-337`). A `FrameRequester` coalesces redraws into `TuiEvent::Draw`; `App` and `ChatWidget` render into the terminal buffer (`codex-rs/tui/src/tui/frame_requester.rs:1-65`; `codex-rs/tui/src/app.rs:1269-1364`; `codex-rs/tui/src/tui.rs:880-950`).

The final answer has two complementary signals:

- assistant deltas provide streaming display, while authoritative `ItemCompleted` consolidates the visible markdown into a resize-safe history cell;
- `TurnCompleted` carries terminal status/timing and clears “Working”; it does **not** carry the answer text (`codex-rs/tui/src/chatwidget/streaming.rs:17-58`, `:268-466`; `codex-rs/tui/src/chatwidget/turn_runtime.rs:92-185`).

## 12. Identity ledger

| Identity/object | Created | Stored/copied | Correlates |
| --- | --- | --- | --- |
| TUI app-server request ID | `AppServerSession::next_request_id`; integer from 1 | client response map; connection-scoped in server | one JSON-RPC request/response only |
| startup request ID | `startup-thread-start-{UUID}` | same request path | asynchronous initial `thread/start` |
| thread ID | UUIDv7 in `Session::new`, or restored | `Session`, `CodexThread`, `LiveThread`, rollout metadata, SQLite | durable conversation/runtime lookup |
| session ID | root usually derived from thread ID; restored/shared for other cases | session metadata/agent control | runtime/agent grouping; not always thread identity |
| submission/public turn ID | UUIDv7 in `Codex::submit` | `TurnContext.sub_id`, `Event.id`, v2 turn/item notifications | one public turn/task |
| model response ID | provider `ResponseEvent::Completed` | private last-response/transport state | incremental transport and telemetry |
| tool call ID | model `ResponseItem` | `ToolCall`, invocation, item/approval, paired output | exactly one model call/output pair |
| approval request ID | app-server outgoing sender | request callback/TUI correlation map | client answer; distinct from call/item ID |
| root cancellation token | `Session::start_task` | `RunningTask`; child tokens per sample/tool | interrupt/replacement/shutdown propagation |
| rollout path | `LiveThread`/local store | `SessionConfigured`, `CodexThread`, TUI session state | local durable history location; lazy for new threads |

## 13. Alternative and failure paths that change the normal flow

- **Steering and queued input.** Active input is steered/queued before replacement. The TUI and core both handle stale active-turn races (`codex-rs/tui/src/app/thread_routing.rs:517-677`; `codex-rs/core/src/session/handlers.rs:194-285`).
- **Interrupt.** TUI sends `turn/interrupt` with the cached active ID and may retry once with the server-reported ID. Core cancels the root token, waits a grace period, aborts the task if necessary, records aborted tool/history state, emits `TurnAborted`, and flushes (`codex-rs/app-server/src/request_processors/turn_processor.rs:1357-1410`; `codex-rs/core/src/tasks/mod.rs:834-918`).
- **Retry.** Retryable transport/stream failures remain within one sampling step. TUI receives retry notifications and keeps the task alive (`codex-rs/app-server/src/bespoke_event_handling.rs:921-936`; `codex-rs/tui/src/chatwidget/protocol.rs:19-28`).
- **Compaction.** Pre-turn or mid-turn compaction rewrites `ContextManager`, increments history version, and rebuilds the next prompt. Mid-turn compaction occurs only when follow-up remains and window/token policy requires it (`codex-rs/core/src/session/turn.rs:346-370`; `codex-rs/core/src/context_manager/history.rs:183-190`).
- **Approval cancellation.** Pending decisions are oneshots in `TurnState`; clearing/dropping them resolves waiters as `Abort`. App-server aborts stale per-turn server requests during turn transitions.
- **Remote disconnect.** TUI displays an error and exits fatally rather than pretending the turn completed (`codex-rs/tui/src/app/app_server_events.rs:31-58`).
- **Runtime shutdown.** Closing the submission channel performs full teardown even without an explicit shutdown op. `CodexThread::shutdown_and_wait` and manager-wide bounded shutdown are the public lifecycle paths (`codex-rs/core/src/session/handlers.rs:859-870`; `codex-rs/core/src/codex_thread.rs:211-218`; `codex-rs/core/src/thread_manager.rs:898-947`).
- **History modes.** Current `thread/start` rejects paginated app-server history mode in this path as unsupported. Local legacy storage uses JSONL as replay authority; do not extrapolate its exact materialization behavior to every remote/store implementation.

## 14. Focused evidence

Inspected tests that anchor the normal path (the four marked **executed** also passed in the coordinated run):

- target selection and workspace semantics: `codex-rs/tui/src/lib.rs:2207-2315` (**executed:** explicit-remote precedence case);
- embedded app-server RPC: `codex-rs/tui/src/lib.rs:2741-2759`;
- thread creation and lazy rollout materialization: `codex-rs/app-server/tests/suite/v2/thread_start.rs:316-467`;
- turn correlation: `codex-rs/app-server/tests/suite/v2/turn_start.rs:221-324`, `:1404-1549`;
- a blocking tool plus two sampling requests in one turn: `codex-rs/app-server/tests/suite/v2/turn_start.rs:1003-1108` (**executed**);
- approval and follow-up: `codex-rs/app-server/tests/suite/v2/turn_start.rs:2116-2284`;
- call/output ordering and parallel execution: `codex-rs/core/tests/suite/tool_parallelism.rs:91-436`;
- interruption and aborted tool output: `codex-rs/core/tests/suite/abort_tasks.rs:22-176`;
- retry after missing `response.completed`: `codex-rs/core/tests/suite/stream_no_completed.rs:27-99`;
- WebSocket fallback/stickiness: `codex-rs/core/tests/suite/websocket_fallback.rs:29-255`;
- terminal persistence barriers: `codex-rs/core/src/session/tests.rs:9512-9575`;
- local resume/history/metadata: `codex-rs/thread-store/src/local/mod.rs:442-792`, `:851-900` (**executed:** resume-and-append case);
- final item consolidation and working-state completion: `codex-rs/tui/src/chatwidget/tests/app_server.rs:483-587` (**executed**).

## 15. Known documentation drift and evidence gaps

Meaningful conflicts with older repository prose:

1. `codex/openwiki/domain/core-concepts.md` names core `Op::UserTurn`; executable core uses `Op::UserInput`. `UserTurn` is TUI-private `AppCommand` vocabulary.
2. OpenWiki and `codex-rs/docs/protocol_v1.md` often define a turn as one request/tool cycle. The current public turn spans one task and potentially several sampling steps.
3. `protocol_v1.md` describes deferred session configuration, automatic task abort on new input, approval terminating a task, and a response ID in `TurnComplete`; none matches the current path.
4. Older diagrams present app-server mainly as an external IDE boundary. Current TUI always uses it, including embedded mode.
5. `core/src/tools/orchestrator.rs` says escalation retry avoids reapproval through caching; current executable branches can request a fresh reviewer/user/network decision.
6. `state/src/lib.rs` places rollout backfill orchestration in state; current orchestration lives under core rollout metadata/state-DB code.

Remaining limits:

- Shell/exec is the representative approval+sandbox trace. MCP, hosted, dynamic, extension, and connector tools share the router/registry boundary but have handler-specific execution and may not use `ToolOrchestrator`.
- Exact effective approval and sandbox policies depend on trust, managed requirements, platform, environment, and launch configuration.
- Rust Analyzer corroborated the principal incoming calls, but some broad call graphs were truncated and some precise-range requests selected enclosing `impl` blocks; every conclusion above was rechecked with direct source and `rg`.
- This survey is broad and critical-path focused; it is not a claim that every crate or tool implementation was read.

## Takeaway

When debugging an interactive prompt, follow ownership and correlation together:

```text
App/ChatWidget
→ AppServerSession (request ID)
→ app-server (ThreadId + v2 Turn)
→ CodexThread/Codex (submission queue)
→ Session (one ActiveTurn)
→ RegularTask/TurnContext (public turn ID)
→ run_turn/ModelClientSession (model-run segment)
→ StepContext/ToolRouter (logical sampling step; retries reuse it)
→ ToolCallRuntime (tool call ID)
→ LiveThread/ThreadStore (ordered durable items)
→ core Event → v2 notification → ChatWidget render
```

That chain identifies where to inspect a failure: transport and protocol in TUI/app-server, lifecycle in `ThreadManager`/`Session`, request content in `run_turn`/`ModelClientSession`, execution safety in tool orchestration, durability in `LiveThread`/`ThreadStore`, and presentation in the TUI event/streaming/rendering modules.
