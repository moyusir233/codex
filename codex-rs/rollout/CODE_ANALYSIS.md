# codex-rollout crate analysis

## Scope

This document analyzes the `codex-rollout` crate at `/Users/bytedance/project/codex/codex-rs/rollout`.

The crate is the persistence and discovery layer for Codex session rollouts: append-only `.jsonl` files that capture session metadata, user/agent events, and selected response items. It also acts as the bridge between those rollout files, a sidecar thread-name index, and the SQLite-backed `codex_state` runtime.

Key entry points are re-exported from [lib.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/lib.rs#L1-L64).

## High-level responsibilities

### 1. Persist live session history to rollout files

`RolloutRecorder` owns an async background writer that buffers rollout items and writes them to JSONL files on disk. It supports both creating a new rollout and resuming an existing one. See [recorder.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/recorder.rs#L77-L188) and the writer state machine in [recorder.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/recorder.rs#L1095-L1441).

Concrete behavior:

- New sessions defer file creation until `persist()` or `flush()` actually needs to materialize the file.
- Recorded items are filtered by persistence policy before enqueueing.
- Session metadata is written once, before the first persisted rollout items.
- Writes are retried after reopening the file if the first write attempt fails.

### 2. List and inspect recorded threads

The `list` module scans rollout files under `sessions/` or `archived_sessions/`, derives thread summaries, and provides timestamp-based pagination. See [list.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/list.rs#L30-L76), [list.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/list.rs#L309-L554), and [list.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/list.rs#L697-L759).

Concrete behavior:

- Supports sorting by `CreatedAt` or `UpdatedAt`.
- Supports nested date-based layout (`sessions/YYYY/MM/DD/...`) and flat layout.
- Extracts summary metadata by reading only the head of a rollout file plus a limited extra scan for the first user event.
- Filters by allowed session source and model provider.

### 3. Keep SQLite thread state aligned with rollout files

The crate integrates with `codex_state` through `state_db.rs` and `metadata.rs`. It backfills SQLite from rollout files, incrementally applies new rollout items, repairs stale rollout paths, and uses SQLite for optimized listing/search when available. See [state_db.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/state_db.rs#L23-L115), [state_db.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/state_db.rs#L185-L263), [state_db.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/state_db.rs#L326-L537), and [metadata.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/metadata.rs#L99-L357).

Concrete behavior:

- Opens SQLite only when the DB exists and backfill is complete.
- Backfills thread metadata in batches from existing rollout files.
- Repairs stale or missing rollout paths after filesystem fallback succeeds.
- Updates `updated_at` cheaply when a rollout item does not affect metadata, otherwise applies the full item set to SQLite.

### 4. Maintain a lightweight thread-name index

`session_index.rs` provides an append-only `session_index.jsonl` file that maps thread IDs to thread names and resolves the latest visible name by scanning from the end. See [session_index.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/session_index.rs#L20-L149) and [session_index.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/session_index.rs#L156-L259).

This index is used primarily for title-based search parity when the filesystem fallback path cannot rely on SQLite.

### 5. Define what is worth persisting

`policy.rs` decides which `RolloutItem`, `ResponseItem`, and `EventMsg` variants should be written to rollout storage, and distinguishes between `Limited` and `Extended` event persistence modes. See [policy.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/policy.rs#L5-L76) and [policy.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/policy.rs#L92-L186).

## Public API surface

The public exports in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/lib.rs#L21-L64) form four main API groups.

### Configuration

- `RolloutConfigView` trait and `RolloutConfig` struct in [config.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/config.rs#L5-L100)
- Encapsulates `codex_home`, `sqlite_home`, `cwd`, `model_provider_id`, and `generate_memories`
- Designed so callers can pass borrowed, owned, or `Arc<T>` config views

### Recording

- `RolloutRecorder`
- `RolloutRecorderParams::{Create, Resume}`
- `append_rollout_item_to_path`

Key methods are in [recorder.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/recorder.rs#L217-L778):

- `new`
- `record_items`
- `persist`
- `flush`
- `shutdown`
- `load_rollout_items`
- `get_rollout_history`
- `list_threads`
- `list_archived_threads`
- `find_latest_thread_path`

### Listing and file lookup

From [list.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/list.rs#L313-L1297):

- `get_threads`
- `get_threads_in_root`
- `read_thread_item_from_rollout`
- `read_head_for_summary`
- `read_session_meta_line`
- `find_thread_path_by_id_str`
- `find_archived_thread_path_by_id_str`
- `parse_cursor`
- `rollout_date_parts`

### SQLite integration

From [state_db.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/state_db.rs#L23-L537):

- `init`
- `get_state_db`
- `open_if_present`
- `list_thread_ids_db`
- `list_threads_db`
- `find_rollout_path_by_id`
- `get_dynamic_tools`
- `persist_dynamic_tools`
- `mark_thread_memory_mode_polluted`
- `reconcile_rollout`
- `read_repair_rollout_path`
- `apply_rollout_items`
- `touch_thread_updated_at`

## Core data model

### Filesystem layout

Primary rollout layout:

- `~/.codex/sessions/YYYY/MM/DD/rollout-YYYY-MM-DDThh-mm-ss-<uuid>.jsonl`

Archived layout:

- `~/.codex/archived_sessions/...`

The constants live in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/lib.rs#L21-L30), and filename parsing lives in [list.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/list.rs#L877-L892).

### Summary types

The listing API centers on:

- `ThreadsPage`: page of `ThreadItem`s plus cursor and scan metadata
- `ThreadItem`: path, thread id, first user message, cwd, git info, source, agent info, provider, CLI version, created/updated timestamps
- `Cursor`: opaque wrapper around `OffsetDateTime`

Defined in [list.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/list.rs#L30-L147).

### Recorder command model

The writer task processes a small command protocol:

- `AddItems`
- `Persist`
- `Flush`
- `Shutdown`

Defined in [recorder.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/recorder.rs#L102-L157).

This isolates caller-side session logic from file I/O and lets the writer own buffering and recovery.

## Main runtime flows

### Flow 1: Create a new rollout and persist live items

Relevant code:

- Recorder construction in [recorder.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/recorder.rs#L459-L590)
- Writer loop in [recorder.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/recorder.rs#L1301-L1352)
- State machine in [recorder.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/recorder.rs#L1095-L1299)

Step-by-step:

1. `RolloutRecorder::new` receives either `Create` or `Resume`.
2. In `Create`, it precomputes the final path and creates a `SessionMeta` object, but does not create the file yet.
3. A Tokio task is spawned with an `mpsc` receiver and a `RolloutWriterState`.
4. `record_items` filters items through `is_persisted_response_item`, optionally sanitizes them, and sends `AddItems`.
5. `AddItems` buffers items and, if the file is already materialized, flushes opportunistically.
6. `persist`, `flush`, or `shutdown` force a barrier and cause the writer to open the file if needed, write session metadata if still pending, write buffered items, flush the file, and sync SQLite state.

Design implications:

- Callers do not block on every individual append.
- Unwritten items stay buffered if materialization fails.
- Session metadata is guaranteed to precede later rollout items once the file is created.

### Flow 2: List threads from filesystem storage

Relevant code:

- Entrypoints in [list.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/list.rs#L309-L385)
- Traversal logic in [list.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/list.rs#L391-L664)
- Summary extraction in [list.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/list.rs#L697-L759)
- Head scanning in [list.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/list.rs#L1022-L1107)

Step-by-step:

1. `get_threads` targets `codex_home/sessions`, while `get_threads_in_root` can target any root.
2. The caller chooses `CreatedAt` or `UpdatedAt`, nested or flat layout, source filters, and provider filters.
3. The traversal code enumerates date directories or flat files, parses timestamps and UUIDs from filenames, and scans newest-first.
4. For each candidate file, `build_thread_item` reads a small head slice and extracts:
   - session metadata
   - first user message
   - provider and source
   - created and updated timestamps
5. Pagination uses a timestamp-only `Cursor`, implemented through `AnchorState`.

Important detail:

- `UpdatedAt` sorting cannot rely on filenames, so it must collect candidates, query mtimes, sort them, then filter and paginate.
- `CreatedAt` sorting is cheaper because the filename encodes the order.

### Flow 3: Prefer SQLite for listing/search, fall back to files, then repair

Relevant code:

- [recorder.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/recorder.rs#L217-L390)
- [state_db.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/state_db.rs#L185-L263)

Step-by-step:

1. `RolloutRecorder::list_threads` or `list_archived_threads` first tries `state_db::get_state_db`.
2. If a search term is present, it prefers SQLite because title search is indexed there.
3. If SQLite does not answer, the code falls back to filesystem scanning.
4. Every filesystem result is fed through `read_repair_rollout_path` to repair stale or missing rollout paths in SQLite.
5. SQLite is queried again after repair; only if that still fails does the code return the filesystem page directly.

This is a migration-friendly design: filesystem remains the source of truth, while SQLite is treated as an accelerator that can heal itself from rollout data.

### Flow 4: Backfill SQLite from rollout files

Relevant code:

- Extraction in [metadata.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/metadata.rs#L99-L136)
- Backfill worker in [metadata.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/metadata.rs#L138-L357)
- DB initialization in [state_db.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/state_db.rs#L25-L77)

Step-by-step:

1. `state_db::init` initializes `codex_state::StateRuntime`.
2. If backfill is not complete, it spawns `metadata::backfill_sessions`.
3. Backfill collects rollout paths from both active and archived roots.
4. Paths are sorted by a string watermark relative to `codex_home`.
5. Each rollout is loaded with `RolloutRecorder::load_rollout_items`.
6. `builder_from_items` or `builder_from_session_meta` constructs a `ThreadMetadataBuilder`.
7. `codex_state::apply_rollout_item` is applied over all rollout items to reconstruct thread metadata.
8. The result is normalized and upserted into SQLite, including dynamic tools and memory mode.
9. Progress is checkpointed so the worker can resume after interruption.

### Flow 5: Resolve thread names via sidecar index

Relevant code:

- [session_index.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/session_index.rs#L27-L149)
- reverse scanning helpers in [session_index.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/session_index.rs#L156-L259)

Step-by-step:

1. Name updates are appended to `session_index.jsonl`.
2. `find_thread_name_by_id` scans from the end, returning the newest entry for a given thread.
3. `find_thread_meta_by_name_str` streams candidate thread IDs newest-first for a matching name.
4. For each candidate ID, it attempts to resolve a real rollout path and readable session metadata.
5. Unsaved or partial rollouts are skipped so a stale index entry does not hide an older valid session.

## Module-by-module notes

### `config.rs`

Purpose:

- Defines the minimal config contract the rest of the crate needs.

Notable design:

- `RolloutConfigView` is intentionally small and object-safe enough for borrowed or shared config objects.
- `RolloutConfig::from_view` snapshots the config for spawned tasks or async work.

Reference: [config.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/config.rs#L5-L100)

### `policy.rs`

Purpose:

- Centralizes persistence policy for rollout storage and memory generation.

Notable design:

- `Limited` vs `Extended` applies only to event persistence, not the full `ResponseItem` set.
- Memory persistence is stricter than rollout persistence.
- Extended mode still sanitizes `ExecCommandEnd` output before writing.

References:

- policy rules: [policy.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/policy.rs#L12-L76)
- event classification: [policy.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/policy.rs#L92-L186)
- sanitization hook: [recorder.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/recorder.rs#L190-L215)

### `list.rs`

Purpose:

- Enumerates rollout files and derives lightweight thread summaries.

Notable design:

- Uses filename structure for fast created-at ordering.
- Uses head-only JSONL parsing for cheap summaries.
- Applies a hard cap of `MAX_SCAN_FILES = 10000` to bound work.
- Supports both nested date layout and flat layout.

Important limitation:

- Cursor pagination uses only timestamps, not `(timestamp, uuid)`, even though some comments mention stable resume after a `(ts, id)` pair. The implementation in `AnchorState::should_skip` ignores the `id` argument. This creates known tie behavior for multiple files in the same second on the filesystem path.

References:

- pagination model: [list.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/list.rs#L133-L183)
- traversal: [list.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/list.rs#L443-L554)
- head scan logic: [list.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/list.rs#L1022-L1107)

### `metadata.rs`

Purpose:

- Reconstructs `codex_state` thread metadata from rollout contents.

Notable design:

- Prefers `SessionMeta` when present, but falls back to filename-derived identity and timestamp.
- Applies rollout items incrementally through `codex_state::apply_rollout_item`.
- Backfill preserves existing SQLite git fields where rollout data is incomplete.

References:

- builder logic: [metadata.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/metadata.rs#L40-L97)
- extraction: [metadata.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/metadata.rs#L99-L136)
- backfill: [metadata.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/metadata.rs#L138-L357)

### `recorder.rs`

Purpose:

- Implements the live recorder, DB-backed listing wrapper, and rollout history loader.

Notable design:

- Background writer owns the file handle and buffering.
- `persist`, `flush`, and `shutdown` are explicit synchronization barriers.
- Metadata-relevant writes update SQLite incrementally; metadata-irrelevant writes try a cheaper `touch_thread_updated_at`.
- Listing logic is centralized here because it coordinates filesystem, SQLite, search fallback, and archived views.

References:

- recorder API: [recorder.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/recorder.rs#L217-L778)
- writer state: [recorder.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/recorder.rs#L1095-L1299)
- session meta write + DB sync: [recorder.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/recorder.rs#L1354-L1441)

### `session_index.rs`

Purpose:

- Maintains append-only thread-name history outside SQLite.

Notable design:

- Reverse scanning avoids rewriting the index.
- Append order, not `updated_at`, determines the latest visible entry.
- Batch name lookup reads forward and overwrites duplicates, which is equivalent to last-write-wins.

Reference: [session_index.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/session_index.rs#L20-L259)

### `state_db.rs`

Purpose:

- Wraps `codex_state::StateRuntime` behind rollout-specific conventions.

Notable design:

- SQLite is gated behind backfill completeness.
- Every DB list result is validated against filesystem existence.
- Reconciliation can rebuild metadata from rollout contents when direct incremental information is insufficient.

Reference: [state_db.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/state_db.rs#L23-L537)

## Dependencies and how they are used

From [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/rollout/Cargo.toml#L15-L49):

- `tokio`
  - Async filesystem, channels, spawned tasks, and tests
  - Core to recorder writer task and directory traversal
- `serde`, `serde_json`
  - JSONL serialization/deserialization for rollout files and session index
- `time`, `chrono`
  - `time` is used heavily for filename parsing, cursor handling, and RFC3339 formatting
  - `chrono` is used mostly in SQLite metadata integration because `codex_state` APIs operate with `chrono::DateTime<Utc>`
- `uuid`
  - Parses and carries thread IDs encoded in filenames
- `async-trait`
  - Used for the async rollout-file visitor abstraction in `list.rs`
- `anyhow`
  - Used mainly by metadata extraction for richer error propagation
- `tracing`
  - Logging for DB discrepancies, writer errors, and backfill progress
- `codex-protocol`
  - Defines `RolloutItem`, `RolloutLine`, `EventMsg`, `SessionMeta`, `ThreadId`, and related protocol types
- `codex-state`
  - SQLite runtime, thread metadata model, incremental rollout application, and backfill state
- `codex-file-search`
  - Filesystem fallback for finding rollout files by thread UUID
- `codex-git-utils`
  - Collects git metadata at session creation time
- `codex-login`
  - Supplies `originator()` via re-exported `default_client`
- `codex-otel`
  - Emits backfill metrics and timers
- `codex-utils-path`
  - Normalizes paths for cwd comparison and SQLite storage
- `codex-utils-string`
  - Truncates persisted aggregated command output in extended event mode

## Testing strategy

The crate has focused module-local tests rather than large integration suites.

### `src/tests.rs`

Coverage highlights from [tests.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/tests.rs#L219-L1507):

- Filesystem lookup falls back when SQLite paths are stale or missing
- Created-at listing order and pagination
- Delayed user-event discovery beyond the initial head scan
- Base-instructions preservation in session metadata heads
- `updated_at` behavior from file mtime
- Same-second pagination tie behavior on the filesystem fallback
- Source and model-provider filtering

### `src/recorder_tests.rs`

Coverage highlights from [recorder_tests.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/recorder_tests.rs#L65-L576):

- Deferred materialization and idempotent `persist()`
- Recovery after filesystem and write-handle failures
- Incremental SQLite updates for metadata-irrelevant events
- Pagination correctness with DB disabled
- Dropping stale SQLite rollout paths
- Repairing stale SQLite rollout paths
- Resume-path selection using latest `TurnContext.cwd`

### `src/metadata_tests.rs`

Coverage highlights from [metadata_tests.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/metadata_tests.rs#L37-L328):

- Metadata extraction from `SessionMeta`
- Memory-mode recovery from the latest session-meta line
- Filename fallback when metadata lines are absent
- Backfill resume/checkpoint logic
- Git info merge behavior during backfill
- CWD normalization before upsert

### `src/session_index_tests.rs`

Coverage highlights from [session_index_tests.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/session_index_tests.rs#L50-L310):

- Reverse-scan latest-entry resolution
- Skipping unsaved or partial rollouts when resolving by name
- Ignoring historical names after a rename
- Batch name resolution semantics

### `src/state_db_tests.rs`

Coverage highlights from [state_db_tests.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/state_db_tests.rs#L11-L24):

- Cursor-to-anchor timestamp normalization

### Overall testing assessment

Strengths:

- Good coverage of migration/repair behavior between filesystem and SQLite
- Good coverage of writer retry semantics
- Good coverage of subtle session-index edge cases

Gaps:

- Several DB-first listing tests in `src/tests.rs` are commented out, suggesting incomplete or unstable behavior around older list APIs
- Limited explicit test coverage for archived thread listing behavior
- No direct stress tests for large scans, concurrent writers, or channel saturation

## Design observations

### What the crate does well

- Uses rollout files as the durable source of truth while treating SQLite as a cache/index.
- Keeps live write ordering under control by funneling all writes through a single writer task.
- Separates persistence policy from writer mechanics.
- Degrades gracefully when SQLite is missing, stale, or incomplete.
- Limits list-time work through scan caps and head-only parsing.

### Trade-offs and constraints

- Filesystem listing is intentionally approximate compared with SQLite-backed listing.
- `UpdatedAt` sorting is inherently more expensive because mtimes must be read and sorted.
- Backfill completeness is used as a binary readiness gate for SQLite, which simplifies behavior but delays DB usage during migration or initialization.
- The session index is simple and append-only, but name-based search is weaker than a true indexed store.

### Important implementation details

- The writer flushes after every written line in `JsonlWriter::write_line`, which favors durability and simplicity over throughput. See [recorder.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/recorder.rs#L1486-L1492).
- `read_head_summary` can scan beyond the initial head limit when session metadata is found but the first user event is delayed, which avoids missing valid sessions with extra metadata lines. See [list.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/list.rs#L1031-L1104).
- Archived listing uses flat layout in `list_threads_from_files_desc_unfiltered`, while active listing uses nested date layout. See [recorder.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/recorder.rs#L877-L914).

## Open questions

### 1. Should cursor pagination include UUID tie-breaking in the public token?

The code comments describe resuming after the last `(ts, id)` pair, but the actual `Cursor` stores only a timestamp and `AnchorState::should_skip` ignores the `id`. Tests explicitly document the same-second tie loss on the filesystem fallback. See [list.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/list.rs#L149-L183) and [tests.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/tests.rs#L1207-L1331).

Open question:

- Is the current behavior acceptable only during migration because SQLite-backed listing avoids the tie, or is the filesystem cursor format expected to evolve?

### 2. Is per-line flush intentional for production workloads?

`JsonlWriter::write_line` flushes after every line, and the outer writer flushes again after a batch. This is safe but can be costly on slow filesystems. See [recorder.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/recorder.rs#L1256-L1265) and [recorder.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/recorder.rs#L1486-L1492).

Open question:

- Is this durability requirement intentional, or could flush frequency be relaxed behind a configuration or batching policy?

### 3. Why are archived rollouts treated as flat files?

Archived listing currently routes through `ThreadListLayout::Flat`, while active listing uses nested-by-date traversal. See [recorder.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/recorder.rs#L887-L900).

Open question:

- Is archived storage guaranteed to be flat, or is this just an optimization/legacy assumption that could break if archived rollouts adopt the same nested structure?

### 4. What is the intended long-term role of `session_index.jsonl`?

Today it fills a gap for title lookup/search parity, especially when SQLite is unavailable or incomplete. See [session_index.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/session_index.rs#L27-L149) and [recorder.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/recorder.rs#L991-L1014).

Open question:

- Once SQLite migration is complete, should this sidecar index remain authoritative for any use case, or can it become a compatibility path only?

### 5. Why do some DB listing tests remain commented out?

`src/tests.rs` contains disabled tests for DB-preferred list behavior. See [tests.rs](file:///Users/bytedance/project/codex/codex-rs/rollout/src/tests.rs#L92-L217).

Open question:

- Are those tests obsolete after API changes, or do they indicate unresolved correctness issues in DB-first listing semantics?

## Bottom line

`codex-rollout` is not just a file writer. It is a hybrid persistence subsystem with three synchronized views of session history:

- rollout JSONL files as durable truth
- SQLite thread metadata as query/index acceleration
- `session_index.jsonl` as a lightweight title-resolution sidecar

Its main design theme is graceful migration: keep the filesystem authoritative, opportunistically use SQLite when trustworthy, and repair drift whenever the filesystem reveals a better answer.
