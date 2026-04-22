# codex-utils-elapsed Code Analysis

## Overview

This crate is a very small formatting utility that converts a `std::time::Duration` into a compact, human-readable elapsed-time string. Its scope is intentionally narrow:

- expose one public helper for formatting elapsed durations
- keep formatting logic dependency-free and deterministic
- support short UI-friendly outputs rather than verbose prose

The entire implementation lives in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/elapsed/src/lib.rs#L1-L71), and the crate manifest is minimal in [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/utils/elapsed/Cargo.toml#L1-L8).

## Concrete Responsibilities

The crate currently has one concrete responsibility:

1. Convert a `Duration` into one of three compact display formats:
   - `< 1s` as milliseconds, for example `250ms`
   - `1s..60s` as fractional seconds with two decimals, for example `1.50s`
   - `>= 60s` as minutes and zero-padded seconds, for example `1m 15s`

It does not currently:

- parse duration strings
- localize output
- format hours, days, or larger units explicitly
- offer configurable precision or style
- expose structured formatting metadata

## Manifest And Build Integration

The manifest in [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/utils/elapsed/Cargo.toml#L1-L8) shows that the crate inherits workspace metadata:

- package name: `codex-utils-elapsed`
- version: inherited from the workspace
- edition: inherited from the workspace
- license: inherited from the workspace
- lints: inherited from the workspace

The workspace root defines the inherited values in [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/Cargo.toml#L99-L106):

- version `0.0.0`
- edition `2024`
- license `Apache-2.0`

There are no crate-specific dependencies, dev-dependencies, features, or build scripts. Bazel integration is equally small in [BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/utils/elapsed/BUILD.bazel#L1-L6), which exports the crate as `codex_utils_elapsed`.

## Module Structure

There is only one Rust module:

- [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/elapsed/src/lib.rs#L1-L71)

Inside that file:

- `format_duration(Duration) -> String` is the public API
- `format_elapsed_millis(i64) -> String` is a private helper containing the branch logic
- `tests` is an inline unit test module

This is effectively a single-purpose utility crate rather than a layered library.

## Public API

The only public API is [format_duration](file:///Users/bytedance/project/codex/codex-rs/utils/elapsed/src/lib.rs#L3-L12):

```rust
pub fn format_duration(duration: Duration) -> String {
    let millis = duration.as_millis() as i64;
    format_elapsed_millis(millis)
}
```

API characteristics:

- input is `std::time::Duration`, so callers cannot pass negative durations
- output is an owned `String`
- formatting policy is fixed; callers cannot select alternate units or precision
- the function is pure and allocation behavior is limited to building the output string

## Internal Flow

The runtime flow is simple and linear:

1. `format_duration` receives a `Duration`
2. it converts the value to total milliseconds using `duration.as_millis()`
3. it casts that `u128` millisecond count to `i64`
4. it delegates to the private formatter
5. `format_elapsed_millis` selects a display format based on thresholds

The branch logic lives in [format_elapsed_millis](file:///Users/bytedance/project/codex/codex-rs/utils/elapsed/src/lib.rs#L14-L24):

```rust
fn format_elapsed_millis(millis: i64) -> String {
    if millis < 1000 {
        format!("{millis}ms")
    } else if millis < 60_000 {
        format!("{:.2}s", millis as f64 / 1000.0)
    } else {
        let minutes = millis / 60_000;
        let seconds = (millis % 60_000) / 1000;
        format!("{minutes}m {seconds:02}s")
    }
}
```

### Branch Semantics

- sub-second durations preserve integer millisecond precision only
- second-range durations use `f64` division and always print two decimals
- minute-range durations truncate to whole seconds within the minute display
- durations longer than one hour are still expressed only in minutes and seconds

Example outputs based on current behavior:

- `0ms`
- `250ms`
- `1.50s`
- `60.00s` for `59_999ms`
- `1m 00s` for `60_000ms`
- `60m 01s` for `3_601_000ms`

## Downstream Usage

The crate is registered as a workspace dependency in [workspace Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/Cargo.toml#L178-L199) and is consumed by the TUI crate in [tui/Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/tui/Cargo.toml#L54-L58).

One concrete usage appears in [render.rs](file:///Users/bytedance/project/codex/codex-rs/tui/src/exec_cell/render.rs#L233-L245), where command execution durations are rendered in the UI:

```rust
let duration = call
    .duration
    .map(format_duration)
    .unwrap_or_else(|| "unknown".to_string());
```

This context explains the crate’s design:

- output needs to be short enough for status lines
- precision beyond milliseconds or two decimal places is unnecessary
- a stable, allocation-light helper is preferable to pulling in a heavier formatting crate

## Dependencies

Runtime dependencies:

- none beyond the Rust standard library

Workspace/build dependencies:

- workspace package metadata inherited from [workspace Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/Cargo.toml#L99-L106)
- Bazel wrapper in [BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/utils/elapsed/BUILD.bazel#L1-L6)

This keeps compile time, maintenance surface, and behavioral complexity extremely low.

## Testing

Tests are inline unit tests in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/elapsed/src/lib.rs#L26-L71). They cover four cases:

- [test_format_duration_subsecond](file:///Users/bytedance/project/codex/codex-rs/utils/elapsed/src/lib.rs#L30-L39): verifies millisecond formatting and zero
- [test_format_duration_seconds](file:///Users/bytedance/project/codex/codex-rs/utils/elapsed/src/lib.rs#L41-L51): verifies two-decimal seconds and the `59_999ms -> 60.00s` case
- [test_format_duration_minutes](file:///Users/bytedance/project/codex/codex-rs/utils/elapsed/src/lib.rs#L53-L64): verifies minute formatting, zero-padded seconds, and long durations
- [test_format_duration_one_hour_has_space](file:///Users/bytedance/project/codex/codex-rs/utils/elapsed/src/lib.rs#L66-L70): verifies exact one-hour rendering as `60m 00s`

Observed test status:

- `cargo test -p codex-utils-elapsed` passes
- 4 unit tests pass
- 0 doc tests exist
- no integration tests exist under a `tests/` directory

## Design Assessment

### Strengths

- Minimal API surface makes the crate easy to understand and reuse.
- No external dependencies reduce maintenance and compile-time cost.
- Formatting thresholds are explicit and easy to audit.
- The function is pure and deterministic, which makes unit testing straightforward.
- The output matches narrow UI-display use cases well.

### Tradeoffs

- The API is intentionally inflexible; alternate display styles require code changes.
- The minute branch does not show hours, so long runtimes become large minute counts.
- The seconds branch rounds using floating-point formatting, while the minute branch truncates seconds.
- Internal logic is based on milliseconds only, so sub-millisecond precision is discarded.

### Rust-Specific Notes

- Accepting `Duration` in the public API is idiomatic and prevents negative-duration misuse.
- Keeping the helper private avoids exposing an `i64`-based API that would be easier to misuse.
- The crate does not need traits, generics, lifetimes, or custom error types because the problem is narrow and total.

## Open Questions

1. Should `59_999ms` really render as `60.00s`, or should boundary-rounding normalize to `1m 00s` for consistency with the next branch?
2. Should the implementation avoid `as i64` on `duration.as_millis()`? Very large `Duration` values can overflow the intended semantic range when cast from `u128` to `i64`.
3. Should long durations remain minute-only, or should the formatter eventually support `h`, `m`, and `s` units for values over one hour?
4. Should the crate expose style variants, such as terse (`1m15s`) versus spaced (`1m 15s`) or exact integer seconds versus rounded decimals?
5. Should boundary behavior be tested more exhaustively around `999ms`, `1000ms`, `59_994ms`, `59_995ms`, `59_999ms`, and `60_000ms`?
6. Should there be integration or snapshot tests in the TUI layer to validate that status-line rendering still matches UI expectations?

## Summary

`codex-utils-elapsed` is a deliberately tiny workspace utility focused on one job: compact elapsed-time formatting for UI display. Its implementation is easy to reason about, dependency-free, and well aligned with short status rendering in the TUI. The main design risks are not structural complexity but edge-case policy decisions: boundary rounding, very large durations, and whether longer runtimes should eventually use richer units than minutes and seconds.
