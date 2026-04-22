# codex-test-binary-support Code Analysis

## Scope

This document analyzes the crate at `test-binary-support/`, focusing on:

- `Cargo.toml`
- `lib.rs`
- `BUILD.bazel`
- key consumer test modules in:
  - `core/tests/suite/mod.rs`
  - `exec-server/tests/common/mod.rs`

The crate is intentionally small. It exists to make integration test binaries behave like the real multi-entry Codex executable, while keeping the required alias directory and temporary `CODEX_HOME` alive for the duration of the test process.

## High-Level Responsibilities

The crate owns four concrete responsibilities:

1. Inspect the current test binary invocation early in process startup.
2. Decide, through a caller-provided classifier, whether the process should:
   - immediately dispatch into an arg0-based helper entry point,
   - skip setup entirely, or
   - install helper aliases for later subprocesses.
3. Create an isolated temporary `CODEX_HOME` so arg0 alias installation does not touch a developer's real environment.
4. Hold the alias-installation guard and temporary directory for the entire test run so spawned subprocesses can reliably reuse helper entry points.

In practice, this crate is a test-only bootstrap wrapper around `codex-arg0`.

## Crate Layout

The layout is minimal:

- `Cargo.toml`
  - declares library crate `codex-test-binary-support`
  - depends only on `codex-arg0` and `tempfile`
- `lib.rs`
  - contains all production logic
- `BUILD.bazel`
  - mirrors the library target for Bazel builds

There is no `src/` directory and there are no submodules. That matches the crate's narrow scope.

## Public API Surface

The public API contains one guard type, one mode enum, and one setup function.

### `TestBinaryDispatchGuard`

This guard keeps test bootstrap state alive:

- `_codex_home: TempDir`
  - preserves the temporary `CODEX_HOME` directory for the process lifetime
- `arg0: Arg0PathEntryGuard`
  - preserves the arg0-installed alias directory and exposes resolved helper paths
- `_previous_codex_home: Option<OsString>`
  - stores the original environment value for diagnostic/lifetime clarity, even though restoration happens immediately after alias installation

The only public method is:

- `paths(&self) -> &Arg0DispatchPaths`
  - forwards to `Arg0PathEntryGuard::paths()`
  - lets tests discover the concrete helper alias paths created by `codex-arg0`

This type is not about active behavior. Its main purpose is ownership: if the guard is dropped, the temp directories can be removed and helper subprocesses may stop working.

### `TestBinaryDispatchMode`

This enum lets each consuming test suite define its startup policy:

- `DispatchArg0Only`
  - run `codex_arg0::arg0_dispatch()` only for the current invocation
  - used when the test binary was reinvoked specifically as a helper or via a hidden helper argument
- `Skip`
  - do nothing and continue normal test execution
- `InstallAliases`
  - create temporary test state and install helper aliases for future subprocess use

This is the core extension point of the crate.

### `configure_test_binary_dispatch(...) -> Option<TestBinaryDispatchGuard>`

Signature:

```rust
pub fn configure_test_binary_dispatch<F>(
    codex_home_prefix: &str,
    classify: F,
) -> Option<TestBinaryDispatchGuard>
where
    F: FnOnce(&str, Option<&str>) -> TestBinaryDispatchMode
```

Parameters:

- `codex_home_prefix`
  - prefix used when creating a test-local temporary `CODEX_HOME`
- `classify`
  - callback that receives:
    - the basename of `argv[0]`
    - the first real command-line argument, if present and valid UTF-8
  - returns the desired `TestBinaryDispatchMode`

Return contract:

- `Some(TestBinaryDispatchGuard)`
  - aliases were installed successfully and must remain alive
- `None`
  - either setup was intentionally skipped, or control was handed to `arg0_dispatch()` without needing a persistent guard

## Core Control Flow

`configure_test_binary_dispatch()` has a simple three-branch flow.

### 1. Inspect process invocation

The function reads:

- `argv[0]` from `std::env::args_os()`
- the basename via `Path::file_name()`
- the first real argument (`argv[1]`) if present

It then delegates the decision to the caller-provided `classify` closure. This keeps crate logic generic while allowing each test suite to recognize its own helper entry points.

### 2. `DispatchArg0Only`

If classification returns `DispatchArg0Only`, the crate:

1. Calls `codex_arg0::arg0_dispatch()`.
2. Ignores the returned guard.
3. Returns `None`.

This mode exists for invocations that are already acting as helpers. The important side effect is inside `arg0_dispatch()` itself:

- it may transfer control to helper code and terminate the process,
- or it may perform whatever minimal bootstrap is needed for that helper path.

The test support crate deliberately does not retain extra state here because the helper-style invocation is expected to be short-lived and purpose-specific.

### 3. `Skip`

If classification returns `Skip`, the crate returns `None` immediately. No temp directory is created and no environment is changed.

### 4. `InstallAliases`

If classification returns `InstallAliases`, the crate performs the full test bootstrap:

1. Creates a temporary directory using `tempfile::Builder` with the requested prefix.
2. Saves the current `CODEX_HOME`.
3. Temporarily sets `CODEX_HOME` to the new temp directory.
4. Calls `codex_arg0::arg0_dispatch()`.
5. Expects that call to install helper aliases rooted in the temporary `CODEX_HOME`.
6. Restores the previous `CODEX_HOME` value immediately after alias installation.
7. Returns a `TestBinaryDispatchGuard` that keeps both temp resources alive.

The temporary `CODEX_HOME` is only needed during alias creation. The returned guard keeps the directory alive afterward without leaving the process environment permanently redirected.

## Relationship to `codex-arg0`

This crate is a thin adapter over `codex-arg0`, not an independent dispatch implementation.

`codex-arg0` already knows how to:

- dispatch helper entry points based on `argv[0]` or hidden `argv[1]` values
- create a temporary alias directory
- prepend that alias directory to `PATH`
- expose resolved helper paths through `Arg0DispatchPaths`

What `codex-test-binary-support` adds is test isolation:

- it avoids using the developer's real `CODEX_HOME`
- it lets each test suite choose when dispatch should happen
- it provides a guard tailored to integration test startup patterns

This is why the dependency surface is so small: almost all heavy lifting is delegated downward.

## Dependency Analysis

`Cargo.toml` lists only two dependencies.

### Workspace dependency

- `codex-arg0`
  - provides:
    - `arg0_dispatch()`
    - `Arg0DispatchPaths`
    - `Arg0PathEntryGuard`
  - this is the real helper bootstrap engine

### External dependency

- `tempfile`
  - creates the temporary `CODEX_HOME`
  - ensures automatic cleanup when the guard is dropped

The crate intentionally has no direct dependency on helper crates like `apply-patch` or `exec-server`. It stays decoupled by letting `codex-arg0` own the actual dispatch matrix.

## How Consumers Use It

There are two important workspace consumers.

### `core/tests/suite/mod.rs`

The core integration test suite installs aliases at process startup with `#[ctor]`:

- if `argv[1]` matches `CODEX_CORE_APPLY_PATCH_ARG1`, it returns `DispatchArg0Only`
- if `argv[0]` equals the Linux sandbox alias, it returns `DispatchArg0Only`
- otherwise it returns `InstallAliases`

That lets one compiled test binary impersonate:

- the internal apply-patch helper
- the Linux sandbox helper
- the normal test harness process that later spawns subprocesses

The static guard keeps the aliases alive for the full suite lifetime.

### `exec-server/tests/common/mod.rs`

The exec-server test support uses the same startup pattern, but adds one more behavior after setup:

- it dispatches directly when `argv[1]` equals `CODEX_FS_HELPER_ARG1`
- it dispatches directly when `argv[0]` is the Linux sandbox alias
- otherwise it installs aliases

After obtaining the optional guard, it may also run an embedded `exec-server` main path directly from the test binary when invoked with `exec-server --listen <url>`.

This demonstrates the main architectural value of the crate: one integration-test binary can expose multiple helper identities without requiring separate compiled helper binaries.

## Environment and Safety Notes

The most delicate part of the implementation is environment mutation.

### Temporary `CODEX_HOME` override

The crate temporarily modifies `CODEX_HOME` before calling `arg0_dispatch()`. This is needed because `codex-arg0` uses `CODEX_HOME` to determine where to create its temp alias directory.

### Unsafe environment APIs

The implementation uses:

```rust
unsafe {
    std::env::set_var("CODEX_HOME", codex_home.path());
}
```

and later:

```rust
unsafe {
    std::env::remove_var("CODEX_HOME");
}
```

The safety comment explains the intended invariant: this code runs from a test constructor before test threads begin. That is important because environment mutation is process-global and only sound before concurrent access begins.

### Immediate restoration

Restoring the previous `CODEX_HOME` after alias installation is a good design choice:

- later test code sees the original environment
- alias files still remain usable because the temp directory itself is retained by the guard
- the scope of the global mutation stays as short as possible

## Testing Status

This crate has no inline unit tests and no dedicated `tests/` directory.

I ran:

- `cargo test -p codex-test-binary-support`

Current result:

- the crate compiles successfully
- there are `0` unit tests
- there are `0` doc tests

That means confidence comes primarily from integration usage in downstream crates rather than direct local tests.

### Effective indirect coverage

Despite having no local tests, the crate is exercised indirectly by:

- `core` integration tests
- `exec-server` integration tests

Those consumers depend on it during process startup, helper reinvocation, and subprocess path discovery.

### Coverage gaps

There is still no crate-local verification for:

- restoration of `CODEX_HOME` after setup
- failure behavior when temp dir creation fails
- behavior when `arg0_dispatch()` unexpectedly returns `None` during `InstallAliases`
- UTF-8 edge cases in `argv[0]` / `argv[1]` classification
- constructor ordering assumptions across different test harness setups

## Design Assessment

The crate is well-targeted and intentionally thin.

### Strengths

- Narrow abstraction
  - it adds only test-specific behavior on top of `codex-arg0`
- Good isolation
  - helper aliases are created under a temporary `CODEX_HOME`, not the real one
- Flexible classification
  - each consuming test suite controls dispatch policy without duplicating setup code
- Lifetime-safe resource model
  - the returned guard makes it hard to accidentally drop needed temp state
- Minimal dependency footprint
  - only one workspace dependency and one external dependency

### Trade-offs

- No direct tests
  - correctness is mostly validated through higher-level integration suites
- `Option`-based API
  - `None` can mean either intentional skip or non-persistent dispatch path, which is concise but not fully self-describing
- Panic-based failures
  - temp dir creation failure and missing alias installation both panic during startup
- Environment coupling
  - the design depends on early single-threaded execution via `#[ctor]`

## Open Questions

These are the main follow-up questions that remain after reviewing the crate.

1. Should `configure_test_binary_dispatch()` return a richer result type than `Option<TestBinaryDispatchGuard>`?
   - A dedicated enum could distinguish `Skipped`, `Dispatched`, and `Installed`.

2. Should this crate have at least a few focused unit or subprocess tests?
   - The current design is small enough that direct tests for `CODEX_HOME` restoration and mode handling would be cheap and valuable.

3. Should `InstallAliases` handle `arg0_dispatch()` returning `None` without panicking?
   - Today that is treated as an invariant violation, but the failure message gives little context.

4. Is the stored `_previous_codex_home` field still necessary after restoration?
   - It currently extends lifetime ownership of the old value, but it is not read after construction.

5. Should non-UTF-8 `argv[0]` or `argv[1]` be observable to callers?
   - The current API silently maps those cases to empty string / `None`, which is pragmatic but lossy.

6. Are there any test suites that need a fourth mode, such as "install aliases and also continue helper dispatch"?
   - Current consumers do not, but future test harnesses might.

## Bottom Line

`codex-test-binary-support` is a small but strategically useful test harness crate. It does not implement helper dispatch itself; instead, it safely adapts `codex-arg0` for integration-test binaries by installing aliases inside a temporary `CODEX_HOME`, restoring the ambient environment, and holding the required resources alive through a guard. The design is clean and focused, with the main weaknesses being lack of direct tests and a slightly compressed result model.
