# codex-shell-escalation: Code Analysis

## Purpose and scope

`codex-shell-escalation` is a Unix-only crate that lets a patched interactive shell delegate each intercepted `execve(2)` attempt to a supervising server. The server decides whether the command:

- runs normally inside the shell process,
- is re-executed outside the shell sandbox with inherited stdio, or
- is denied outright.

The crate contains both the library protocol/runtime and the `codex-execve-wrapper` helper binary. The high-level contract is described in [README.md](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/README.md#L1-L29), while the module-level overview and ASCII flow diagrams live in [mod.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/mod.rs#L1-L55).

## Package and build surface

- The Cargo package is named `codex-shell-escalation`, exports the Rust crate `codex_shell_escalation`, and defines one binary: `codex-execve-wrapper` in [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/Cargo.toml#L1-L39).
- Bazel mirrors the crate with a lightweight wrapper target in [BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/BUILD.bazel#L1-L6).
- The public library surface is a thin Unix-gated re-export layer in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/lib.rs#L1-L35).
- The shell integration depends on a local zsh patch that injects `EXEC_WRAPPER` ahead of the original target executable, shown in [zsh-exec-wrapper.patch](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/patches/zsh-exec-wrapper.patch#L1-L34).

## Concrete responsibilities

### 1. Define the escalation wire protocol

The crate defines the request/response types and control-plane enums in [escalate_protocol.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_protocol.rs#L10-L88):

- `ESCALATE_SOCKET_ENV_VAR` and `EXEC_WRAPPER_ENV_VAR` identify the inherited socket FD and wrapper executable.
- `EscalateRequest` carries the intercepted executable path, argv, working directory, and environment.
- `EscalateResponse` returns an `EscalateAction` of `Run`, `Escalate`, or `Deny`.
- `EscalationDecision` / `EscalationExecution` express the server-side policy decision before it is serialized into a simpler `EscalateAction`.
- `SuperExecMessage` and `SuperExecResult` support the second stage of escalation, where the wrapper forwards open file descriptors and receives the child exit code.

### 2. Provide the wrapper-side client runtime

[escalate_client.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_client.rs#L19-L124) is the logic executed by `codex-execve-wrapper`:

- Reads the inherited datagram socket FD from `CODEX_ESCALATE_SOCKET`.
- Creates a per-request stream socket pair for request/response traffic.
- Sends a one-byte handshake datagram plus the server stream endpoint via `SCM_RIGHTS`.
- Sends the full `EscalateRequest` over the stream socket.
- On `Escalate`, duplicates stdin/stdout/stderr and forwards those duplicated FDs so the server can faithfully attach them to the escalated child.
- On `Run`, performs a raw `libc::execv` instead of `std::process::Command` to avoid unwanted signal-mask or stdio manipulation.
- On `Deny`, writes a human-readable error to stderr and exits with code `1`.

### 3. Provide the supervising server runtime

[escalate_server.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_server.rs#L29-L375) is the core orchestration layer:

- `EscalationPolicy` decides whether an intercepted command should run, escalate, or deny.
- `ShellCommandExecutor` lets higher-level code keep ownership of actual shell spawning and sandbox integration.
- `EscalateServer::start_session()` creates the datagram socket pair, marks only the client endpoint as inheritable across `exec`, and returns the environment overlay required by the shell session in [escalate_server.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_server.rs#L180-L219).
- `EscalateServer::exec()` is a convenience API that starts a session, builds the shell command line, and delegates actual process execution to `ShellCommandExecutor::run()` in [escalate_server.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_server.rs#L145-L178).
- `escalate_task()` continuously accepts handshake datagrams and spawns one worker per intercepted exec request in [escalate_server.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_server.rs#L222-L258).
- `handle_escalate_session_with_policy()` resolves relative paths, invokes policy, optionally receives stdio FDs, prepares the final command, spawns the escalated child, and returns its exit status to the wrapper in [escalate_server.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_server.rs#L260-L375).

### 4. Own the UNIX socket and FD-passing primitives

[socket.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/socket.rs#L1-L523) is infrastructure that everything else depends on:

- `AsyncSocket` wraps `AF_UNIX` stream sockets for framed JSON messages plus optional passed FDs.
- `AsyncDatagramSocket` wraps `AF_UNIX` datagram sockets for handshake messages plus passed FDs.
- The framing layer prefixes stream messages with a little-endian `u32` payload length, then sends/receives JSON bytes.
- FD passing uses `SCM_RIGHTS`, with explicit control-message construction and parsing.
- The implementation intentionally uses `Socket::pair_raw` and then re-applies `FD_CLOEXEC` because `socket2::Socket::pair()` can apply extra platform-specific flags that fail on Apple UNIX sockets.

### 5. Export a pause-aware timeout helper

[stopwatch.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/stopwatch.rs#L10-L129) provides a small async stopwatch abstraction:

- Starts timing immediately on creation.
- Can expose a `CancellationToken` that cancels when the active elapsed time reaches a limit.
- Supports nested `pause_for()` calls so higher-level runtimes can suspend timeout accounting while waiting on external approval or similar pauses.

The stopwatch is exported by the crate, but it is not consumed inside this crate’s own server/client path. It appears intended for higher-level orchestration code, which matches its external usage elsewhere in the workspace.

## Public API surface

The public exports from [lib.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/lib.rs#L1-L35) and [mod.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/mod.rs#L56-L79) break down into these groups:

- **Protocol types:** `EscalateAction`, `EscalationDecision`, `EscalationExecution`, `EscalationPermissions`, `Permissions`.
- **Server-side APIs:** `EscalateServer`, `EscalationSession`, `EscalationPolicy`, `ShellCommandExecutor`, `ExecParams`, `ExecResult`, `PreparedExec`.
- **Wrapper-side APIs:** `run_shell_escalation_execve_wrapper()`, `main_execve_wrapper()`.
- **Utilities:** `ESCALATE_SOCKET_ENV_VAR`, `Stopwatch`.

The design keeps the crate reusable: callers implement policy and execution adapters, while the crate owns the transport and interception protocol.

## End-to-end control flow

### Session setup

1. The caller constructs `EscalateServer` with:
   - a shell path,
   - an exec wrapper path,
   - an `EscalationPolicy` implementation.
2. `start_session()` creates a datagram socket pair and returns an `EscalationSession` that exposes only two environment variables:
   - `CODEX_ESCALATE_SOCKET=<fd>`
   - `EXEC_WRAPPER=<path>`
3. The caller launches a shell and merges that environment overlay into the shell’s environment.

Relevant code: [escalate_server.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_server.rs#L185-L219)

### Intercepted exec path

1. The patched zsh code rewrites `execve(pth, argv, envp)` into `execve(EXEC_WRAPPER, [EXEC_WRAPPER, original_path, ...argv], envp)` when `EXEC_WRAPPER` is present.
2. `codex-execve-wrapper` parses the intercepted target path and argv in [execve_wrapper.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/execve_wrapper.rs#L6-L24) and then calls `run_shell_escalation_execve_wrapper()`.
3. The wrapper sends a datagram handshake containing a dedicated stream socket FD for this intercepted exec request.
4. The server accepts that stream FD and handles the request concurrently in its own task.

Relevant code: [zsh-exec-wrapper.patch](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/patches/zsh-exec-wrapper.patch#L16-L31), [escalate_client.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_client.rs#L36-L63), [escalate_server.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_server.rs#L222-L258)

### Decision path

1. The worker receives `EscalateRequest`.
2. Relative executable paths are resolved against the request’s `workdir`.
3. The policy decides `Run`, `Escalate(execution)`, or `Deny`.
4. The worker sends the corresponding `EscalateResponse`.

Relevant code: [escalate_server.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_server.rs#L267-L301)

### Non-escalated execution

1. The wrapper receives `EscalateAction::Run`.
2. It invokes raw `libc::execv(file, argv)` and never returns on success.
3. If `execv` fails, the wrapper returns an error.

Relevant code: [escalate_client.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_client.rs#L93-L115)

### Escalated execution

1. The wrapper receives `EscalateAction::Escalate`.
2. It duplicates stdin/stdout/stderr and sends those duplicates along with the destination fd numbers `[0, 1, 2]`.
3. The server invokes `ShellCommandExecutor::prepare_escalated_exec()` to convert the intercepted request plus `EscalationExecution` into a concrete process launch plan.
4. The server spawns the command with all stdio temporarily set to null, then uses `pre_exec` and `dup2()` to remap the received descriptors onto the requested destination fds in the child just before `exec`.
5. After the child exits, the server sends `SuperExecResult { exit_code }` back to the wrapper.

Relevant code: [escalate_client.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_client.rs#L64-L92), [escalate_server.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_server.rs#L296-L365)

### Denial path

1. The server returns `EscalateAction::Deny { reason }`.
2. The wrapper prints either `Execution denied` or `Execution denied: <reason>`.
3. The wrapper exits with status `1`.

Relevant code: [escalate_client.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_client.rs#L116-L122)

## Key abstractions and extension points

### `EscalationPolicy`

Defined in [escalation_policy.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalation_policy.rs#L1-L14), this trait is intentionally narrow:

- input: resolved executable path, argv, workdir,
- output: `EscalationDecision`.

It does not receive the full environment, timeout, or shell metadata, so policy decisions stay focused on the intercepted executable request itself.

### `ShellCommandExecutor`

Defined in [escalate_server.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_server.rs#L29-L62), this is the main integration seam:

- `run()` lets the embedding application launch the main shell command using its own sandboxing, capture, timeout, and process lifecycle policies.
- `prepare_escalated_exec()` lets the embedding application transform an intercepted subcommand into a concrete command/cwd/env/arg0 plan before the crate spawns it.

This separation is a strong design choice because it keeps sandbox-specific logic out of the protocol crate.

### `EscalationSession`

Defined in [escalate_server.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_server.rs#L95-L125), it owns:

- the exported environment overlay,
- the background handshake task,
- the client socket endpoint that must remain open across shell startup,
- cancellation on drop.

The `Drop` impl closes the inherited socket, cancels the session token, and aborts the background task. This makes session teardown explicit and defensive.

## Dependency analysis

From [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/Cargo.toml#L14-L39):

- `anyhow`: ergonomic error propagation across async/process/socket boundaries.
- `async-trait`: async trait methods for `EscalationPolicy` and `ShellCommandExecutor`.
- `clap`: CLI parsing for `codex-execve-wrapper`.
- `serde` + `serde_json`: serialize protocol messages over the UNIX sockets.
- `tokio` + `tokio-util`: async runtime, async fd/process handling, cancellation tokens.
- `socket2` + `libc`: low-level UNIX socket creation, raw fd transfer, `execv`, `dup2`, `recvmsg`, and control-message handling.
- `codex-protocol`: reuses workspace permission models, especially `EscalationPermissions`.
- `codex-utils-absolute-path`: ensures path normalization and relative resolution behavior are centralized.
- `tracing` + `tracing-subscriber`: debugging output for the wrapper binary and server internals.
- `pretty_assertions` + `tempfile` in tests: clearer diffs and temporary filesystem fixtures.

The dependency set is small and purpose-built; almost every non-workspace dependency maps directly to a concrete systems need.

## Unsafe code and invariants

This crate uses `unsafe` in a focused, systems-level way.

### Socket buffer and control-message handling

[socket.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/socket.rs#L23-L79) uses `MaybeUninit`, raw pointer casts, and direct `cmsghdr` parsing. The core invariants are:

- only initialized portions of buffers are reinterpreted,
- `SCM_RIGHTS` payload lengths are normalized before fd extraction,
- every received raw fd is immediately wrapped as `OwnedFd`.

### Raw fd ownership transfer

- `AsyncDatagramSocket::from_raw_fd()` assumes unique ownership of the passed raw fd in [socket.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/socket.rs#L378-L380).
- `get_escalate_client()` in [escalate_client.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_client.rs#L19-L28) relies on the environment fd being consumed exactly once.

### Child stdio remapping

[escalate_server.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_server.rs#L330-L347) uses `pre_exec` plus `dup2()` to remap received FDs into the child process. The important invariant is that the `msg.fds` destination numbers and the received `fds` array stay aligned one-to-one. The code checks that count before spawning.

### Direct `execv`

[escalate_client.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_client.rs#L93-L115) calls `libc::execv` directly to preserve transparency. This avoids higher-level process helpers mutating stdio or signal behavior before the target command runs.

## Testing analysis

The crate has unusually good unit/integration-style coverage for tricky FD-passing behavior.

### Socket tests

[socket.rs tests](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/socket.rs#L412-L523) cover:

- stream round-trip of JSON payloads and FDs,
- large payload framing beyond a single chunk,
- datagram round-trip of bytes and FDs,
- validation of excessive-fd rejection,
- oversized length-prefix rejection,
- EOF before frame header completion.

This gives confidence in the transport layer, which is the highest-risk low-level component.

### Wrapper/client tests

[escalate_client.rs tests](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_client.rs#L126-L143) contain a focused ownership test that verifies duplicated transfer FDs do not close the original descriptor.

Coverage here is intentionally light; most client behavior is exercised indirectly through server-side session tests.

### Server/session tests

[escalate_server.rs tests](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_server.rs#L377-L1062) cover:

- exported environment overlay and client socket lifecycle in [start_session_exposes_wrapper_env_overlay](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_server.rs#L597-L637),
- post-spawn socket cleanup hook in [exec_closes_parent_socket_after_shell_spawn](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_server.rs#L639-L671),
- `Run` decision flow in [handle_escalate_session_respects_run_in_sandbox_decision](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_server.rs#L673-L710),
- relative-path resolution in [handle_escalate_session_resolves_relative_file_against_request_workdir](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_server.rs#L712-L750),
- end-to-end escalated command execution in [handle_escalate_session_executes_escalated_command](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_server.rs#L752-L795),
- the subtle src-fd-equals-dst-fd overlap case during `dup2()` in [handle_escalate_session_accepts_received_fds_that_overlap_destinations](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_server.rs#L844-L915),
- permission propagation into `prepare_escalated_exec()` in [handle_escalate_session_passes_permissions_to_executor](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_server.rs#L917-L970),
- session drop aborting workers and killing an in-flight escalated child in [dropping_session_aborts_intercept_workers_and_kills_spawned_child](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_server.rs#L972-L1062).

This is the strongest part of the test suite because it directly targets race conditions, descriptor semantics, and cancellation behavior.

### Stopwatch tests

[stopwatch.rs tests](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/stopwatch.rs#L131-L237) cover:

- normal timeout firing,
- pausing across async waits,
- overlapping nested pauses,
- unlimited timers that never cancel.

## Design observations

### What is designed well

- **Clear layering:** protocol, wrapper, server, socket primitives, and timeout utilities are separated cleanly.
- **Good integration seams:** policy and execution are traits, so the crate stays decoupled from the workspace’s actual sandbox implementation.
- **Concurrency model is simple:** one datagram listener plus one task per intercepted exec request.
- **Teardown is defensive:** `EscalationSession` closes the inheritable socket and cancels/aborts worker state on drop.
- **Transport matches the problem:** datagrams are used only for lightweight handshake + fd passing, while stream sockets carry structured request/response frames.
- **Platform-specific details are acknowledged:** the comments around `socket2::pair_raw()` and `execv` show deliberate handling of macOS and UNIX behavior, not accidental systems code.

### Trade-offs and constraints

- The protocol depends on a patched zsh, so this is not a generic shell integration story.
- The worker protocol is local-machine, UNIX-only, and fd-centric; that is appropriate here, but not portable.
- JSON is simple and debuggable, but it adds serialization overhead compared with a binary format. That is probably acceptable because intercepted exec frequency is limited and messages are small.
- The environment overlay model is intentionally minimal, which is clean, but it means callers must be careful to merge rather than replace their shell environment.

## Open questions and follow-up ideas

### 1. `ExecParams::timeout_ms` is defined but not consumed in this crate

`ExecParams` includes `timeout_ms` in [escalate_server.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_server.rs#L64-L74), but `EscalateServer::exec()` never reads it in [escalate_server.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_server.rs#L145-L178). That suggests one of two things:

- timeout enforcement is expected entirely in the caller’s `ShellCommandExecutor::run()`, or
- the field is vestigial / future-facing and could mislead API consumers.

This is the biggest API-level ambiguity in the crate.

### 2. The wrapper-side inherited socket fd appears to be single-use, but that invariant is not enforced

[escalate_client.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_client.rs#L19-L28) includes a TODO noting that `AsyncDatagramSocket::from_raw_fd()` takes ownership of the fd, so calling `get_escalate_client()` more than once would be unsafe at the ownership level. A tighter API or explicit one-time guard would reduce footgun risk.

### 3. Signal forwarding during escalated execution is not implemented

[escalate_client.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_client.rs#L79-L79) has a TODO about forwarding signals over the super-exec socket. Today, escalated children inherit stdio, but signal semantics may differ from a truly transparent local `exec`.

### 4. Protocol limits are hard-coded

[socket.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/socket.rs#L19-L21) fixes `MAX_FDS_PER_MESSAGE = 16` and `MAX_DATAGRAM_SIZE = 8192`. That is reasonable for current usage, but future features may need:

- more than stdio plus a small number of extra descriptors,
- larger handshake payloads or metadata.

### 5. Policy decisions do not currently see the request environment

`EscalationPolicy::determine_action()` only receives file, argv, and workdir in [escalation_policy.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalation_policy.rs#L5-L13), even though the wrapper sends `env` in `EscalateRequest`. That may be a deliberate minimization, but it also means policy cannot inspect environment-driven behavior without moving that logic into `ShellCommandExecutor::prepare_escalated_exec()`.

### 6. Denials are coarse at the process-contract level

The deny path always exits the wrapper with `1` in [escalate_client.rs](file:///Users/bytedance/project/codex/codex-rs/shell-escalation/src/unix/escalate_client.rs#L116-L122). If downstream callers ever want richer denial semantics, the current protocol will need to grow.

## Overall assessment

This crate is a focused systems component with a clear mission: preserve shell-like `exec` behavior while letting a supervisor selectively re-home specific commands outside the current shell sandbox. Its best qualities are:

- strong separation between protocol and policy,
- careful fd ownership and socket framing,
- good tests around race-prone UNIX behavior,
- explicit lifecycle management for inheritable sockets.

The main uncertainties are not in the transport itself; they are in API semantics and future evolution:

- whether timeout handling belongs here or exclusively above it,
- how transparent escalated signal behavior needs to be,
- whether policy should stay narrow or expand to include more request context.

As written, the crate looks production-oriented and intentionally scoped, with the low-level transport and lifecycle problems handled more carefully than typical application code.
