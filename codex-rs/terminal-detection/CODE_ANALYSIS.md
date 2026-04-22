# codex-terminal-detection crate analysis

## Scope

This document analyzes the `codex-terminal-detection` crate at `/Users/bytedance/project/codex/codex-rs/terminal-detection`.

Primary references:

- [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/Cargo.toml)
- [lib.rs](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L1-L510)
- [terminal_tests.rs](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/terminal_tests.rs#L1-L843)

Important consumers:

- [login/default_client.rs](file:///Users/bytedance/project/codex/codex-rs/login/src/auth/default_client.rs#L131-L155)
- [core/session.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/session/session.rs#L436-L456)
- [cli/main.rs](file:///Users/bytedance/project/codex/codex-rs/cli/src/main.rs#L1450-L1466)
- [tui/diff_render.rs](file:///Users/bytedance/project/codex/codex-rs/tui/src/diff_render.rs#L1048-L1090)
- [tui/lib.rs](file:///Users/bytedance/project/codex/codex-rs/tui/src/lib.rs#L1631-L1640)
- [tui/tui.rs](file:///Users/bytedance/project/codex/codex-rs/tui/src/tui.rs#L477-L487)
- [tui/chatwidget.rs](file:///Users/bytedance/project/codex/codex-rs/tui/src/chatwidget.rs#L5101-L5105)
- [tui/chat_composer.rs](file:///Users/bytedance/project/codex/codex-rs/tui/src/bottom_pane/chat_composer.rs#L527-L530)

## High-level role

`codex-terminal-detection` is a very small, single-module crate that turns process environment signals into:

- a structured terminal classification via `TerminalInfo`,
- a sanitized terminal token for HTTP/OpenTelemetry user-agent strings via `user_agent()`,
- a small amount of terminal/multiplexer-aware behavior for the TUI.

The crate does not own rendering, telemetry pipelines, HTTP clients, or shell integration. Instead, it centralizes terminal detection so the rest of the workspace does not duplicate ad hoc environment probing.

The module comment in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L1-L4) is accurate: the crate mainly feeds terminal metadata into user-agent logging and terminal-specific TUI choices.

## Package structure

This crate is intentionally minimal:

- [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/Cargo.toml#L1-L18) defines a library crate named `codex_terminal_detection`.
- [lib.rs](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L1-L510) contains all production code.
- [terminal_tests.rs](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/terminal_tests.rs#L1-L843) contains the entire test suite, included via `#[cfg(test)]`.

There are no submodules beyond the test module, no feature flags, and no build script.

## Public API

### Data types

The crate exposes three public data types in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L8-L68):

- `TerminalInfo`: the main output struct, carrying `name`, `term_program`, `version`, `term`, and `multiplexer`.
- `TerminalName`: an enum of recognized terminal categories such as `Iterm2`, `WezTerm`, `WindowsTerminal`, `Dumb`, and `Unknown`.
- `Multiplexer`: an enum for `Tmux { version: Option<String> }` and `Zellij {}`.

`TerminalInfo` also exposes one convenience method in [TerminalInfo::is_zellij](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L208-L211), which is a narrow API intended for downstream UI decisions.

### Functions

The public functional surface is only two functions in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L262-L272):

- `user_agent() -> String`: returns a sanitized token suitable for embedding inside a larger HTTP user-agent string.
- `terminal_info() -> TerminalInfo`: returns structured terminal metadata for the current process.

This is a deliberately small API. Most implementation details remain private, which keeps the rest of the workspace dependent on normalized outputs rather than on raw detection heuristics.

## Concrete responsibilities

### 1. Normalize terminal identity

The crate converts a mix of environment variables into a consistent `TerminalInfo` shape in [detect_terminal_info_from_env](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L274-L375).

Handled terminal categories include:

- Apple Terminal
- Ghostty
- iTerm2
- Warp
- VS Code integrated terminal
- WezTerm
- kitty
- Alacritty
- Konsole
- GNOME Terminal
- VTE-based terminals
- Windows Terminal
- `TERM=dumb`
- unknown terminals

### 2. Detect multiplexers

The crate independently classifies terminal multiplexers in [detect_multiplexer](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L377-L392):

- `tmux` is detected from `TMUX` or `TMUX_PANE`.
- `zellij` is detected from `ZELLIJ`, `ZELLIJ_SESSION_NAME`, or `ZELLIJ_VERSION`.

That multiplexer metadata is preserved in `TerminalInfo` even when the underlying terminal remains unknown.

### 3. Prefer underlying terminal inside tmux

When `TERM_PROGRAM=tmux`, the crate attempts to identify the real client terminal by calling `tmux display-message` in [tmux_client_info](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L438-L457) and then re-mapping the result in [terminal_from_tmux_client_info](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L398-L420).

This is an important design choice: it avoids collapsing all tmux sessions into a generic “tmux” terminal and instead attributes them to the actual client terminal when possible.

### 4. Produce safe header tokens

The crate formats a compact token in [TerminalInfo::user_agent_token](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L173-L206) and sanitizes it in [sanitize_header_value](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L459-L469).

The allowed character set is intentionally narrow:

- ASCII alphanumeric
- `-`
- `_`
- `.`
- `/`

Everything else is replaced with `_`, which makes the output safe to embed in HTTP header values.

## Detection flow

The central flow lives in [detect_terminal_info_from_env](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L274-L375).

### 1. Detect multiplexer first

`detect_terminal_info_from_env` starts by calling `detect_multiplexer`. This lets the final `TerminalInfo` preserve multiplexer context regardless of which terminal probe succeeds later.

### 2. Give `TERM_PROGRAM` highest priority

If `TERM_PROGRAM` is present and non-empty:

- it wins over later probes such as `WEZTERM_VERSION` or `WT_SESSION`,
- `TERM_PROGRAM_VERSION` is captured as the terminal version,
- `terminal_name_from_term_program` maps the raw string into a `TerminalName`.

This precedence is documented directly in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L276-L283) and is confirmed by tests such as [detects_term_program](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/terminal_tests.rs#L58-L123).

### 3. Special-case `TERM_PROGRAM=tmux`

If `TERM_PROGRAM` says `tmux` and a tmux multiplexer is also detected:

- `tmux_client_info()` fetches `#{client_termtype}` and `#{client_termname}`,
- `split_term_program_and_version()` splits a string like `ghostty 1.2.3`,
- the code builds `TerminalInfo` from the underlying client instead of from tmux itself.

This path is implemented in [detect_terminal_info_from_env](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L291-L302) and [terminal_from_tmux_client_info](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L398-L420), with coverage in:

- [detects_tmux_multiplexer](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/terminal_tests.rs#L279-L302)
- [detects_tmux_client_termtype](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/terminal_tests.rs#L321-L344)
- [detects_tmux_client_termname](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/terminal_tests.rs#L346-L369)
- [detects_tmux_term_program_uses_client_termtype](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/terminal_tests.rs#L371-L397)

### 4. Fall back through terminal-specific env vars

If `TERM_PROGRAM` is absent, the crate checks a terminal-specific chain:

- `WEZTERM_VERSION`
- iTerm2 session/profile variables
- `TERM_SESSION_ID` for Apple Terminal
- kitty markers
- Alacritty markers
- `KONSOLE_VERSION`
- `GNOME_TERMINAL_SCREEN`
- `VTE_VERSION`
- `WT_SESSION`

This chain is implemented in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L305-L368).

### 5. Use `TERM` as a capability fallback

If no stronger signal exists:

- `TERM=dumb` maps to `TerminalName::Dumb`,
- any other non-empty `TERM` becomes `term: Some(...)` with `TerminalName::Unknown`,
- if even `TERM` is missing, the result is fully unknown.

That behavior is implemented in [TerminalInfo::from_term](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L146-L160) and exercised in [detects_term_fallbacks](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/terminal_tests.rs#L793-L843).

## API behavior details

### `terminal_info()`

[terminal_info()](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L267-L272):

- reads the live process environment through `ProcessEnvironment`,
- computes the result once,
- caches it in a `static OnceLock<TerminalInfo>`,
- returns a clone on every call.

This means terminal detection is process-global and immutable after first use.

### `user_agent()`

[user_agent()](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L262-L265) delegates to `terminal_info().user_agent_token()`.

The formatting rules in [TerminalInfo::user_agent_token](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L173-L206) are:

- prefer `term_program` and append `/version` when available,
- otherwise use `term`,
- otherwise use a canonical name per `TerminalName`,
- sanitize the final string for header safety.

This keeps the token compact and stable enough for telemetry.

### Name normalization

[terminal_name_from_term_program](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L471-L495) normalizes candidate strings by:

- trimming,
- removing spaces, hyphens, underscores, and dots,
- lowercasing ASCII,
- matching against a small known synonym table.

That allows inputs like `iTerm.app`, `iTerm2`, `Apple_Terminal`, and `gnome-terminal` to resolve cleanly.

## Internal design choices

### Environment abstraction for testability

The private [Environment](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L216-L240) trait isolates environment access and tmux probing.

That provides two benefits:

- production code can use `ProcessEnvironment`,
- tests can inject a fake environment without touching real process env vars.

This is the main reason the test suite is broad despite the crate’s small size.

### Single-module, mostly pure logic

Most helpers are deterministic string/option transformations:

- [split_term_program_and_version](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L431-L435)
- [terminal_name_from_term_program](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L471-L495)
- [format_terminal_version](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L497-L501)
- [none_if_whitespace](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L504-L505)
- [sanitize_header_value](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L462-L469)

Only two places touch the outside world:

- `std::env` access in [ProcessEnvironment::var](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L245-L255)
- `tmux` subprocess execution in [tmux_display_message](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L445-L457)

That keeps the code easy to reason about.

### Conservative error handling

The crate intentionally degrades to `Unknown` rather than failing:

- invalid UTF-8 env vars are ignored with a `tracing::warn!` in [ProcessEnvironment::var](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L247-L253),
- failed tmux commands simply return `None`,
- empty and whitespace-only values are filtered out by `none_if_whitespace`.

This is appropriate because terminal detection is diagnostic/UX metadata, not a correctness-critical subsystem.

## Dependencies

[Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/Cargo.toml#L1-L18) shows that the crate has almost no dependency surface.

### Runtime dependencies

- `tracing`: only used to warn when an environment variable exists but is not valid UTF-8.

### Dev dependencies

- `pretty_assertions`: improves test failure diffs for the many `assert_eq!` checks.

### Standard library usage

The crate relies heavily on std-only primitives:

- `OnceLock` for memoization,
- `std::env` for environment reads,
- `std::process::Command` for tmux probing.

## Testing

The test suite in [terminal_tests.rs](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/terminal_tests.rs#L1-L843) is comprehensive relative to crate size.

### Test strategy

Tests use [FakeEnvironment](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/terminal_tests.rs#L5-L40), which implements the private `Environment` trait and lets each test specify:

- fake env variables,
- fake tmux client info,
- expected `TerminalInfo`,
- expected user-agent token.

This avoids reliance on the host terminal.

### Covered cases

The suite covers:

- `TERM_PROGRAM` precedence and version handling
- every recognized terminal family
- tmux multiplexer detection and tmux client overrides
- zellij detection
- fallback to `TERM`
- `TERM=dumb`
- `TerminalInfo::is_zellij`

### Gaps

The tests are strong on classification logic but lighter on low-level helper edge cases:

- there is no direct unit test for `sanitize_header_value`,
- there is no test for invalid UTF-8 env var handling,
- there is no test for `tmux_display_message` command failures,
- there is no test for `OnceLock` caching behavior.

### Validation status

`cargo test -p codex-terminal-detection` passes in the workspace root on this machine, with 20 tests passing.

## Downstream usage and impact

The crate has two main categories of consumers.

### 1. User-agent and telemetry identity

[login/default_client.rs](file:///Users/bytedance/project/codex/codex-rs/login/src/auth/default_client.rs#L131-L155) uses `user_agent()` as part of the larger Codex HTTP user-agent string, combining:

- product/build version,
- OS information,
- originator,
- terminal token from this crate,
- optional user-configured suffix.

[core/session.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/session/session.rs#L440-L456) also stores the terminal token as session telemetry metadata.

So this crate directly influences what backend systems and telemetry pipelines see as the client terminal type.

### 2. TUI behavior and ergonomics

The TUI uses `terminal_info()` for terminal-specific behavior:

- [cli/main.rs](file:///Users/bytedance/project/codex/codex-rs/cli/src/main.rs#L1450-L1466) warns or refuses to start interactive mode when `TERM=dumb`.
- [tui/diff_render.rs](file:///Users/bytedance/project/codex/codex-rs/tui/src/diff_render.rs#L1048-L1090) adjusts diff color policy based on detected terminal identity, especially Windows Terminal.
- [tui/lib.rs](file:///Users/bytedance/project/codex/codex-rs/tui/src/lib.rs#L1631-L1640) disables alternate-screen mode automatically under Zellij.
- [tui/tui.rs](file:///Users/bytedance/project/codex/codex-rs/tui/src/tui.rs#L477-L487) caches an `is_zellij` flag during startup.
- [tui/chat_composer.rs](file:///Users/bytedance/project/codex/codex-rs/tui/src/bottom_pane/chat_composer.rs#L527-L530) uses the same Zellij signal in composer behavior.
- [tui/chatwidget.rs](file:///Users/bytedance/project/codex/codex-rs/tui/src/chatwidget.rs#L5103-L5104) derives terminal-specific keybindings from `terminal_info()`.

This confirms the crate is not just telemetry plumbing; it also gates visible UX decisions.

## Strengths

- Very small public API with clear semantics.
- Good separation between detection logic and environment access.
- Strong testability through the `Environment` abstraction.
- Explicit detection precedence, including tricky tmux handling.
- Safe degradation to `Unknown` instead of panicking or hard-failing.
- Useful downstream impact despite minimal implementation size.

## Limitations and trade-offs

### Process-global caching

`terminal_info()` caches the first observed result in a `OnceLock`. That is efficient, but it means:

- tests must avoid the public cached function when they need isolation,
- any env changes after first access are ignored for the life of the process.

This is likely intentional, but it is a real behavioral constraint.

### Synchronous tmux subprocess

The tmux path runs `tmux display-message` synchronously in [tmux_display_message](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L445-L457). That is acceptable because of `OnceLock` amortization, but it still means first-use detection can block on an external command.

### Narrow version parsing for tmux client termtype

[split_term_program_and_version](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L431-L435) splits on whitespace and only captures the second token as the version. That works for strings like `ghostty 1.2.3`, but it assumes a very simple tmux `client_termtype` format.

### Fixed recognition table

The terminal recognition table in [terminal_name_from_term_program](file:///Users/bytedance/project/codex/codex-rs/terminal-detection/src/lib.rs#L471-L495) is manual and closed. New terminals or uncommon aliases require code changes.

## Open questions

These are the main design questions that seem worth confirming with maintainers.

1. Should `terminal_info()` remain permanently cached, or should there be an internal escape hatch for tests and rare re-detection scenarios?
2. Is a synchronous `tmux` subprocess acceptable on first use in all startup paths, especially latency-sensitive CLI/TUI initialization?
3. Should the crate expose a richer public API for multiplexers, such as `is_tmux()` or accessors for tmux version, instead of requiring downstream pattern matches?
4. Should `sanitize_header_value` and invalid UTF-8 handling receive direct unit tests, since they affect telemetry correctness?
5. Should `TERM_PROGRAM` always override `WT_SESSION`, or are there cases where Windows Terminal should win even when another `TERM_PROGRAM` is set?
6. Is there value in supporting more terminal families or aliases, or is the current recognized set intentionally constrained to the terminals Codex actively optimizes for?

## Bottom line

`codex-terminal-detection` is a focused utility crate that does one job well: convert messy terminal and multiplexer environment signals into stable, reusable metadata. Its design is pragmatic Rust:

- small API surface,
- private implementation details,
- std-heavy implementation,
- explicit fallbacks,
- test-friendly abstraction,
- low failure blast radius.

The crate’s importance is larger than its size because its outputs feed both backend-facing user-agent/telemetry strings and user-facing TUI behavior.
