# codex-cloud-tasks analysis

## Scope

- Crate manifest: [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/Cargo.toml#L1-L45)
- Public entry point and command/TUI orchestration: [lib.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L1-L2407)
- CLI shape: [cli.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/cli.rs#L1-L120)
- App state and background event model: [app.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/app.rs#L1-L512)
- Environment discovery: [env_detect.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/env_detect.rs#L1-L362)
- TUI rendering: [ui.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/ui.rs#L1-L1046)
- New-task composer wrapper: [new_task.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/new_task.rs#L1-L35)
- Scroll model for details overlays: [scrollable_diff.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/scrollable_diff.rs#L1-L176)
- Auth/URL/logging helpers: [util.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/util.rs#L1-L157)
- Tests: [lib.rs tests](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L2135-L2407), [app.rs tests](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/app.rs#L353-L512), [env_filter.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/tests/env_filter.rs#L1-L39)
- Bazel target: [BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/BUILD.bazel#L1-L7)

## Responsibilities

- Provide the `codex cloud` surface for both non-interactive subcommands and an interactive terminal UI through [run_main](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L738-L2024).
- Adapt the generic backend contract from [CloudBackend](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-client/src/api.rs#L133-L170) into user-facing workflows: list, status, diff, apply, and create task.
- Resolve auth, user-agent, ChatGPT account headers, base URL normalization, and mock-vs-real backend selection in [init_backend](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L43-L115) and [util.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/util.rs#L11-L133).
- Discover likely cloud environments from local git remotes and backend environment APIs via [autodetect_environment_id](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/env_detect.rs#L25-L108) and [list_environments](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/env_detect.rs#L254-L362).
- Maintain responsive TUI state using an internal app/event model, async background tasks, and redraw coalescing in [App](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/app.rs#L46-L75), [AppEvent](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/app.rs#L297-L350), and the main event loop in [run_main](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L829-L2009).
- Present task details as either diff or conversation views, including multi-attempt navigation for best-of-N tasks, via [DiffOverlay](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/app.rs#L136-L289), [collect_attempt_diffs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L304-L345), and [draw_diff_overlay](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/ui.rs#L312-L467).

## Public API

- The crate’s public Rust surface is intentionally tiny:
  - [Cli](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/cli.rs#L5-L13) is re-exported from [lib.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L1-L8).
  - [run_main](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L738-L2024) is the operational entry point.
- `Cli` supports five non-interactive subcommands in [Command](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/cli.rs#L15-L27):
  - [ExecCommand](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/cli.rs#L29-L50): create a task from prompt text/stdin, environment, attempts, and optional branch
  - [StatusCommand](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/cli.rs#L74-L79): fetch one task summary and exit nonzero unless status is ready
  - [ListCommand](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/cli.rs#L81-L98): list tasks in text or JSON with pagination
  - [ApplyCommand](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/cli.rs#L100-L109): apply a selected attempt locally
  - [DiffCommand](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/cli.rs#L111-L120): print a selected attempt diff
- The CLI validators in [parse_attempts](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/cli.rs#L52-L61) and [parse_limit](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/cli.rs#L63-L72) encode product limits directly into argument parsing.

## Module layout

- [lib.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L1-L2407) is the true crate center:
  - backend/auth init
  - command dispatch
  - TUI bootstrap and event loop
  - task-attempt selection/apply helpers
  - formatting helpers and several unit tests
- [app.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/app.rs#L1-L512) holds state, overlay models, async event enums, and the reusable `load_tasks` backend call.
- [ui.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/ui.rs#L1-L1046) is rendering-only: list, footer, detail overlay, apply modal, env modal, best-of modal, and line styling for markdown/diffs.
- [env_detect.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/env_detect.rs#L1-L362) is a mini environment-selection subsystem that talks to backend REST endpoints directly instead of going through `CloudBackend`.
- [new_task.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/new_task.rs#L1-L35) is a thin wrapper over `codex_tui::ComposerInput` with cloud-task-specific hints.
- [scrollable_diff.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/scrollable_diff.rs#L1-L176) is a local generic utility for wrapped scrolling text, despite the name being “diff”-specific.
- [util.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/util.rs#L1-L157) centralizes cross-cutting concerns that would otherwise clutter command/TUI logic.

## Backend contract

- This crate depends on the backend abstraction defined in [cloud-tasks-client/api.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-client/src/api.rs#L20-L170), not directly on raw HTTP everywhere.
- The main types consumed here are:
  - [TaskSummary](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-client/src/api.rs#L33-L50) for list/status rows
  - [TaskText](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-client/src/api.rs#L110-L131) for prompt/conversation/attempt metadata
  - [TurnAttempt](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-client/src/api.rs#L63-L71) for sibling best-of-N attempts
  - [ApplyOutcome](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-client/src/api.rs#L81-L90) for both preflight and real apply
  - [CreatedTask](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-client/src/api.rs#L92-L95) for task creation
- In debug builds, [init_backend](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L43-L115) can swap to [MockClient](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L17-L161) with `CODEX_CLOUD_TASKS_MODE=mock`.

## Command flows

### Non-interactive flows

- `exec`:
  - Reads prompt text from argv or stdin via [resolve_query_input](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L235-L260).
  - Resolves an environment id or label through [resolve_environment_id](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L190-L233).
  - Picks a git ref from explicit branch, current branch, default branch, or `"main"` using [resolve_git_ref_with_git_info](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L141-L163).
  - Calls `CloudBackend::create_task` in [run_exec_command](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L165-L188) and prints a browser URL.
- `status`:
  - Parses either raw IDs or task URLs with [parse_task_id](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L262-L281).
  - Fetches a summary and renders three status lines via [format_task_status_lines](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L415-L480) from [run_status_command](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L501-L515).
  - Exits with code `1` for any non-ready task, so callers can use it in scripts.
- `list`:
  - Optionally resolves an environment filter before calling `CloudBackend::list_tasks` in [run_list_command](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L517-L582).
  - Emits either pretty JSON or formatted URLs + status lines.
  - Preserves backend pagination cursor and prints the next command for manual paging.
- `diff` and `apply`:
  - Build a merged attempt list from primary task details plus sibling attempts in [collect_attempt_diffs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L304-L345).
  - Select a 1-based attempt through [select_attempt](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L347-L365).
  - Print the diff in [run_diff_command](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L584-L591) or apply it in [run_apply_command](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L593-L612).

### Interactive TUI flow

- Startup path in [run_main](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L738-L928):
  - initializes logging
  - creates backend/auth context
  - enters alternate screen + raw mode
  - creates `App`
  - spawns initial task load, environment list fetch, and environment autodetection concurrently
  - starts a redraw scheduler based on future instants rather than a fixed interval
- State updates are message-driven:
  - background tasks send [AppEvent](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/app.rs#L297-L350)
  - the `tokio::select!` loop in [run_main](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L930-L2009) merges redraw timers, app events, and terminal events
  - UI rendering is centralized through [ui::draw](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/ui.rs#L28-L57)
- The TUI supports four main user journeys:
  - browse and refresh filtered task lists
  - open task details and toggle prompt/diff views
  - preflight/apply selected diffs
  - create new tasks with environment and best-of selection

## Environment discovery flow

- The environment subsystem does more than simple listing:
  - [get_git_origins](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/env_detect.rs#L171-L210) shells out to git to inspect remotes
  - [parse_owner_repo](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/env_detect.rs#L218-L252) normalizes GitHub SSH/HTTPS URL shapes
  - by-repo backend endpoints are queried first, then a global environments endpoint
- Selection heuristics in [pick_environment_row](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/env_detect.rs#L110-L145):
  - exact label match if requested
  - single environment if only one exists
  - pinned environment
  - highest `task_count`, else first entry
- For explicit CLI `--env`, [resolve_environment_id](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L190-L233) treats ids and labels differently and rejects ambiguous labels instead of silently choosing one.

## UI and state design

- `App` in [app.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/app.rs#L46-L75) is a mutable view model, not a reducer/store abstraction. It tracks tasks, selection, inflight flags, environment state, modal state, best-of selection, and overlay state in one struct.
- `DiffOverlay` in [app.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/app.rs#L136-L289) is the main detail abstraction:
  - it stores the currently visible content as both semantic attempt data and flattened display fields
  - it can switch between `Prompt` and `Diff` views with [DetailView](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/app.rs#L291-L295)
  - it supports cyclic attempt navigation while keeping scroll state consistent
- `ScrollableDiff` in [scrollable_diff.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/scrollable_diff.rs#L21-L176) is a reusable text viewport:
  - caches wrapped lines per width
  - tracks source-line indices so conversation styling can respect raw line boundaries
  - wraps using Unicode display widths, not byte counts
- `ui.rs` separates rendering from business logic fairly well:
  - list/footer/new-task rendering in [draw_new_task_page](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/ui.rs#L104-L174), [draw_list](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/ui.rs#L176-L234), and [draw_footer](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/ui.rs#L236-L310)
  - detail overlay rendering in [draw_diff_overlay](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/ui.rs#L312-L467)
  - modal rendering in [draw_apply_modal](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/ui.rs#L469-L550), [draw_env_modal](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/ui.rs#L893-L991), and [draw_best_of_modal](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/ui.rs#L993-L1046)

## Data formatting and UX helpers

- Task summaries are reduced to compact CLI/TUI lines through [summary_line](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L376-L413), [format_task_status_lines](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L415-L480), and [render_task_item](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/ui.rs#L788-L844).
- Conversation fallback formatting comes from [conversation_lines](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L2028-L2053), while richer prompt/assistant markdown styling is in [style_conversation_lines](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/ui.rs#L558-L652).
- Verbose backend errors are compressed for humans in [pretty_lines_from_error](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L2055-L2133), including extraction of embedded JSON `turn_status` and assistant error fields.
- Utility helpers like [task_url](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/util.rs#L120-L133) and [format_relative_time](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/util.rs#L135-L157) keep display logic reasonably centralized.

## Dependencies

- External crates from [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/Cargo.toml#L14-L45):
  - `anyhow` for end-to-end command error propagation
  - `clap` for subcommand/flag parsing
  - `tokio`, `tokio-stream`, `async-trait` for async orchestration
  - `crossterm` and `ratatui` for the terminal UI
  - `reqwest`, `serde`, `serde_json`, `chrono` for environment discovery and output formatting
  - `owo-colors`, `supports-color`, `unicode-width` for CLI/TUI presentation polish
  - `tracing` and `tracing-subscriber` for minimal logging
- Workspace dependencies with concrete roles:
  - [codex-cloud-tasks-client](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-client/src/api.rs#L133-L170) provides the transport-agnostic backend trait and domain types
  - [codex-cloud-tasks-mock-client](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L17-L161) provides the debug mock backend
  - `codex-login` and `codex-core` back auth/config loading in [load_auth_manager](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/util.rs#L63-L72)
  - `codex-git-utils` supplies branch detection in [RealGitInfo](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L124-L139)
  - `codex-tui` supplies the composer widget and markdown rendering used in [new_task.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/new_task.rs#L1-L35) and [ui.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/ui.rs#L24-L27)
  - `codex-client` supplies the custom-CA reqwest builder used by [env_detect.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/env_detect.rs#L1-L5)

## Testing

- Unit tests in [lib.rs tests](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L2135-L2407) cover:
  - branch-resolution precedence and trimming
  - task-status/task-list formatting
  - parsing task IDs from raw strings and URLs
  - collecting sibling attempt diffs from the mock backend
  - attempt selection bounds
  - a slow ignored composer rendering regression test
- Unit tests in [app.rs tests](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/app.rs#L353-L512) cover `load_tasks` environment filtering against a fake backend.
- Integration-style coverage in [env_filter.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/tests/env_filter.rs#L1-L39) verifies that the mock backend returns different task sets per environment.
- The mock backend itself is deterministic enough for behavior tests; see [MockClient](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L17-L161).
- Verification run:
  - `cargo test -p codex-cloud-tasks` passed locally from `/Users/bytedance/project/codex/codex-rs`

## Design observations

- The crate is intentionally product-facing rather than library-like:
  - most code is organized around user journeys, not reusable domain services
  - `run_main` is effectively the application controller
- The backend boundary is mostly clean:
  - `CloudBackend` gives the crate a stable contract for list/detail/apply/create operations
  - the mock client makes non-network testing straightforward
- Environment discovery is the main architectural exception:
  - it bypasses `CloudBackend`
  - it performs raw HTTP and git shell-outs inside this crate
  - it duplicates some base-URL/header logic already present elsewhere
- The TUI architecture is pragmatic:
  - it uses direct mutable state and `tokio::spawn` instead of a more formal Elm/reducer architecture
  - this keeps interaction code straightforward, but makes the main event loop large and harder to reason about as features grow
- Attempt handling is one of the stronger parts of the design:
  - the crate understands best-of-N tasks as first-class UX
  - CLI and TUI both reuse the same underlying concepts: attempt placement, sibling attempts, preflight/apply with diff overrides
- Error handling is user-oriented rather than type-rich:
  - commands mostly return `anyhow::Result`
  - the code frequently formats and prints final messages itself, including explicit `std::process::exit(1)` branches

## Gaps and open questions

- Why is [codex-cloud-tasks-mock-client](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/Cargo.toml#L21-L22) in normal dependencies instead of only dev-dependencies?
  - `Cargo.toml` even contains a TODO acknowledging this.
  - It increases the production dependency surface for a debug-only path.
- Should environment discovery be pulled behind a typed client abstraction?
  - [env_detect.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/env_detect.rs#L25-L362) manually builds URLs, headers, and decode paths.
  - That duplicates transport concerns that `cloud-tasks-client` otherwise hides.
- Is [run_main](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L738-L2024) too large to maintain comfortably?
  - It owns terminal setup/teardown, async orchestration, event handling, modal semantics, task loading, and action dispatch in one function.
  - A future feature spike will likely make this the primary maintenance hotspot.
- Should logging write to a plain relative [error.log](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/util.rs#L17-L27)?
  - Current behavior depends on the caller’s working directory and can scatter logs across repos.
  - There is no rotation, location policy, or explicit user control.
- Should auth/config overrides be threaded through properly?
  - [load_auth_manager](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/util.rs#L63-L72) contains a TODO about CLI overrides.
  - That suggests the crate does not yet integrate cleanly with the wider config override mechanism.
- Is test coverage sufficient around the riskiest code paths?
  - There is good logic coverage, but little direct exercise of terminal event behavior, environment HTTP failures, auth header construction, or real apply/preflight error permutations.

## Bottom line

- `codex-cloud-tasks` is the user-facing cloud-task application layer for this workspace.
- Its strongest traits are:
  - a narrow backend contract
  - practical multi-attempt handling
  - a responsive, featureful terminal UI
- Its main architectural pressure points are:
  - the oversized `run_main` controller
  - the ad hoc environment-discovery subsystem
  - operational helpers like logging/auth wiring that still leak implementation details
- Overall, the crate already has a coherent product shape: it is not just a thin CLI wrapper, but a full terminal client for browsing, creating, inspecting, and applying cloud-generated coding tasks.
