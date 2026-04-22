# codex-linux-sandbox: Code Analysis

## Scope

This document analyzes the crate at `/Users/bytedance/project/codex/codex-rs/linux-sandbox`.
It focuses on the manifest, core source modules, build-time behavior, important tests,
and the current design tradeoffs visible in the implementation.

## High-Level Purpose

`codex-linux-sandbox` is a Linux-only helper crate that provides both:

- a binary, `codex-linux-sandbox`, used as the sandbox entrypoint for running commands, and
- a library entrypoint, `codex_linux_sandbox::run_main()`, so other Codex binaries can dispatch
  into the same sandbox logic without duplicating behavior.

The crate implements a two-layer Linux sandbox model:

- filesystem isolation is primarily done with bubblewrap,
- in-process restrictions are applied with `PR_SET_NO_NEW_PRIVS` and seccomp,
- Landlock remains as an explicit legacy fallback for filesystem policy enforcement,
- managed proxy routing adds a special restricted network mode for proxy-only access.

In short: this crate converts Codex sandbox policies into an executable Linux sandbox pipeline.

## Crate Surface and Packaging

### Manifest

The crate declares both a binary and a library:

- package name: `codex-linux-sandbox`
- binary path: `src/main.rs`
- library path: `src/lib.rs`

The implementation is gated heavily on `target_os = "linux"`. Non-Linux builds compile a stub
`run_main()` that panics immediately.

### Build-Time Responsibilities

`build.rs` is a meaningful part of the crate, not just boilerplate:

- it advertises the custom `vendored_bwrap_available` cfg to rustc/clippy,
- it rebuilds when bubblewrap sources or selected environment variables change,
- on Linux, unless `CODEX_SKIP_VENDORED_BWRAP` is set, it compiles vendored bubblewrap C sources,
- it probes `libcap` via `pkg-config`, generates a tiny `config.h`, renames `main` to
  `bwrap_main`, and links the result into the Rust crate.

This means the crate can fall back to an embedded bubblewrap implementation when no usable
system `bwrap` is found.

## Source Layout

### `src/lib.rs`

This is the public library entrypoint. It conditionally declares the Linux modules and exposes
only one public function:

- `pub fn run_main() -> !`

Everything else in the crate is internal implementation detail.

### `src/main.rs`

The binary is intentionally minimal. It just forwards to `codex_linux_sandbox::run_main()`.

### `src/linux_run_main.rs`

This is the orchestration center of the crate. It:

- parses CLI arguments,
- resolves legacy vs split policy inputs,
- decides whether to stay in-process, use bubblewrap, or use legacy Landlock,
- prepares managed proxy routing when needed,
- builds the inner re-exec command,
- applies seccomp in the correct stage,
- finally `execvp`s the target command.

If one file defines the runtime behavior of the crate, this is it.

### `src/bwrap.rs`

This module translates `FileSystemSandboxPolicy` into concrete bubblewrap argv.
It is responsible for:

- choosing when bubblewrap can be skipped,
- building mount overlays for read-only, writable, and unreadable paths,
- preserving sensitive subpaths under writable roots,
- handling symlinked writable roots safely,
- expanding unreadable glob patterns to concrete masked paths,
- constructing namespace flags such as `--unshare-user`, `--unshare-pid`, and `--unshare-net`.

This is the core filesystem policy compiler.

### `src/landlock.rs`

This module applies in-process Linux restrictions:

- `PR_SET_NO_NEW_PRIVS`,
- seccomp filters for restricted networking,
- optional legacy Landlock filesystem rules.

In the current design, seccomp is active and central; Landlock filesystem rules are kept as a
legacy backend rather than the default path.

### `src/launcher.rs`

This module decides how bubblewrap is launched:

- prefer a system `bwrap` found on `PATH`,
- probe whether it supports `--argv0`,
- fall back to the vendored build when needed,
- ensure preserved file descriptors survive `execv` when using the system binary.

This isolates the "which bubblewrap do we trust and how do we exec it?" logic from the policy logic.

### `src/vendored_bwrap.rs`

This is the Rust/FFI adapter for the build-time-compiled bubblewrap C code.
If vendored bubblewrap was linked successfully, it exposes:

- `exec_vendored_bwrap(argv, preserved_files) -> !`

Otherwise it panics with a clear build/configuration error.

### `src/proxy_routing.rs`

This module implements managed proxy mode. It:

- discovers proxy environment variables,
- only accepts loopback proxy endpoints,
- creates a host-side TCP-to-UDS bridge,
- creates a sandbox-side loopback TCP listener that forwards to the UDS,
- rewrites proxy environment variables inside the sandbox to use the local bridge,
- cleans up transient socket directories and helper processes.

This is the most operationally complex module because it mixes policy, process management, sockets,
and namespace assumptions.

## Core Runtime Flow

### 1. Parse command and policies

`LandlockCommand` in `src/linux_run_main.rs` accepts both:

- legacy `SandboxPolicy`, and
- split `FileSystemSandboxPolicy` + `NetworkSandboxPolicy`.

The helper keeps the legacy field names for compatibility, even though bubblewrap is now the
default path and Landlock is no longer the primary mechanism.

`resolve_sandbox_policies(...)` normalizes the input into:

- `sandbox_policy`
- `file_system_sandbox_policy`
- `network_sandbox_policy`

It rejects partial split-policy input and verifies that combined legacy+split input is either
semantically equivalent or requires direct runtime enforcement.

### 2. Fast path for unrestricted filesystem

If the filesystem policy grants full disk write access and managed proxy routing is not requested,
the helper skips bubblewrap entirely and only applies the in-process restrictions needed by policy.

This avoids paying bubblewrap overhead when filesystem isolation adds no value.

### 3. Bubblewrap outer stage

If bubblewrap is active, the helper:

- optionally prepares managed proxy bridge metadata on the host,
- serializes policy state,
- builds an inner command that re-execs this same binary with
  `--apply-seccomp-then-exec`,
- constructs bubblewrap args,
- optionally probes whether `--proc /proc` works in the current environment,
- launches bubblewrap.

This stage establishes the filesystem and namespace view before seccomp is applied.

### 4. Bubblewrap inner stage

Inside the bubblewrap environment, the helper:

- optionally activates managed proxy routes inside the new network namespace,
- applies `PR_SET_NO_NEW_PRIVS` and seccomp,
- skips filesystem Landlock because bubblewrap already handled filesystem isolation,
- `exec`s the target command.

This sequencing matters because setuid/system bubblewrap needs to run before `no_new_privs` is set.

### 5. Legacy Landlock path

If `--use-legacy-landlock` is selected, the helper skips bubblewrap and:

- applies `PR_SET_NO_NEW_PRIVS`,
- installs seccomp if the network policy requires it,
- installs Landlock filesystem rules when compatible,
- `exec`s the target command.

This path only supports legacy-compatible policy shapes and explicitly rejects split policies that
need more expressive runtime enforcement.

## Bubblewrap Policy Compilation

The bubblewrap module is the most policy-heavy part of the crate.

### Filesystem model

The effective model is:

- read-only by default,
- explicit writable roots rebound with `--bind`,
- protected subpaths re-applied after broader writable binds,
- unreadable paths masked with tmpfs or `--ro-bind-data`,
- narrower writable descendants can reopen broader denied/read-only parents.

The implementation relies heavily on mount ordering to make overlapping rules behave correctly.

### Important behaviors

- Full-write + full-network can bypass bubblewrap entirely.
- Full-write + unreadable globs still uses bubblewrap, because glob matches must be concretely masked.
- Restricted read starts from `--tmpfs /` and layers approved readable mounts.
- Full read starts from `--ro-bind / /`.
- `/dev` is rebuilt with `--dev /dev` so standard device nodes stay available.
- Missing writable roots are skipped instead of failing startup, which helps cross-platform/shared configs.

### Symlink safety

The module explicitly treats writable symlinks as dangerous.
If a protected read-only or deny-read path crosses a symlink that remains writable from inside the
sandbox, the code fails closed instead of masking the symlink's current target.

That is a good safety choice because otherwise the sandbox would depend on a startup-time symlink
snapshot and become vulnerable to post-start TOCTTOU path rewrites.

### Unreadable glob expansion

Unreadable glob entries are expanded before launch:

- preferred mechanism: `rg --files --hidden --no-ignore --glob ...`,
- fallback: internal `globset`-based filesystem walk,
- hard cap: `MAX_UNREADABLE_GLOB_MATCHES = 8192`.

This is practical and intentionally conservative:

- it masks actual existing paths for bubblewrap,
- it preserves deny-read intent even when ripgrep is absent,
- it refuses overly broad expansions instead of silently weakening the sandbox.

One subtle limitation is that bubblewrap only sees startup-time matches; later-created files that
would match a deny glob are not automatically masked by bubblewrap itself. The source comments
acknowledge that other helpers still evaluate original patterns at read time, but the boundary
between bwrap-time masking and runtime policy checks is worth keeping in mind.

## Seccomp and Landlock Behavior

`src/landlock.rs` makes a clear distinction between filesystem and network enforcement.

### `PR_SET_NO_NEW_PRIVS`

The helper only enables `no_new_privs` when needed:

- when seccomp will be installed, or
- when the legacy Landlock filesystem backend is used.

That avoids breaking system bubblewrap setups that still depend on setuid behavior before the
sandbox is fully established.

### Network seccomp modes

There are two internal seccomp modes:

- `Restricted`: deny common network syscalls and permit only AF_UNIX sockets,
- `ProxyRouted`: allow AF_INET/AF_INET6 sockets in the isolated namespace but deny AF_UNIX
  creation for the user command.

This distinction is important:

- normal restricted mode blocks outward networking broadly,
- managed proxy mode still allows the command to talk to the local TCP bridge, while trying to
  prevent alternative AF_UNIX bypasses.

### Legacy Landlock

Landlock is present but secondary.
The implementation supports a legacy write-allowlist model and explicitly rejects restricted
read-only filesystem semantics for the legacy backend.

This is consistent with the rest of the crate: expressive filesystem policy is expected to go
through bubblewrap, not Landlock.

## Managed Proxy Routing Design

Managed proxy mode is a custom network design layered on top of bubblewrap network isolation.

### Intent

The goal is:

- deny direct egress,
- still allow traffic to configured proxy endpoints,
- keep that allowance fail-closed,
- only trust proxy configuration that points to loopback addresses.

### How it works

On the host side:

- scan proxy-related env vars,
- parse only loopback endpoints such as `127.0.0.1`, `::1`, or `localhost`,
- create a private temp directory for UDS sockets,
- spawn host bridge processes that accept UDS traffic and relay it to the original TCP proxy endpoint.

Inside the sandbox network namespace:

- create local loopback listeners,
- connect those listeners to the host UDS sockets,
- rewrite the proxy env vars to point to `127.0.0.1:<local_port>`.

The user command then talks to a local proxy address, but the actual path is:

`sandbox process -> local loopback TCP -> UDS bridge -> host TCP proxy endpoint`

### Design strengths

- fail-closed when no valid proxy env is present,
- excludes non-loopback proxy endpoints from automatic routing,
- cleans up socket directories associated with dead processes,
- uses `PR_SET_PDEATHSIG` in helper children to reduce orphaned bridge processes.

### Design complexity / risk

- depends on `fork`, raw fds, child process management, and thread spawning,
- may need permissions to create namespaces or bring loopback up,
- rewrites env vars dynamically, so failures can be environmental rather than purely logical,
- error handling is a mix of `Result` and hard panic paths.

This is effective but operationally dense code.

## Public and Internal APIs

### Public API

The public Rust API is intentionally tiny:

- `codex_linux_sandbox::run_main() -> !`

This crate is mostly an executable subsystem, not a reusable library abstraction.

### Important internal APIs

- `linux_run_main::run_main()` — top-level orchestration
- `resolve_sandbox_policies(...)` — normalize legacy/split policy inputs
- `build_inner_seccomp_command(...)` — prepare self re-exec inside bubblewrap
- `run_bwrap_with_proc_fallback(...)` — bubblewrap startup with `/proc` probing
- `bwrap::create_bwrap_command_args(...)` — compile filesystem policy to bubblewrap argv
- `landlock::apply_sandbox_policy_to_current_thread(...)` — apply `no_new_privs`, seccomp, and optional legacy Landlock
- `launcher::exec_bwrap(...)` — choose and exec system or vendored bubblewrap
- `proxy_routing::prepare_host_proxy_route_spec(...)` / `activate_proxy_routes_in_netns(...)` — managed proxy bridge lifecycle

## Dependencies and Their Roles

The manifest shows a small but meaningful dependency set.

### Core workspace crates

- `codex-protocol`: provides `SandboxPolicy`, split policy types, and error types.
- `codex-sandboxing`: provides helper logic such as system bubblewrap discovery and shared constants.
- `codex-utils-absolute-path`: used heavily to maintain normalized absolute path semantics.

These three crates define most of the semantic contract that `codex-linux-sandbox` implements.

### External crates

- `clap`: CLI parsing for the helper entrypoint.
- `landlock`: Landlock ruleset construction.
- `libc`: low-level Linux syscalls, `exec`, `fork`, pipes, prctl, ioctl, fd flags.
- `seccompiler`: seccomp BPF filter construction and installation.
- `serde` / `serde_json`: serialization of policies and proxy route specs across re-exec boundaries.
- `globset`: fallback glob expansion for unreadable path matching.
- `url`: parsing and rewriting proxy URLs.

### Build dependencies

- `cc`: compile vendored bubblewrap C files.
- `pkg-config`: discover `libcap` headers needed by bubblewrap.

### Dev dependencies

- `tokio`: async integration tests.
- `tempfile`: temp dirs/files for policy and filesystem tests.
- `pretty_assertions`: clearer failures.
- `codex-config` and `codex-core`: integration tests drive the sandbox the same way the higher-level execution stack does.

## Testing Strategy

The crate has good breadth of tests across multiple levels.

### Unit tests in source modules

The modules contain focused unit tests for:

- bubblewrap launcher selection and fd inheritance,
- filesystem mount ordering and carveout behavior,
- symlink fail-closed handling,
- unreadable glob expansion,
- proc mount failure detection,
- policy resolution rules,
- network seccomp mode selection,
- proxy route planning and URL rewriting.

This is strong coverage for the tricky policy compiler logic.

### Integration tests in `tests/suite/landlock.rs`

Despite the filename, this suite mostly validates end-to-end Linux sandbox behavior:

- root read succeeds,
- root write fails,
- writable roots behave correctly,
- `NoNewPrivs` is set,
- timeouts surface,
- network tools such as `curl`, `wget`, `ping`, `nc`, `ssh`, and `getent` are blocked,
- `.git` and `.codex` writes are denied even under writable roots,
- split-policy carveouts and nested reopen behavior work at runtime.

These tests are especially valuable because they exercise the crate through `codex_core::process_exec_tool_call`,
which is close to the real production call path.

### Integration tests in `tests/suite/managed_proxy.rs`

These validate the managed proxy subsystem:

- fail-closed behavior without proxy env vars,
- successful routing through a loopback proxy bridge,
- denial of direct egress in proxy-only mode,
- denial of AF_UNIX socket creation for user commands.

The tests also explicitly skip in environments where the necessary namespace permissions are unavailable,
which reflects the real deployment complexity of this feature.

## Design Assessment

### Strengths

- Clear separation of concerns between orchestration, filesystem compilation, process launching,
  seccomp, and proxy routing.
- Good fail-closed choices around writable symlinks and missing proxy configuration.
- Strong practical handling of real Linux deployment issues, especially older system `bwrap`,
  environments that cannot mount `/proc`, and missing `rg`.
- High-value integration coverage for actual sandbox behavior, not just argument generation.
- Very small public API, which limits accidental coupling.

### Tradeoffs and weak spots

- Error handling is inconsistent: policy-building APIs often return `Result`, but many runtime
  setup paths still `panic!`, especially around `exec`, fd handling, and child process setup.
- `linux_run_main.rs` is doing a lot: CLI compatibility, policy resolution, process staging,
  `/proc` preflight, proxy setup, and final exec.
- The main CLI struct is still named `LandlockCommand`, which no longer reflects the dominant backend.
- Managed proxy routing is sophisticated but increases the amount of unsafe-adjacent operational code.
- Some behavior is recognized by stderr string matching, especially `/proc` mount failure detection,
  which can be brittle across bubblewrap versions.

## Notable Design Details

### Bubblewrap compatibility strategy

The launcher prefers a system binary when available, but it does not assume feature parity.
The explicit `--help` probe for `--argv0` support is a pragmatic compatibility choice for older distro packages.

### Two-stage self re-exec

The outer/inner stage split is one of the key design ideas in the crate:

- outer stage establishes namespaces and filesystem view,
- inner stage applies seccomp only after bubblewrap has done its work.

That sequencing is correct for environments where bubblewrap may still rely on setuid behavior.

### `/proc` preflight fallback

Before the real run, the helper can start a short-lived child bubblewrap process running `/bin/true`
with `--proc /proc`, capture stderr, and disable `/proc` mounting if the environment is known to reject it.

This improves robustness in locked-down container environments, although it is heuristic rather than structural.

## Open Questions

These are the main questions that stood out during analysis.

1. Should more runtime setup failures return structured errors instead of panicking?
   The current panic-heavy approach is simple, but it makes caller-facing failure modes less uniform.

2. Should `linux_run_main.rs` be split further?
   It currently owns CLI compatibility, policy normalization, bubblewrap staging, `/proc` fallback,
   and proxy-mode activation.

3. Is the `/proc` failure detection robust enough across bubblewrap versions and locales?
   It currently depends on matching specific stderr fragments.

4. How strong is the long-term story for unreadable globs created after sandbox startup?
   Startup-time expansion is practical for bubblewrap, but it is not a complete dynamic deny mechanism on its own.

5. Is legacy naming worth cleaning up now?
   `LandlockCommand` and some comments retain the old mental model, while the actual design is now bubblewrap-first.

6. Should managed proxy mode be factored into its own crate or lower-level subsystem?
   It is cohesive in purpose, but large enough to be a subsystem by itself.

7. Are there environments where bringing loopback up inside the isolated namespace is too privileged or surprising?
   The code handles some failures, but this remains a deployment-sensitive operation.

## Bottom Line

`codex-linux-sandbox` is a focused Linux sandbox execution crate with a small public API and a fairly
large amount of internal systems logic. The current design is bubblewrap-first, seccomp-backed, and
keeps Landlock only as a constrained legacy path. The strongest parts of the crate are the filesystem
policy compiler in `bwrap.rs`, the staged execution model in `linux_run_main.rs`, and the breadth of
tests that validate real runtime behavior. The main complexity hotspot is managed proxy routing, and
the main maintainability hotspot is that orchestration and error handling still lean heavily on one
large runtime module plus many panic-based fatal paths.
