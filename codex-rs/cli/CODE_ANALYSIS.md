# codex-cli crate analysis

## Scope

This document analyzes the `codex-cli` crate at `/Users/bytedance/project/codex/codex-rs/cli`. The crate is both:

- a binary crate that builds the `codex` executable via [main.rs](file:///Users/bytedance/project/codex/codex-rs/cli/src/main.rs#L677-L1174)
- a small library crate that exposes sandbox/login helpers and argument structs via [lib.rs](file:///Users/bytedance/project/codex/codex-rs/cli/src/lib.rs#L1-L73)

At a high level, this crate is an orchestration shell around the wider Codex workspace. It does very little domain work itself. Instead, it:

- defines the user-facing CLI grammar with `clap`
- normalizes top-level flags into config overrides
- routes subcommands into specialized workspace crates
- hosts a few CLI-specific integrations that do not naturally belong elsewhere:
  - login UX and file-backed login tracing
  - sandbox execution wrappers
  - plugin marketplace CLI glue
  - MCP configuration/login/logout/list/get commands
  - a thin raw Responses API streaming bridge
  - desktop app open/install helpers for macOS and Windows

## Manifest and packaging

The manifest is [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/cli/Cargo.toml#L1-L79).

- Package name is `codex-cli`; binary name is `codex`; library name is `codex_cli`.
- `build.rs` is used only for a macOS-specific linker tweak, adding `-ObjC` on macOS builds in [build.rs](file:///Users/bytedance/project/codex/codex-rs/cli/build.rs#L1-L5).
- Most functionality comes from workspace crates such as `codex-core`, `codex-tui`, `codex-exec`, `codex-app-server`, `codex-mcp-server`, `codex-models-manager`, `codex-login`, and `codex-state`.
- External dependencies are mostly infrastructure:
  - CLI parsing: `clap`, `clap_complete`
  - async/runtime: `tokio`
  - error handling: `anyhow`
  - serialization: `serde_json`, `toml`
  - terminal UX: `owo-colors`, `supports-color`
  - logging: `tracing`, `tracing-appender`, `tracing-subscriber`
  - OS/platform helpers: `libc`, `tempfile`, `regex-lite`
- Dev dependencies focus on command-level integration testing: `assert_cmd`, `predicates`, `sqlx`, `pretty_assertions`.

## Core responsibilities

### 1. CLI surface definition

The central CLI grammar lives in [main.rs](file:///Users/bytedance/project/codex/codex-rs/cli/src/main.rs#L69-L488).

Key top-level command groups:

- `exec`, `review`
- `login`, `logout`
- `mcp`
- `plugin marketplace`
- `mcp-server`
- `app-server`
- `app` on macOS/Windows only
- `sandbox`
- `debug`
- `execpolicy`
- `apply`
- `resume`, `fork`
- `cloud`
- hidden/internal commands such as `responses-api-proxy`, `responses`, and `stdio-to-uds`
- `features`

This file owns the user-facing command taxonomy and compatibility behavior, including:

- visible aliases like `exec -> e` and `apply -> a`
- hidden/internal commands
- top-level global flags like config overrides, feature toggles, and remote TUI options
- precedence rules between root-level flags and subcommand-specific flags

### 2. Routing and orchestration

The real runtime dispatcher is [cli_main](file:///Users/bytedance/project/codex/codex-rs/cli/src/main.rs#L684-L1174).

Its pattern is consistent:

1. Parse the full CLI.
2. Fold `--enable/--disable` feature toggles into raw config overrides.
3. Capture root interactive remote settings.
4. Match the selected subcommand.
5. Reject remote mode for non-interactive commands when appropriate.
6. Prepend root config overrides into the subcommand-specific override list.
7. Delegate execution into another workspace crate or a local helper module.

This makes the crate a coordination layer rather than a business-logic crate.

### 3. Interactive TUI bootstrapping

When no subcommand is provided, the crate runs the interactive TUI via [run_interactive_tui](file:///Users/bytedance/project/codex/codex-rs/cli/src/main.rs#L1439-L1491).

This wrapper adds CLI-specific behavior before handing off to `codex_tui::run_main`:

- normalizes prompt newlines
- warns or blocks if `TERM=dumb`
- validates remote websocket usage
- reads remote auth tokens from an environment variable
- passes arg0-dispatched executable paths into the TUI runtime

The post-run UX is also owned here:

- [format_exit_messages](file:///Users/bytedance/project/codex/codex-rs/cli/src/main.rs#L490-L517) renders token-usage and resume hints
- [handle_app_exit](file:///Users/bytedance/project/codex/codex-rs/cli/src/main.rs#L520-L538) prints messages and optionally triggers a self-update
- [run_update_action](file:///Users/bytedance/project/codex/codex-rs/cli/src/main.rs#L541-L580) handles OS-specific update command execution

### 4. Session continuation UX

The `resume` and `fork` subcommands are implemented as CLI rewrites into a final `TuiCli` value:

- [finalize_resume_interactive](file:///Users/bytedance/project/codex/codex-rs/cli/src/main.rs#L1502-L1528)
- [finalize_fork_interactive](file:///Users/bytedance/project/codex/codex-rs/cli/src/main.rs#L1530-L1554)
- [merge_interactive_cli_flags](file:///Users/bytedance/project/codex/codex-rs/cli/src/main.rs#L1556-L1586)

This is a notable design choice: instead of maintaining a separate execution path for resume/fork, the crate rewrites those commands back into the common TUI entry path with adjusted flags.

### 5. Feature-flag CLI management

The crate owns a small operational interface for feature flags:

- global `--enable` / `--disable` are converted into config overrides by [FeatureToggles::to_overrides](file:///Users/bytedance/project/codex/codex-rs/cli/src/main.rs#L622-L643)
- `codex features list` loads the effective config and prints sorted feature rows in [main.rs](file:///Users/bytedance/project/codex/codex-rs/cli/src/main.rs#L1106-L1170)
- `codex features enable/disable` persists changes to `config.toml` through [enable_feature_in_config](file:///Users/bytedance/project/codex/codex-rs/cli/src/main.rs#L1193-L1216)

The command also includes stage-aware messaging, especially warning users when enabling under-development features in [maybe_print_under_development_feature_warning](file:///Users/bytedance/project/codex/codex-rs/cli/src/main.rs#L1218-L1239).

## Public APIs exposed by the library crate

The library side is intentionally narrow and mostly exists so the binary can share reusable helpers with tests or future crates.

Exported items from [lib.rs](file:///Users/bytedance/project/codex/codex-rs/cli/src/lib.rs#L1-L73):

- command structs
  - `SeatbeltCommand`
  - `LandlockCommand`
  - `WindowsCommand`
- sandbox runners
  - `run_command_under_landlock`
  - `run_command_under_seatbelt`
  - `run_command_under_windows`
- login helpers
  - `read_api_key_from_stdin`
  - `run_login_status`
  - `run_login_with_api_key`
  - `run_login_with_chatgpt`
  - `run_login_with_device_code`
  - `run_login_with_device_code_fallback_to_browser`
  - `run_logout`

The library is not a general-purpose SDK. It mainly exposes helper routines that are still fundamentally CLI-oriented.

## Module-by-module analysis

### main.rs

File: [main.rs](file:///Users/bytedance/project/codex/codex-rs/cli/src/main.rs)

Responsibilities:

- defines the CLI schema
- dispatches subcommands
- applies root-level config and feature overrides
- guards remote mode usage
- bridges into TUI, exec, app server, MCP server, cloud tasks, and other workspace crates
- implements debug/feature helper commands that are CLI-local

Concrete delegation points:

- `exec` / `review` -> `codex_exec::run_main`
- `mcp-server` -> `codex_mcp_server::run_main`
- `app-server` -> `codex_app_server::run_main_with_transport`
- default interactive -> `codex_tui::run_main`
- `cloud` -> `codex_cloud_tasks::run_main`
- `responses-api-proxy` -> `codex_responses_api_proxy::run_main`
- `stdio-to-uds` -> `codex_stdio_to_uds::run`
- `exec-server` -> `codex_exec_server::run_main`

Interesting design details:

- Root config overrides are prepended, not appended, by [prepend_config_flags](file:///Users/bytedance/project/codex/codex-rs/cli/src/main.rs#L1374-L1383), so later subcommand-local overrides win.
- Remote websocket flags are intentionally restricted to interactive TUI flows through [reject_remote_mode_for_subcommand](file:///Users/bytedance/project/codex/codex-rs/cli/src/main.rs#L1385-L1417).
- `review` is not a standalone execution engine; it is re-expressed as `exec review`, reducing duplicated dispatch logic.

### login.rs

File: [login.rs](file:///Users/bytedance/project/codex/codex-rs/cli/src/login.rs)

Responsibilities:

- implements all direct `codex login` and `codex logout` UX
- initializes login-specific file logging
- mediates among browser login, device-code login, and API-key login
- loads config and honors forced-login-mode restrictions
- prints user-facing status messages and exits the process

Key APIs:

- [run_login_with_chatgpt](file:///Users/bytedance/project/codex/codex-rs/cli/src/login.rs#L131-L159)
- [run_login_with_api_key](file:///Users/bytedance/project/codex/codex-rs/cli/src/login.rs#L161-L188)
- [run_login_with_device_code](file:///Users/bytedance/project/codex/codex-rs/cli/src/login.rs#L217-L250)
- [run_login_with_device_code_fallback_to_browser](file:///Users/bytedance/project/codex/codex-rs/cli/src/login.rs#L252-L314)
- [run_login_status](file:///Users/bytedance/project/codex/codex-rs/cli/src/login.rs#L316-L345)
- [run_logout](file:///Users/bytedance/project/codex/codex-rs/cli/src/login.rs#L347-L364)

Notable design choices:

- Direct login commands return `!` and call `std::process::exit`, which keeps UX simple but makes the module more command-oriented than library-oriented.
- The module intentionally does not reuse the full TUI tracing stack. Instead it creates a small file-backed logger in [init_login_file_logging](file:///Users/bytedance/project/codex/codex-rs/cli/src/login.rs#L39-L105) to produce `codex-login.log`.
- API-key ingestion is intentionally stdin-only via [read_api_key_from_stdin](file:///Users/bytedance/project/codex/codex-rs/cli/src/login.rs#L190-L215), preventing keys from being exposed in shell history.

### debug_sandbox.rs

File: [debug_sandbox.rs](file:///Users/bytedance/project/codex/codex-rs/cli/src/debug_sandbox.rs)

Responsibilities:

- powers `codex sandbox macos`, `codex sandbox linux`, and `codex sandbox windows`
- builds a config suitable for standalone sandbox command execution
- creates a child process with a sandbox wrapper appropriate to the current OS
- preserves inherited stdio behavior
- optionally starts auxiliary managed-network proxy support
- maps the child exit status back to the parent process exit code

Key APIs:

- [run_command_under_seatbelt](file:///Users/bytedance/project/codex/codex-rs/cli/src/debug_sandbox.rs#L39-L68)
- [run_command_under_landlock](file:///Users/bytedance/project/codex/codex-rs/cli/src/debug_sandbox.rs#L70-L89)
- [run_command_under_windows](file:///Users/bytedance/project/codex/codex-rs/cli/src/debug_sandbox.rs#L91-L110)
- shared implementation in [run_command_under_sandbox](file:///Users/bytedance/project/codex/codex-rs/cli/src/debug_sandbox.rs#L119-L348)

Important flow:

1. Parse and load config overrides.
2. Build a sandbox-aware `Config`.
3. Compute cwd and environment.
4. Optionally start a managed network proxy.
5. Spawn a platform-specific sandbox wrapper.
6. Await child exit.
7. Forward child exit code through [exit_status.rs](file:///Users/bytedance/project/codex/codex-rs/cli/src/exit_status.rs#L1-L23).

Notable design details:

- The module supports both legacy `sandbox_mode` and newer permission-profile configs. The branching logic is in [load_debug_sandbox_config_with_codex_home](file:///Users/bytedance/project/codex/codex-rs/cli/src/debug_sandbox.rs#L403-L439).
- `--full-auto` is only allowed for legacy `sandbox_mode`; it is rejected for active permission-profile configs.
- Windows is handled differently: it captures stdout/stderr and then exits the current process with the child exit code from inside the helper path, which matches the sandbox backend’s API shape rather than the Unix/macOS spawn model.

Supporting macOS-only modules:

- [debug_sandbox/pid_tracker.rs](file:///Users/bytedance/project/codex/codex-rs/cli/src/debug_sandbox/pid_tracker.rs#L1-L275) tracks descendant PIDs using `kqueue` and `proc_listchildpids`.
- [debug_sandbox/seatbelt.rs](file:///Users/bytedance/project/codex/codex-rs/cli/src/debug_sandbox/seatbelt.rs#L1-L114) tails macOS `log stream`, correlates sandbox denial log events to tracked PIDs, and emits deduplicated denial summaries.

### mcp_cmd.rs

File: [mcp_cmd.rs](file:///Users/bytedance/project/codex/codex-rs/cli/src/mcp_cmd.rs)

Responsibilities:

- manages configured MCP servers in user config
- renders both human-readable and JSON views of effective MCP server state
- supports OAuth login/logout for streamable HTTP MCP servers
- validates naming and transport inputs

CLI/API surface:

- [McpCli](file:///Users/bytedance/project/codex/codex-rs/cli/src/mcp_cmd.rs#L31-L189) with `list`, `get`, `add`, `remove`, `login`, `logout`
- transport-aware add flow in [run_add](file:///Users/bytedance/project/codex/codex-rs/cli/src/mcp_cmd.rs#L239-L355)
- remove flow in [run_remove](file:///Users/bytedance/project/codex/codex-rs/cli/src/mcp_cmd.rs#L357-L388)
- OAuth login/logout flows in [run_login](file:///Users/bytedance/project/codex/codex-rs/cli/src/mcp_cmd.rs#L390-L441) and [run_logout](file:///Users/bytedance/project/codex/codex-rs/cli/src/mcp_cmd.rs#L443-L473)
- list/get renderers in [run_list](file:///Users/bytedance/project/codex/codex-rs/cli/src/mcp_cmd.rs#L475-L724) and [run_get](file:///Users/bytedance/project/codex/codex-rs/cli/src/mcp_cmd.rs#L726-L895)

Key behaviors:

- `add` supports either:
  - stdio launchers with command args and `--env`
  - streamable HTTP servers with `--url` and optional bearer-token env var
- config persistence goes through `ConfigEditsBuilder`
- OAuth support is partially auto-detected after `mcp add`
- scope negotiation supports discovery, explicit scopes, configured scopes, and a compatibility retry-without-scopes path in [perform_oauth_login_retry_without_scopes](file:///Users/bytedance/project/codex/codex-rs/cli/src/mcp_cmd.rs#L191-L237)

Design observations:

- This module is a good example of CLI-specific orchestration living close to persistence and auth wiring, while still delegating heavy logic to `codex_core`, `codex_mcp`, and `codex_rmcp_client`.
- Output formatting is intentionally duplicated for text and JSON, favoring predictable CLI output over abstracting into a rendering layer.

### marketplace_cmd.rs

File: [marketplace_cmd.rs](file:///Users/bytedance/project/codex/codex-rs/cli/src/marketplace_cmd.rs)

Responsibilities:

- manages plugin marketplaces below `codex plugin marketplace`
- delegates add/remove/upgrade work into plugin-management code in `codex_core`
- prints stable command-line outcome summaries

Key APIs:

- [MarketplaceCli](file:///Users/bytedance/project/codex/codex-rs/cli/src/marketplace_cmd.rs#L15-L78)
- add in [run_add](file:///Users/bytedance/project/codex/codex-rs/cli/src/marketplace_cmd.rs#L80-L115)
- upgrade in [run_upgrade](file:///Users/bytedance/project/codex/codex-rs/cli/src/marketplace_cmd.rs#L117-L131)
- remove in [run_remove](file:///Users/bytedance/project/codex/codex-rs/cli/src/marketplace_cmd.rs#L133-L151)
- outcome rendering in [print_upgrade_outcome](file:///Users/bytedance/project/codex/codex-rs/cli/src/marketplace_cmd.rs#L153-L189)

Important behavior:

- marketplace operations are intentionally nested under `plugin marketplace`, not exposed at top level
- add supports local directories and git-like sources, but sparse checkout is only meaningful for git-backed sources
- upgrade reports partial failures and bails if any marketplace upgrade failed

### responses_cmd.rs

File: [responses_cmd.rs](file:///Users/bytedance/project/codex/codex-rs/cli/src/responses_cmd.rs)

Responsibilities:

- exposes a hidden/internal raw Responses API bridge
- reads one JSON payload from stdin
- enforces `stream: true`
- constructs the configured authenticated model provider
- streams events and converts them into replayable JSON lines

Key APIs:

- [run_responses_command](file:///Users/bytedance/project/codex/codex-rs/cli/src/responses_cmd.rs#L11-L56)
- [response_event_to_json](file:///Users/bytedance/project/codex/codex-rs/cli/src/responses_cmd.rs#L58-L142)

Design note:

- The explicit event remapping keeps CLI output stable and replayable even if internal event enums are richer or named differently.

### app_cmd.rs and desktop_app/*

Files:

- [app_cmd.rs](file:///Users/bytedance/project/codex/codex-rs/cli/src/app_cmd.rs#L1-L25)
- [desktop_app/mod.rs](file:///Users/bytedance/project/codex/codex-rs/cli/src/desktop_app/mod.rs#L1-L22)
- [desktop_app/mac.rs](file:///Users/bytedance/project/codex/codex-rs/cli/src/desktop_app/mac.rs#L1-L317)
- [desktop_app/windows.rs](file:///Users/bytedance/project/codex/codex-rs/cli/src/desktop_app/windows.rs#L1-L132)

Responsibilities:

- powers `codex app`
- canonicalizes the requested workspace path
- opens an installed desktop app if available
- falls back to installer/download flows when the app is missing

Platform-specific behavior:

- macOS:
  - probes `/Applications/Codex.app` and `~/Applications/Codex.app`
  - chooses download URL based on architecture/translation status
  - downloads a DMG with `curl`
  - mounts with `hdiutil`
  - copies the app bundle with `ditto`
  - launches via `open -a`
- Windows:
  - checks installed apps via PowerShell `Get-StartApps`
  - opens installed app via `explorer.exe shell:AppsFolder\...`
  - otherwise opens a Microsoft Store installer URL or fallback web URL

This code is intentionally pragmatic and shell-command-centric because it has to integrate with native platform app-install mechanisms.

### wsl_paths.rs

File: [wsl_paths.rs](file:///Users/bytedance/project/codex/codex-rs/cli/src/wsl_paths.rs#L1-L61)

Responsibility:

- normalizes Windows-style absolute paths into `/mnt/<drive>/...` when running under WSL

Current use:

- update-command execution on non-Windows platforms uses it to avoid path mismatches when a command path originates from Windows context.

### exit_status.rs

File: [exit_status.rs](file:///Users/bytedance/project/codex/codex-rs/cli/src/exit_status.rs#L1-L23)

Responsibility:

- converts a child `ExitStatus` into a process exit code, preserving Unix signal semantics as `128 + signal`

This is a tiny but important utility because the sandbox commands behave like wrappers and need to preserve child exit behavior.

## Execution flow by scenario

### Default interactive run

1. `main()` uses arg0 dispatch to resolve auxiliary executable paths.
2. `cli_main()` parses CLI state.
3. Root feature toggles become config overrides.
4. With no subcommand, the code calls [run_interactive_tui](file:///Users/bytedance/project/codex/codex-rs/cli/src/main.rs#L1439-L1491).
5. TUI returns `AppExitInfo`.
6. The CLI prints token usage/resume hints and optionally runs an update action.

### Non-interactive `exec` / `review`

1. Parse top-level flags.
2. Reject remote mode.
3. Propagate shared exec-root options and config overrides.
4. Delegate to `codex_exec::run_main`.
5. `review` is rewritten into `exec review`.

### `mcp add`

1. Parse transport flags.
2. Load config and validate server name.
3. Load current global MCP server definitions from config.
4. Materialize a `McpServerTransportConfig`.
5. Persist the updated server map with `ConfigEditsBuilder`.
6. Probe OAuth support and optionally begin OAuth login immediately.

### `sandbox linux|macos|windows`

1. Build sandbox config from CLI overrides and compatibility rules.
2. Derive environment from shell environment policy.
3. Optionally start managed network proxy.
4. Spawn platform-specific sandbox wrapper.
5. Wait for completion and return the child’s exit status.

### `debug clear-memories`

1. Load config with profile-aware overrides.
2. Open state DB if present.
3. Clear memory-related DB tables through `StateRuntime`.
4. Clear memory-root directory contents through `codex_core`.
5. Print a combined status message.

## Dependency relationships

### Direct infrastructure dependencies

- `clap` and `clap_complete` define and render the CLI grammar.
- `tokio` drives async process spawning, stdin reads, and network/service entrypoints.
- `anyhow` is used for almost all internal error propagation.
- `serde_json` and `toml` support JSON rendering and CLI config overrides.

### Workspace crates that carry most product logic

- `codex-core`
  - config loading/editing
  - prompt input construction
  - plugin and marketplace operations
  - sandbox/network policy helpers
  - memory cleanup
- `codex-tui`
  - interactive UI and app exit metadata
- `codex-exec`
  - non-interactive execution and review mode
- `codex-app-server` / `codex-app-server-protocol`
  - app server runtime and schema/bindings generation
- `codex-mcp`, `codex-mcp-server`, `codex-rmcp-client`
  - MCP server management and OAuth flows
- `codex-login`
  - auth storage and login implementations
- `codex-models-manager` / `codex-model-provider` / `codex-api`
  - model catalog and raw Responses streaming
- `codex-state`
  - local state DB used by debug memory reset
- `codex-cloud-tasks`, `codex-exec-server`, `codex-stdio-to-uds`
  - specialized service entrypoints

### Platform-specific dependencies

- macOS-only: `libc` APIs, `sandbox-exec`, `log stream`, `hdiutil`, `ditto`, `open`
- Windows-only: `powershell.exe`, `explorer.exe`, `codex-windows-sandbox`

The crate deliberately keeps many platform-specific concerns local rather than trying to abstract them behind a fully uniform interface.

## Testing strategy

The crate has both inline unit tests and external integration tests.

### Inline unit tests

Most modules contain focused parser/utility tests:

- [main.rs tests](file:///Users/bytedance/project/codex/codex-rs/cli/src/main.rs#L1594-L2358)
  - command parsing
  - hidden/visible command behavior
  - resume/fork flag merging
  - remote-mode restrictions
  - app-server listen/auth flag parsing
  - feature toggle override generation
- [marketplace_cmd.rs tests](file:///Users/bytedance/project/codex/codex-rs/cli/src/marketplace_cmd.rs#L191-L240)
  - sparse flag parsing
  - optional marketplace-name parsing
- [responses_cmd.rs tests](file:///Users/bytedance/project/codex/codex-rs/cli/src/responses_cmd.rs#L144-L243)
  - event-name and envelope mapping stability
- [login.rs tests](file:///Users/bytedance/project/codex/codex-rs/cli/src/login.rs#L393-L408)
  - API-key masking
- [debug_sandbox.rs tests](file:///Users/bytedance/project/codex/codex-rs/cli/src/debug_sandbox.rs#L465-L571)
  - permission-profile vs legacy sandbox-mode behavior
- [debug_sandbox/pid_tracker.rs tests](file:///Users/bytedance/project/codex/codex-rs/cli/src/debug_sandbox/pid_tracker.rs#L277-L372)
  - descendant tracking on macOS
- platform utility tests in desktop-app and WSL modules

### Integration tests

The `tests/` directory verifies end-to-end command behavior by invoking the built `codex` binary.

- [debug_clear_memories.rs](file:///Users/bytedance/project/codex/codex-rs/cli/tests/debug_clear_memories.rs#L1-L132)
  - seeds SQLite state and memory directories, then verifies cleanup
- [debug_models.rs](file:///Users/bytedance/project/codex/codex-rs/cli/tests/debug_models.rs#L1-L40)
  - checks that debug model dumps are valid JSON
- [execpolicy.rs](file:///Users/bytedance/project/codex/codex-rs/cli/tests/execpolicy.rs#L1-L119)
  - validates JSON output for policy matches and justifications
- [features.rs](file:///Users/bytedance/project/codex/codex-rs/cli/tests/features.rs#L1-L91)
  - verifies config edits, warnings, and sorted list output
- [marketplace_add.rs](file:///Users/bytedance/project/codex/codex-rs/cli/tests/marketplace_add.rs#L1-L118)
  - validates local marketplace addition and error cases
- [marketplace_remove.rs](file:///Users/bytedance/project/codex/codex-rs/cli/tests/marketplace_remove.rs#L1-L70)
  - validates config/install-root cleanup
- [marketplace_upgrade.rs](file:///Users/bytedance/project/codex/codex-rs/cli/tests/marketplace_upgrade.rs#L1-L36)
  - verifies new subcommand placement under `plugin marketplace`
- [mcp_add_remove.rs](file:///Users/bytedance/project/codex/codex-rs/cli/tests/mcp_add_remove.rs#L1-L228)
  - checks config persistence for stdio/HTTP MCP servers
- [mcp_list.rs](file:///Users/bytedance/project/codex/codex-rs/cli/tests/mcp_list.rs#L1-L166)
  - checks human-readable and JSON rendering, including masking

Testing style observations:

- Integration tests are command-centric and user-observable.
- Most tests use temporary `CODEX_HOME` directories, which keeps them isolated and realistic.
- Parser behavior is heavily covered, which fits this crate’s role as a CLI façade.

## Design patterns and architectural choices

### Thin-shell architecture

The crate is intentionally thin. Most logic is delegated outward. That keeps command ownership local without turning this crate into the center of the product.

### CLI-first ergonomics over purity

Several modules prefer process-level UX patterns over pure return-value APIs:

- login functions exit directly
- sandbox wrappers preserve child exit semantics by terminating the current process
- output formatting is duplicated for predictable CLI rendering

This is appropriate for a top-level executable crate, but it also makes some modules harder to reuse as generic libraries.

### Compatibility-preserving command evolution

The code shows careful migration behavior:

- deprecated login flags still parse enough to emit guidance
- hidden commands remain parseable
- legacy feature names are still accepted in tests
- marketplace commands moved under `plugin marketplace`, with tests verifying the old location no longer parses
- MCP OAuth retries without scopes for older providers

### Platform-aware pragmatism

Instead of forcing a uniform abstraction over OS behavior, the crate keeps strongly platform-specific code close to the command that needs it. Examples:

- macOS app installation flow
- Windows shell/app-launch flow
- macOS sandbox denial logging via `log stream`
- WSL path normalization for update commands

### Config-precedence discipline

The precedence rules are explicit and consistent:

- root overrides apply broadly
- subcommand-local overrides win
- resume/fork merge into the shared TUI path with subcommand values taking precedence

This is important because the crate exists largely to translate CLI surface area into config/runtime behavior.

## Risk areas and maintenance hotspots

- `main.rs` is large and central; adding new subcommands increases the chance of precedence or dispatch regressions.
- `login.rs` and `debug_sandbox.rs` are tightly coupled to process exit behavior, which complicates composability and testability.
- MCP list/get formatting duplicates transport rendering logic in both text and JSON paths.
- Desktop app installation flows depend on external system tools and specific output formats, especially `hdiutil`.
- The macOS denial logger depends on log schema stability and process-tree tracking accuracy.

## Open questions

These are not confirmed bugs; they are areas worth clarifying for future maintainers.

1. Should `main.rs` be split further?
   - The command surface is large enough that feature-specific command routers may now justify extraction into dedicated modules.

2. Should login commands stop calling `std::process::exit` internally?
   - Returning structured results would improve composability and testing, but would slightly complicate current CLI UX code.

3. Why does `mcp add` validate config overrides by loading config, even though add/remove mostly mutate config directly?
   - This may be intentional to fail early on invalid overrides, but the coupling is subtle and deserves explicit rationale.

4. Should MCP rendering be centralized?
   - `list --json`, `list` text mode, `get --json`, and `get` text mode each reconstruct transport details separately.

5. Is the Windows sandbox flow intentionally asymmetric with Unix/macOS?
   - It currently captures output and exits from inside the helper path rather than returning a child handle to the shared path.

6. How stable is the macOS denial-log parsing contract?
   - `seatbelt.rs` depends on log-stream NDJSON containing `eventMessage` with a specific text shape.

7. Should the desktop-app install/open logic live in this crate long term?
   - It is operationally correct here, but it is much more platform-installation code than CLI grammar code.

## Bottom line

`codex-cli` is best understood as the product’s command router and UX adapter. It owns:

- the CLI grammar
- flag/config precedence
- command dispatch
- a few UX-heavy local integrations

It does not own most of the application’s deep logic. That logic is intentionally delegated to the surrounding workspace crates. The result is a crate that is operationally important, broad in surface area, and relatively thin in domain depth.
