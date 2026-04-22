# CODE_ANALYSIS: codex-process-hardening

## Overview

`codex-process-hardening` is a very small, single-module crate that performs early process hardening for binaries that want to reduce runtime attack surface and avoid noisy inherited environment behavior.

The crate is designed to run **before `main()`**, typically via `#[ctor::ctor]`, so it can mutate process-global state before worker threads start and before the rest of the application initializes. In this repository, a concrete example appears in `responses-api-proxy/src/main.rs`, where `codex_process_hardening::pre_main_hardening()` is invoked from a constructor.

At a high level, the crate does three things:

1. Prevents debugger attachment where supported.
2. Disables core dumps.
3. Removes environment variables that can affect dynamic loading or produce undesirable allocator logging.

## Crate Structure

- `Cargo.toml`
  - Declares a library crate named `codex_process_hardening`.
  - Depends only on `libc`.
  - Uses `pretty_assertions` in tests.
- `src/lib.rs`
  - Contains the full implementation and unit tests.
- `README.md`
  - Provides a short consumer-facing summary.
- `BUILD.bazel`
  - Exposes the crate to the Bazel build.

There are no submodules. All functionality lives in `src/lib.rs`.

## Responsibilities

The crate has a narrow, well-defined responsibility set:

- Provide a single public entry point for process hardening.
- Dispatch to platform-specific hardening behavior at compile time.
- Fail closed when critical hardening syscalls fail by printing an error and terminating the process.
- Use byte-oriented environment key inspection on Unix so non-UTF-8 environment keys are handled correctly.

It does **not** try to be a general sandboxing layer. It only applies a few early safety and hygiene controls.

## Public API

### `pre_main_hardening()`

This is the only public function.

Behavior:

- On Linux/Android:
  - calls `pre_main_hardening_linux()`
- On macOS:
  - calls `pre_main_hardening_macos()`
- On FreeBSD/OpenBSD:
  - calls `pre_main_hardening_bsd()`
- On Windows:
  - calls `pre_main_hardening_windows()`

The dispatch is controlled entirely by `#[cfg(...)]`, so unsupported branches are removed at compile time.

## Internal APIs and Behavior

### Linux / Android path

`pre_main_hardening_linux()` performs:

1. `prctl(PR_SET_DUMPABLE, 0, ...)`
   - Marks the process non-dumpable.
   - This both reduces attach/debug behavior and helps prevent core dumps.
   - On failure, the process prints an error and exits with code `5`.
2. `set_core_file_size_limit_to_zero()`
   - Calls `setrlimit(RLIMIT_CORE, {0, 0})`.
   - On failure, exits with code `7`.
3. `remove_env_vars_with_prefix(b"LD_")`
   - Clears dynamic-loader-related environment variables such as `LD_PRELOAD`.

### macOS path

`pre_main_hardening_macos()` performs:

1. `ptrace(PT_DENY_ATTACH, ...)`
   - Prevents debugger attachment.
   - On failure, exits with code `6`.
2. `set_core_file_size_limit_to_zero()`
   - Disables core dumps.
   - On failure, exits with code `7`.
3. `remove_env_vars_with_prefix(b"DYLD_")`
   - Clears dynamic-loader override variables.
4. `remove_env_vars_with_prefix(b"MallocStackLogging")`
5. `remove_env_vars_with_prefix(b"MallocLogFile")`
   - Removes allocator logging controls to avoid noisy diagnostics propagating into Codex or child processes.

### FreeBSD / OpenBSD path

`pre_main_hardening_bsd()` performs:

1. `set_core_file_size_limit_to_zero()`
2. `remove_env_vars_with_prefix(b"LD_")`

Unlike Linux and macOS, there is no ptrace-related hardening here.

### Windows path

`pre_main_hardening_windows()` currently does nothing and is explicitly a TODO.

## Execution Flow

The intended runtime flow is:

1. A consumer binary defines a constructor function with `#[ctor::ctor]`.
2. That constructor calls `codex_process_hardening::pre_main_hardening()`.
3. `pre_main_hardening()` dispatches to the OS-specific implementation.
4. The selected implementation:
   - performs syscall-based hardening,
   - clears selected environment variables,
   - exits immediately if a required hardening step fails.
5. If all steps succeed, normal program startup continues into `main()`.

This flow is important because some operations are process-global:

- `std::env::remove_var` is `unsafe` in Rust 2024 because environment mutation is fundamentally racy in multithreaded contexts.
- The crate’s design relies on the "pre-main, before threads" usage model to make that safe.

## Unsafe Code and Safety Model

The crate uses `unsafe` in a few targeted places:

- `libc::prctl(...)`
- `libc::ptrace(...)`
- `libc::setrlimit(...)`
- `std::env::remove_var(...)`

### Safety assumptions

1. The OS syscalls are called with correct constants and argument shapes.
2. Environment mutation happens before any concurrent access from other threads.
3. The caller honors the crate’s contract and invokes it as early as possible, ideally before `main()`.

### Why the unsafe is reasonable here

- The libc calls are thin FFI wrappers around well-known process-control syscalls.
- The environment removal logic collects matching keys first, then removes them in a second pass, avoiding iterator invalidation concerns.
- The helper works on raw Unix bytes via `OsStrExt::as_bytes()`, which correctly preserves non-UTF-8 keys.

## Dependency Analysis

### Direct dependencies

- `libc`
  - Required for `prctl`, `ptrace`, `setrlimit`, `rlimit`, and platform constants.
  - This is the only runtime dependency, which keeps the crate lightweight and suitable for very early startup.

### Dev dependencies

- `pretty_assertions`
  - Used only in unit tests for clearer equality diff output.

### Workspace inheritance

From the workspace:

- version: `0.0.0`
- edition: `2024`
- license: `Apache-2.0`
- lints: inherited from workspace Clippy/rust lint configuration

The 2024 edition matters because it explains why `remove_var` appears in an `unsafe` block.

## Testing

The crate includes two Unix-only unit tests, both focused on the pure helper `env_keys_with_prefix(...)`.

### What is tested

1. Non-UTF-8 key handling
   - Confirms that prefix matching works on raw bytes.
   - Verifies that a non-UTF-8 key with `LD_` prefix is preserved and returned.
2. Prefix filtering
   - Confirms that only matching keys are returned.
   - Demonstrates that unrelated variables such as `PATH` and `DYLD_*` are ignored when filtering for `LD_`.

### What is not tested

- Actual syscall behavior for:
  - `prctl`
  - `ptrace`
  - `setrlimit`
- Actual process exit paths and exit codes
- macOS-specific environment cleanup behavior
- BSD-specific behavior
- Windows behavior
- End-to-end pre-main integration via `#[ctor::ctor]`

### Test quality assessment

The current tests are appropriate for the only pure, side-effect-free helper in the crate, but coverage is intentionally narrow. Most of the crate’s core behavior is OS- and process-global, which makes it harder to unit test directly without subprocess-based integration tests.

## Design Assessment

### Strengths

- Minimal API surface
  - One public function keeps adoption simple.
- Very small dependency footprint
  - Important for pre-main code where initialization risk should stay low.
- Fail-closed behavior
  - Hardening steps do not silently fail.
- Correct Unix environment handling
  - Uses raw bytes instead of assuming UTF-8.
- Compile-time platform dispatch
  - Avoids runtime branching on OS.

### Trade-offs

- Strong coupling to process startup timing
  - Safety depends on being invoked before threads and before other code reads environment state.
- Abrupt failure mode
  - `std::process::exit(...)` is simple and clear, but it prevents callers from handling errors.
- Limited extensibility
  - The current API does not allow opting in or out of specific hardening steps.
- Platform asymmetry
  - Linux/macOS are more fully handled than BSD/Windows.

## Key Implementation Details

### Environment filtering strategy

`env_keys_with_prefix<I>(vars, prefix)` accepts any iterator of `(OsString, OsString)` pairs and returns the keys that start with the given byte prefix.

Notable points:

- It is generic over the iterator source, which keeps it testable.
- It examines only keys, ignoring values.
- It uses `OsStrExt::as_bytes()` on Unix, so matching is byte-exact and not lossy.

### Two-phase removal

`remove_env_vars_with_prefix(prefix)` first gathers keys, then removes them.

That is a good design because:

- it separates discovery from mutation,
- it keeps the filtering helper pure,
- it avoids mutating the environment while iterating over it.

### Exit codes

The crate defines fixed exit codes for some platform failures:

- `5`: Linux/Android `prctl(PR_SET_DUMPABLE, 0)` failure
- `6`: macOS `ptrace(PT_DENY_ATTACH)` failure
- `7`: `setrlimit(RLIMIT_CORE, ...)` failure on supported Unix targets

This makes failure reasons at least partially machine-distinguishable.

## Integration Notes

The visible in-repo integration point is `responses-api-proxy`, which uses:

```rust
#[ctor::ctor]
fn pre_main() {
    codex_process_hardening::pre_main_hardening();
}
```

That demonstrates the intended operational contract more clearly than the library code alone.

## Open Questions

1. Should Windows implement equivalent protections?
   - The stub suggests this is planned but unspecified.
2. Should BSD platforms also deny debugger attachment where possible?
   - Today they only disable core dumps and clear `LD_*`.
3. Should the crate expose a configurable API?
   - Some consumers may want only env cleanup, or only anti-debugging.
4. Should there be subprocess integration tests?
   - They would allow validation of exit codes, environment cleanup, and pre-main execution behavior.
5. Why is `target_os = "netbsd"` included in the `SET_RLIMIT_CORE_FAILED_EXIT_CODE` cfg but not in the BSD hardening entry point?
   - This may be intentional future-proofing, or it may be an inconsistency.
6. Should failures log through a structured logger instead of `eprintln!`?
   - Pre-main timing may make logging initialization unavailable, but the trade-off is worth documenting.
7. Should environment variable removal be broadened or narrowed?
   - Prefix-based deletion is simple, but it may be worth explicitly documenting the exact threat model.

## Suggested Future Improvements

- Implement Windows hardening or explicitly document why it is omitted.
- Add subprocess-based integration tests for Linux/macOS where CI permits.
- Document the safety contract more explicitly in the public API docs:
  - must run before `main()`,
  - must run before threads,
  - intended for constructor usage.
- Consider returning richer error information in an alternate API for callers that do not want forced process exit.
- Reconcile or document the current `netbsd` cfg mismatch.

## Bottom Line

`codex-process-hardening` is a focused early-startup hardening crate. Its design prioritizes:

- very early execution,
- minimal dependencies,
- small trusted code surface,
- explicit failure on hardening errors.

The implementation is straightforward and appropriate for its scope. The biggest gaps are platform completeness, test depth for syscall paths, and explicit documentation of the safety contract around environment mutation.
