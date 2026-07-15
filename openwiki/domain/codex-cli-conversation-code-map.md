# Codex CLI conversation: implementation code map

This page is an implementation index for one interactive `codex` conversation in the current Rust workspace. It follows the default no-subcommand launch, a new thread, one text submission, a model-requested command tool, approval, tool output, a follow-up sample, and a final assistant message. It also identifies where resume, remote app-server, steering, interruption, retry, compaction, and failure diverge.

The source of truth is the current tree under the workspace's `codex/` repository; inline citations beginning with `codex-rs/` are relative to that repository root. For example, `codex-rs/core/src/session/turn.rs` resolves to `../../../codex/codex-rs/core/src/session/turn.rs` from this document. Documentation is used only as supporting evidence. Line numbers are deliberately precise enough for navigation, but should be rechecked after source edits.

Companions: [end-to-end flow](codex-cli-conversation-flow.md) and [ownership/type map](codex-cli-conversation-type-map.md).

## Evidence-status legend

- **Confirmed — code:** executable control flow or a type definition establishes the claim.
- **Confirmed — test:** a current test asserts the observable behavior.
- **Documented intent:** a comment or OpenWiki page says this, but executable behavior remains authoritative.
- **Interpretation:** an architectural name used here to make the code easier to reason about.
- **Uncertain:** the inspected evidence does not completely determine the behavior; the gap is recorded explicitly.

Unless a row says otherwise, stage and call-chain rows below are **Confirmed — code**.

## Canonical path at a glance

```text
main / arg0 bootstrap
  -> cli_main (no subcommand)
  -> codex_tui::run_main
  -> start_app_server (embedded, local-daemon, or explicit remote)
  -> JSON-RPC thread/start
  -> ThreadManager::start_thread_with_options
  -> Codex::spawn -> Session::new -> submission_loop
  -> CodexThread installed in ThreadManager
  -> App::run
  -> composer -> AppCommand::UserTurn -> JSON-RPC turn/start
  -> CodexThread::submit_user_input_with_client_user_message_id
  -> Submission { UUIDv7 id, Op::UserInput }
  -> user_input_or_turn_inner -> TurnContext -> RegularTask
  -> run_turn (local ModelClientSession) -> fresh logical-step StepContext -> Prompt
  -> ModelClientSession::stream -> Responses API
  -> ResponseEvent::OutputItemDone(tool call)
  -> ToolRouter -> ToolCallRuntime -> ToolOrchestrator
  -> approval request/event -> app-server server request -> TUI decision -> Op::ExecApproval
  -> normalized tool output recorded in rollout/history
  -> another sampling request
  -> final assistant item -> TurnComplete
  -> persist first, project through app-server, update ChatWidget, draw
```

The most important naming correction is that a user-visible **turn is not one model request**. A `RegularTask` and its `TurnContext` represent the app-server turn. That task can invoke `run_turn` more than once for late pending input; each invocation owns a local `ModelClientSession` and can execute several logical sampling iterations. Each outer iteration captures a fresh `StepContext`, while transport retries inside it reuse the same step.

## Stage-to-symbol map

| Stage | Crate/module | Entry symbols | Primary downstream symbols | State/event boundary |
|---|---|---|---|---|
| Process bootstrap | `codex-arg0`, `codex-cli` | `arg0_dispatch_or_else`, `build_runtime`, `cli_main` | `run_interactive_tui` | OS thread -> Tokio multi-thread runtime |
| TUI/config bootstrap | `codex-tui::lib` | `run_main`, `app_server_target_for_launch`, `run_ratatui_app` | `start_app_server`, `AppServerSession::new`, `App::run` | direct Rust call -> JSON-RPC-shaped client |
| App-server selection | `codex-tui::lib`, `codex-app-server-client` | `AppServerTarget`, `start_app_server` | `InProcessAppServerClient` or `RemoteAppServerClient` | bounded client channels or socket transport |
| New thread | `codex-app-server::request_processors::thread_processor` | `thread_start_inner`, `thread_start_task` | `ThreadManager::start_thread_with_options` | JSON-RPC request id -> core thread id |
| Core runtime initialization | `codex-core::{thread_manager,session}` | `spawn_thread_with_source`, `Codex::spawn`, `Session::new`, `finalize_thread_spawn` | `submission_loop`, `CodexThread` | bounded submission queue, unbounded event queue, watch status |
| User submission | `codex-tui::{chatwidget,app}`, `codex-app-server` | `submit_user_message_with_history_and_shell_escape_policy`, `submit_active_thread_op`, `turn_start_inner` | `CodexThread::submit_user_input_with_client_user_message_id` | TUI `AppCommand::UserTurn` -> v2 `TurnStartParams` -> core `Op::UserInput` |
| Task/turn startup | `codex-core::session::handlers`, `codex-core::tasks` | `submission_loop`, `user_input_or_turn_inner`, `Session::spawn_task`, `start_task`, `RegularTask::run` | `run_turn` | submission id becomes turn id; cancellation token and active task installed |
| Sampling request | `codex-core::session::turn`, `codex-core::client` | `capture_step_context`, `build_prompt`, `run_sampling_request`, `ModelClientSession::stream` | HTTP or WebSocket Responses transport | history + exact step tool view -> `ResponsesApiRequest` |
| Stream/tool loop | `codex-core::{stream_events_utils,tools}` | `try_run_sampling_request`, `handle_output_item_done`, `ToolCallRuntime::handle_tool_call` | `ToolRouter`, tool handler, `ToolOrchestrator` | streamed `ResponseEvent` -> ordered tool future |
| Approval/sandbox | `codex-core::tools`, app-server bespoke handling, TUI request handling | `resolve_tool_apporval`, `request_command_approval`, `apply_bespoke_event_handling` | server request, `Op::ExecApproval`, sandbox execution | oneshot stored in `TurnState`; app-server request id correlates UI response |
| Follow-up sampling | `codex-core::session::turn` | `drain_in_flight`, `record_conversation_items`, `run_turn` loop | next `run_sampling_request` | tool output paired in history before next prompt |
| Persistence/events | `codex-core::session`, `codex-thread-store`, `codex-rollout` | `send_event_raw_with_persistence`, `LiveThread::append_items`, `flush_rollout` | event channel and local store | durable rollout append precedes event delivery |
| Projection/rendering | `codex-app-server`, `codex-tui` | listener `next_event`, `apply_bespoke_event_handling`, `handle_server_notification`, `Tui::draw` | transcript/history cells and terminal frame | core `EventMsg` -> v2 notification -> widget state -> Ratatui |

## API call-chain tables

### 1. CLI to TUI startup

| Order | Caller | Function or method | Receiver/type | Inputs | Output or side effect | Next consumer |
|---:|---|---|---|---|---|---|
| 1 | OS loader | `main` | free function | argv/env | takes remote-control env flag, calls arg0 dispatcher | `arg0_dispatch_or_else` |
| 2 | `main` | `arg0_dispatch_or_else` | free generic function | async `cli_main` closure | performs arg0 helper dispatch, starts `codex-main` OS thread with 16 MiB stack | `build_runtime` |
| 3 | spawned OS thread | `build_runtime` | free function | none | Tokio multi-thread runtime with `enable_all`; `block_on` root future | `cli_main` |
| 4 | arg0 closure | `cli_main` | free async function | `Arg0DispatchPaths`, remote flag | parses private `MultitoolCli`, folds feature/config overrides, dispatches subcommand | no-subcommand branch |
| 5 | `cli_main` | `run_interactive_tui` | free async function | `TuiCli`, optional remote endpoint/auth env, helper paths | validates terminal and remote args; starts TUI with local-state recovery loop | `codex_tui::run_main` |
| 6 | CLI | `codex_tui::run_main` | public async function | TUI CLI/config-loader/remote endpoint | loads effective config, logging/state services, chooses app-server target | `run_ratatui_app` |

Evidence: `codex-rs/cli/src/main.rs:956-993`, `codex-rs/cli/src/main.rs:2236-2305`, `codex-rs/arg0/src/lib.rs:208-280`, `codex-rs/tui/src/lib.rs:848-940`.

### 2. TUI to app-server startup

| Order | Caller | Function or method | Receiver/type | Inputs | Output or side effect | Next consumer |
|---:|---|---|---|---|---|---|
| 1 | `run_main` | `can_reuse_implicit_local_daemon` | free function | CLI overrides, loader overrides, strict mode, non-replayable flags | true only when this invocation's launch state can safely be reused | daemon probe |
| 2 | `run_main` | `app_server_target_for_launch` | free function | explicit endpoint, probed default socket, reuse flag | private `AppServerTarget::{Remote,LocalDaemon,Embedded}` | config/workspace routing |
| 3 | `run_main` | `run_ratatui_app` | free async function | target plus resolved config/services | initializes terminal, logging and panic hook | `start_app_server` |
| 4 | `run_ratatui_app` | `start_app_server` | free async function | target and local runtime dependencies | `AppServerClient::InProcess` for embedded; remote client for daemon/remote | `AppServerSession::new` |
| 5 | `run_ratatui_app` | `AppServerSession::new` | private session façade | client, `ThreadParamsMode` | owns client and monotonically increasing request-id counter (starts at 1) | bootstrap RPCs and `App::run` |
| 6 | `run_ratatui_app` | `App::run` | private `App` | TUI, app-server session, initial config/start action | creates unbounded `AppEvent` channel; starts/resumes/forks thread; enters select loop | event dispatch/render loop |

Explicit remote always wins. With no explicit remote, a reusable default daemon socket selects `LocalDaemon`; otherwise the TUI embeds app-server. Both embedded and local-daemon launches use embedded thread-parameter semantics; explicit remote uses remote workspace semantics. Even the in-process path preserves JSON-RPC request/result/notification shapes rather than exposing core handles directly.

Evidence: `codex-rs/tui/src/lib.rs:261-289`, `codex-rs/tui/src/lib.rs:446-478`, `codex-rs/tui/src/lib.rs:799-844`, `codex-rs/tui/src/lib.rs:1237-1324`, `codex-rs/tui/src/app.rs:629-679`, `codex-rs/tui/src/app.rs:759-891`, `codex-rs/app-server-client/src/lib.rs:96-124`, `codex-rs/app-server-client/src/lib.rs:431-475`, `codex-rs/app-server-client/src/remote.rs:151-183`.

### 3. New-thread creation

| Order | Caller | Function or method | Receiver/type | Inputs | Output or side effect | Next consumer |
|---:|---|---|---|---|---|---|
| 1 | startup helper or `AppServerSession` | typed `thread/start` | `AppServerRequestHandle` | v2 `ThreadStartParams` and app-server request id | JSON-RPC-shaped request | `MessageProcessor` |
| 2 | `MessageProcessor` | request dispatch | processor | `ClientRequest::ThreadStart` | delegates to request processor | `thread_start_inner` |
| 3 | processor | `thread_start_inner` | request processor | connection-scoped request id, params/context | validates incompatible/paginated settings; builds overrides; spawns background task | `thread_start_task` |
| 4 | background task | `thread_start_task` | associated async function | config manager and normalized options | loads effective config/trust, validates dynamic tools | `ThreadManager::start_thread_with_options` |
| 5 | app-server | `start_thread_with_options` | `ThreadManager` | `StartThreadOptions` | delegates to shared manager state | `spawn_thread_with_source` |
| 6 | manager state | `spawn_thread_with_source` | `ThreadManagerState` | config, initial history, services, source/options | reuses a matching running resumed thread or calls `Codex::spawn` | `Codex::spawn` |
| 7 | manager | `Codex::spawn` / `spawn_internal` | `Codex` factory | `CodexSpawnArgs` | bounded submission + unbounded event channels; resolves model/instructions; awaits `Session::new`; spawns submission loop | `finalize_thread_spawn` |
| 8 | core factory | `Session::new` | `Session` factory | session configuration, history, shared services | assigns thread/session IDs; initializes/resumes `LiveThread`; builds `Arc<Session>`; emits initial `SessionConfigured` | `Codex::spawn_internal` |
| 9 | manager state | `finalize_thread_spawn` | `ThreadManagerState` | `Codex`, `ThreadId`, source | consumes first event; requires `SessionConfigured` with `INITIAL_SUBMIT_ID`; installs `Arc<CodexThread>` under write lock | app-server thread listener |
| 10 | app-server | listener attach + response | thread processor | `CodexThread`, session snapshot | attaches event listener, sends `ThreadStartResponse`, then `ThreadStarted` notification | TUI startup state |

Evidence: `codex-rs/app-server-protocol/src/protocol/v2/thread.rs:56-208`, `codex-rs/app-server/src/message_processor.rs:1018-1024`, `codex-rs/app-server/src/request_processors/thread_processor.rs:930-1039`, `codex-rs/app-server/src/request_processors/thread_processor.rs:1089-1371`, `codex-rs/core/src/thread_manager.rs:647-703`, `codex-rs/core/src/thread_manager.rs:1519-1668`, `codex-rs/core/src/session/mod.rs:476-740`, `codex-rs/core/src/session/session.rs:480-745`.

### 4. Turn submission

| Order | Caller | Function or method | Receiver/type | Inputs | Output or side effect | Next consumer |
|---:|---|---|---|---|---|---|
| 1 | composer/keyboard path | `submit_user_message_with_history_and_shell_escape_policy` | `ChatWidget` | `UserMessage`, render/history policy | converts text/images/mentions/skills to v2 `UserInput`; builds private `AppCommand::UserTurn`; marks local pending state | `ChatWidget::submit_op` |
| 2 | widget | `submit_op` | `ChatWidget` | `AppCommand` | emits `AppEvent::CodexOp` | `App` event loop |
| 3 | `App` | `submit_active_thread_op` / `try_submit_active_thread_op_via_app_server` | `App` | active `ThreadId`, command | if active turn exists, tries `turn/steer`; if missing/idle, calls `turn/start` | `AppServerSession::turn_start` |
| 4 | TUI session | `turn_start` | `AppServerSession` | thread id, input, cwd, model, permissions, schema | allocates app-server request id; sends typed v2 `TurnStartParams` | app-server `turn_start_inner` |
| 5 | app-server | `turn_start_inner` | turn processor | connection request id and v2 params | loads thread; validates/maps input; computes sticky overrides; builds core `Op::UserInput` | `CodexThread::submit_user_input_with_client_user_message_id` |
| 6 | thread handle | submission helper | `CodexThread` -> `Codex` | `Op`, trace, optional client message id | creates UUIDv7 submission id and sends `Submission` into bounded queue | `submission_loop` |
| 7 | app-server | `TurnStartResponse` | turn processor | returned submission id | exposes the submission id as v2 `turn.id` | TUI active-turn correlation |

The TUI's `AppCommand::UserTurn` is a UI command. The core protocol does **not** have `Op::UserTurn`; its variant is `Op::UserInput`.

Evidence: `codex-rs/tui/src/chatwidget/input_submission.rs:65-350`, `codex-rs/tui/src/chatwidget.rs:1808-1820`, `codex-rs/tui/src/app/event_dispatch.rs:340-343`, `codex-rs/tui/src/app/thread_routing.rs:418-458`, `codex-rs/tui/src/app/thread_routing.rs:517-676`, `codex-rs/tui/src/app_server_session.rs:787-830`, `codex-rs/app-server/src/request_processors/turn_processor.rs:443-549`, `codex-rs/core/src/codex_thread.rs:254-295`, `codex-rs/core/src/session/mod.rs:746-796`, `codex-rs/core/src/session/mod.rs:909-911`.

### 5. Sampling-request construction

| Order | Caller | Function or method | Receiver/type | Inputs | Output or side effect | Next consumer |
|---:|---|---|---|---|---|---|
| 1 | submission loop | `user_input_or_turn_inner` | free async handler | submission id, `Op::UserInput`, optional client id | applies per-turn settings; creates `Arc<TurnContext>`; steers if possible, otherwise builds `TurnInput` | `Session::spawn_task` |
| 2 | handler | `spawn_task` / `start_task` | `Arc<Session>` | turn context, input, `RegularTask` | aborts prior task; installs `ActiveTurn`/`RunningTask`, token/cancellation state; emits lifecycle; spawns task | `RegularTask::run` |
| 3 | spawned task | `RegularTask::run` | `Arc<RegularTask>` | `SessionTaskContext`, turn context/input/token | emits core `TurnStarted`; obtains optional prewarmed model session | `run_turn` |
| 4 | task | `run_turn` | free async function | session, turn context, input, `ModelClientSession`, token | compacts if needed; records context/instructions/input; enters sampling loop | `capture_step_context` |
| 5 | loop | `Session::capture_step_context` | `Session` | same `Arc<TurnContext>` | immutable request snapshot: environments, capability roots, MCP/tool view, loaded instructions | history and tool construction |
| 6 | loop | `ContextManager::for_prompt` | cloned context manager | model input modalities | normalizes legal model history and returns `Vec<ResponseItem>` | `run_sampling_request` |
| 7 | sampling wrapper | `built_tools` / `ToolRouter::from_context` | step context | exact step snapshot + tool params | constructs registry and model-visible `ToolSpec`s from the same view | `build_prompt` |
| 8 | wrapper | `build_prompt` | free function | history, router, turn/base instructions | `Prompt` with tools, parallel flag, schema | `try_run_sampling_request` |
| 9 | sampling | `ModelClientSession::stream` | current `run_turn` client session | prompt/model/reasoning/service tier/metadata | builds `ResponsesApiRequest`; chooses WebSocket when provider supports it, otherwise HTTP | streamed `ResponseEvent`s |

Evidence: `codex-rs/core/src/session/handlers.rs:194-285`, `codex-rs/core/src/tasks/mod.rs:314-449`, `codex-rs/core/src/tasks/regular.rs:37-90`, `codex-rs/core/src/session/turn.rs:143-433`, `codex-rs/core/src/session/mod.rs:2879-2934`, `codex-rs/core/src/context_manager/history.rs:121-162`, `codex-rs/core/src/session/turn.rs:1085-1164`, `codex-rs/core/src/client.rs:824-915`, `codex-rs/core/src/client.rs:1774-1821`.

### 6. Model-stream processing

| Order | Caller | Function or method | Receiver/type | Inputs | Output or side effect | Next consumer |
|---:|---|---|---|---|---|---|
| 1 | `run_sampling_request` | `try_run_sampling_request` | free async function | tool runtime, session, turn, prompt, child cancellation token | opens stream with cancellation; initializes ordered tool-future queue and item parsers | stream loop |
| 2 | stream loop | `ResponseStream::next` | stream | provider frames | cancellation -> `TurnAborted`; transport error -> sampling error; EOF before completion -> stream error | event match |
| 3 | event match | `OutputItemAdded` branch | stream state | partial response item | emits item-start/deltas and prepares tool argument diff consumer | later deltas/done |
| 4 | event match | delta branches | stream state | text/reasoning/tool deltas | updates parser; emits core delta events without completing item | app-server/TUI streaming projection |
| 5 | event match | `OutputItemDone` branch | stream state | complete `ResponseItem` | finalizes pending parsers; calls `handle_output_item_done`; queues tool future or records final content | tool runtime or completion |
| 6 | event match | `Completed` branch | stream state | token usage, `end_turn`, response id | flushes text parsers, records usage; `end_turn == false` forces follow-up; returns sampling result | wrapper drain/retry |
| 7 | wrapper | retry loop in `run_sampling_request` | current `run_turn` client session | sampling error and provider retry budget | rebuilds prompt from current history for retries; returns hard errors when exhausted/non-retryable | `run_turn` |

The model response id is transport state, especially for incremental WebSocket requests; it is not the app-server turn id. The turn id remains the core submission id.

Evidence: `codex-rs/core/src/session/turn.rs:1940-2332`, `codex-rs/core/src/session/turn.rs:1113-1218`, `codex-rs/core/src/client.rs:297-381`, `codex-rs/codex-api/src/common.rs:74-161`, `codex-rs/codex-api/src/common.rs:216-267`.

### 7. Tool dispatch and approval

| Order | Caller | Function or method | Receiver/type | Inputs | Output or side effect | Next consumer |
|---:|---|---|---|---|---|---|
| 1 | stream done branch | `handle_output_item_done` | `HandleOutputCtx` | model `ResponseItem` | `ToolRouter::build_tool_call`; records model call item immediately; returns tool future and `needs_follow_up` | `ToolCallRuntime` |
| 2 | tool future | `handle_tool_call` | cloneable `ToolCallRuntime` | `ToolCall`, child cancellation token | spawns dispatch task; takes read gate for parallel-capable tool, write gate otherwise | `ToolRouter` |
| 3 | runtime | dispatch method | `ToolRouter` -> `ToolRegistry` | `ToolInvocation` with session/turn/step/call id | validates kind/handler, runs pre-hooks, handler, post-hooks, normalizes result | concrete handler |
| 4 | shell handler | policy evaluation | shell handler/runtime | command, turn permission state, exec policy | computes `ExecApprovalRequirement::{Skip,NeedsApproval,Forbidden}` | `ToolOrchestrator::run` |
| 5 | orchestrator | `resolve_tool_apporval` | approval helpers | requirement, reviewer policy, approval store/context | permission hook/reviewer may skip, forbid, or request user decision | `Session::request_command_approval` |
| 6 | session | `request_command_approval` | `Session` | call/approval id, command/reason/cwd | stores oneshot sender in `TurnState` before emitting `ExecApprovalRequest` | app-server listener |
| 7 | projection | `apply_bespoke_event_handling` | app-server | core approval event | sends JSON-RPC server request, spawns waiter; maps resolved response to core `ReviewDecision` | `Op::ExecApproval` submission |
| 8 | submission loop | `exec_approval` handler | `Session` | approval id/turn id/decision | removes/resolves pending oneshot; wakes orchestrator | sandbox attempt |
| 9 | orchestrator | `run_attempt` / `ToolRuntime::run` | runtime | approved request, selected sandbox | executes command; sandbox denial may trigger permitted escalation/retry; cancellation tears down according to tool policy | normalized tool output |

The exact `Arc<StepContext>` used to advertise a tool is also carried into its execution. `ToolCallRuntime`'s `RwLock<()>` is an admission gate: read locks allow supported parallel calls to overlap; a write lock serializes a non-parallel call against all others. Results may finish concurrently, but `FuturesOrdered` preserves model-call order when they are appended to conversation history.

Evidence: `codex-rs/core/src/stream_events_utils.rs:233-379`, `codex-rs/core/src/tools/router.rs:29-238`, `codex-rs/core/src/tools/registry.rs:322-437`, `codex-rs/core/src/tools/parallel.rs:42-199`, `codex-rs/core/src/tools/orchestrator.rs:40-374`, `codex-rs/core/src/tools/approvals.rs:137-258`, `codex-rs/core/src/tools/sandboxing.rs:121-176`, `codex-rs/core/src/tools/sandboxing.rs:390-431`, `codex-rs/core/src/session/mod.rs:2171-2274`, `codex-rs/app-server/src/bespoke_event_handling.rs:558-659`.

### 8. Follow-up sampling

| Order | Caller | Function or method | Receiver/type | Inputs | Output or side effect | Next consumer |
|---:|---|---|---|---|---|---|
| 1 | `try_run_sampling_request` | `drain_in_flight` | ordered future queue | tool futures in model emission order | awaits each; converts `ResponseInputItem` to `ResponseItem`; records it | session history + rollout |
| 2 | drain | `Session::record_conversation_items` | `Session` | normalized tool-output item | locks `SessionState`, updates `ContextManager`; persists corresponding rollout items and emits raw-item events | next prompt construction |
| 3 | sample completion | `SamplingRequestResult` | value | `needs_follow_up`, optional last message | tool call, recoverable malformed call, `end_turn=false`, pending input, or hook can require continuation | `run_turn` loop |
| 4 | `run_turn` | loop continuation | same `TurnContext` and current run-segment `ModelClientSession` | updated history; newly captured `StepContext` | can compact first; builds next prompt including paired tool result | `run_sampling_request` |
| 5 | next sample | model transport | same run-segment client session | follow-up request | assistant response or additional tool call | stream loop again |

Invariant: the completed model tool-call item is recorded before execution is launched, and its output is drained/recorded before a follow-up prompt is built. History normalization is the final guard that supplies a legal call/output sequence to the provider.

Evidence: `codex-rs/core/src/session/turn.rs:1893-1916`, `codex-rs/core/src/session/turn.rs:2460-2480`, `codex-rs/core/src/session/mod.rs:2819-2860`, `codex-rs/core/src/context_manager/history.rs:121-162`, `codex-rs/core/src/context_manager/history.rs:359-425`, `codex-rs/core/src/session/turn.rs:291-421`.

### 9. Completion, persistence, app-server projection, and TUI rendering

| Order | Caller | Function or method | Receiver/type | Inputs | Output or side effect | Next consumer |
|---:|---|---|---|---|---|---|
| 1 | final stream item | `handle_output_item_done` | output handler | assistant `ResponseItem` | finalizes/emits `ItemCompleted`, records canonical assistant item and last message | sampling completion |
| 2 | task wrapper | pre-terminal `flush_rollout` | `Session` | accumulated rollout writes | persistence barrier; warning event on failure | `on_task_finished` |
| 3 | task wrapper | `on_task_finished` | `Session` | task result and `TurnContext` | removes `RunningTask`, computes usage/timing, emits `TurnComplete` or `TurnAborted`, clears active turn if still identical | event persistence |
| 4 | session event path | `send_event_raw_with_persistence` | `Session` | core `Event` | converts to rollout items; appends through `LiveThread`; then delivers on unbounded event channel | app-server listener |
| 5 | local store | `LiveThread::append_items` -> local writer | `LiveThread`/`ThreadStore` | ordered `RolloutItem`s | filters/canonicalizes and writes JSONL; local writer flushes before metadata update, preventing SQLite from getting ahead | metadata projection/event delivery |
| 6 | listener task | `CodexThread::next_event` | `CodexThread` | unbounded core event queue | receives persisted core event | `apply_bespoke_event_handling` |
| 7 | projection | bespoke event handling | app-server | `EventMsg::{ItemCompleted,TurnComplete,...}` | converts to v2 `ItemCompleted` and `TurnCompleted` notifications | app-server client event stream |
| 8 | TUI client loop | `handle_server_notification` | `ChatWidget`/app routing | `ServerNotification` | applies deltas; treats completed item as authoritative; clears running status on completion | transcript state |
| 9 | TUI loop | `Tui::draw` | `Tui` | latest app/widget state | `terminal.draw` renders frame/history | user's terminal |
| 10 | completion barrier | second `flush_rollout` | `Session` | terminal event | ensures buffered writers also materialize terminal turn event | durable completed thread |

Persistence ordering is explicit: `Session::send_event_raw_with_persistence` appends first and calls `deliver_event_raw` afterward. Persistence failures are logged and surfaced where explicit flushes fail, but `persist_rollout_items` itself does not make every event send fail. JSONL rollout history is the durable replay stream for the local store; SQLite is a queryable metadata/state projection, not a substitute for ordered rollout replay.

Evidence: `codex-rs/core/src/stream_events_utils.rs:319-411`, `codex-rs/core/src/tasks/mod.rs:400-442`, `codex-rs/core/src/tasks/mod.rs:563-809`, `codex-rs/core/src/session/mod.rs:1947-1992`, `codex-rs/core/src/session/mod.rs:3493-3507`, `codex-rs/thread-store/src/live_thread.rs:152-214`, `codex-rs/thread-store/src/local/live_writer.rs:114-169`, `codex-rs/app-server/src/request_processors/thread_lifecycle.rs:302-350`, `codex-rs/app-server/src/bespoke_event_handling.rs:152-202`, `codex-rs/app-server/src/bespoke_event_handling.rs:940-1002`, `codex-rs/app-server/src/bespoke_event_handling.rs:1230-1246`, `codex-rs/app-server/src/bespoke_event_handling.rs:1374-1383`, `codex-rs/tui/src/chatwidget/protocol.rs:4-85`, `codex-rs/tui/src/chatwidget/protocol.rs:232-339`, `codex-rs/tui/src/tui.rs:880-951`.

## Identifier and correlation ledger

| Identity/object | Created | Representation and copies | Stored/correlated | Final consumer or lifetime |
|---|---|---|---|---|
| Thread ID | `Session::new`; `ThreadId::default()` uses UUIDv7 for new/cleared/forked histories | `ThreadId` wrapper in core; string in v2 JSON | `Session.thread_id`, `CodexThread`, `ThreadManagerState.threads`, `LiveThread`, app-server `Thread`, TUI active/primary IDs | lifetime of durable thread; resume reuses it |
| Session ID | `Session::new` | `SessionId` wrapper; root defaults from thread id; subagents may share agent-control session id; resume may restore persisted id | session metadata and app-server thread snapshot | logical multi-agent/session grouping; not always equal to thread id |
| App-server client request ID | `AppServerSession::next_request_id` (integer counter starting at 1); bootstrap thread helper also uses a UUID string id | JSON-RPC `RequestId`, connection-scoped server side | pending response map/`ConnectionRequestId` | consumed when typed response/error resolves |
| Submission / turn ID | normal `Codex::submit`, `submit_with_trace`, and client-message-ID helpers via `new_submission_id()` UUIDv7 | `Submission.id: String`; copied to `TurnContext.sub_id`; app-server returns it as `Turn.id`; `submit_with_id` instead accepts a supplied ID | active-turn state, events, tracing, TUI thread-event store | one user-visible turn/task, including all its sampling steps |
| Client user-message ID | optional v2 `TurnStartParams.client_user_message_id` | optional string passed through app-server/core | user input history item | de-duplication/correlation supplied by client; TUI normal path currently sends `None` |
| Model response ID | provider `ResponseEvent::Completed` | provider string; held by `ModelClientSession` request state where supported | WebSocket incremental `previous_response_id`/routing logic | transport optimization; not exposed as app-server turn identity |
| Tool call ID | model `ResponseItem::{FunctionCall,CustomToolCall,...}` | string preserved in `ToolCall`, invocation, output and item events | history pairs, approval records, app-server item/request projection, TUI pending approval | ends after paired tool output and UI item resolution; remains in rollout |
| Approval ID / app-server server-request ID | approval ID may derive from call/request; app-server allocates server-request correlation id | core approval fields plus JSON-RPC `RequestId` | oneshot in `TurnState`; app-server outgoing pending server requests; TUI request maps | TUI response resolves server request, then `Op::ExecApproval` resolves core oneshot |
| Cancellation token | `Session::start_task` | root task `CancellationToken`; child tokens passed to task, sample, tool | `RunningTask`; tool runtime selects cancellation/cleanup behavior | cancelled by interrupt/replacement/shutdown; dropped with active task |
| Active task handle | `Session::start_task` | `RunningTask` containing abort-on-drop join handle, `Notify`, task object, token, contexts | `Session.active_turn: Mutex<Option<ActiveTurn>>` | taken on finish/abort; terminal lifecycle emitted exactly for surviving task state |
| Rollout path | `LiveThread::create/resume`, retrieved during `Session::new` | optional `PathBuf` | initial `SessionConfigured`, `CodexThread`, app-server thread record | durable JSONL location for local non-ephemeral thread |
| Persistent thread record | store create/resume and first materializing append | `ThreadStore` metadata + ordered rollout items; local SQLite projection | `LiveThread`, local writer, state DB | list/resume/history reconstruction |

Evidence for ID creation: `codex-rs/protocol/src/thread_id.rs:16-62`, `codex-rs/protocol/src/session_id.rs:15-73`, `codex-rs/core/src/session/session.rs:507-548`, `codex-rs/core/src/session/mod.rs:746-796`, `codex-rs/core/src/session/mod.rs:909-911`, `codex-rs/tui/src/app_server_session.rs:175-231`, `codex-rs/tui/src/app_server_session.rs:1184-1188`.

## Concurrency, locks, channels, and ordering boundaries

| Boundary | Mechanism | What it guarantees | Important caveat |
|---|---|---|---|
| TUI event loop | Tokio unbounded `AppEvent` channel plus `select!` | UI events, server notifications and redraws converge on app state | app-server events have separate lossless/best-effort classification; lag is surfaced |
| Embedded app-server façade | bounded command and event `mpsc`; worker task | same typed JSON-RPC envelope as remote path, backpressure at façade | an overloaded server request is rejected rather than silently dropped |
| Remote client | bounded command channel, remote transport/event task | process/host boundary with same app-server protocol | disconnection is an explicit `AppServerEvent::Disconnected` |
| Core operation queue | bounded async channel, capacity 512 | one submission loop serially dispatches `Op`s | task execution is spawned, so approvals/interruption can be accepted while it runs |
| Core event queue | unbounded async channel | producers are not blocked by TUI/app-server slowness | downstream app-server client queues still enforce delivery policy |
| Thread registry | `Arc<RwLock<HashMap<ThreadId, Arc<CodexThread>>>>` | concurrent lookup, exclusive insert/remove | `finalize_thread_spawn` shuts down a duplicate spawned runtime |
| Session mutable state | Tokio mutexes for `SessionState`, `active_turn`, and `TurnState` | explicit thread/session/turn mutation boundaries | locks are intentionally released before long I/O where the critical functions show scoped blocks |
| One active task | `active_turn` mutex and `RunningTask` | `spawn_task` aborts old tasks; debug assertions require no task at install | pending steer/input can remain associated with the active turn |
| Model sampling | one `ModelClientSession` per `run_turn` invocation, reused for its samples/retries | sticky WebSocket/routing state does not leak into another model-turn invocation; a `RegularTask` can re-enter `run_turn` | each outer sampling iteration captures a fresh `StepContext`; retries do not |
| Tool admission | `Arc<RwLock<()>>` | read = parallel-supported calls; write = serialization barrier | ordered history is separately guaranteed by `FuturesOrdered` |
| Approval wait | oneshot sender in `TurnState` | request is registered before event emission, avoiding a response race | missing sender is treated as abort, not implicit approval |
| Persistence | `LiveThread` internal metadata mutex and store writer | ordered append and flush; local JSONL reaches disk before SQLite metadata advances | `persist_rollout_items` logs append failures; explicit flush creates user-visible warning barrier |

## Critical-function walkthroughs

The index identifies the functions whose blocks jointly cover the canonical path. “Omitted” means the branch exists but is outside the selected scenario; it is not evidence that the branch is unimportant.

| Function | Why critical | Direct caller | Downstream consumer | Detailed block |
|---|---|---|---|---|
| `arg0_dispatch_or_else` | creates the actual async runtime | CLI `main` | `cli_main` | A |
| `cli_main` / `run_interactive_tui` | selects interactive mode and resolves launch failures | arg0 root future | TUI `run_main` | B |
| `run_main` / `run_ratatui_app` | selects app-server topology and constructs TUI runtime | CLI | `start_app_server`, `App::run` | C |
| `thread_start_inner` / `thread_start_task` | JSON-RPC-to-core thread construction | message processor | `ThreadManager` | D |
| `spawn_thread_with_source` / `finalize_thread_spawn` | creates/reuses and registers live core thread | app-server or other core clients | `CodexThread` | E |
| `Codex::spawn_internal` | creates channels, resolves thread-static configuration, launches dispatcher | manager state | `Session::new`, `submission_loop` | F |
| `Session::new` | assigns identity and builds persistence/services/mutable state | `Codex::spawn_internal` | active `Session` | G |
| `turn_start_inner` / TUI route | converts UI user turn to core operation | TUI/app-server | core submission queue | H |
| `submission_loop` / `user_input_or_turn_inner` | operation dispatch and active-turn decision | `Codex` queue | `RegularTask` | I |
| `Session::start_task` / `RegularTask::run` | owns task lifecycle and terminal event boundary | input handler | `run_turn` | J |
| `run_turn` | owns multi-sample user-visible turn loop | `RegularTask` | sampling wrapper | K |
| `run_sampling_request` / `try_run_sampling_request` | builds/retries request and consumes stream | `run_turn` | model/tool handlers | L |
| `handle_output_item_done` / `ToolCallRuntime::handle_tool_call` | turns model item into concurrent tool work | stream loop | router/registry | M |
| `ToolOrchestrator::run` / approval path | policy, approval, sandbox and escalation | concrete tool handler | execution runtime | N |
| `on_task_finished` / `send_event_raw_with_persistence` | terminal lifecycle and durable-before-visible event ordering | spawned task | app-server listener | O |
| `apply_bespoke_event_handling` / TUI notification handler | protocol projection and rendering state | listener | Ratatui draw | P |

### A. `arg0_dispatch_or_else`

Source: `codex-rs/arg0/src/lib.rs:208-280`.

1. `arg0_dispatch()` first handles helper-name (`argv[0]`) dispatch and retains a temp-directory guard whose aliases must outlive the invocation.
2. It records `current_exe`, then builds a named `codex-main` OS thread with the same 16 MiB stack budget as Tokio workers. This prevents the root future from running on the caller's smaller OS stack.
3. Inside that thread, `build_runtime()` constructs a Tokio multi-thread runtime, enables I/O/time drivers, and applies the worker stack size.
4. `runtime.block_on(run_main_with_arg0_guard(...))` creates helper paths and awaits the CLI future.
5. The guard is dropped only after the async entry point returns, so any sandbox/exec-wrapper alias remains valid.
6. Join success returns the result; a panic payload is resumed on the original thread.

Omitted: the individual helper executable dispatch implementations and dotenv filtering, because the no-subcommand conversation re-enters the ordinary CLI branch after the shared bootstrap.

### B. `cli_main` and `run_interactive_tui`

Sources: `codex-rs/cli/src/main.rs:964-1030`, `codex-rs/cli/src/main.rs:2236-2305`.

1. Clap parses private `MultitoolCli`; feature toggles are folded into raw config overrides so every command sees them.
2. The `None` subcommand branch prepends root overrides to interactive overrides and awaits `run_interactive_tui`.
3. The interactive wrapper normalizes CRLF in a CLI-supplied prompt and rejects a noninteractive dumb terminal; an interactive dumb terminal requires confirmation.
4. It resolves explicit remote endpoint/auth arguments. Usage errors become a fatal TUI exit result; other I/O errors propagate.
5. It repeatedly invokes `codex_tui::run_main`. Normal success returns `AppExitInfo`.
6. On a local state-DB startup error it distinguishes locked, nonrecoverable, already-attempted, and auto-backup-recoverable cases. Only the recoverable case backs up and retries.

Omitted: concrete subcommand branches (`exec`, `review`, MCP, daemon, completion, and others) because `subcommand == None` is the canonical launch.

### C. `run_main` and `run_ratatui_app`

Sources: `codex-rs/tui/src/lib.rs:848-940`, `codex-rs/tui/src/lib.rs:1237-1335`, `codex-rs/tui/src/lib.rs:1740-1765`.

1. `run_main` resolves dangerous-mode overrides first, parses `-c` values, locates `CODEX_HOME`, and computes whether all launch overrides can be replayed by an existing daemon.
2. It probes the default daemon socket only when no explicit remote was supplied and reuse is safe.
3. `app_server_target_for_launch` gives explicit remote highest precedence, then local daemon, else embedded.
4. Workspace/config paths are chosen using `uses_remote_workspace`; an explicit remote can carry a remote cwd override, while local targets resolve local environments.
5. Configuration, logging/state layers, and terminal prerequisites are initialized before `run_ratatui_app`.
6. `run_ratatui_app` installs color-eyre/panic handling, enters raw terminal mode, and starts the selected app-server.
7. Start failure restores the terminal before returning. Success wraps the client in `AppServerSession`, preserving `ThreadParamsMode`.
8. It performs new/resume/fork bootstrap and finally awaits `App::run`; the app owns the event/select/render loop.

Omitted: update prompts, OSS bootstrap details, onboarding/auth screens, and exit-time feedback because they bracket but do not change the conversation call chain.

### D. `thread_start_inner` and `thread_start_task`

Sources: `codex-rs/app-server/src/request_processors/thread_processor.rs:930-1039`, `codex-rs/app-server/src/request_processors/thread_processor.rs:1089-1371`.

1. `thread_start_inner` destructures v2 parameters. It rejects paginated history (“not supported yet”) and rejects simultaneous legacy `sandbox` plus named `permissions`.
2. It resolves environment selections and converts protocol fields to type-safe config overrides.
3. It clones only the services required by a detached listener/start task, including the outgoing connection handle and request trace.
4. It spawns `thread_start_task` into a tracked background-task set and returns immediately; failures are converted to JSON-RPC errors on the original request id.
5. The task loads effective configuration. If explicit/effective permissions imply project trust, it persists or overlays trust and reloads config, so the final permission profile is authoritative.
6. It emits unique exec-policy warnings, resolves default environments, validates dynamic tools, and builds extension initialization.
7. `ThreadManager::start_thread_with_options` performs core construction; invalid/unsupported/core errors map to suitable JSON-RPC errors.
8. The app-server attaches a thread listener before sending the response, updates watched thread state, snapshots effective configuration, sends `ThreadStartResponse`, then broadcasts `ThreadStarted`.

Omitted: detailed trust-key serialization and every response field mapping; these are repetitive mapping code outside the object-lifecycle backbone.

### E. `spawn_thread_with_source` and `finalize_thread_spawn`

Source: `codex-rs/core/src/thread_manager.rs:1519-1668`.

1. Resume first takes the thread-map write lock. A matching running thread with the same rollout path is returned; a mismatched path is an invalid request; a stale nonrunning entry is removed.
2. Parent instructions, trace ancestry, multi-agent version, and originator are derived without holding the map lock.
3. The manager passes its shared services and normalized source/history/options into crate-private `Codex::spawn`.
4. `finalize_thread_spawn` awaits exactly one event from the new `Codex` and requires the first event to be `SessionConfigured` under `INITIAL_SUBMIT_ID`; any other ordering is a construction failure.
5. It takes the map write lock and inserts an `Arc<CodexThread>` only into a vacant entry.
6. If another spawn won the race, it shuts down the duplicate `Codex` and returns an error rather than leaking a second runtime.
7. Resumed threads emit an explicit resume lifecycle only after successful finalization.

Invariant: `ThreadManagerState` owns the live-thread registry; `CodexThread` is the stable public handle stored there; `Codex` itself is the queue/session façade hidden inside that handle.

### F. `Codex::spawn_internal`

Source: `codex-rs/core/src/session/mod.rs:500-740`.

1. It creates a bounded submission channel (`SUBMISSION_CHANNEL_CAPACITY == 512`) and an unbounded event channel.
2. It incorporates instruction-loading warnings and selects/inherits/loads the execution policy. Guardian-review sources deliberately ignore caller exec-policy rules.
3. It freezes config in an `Arc`, refreshes model metadata according to root/subagent strategy, chooses the effective model, and records a provider fallback when allowed.
4. It resolves base instructions in this order: explicit config, resumed session metadata, current model defaults. Dynamic tools likewise come from start args or resumed history.
5. It constructs `SessionConfiguration`, including provider, collaboration/model settings, permission state, environment selections, source/parent IDs, history mode and tools.
6. `Session::new` is awaited. Initialization errors are mapped with `CODEX_HOME` context; optional warnings are sent as initial events.
7. It clones `Arc<Session>` into a `tokio::spawn` running `submission_loop` until `Op::Shutdown`.
8. Returned `Codex` owns the submission sender, event receiver, agent-status watch receiver, session arc and a shared termination future.

Omitted: every optional subsystem's constructor field; the type map documents service ownership, while the canonical path needs only the channel, model/config, persistence, and task services.

### G. `Session::new`

Source: `codex-rs/core/src/session/session.rs:480-820` (construction continues beyond the displayed initialization futures).

1. It normalizes fork/parent fields from explicit configuration and initial history.
2. New/cleared/forked histories get a UUIDv7 `ThreadId`; resume reuses `conversation_id`.
3. It restores a persisted `SessionId` when valid. Root sessions otherwise derive it from the thread id; non-root agents use the shared `AgentControl` session id. This is why “thread equals session” is not a general invariant.
4. Persistence, state-DB discovery, and auth/MCP projection are independent futures joined with `tokio::join!` to reduce startup latency.
5. Non-ephemeral new histories call `LiveThread::create`; resumed histories call `LiveThread::resume` with history/path. Ephemeral sessions use no live thread.
6. `LiveThreadInitGuard` owns cleanup until complete session construction succeeds, preventing a half-initialized persistent thread from being left live.
7. It builds long-lived `SessionServices` (with locks/atomic swaps), mutable `SessionState`, `InputQueue`, `Mutex<Option<ActiveTurn>>`, model client, extension data, and status/event senders in one `Arc<Session>`.
8. It persists/emits startup metadata and `SessionConfigured` as the first externally consumed core event, followed by warnings/MCP readiness as applicable.

Omitted: exhaustive MCP authentication status calculation, shell discovery, startup prewarming, every service field, and all warning variants. They run during initialization but do not change identity or the canonical submission path.

### H. TUI route and app-server `turn_start_inner`

Sources: `codex-rs/tui/src/app/thread_routing.rs:517-676`, `codex-rs/app-server/src/request_processors/turn_processor.rs:443-549`.

1. For `AppCommand::UserTurn`, TUI first checks its cached active-turn id and tries `turn/steer`.
2. A “missing active turn” race clears cached state and falls through to start. One expected-turn mismatch can update the cached id and retry. A non-steerable active turn is queued/reported in UI rather than silently converted.
3. Idle path derives effective permission overrides from current config and sends `turn/start` with model, effort, cwd, workspace roots and input.
4. App-server loads the `CodexThread`, rejects input forbidden by source or size, records client identity/capability, and resolves environments.
5. It converts each v2 `UserInput` into core input, resolves sticky per-turn settings, and builds `Op::UserInput`.
6. `CodexThread::submit_user_input_with_client_user_message_id` returns the generated submission id; app-server returns it as the v2 turn id.

Omitted: memory-startup side task after nonempty input and all v2 optional field mappings; neither changes the submission/task identity chain.

### I. `submission_loop` and `user_input_or_turn_inner`

Sources: `codex-rs/core/src/session/handlers.rs:714-855`, `codex-rs/core/src/session/handlers.rs:194-285`.

1. The loop serially receives `Submission` values until channel closure or a successful `Op::Shutdown` handler.
2. `Interrupt`, approval responses, settings, compact, rollback and other operations are dispatched while a task may be running; this is why the task itself is spawned rather than awaited in the submission loop.
3. `Op::UserInput` calls `user_input_or_turn`, then the inner handler destructures only that exact variant.
4. It applies nondefault thread-setting overrides and creates `Arc<TurnContext>` with the submission id. Failure is already emitted by `new_turn_with_sub_id`.
5. It emits a non-materializing settings-applied event where needed and warnings for the effective model.
6. It first calls `Session::steer_input`. Success adds input to the running turn and records telemetry.
7. `NoActiveTurn` merges additional context under the `SessionState` mutex, constructs `TurnInput`s, refreshes requested MCP state, and spawns a `RegularTask`.
8. Other steering errors become core error events correlated to the submission id.

Omitted: realtime audio/text operations, review tasks, shell-command tasks, rollback and extension operations. The dispatch table remains the navigation point for those alternatives.

### J. `Session::start_task` and `RegularTask::run`

Sources: `codex-rs/core/src/tasks/mod.rs:314-449`, `codex-rs/core/src/tasks/regular.rs:37-90`, `codex-rs/core/src/tasks/mod.rs:563-809`.

1. Public-in-crate `spawn_task` aborts all existing tasks as `Replaced`, clears connector selection, then enters `start_task`.
2. `start_task` erases the task behind `Arc<dyn AnySessionTask>`, marks timing, snapshots starting usage, and creates the root cancellation token plus completion `Notify`.
3. It clears the per-turn guardian circuit breaker, moves pending queue items into the selected `TurnState`, and emits turn-start lifecycle.
4. Under the `active_turn` mutex it requires `turn.task.is_none()`, obtains an agent execution guard, and constructs `SessionTaskContext`.
5. A spawned async block runs the task with child cancellation. Before terminal completion it flushes rollout; failure emits a warning. If cancellation has not won, it calls `on_task_finished`; then it notifies waiters.
6. `RunningTask` is installed only after spawning and owns the abort-on-drop handle, cancellation root, turn/extension contexts and execution/timer guards.
7. `RegularTask::run` emits `TurnStarted` immediately, resolves optional startup-prewarmed `ModelClientSession`, then calls `run_turn`.
8. If pending input remains after `run_turn`, it loops with empty explicit input under the same task and `TurnContext`; otherwise it returns the last message.
9. `on_task_finished` atomically takes the installed task, accounts pending input/usage/timing, emits `TurnComplete` or `TurnAborted`, conditionally clears the exact matching active turn, and performs the post-terminal flush.

Invariant: there is at most one installed running task for a session. Replacement first takes and cancels the old `ActiveTurn`; the cancelled task skips normal `on_task_finished`, and the final `TurnState` pointer check prevents cleanup from clearing a different task-less active state.

### K. `run_turn`

Source: `codex-rs/core/src/session/turn.rs:143-433`.

1. It creates or consumes one `ModelClientSession` local to this `run_turn` invocation; pre-sampling compaction aborts on cancellation, while other compaction errors emit terminal error lifecycle and stop the turn cleanly.
2. It captures the first `StepContext`, concurrently records context/world changes and calculates diff roots, then loads skill/plugin injection. Hook cancellation/stop returns early.
3. It records explicit input and injections, freezes previous-turn settings, and creates one turn-scoped diff tracker.
4. On each loop, pending steering input is drained only when safe. Hooks and budget/time reminders can record additional context.
5. It reuses the first step snapshot once, then captures a new `StepContext` for each later sampling request. History is cloned and normalized without holding session state across network I/O.
6. `run_sampling_request` returns `needs_follow_up` and last assistant text. Pending input is ORed into continuation state.
7. If continuation is required and context pressure/new-window request is present, auto-compaction runs. Cancellation propagates; other failure emits terminal error and ends. The loop then continues.
8. With no follow-up, stop hooks may inject a continuation prompt (loop), force stop, or allow legacy after-agent hook before break.
9. `TurnAborted` propagates to task lifecycle. Invalid-image failure tries history sanitization and retries once possible; other errors emit protocol error plus UI error and let the user continue later.

Omitted: implementation details of world-state diffing, skill/plugin expansion, hook bodies, token-budget math and plan-mode content parsing. Their call sites and continuation effects are included.

### L. `run_sampling_request` and `try_run_sampling_request`

Sources: `codex-rs/core/src/session/turn.rs:1085-1218`, `codex-rs/core/src/session/turn.rs:1893-2480`.

1. The wrapper builds one `ToolRouter` from the `StepContext`, reads current base instructions, and constructs cloneable `ToolCallRuntime` with the same step snapshot.
2. It reuses initial input for attempt one. Retry attempts clone current normalized history, so tool/events recorded during recovery are not lost.
3. `build_prompt` copies model-visible specs, parallel support and output schema into `Prompt`.
4. Retry budget comes from `ModelProviderInfo`. Context-window and usage-limit errors have dedicated state updates/returns; retryable transport failures back off and retry until the provider budget is exhausted.
5. `try_run_sampling_request` starts timing/tracing and calls `ModelClientSession::stream` under a child cancellation token.
6. The stream loop distinguishes cancellation, provider error, premature EOF, partial item/delta, completed item, usage/rate metadata, and `Completed`.
7. Completed tool items produce futures without blocking stream consumption. `FuturesOrdered` retains model order.
8. `Completed` records token usage, makes `end_turn == false` a continuation, and exits. After the stream outcome, all in-flight tools are drained before returning.

Omitted: each telemetry field, plan/review-specific stream transformation, and all metadata-only `ResponseEvent` variants. Their state effects are localized and do not alter normal tool-follow-up ordering.

### M. `handle_output_item_done` and `ToolCallRuntime::handle_tool_call`

Sources: `codex-rs/core/src/stream_events_utils.rs:319-411`, `codex-rs/core/src/tools/parallel.rs:75-199`.

1. The output handler asks `ToolRouter::build_tool_call` whether a response item is a recognized call.
2. A valid tool call is recorded as model output before execution, and a boxed future invoking `ToolCallRuntime` is returned with `needs_follow_up = true`.
3. A non-tool assistant/reasoning item is finalized through turn-item contribution logic and recorded; assistant text can become `last_agent_message`.
4. A recoverable malformed call is converted to a model-visible failure output and requests a follow-up; unrecoverable errors propagate.
5. `ToolCallRuntime` clones router/session/step/tracker and spawns a dispatch task. It computes parallel support before admission.
6. Its `tokio::select!` races cancellation with acquisition/execution. Parallel calls take a read lock, serial calls a write lock.
7. Cancellation either awaits runtime cleanup for tools that require it or aborts the dispatch task; cancellation errors are normalized rather than double-emitting lifecycle.
8. Completed registry results are converted to the provider-specific `ResponseInputItem`, retaining call id.

Omitted: code-mode diff-consumer formatting and every response-item variant conversion. The critical distinction is valid tool, ordinary content, recoverable malformed call, or hard error.

### N. `ToolOrchestrator::run` and approval path

Sources: `codex-rs/core/src/tools/orchestrator.rs:137-374`, `codex-rs/core/src/tools/approvals.rs:137-258`, `codex-rs/core/src/session/mod.rs:2171-2274`, `codex-rs/app-server/src/bespoke_event_handling.rs:558-659`.

1. The concrete runtime supplies an `ExecApprovalRequirement`. `Skip` can proceed; `Forbidden` returns a normalized refusal; `NeedsApproval` enters reviewer resolution.
2. Approval resolution considers permission hooks, configured guardian/user reviewer and session approval cache. It never treats absence of a response as approval.
3. `Session::request_command_approval` creates a oneshot pair, inserts the sender into `TurnState` under lock, emits the request, and awaits the receiver.
4. The app-server projection allocates/sends a server request and spawns a waiter so its thread event listener stays live. The TUI resolves that JSON-RPC request id.
5. App-server maps the v2 answer to core `ReviewDecision` and submits `Op::ExecApproval`; the serial submission loop removes and sends through the stored oneshot.
6. Orchestration selects a sandbox attempt and calls the runtime. A sandbox denial can cause a policy-permitted approval/escalation retry; denial restrictions (including read/network constraints) still apply.
7. Execution success or failure is normalized into tool output and lifecycle/metrics, never thrown away as an unpaired call.

Omitted: patch approval, elicitation, dynamic-tool and `request_user_input` server-request schemas; they reuse similar projection/wait patterns but are not the canonical shell approval.

### O. `on_task_finished` and `send_event_raw_with_persistence`

Sources: `codex-rs/core/src/tasks/mod.rs:563-809`, `codex-rs/core/src/session/mod.rs:1947-1992`, `codex-rs/core/src/session/mod.rs:3493-3507`.

1. Task result maps `TurnAborted` to explicit abort reason; unexpected errors are logged but terminal cleanup still runs.
2. Under `active_turn` lock it takes the `RunningTask`, detaches its already-current handle, and captures `TurnState`. Missing state makes a stale completion a no-op.
3. It moves/records pending inputs, computes per-turn tool and token metrics, timing and terminal error state.
4. It emits either `TurnAborted` or `TurnComplete` using the same turn/submission id.
5. `send_event_raw_with_persistence` converts persistable event messages to rollout items, awaits `persist_rollout_items`, then traces/delivers the event. This is the durable-before-visible ordering point.
6. It clears the guardian breaker and takes `active_turn` only if it still refers to the same task-less `TurnState`; then it emits thread-idle lifecycle.
7. A final explicit flush makes the terminal event durable even for buffering stores.

Failure boundary: append errors in `persist_rollout_items` are logged; explicit flush failures generate a warning before completion and are logged after the terminal event. The runtime continues rather than silently claiming persistence succeeded.

### P. App-server projection and TUI handling

Sources: `codex-rs/app-server/src/request_processors/thread_lifecycle.rs:302-350`, `codex-rs/app-server/src/bespoke_event_handling.rs:136-202`, `codex-rs/app-server/src/bespoke_event_handling.rs:940-1002`, `codex-rs/tui/src/chatwidget/protocol.rs:4-85`, `codex-rs/tui/src/chatwidget/protocol.rs:232-339`, `codex-rs/tui/src/tui.rs:880-951`.

1. The per-thread listener awaits `CodexThread::next_event`. A receive failure logs and terminates that listener.
2. `apply_bespoke_event_handling` translates core lifecycle/item events to stable v2 notifications and handles request/response protocols that cannot be a one-way notification.
3. `TurnStarted` resets stale pending server requests and publishes the active turn. `ItemStarted`, deltas and `ItemCompleted` provide streaming plus authoritative final item state. `TurnComplete` becomes `TurnCompleted` with terminal status/error.
4. The app-server client marks transcript/terminal notifications as lossless; noncritical queue pressure can surface `Lagged` rather than corrupting the transcript silently.
5. TUI `handle_server_notification` routes by notification kind. It sets active/running state on start, applies deltas, consolidates the completed assistant item, and clears working state on completion.
6. The outer TUI loop schedules `Tui::draw`; `terminal.draw` renders the latest transcript and bottom-pane state.

Omitted: mappings for every event type and inactive-thread buffering/replay. Those are adjacent projection behaviors; the canonical final answer uses item delta/completion plus turn completion.

## Alternative and failure paths that affect the normal model

| Path | Divergence | Rejoin/terminal behavior | Evidence |
|---|---|---|---|
| Resume | `ThreadManagerState` first reuses a matching running thread; otherwise `Session::new` reuses persisted IDs/history and `LiveThread::resume` | emits resume lifecycle after registration; future input follows same submission path | `thread_manager.rs:1519-1633`, `session/session.rs:507-629` |
| Fork | initial history and parent/source/trace fields differ | gets a new thread id and configured inherited history, then same runtime | `thread_manager.rs:730-800`, `session/session.rs:490-548` |
| Steer while active | TUI uses `turn/steer`; core `user_input_or_turn_inner` also tries `Session::steer_input` defensively | input is queued/drained into same active `TurnContext`; no second task is created | `tui/src/app/thread_routing.rs:571-646`, `session/handlers.rs:224-285`, `session/turn.rs:236-276` |
| Interrupt | v2 `turn/interrupt` maps to core interrupt operation/cancellation | task token is cancelled; tool cleanup policy applies; terminal `TurnAborted` is emitted | `session/handlers.rs:726-729`, `tasks/mod.rs:811-875`, `tools/parallel.rs:162-190` |
| Task replacement | `spawn_task` calls `abort_all_tasks(Replaced)` | old task cannot clear new `TurnState`; new task installs normally | `tasks/mod.rs:314-323`, `tasks/mod.rs:492-561` |
| Model retry | wrapper retains one `ModelClientSession`, reconstructs prompt after first attempt | retries to provider budget; usage/context/auth have special handling | `session/turn.rs:1113-1218`, `client.rs:1395-1819` |
| WebSocket fallback | provider capability selects WebSocket; transport failure can make session use HTTP | same `ResponseEvent` abstraction feeds stream loop | `client.rs:1523-1819`, `model-provider-info/src/lib.rs:329-363` |
| Mid-turn compaction | continuation plus context threshold/new-window request calls `run_auto_compact` | continuation resumes before unsafe pending-input drain | `session/turn.rs:347-382` |
| Malformed tool call | output handler distinguishes recoverable function-call error | records model-visible error output and follows up; hard errors abort sampling | `stream_events_utils.rs:319-411` |
| Sandbox denial | orchestrator can request approval/escalation only when policy permits | retries in selected sandbox or returns normalized failure output | `tools/orchestrator.rs:137-374` |
| Approval decline/cancel | oneshot resolves a non-approved `ReviewDecision` | tool returns refusal/cancellation output; model may receive it for follow-up | `tools/approvals.rs:180-258`, `session/mod.rs:2171-2274` |
| Stream EOF/error | EOF before `response.completed` is a stream error; cancellation is `TurnAborted` | wrapper may retry; exhausted error is emitted to UI and turn completes/aborts consistently | `session/turn.rs:2017-2044`, `session/turn.rs:421-464` |
| Persistence failure | append errors log; preterminal flush warns; postterminal flush logs | conversation runtime remains usable, but UI receives an explicit save warning when barrier fails | `session/mod.rs:3493-3507`, `tasks/mod.rs:400-442`, `tasks/mod.rs:803-809` |
| App-server lag/disconnect | lossless events are backpressured; best-effort loss produces `Lagged`; remote disconnect is explicit | TUI warns/terminates according to event kind; terminal transcript events are protected | `app-server-client/src/lib.rs:96-143`, `app-server-client/src/remote.rs:151-250` |

## Tests inspected

These were read as evidence. “Inspected” does not mean executed.

| Boundary | Test | What it establishes |
|---|---|---|
| Target selection | `codex-rs/tui/src/lib.rs:2208-2308` target-selection unit tests | default daemon, explicit remote precedence, and non-replayable override behavior |
| Embedded app-server | `codex-rs/tui/src/lib.rs:2741-2797` | in-process target supports typed `thread/start` RPC |
| New thread | `codex-rs/app-server/tests/suite/v2/thread_start.rs:316-390` `thread_start_creates_thread_and_emits_started` | response/notification and lazy rollout materialization |
| Empty turn | `codex-rs/app-server/tests/suite/v2/turn_start.rs:221-286` | `turn/start` can run a model request and complete even with empty input |
| Tool + follow-up | `codex-rs/app-server/tests/suite/v2/turn_start.rs:1003-1114` `turn_profile_tracks_blocking_tool_and_follow_up_sampling` | blocking tool spans two sampling requests and still produces one turn profile |
| Approval + follow-up | `codex-rs/app-server/tests/suite/v2/turn_start.rs:2116-2284` | v2 exec server request, approval resolution, and second model request |
| TUI startup queue | `codex-rs/tui/src/app/tests/startup.rs:139-181` | prompt queued before startup thread creation is submitted after start |
| Final TUI state | `codex-rs/tui/src/chatwidget/tests/app_server.rs:521-588` | authoritative answer item renders and `TurnCompleted` clears working status |
| Output handler | `codex-rs/core/src/stream_events_utils_tests.rs:271-330` | completed assistant output contributes last agent message |
| Tool mailbox continuation | `codex-rs/core/src/session/tests.rs:10380-10490` | tool calls reopen current-turn mailbox delivery/follow-up behavior |
| Tool routing | `codex-rs/core/src/tools/router_tests.rs:107-420` | call parsing, kind validation, dispatch and normalized failures |
| Tool cancellation | `codex-rs/core/src/tools/parallel.rs:408-801` | cancellation before admission, after completion, and while runtime cleanup waits |
| Persistence projection | `codex-rs/thread-store/src/local/mod.rs:442-565` | `LiveThread` append/flush updates SQLite metadata from rollout items |
| Buffered shutdown | `codex-rs/thread-store/src/local/mod.rs:642-683` | shutdown materializes buffered items before metadata read |
| Resume | `codex-rs/thread-store/src/local/mod.rs:686-735`, `:851-885` | history is loaded before observation; resumed writer appends |
| Rollout/state sync | `codex-rs/rollout/src/state_db_tests.rs:50-141` | rollout metadata/backfill behavior into state DB |

## Tests executed

The coordinated research run executed four focused tests with the repository-required `just test`/nextest wrapper. All passed:

```text
rtk just test -p codex-tui app_server_target_for_launch_prefers_explicit_remote_endpoint
  PASS — 1 passed, 3047 skipped

rtk just test -p codex-thread-store resume_thread_reopens_live_writer_and_appends
  PASS — 1 passed, 91 skipped

rtk just test -p codex-app-server turn_profile_tracks_blocking_tool_and_follow_up_sampling
  PASS — 1 passed, 951 skipped

rtk just test -p codex-tui live_app_server_turn_completed_clears_working_status_after_answer_item
  PASS — 1 passed, 3047 skipped
```

Nextest warned that `profile.local.inherits` is an unknown configuration key in `codex-rs/.config/nextest.toml`; it did not prevent test discovery or execution. The complete workspace suite was deliberately not run.

## Semantic-navigation record

Rust Analyzer was initialized against the Cargo-metadata workspace root `/Users/bytedance/project/harness-engine/codex/codex-rs` using the lesson research artifact at `docs/codex-dev-lesson/research/ra_initialize_params.json`.

Successful queries:

- workspace symbols resolved `ThreadManager`, `CodexThread`, and `ToolCallRuntime`;
- incoming calls for `run_turn` identified the production caller `RegularTask::run`;
- incoming calls for `handle_output_item_done` identified `try_run_sampling_request` plus focused tests;
- incoming calls for `ToolCallRuntime::handle_tool_call` identified the output handler and cancellation tests;
- incoming calls for `start_thread_with_options` showed app-server construction plus convenience/test/memory callers.

Limitations and failures:

- Two early commands used unsupported `--file`; the corrected CLI form uses `-p`. They were not repeated unchanged.
- `incoming-calls` on TUI `AppServerSession::turn_start` returned no results, likely because the requested position/range did not select the method symbol. Text search and direct caller inspection establish its canonical caller.
- One attempt used a nonexistent `get-precise-range` subcommand. It was not retried; direct source inspection supplied the range.
- `run_turn` outgoing-call output was too large and truncated. The useful edges (model session, compaction, step context, skills/hooks) were corroborated by direct inspection. Therefore the exhaustive outgoing-call graph remains **Uncertain**, while the critical call chain in this page is confirmed directly.
- `start_thread_with_options` precise-range selection sometimes returned the enclosing `impl`, so its incoming-call result was treated as discovery only and every cited production caller was verified textually.

## Documentation drift found during tracing

| Existing statement or implication | Current implementation | Status/evidence |
|---|---|---|
| OpenWiki core-concepts names core input operation `Op::UserTurn` | core has `Op::UserInput`; only private TUI `AppCommand` has `UserTurn` | **Confirmed — code:** `protocol/src/protocol.rs:528-581`, `tui/src/app_command.rs:26-70` |
| Protocol narrative treats a turn as one model/tool cycle | one `RegularTask`/`TurnContext` can execute multiple model sampling steps | **Confirmed — code/test:** `tasks/regular.rs:67-94`, `session/turn.rs:236-421`; app-server follow-up test at `turn_start.rs:1003` |
| High-level diagrams suggest the TUI can call core directly | current TUI always uses app-server protocol, including an in-process client | **Confirmed — code:** `tui/src/lib.rs:1237-1335`, `app-server-client/src/lib.rs:431-475` |
| State-runtime comments imply rollout backfill orchestration lives under state | concrete orchestration is in the `codex-rollout` crate's `state_db.rs`/`metadata.rs`; `StateRuntime` owns SQLite runtimes/stores | **Confirmed — code:** `state/src/runtime.rs:166-290`, `rollout/src/state_db.rs:148-289`, `rollout/src/metadata.rs:163-270` |
| Memory README says write orchestration was removed from core | current memory startup/write flow is in `codex-memories-write` and app-server calls it after turn start | **Confirmed — code:** `app-server/src/request_processors/turn_processor.rs:550-575`, `memories/write/src/lib.rs` |
| Some references point to `codex-mcp/src/mcp_connection_manager.rs` | current file is `codex-mcp/src/connection_manager.rs` | **Confirmed — code:** repository file map |
| Protocol exposes paginated thread history mode | `thread/start` currently rejects it with method-not-found | **Confirmed — code:** `thread_processor.rs:967-973` |
| Older feature-flag descriptions govern response WebSocket transport | provider capability now chooses WebSocket; removed legacy flags are not the active switch | **Confirmed — code:** `client.rs:1774-1819`, `model-provider-info/src/lib.rs:329-363` |
| Thread and session identity are often presented as interchangeable | equal for a fresh root thread by default, but resume/subagent rules can diverge | **Confirmed — code:** `session/session.rs:507-548` |

## Unresolved evidence gaps

1. **Remote-daemon deployment details:** the protocol/client path is confirmed, but this trace did not inspect every daemon startup, authentication, reconnect, and socket-framing branch. Those do not alter the core turn/task/sampling ownership model.
2. **Provider-specific wire nuances:** the Responses request builder and HTTP/WebSocket selection were inspected, but not every provider implementation or every retry header. Claims here are limited to the shared current path.
3. **All tool families:** shell approval/sandbox execution was traced end to end. MCP, dynamic tools, web tools, collaboration tools and code mode share the router/registry boundary but have handler-specific policy and result rules not exhaustively mapped here.
4. **Every event projection:** start/delta/completed/approval/terminal events were traced. The large bespoke event match contains many unrelated notifications; no claim of exhaustive event coverage is made.
5. **Motivation:** where this page says an abstraction “keeps” or “prevents” an invariant, that is grounded in code structure or comments. Broader design history is unavailable and should be treated as interpretation, not author intent.

## Navigation takeaway

For most conversation-runtime changes, start at the boundary that owns the behavior:

- launch or target selection: `tui/src/lib.rs`;
- JSON-RPC thread/turn behavior: `app-server/src/request_processors/` and `app-server-protocol/src/protocol/v2/`;
- live-thread ownership: `core/src/thread_manager.rs`, `core/src/codex_thread.rs`, `core/src/session/`;
- user-visible turn/task lifecycle: `core/src/session/handlers.rs`, `core/src/tasks/`, `core/src/session/turn.rs`;
- request/stream behavior: `core/src/client.rs`, `codex-api/src/common.rs`;
- tool policy/execution: `core/src/tools/`;
- durable replay: `thread-store/`, `rollout/`, and core session persistence methods;
- final UI behavior: app-server bespoke projection, then `tui/src/chatwidget/protocol.rs` and transcript rendering.
