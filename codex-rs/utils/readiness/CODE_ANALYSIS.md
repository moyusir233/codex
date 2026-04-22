# codex-utils-readiness analysis

## Scope

- Crate path: [utils/readiness](file:///Users/bytedance/project/codex/codex-rs/utils/readiness)
- Primary implementation: [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs)
- Package metadata: [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/Cargo.toml), [BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/BUILD.bazel)
- Internal shape is intentionally tiny: one source file contains the public types, the private error module, and all tests.

## Responsibilities

- Provide a small async-compatible readiness gate that transitions only once from “not ready” to “ready”, implemented by [ReadinessFlag](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L43-L184).
- Require explicit authorization before a producer can flip the gate by issuing opaque subscription tokens through [subscribe](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L116-L140) and consuming them in [mark_ready](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L142-L166).
- Let consumers cheaply poll or await readiness via [is_ready](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L97-L114) and [wait_ready](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L168-L183).
- Avoid indefinite mutex stalls by wrapping token-set access in [with_tokens](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L66-L74), which times out after [LOCK_TIMEOUT](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L18-L18).
- Model an “optional blocker” rather than a strict countdown latch: if nobody has subscribed, [is_ready](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L97-L114) promotes the gate to ready automatically.

## Module layout

- [src/lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs) contains everything:
  - [Token](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L14-L16)
  - [Readiness](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L20-L41)
  - [ReadinessFlag](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L43-L184)
  - private [errors](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L186-L196)
  - inline [tests](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L198-L333)
- [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/Cargo.toml#L1-L18) declares a minimal dependency set and no feature flags.
- [BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/BUILD.bazel#L1-L6) only registers the crate for Bazel and adds no crate-specific build behavior.

## Public API

### `Token`

- [Token](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L14-L16) is an opaque `Copy`/`Hash` wrapper around `i32`.
- The tuple field is private, so external crates can store and pass tokens back but cannot forge arbitrary tokens directly.
- Token generation starts from `1` and skips `0`, enforced in [new](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L56-L64) and [subscribe](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L124-L139).

### `Readiness`

- [Readiness](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L20-L41) defines the crate’s behavioral contract:
  - [is_ready](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L25-L25): cheap readiness check
  - [subscribe](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L30-L30): reserve authorization to mark ready
  - [mark_ready](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L37-L37): consume a token and attempt the one-way transition
  - [wait_ready](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L40-L40): async block until ready
- The trait currently has a single implementation, [impl Readiness for ReadinessFlag](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L95-L184), and the workspace does not use `dyn Readiness`.

### `ReadinessFlag`

- [ReadinessFlag](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L43-L52) combines:
  - `AtomicBool ready` for lock-free ready reads
  - `AtomicI32 next_id` for token issuance
  - `Mutex<HashSet<Token>>` for active-authorizer tracking
  - `watch::Sender<bool>` for waking async waiters
- [new](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L54-L64) initializes a false watch channel and an empty token set.
- [Default](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L81-L85) delegates to `new`.
- [Debug](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L87-L93) intentionally exposes only the ready bit, not token contents.

### `ReadinessError`

- [ReadinessError](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L186-L196) has only two cases:
  - `TokenLockFailed`
  - `FlagAlreadyReady`
- It is referenced from public trait signatures, but it lives inside a private `errors` module. That means it is part of the semantic API surface, yet awkward to name from another crate.

## Control flow and runtime behavior

### Construction and idle behavior

- A fresh [ReadinessFlag::new](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L54-L64) starts with `ready = false`, an empty token set, and `watch` initialized to `false`.
- If no producer ever subscribes, the first successful call to [is_ready](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L97-L114) or [wait_ready](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L168-L183) will observe the empty token set and permanently flip the gate to ready.
- This design makes the gate safe to attach to optional background work: absence of a producer never leaves waiters stuck forever.

### Subscription path

1. [subscribe](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L116-L140) first checks the atomic ready bit.
2. It then calls [with_tokens](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L66-L74), which acquires `tokens` with a one-second timeout.
3. While holding the mutex, it re-checks readiness to close the race with concurrent [mark_ready](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L142-L166).
4. It generates candidate IDs with `fetch_add`, skipping `0` and retrying until insertion into the `HashSet` succeeds.
5. It returns `FlagAlreadyReady` if readiness flipped before insertion, or `TokenLockFailed` if the mutex could not be acquired in time.

### Mark-ready path

1. [mark_ready](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L142-L166) fast-rejects if the gate is already ready or the token is zero.
2. Inside [with_tokens](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L66-L74), it removes the token from the active set.
3. Unknown or already-consumed tokens return `Ok(false)`.
4. A valid token stores `true` into the atomic ready bit, clears all remaining tokens, and returns `Ok(true)`.
5. After the lock scope ends, it performs a best-effort `watch` broadcast so pending waiters wake up.

### Wait path

1. [wait_ready](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L168-L183) first calls [is_ready](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L97-L114).
2. If ready is already true, or if `is_ready` can auto-promote the empty-token case, it returns immediately.
3. Otherwise it subscribes to the `watch` channel and checks the current value through `borrow()`.
4. It then loops on `changed().await` until a `true` value appears.

### Important semantic detail: `is_ready` is not a pure getter

- [is_ready](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L97-L114) does more than inspect state.
- If the atomic says “not ready” and `tokens.try_lock()` succeeds with an empty set, it:
  - swaps the atomic to `true`
  - drops the lock
  - broadcasts readiness
- This side effect is central to the crate’s behavior. The name suggests a pure query, but the method can commit the one-way transition.

## Concrete downstream usage

- Each turn context in `codex-core` gets its own gate through [TurnContext::tool_call_gate](file:///Users/bytedance/project/codex/codex-rs/core/src/session/turn_context.rs#L67-L67) and fresh initialization sites such as [turn_context.rs:L193-L193](file:///Users/bytedance/project/codex/codex-rs/core/src/session/turn_context.rs#L193-L193) and [review.rs:L135-L135](file:///Users/bytedance/project/codex/codex-rs/core/src/session/review.rs#L135-L135).
- When ghost snapshots are enabled, the session subscribes for authorization in [maybe_start_ghost_snapshot](file:///Users/bytedance/project/codex/codex-rs/core/src/session/mod.rs#L2803-L2828) before spawning the background task.
- The background snapshot task releases the gate by calling [mark_ready](file:///Users/bytedance/project/codex/codex-rs/core/src/tasks/ghost_snapshot.rs#L151-L155) when the task completes or is cancelled.
- Mutating tool execution waits on the gate in [registry.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/tools/registry.rs#L360-L365), so tool calls do not race ahead of ghost snapshot preparation.
- If ghost snapshots are disabled or subscription fails, the lack of active tokens means [wait_ready](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L168-L183) returns immediately through the empty-token auto-ready path.

## Dependencies

- [async-trait](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/Cargo.toml#L7-L12) enables async functions in the [Readiness](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L20-L41) trait.
- [tokio](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/Cargo.toml#L7-L15) supplies:
  - [tokio::sync::Mutex](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L10-L10) for token-set coordination
  - [tokio::sync::watch](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L11-L11) for waiter notification
  - [tokio::time::timeout](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L70-L72) for bounded lock acquisition
- [thiserror](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/Cargo.toml#L7-L12) derives the small error enum in [errors](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L186-L196).
- [assert_matches](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/Cargo.toml#L13-L16) supports precise test assertions in [tests](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L198-L333).
- [time](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/Cargo.toml#L7-L12) is listed as a runtime dependency, but [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs) only uses `std::time::Duration` and `tokio::time`; the external `time` crate does not appear in this implementation.

## Testing

- All tests are inline in [tests](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L198-L333); there are no separate integration tests.
- Positive-path coverage:
  - [subscribe_and_mark_ready_roundtrip](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L209-L217)
  - [wait_ready_unblocks_after_mark_ready](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L238-L253)
  - [mark_ready_twice_uses_single_token](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L255-L263)
- Auto-ready and invalid-token coverage:
  - [mark_ready_rejects_unknown_token](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L229-L236)
  - [is_ready_without_subscribers_marks_flag_ready](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L265-L276)
- Contention and edge-case coverage:
  - [subscribe_returns_error_when_lock_is_held](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L278-L310)
  - [subscribe_skips_zero_token](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L312-L321)
  - [subscribe_avoids_duplicate_tokens](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L323-L332)
- Validation result: `cargo test -p codex-utils-readiness` passed locally on 2026-04-22 with 9 unit tests passing and 0 doctests in this crate.

## Design assessment

- The crate is intentionally narrow and does one job well: coordinate a one-time readiness transition between one or more producers and many waiters with minimal API surface.
- Using an opaque [Token](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L14-L16) is a good fit for Rust. It prevents accidental readiness changes by unrelated callers while keeping the authorization mechanism lightweight and copyable.
- The combination of atomics for the hot path and a mutex-protected `HashSet` for authorization state keeps the common read path cheap without overcomplicating the implementation.
- The most unusual choice is the side-effecting [is_ready](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L97-L114). It is practical for the main `codex-core` use case because an unsubscribed gate should not block tool calls, but it makes the method name slightly misleading.
- [mark_ready](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L142-L166) clears the entire token set after the first successful authorizer. That means the crate models “first authorized producer wins” rather than “all producers must report done”.
- The one-second timeout in [with_tokens](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L66-L74) avoids silent hangs, but it also turns transient lock contention into an observable operational failure.
- The trait abstraction is currently light-weight to the point of being debatable: the workspace imports [Readiness](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L20-L41) for method availability, but there is only one implementation and no trait-object use.

## Constraints and assumptions

- The gate is one-way only; there is no reset API and the docs explicitly state that `true` is irreversible in [Readiness::is_ready](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L22-L25).
- The implementation assumes duplicate-token risk is low but still handles `i32` wraparound by retrying until insertion succeeds in [subscribe](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L130-L135).
- The crate assumes Tokio is available because both waiting and lock timeouts rely on Tokio synchronization primitives.
- The implementation deliberately accepts “best effort” broadcasting: [mark_ready](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L163-L165) ignores `watch::Sender::send` failure when no receivers exist.

## Open questions

1. Should [is_ready](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L97-L114) be renamed or split into separate query and auto-promote operations? Its current behavior is correct for the main use case but surprising for callers who expect a pure check.
2. Should [ReadinessError](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L186-L196) be re-exported from the crate root, or the `errors` module made public, so downstream crates can name and match the error type directly?
3. Is the one-second [LOCK_TIMEOUT](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L18-L18) appropriate under heavy executor load, or should lock acquisition be unbounded and instrumented instead?
4. Does the workspace need a stricter barrier variant where all subscribers must finish, rather than the current “any one valid token marks ready and clears the rest” behavior?
5. Is [async-trait](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/Cargo.toml#L7-L12) still justified now that the trait has one implementation and no dynamic dispatch usage?
6. Should [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/Cargo.toml#L7-L12) remove the apparently unused `time` dependency?

## Representative snippet

The core semantic quirk of the crate is concentrated in `is_ready`:

```rust
fn is_ready(&self) -> bool {
    if self.load_ready() {
        return true;
    }

    if let Ok(tokens) = self.tokens.try_lock()
        && tokens.is_empty()
    {
        let was_ready = self.ready.swap(true, Ordering::AcqRel);
        drop(tokens);
        if !was_ready {
            let _ = self.tx.send(true);
        }
        return true;
    }

    self.load_ready()
}
```

- Source: [lib.rs:L97-L114](file:///Users/bytedance/project/codex/codex-rs/utils/readiness/src/lib.rs#L97-L114)
- This explains why the crate behaves like an optional gate rather than a mandatory barrier: an unsubscribed flag self-resolves to ready as soon as someone asks.
