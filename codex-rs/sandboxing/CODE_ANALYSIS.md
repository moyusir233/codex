# codex-sandboxing Code Analysis

## Scope

This document analyzes the crate at `/Users/bytedance/project/codex/codex-rs/sandboxing`.
It is based on:

- `Cargo.toml`
- the production modules under `src/`
- the bundled Seatbelt policy assets
- the unit tests colocated beside the modules
- a few downstream call sites in `core/`, `exec-server/`, and `cli/` to understand integration flow

The crate is not a standalone sandbox runtime. Instead, it is the policy-selection and
command-transformation layer that translates Codex sandbox policy objects into platform-specific
execution requests.

## High-Level Purpose

`codex-sandboxing` sits between high-level runtime policy and low-level process execution.
Its main job is to answer two questions:

1. Should this command run under a platform sandbox at all?
2. If yes, what concrete argv, environment, and policy payloads should the executor spawn?

In practice that means:

- selecting `None`, macOS Seatbelt, Linux seccomp/helper, or Windows restricted-token sandbox modes
- merging extra runtime-granted permissions into the base sandbox policy
- translating policy objects into platform-specific launch arguments
- carrying enough metadata forward so downstream crates can execute the transformed command

The crate is intentionally thin on actual sandbox enforcement. Enforcement happens elsewhere:

- macOS enforcement is delegated to `/usr/bin/sandbox-exec`
- Linux enforcement is delegated to the `codex-linux-sandbox` helper
- Windows enforcement is handled by downstream Windows-specific execution paths

This crate owns the translation boundary, not the enforcement engine.

## Crate Surface

### Manifest

The crate declares a single library:

- package name: `codex-sandboxing`
- library crate name: `codex_sandboxing`
- root: `src/lib.rs`

There is no binary target and no build script.

### Public Exports

The public API exposed through `src/lib.rs` is small and purpose-driven:

- `SandboxManager`
  - central orchestrator for initial sandbox selection and command transformation
- `SandboxType`
  - enum describing the execution strategy: `None`, `MacosSeatbelt`, `LinuxSeccomp`, `WindowsRestrictedToken`
- `SandboxablePreference`
  - caller preference: `Auto`, `Require`, `Forbid`
- `SandboxCommand`
  - portable command description before transformation
- `SandboxTransformRequest`
  - all inputs required to derive a transformed execution request
- `SandboxExecRequest`
  - concrete launch request after policy merging and platform translation
- `SandboxTransformError`
  - transformation-time error type
- `get_platform_sandbox(...)`
  - detects the platform-specific sandbox kind to use when sandboxing is required

Platform-specific exports:

- Linux only:
  - `find_system_bwrap_in_path(...)`
  - `system_bwrap_warning(...)`
- macOS only:
  - public `seatbelt` module
- all platforms:
  - public `landlock` and `policy_transforms` modules

## Module Responsibilities

### `src/manager.rs`

This is the core orchestration module.

It defines the crate’s primary data model:

- `SandboxType`
- `SandboxablePreference`
- `SandboxCommand`
- `SandboxTransformRequest`
- `SandboxExecRequest`
- `SandboxTransformError`
- `SandboxManager`

`SandboxManager` has two main responsibilities:

1. `select_initial(...)`
   - chooses whether a platform sandbox is required
   - respects caller preference (`Auto` / `Require` / `Forbid`)
   - delegates the decision logic to `policy_transforms::should_require_platform_sandbox(...)`
   - resolves the actual platform implementation via `get_platform_sandbox(...)`

2. `transform(...)`
   - merges additional permissions into the base policies
   - converts a portable command into the final platform-specific command line
   - returns a `SandboxExecRequest` that downstream executors can spawn

The `transform(...)` path is the heart of the crate:

- it computes effective policy objects by merging base policy and ad hoc permissions
- it preserves cwd, env, network context, and Windows metadata
- it rewrites argv differently per platform

Per-sandbox behavior:

- `SandboxType::None`
  - returns the original command as strings
- `SandboxType::MacosSeatbelt`
  - prefixes the command with `/usr/bin/sandbox-exec`
  - generates an inline SBPL policy via `seatbelt::create_seatbelt_command_args(...)`
- `SandboxType::LinuxSeccomp`
  - requires a path to the `codex-linux-sandbox` helper
  - serializes policy objects into helper CLI flags
  - optionally guards against WSL1 + bubblewrap-involving paths
- `SandboxType::WindowsRestrictedToken`
  - currently leaves argv unchanged here and relies on downstream Windows execution plumbing

Important design detail: `SandboxExecRequest` carries both the transformed launch command and the
effective policy objects that produced it. That lets downstream runtime code preserve policy
metadata for execution, telemetry, and environment shaping.

### `src/policy_transforms.rs`

This module owns policy math.

Its responsibilities include:

- normalizing caller-supplied additional permission profiles
- merging multiple permission profiles
- intersecting requested permissions with granted permissions
- projecting additional permissions into both:
  - legacy `SandboxPolicy`
  - split `FileSystemSandboxPolicy` / `NetworkSandboxPolicy`
- deciding whether a platform sandbox is necessary at all

This is the most policy-heavy module in the crate.

Notable behaviors:

- file-system permission entries are deduplicated
- explicit paths are canonicalized while trying to preserve symlink intent
- glob entries are only allowed for deny-read semantics
- network permissions collapse to effectively boolean enable/disable behavior
- deny entries and glob scan depth are preserved when policies are merged

One subtle design choice appears in `sandbox_policy_with_additional_permissions(...)`:

- if a read-only sandbox receives extra write permissions, the code upgrades it to
  `SandboxPolicy::WorkspaceWrite`
- the source comment explicitly says this is an approximation that may grant more access than the
  precise request

That is a deliberate tradeoff and an important current limitation.

### `src/seatbelt.rs`

This module translates effective policy into a macOS Seatbelt profile and command line.

It owns:

- the hardcoded executable path `/usr/bin/sandbox-exec`
- inclusion of bundled SBPL fragments
- filesystem allow/deny rule generation
- dynamic network policy generation
- proxy-aware loopback and Unix-domain-socket rules
- conversion of unreadable globs into Seatbelt regex denies
- parameterization of the generated policy with `-DKEY=VALUE` bindings

This is the most platform-specific and policy-dense module in the crate.

Key implementation ideas:

- filesystem access is expressed as generated SBPL forms with parameterized roots
- explicit unreadable roots are translated into carve-outs from broader read/write access
- unreadable glob patterns are translated into anchored regex deny rules
- proxy configuration narrows network access to loopback proxy ports instead of granting blanket
  outbound networking
- Unix socket permissions can be:
  - fully open
  - explicitly allowlisted by path

The network path is intentionally fail-closed in several cases:

- proxy config exists but no valid ports can be inferred
- managed-network enforcement is enabled but no usable proxy endpoints exist

In those cases the module returns only the restricted network profile, not broad network access.

`seatbelt.rs` also contains the only explicit `unsafe` usage in this crate:

- `libc::confstr(...)` is wrapped to obtain Darwin system directories for policy parameters

The unsafe blocks are small and locally justified:

- one call into `libc::confstr`
- one `CStr::from_ptr(...)` over the returned NUL-terminated buffer

### `src/landlock.rs`

Despite the module name, this file is primarily a Linux helper-argv builder.

Its responsibilities are:

- define the helper arg0 alias `codex-linux-sandbox`
- decide whether proxy-only networking should be enabled for the helper
- serialize split sandbox policies into the CLI expected by `codex-linux-sandbox`

This module does not itself apply Landlock or seccomp restrictions. Instead, it packages policy
state for the external helper.

The naming reflects historical evolution:

- the public API still says “landlock”
- the active `SandboxType` is called `LinuxSeccomp`
- the actual helper path supports bubblewrap, seccomp, and a legacy Landlock mode

That makes this module more of a Linux policy transport layer than a direct Landlock adapter.

### `src/bwrap.rs`

This module deals with system bubblewrap discovery and user guidance.

It owns:

- finding a trusted `bwrap` on `PATH`
- skipping workspace-local `bwrap` binaries to avoid path injection from the current project
- detecting WSL1 from `/proc/version`
- probing whether system bubblewrap can create user namespaces
- generating startup warnings when:
  - `bwrap` is missing
  - user namespace creation appears unavailable
  - WSL1 is detected

This module is user-experience and safety glue around Linux prerequisites. It does not construct
the final Linux helper command; that happens in `manager.rs` and `landlock.rs`.

### `src/lib.rs`

This file exposes the public surface and handles error conversion into `codex_protocol::error::CodexErr`.

Notable points:

- Linux-only exports are conditionally re-exported
- on non-Linux platforms, `system_bwrap_warning(...)` is stubbed to `None`
- `SandboxTransformError` is mapped into protocol-level errors

## End-to-End Flow

The crate’s runtime flow is easiest to understand as a pipeline.

### 1. Caller selects a base policy

Downstream crates such as `core` or `exec-server` decide:

- sandbox policy objects
- file-system/network policy split
- managed-network requirements
- platform settings such as Windows sandbox level

### 2. `SandboxManager::select_initial(...)` chooses a sandbox mode

Inputs:

- file-system policy
- network policy
- preference (`Auto`, `Require`, `Forbid`)
- Windows sandbox enabled/disabled state
- managed-network requirement flag

Selection logic:

- `Forbid` forces `SandboxType::None`
- `Require` forces the platform sandbox if one exists
- `Auto` only enables sandboxing when `should_require_platform_sandbox(...)` says the policy needs it

The main decision heuristic is:

- managed-network requirements always require a platform sandbox
- restricted network access usually requires a platform sandbox unless an external sandbox is already
  responsible
- write restrictions on Linux/macOS require a platform sandbox when filesystem access is not
  effectively unrestricted

### 3. `SandboxManager::transform(...)` computes effective policies

Before building argv, the manager merges any command-specific `additional_permissions` into:

- legacy `SandboxPolicy`
- `FileSystemSandboxPolicy`
- `NetworkSandboxPolicy`

This is done through `policy_transforms`.

The result is important because the transformed command must reflect the actual permissions granted
for this execution, not just the session’s base policy.

### 4. The platform adapter rewrites argv

Depending on `SandboxType`:

- none:
  - keep the original argv
- macOS:
  - build SBPL text
  - prepend `/usr/bin/sandbox-exec -p <policy> ... --`
- Linux:
  - require a `codex-linux-sandbox` executable path
  - serialize policy objects as JSON flags
  - prepend helper executable path
  - set an arg0 override so self-reexec works under the helper alias
- Windows:
  - preserve argv here; downstream Windows runtime applies the real enforcement strategy

### 5. Downstream execution code consumes `SandboxExecRequest`

This crate stops at the point where execution becomes platform/runtime-specific.

Downstream examples:

- `core/src/exec.rs`
  - uses `SandboxManager` to build a transformed request before spawning commands
- `core/src/sandboxing/mod.rs`
  - converts `SandboxExecRequest` into the runtime’s `ExecRequest`
  - injects environment markers such as `CODEX_SANDBOX_NETWORK_DISABLED_ENV_VAR`
- `exec-server/src/fs_sandbox.rs`
  - uses the same manager for file-system helper commands
- `cli/src/debug_sandbox.rs`
  - directly exercises the platform-specific argument builders for debugging

This confirms the intended architectural role: `codex-sandboxing` is shared transformation logic
used by multiple execution frontends.

## Dependencies

### First-party crates

- `codex-network-proxy`
  - models managed proxy/network settings
  - applies proxy env vars
  - provides Unix-socket and loopback policy inputs on macOS
- `codex-protocol`
  - defines the sandbox policy types, permission profiles, and protocol error mapping
- `codex-utils-absolute-path`
  - provides normalized absolute path handling

### Third-party crates

- `dunce`
  - cross-platform canonicalization behavior used in tests
- `libc`
  - macOS `confstr(...)` access
- `serde_json`
  - serializes policies for the Linux helper CLI
- `regex-lite`
  - escapes and validates Seatbelt regex generation for unreadable globs
- `tracing`
  - logs warnings when policy/socket normalization fails
- `url`
  - parses proxy env vars to infer loopback ports
- `which`
  - discovers trusted `bwrap` binaries on `PATH`

### Dev dependencies

- `anyhow`
- `async-trait`
- `pretty_assertions`
- `tempfile`
- `tokio`

These are used heavily in tests, especially where proxy configuration or actual Seatbelt invocation
is exercised.

## Testing Strategy

The crate has colocated test modules for every major production module:

- `manager_tests.rs`
- `bwrap_tests.rs`
- `landlock_tests.rs`
- `seatbelt_tests.rs`
- `policy_transforms_tests.rs`

That gives the crate strong unit-level coverage with some selective platform integration behavior.

### What the tests cover well

#### Selection and transformation

`manager_tests.rs` validates:

- when sandboxing is selected automatically
- how managed-network requirements influence selection
- additional-permission merging effects
- WSL1 restrictions for Linux bubblewrap-involving paths
- Linux helper arg0 override behavior

#### Bubblewrap discovery and warnings

`bwrap_tests.rs` validates:

- missing `bwrap` warning behavior
- namespace-failure warning behavior
- WSL1 detection
- path-search behavior that skips workspace-local `bwrap`

#### Linux helper argv generation

`landlock_tests.rs` validates:

- presence/absence of `--use-legacy-landlock`
- presence of `--allow-network-for-proxy`
- split policy serialization into helper flags

#### macOS Seatbelt translation

`seatbelt_tests.rs` is the largest and most behavior-rich suite. It validates:

- base policy assumptions
- proxy-routed network rules
- explicit local-binding behavior
- fail-closed behavior when proxy information is incomplete
- Unix-socket allowlist and allow-all modes
- unreadable root carve-outs
- unreadable glob translation into regex
- canonicalization of symlinked glob prefixes
- protection of `.git` and `.codex` paths under legacy policy conversion
- some real `sandbox-exec` command executions, not just string matching

#### Permission math

`policy_transforms_tests.rs` validates:

- normalization of additional permissions
- symlink-preserving path normalization
- deny-glob handling and bounded/unbounded glob depth
- permission intersection rules
- policy merging while preserving deny entries
- sandbox-requirement heuristics

### What the tests do not fully cover

- there is no full end-to-end test in this crate for Linux helper execution itself; that belongs to
  `codex-linux-sandbox`
- Windows behavior is mostly structural here because real Windows enforcement is downstream
- some panic paths remain untested, especially UTF-8 and JSON serialization assumptions

Overall, the crate is well covered in the translation layer and intentionally leaves true
enforcement testing to platform-specific crates.

## Design Observations

### 1. Clear separation between policy math and enforcement

This crate is architecturally clean in one important way:

- policy selection and transformation live here
- sandbox enforcement engines live elsewhere

That separation makes the crate reusable across:

- the main core runtime
- debug CLI flows
- exec-server helper flows

### 2. Split-policy migration is in progress but incomplete

The crate clearly supports both:

- legacy `SandboxPolicy`
- newer split `FileSystemSandboxPolicy` and `NetworkSandboxPolicy`

The manager computes and carries both. The Linux helper builder serializes all three policy views.
This suggests the system is in a compatibility phase where both representations are still needed.

### 3. Linux naming still reflects older architecture

The Linux-specific pieces are historically layered:

- module name: `landlock`
- sandbox enum variant: `LinuxSeccomp`
- helper behavior: bubblewrap + seccomp, with optional legacy Landlock

The code is internally consistent, but the terminology exposes the migration path and can make the
actual responsibility of each piece less obvious to new readers.

### 4. macOS policy generation is sophisticated and security-conscious

The Seatbelt generator does more than basic allowlisting:

- it accounts for proxy-only networking
- it handles explicit Unix-socket routing
- it canonicalizes paths when useful
- it preserves unreadable carve-outs even when access is otherwise broad
- it encodes fail-closed behavior when managed network constraints are underspecified

This is a strong sign that macOS policy generation is security-sensitive, not a thin wrapper.

### 5. Some transformation paths deliberately panic

A few code paths use `panic!` or `unwrap_or_else(...)` with panic messages for:

- policy serialization failures
- non-UTF-8 cwd values when building Linux helper args
- impossible root-path construction

That implies these are treated as invariant violations rather than recoverable user errors.
That may be correct, but it is a notable design choice because this crate otherwise exposes a
recoverable `SandboxTransformError`.

### 6. Windows support is metadata-forwarding, not full translation

The crate includes `WindowsRestrictedToken` in the public API and forwards Windows sandbox settings
through `SandboxExecRequest`, but it does not build a Windows helper command here.

That means:

- sandbox selection is centralized
- Windows enforcement remains decentralized downstream

This asymmetry is likely intentional, but it matters for maintainability expectations.

## Notable Constraints and Invariants

- Linux sandbox transformation requires `codex_linux_sandbox_exe`; otherwise transformation fails
- WSL1 cannot use Linux bubblewrap-involving paths and is rejected early
- macOS Seatbelt only supports `/usr/bin/sandbox-exec`, not arbitrary `PATH` lookups
- additional permission globs only support deny-read semantics
- many CLI argument builders assume cwd and policy paths are valid UTF-8
- `SandboxCommand` stores program as `OsString`, but transformed commands are eventually flattened
  into `Vec<String>` using lossy conversion where necessary

## Open Questions

1. Should Linux naming be modernized?
   - The public `landlock` module now mostly packages arguments for `codex-linux-sandbox`, not just
     Landlock. A rename could better reflect the current responsibility, but it would have API
     compatibility costs.

2. Should `SandboxTransformError` cover more invariant failures?
   - Some failures currently panic instead of returning errors, especially serialization and UTF-8
     assumptions in Linux/macOS builders.

3. Is the legacy `SandboxPolicy` still required long term?
   - The crate carries both legacy and split-policy representations in parallel. If all consumers
     can move to split policies, the API and transformation logic could simplify substantially.

4. Should read-only plus extra writes become a more precise sandbox variant?
   - The current upgrade from `ReadOnly` to `WorkspaceWrite` is acknowledged in code as broader than
     the exact request.

5. Should Windows translation also be centralized here?
   - Today this crate centralizes selection but not a true platform command transformation for
     Windows. That keeps the API asymmetric across platforms.

6. How much path canonicalization is desirable?
   - Some code preserves symlink intent while some Seatbelt deny-glob logic adds canonicalized
     variants. The balance is security-sensitive and could merit a more explicit documented policy.

7. Should `allow_network_for_proxy(...)` remain a boolean passthrough?
   - Right now it effectively mirrors `enforce_managed_network`. The dedicated function suggests
     future policy nuance may be expected.

## Bottom Line

`codex-sandboxing` is best understood as Codex’s shared sandbox translation layer.

It owns:

- sandbox mode selection
- permission-profile normalization and merging
- platform-specific command rewriting
- Linux prerequisite warnings
- macOS Seatbelt policy synthesis

It does not own:

- actual Linux sandbox enforcement
- actual process spawning
- most Windows enforcement details

The crate is compact, well-tested, and security-oriented. Its main complexity comes from policy
compatibility, macOS Seatbelt generation, and the need to bridge high-level sandbox intent to
multiple downstream execution environments without duplicating translation logic.
