# codex-exec crate analysis

## Overview

The `codex-exec` crate is the non-interactive execution client for Codex. It packages:

- a binary entrypoint that parses CLI arguments and dispatches either normal exec behavior or arg0-based sandbox behavior via [main.rs](file:///Users/bytedance/project/codex/codex-rs/exec/src/main.rs#L1-L45)
- a library that builds effective configuration, starts an in-process app-server client, creates or resumes a thread, starts either a user turn or a review run, and streams server events through an output processor via [lib.rs](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L216-L530) and [lib.rs](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L532-L904)
- two output adapters: a human-oriented stderr/stdout renderer and a machine-oriented JSONL event emitter via [event_processor_with_human_output.rs](file:///Users/bytedance/project/codex/codex-rs/exec/src/event_processor_with_human_output.rs#L23-L416) and [event_processor_with_jsonl_output.rs](file:///Users/bytedance/project/codex/codex-rs/exec/src/event_processor_with_jsonl_output.rs#L58-L621)
- an exported JSON event schema for downstream consumers via [exec_events.rs](file:///Users/bytedance/project/codex/codex-rs/exec/src/exec_events.rs#L8-L312)

At a high level, `codex-exec` is a thin orchestration layer around `codex-app-server-client` plus local CLI-specific policy. It does not implement the agent itself; it configures the environment, translates CLI intent into app-server RPCs, filters notifications to the active thread/turn, and renders output with strict stdout/stderr discipline.

## Manifest and build shape

The crate manifest in [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/exec/Cargo.toml#L1-L75) shows a split binary/library crate:

- package name: `codex-exec`
- library crate: `codex_exec`
- binary: `codex-exec`
- custom integration test target: `tests/all.rs`
- `autotests = false`, which keeps test aggregation explicit

The Bazel target in [BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/exec/BUILD.bazel#L1-L7) mirrors the crate as `codex_exec` and marks tests with `no-sandbox`.

## Concrete responsibilities

### 1. CLI surface and argument normalization

The CLI is defined in [cli.rs](file:///Users/bytedance/project/codex/codex-rs/exec/src/cli.rs#L9-L285). Core responsibilities:

- define the default `exec` flow when no subcommand is present
- define `resume` and `review` subcommands
- expose execution-wide flags like `--json`, `--ephemeral`, `--skip-git-repo-check`, `--output-schema`, and `-o/--output-last-message`
- flatten shared workspace-wide options from `codex_utils_cli::SharedCliOptions`
- mark selected shared flags as global so they still parse after subcommands via [mark_exec_global_args](file:///Users/bytedance/project/codex/codex-rs/exec/src/cli.rs#L131-L137)
- reinterpret `resume --last <PROMPT>` so the positional argument becomes a prompt instead of a session id via [ResumeArgsRaw -> ResumeArgs](file:///Users/bytedance/project/codex/codex-rs/exec/src/cli.rs#L199-L237)

`main.rs` adds one more normalization step: root-level `--config` overrides are parsed outside the inner CLI and then spliced back into `Cli.config_overrides` before calling [run_main](file:///Users/bytedance/project/codex/codex-rs/exec/src/main.rs#L28-L40). That keeps downstream logic unaware of the two-level parse shape.

### 2. Config resolution and startup policy

Startup is dominated by [run_main](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L216-L530). It:

- computes ANSI policy from `--color`
- parses `-c/--config` key-value overrides
- resolves cwd with canonicalization
- locates `CODEX_HOME`
- loads config TOML with loader overrides for `--ignore-user-config` and `--ignore-rules`
- resolves OSS provider/model selection when `--oss` is active
- builds the final `Config` with harness overrides such as sandbox mode, approval policy, profile, model, cwd, extra writable roots, and ephemeral mode
- validates exec policy rules
- enforces login restrictions
- initializes tracing/OpenTelemetry
- constructs `InProcessClientStartArgs` for the app-server client

Important policy choices happen here:

- headless exec defaults approvals to `AskForApproval::Never` in `ConfigOverrides`, unless feature/config layers later change it
- `--full-auto` upgrades sandbox mode to `WorkspaceWrite`
- `--dangerously-bypass-approvals-and-sandbox` upgrades sandbox to `DangerFullAccess`
- OSS mode enables raw reasoning output by default via `show_raw_agent_reasoning`

### 3. Session creation, resume, and turn orchestration

[run_exec_session](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L532-L904) is the central control loop. It:

- selects the output processor implementation based on `--json`
- validates OSS provider readiness when needed
- translates CLI intent into an `InitialOperation`
  - normal prompt -> `UserTurn`
  - `resume` -> prompt plus optional images, then resume-or-start thread
  - `review` -> `ReviewStart`
- optionally blocks execution outside a Git repo unless bypassed
- starts the in-process app-server client
- chooses `thread/start` vs `thread/resume`
- emits the effective config summary through the selected event processor
- starts the turn or review
- enters an event loop over server requests, server notifications, and Ctrl-C interrupts
- requests `turn/interrupt` on Ctrl-C
- requests `thread/unsubscribe` on graceful shutdown
- exits with code 1 if a fatal server-side error was observed

This function is intentionally stateful but small-scope: it keeps only the current thread id, turn id, request id sequence, and error/shutdown state in process-local memory.

### 4. Output shaping

The shared trait in [event_processor.rs](file:///Users/bytedance/project/codex/codex-rs/exec/src/event_processor.rs#L7-L48) gives exec one abstraction point:

- `print_config_summary`
- `process_server_notification`
- `process_warning`
- `print_final_output`

That trait is used to keep the control plane in `lib.rs` output-format agnostic.

### 5. JSON event contract

[exec_events.rs](file:///Users/bytedance/project/codex/codex-rs/exec/src/exec_events.rs#L8-L312) defines the external JSONL model. Responsibilities:

- provide stable event names like `thread.started`, `turn.started`, `item.completed`, and `error`
- normalize app-server item variants into a smaller exec-specific schema
- derive `Serialize`, `Deserialize`, `PartialEq`, and `ts_rs::TS` so the same shape can feed tests, wire output, and generated TS types

The schema is intentionally narrower than the internal app-server protocol. For example, file changes collapse update details down to `Add | Delete | Update`, and some warning/deprecation notifications are represented as error-like items instead of a dedicated warning event.

## Public and semi-public APIs

### Re-exported types

The library re-exports:

- CLI types: `Cli`, `Command`, `ReviewArgs`
- output status helpers: `CodexStatus`, `CollectedThreadEvents`, `EventProcessorWithJsonOutput`
- JSON event model types from `exec_events.rs`

Those re-exports live near the top of [lib.rs](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L13-L127). The practical public API is therefore:

- parse CLI arguments from `Cli`
- invoke [run_main](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L216-L530)
- embed or test JSON event emission through `EventProcessorWithJsonOutput` and the `exec_events` types

### Internal helpers that define behavior

Key internal API boundaries:

- thread bootstrap mapping via [thread_start_params_from_config](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L923-L935) and [thread_resume_params_from_config](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L937-L949)
- active-turn notification filtering via [should_process_notification](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L1070-L1120)
- terminal-item recovery via [maybe_backfill_turn_completed_items](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L1122-L1163)
- resume lookup heuristics via [resolve_resume_thread_id](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L1234-L1341)
- unsupported interactive server request handling via [handle_server_request](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L1422-L1545)
- prompt/stdin decoding via [decode_prompt_bytes](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L1599-L1627), [read_prompt_from_stdin](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L1646-L1691), and [resolve_root_prompt](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L1719-L1730)
- review target building via [build_review_request](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L1732-L1760)

## Module-by-module notes

### `src/main.rs`

- Binary entrypoint only
- Uses `codex_arg0::arg0_dispatch_or_else` so the same binary can also behave as `codex-linux-sandbox`
- Merges root-level config overrides into inner exec CLI state

### `src/cli.rs`

- Defines the CLI contract
- Wraps shared CLI options in `ExecSharedCliOptions`
- Makes model/full-auto/danger bypass flags global across subcommands
- Implements the `resume --last <prompt>` positional reinterpretation

### `src/lib.rs`

- Owns startup, configuration, tracing, app-server startup, request/notification loop, shutdown, and exit code policy
- Contains most of the crate’s business rules
- Keeps stdout/stderr constraints explicit at the crate root with `#![deny(clippy::print_stdout)]`, then allows printing only in narrowly-controlled places

### `src/event_processor.rs`

- Defines the output abstraction
- Centralizes `--output-last-message` file writing through `handle_last_message`

### `src/event_processor_with_human_output.rs`

- Renders user-facing, readable progress to stderr
- Preserves the invariant that stdout contains only the final message when appropriate
- Tracks whether the final message was already rendered during streaming so it is not duplicated on shutdown
- Summarizes config, reasoning, commands, patches, MCP calls, plan updates, token usage, and terminal turn state

### `src/event_processor_with_jsonl_output.rs`

- Converts app-server notifications into a synthetic exec JSONL event stream
- Maintains an internal map from raw app-server item ids to stable synthetic exec ids
- Emits todo lists as started/updated/completed synthetic items derived from plan updates
- Recovers unfinished item completions from terminal turn payloads
- Stores last token usage and final message for shutdown/file-output handling

### `src/exec_events.rs`

- Defines the JSONL wire contract consumed by tests or external automation
- Uses serde tagged enums for readability and compatibility
- Uses `ts-rs` derives so the same shapes can be surfaced to TypeScript consumers

## Execution flow

### Normal prompt flow

1. Parse CLI and merge root config overrides in [main.rs](file:///Users/bytedance/project/codex/codex-rs/exec/src/main.rs#L28-L40).
2. Build effective config and start tracing in [run_main](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L216-L530).
3. Resolve the prompt, optionally appending piped stdin as `<stdin>...</stdin>` in [resolve_root_prompt](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L1719-L1730).
4. Start an in-process app-server client and issue `thread/start` in [run_exec_session](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L650-L704).
5. Send `turn/start` with input items, cwd, approval policy, sandbox policy, and optional output schema in [run_exec_session](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L734-L767).
6. Stream notifications, filter them to the active thread/turn, and pass them to the event processor in [run_exec_session](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L796-L893).
7. Backfill missing final turn items from `thread/read` when non-ephemeral threads complete with empty item lists in [maybe_backfill_turn_completed_items](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L1122-L1163).
8. Unsubscribe, shut down the client, print final output, and use exit code 1 if a fatal error was seen in [run_exec_session](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L895-L903).

### Resume flow

Resume adds one lookup phase before starting the next turn:

- `--last` walks paginated `thread/list` results ordered by `updated_at`
- default resume filters by current cwd, but `--all` disables that
- named resumes first try UUID parsing, then state DB exact-title lookup, then filesystem thread metadata, then paginated search-term lookup
- latest cwd may be derived from the rollout JSONL’s most recent `TurnContext` instead of the thread’s stored cwd

That logic lives in [resolve_resume_thread_id](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L1234-L1341) plus [latest_thread_cwd](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L1204-L1211) and [parse_latest_turn_context_cwd](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L1213-L1228).

### Review flow

Review skips normal prompt construction and instead builds a `ReviewRequest` from mutually-exclusive CLI flags in [build_review_request](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L1732-L1760), then sends `review/start` in [run_exec_session](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L768-L792).

## Dependency analysis

The manifest in [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/exec/Cargo.toml#L23-L75) shows that this crate is mostly a coordinator over other workspace crates.

### Core runtime dependencies

- `clap`: CLI parsing and subcommand handling
- `tokio`: async runtime, signal handling, and mpsc interrupt channel
- `tracing` and `tracing-subscriber`: logging and span management
- `serde` and `serde_json`: config/schema parsing and JSONL output
- `supports-color` and `owo-colors`: terminal capability detection and styling
- `uuid`: session id parsing for resume lookup
- `ts-rs`: TypeScript-exportable event schema derivations

### Workspace-specific dependencies

- `codex-app-server-client`: starts the in-process app-server client and transports events/requests
- `codex-app-server-protocol`: typed request/response/notification model used on the client boundary
- `codex-core`: config loading, policy validation, review prompts, state DB lookup, OTEL setup, and path utilities
- `codex-protocol`: core protocol enums/types shared with the broader system
- `codex-login`: login restriction enforcement and default originator/residency controls
- `codex-utils-cli`: shared CLI options and config override parsing
- `codex-utils-oss` and `codex-model-provider-info`: OSS provider selection and readiness checks
- `codex-git-utils`: trusted-directory / git-root detection
- `codex-feedback`, `codex-otel`, `codex-cloud-requirements`, `codex-arg0`: startup plumbing

### Architectural implication

`codex-exec` deliberately avoids becoming a second agent implementation. The app-server owns conversation execution and most item semantics; `codex-exec` owns:

- CLI-specific defaults
- resume/review command semantics
- environment/bootstrap policy
- event filtering and output translation

## Output behavior and invariants

One subtle design constraint is documented at the top of [lib.rs](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L1-L5):

- in normal mode, stdout should contain only the final message, if any
- in `--json` mode, stdout must contain valid JSONL, one event per line
- everything else must go to stderr

The crate enforces this with:

- crate-level `#![deny(clippy::print_stdout)]`
- human output routed mostly to `eprintln!`
- JSON output routed only through the JSON event emitter
- final message printing guarded by [should_print_final_message_to_stdout](file:///Users/bytedance/project/codex/codex-rs/exec/src/event_processor_with_human_output.rs#L549-L555) and [should_print_final_message_to_tty](file:///Users/bytedance/project/codex/codex-rs/exec/src/event_processor_with_human_output.rs#L557-L564)

## Error handling and policy choices

Several choices are intentionally strict:

- missing prompt or unreadable/invalid schema files cause immediate process exit with code 1
- running outside a Git repo is rejected unless `--skip-git-repo-check` or full danger bypass is used
- many interactive app-server requests are rejected because exec mode is non-interactive, including command approval, file-change approval, permission approval, dynamic tool calls, and `request_user_input` via [handle_server_request](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L1422-L1545)
- MCP elicitation is auto-cancelled instead of surfaced to a user
- server-reported terminal errors set `error_seen`, which forces a non-zero exit code for automation

This makes the crate more predictable for scripting, but it also narrows supported behaviors compared with an interactive client.

## Testing coverage

### Unit tests in `src/*_tests.rs`

The crate has focused unit tests for local behavior:

- [cli_tests.rs](file:///Users/bytedance/project/codex/codex-rs/exec/src/cli_tests.rs#L1-L72): resume parsing, global flags after subcommands, and config isolation flags
- [main_tests.rs](file:///Users/bytedance/project/codex/codex-rs/exec/src/main_tests.rs#L1-L37): root config override parsing around the top-level parser
- [lib_tests.rs](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib_tests.rs#L1-L435): review request construction, trace parenting, prompt decoding, lagged-event warning text, backfill policy, approvals reviewer propagation, and session bootstrap mapping
- [event_processor_with_human_output_tests.rs](file:///Users/bytedance/project/codex/codex-rs/exec/src/event_processor_with_human_output_tests.rs#L15-L320): final-message duplication rules, reasoning selection, fallback-to-plan behavior, and stale-final-message cleanup
- [event_processor_with_jsonl_output_tests.rs](file:///Users/bytedance/project/codex/codex-rs/exec/src/event_processor_with_jsonl_output_tests.rs#L5-L29): failure behavior for `--output-last-message`

### Integration tests

Integration tests are aggregated through [tests/all.rs](file:///Users/bytedance/project/codex/codex-rs/exec/tests/all.rs#L1-L5) and [tests/suite/mod.rs](file:///Users/bytedance/project/codex/codex-rs/exec/tests/suite/mod.rs#L1-L12). Notable coverage includes:

- [prompt_stdin.rs](file:///Users/bytedance/project/codex/codex-rs/exec/tests/suite/prompt_stdin.rs#L8-L171): prompt-from-stdin, prompt-plus-stdin-append, and empty-stdin rejection
- [output_schema.rs](file:///Users/bytedance/project/codex/codex-rs/exec/tests/suite/output_schema.rs#L8-L62): ensures `--output-schema` is passed into the model request in the expected JSON schema wrapper
- [ephemeral.rs](file:///Users/bytedance/project/codex/codex-rs/exec/tests/suite/ephemeral.rs#L8-L53): rollout persistence vs `--ephemeral`
- [resume.rs](file:///Users/bytedance/project/codex/codex-rs/exec/tests/suite/resume.rs#L115-L544): append-to-same-session semantics, `--last`, cwd filtering, `--all`, image attachments, and preservation of CLI overrides across resume
- [apply_patch.rs](file:///Users/bytedance/project/codex/codex-rs/exec/tests/suite/apply_patch.rs#L16-L150): standalone apply-patch emulation plus agent-driven patch application
- [mcp_required_exit.rs](file:///Users/bytedance/project/codex/codex-rs/exec/tests/suite/mcp_required_exit.rs#L8-L38): non-zero exit when required MCP servers fail initialization
- [server_error_exit.rs](file:///Users/bytedance/project/codex/codex-rs/exec/tests/suite/server_error_exit.rs#L7-L33): non-zero exit on server-reported response failure
- [tests/event_processor_with_json_output.rs](file:///Users/bytedance/project/codex/codex-rs/exec/tests/event_processor_with_json_output.rs#L77-L1586): deep coverage for JSON event translation, item-id reuse, todo lifecycle, token usage emission, turn completion reconciliation, reroute handling, and final-message recovery

### What the tests say about design intent

The tests strongly suggest these intended invariants:

- resume must mutate the original stored session, not fork a new file accidentally
- prompt handling must remain backward compatible with existing stdin behavior
- JSON output is treated as an external contract and gets the heaviest structural testing
- automation-friendly non-zero exit codes matter
- final-message handling is subtle enough to justify dedicated unit coverage

## Design observations

### Clear separation of concerns

The crate has a solid boundary between:

- CLI parsing
- config/bootstrap
- app-server orchestration
- output rendering
- exported JSON schema

That separation is most visible in the `EventProcessor` trait and the `exec_events` module.

### Good reuse of existing infrastructure

Resume logic intentionally avoids directly reading rollout storage for the actual resume operation; it uses `thread/list` and `thread/resume` APIs and only consults rollout/state-db data as lookup aids. That reduces coupling to on-disk history format.

### Some semantic loss in normalization

The JSON output layer simplifies several app-server distinctions:

- warnings and deprecations become error-like items
- declined patch application becomes failed patch status
- some unsupported server notifications are ignored entirely

This is probably acceptable for exec consumers, but it is a meaningful design tradeoff.

### Exit-heavy control flow

`run_main`, prompt/schema loading helpers, and git/config failure paths frequently use `eprintln!` plus `std::process::exit(1)` instead of bubbling typed errors. That is ergonomic for a CLI, but it makes some behavior harder to compose as a pure library.

### Resume logic is intentionally heuristic

The resume path combines:

- UUID detection
- state DB exact-title lookup
- filesystem metadata lookup
- paginated thread search
- cwd matching based on latest turn context

This is practical, but it means “resume the right thread” depends on multiple storage layers and fallback behaviors rather than one authoritative index.

## Open questions

1. Should `exec` remain strictly non-interactive, or should some server request types eventually gain safe non-TTY workflows?
   - Current behavior rejects approvals, dynamic tool calls, and user-input requests in [handle_server_request](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L1422-L1545).

2. Can app-server emit complete terminal turn items reliably enough to remove the `thread/read` backfill path?
   - The current workaround in [maybe_backfill_turn_completed_items](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L1122-L1163) exists because terminal delivery may omit final items under backpressure.

3. Is the JSONL warning/error model too lossy for downstream tooling?
   - Warnings, deprecations, reroutes, and some failures collapse into `ErrorItem` or `ThreadErrorEvent` instead of dedicated event classes.

4. Should declined patch application remain distinguishable from failure in exported JSON?
   - The JSON mapper currently folds `Declined` into failed status in [event_processor_with_jsonl_output.rs](file:///Users/bytedance/project/codex/codex-rs/exec/src/event_processor_with_jsonl_output.rs#L195-L201).

5. Should prompt/schema/config helper functions return errors instead of exiting the process?
   - The current CLI-oriented approach is simple, but it limits reuse of `run_main` internals in embedded contexts.

6. Is `resume --last` sufficiently deterministic when multiple storage sources disagree?
   - The tests account for timestamp granularity and cwd drift, which suggests the underlying sort/filter inputs are not fully authoritative.

## Bottom line

`codex-exec` is a coordination crate, not an agent crate. Its core value is:

- translating CLI intent into app-server calls
- enforcing safe/default headless policy
- keeping output deterministic for humans and automation
- exporting a simpler JSON event contract than the full internal protocol

The implementation is generally well-factored for that role. The most complex parts are not algorithmic Rust tricks, but boundary management: config layering, resume selection, event filtering, final-message discipline, and loss-aware normalization from richer internal events to a smaller CLI contract.
