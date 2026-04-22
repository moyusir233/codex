# codex-utils-cache analysis

## Scope

- Crate path: [utils/cache](file:///Users/bytedance/project/codex/codex-rs/utils/cache)
- Primary source: [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs)
- Build metadata: [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/utils/cache/Cargo.toml), [BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/utils/cache/BUILD.bazel)
- Public surface area is intentionally small: one cache type, one digest helper, and inline unit tests.

## Responsibilities

- Provide a tiny shared LRU cache wrapper for synchronous-style call sites that already run inside Tokio, implemented by [BlockingLruCache](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L13-L120).
- Hide the underlying [lru::LruCache](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L5-L5) behind a [tokio::sync::Mutex](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L8-L9) and expose blocking, non-async methods such as [get](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L72-L81), [insert](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L83-L87), and [get_or_insert_with](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L29-L44).
- Make caching optional at runtime: every cache operation goes through [lock_if_runtime](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L122-L128), and if no Tokio runtime is active the crate degrades to a no-op cache instead of persisting entries.
- Provide a small content-addressing utility, [sha1_digest](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L130-L141), for callers that want stable cache keys derived from bytes rather than paths or object identity.

## Module layout

- [src/lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs) contains the entire implementation and all tests.
- [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/utils/cache/Cargo.toml#L1-L16) declares only three runtime dependencies and no feature flags specific to this crate.
- [BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/utils/cache/BUILD.bazel#L1-L6) registers the crate for Bazel builds but adds no crate-specific build logic.

## Public API

### `BlockingLruCache<K, V>`

- [new](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L21-L27) constructs a bounded LRU cache and requires `NonZeroUsize`, pushing zero-capacity handling to the caller.
- [try_with_capacity](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L66-L70) is a convenience constructor that returns `None` for zero, which fits optional-cache configuration flows.
- [get](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L72-L81), [insert](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L83-L87), [remove](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L89-L97), and [clear](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L99-L104) mirror standard map operations, but each silently becomes a no-op outside Tokio.
- [get_or_insert_with](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L29-L44) and [get_or_try_insert_with](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L46-L64) implement memoization and error-aware memoization; both require `V: Clone` because the cache retains ownership and returns a cloned value.
- [with_mut](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L106-L114) exposes the raw `LruCache` for custom mutations. Outside Tokio, it passes a temporary unbounded cache, so mutations are visible only inside the callback.
- [blocking_lock](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L116-L119) exposes the inner `MutexGuard` directly for advanced callers that need APIs not wrapped by this crate.

### `sha1_digest`

- [sha1_digest](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L130-L141) returns a fixed `[u8; 20]` digest, which is ergonomic for `Hash`, `Eq`, and stack allocation.
- The helper is intentionally narrow: it hashes raw bytes and leaves encoding, salting, and string formatting to the caller.

## Control flow and runtime behavior

### Cache operation path

- Every mutating or reading operation calls [lock_if_runtime](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L122-L128).
- `lock_if_runtime` first checks `tokio::runtime::Handle::try_current()`. If there is no current runtime, it returns `None`.
- If a runtime exists, it enters [tokio::task::block_in_place](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L127-L127) and then uses `Mutex::blocking_lock()` to obtain the guard.
- Callers branch on `Option<MutexGuard<...>>`:
  - cache hits return cloned entries;
  - cache misses compute values and insert them when a runtime is present;
  - outside Tokio, reads miss and writes are dropped;
  - `with_mut` uses a temporary local cache when disabled.

### Memoization path

- [get_or_insert_with](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L29-L44) locks the cache, checks for an existing key via `guard.get(&key)`, and updates LRU recency on hits because `lru::LruCache::get` is recency-aware.
- On misses, it executes the value factory while still holding the mutex and then inserts via `guard.put(key, v.clone())`.
- [get_or_try_insert_with](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L46-L64) follows the same path, but propagates factory errors and skips insertion when the factory fails.

### No-runtime behavior

- The crate deliberately favors “best effort caching” over failure: outside Tokio, [insert](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L83-L87) returns `None`, [get](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L72-L81) always misses, and [clear](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L99-L104) does nothing.
- This makes the type usable from code paths that may run in tests, setup code, or sync contexts without forcing a runtime bootstrap.
- The tradeoff is that caching failures are silent. Callers cannot distinguish “not found” from “cache unavailable” without checking their runtime assumptions separately.

## Concrete downstream usage

- [utils/image/src/lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/image/src/lib.rs#L52-L118) uses `BlockingLruCache<ImageCacheKey, EncodedImage>` plus [sha1_digest](file:///Users/bytedance/project/codex/codex-rs/utils/image/src/lib.rs#L62-L67) to memoize processed image payloads by file content and image mode.
- [core/src/context_manager/history.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/context_manager/history.rs#L524-L529) uses `BlockingLruCache<[u8; 20], Option<i64>>` and [sha1_digest](file:///Users/bytedance/project/codex/codex-rs/core/src/context_manager/history.rs#L593-L610) to cache expensive byte-estimation work for inline image data URLs.
- Those two call sites show the intended niche clearly:
  - small process-wide caches;
  - content-derived keys;
  - synchronous helper functions that still execute within Tokio-driven application flows.

## Dependencies

- [lru = 0.16.3](file:///Users/bytedance/project/codex/codex-rs/Cargo.toml#L268-L268) provides the actual bounded LRU implementation with recency tracking and eviction.
- [sha1 = 0.10.6](file:///Users/bytedance/project/codex/codex-rs/Cargo.toml#L314-L314) provides the digest implementation used by [sha1_digest](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L135-L141).
- [tokio = 1](file:///Users/bytedance/project/codex/codex-rs/Cargo.toml#L341-L341) provides:
  - `tokio::sync::Mutex` for async-runtime-aware mutual exclusion;
  - `blocking_lock()` for synchronous access;
  - `Handle::try_current()` for runtime detection;
  - `block_in_place()` to avoid blocking the runtime worker directly.
- [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/utils/cache/Cargo.toml#L10-L16) enables Tokio features `sync`, `rt`, and `rt-multi-thread` for the library, and adds `macros` in tests for `#[tokio::test]`.

## Testing

- [stores_and_retrieves_values](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L149-L156) validates the basic insert/get path inside a multi-thread Tokio runtime.
- [evicts_least_recently_used](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L158-L170) validates that `get` refreshes recency and that later inserts evict the true LRU entry.
- [disabled_without_runtime](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L172-L192) validates the no-runtime contract, including the ephemeral behavior of [with_mut](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L106-L114) and the `None` return from [blocking_lock](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L116-L119).
- `cargo test -p codex-utils-cache` passes locally on 2026-04-22 with 3 unit tests and 0 doctests failing.

## Design assessment

- The crate is intentionally minimal and easy to adopt. It avoids async signatures, exposes only essential operations, and leaves policy such as cache sizing and key design to callers.
- The API is optimized for cheap-to-clone values. Requiring `V: Clone` on read-through methods keeps ownership simple, but it makes the cache less attractive for large or non-cloneable payloads unless callers wrap values in `Arc`.
- The implementation serializes the value factory under the mutex in [get_or_insert_with](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L34-L41) and [get_or_try_insert_with](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L55-L61). This prevents duplicate work for the same key, but it also blocks unrelated keys while expensive factories run.
- The no-runtime fallback is pragmatic for library ergonomics, but it is semantically unusual: the same API sometimes behaves like a real cache and sometimes like a pass-through computation layer.
- [with_mut](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L106-L114) is the sharpest edge in the API because its fallback path uses a temporary unbounded cache. That is useful for callback logic reuse, but it can surprise callers who assume mutations persist or respect configured capacity.

## Design constraints and assumptions

- `K: Eq + Hash` is required at the type level because the underlying LRU uses hash-based lookup.
- `Borrow<Q>` support in [get](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L73-L80) and [remove](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L90-L96) allows ergonomic lookups such as `String` keys queried with `&str`.
- The crate assumes callers are primarily on Tokio multi-thread runtimes, which matches the test setup and the use of `block_in_place`.
- The crate assumes caches are small and contention is limited; there is no sharding, per-key lock, or async-aware read-through coordination beyond the single mutex.

## Open questions

- Should [lock_if_runtime](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L122-L128) explicitly handle Tokio current-thread runtimes? `block_in_place` is only valid on the multi-thread runtime, so the current “runtime exists” check is broader than the implementation guarantee.
- Should the crate expose whether caching is disabled, so callers can tell the difference between a genuine miss and “no runtime available”?
- Should `with_mut` preserve configured capacity in its fallback path instead of using `LruCache::unbounded()`?
- Should read-through methods release the mutex before computing expensive values, possibly with a second check or a more advanced single-flight mechanism?
- Should the crate add tests for `sha1_digest`, `get_or_try_insert_with`, `try_with_capacity`, and direct [blocking_lock](file:///Users/bytedance/project/codex/codex-rs/utils/cache/src/lib.rs#L116-L119) behavior inside Tokio?

## Representative snippet

The core runtime gate is small but defines the crate’s semantics:

```rust
fn lock_if_runtime<K, V>(m: &Mutex<LruCache<K, V>>) -> Option<MutexGuard<'_, LruCache<K, V>>>
where
    K: Eq + Hash,
{
    tokio::runtime::Handle::try_current().ok()?;
    Some(tokio::task::block_in_place(|| m.blocking_lock()))
}
```

- This single helper explains why the crate is synchronous to call, Tokio-dependent for actual caching, and a no-op outside a runtime.
