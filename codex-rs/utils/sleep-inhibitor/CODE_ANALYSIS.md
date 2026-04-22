# codex-utils-sleep-inhibitor: Code Analysis

## Overview

`codex-utils-sleep-inhibitor` is a small cross-platform utility crate that prevents the host machine from entering idle sleep while Codex is actively processing a turn. The crate exposes a single public type, `SleepInhibitor`, and hides platform-specific implementations behind conditional compilation.

The design goal is intentionally narrow:

- keep the public API tiny;
- avoid leaking OS-specific concepts into callers;
- fail softly if sleep inhibition is unavailable;
- ensure acquired OS resources are released via RAII or process cleanup.

The crate is consumed by the TUI layer, where it is toggled alongside turn lifecycle state.

## Responsibilities

The crate has six concrete responsibilities:

1. Track whether the caller has requested idle-sleep prevention for the current turn.
2. Respect a caller-provided feature flag (`enabled`) so the behavior can be disabled without changing call sites.
3. Select a platform-specific backend at compile time.
4. Acquire the relevant OS-specific sleep-prevention mechanism when a turn starts.
5. Release that mechanism when the turn ends or the backend is dropped.
6. Log failures as warnings instead of surfacing them as user-facing errors.

It does **not** try to:

- expose detailed runtime status about whether the OS accepted an inhibition request;
- coordinate multiple concurrent callers;
- prevent every kind of sleep/display idle behavior on every platform;
- offer async APIs, retries with backoff, or health metrics.

## Public API

The public surface is defined entirely in `src/lib.rs`.

### `SleepInhibitor`

Fields:

- `enabled: bool` — whether the feature is allowed to take effect.
- `turn_running: bool` — caller-visible logical state.
- `platform: imp::SleepInhibitor` — backend selected by `cfg`.

Public methods:

- `SleepInhibitor::new(enabled: bool) -> Self`
  - creates the wrapper and instantiates the platform backend;
  - does not acquire any OS resource yet.
- `set_turn_running(&mut self, turn_running: bool)`
  - records the requested state;
  - calls `acquire()` when `enabled && turn_running`;
  - calls `release()` otherwise.
- `is_turn_running(&self) -> bool`
  - returns the latest logical state requested by the caller, not the verified OS inhibition state.

Important semantic detail:

- `is_turn_running()` reports caller intent, even if inhibition failed internally.
- repeated `set_turn_running(true)` calls are allowed; idempotence is delegated to the backend implementation.

## Module Layout

- `src/lib.rs`
  - public wrapper;
  - conditional backend selection;
  - simple unit tests.
- `src/linux_inhibitor.rs`
  - Linux backend that spawns a helper process from available desktop/system tooling.
- `src/macos.rs`
  - macOS backend using IOKit power assertions directly.
- `src/iokit_bindings.rs`
  - bindgen-generated FFI declarations used only by the macOS backend.
- `src/windows_inhibitor.rs`
  - Windows backend using native power request APIs.
- `src/dummy.rs`
  - no-op backend for unsupported platforms.

## Control Flow

### High-level flow

1. Caller constructs `SleepInhibitor::new(enabled)`.
2. Caller invokes `set_turn_running(true)` when an agent turn starts.
3. The wrapper stores `turn_running = true`.
4. If disabled, it forces `release()` and returns.
5. If enabled, it calls the selected backend’s `acquire()`.
6. When the turn ends, caller invokes `set_turn_running(false)`.
7. The wrapper stores `turn_running = false` and calls `release()`.
8. Backend-specific RAII cleanup runs again on drop if anything remains active.

### Integration flow in the TUI

The TUI uses the crate as a lifecycle companion to turn execution:

- on task start, it enables inhibition by calling `set_turn_running(true)`;
- on task completion/failure/finalization, it calls `set_turn_running(false)`;
- when restoring saved thread input state, it synchronizes inhibitor state with `agent_turn_running`;
- when the `PreventIdleSleep` feature is toggled, it recreates the inhibitor with the new `enabled` flag and immediately replays the current running state.

This integration keeps the crate stateless from the caller’s perspective except for a single boolean.

## Platform Implementations

### Linux backend

File: `src/linux_inhibitor.rs`

Approach:

- tries to keep the machine awake by spawning an external blocker process;
- prefers the last successful backend on subsequent acquisitions;
- falls back between two tools:
  - `systemd-inhibit`
  - `gnome-session-inhibit`

Key types:

- `LinuxSleepInhibitor`
  - `state: InhibitState`
  - `preferred_backend: Option<LinuxBackend>`
  - `missing_backend_logged: bool`
- `InhibitState`
  - `Inactive`
  - `Active { backend, child }`
- `LinuxBackend`
  - enum of supported helper commands.

Acquire logic:

- if already active, `try_wait()` probes whether the child is still alive;
- if still alive, acquisition is a no-op;
- if the child exited or status probing failed, the backend logs a warning and restarts selection;
- it tries preferred backend first, then fallback;
- after spawn, it probes the child immediately with `try_wait()` to reject helpers that exit right away;
- once a backend stays alive, it becomes the new preferred backend.

Release logic:

- takes ownership of the active child with `std::mem::take(&mut self.state)`;
- kills the child process;
- waits to reap it;
- suppresses some benign “already exited” style errors via `child_exited()`.

Process lifetime safety:

- uses `CommandExt::pre_exec` and `libc::prctl(PR_SET_PDEATHSIG, SIGTERM)` so the helper receives `SIGTERM` if the original parent dies;
- captures `parent_pid` before spawn and compares `getppid()` in the child to reduce the fork/exec race around parent death.

Trade-offs:

- avoids adding a DBus dependency or implementing a Linux-specific protocol;
- depends on external programs being installed;
- behavior varies by environment, init system, and desktop session.

### macOS backend

File: `src/macos.rs`

Approach:

- uses IOKit `IOPMAssertionCreateWithName` directly instead of shelling out to `caffeinate`.

Key types:

- backend `SleepInhibitor { assertion: Option<MacSleepAssertion> }`
- `MacSleepAssertion { id: IOPMAssertionID }`

Acquire logic:

- no-op if an assertion is already held;
- otherwise creates a `MacSleepAssertion` with reason string `"Codex is running an active turn"`;
- on failure, logs a warning and leaves the backend inactive.

Release logic:

- sets `assertion = None`, which drops `MacSleepAssertion`.

RAII cleanup:

- `Drop for MacSleepAssertion` calls `IOPMAssertionRelease(self.id)`;
- failure to release is logged but not propagated.

FFI notes:

- `core-foundation` supplies `CFString`;
- generated bindgen declarations in `src/iokit_bindings.rs` define IOKit symbols;
- code casts `CFString` refs into bindgen’s `CFStringRef` alias because the opaque `__CFString` definitions originate from different declarations.

Trade-offs:

- native API avoids subprocess management and is more direct than `caffeinate`;
- relies on manually maintained/generated FFI bindings;
- correctness depends on the safety assumptions around opaque CF types and assertion lifecycle.

### Windows backend

File: `src/windows_inhibitor.rs`

Approach:

- uses `PowerCreateRequest` + `PowerSetRequest(PowerRequestSystemRequired)`.

Key types:

- `WindowsSleepInhibitor { request: Option<PowerRequest> }`
- `PowerRequest { handle, request_type }`

Acquire logic:

- no-op if a request is already active;
- builds a UTF-16 reason string;
- creates a `REASON_CONTEXT`;
- calls `PowerCreateRequest`;
- calls `PowerSetRequest` with `PowerRequestSystemRequired`;
- on any failure, converts OS error details into a string and logs a warning.

Release logic:

- sets `request = None`, triggering `Drop` on `PowerRequest`.

RAII cleanup:

- `Drop for PowerRequest` calls `PowerClearRequest`;
- then closes the underlying handle with `CloseHandle`.

Behavioral choice:

- intentionally uses `PowerRequestSystemRequired` rather than a display-related request, matching the macOS behavior of preventing idle system sleep without forcing the display to remain on.

### Dummy backend

File: `src/dummy.rs`

Approach:

- exposes the same internal API with empty `acquire()` and `release()` implementations.

Purpose:

- keeps the crate buildable on unsupported targets;
- preserves a consistent public API without per-platform feature gates.

## Dependencies

### Direct dependencies

- `tracing`
  - used for warning logs across all real backends.

### Target-specific dependencies

- macOS: `core-foundation`
  - constructs `CFString` values for IOKit.
- Linux: `libc`
  - used for `getpid`, `getppid`, `prctl`, `raise`, constants, and `pre_exec` child setup.
- Windows: `windows-sys`
  - provides raw Win32 power-management and handle APIs.

### External runtime dependencies

Linux also depends on platform tooling at runtime:

- `systemd-inhibit`, or
- `gnome-session-inhibit`

This is not expressed in Cargo metadata, so absence is handled dynamically with warnings and graceful degradation.

## Testing

### Unit tests in this crate

`src/lib.rs` contains four platform-agnostic tests:

- toggles true then false without panic;
- disabled mode does not panic;
- repeated `true` calls are tolerated;
- multiple toggles are tolerated.

`src/linux_inhibitor.rs` contains one small invariant test:

- `BLOCKER_SLEEP_SECONDS == i32::MAX`

### Integration coverage outside the crate

The TUI has a behavioral integration test ensuring restored thread state synchronizes the inhibitor’s logical state with `agent_turn_running`.

### Observed test execution

`cargo test -p codex-utils-sleep-inhibitor` passes successfully in the workspace.

### Coverage assessment

Current tests validate API stability and non-panicking behavior, but they do not deeply verify platform effects.

Notably missing:

- Linux tests around backend selection, restart, and release behavior;
- macOS tests validating assertion create/release paths;
- Windows tests validating request create/clear/close paths;
- tests asserting warning behavior or error handling;
- trait-based mocking of OS interactions.

## Design Characteristics

### Strengths

- minimal public API makes the crate easy to integrate and hard to misuse;
- conditional compilation keeps platform details isolated;
- RAII ensures best-effort cleanup of OS resources;
- failures degrade gracefully instead of interrupting turn processing;
- Linux backend contains thoughtful process-lifetime handling via parent-death signaling;
- implementation choice aligns behavior across macOS and Windows around “system required” rather than “display required”.

### Notable design decisions

- **Soft-failure model**
  - backend failures are logged and ignored rather than surfaced as `Result`.
- **Intent state vs actual OS state**
  - `turn_running` reflects caller intent, not confirmed inhibition success.
- **Backend-local idempotence**
  - wrapper forwards repeated transitions and trusts each backend to be safe.
- **Runtime fallback on Linux**
  - backend availability is determined dynamically rather than at build time.
- **Native APIs on macOS/Windows**
  - avoids subprocesses where a reliable OS API exists.

### Limitations

- callers cannot inspect whether inhibition is actually active;
- Linux behavior depends on external executables and environment conventions;
- there is no abstraction boundary for mocking system calls in tests;
- the public API is tied specifically to “turn running” rather than a more general inhibition token or scope guard model;
- warning-only failure handling may hide persistent environment issues from higher layers.

## Safety and Resource Management

### Unsafe code

Unsafe operations are limited and purposeful:

- macOS FFI calls to `IOPMAssertionCreateWithName` and `IOPMAssertionRelease`;
- Linux child `pre_exec` setup using raw libc functions;
- Windows raw Win32 calls for power request management.

The code generally documents each unsafe block with the expected invariants:

- valid pointers/out-pointers on macOS;
- child-only libc setup during `pre_exec` on Linux;
- handle ownership and enum validity on Windows.

### Resource cleanup model

- wrapper type owns the backend directly;
- backend holds an `Option` or enum state representing an active OS resource;
- setting that state to `None` or replacing it triggers `Drop`;
- Linux explicitly kills and waits for helper children;
- macOS releases the IOKit assertion;
- Windows clears the power request and closes the handle.

This is a good fit for Rust’s ownership model because activation state maps naturally to owned resource presence.

## Call-Site Design Fit

This crate fits well with the TUI’s turn lifecycle:

- `on_task_started()` activates inhibition;
- normal completion and failure paths deactivate it;
- restored sessions resynchronize it;
- feature toggles rebuild it with new policy and replay current state.

The integration is intentionally forgiving: even if the backend cannot inhibit sleep, turn execution proceeds normally.

## Open Questions

1. Should the crate expose an “actually active” status separate from `is_turn_running()` so callers can distinguish logical intent from successful OS inhibition?
2. Should Linux support additional backends such as DBus/logind directly, rather than depending on `systemd-inhibit` or `gnome-session-inhibit` binaries?
3. Should repeated `set_turn_running(true)` or `false` calls be short-circuited in the wrapper to avoid extra backend probes and log noise?
4. Should persistent backend failure be rate-limited or surfaced to the UI, rather than only logged with `tracing::warn!`?
5. Should the crate introduce an internal trait boundary for system interactions so Linux/macOS/Windows logic can be unit tested without the real OS APIs or external commands?
6. Should there be explicit tests for drop-time cleanup behavior, especially on Linux where child process lifecycle is central to correctness?
7. Should the public API eventually evolve from a boolean “turn running” setter into a scoped guard model, if future callers need nested or multi-owner inhibition?

## Summary

This crate is a focused, well-contained utility that wraps OS-specific idle-sleep inhibition behind a tiny Rust API. Its strongest qualities are simplicity, graceful degradation, and careful cleanup. The most significant risks are limited observability, platform-specific runtime variance on Linux, and shallow automated testing around real backend behavior.
