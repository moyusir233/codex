# codex-thread-store analysis

## Scope

`codex-thread-store` is a storage-boundary crate for persisted Codex conversation threads. It defines a backend-neutral trait surface, a shared domain model, a local filesystem/SQLite-backed implementation, and an early remote gRPC-backed implementation.

At the moment, the crate is only partially implemented:

- The trait surface is broad and models the full intended persistence lifecycle in [store.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/store.rs#L19-L67) and [recorder.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/recorder.rs#L7-L28).
- The local backend currently implements read/list/update-metadata/archive/unarchive in [local/mod.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/mod.rs#L30-L121).
- The remote backend currently implements only `list_threads` over gRPC in [remote/mod.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/remote/mod.rs#L26-L111) and [remote/list_threads.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/remote/list_threads.rs#L12-L55).

This makes the crate feel like an extraction layer in progress: the domain model is already centralized, but backend parity is not there yet.

## Crate surface

### Public exports

[lib.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/lib.rs#L7-L36) flattens the crate into a consumer-friendly API:

- Errors: `ThreadStoreError`, `ThreadStoreResult`
- Backends: `LocalThreadStore`, `RemoteThreadStore`
- Traits: `ThreadStore`, `ThreadRecorder`
- Request/response/domain types: `CreateThreadParams`, `ListThreadsParams`, `StoredThread`, `ThreadPage`, `ThreadMetadataPatch`, and related enums

This is a clean crate boundary: downstream code does not need to know the internal `local` or `remote` module layout.

### Core abstraction

The main trait in [store.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/store.rs#L19-L67) defines the intended thread persistence contract:

- create or resume a live recorder
- append items outside the recorder path
- load replay history
- read and list thread summaries
- mutate metadata
- archive and unarchive threads

The separate `ThreadRecorder` trait in [recorder.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/recorder.rs#L7-L28) preserves the idea that persistence may be lazy, buffered, and explicitly flushed or shut down.

One notable design choice is `fn as_any(&self) -> &dyn Any`, which allows API-boundary code to downcast when an operation only makes sense for a concrete implementation. That is pragmatic, but it also signals that some callers still need backend-specific escape hatches.

## Domain model

The request/response layer in [types.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/types.rs#L19-L238) is the center of the crate.

### Most important types

- `StoredThread` in [types.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/types.rs#L146-L195) is the canonical read/list result. It combines identity, summary text, timestamps, execution context, git metadata, approval/sandbox settings, and optional history.
- `StoredThreadHistory` in [types.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/types.rs#L75-L82) is a storage-neutral replay payload of `RolloutItem`s.
- `ListThreadsParams` in [types.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/types.rs#L115-L135) models pagination, sort, source filters, provider filters, archived-vs-active selection, and text search.
- `ThreadMetadataPatch` in [types.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/types.rs#L211-L230) exposes three mutable areas: `name`, `memory_mode`, and `git_info`.

### Modeling observations

- `StoredThread.rollout_path` is optional, which cleanly separates local and remote stores.
- `history` is optional and attached on demand, so read/list can stay lightweight.
- `ThreadEventPersistenceMode` anticipates different replay surfaces for CLI-style replay versus richer app-server reconstruction in [types.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/types.rs#L19-L27).
- `OptionalStringPatch = Option<Option<String>>` is a conventional Rust patch encoding: omitted means unchanged, `Some(None)` means clear, `Some(Some(v))` means replace.

The model is richer than what the current implementations use, which again reinforces that the crate is ahead of its backend coverage.

## Error model

[error.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/error.rs#L3-L36) defines four buckets:

- `ThreadNotFound`
- `InvalidRequest`
- `Conflict`
- `Internal`

In practice, the local code mostly returns `InvalidRequest` and `Internal`, even for missing threads. For example, `read_thread` and `archive_thread` commonly report `"no rollout found for thread id ..."` as `InvalidRequest` rather than `ThreadNotFound`; see [read_thread.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/read_thread.rs#L38-L46) and [archive_thread.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/archive_thread.rs#L15-L24).

That means the enum is slightly more expressive than the actual behavior today.

## Local backend

### Responsibility

`LocalThreadStore` in [local/mod.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/mod.rs#L30-L121) is the real implementation in this crate slice. It sits on top of:

- rollout files managed by `codex-rollout`
- SQLite metadata maintained via `codex-state`

It is not a pure filesystem adapter. The read path prefers richer SQLite metadata when available, but still falls back to rollout-file parsing when necessary.

### Implemented operations

- `read_thread`
- `list_threads`
- `update_thread_metadata`
- `archive_thread`
- `unarchive_thread`

### Unimplemented operations

- `create_thread`
- `resume_thread_recorder`
- `append_items`
- `load_history`

All four currently return `"local thread store does not implement ... in this slice"` via [local/mod.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/mod.rs#L65-L88) and [local/mod.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/mod.rs#L117-L120).

That is a meaningful limitation because the trait surface suggests this crate should eventually own the live recorder path too, but it currently only owns read-side and maintenance operations.

## Local read flow

The core read path lives in [read_thread.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/read_thread.rs#L25-L327).

### High-level flow

`read_thread(...)` in [read_thread.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/read_thread.rs#L25-L47) does this:

1. Try to read thread metadata from SQLite via `StateRuntime`.
2. If SQLite has a record and it matches the archived filter, build `StoredThread` from SQLite.
3. Otherwise resolve the rollout path from active sessions, or archived sessions if allowed.
4. Read the rollout summary from the file.
5. Optionally load full history from the rollout file.

### Important details

- SQLite-first behavior: [read_thread.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/read_thread.rs#L29-L35) prefers database metadata because it can hold richer values like title, model, reasoning effort, agent path, and archive state.
- Active-before-archived lookup: [read_thread.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/read_thread.rs#L98-L126) chooses an active rollout first when `include_archived` is true, which avoids accidentally surfacing an outdated archived copy.
- History loading is file-based even for SQLite-backed summaries: [read_thread.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/read_thread.rs#L80-L96) and [read_thread.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/read_thread.rs#L158-L167) require a rollout path and use `RolloutRecorder::load_rollout_items`.

### Fallback layers

There are effectively three summary sources:

- SQLite metadata via `stored_thread_from_sqlite_metadata` in [read_thread.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/read_thread.rs#L182-L233)
- Rollout-derived `ThreadItem` via `stored_thread_from_rollout_item` in [helpers.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/helpers.rs#L87-L134)
- Raw `session_meta` fallback via `stored_thread_from_session_meta` and `stored_thread_from_meta_line` in [read_thread.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/read_thread.rs#L235-L296)

That layered design is robust: it can still return a thread even if the richer derived summary is absent, as long as the underlying rollout file exists.

### Summary construction behavior

Some specific behaviors are worth calling out:

- Thread title precedence: SQLite title wins when it is distinct from the first user message; otherwise the legacy title index is consulted in [read_thread.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/read_thread.rs#L186-L192) and [read_thread.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/read_thread.rs#L298-L312).
- Session source parsing is defensive: [read_thread.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/read_thread.rs#L314-L327) accepts both structured JSON and bare strings.
- Rollout-path reads canonicalize relative paths against `codex_home` in [read_thread.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/read_thread.rs#L66-L78).

## Local list flow

[list_threads.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/list_threads.rs#L14-L101) is intentionally thin.

### Flow

1. Parse the opaque cursor with `codex_rollout::parse_cursor`.
2. Map crate enums to `codex_rollout` enums.
3. Delegate to `RolloutRecorder::list_threads` or `list_archived_threads`.
4. Convert rollout `ThreadItem`s into `StoredThread`s.
5. Serialize the next cursor back into a string.

### Key observations

- The crate treats cursor shape as implementation-defined and opaque, which is good API hygiene.
- The list path is mostly a façade over `codex-rollout`; thread-store adds domain conversion and error mapping, not new indexing logic.
- Missing model providers are normalized to the default provider id in [helpers.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/helpers.rs#L112-L116).

This keeps listing logic centralized in rollout/state code while giving app-server-facing code a backend-neutral result shape.

## Local metadata updates

The mutable update path in [update_thread_metadata.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/update_thread_metadata.rs#L28-L180) is intentionally narrow.

### What works

- set thread name
- set thread memory mode

### What does not work

- git metadata updates are explicitly unsupported in this slice in [update_thread_metadata.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/update_thread_metadata.rs#L32-L37)
- patches that modify both `name` and `memory_mode` at once are rejected in [update_thread_metadata.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/update_thread_metadata.rs#L38-L43)

### How the update is applied

- Name updates append a `ThreadNameUpdated` event to the rollout file and also update the legacy thread-name index in [update_thread_metadata.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/update_thread_metadata.rs#L79-L100).
- Memory-mode updates reread the latest `session_meta`, modify it, and append a new `SessionMeta` line in [update_thread_metadata.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/update_thread_metadata.rs#L102-L128).
- After either change, the code reconciles the rollout back into SQLite via `codex_rollout::state_db::reconcile_rollout` in [update_thread_metadata.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/update_thread_metadata.rs#L56-L76).

This is a smart design for consistency: the rollout remains the append-only source of truth, and SQLite is repaired or refreshed from it rather than mutated independently.

## Local archive and unarchive flow

The archive operations live in [archive_thread.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/archive_thread.rs#L11-L57) and [unarchive_thread.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/unarchive_thread.rs#L15-L99).

### Archive

`archive_thread(...)`:

- resolves the active rollout by thread id
- canonicalizes and validates that the file is actually under the active `sessions` tree
- validates that the filename suffix matches the thread id
- renames the file into `archived_sessions`
- marks the SQLite record archived when a state DB is available

The path safety helpers are in [helpers.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/helpers.rs#L22-L80). They prevent callers from moving arbitrary files by smuggling in unrelated paths.

### Unarchive

`unarchive_thread(...)`:

- resolves the archived rollout by thread id
- validates that it is inside the archived tree
- extracts year/month/day from the filename timestamp
- recreates the dated sessions directory
- renames the file back into active sessions
- touches the file mtime so `updated_at` reflects the unarchive action
- clears archive state in SQLite when present
- rereads the rollout and returns the new summary

The mtime bump in [helpers.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/helpers.rs#L82-L85) and [unarchive_thread.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/unarchive_thread.rs#L70-L72) is a subtle but useful choice: it lets listing and summary timestamps surface the restoration event without rewriting historical content.

## Remote backend

`RemoteThreadStore` in [remote/mod.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/remote/mod.rs#L26-L111) is an early gRPC client wrapper.

### Current scope

- Implemented: `list_threads`
- Stubbed: every other trait method

### Wire format

The protobuf schema in [codex.thread_store.v1.proto](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/remote/proto/codex.thread_store.v1.proto#L1-L86) currently exposes only the `ListThreads` RPC and a wire representation of `StoredThread`.

The generated Rust file is checked in at [codex.thread_store.v1.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/remote/proto/codex.thread_store.v1.rs#L1-L460), and the repo explicitly avoids build-time code generation per [AGENTS.md](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/remote/AGENTS.md#L1-L13).

### Conversion layer

[remote/helpers.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/remote/helpers.rs#L22-L255) does the main translation work:

- map tonic status codes into `ThreadStoreError`
- map local enums into proto enums
- rebuild `SessionSource`, including sub-agent variants
- convert proto `StoredThread` into domain `StoredThread`

The remote mapping deliberately sets:

- `rollout_path: None`
- default approval mode and sandbox policy
- `history: None`
- `token_usage: None`

That is consistent with the current proto surface, but it also means remote reads are not yet full-fidelity relative to local reads.

### Important mismatch

`ListThreadsParams` has both `sort_key` and `sort_direction`, but the remote request only sends `sort_key`; see [remote/list_threads.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/remote/list_threads.rs#L16-L35) and [codex.thread_store.v1.proto](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/remote/proto/codex.thread_store.v1.proto#L9-L17).

As a result, `sort_direction` is silently ignored by the remote backend today.

## Dependencies and what they mean

`Cargo.toml` in [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/thread-store/Cargo.toml#L18-L39) reveals the architectural center of gravity.

### Core runtime dependencies

- `codex-rollout`: local rollout listing, history loading, path lookups, archive/unarchive helpers, state-db reconciliation
- `codex-state`: SQLite metadata runtime and thread metadata storage
- `codex-protocol`: `ThreadId`, `RolloutItem`, session source, sandbox, approval, git, and related domain types
- `tonic`, `prost`, `tonic-prost`: remote gRPC transport and generated types
- `chrono`, `serde`, `serde_json`, `thiserror`, `async-trait`: domain serialization and async trait ergonomics

### Architectural implication

This crate does not own persistence primitives itself. It composes:

- `codex-rollout` as the append-only file/history layer
- `codex-state` as the indexed metadata layer
- `codex-protocol` as the cross-crate domain vocabulary

So `thread-store` is mainly the boundary and orchestration layer, not the storage engine.

## Upstream integration

The crate is already wired into higher-level code.

### Core session layer

`LocalThreadStore` is constructed inside session services in [session.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/session/session.rs#L681-L684) and stored on `SessionServices` in [service.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/state/service.rs#L34-L68).

This indicates local thread storage is part of normal core session runtime state.

### App-server layer

The app-server message processor uses thread-store for:

- reading threads
- listing threads
- updating names and memory mode
- archiving and unarchiving

Those calls appear throughout [codex_message_processor.rs](file:///Users/bytedance/project/codex/codex-rs/app-server/src/codex_message_processor.rs#L2904-L2976) and [codex_message_processor.rs](file:///Users/bytedance/project/codex/codex-rs/app-server/src/codex_message_processor.rs#L3116-L3188), with additional list/read handling in [codex_message_processor.rs](file:///Users/bytedance/project/codex/codex-rs/app-server/src/codex_message_processor.rs#L4021-L4027) and [codex_message_processor.rs](file:///Users/bytedance/project/codex/codex-rs/app-server/src/codex_message_processor.rs#L5328-L5334).

That makes this crate an app-server-facing abstraction even though it lives in the core Rust workspace.

## Testing

The crate has meaningful unit-style integration tests co-located with each module.

### Local backend coverage

- Listing behavior in [local/list_threads.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/list_threads.rs#L103-L340)
  - default provider fallback
  - SQLite title-search preservation
  - active vs archived collections
  - invalid cursor handling
- Read behavior in [local/read_thread.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/read_thread.rs#L335-L882)
  - active vs archived preference
  - include-history behavior
  - fork ancestry
  - SQLite title and metadata precedence
  - session-meta fallback when richer summary data is absent
  - archived SQLite fallback behavior
- Metadata updates in [local/update_thread_metadata.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/update_thread_metadata.rs#L182-L397)
  - append-only write behavior
  - legacy name index updates
  - session-meta id mismatch protection
  - multi-field patch rejection
  - archive-state preservation during SQLite reconciliation
- Archive/unarchive behavior in [local/archive_thread.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/archive_thread.rs#L59-L170) and [local/unarchive_thread.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/unarchive_thread.rs#L101-L198)
  - filesystem moves
  - archived listing/read visibility
  - SQLite archive flag synchronization

### Remote backend coverage

[remote/list_threads.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/remote/list_threads.rs#L57-L244) spins up a real tonic server in tests and validates:

- request serialization
- response mapping
- `StoredThread` proto/domain round-tripping

That is a good level of testing for a transport adapter.

### Test helpers

[test_support.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/local/test_support.rs#L10-L107) is simple but effective: it creates realistic session-meta and user-message rollout files without needing the full runtime.

## Design strengths

- Clear boundary: the trait and domain types make downstream code independent of rollout-file details.
- Layered resilience: local reads tolerate partial data availability by falling back from SQLite to rollout item to raw session meta.
- Append-only mutation model: metadata changes are written as rollout events or session-meta updates, then reconciled into SQLite.
- Safety checks around archive moves: canonicalization and filename validation reduce the risk of moving arbitrary files.
- Good compatibility posture: local-only fields such as `rollout_path` are intentionally excluded from the remote schema.

## Design limitations

- Trait/backend mismatch: most trait methods are unimplemented in both backends, so the abstraction is wider than the actual behavior.
- Error inconsistency: `ThreadNotFound` exists but is rarely used, and many missing-thread cases are reported as `InvalidRequest`.
- Partial patch support: the public patch type allows more combinations than the local implementation accepts.
- Remote feature lag: remote only supports listing and even there ignores `sort_direction`.
- Some summary fields are best-effort defaults rather than persisted truth, especially approval mode, sandbox policy, and token usage on local fallback paths.

## Concrete flow summary

For the currently implemented local path, the operational flow is:

```text
app-server/core caller
    -> ThreadStore trait
    -> LocalThreadStore
        -> codex-rollout for file lookup/list/history/archive helpers
        -> codex-state for richer SQLite metadata
        -> StoredThread / ThreadPage results back to caller
```

For metadata updates:

```text
patch request
    -> append rollout event or session_meta line
    -> reconcile rollout into SQLite
    -> reread thread through normal read path
```

That second flow is particularly important because it keeps one read path and one normalization path.

## Tooling and generated code

Remote protobuf generation is intentionally manual:

- example generator: [generate-proto.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/examples/generate-proto.rs#L1-L18)
- wrapper script: [generate-proto.sh](file:///Users/bytedance/project/codex/codex-rs/thread-store/scripts/generate-proto.sh#L1-L38)
- checked-in generated output: [codex.thread_store.v1.rs](file:///Users/bytedance/project/codex/codex-rs/thread-store/src/remote/proto/codex.thread_store.v1.rs#L1-L460)

This is unusual for small crates, but it makes sense in a mixed Bazel/Cargo workspace where build-time codegen can be painful.

## Open questions

1. Should `ThreadStore` be narrowed to the implemented slice, or should the missing recorder/create/history methods be completed soon?
2. Should missing-thread cases consistently use `ThreadNotFound` instead of `InvalidRequest`?
3. Should `ThreadMetadataPatch` support multi-field atomic updates locally, especially since the public type suggests it can?
4. Should local git metadata updates be implemented, or removed from the public patch type until they are supported?
5. Should remote `ListThreads` add `sort_direction` so the trait contract matches backend behavior?
6. Should remote `StoredThread` eventually carry approval mode, sandbox policy, token usage, and history hooks for parity with local reads?
7. Is rollout append-only data the canonical source of truth long term, or will SQLite become authoritative for some mutable metadata fields?
8. Should the crate expose a dedicated local-path read API publicly, or keep `read_thread_by_rollout_path` as a concrete-backend helper only?

## Bottom line

`codex-thread-store` is already useful as the read/list/archive boundary for persisted threads, especially for app-server workflows. Its strongest design decision is combining a backend-neutral domain model with a layered local implementation that can synthesize a thread summary from multiple persistence sources.

The main thing to understand is that this crate is not yet a full persistence abstraction. It is a partially extracted boundary with solid read-side behavior, solid archive/update mechanics, and an intentionally incomplete write/remote story.
