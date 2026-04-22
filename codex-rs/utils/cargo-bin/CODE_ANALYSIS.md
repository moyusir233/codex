# codex-utils-cargo-bin crate analysis

## Overview

`codex-utils-cargo-bin` is a small test-support crate that hides the differences between running the Codex workspace under Cargo and running it under Bazel. Its job is not to build binaries or resources; it resolves paths to artifacts that were already built and made available to the current test process.

The crate has one source file, [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L1-L226), but it provides three distinct capabilities:

1. Find a compiled binary for the current test run with [cargo_bin](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L33-L69).
2. Find test resources in a Cargo-friendly or Bazel-friendly way with [find_resource!](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L109-L133), [resolve_bazel_runfile](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L135-L161), and [resolve_cargo_runfile](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L163-L166).
3. Recover the repository root by locating a marker file injected into build/test data with [repo_root](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L168-L202).

The manifest is intentionally tiny: [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/Cargo.toml#L1-L12) declares only `assert_cmd`, `runfiles`, and `thiserror`. The Bazel contract is defined in [BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/BUILD.bazel#L1-L17), and the runfiles strategy is described in [README.md](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/README.md#L1-L20).

## Package shape

- Manifest: [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/Cargo.toml#L1-L12)
- Bazel integration: [BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/BUILD.bazel#L1-L17)
- Single library module: [src/lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L1-L226)
- Runfiles/repo marker data: [repo_root.marker](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/repo_root.marker)
- Narrative note on Bazel strategy: [README.md](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/README.md#L1-L20)

There are no internal submodules and no crate-local tests under `tests/` or `#[cfg(test)]`. The crate is designed as a shared utility that many other integration-test crates exercise indirectly.

## Concrete responsibilities

### 1. Normalize binary lookup across Cargo and Bazel

The main entrypoint is [cargo_bin](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L39-L69).

- It derives candidate environment variable names with [cargo_bin_env_keys](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L71-L82), including both `CARGO_BIN_EXE_<name>` and a dash-to-underscore variant.
- It first trusts explicit environment variables, which lets both Cargo and Bazel communicate artifact locations without the caller caring which build system produced them.
- It resolves those values with [resolve_bin_from_env](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L88-L107):
  - under runfiles, it treats the env value as an rlocation path and resolves it through the `runfiles` crate;
  - outside runfiles, it only accepts an absolute existing path.
- If no environment variable is present, it falls back to `assert_cmd::Command::cargo_bin(name)`, then canonicalizes relative paths against the current working directory and verifies the target exists.

This makes callers like [cli_stream.rs](file:///Users/bytedance/project/codex/codex-rs/core/tests/suite/cli_stream.rs#L43-L45), [mcp_list.rs](file:///Users/bytedance/project/codex/codex-rs/cli/tests/mcp_list.rs#L15-L15), and [stdio_to_uds.rs](file:///Users/bytedance/project/codex/codex-rs/stdio-to-uds/tests/stdio_to_uds.rs#L75-L75) independent from the surrounding build runner.

### 2. Normalize resource lookup across Cargo and Bazel

The resource side is split between a macro and two functions.

- [find_resource!](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L119-L133) is the user-facing API for tests.
- It is a macro, not a function, because it needs compile-time `env!` and `option_env!` expansion at the caller site.
- Under Bazel-like runfiles it calls [resolve_bazel_runfile](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L135-L161), combining `_main`, the compile-time `BAZEL_PACKAGE`, and the requested relative resource path.
- Under Cargo it joins the resource against `CARGO_MANIFEST_DIR` through [resolve_cargo_runfile](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L163-L166).
- [normalize_runfile_path](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L204-L226) strips `.` and folds simple `..` segments so Bazel lookups are more tolerant of cross-crate relative paths.

Representative uses:

- [schema_root](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/export.rs#L2286-L2294) resolves vendored schema files.
- [model_availability_nux.rs](file:///Users/bytedance/project/codex/codex-rs/tui/tests/suite/model_availability_nux.rs#L71-L73) resolves a fixture in another crate via `../core/...`.
- [cli_stream.rs](file:///Users/bytedance/project/codex/codex-rs/core/tests/suite/cli_stream.rs#L19-L22) resolves SSE fixtures in `core/tests`.

### 3. Recover the repository root for tests that need workspace-relative paths

[repo_root](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L168-L202) supports tests that need to navigate the repository tree rather than one crate’s manifest directory.

- Under Bazel, [BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/BUILD.bazel#L3-L16) exports `repo_root.marker` as build/test data and injects its runfile path through `CODEX_REPO_ROOT_MARKER`.
- Under Cargo, `repo_root()` uses [resolve_cargo_runfile](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L163-L166) to find `repo_root.marker` inside this crate.
- It then walks four parents upward from the marker path to land at the workspace root.

Representative consumers:

- [apply-patch scenario tests](file:///Users/bytedance/project/codex/codex-rs/apply-patch/tests/suite/scenarios.rs#L10-L18) build a path into `apply-patch/tests/fixtures/scenarios`.
- [core cli_stream.rs](file:///Users/bytedance/project/codex/codex-rs/core/tests/suite/cli_stream.rs#L14-L17) uses it as the `-C` working directory for CLI integration tests.
- [model_availability_nux.rs](file:///Users/bytedance/project/codex/codex-rs/tui/tests/suite/model_availability_nux.rs#L20-L21) uses it to populate trusted project config.

### 4. Provide actionable, typed error reporting

[CargoBinError](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L11-L31) keeps binary-resolution failures structured.

- `CurrentExe` and `CurrentDir` capture platform/environment failures while constructing fallback paths.
- `ResolvedPathDoesNotExist` tells the caller exactly which source of truth produced a missing path.
- `NotFound` includes the binary name, all attempted environment-variable keys, and the fallback failure string from `assert_cmd`.

This is important for integration-test ergonomics because a missing binary is often a build graph or test harness issue, not a normal runtime failure.

### 5. Re-export Bazel runfiles primitives for consumers

[lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L6-L6) contains `pub use runfiles;`.

That exposes the external `runfiles` crate through this utility crate so consumers can share the same dependency and avoid importing Bazel-specific plumbing directly when they only want test helpers from this crate.

## Public API surface

The public API is small and intentionally test-oriented.

### `cargo_bin(name: &str) -> Result<PathBuf, CargoBinError>`

Defined in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L39-L69).

- Input: logical binary target name such as `"codex"` or `"apply_patch"`
- Output: absolute filesystem path to an executable built for the current test run
- Failure mode: typed `CargoBinError`

### `runfiles_available() -> bool`

Defined in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L84-L86).

- Input: none
- Output: whether the process appears to be running in the Bazel manifest-runfiles environment
- Role: branch predicate used by binary, resource, and repo-root resolution

### `resolve_bazel_runfile(bazel_package: Option<&str>, resource: &Path) -> io::Result<PathBuf>`

Defined in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L135-L161).

- Input: compile-time package name plus a resource-relative path
- Output: resolved runfile path if present
- Failure mode: `io::Error` with explicit context for missing package metadata or missing runfile

### `resolve_cargo_runfile(resource: &Path) -> io::Result<PathBuf>`

Defined in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L163-L166).

- Input: path relative to this crate’s manifest directory
- Output: joined path under `CARGO_MANIFEST_DIR`

### `repo_root() -> io::Result<PathBuf>`

Defined in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L168-L202).

- Input: none
- Output: repository root directory for the Codex workspace
- Hidden contract: `repo_root.marker` must be present and the crate must still live four directories below the repo root

### `find_resource!($resource)`

Defined in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L119-L133).

- Input: resource path expression
- Output: `io::Result<PathBuf>`
- Distinguishing feature: expands at the caller site so the caller’s compile-time Bazel package metadata is captured correctly

## Control flow

### Binary resolution flow

1. Caller invokes [cargo_bin](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L39-L69).
2. The crate computes possible env keys with [cargo_bin_env_keys](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L71-L82).
3. If any env key is present, [resolve_bin_from_env](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L88-L107) resolves it:
   - runfiles branch: create `Runfiles`, call `rlocation!`, verify existence;
   - non-runfiles branch: accept only absolute existing paths.
4. If no env key is present, the crate calls `assert_cmd::Command::cargo_bin`.
5. Any relative fallback path is anchored to `current_dir`.
6. Existence is validated before returning.

### Resource resolution flow

1. Caller invokes [find_resource!](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L119-L133).
2. The macro checks [runfiles_available](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L84-L86).
3. Bazel branch:
   - read compile-time `BAZEL_PACKAGE`;
   - prepend `_main/<package>/`;
   - normalize the runfile path with [normalize_runfile_path](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L204-L226);
   - resolve with `runfiles::rlocation!`.
4. Cargo branch:
   - join resource to `CARGO_MANIFEST_DIR`.

### Repo-root resolution flow

1. Caller invokes [repo_root](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L168-L202).
2. The function locates `repo_root.marker` via runfiles or `CARGO_MANIFEST_DIR`.
3. It walks four parent directories upward.
4. The resulting directory becomes the repository root returned to the caller.

## Dependency analysis

The dependency story is intentionally narrow.

- `assert_cmd` from the workspace, declared in [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/Cargo.toml#L10-L12) and versioned in the workspace root [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/Cargo.toml#L212-L212), provides the fallback `cargo_bin` lookup used mainly in Cargo-based tests.
- `runfiles`, declared in [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/Cargo.toml#L10-L12) and sourced from the `rules_rust` repository in the workspace root [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/Cargo.toml#L297-L297), is the Bazel-facing resolver for manifest-based runfiles.
- `thiserror`, declared in [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/Cargo.toml#L10-L12) and versioned in the workspace root [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/Cargo.toml#L338-L338), keeps the only custom error type lightweight and readable.

Notably absent:

- no `anyhow` or tracing dependency
- no filesystem watching or path canonicalization helper crate
- no direct dependency on Cargo internals or Bazel internals beyond `runfiles`

That minimalism fits the crate’s role as low-level test infrastructure.

## Testing and usage evidence

This crate has no dedicated local unit or integration tests in its own directory. Instead, confidence comes from broad downstream usage across the workspace.

### Binary lookup coverage through consumers

- [core/tests/suite/cli_stream.rs](file:///Users/bytedance/project/codex/codex-rs/core/tests/suite/cli_stream.rs#L43-L45) launches the main `codex` binary.
- [apply-patch/tests/suite/scenarios.rs](file:///Users/bytedance/project/codex/codex-rs/apply-patch/tests/suite/scenarios.rs#L42-L48) launches the `apply_patch` binary.
- [app-server/tests/suite/v2/initialize.rs](file:///Users/bytedance/project/codex/codex-rs/app-server/tests/suite/v2/initialize.rs#L214-L214) resolves an auxiliary notify-capture binary.

### Resource lookup coverage through consumers

- [core/tests/suite/cli_stream.rs](file:///Users/bytedance/project/codex/codex-rs/core/tests/suite/cli_stream.rs#L19-L22) resolves an SSE fixture.
- [app-server-protocol/src/export.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/export.rs#L2286-L2294) resolves schema files.
- [tui/tests/suite/model_availability_nux.rs](file:///Users/bytedance/project/codex/codex-rs/tui/tests/suite/model_availability_nux.rs#L71-L73) resolves a fixture from a sibling crate using a parent-directory path.

### Repo-root coverage through consumers

- [apply-patch/tests/suite/scenarios.rs](file:///Users/bytedance/project/codex/codex-rs/apply-patch/tests/suite/scenarios.rs#L10-L18) navigates from the repo root into scenario fixtures.
- [core/tests/common/lib.rs](file:///Users/bytedance/project/codex/codex-rs/core/tests/common/lib.rs#L50-L51) uses the repo root when preparing shared test environment.
- [tui/tests/suite/model_availability_nux.rs](file:///Users/bytedance/project/codex/codex-rs/tui/tests/suite/model_availability_nux.rs#L20-L21) uses it to construct trusted project configuration.

The coverage is therefore broad but indirect: failures tend to surface only when a higher-level integration test executes.

## Design choices and trade-offs

### Why a macro for resources?

`find_resource!` is a macro because compile-time `BAZEL_PACKAGE` must be captured from the caller’s crate, not from `codex-utils-cargo-bin` itself. A normal function would read `env!` from this helper crate and point at the wrong package.

### Why re-check file existence?

The crate does not trust that environment variables or `assert_cmd` outputs are sufficient. Both [cargo_bin](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L46-L68) and [resolve_bazel_runfile](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L150-L160) verify existence before returning. That prevents confusing late failures when callers try to execute a non-existent path.

### Why store a repo marker file?

`repo_root()` avoids assumptions about the caller’s current directory by resolving a marker that is shipped as test/build data. That is more reliable under Bazel, where the process working directory and sandbox layout do not necessarily resemble the source tree.

### Why keep this crate tiny?

Because it sits underneath many test suites, a small surface area reduces coupling. The crate mostly performs path translation, leaving process launch, fixture semantics, and test assertions to downstream crates.

## Notable limitations

- `repo_root()` depends on fixed path depth: four parent traversals from [repo_root.marker](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/repo_root.marker). Moving the crate inside the repository would silently change correctness until tests fail.
- `resolve_cargo_runfile()` only joins paths; it does not verify existence. Callers may discover missing resources later than they would on the Bazel branch.
- `cargo_bin()` only accepts absolute non-runfiles env values. If a future runner exposes relative `CARGO_BIN_EXE_*` paths directly, the env-path fast path will reject them.
- `normalize_runfile_path()` collapses only a subset of path semantics and does not canonicalize filesystem symlinks, which is probably intentional but worth knowing.
- There is no crate-local test suite for edge cases like dash-to-underscore env key fallback, missing `BAZEL_PACKAGE`, or malformed marker depth.

## Open questions

1. [README.md](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/README.md#L3-L16) describes runfiles detection in terms of `RUNFILES_MANIFEST_FILE`, but [runfiles_available](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L84-L86) checks `RUNFILES_MANIFEST_ONLY`. Is that mismatch intentional, or is the README stale?
2. Should [repo_root](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L168-L202) compute the repository root from the marker contents or a stronger invariant instead of assuming exactly four parent levels?
3. Should [resolve_cargo_runfile](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L163-L166) validate existence for parity with the Bazel branch?
4. Would a small local unit-test suite for [cargo_bin_env_keys](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L71-L82) and [normalize_runfile_path](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L204-L226) make failures easier to diagnose than relying purely on downstream integration tests?
5. Is the public re-export [runfiles](file:///Users/bytedance/project/codex/codex-rs/utils/cargo-bin/src/lib.rs#L6-L6) still needed by consumers, or could the crate hide that dependency completely?

## Bottom line

`codex-utils-cargo-bin` is a small but strategically important test-infrastructure crate. It centralizes the workspace’s build-system abstraction boundary for test artifacts: binaries, resources, and repository-relative paths. The implementation is intentionally simple, but many integration tests rely on its assumptions, especially the runfiles detection logic and the fixed-depth repo-root marker strategy.
