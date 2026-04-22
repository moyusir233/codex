# CODE_ANALYSIS

## Overview

`codex-utils-pty` is a small process-spawning crate that gives the rest of the workspace one unified interface for two different execution modes:

- **PTY mode** for interactive programs such as shells and REPLs.
- **Pipe mode** for non-interactive commands where stdin/stdout/stderr should remain regular pipes.

The crate’s main value is not just spawning children. It also normalizes:

- input writing through an async `mpsc::Sender<Vec<u8>>`
- output delivery through separate stdout/stderr channels plus an optional combined broadcast stream
- exit reporting through a oneshot exit-code channel
- process termination and cleanup across Unix and Windows
- PTY resize support
- Unix-specific process-group isolation and inherited-fd preservation

At the crate root, [`src/lib.rs`](src/lib.rs) exposes a deliberately small public API that hides backend differences behind `ProcessHandle`, `SpawnedProcess`, and a handful of spawn helpers.

## Primary Responsibilities

### 1. Provide a backend-neutral process session abstraction

The crate makes PTY-backed and pipe-backed processes look similar to callers:

- both return `SpawnedProcess`
- both expose a `ProcessHandle`
- both provide stdin writes via `writer_sender()`
- both expose exit state via `exit_rx`, `has_exited()`, and `exit_code()`
- both can be force-terminated

This compatibility is the key design choice in the crate. Consumers can often toggle “interactive vs non-interactive” without changing their surrounding control flow.

### 2. Isolate spawned commands from the parent process tree

On Unix, the crate explicitly manages process groups and sessions so child cleanup is reliable:

- pipe children call `detach_from_tty()` in `pre_exec`, which prefers `setsid()` and falls back to `setpgid(0, 0)` when needed
- Linux additionally sets a parent-death signal with `prctl(PR_SET_PDEATHSIG, SIGTERM)`
- termination targets the whole process group rather than only the direct child

This matters because interactive shells often spawn grandchildren; killing only the immediate child would leak work.

### 3. Bridge blocking PTY APIs into Tokio-friendly async control flow

The PTY path depends on `portable-pty`, whose I/O is fundamentally blocking. The crate adapts that model by:

- reading PTY output in `spawn_blocking` tasks
- writing through a Tokio task that serializes writes to the PTY writer
- waiting for child exit in another blocking task

The pipe path is more naturally async because it uses `tokio::process::Command` and async pipe readers/writers.

### 4. Support Unix inherited file descriptor preservation

Both PTY and pipe backends include Unix-only spawn variants that preserve selected file descriptors across exec:

- `spawn_process_with_inherited_fds(...)` in the PTY module
- `spawn_process_no_stdin_with_inherited_fds(...)` in the pipe module

This is used for cases where the child must continue to access a caller-owned FD after exec, while still closing unrelated inherited descriptors.

### 5. Offer minimal Windows ConPTY support behind the same API

On Windows, PTY support is implemented through vendored WezTerm-derived ConPTY bindings in `src/win/*`. The crate exposes:

- `conpty_supported()` to report runtime availability
- `spawn_pty_process(...)` using a custom `ConPtySystem`
- `RawConPty` for lower-level Windows-only integration

## Public API Surface

The crate root in `src/lib.rs` re-exports the intended entry points:

- `spawn_pty_process(program, args, cwd, env, arg0, size) -> Result<SpawnedProcess>`
- `spawn_pipe_process(program, args, cwd, env, arg0) -> Result<SpawnedProcess>`
- `spawn_pipe_process_no_stdin(program, args, cwd, env, arg0) -> Result<SpawnedProcess>`
- `combine_output_receivers(stdout_rx, stderr_rx) -> broadcast::Receiver<Vec<u8>>`
- `conpty_supported() -> bool`
- `ProcessHandle`
- `SpawnedProcess`
- `TerminalSize`

Backward-compatibility aliases still exist:

- `ExecCommandSession = ProcessHandle`
- `SpawnedPty = SpawnedProcess`

### ProcessHandle

`ProcessHandle` in `src/process.rs` is the main runtime control object. It owns:

- the stdin sender
- a boxed child terminator strategy
- read/write/wait task handles
- cached exit state
- optional PTY handles needed to keep the PTY alive and resizable

Important methods:

- `writer_sender()` returns a cloneable sender for raw stdin bytes
- `has_exited()` reports whether the wait task observed process termination
- `exit_code()` returns the cached exit code if known
- `resize(TerminalSize)` resizes the PTY or errors for pipe-backed sessions
- `close_stdin()` drops the stdin sender
- `request_terminate()` kills the child but keeps helper tasks alive so output can continue draining
- `terminate()` kills the child and aborts helper tasks

`Drop` for `ProcessHandle` calls `terminate()`, so dropping the handle is an owning cleanup action, not just a passive reference release.

### SpawnedProcess

`SpawnedProcess` is the spawn result bundle:

- `session: ProcessHandle`
- `stdout_rx: mpsc::Receiver<Vec<u8>>`
- `stderr_rx: mpsc::Receiver<Vec<u8>>`
- `exit_rx: oneshot::Receiver<i32>`

This design keeps control and observation separate: callers can drive the child via `session` while independently consuming output and exit.

## Module Responsibilities

### `src/lib.rs`

Defines the public surface and re-export policy. It keeps the crate API intentionally small and hides most backend details.

### `src/process.rs`

Defines the shared process/session model:

- `TerminalSize`
- `ChildTerminator`
- `PtyMasterHandle`
- `PtyHandles`
- `ProcessHandle`
- `SpawnedProcess`
- `combine_output_receivers(...)`

This file is the abstraction boundary that lets PTY and pipe spawners share one handle type.

### `src/pipe.rs`

Implements non-interactive subprocess spawning with `tokio::process::Command`.

Key responsibilities:

- configure stdio as piped or null
- clear and repopulate the child environment
- optionally override `argv[0]` on Unix
- detach from the controlling TTY on Unix
- preserve requested inherited FDs on Unix
- spawn async tasks for stdin writing and stdout/stderr draining
- wait for exit and cache the exit code
- terminate via process-group kill on Unix or `TerminateProcess` on Windows

Notable behavior:

- stdout and stderr stay truly separate
- reader tasks use `AsyncReadExt::read` loops and forward chunks to `mpsc` channels
- no output retention happens inside the crate; buffering is only channel-based

### `src/pty.rs`

Implements PTY-backed process spawning.

There are two Unix paths:

- **portable path**: use `portable-pty` directly when no inherited FDs must be preserved
- **raw path**: use `openpty` plus manual `std::process::Command` setup when inherited FDs must be preserved

Key responsibilities:

- choose the platform PTY system
- configure child cwd/env/argv
- translate PTY master reads into stdout channel chunks
- translate stdin writes into PTY master writes
- wait for exit and cache exit state
- expose PTY resize support through `ProcessHandle`
- ensure descendant cleanup through process-group kill on Unix

Notable PTY semantics:

- PTY output is funneled into `stdout_rx`
- `stderr_rx` exists for interface compatibility but is effectively unused in PTY mode
- interactive programs therefore present merged terminal output, which matches PTY behavior

### `src/process_group.rs`

Contains Unix-focused lifecycle helpers:

- `set_parent_death_signal(...)`
- `detach_from_tty()`
- `set_process_group()`
- `kill_process_group_by_pid(...)`
- `terminate_process_group(...)`
- `kill_process_group(...)`
- `kill_child_process_group(...)`

This module is important because reliable cleanup is one of the hardest parts of subprocess management, especially for shells that create descendants.

### `src/win/mod.rs`

Defines Windows process wrappers around ConPTY-spawned children. The file is largely vendored from WezTerm, with a documented local fix for an inverted `TerminateProcess` success check.

It provides:

- `WinChild`
- `WinChildKiller`
- `ConPtySystem` re-export
- `conpty_supported()` re-export

### `src/win/conpty.rs`

Implements the custom `portable_pty::PtySystem` adapter backed by Windows ConPTY:

- `ConPtySystem`
- `ConPtyMasterPty`
- `ConPtySlavePty`
- `RawConPty`

This module is what lets the rest of the crate keep using `portable-pty` abstractions even though Windows needs custom plumbing.

### `src/win/psuedocon.rs`

Contains low-level ConPTY FFI:

- dynamic loading of ConPTY entry points from `kernel32.dll` or `conpty.dll`
- Windows build-number detection
- pseudo-console creation, resizing, and child-process spawning
- Windows command-line quoting and environment-block construction

This is the most OS-specific and maintenance-sensitive code in the crate.

### `src/win/procthreadattr.rs`

Wraps `PROC_THREAD_ATTRIBUTE_LIST` management needed to attach a pseudo-console during `CreateProcessW`.

## Main Execution Flows

### Pipe spawn flow

1. `spawn_pipe_process(...)` or `spawn_pipe_process_no_stdin(...)` calls `spawn_process_with_stdin_mode(...)`.
2. The function validates `program`, builds a `tokio::process::Command`, clears env, and adds args/env/cwd.
3. On Unix, `pre_exec`:
   - detaches from the parent TTY/session
   - sets Linux parent-death signal when available
   - closes inherited FDs except the allowlist
4. The child is spawned with piped stdout/stderr and optionally piped stdin.
5. A Tokio writer task forwards bytes from `writer_tx` to child stdin.
6. Separate Tokio reader tasks drain stdout and stderr into `mpsc` channels.
7. A wait task observes exit, stores the exit code, flips `has_exited`, and sends the oneshot exit notification.
8. `ProcessHandle` is returned together with the output and exit receivers.

### PTY spawn flow

1. `spawn_pty_process(...)` calls `spawn_process_with_inherited_fds(...)`.
2. On Unix, the function chooses between:
   - `spawn_process_portable(...)` when no extra inherited FDs are needed
   - `spawn_process_preserving_fds(...)` when explicit FD preservation is required
3. The PTY master/slave pair is created.
4. The child is spawned with its stdio attached to the PTY slave.
5. A blocking reader task reads from the PTY master and forwards chunks into `stdout_rx`.
6. A Tokio task serializes writes from `writer_tx` into the PTY writer.
7. A blocking wait task waits for child exit, caches the exit code, and completes `exit_rx`.
8. `ProcessHandle` retains PTY handles so resize remains possible and premature PTY closure does not signal the child unexpectedly.

### PTY resize flow

1. Caller invokes `session.resize(TerminalSize { rows, cols })`.
2. `ProcessHandle` looks up its retained `PtyHandles`.
3. For normal PTYs, it delegates to `MasterPty::resize`.
4. For the Unix raw-PTY path, it performs `ioctl(TIOCSWINSZ)` on the retained master fd.

### Termination flow

1. Caller chooses either `request_terminate()` or `terminate()`.
2. `request_terminate()` invokes the backend-specific `ChildTerminator`.
3. PTY termination on Unix prefers process-group kill and also attempts the child killer.
4. Pipe termination on Unix kills the stored process group id directly.
5. `terminate()` then aborts reader/writer/wait helper tasks.
6. Dropping `ProcessHandle` implicitly runs `terminate()`.

The crate therefore supports two distinct semantics:

- **drain-after-kill** with `request_terminate()`
- **hard stop including helper tasks** with `terminate()`

## Important Design Choices

### Unified interface over exact fidelity

The crate favors one common API over exposing every backend-specific nuance. The clearest example is `SpawnedProcess`, whose shape is identical for PTY and pipe modes even though PTY stderr is not meaningfully separate.

### Best-effort cleanup over graceful shutdown protocols

Unix termination paths use process-group `SIGKILL` for reliability. That is intentionally forceful and avoids descendant leakage, but it skips graceful shutdown behavior unless the caller implements that at the application protocol level before invoking termination.

### Explicit environment control

Both backends call `env_clear()` and then repopulate the child environment from the provided `HashMap`. That makes subprocess environments deterministic and avoids accidental inheritance.

### Preserve PTY handles to avoid side effects

`ProcessHandle` stores `PtyHandles` specifically so the PTY slave/master lifetimes remain valid. The code comment notes that dropping the slave can send Control+C-like effects to the child, so lifetime management is part of correctness.

### Separate readers for pipe stdout and stderr

The pipe backend drains stdout and stderr concurrently. This avoids a classic deadlock where a child fills stderr while the parent only reads stdout.

### Manual Unix PTY path for inherited-FD support

The raw Unix PTY path exists because the generic `portable-pty` path does not expose the necessary hooks for this exact fd-preservation behavior. The crate therefore accepts some extra complexity to keep this feature available.

## Dependencies and Their Roles

### Runtime and error handling

- `tokio`: async process spawning, channels, tasks, and test runtime
- `anyhow`: error propagation and context

### PTY abstraction

- `portable-pty`: cross-platform PTY traits and default backend integration

### Unix support

- `libc`: `setsid`, `setpgid`, `killpg`, `prctl`, `ioctl`, `openpty`, `fcntl`, and signal handling

### Windows support

- `winapi`: Win32 process and console APIs
- `filedescriptor`: owned Win32 handles and pipes
- `shared_library`: dynamic loading of ConPTY symbols
- `lazy_static`: global cached ConPTY symbol table
- `log`: Windows spawn error logging

### Test-only

- `pretty_assertions`: clearer diffs for byte-oriented expectations

## Testing Coverage

Tests are centralized in `src/tests.rs` and cover both behavior and platform-specific edge cases.

### Core behavior

- PTY Python REPL starts, accepts input, emits output, and exits cleanly
- pipe backend round-trips stdin to stdout
- PTY and pipe backends can be consumed through the same surrounding interface
- pipe backend drains heavy stderr output even when stdout is idle
- pipe backend can expose truly split stdout and stderr streams

### Unix lifecycle and isolation

- pipe children detach from the parent session
- PTY termination kills background children in the same process group
- pipe `terminate()` stops detached reader activity
- PTY resize updates terminal dimensions seen by the child

### Inherited-FD behavior

- PTY spawn can preserve explicit inherited FDs
- PTY preserved-FD path still keeps an interactive Python REPL alive
- PTY preserved-FD path still reports exec failures correctly
- pipe no-stdin spawn can preserve explicit inherited FDs

### Windows-specific notes

- general cross-platform tests run on Windows with `cmd.exe` and ConPTY-specific timing accommodations
- `src/win/psuedocon.rs` includes a Windows build-number unit test

The tests are strong on end-to-end process semantics. They are lighter on isolated unit coverage for individual helper functions, which is reasonable given how OS-dependent the code is.

## Observed Strengths

- **Small public surface**: easy for callers to understand and hard to misuse accidentally.
- **Consistent control model**: PTY and pipe sessions look similar from the outside.
- **Good Unix cleanup story**: process-group handling addresses a real class of leaked-child bugs.
- **Thoughtful inherited-FD support**: the crate explicitly handles a subtle exec-time requirement.
- **Practical tests**: the suite validates real process behavior instead of only mocking.
- **Documented Windows divergence**: the vendored `TerminateProcess` fix is explicitly called out.

## Tradeoffs and Constraints

- **PTY output is merged**: callers that want stderr attribution cannot get it from PTY mode.
- **Termination is forceful**: the default cleanup path optimizes for reliability rather than graceful shutdown.
- **Blocking PTY bridge**: PTY reads and waits require `spawn_blocking`, which is appropriate but more complex than the pipe path.
- **Unix FD closing uses `/dev/fd` scanning**: practical on modern Unix systems, but not perfectly universal.
- **Windows code is partly vendored**: upstream drift in WezTerm-derived code can create maintenance risk.
- **No built-in output retention policy here**: downstream crates must decide how much output history to keep.

## Open Questions

1. Should PTY sessions expose a clearly documented “merged output” API instead of returning an always-empty `stderr_rx` for interface symmetry?
2. Should the crate expose a graceful termination method that sends `SIGTERM` on Unix before falling back to `SIGKILL`, especially since `terminate_process_group(...)` already exists?
3. Is `DEFAULT_OUTPUT_BYTES_CAP` still the right home for this crate? It is exported from `src/lib.rs` but not used internally here.
4. Should `combine_output_receivers(...)` document that source identity is lost and chunk ordering becomes scheduler-dependent once stdout/stderr are merged?
5. Should `close_inherited_fds_except(...)` have stronger comments or tests around macOS and other non-Linux Unix variants where `/dev/fd` behavior may differ?
6. Should the vendored Windows ConPTY code be refreshed periodically or wrapped with a smaller local abstraction to reduce upstream-drift risk?

## Bottom Line

`codex-utils-pty` is a focused subprocess-control crate with one strong idea: present PTY and pipe execution through nearly the same Rust interface while still doing the OS-specific work needed for correct cleanup, interactive behavior, and fd handling.

Its most important internal qualities are:

- disciplined lifecycle management
- pragmatic cross-platform abstraction
- explicit Unix process-group handling
- end-to-end tests that exercise the real operating-system behavior

For consumers that need a clean “spawn, write, read, resize, terminate” layer without re-implementing PTY edge cases, the crate is well-structured and purposeful.
