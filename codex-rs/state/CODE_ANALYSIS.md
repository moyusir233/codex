# codex-state crate analysis

## Scope

This document analyzes the crate at `state/`, focusing on:

- `Cargo.toml`
- `src/lib.rs`
- the core runtime modules under `src/runtime/`
- the model and extraction modules under `src/model/` and `src/extract.rs`
- migration files under `migrations/` and `logs_migrations/`
- the tracing integration in `src/log_db.rs`
- the CLI helper in `src/bin/logs_client.rs`
- representative inline tests across the crate

I also ran:

- `cargo test -p codex-state`

The crate’s tests passed locally: 93 unit tests, 3 binary tests, and 1 doctest.

## Overview

`codex-state` is the local persistence layer for Codex session metadata and related operational state. Its center of gravity is `StateRuntime`, a SQLite-backed service object that owns:

- thread/session metadata storage
- dedicated tracing log persistence in a separate SQLite database
- memory pipeline state for stage-1 extraction and global phase-2 consolidation
- backfill lifecycle state
- agent job and agent job item state
- remote-control enrollment records
- thread dynamic tool definitions and spawn-edge relationships

The crate does not try to be the rollout scanner itself. `src/lib.rs` explicitly says rollout scanning and backfill orchestration live in `codex-core`; this crate persists the extracted results and exposes database-oriented APIs.

At a high level, the architecture looks like this:

1. higher-level code parses rollout JSONL into `RolloutItem` values
2. `apply_rollout_item()` maps those items into `ThreadMetadata`
3. `StateRuntime::apply_rollout_items()` reconciles that metadata with SQLite
4. related subsystems persist logs, memory extraction jobs, agent jobs, and remote-control data in the same runtime
5. callers query the runtime for thread lists, logs, memory state, and job progress

## Primary responsibilities

### 1. Persist canonical thread metadata

The thread store is the oldest and broadest responsibility in the crate. It persists a canonical row per thread, including:

- thread id and rollout path
- creation and update timestamps
- source, model provider, model, reasoning effort
- cwd, CLI version, title, first user message
- sandbox and approval policy
- archive state
- git metadata
- memory mode
- agent nickname, role, and canonical path

The important design point is that SQLite becomes the fast queryable index over rollout-derived metadata, while the rollout file remains the source material.

### 2. Apply rollout items incrementally

The extraction layer is intentionally narrow. `src/extract.rs` does not load files or own storage. Instead it:

- maps `SessionMeta`, `TurnContext`, selected `EventMsg` variants, and a few rollout-specific conventions into `ThreadMetadata`
- decides which rollout items can affect stored thread metadata
- derives best-effort thread titles and first-user-message previews
- handles fallbacks like default model provider and image-only user prompts

`StateRuntime::apply_rollout_items()` is the bridge from extracted items to persistent state.

### 3. Maintain a dedicated logs database

The crate separates logs from the main state DB:

- state DB filename is versioned as `state_5.sqlite`
- logs DB filename is versioned as `logs_2.sqlite`

This is a deliberate concurrency decision. `StateRuntime::init()` opens two SQLite pools and the comments state that logs live in a dedicated file to reduce lock contention with the rest of the store.

The logs subsystem provides:

- batch log insertion
- per-thread and per-process threadless retention pruning
- filtered log querying
- feedback-log aggregation for one or more threads
- startup maintenance for retention, checkpointing, and vacuum

### 4. Coordinate the memory pipeline

The memory subsystem is much more than a simple table wrapper. It implements:

- storage of per-thread stage-1 memory extraction output
- a job table for both stage-1 thread work and singleton global phase-2 consolidation
- optimistic claim / lease / retry semantics
- retention pruning for old stage-1 outputs
- ranking and diffing logic for phase-2 input selection
- remembered baseline snapshots so phase-2 can tell what was added, retained, or removed
- thread-level pollution handling that can trigger global forgetting/reconsolidation

This is one of the most sophisticated parts of the crate.

### 5. Track agent jobs

The agent-job subsystem stores structured long-running job state:

- top-level job metadata in `agent_jobs`
- per-row/per-item work units in `agent_job_items`
- item lifecycle transitions
- result reporting tied to the assigned thread
- job progress aggregation

This is separate from the memory pipeline jobs. Memory jobs use the generic `jobs` table; agent jobs have a dedicated schema with richer item state.

### 6. Persist operational control state

The crate also stores smaller but important operational state:

- rollout backfill lifecycle in `backfill_state`
- remote-control enrollment records in `remote_control_enrollments`
- directional thread spawn edges
- dynamic tool definitions attached to threads

This makes `codex-state` the durable operational memory of the desktop/local runtime.

## Crate layout

The crate is organized around one façade and several focused implementation areas.

- `src/lib.rs`
  - public façade and re-export hub
  - exposes the preferred runtime API and the important model types
- `src/runtime.rs`
  - runtime initialization, database opening, migration policy, legacy DB cleanup, path helpers
- `src/runtime/threads.rs`
  - thread CRUD, filtering, pagination, rollout application, dynamic tools, spawn graph
- `src/runtime/logs.rs`
  - logs insertion, pruning, querying, feedback-log rendering
- `src/runtime/memories.rs`
  - stage-1 outputs, memory jobs, global consolidation state, retention, selection logic
- `src/runtime/agent_jobs.rs`
  - agent job persistence and item lifecycle APIs
- `src/runtime/backfill.rs`
  - singleton backfill state machine
- `src/runtime/remote_control.rs`
  - remote-control enrollment CRUD
- `src/extract.rs`
  - transforms rollout protocol items into thread metadata mutations
- `src/model/`
  - durable/public data contracts
- `src/log_db.rs`
  - `tracing_subscriber::Layer` that asynchronously writes logs into the logs DB
- `src/bin/logs_client.rs`
  - small CLI for tailing/querying the logs DB

The crate has no external integration tests directory; instead, it relies heavily on inline async tests in the relevant modules.

## Public API shape

The public surface is intentionally centered on `StateRuntime`, with a secondary layer of reusable models and a few low-level helpers.

### Preferred entrypoint: `StateRuntime`

`src/lib.rs` documents `StateRuntime` as the preferred entrypoint. It owns:

- configuration such as `codex_home` and `default_provider`
- the state DB pool
- the logs DB pool
- the monotonic in-process high-water mark for `updated_at_ms`

The main public methods cluster into six groups:

- **Threads**
  - `get_thread`
  - `list_threads`
  - `list_thread_ids`
  - `find_rollout_path_by_id`
  - `find_thread_by_exact_title`
  - `upsert_thread`
  - `insert_thread_if_absent`
  - `apply_rollout_items`
  - `mark_archived` / `mark_unarchived`
  - `delete_thread`
  - `set_thread_memory_mode`
  - `update_thread_git_info`
  - spawn-edge and dynamic-tool APIs
- **Logs**
  - `insert_log`
  - `insert_logs`
  - `query_logs`
  - `query_feedback_logs`
  - `query_feedback_logs_for_threads`
  - `max_log_id`
- **Memories**
  - `claim_stage1_jobs_for_startup`
  - `try_claim_stage1_job`
  - `mark_stage1_job_succeeded`
  - `mark_stage1_job_succeeded_no_output`
  - `mark_stage1_job_failed`
  - `try_claim_global_phase2_job`
  - `heartbeat_global_phase2_job`
  - `mark_global_phase2_job_succeeded`
  - `mark_global_phase2_job_failed`
  - `mark_global_phase2_job_failed_if_unowned`
  - `get_phase2_input_selection`
  - `list_stage1_outputs_for_global`
  - `record_stage1_output_usage`
  - `prune_stage1_outputs_for_retention`
  - `mark_thread_memory_mode_polluted`
  - `clear_memory_data`
- **Agent jobs**
  - `create_agent_job`
  - `get_agent_job`
  - `list_agent_job_items`
  - `get_agent_job_item`
  - `mark_agent_job_running`
  - `mark_agent_job_completed`
  - `mark_agent_job_failed`
  - `mark_agent_job_cancelled`
  - item transition APIs and `get_agent_job_progress`
- **Backfill**
  - `get_backfill_state`
  - `try_claim_backfill`
  - `mark_backfill_running`
  - `checkpoint_backfill`
  - `mark_backfill_complete`
- **Remote control**
  - `get_remote_control_enrollment`
  - `upsert_remote_control_enrollment`
  - `delete_remote_control_enrollment`

### Low-level helpers

The crate also intentionally exports:

- `apply_rollout_item()`
  - pure metadata mutation logic for one `RolloutItem`
- `rollout_item_affects_thread_metadata()`
  - filter helper for incremental processing
- DB path helpers:
  - `state_db_filename()`
  - `state_db_path()`
  - `logs_db_filename()`
  - `logs_db_path()`

This is a good separation: consumers that only need domain logic can use the helper functions, but most real code should use `StateRuntime`.

## Data model and schema

### Thread metadata model

`ThreadMetadata` is the canonical record for a persisted thread. It is intentionally richer than the original `threads` table from `0001_threads.sql`; later migrations added:

- `cli_version`
- `first_user_message`
- `agent_nickname`, `agent_role`, `agent_path`
- `model`, `reasoning_effort`
- `memory_mode`
- millisecond timestamps via `created_at_ms` and `updated_at_ms`

`ThreadMetadataBuilder` is the pre-persistence builder used when a thread is first created from rollout information.

Important implementation details:

- build-time defaults use protocol enums for sandbox/approval defaults
- missing model provider falls back to the runtime’s configured default provider
- `prefer_existing_git_info()` preserves already-known Git data during partial rollout updates
- `diff_fields()` is used by tests and useful for debugging reconciliation behavior

### Logs model

The logs model is intentionally small:

- `LogEntry`
  - write-time structure, including `feedback_log_body` and legacy `message`
- `LogRow`
  - read-time structure
- `LogQuery`
  - log filter object

The current logs schema lives in `logs_migrations/` and stores `feedback_log_body` rather than `message` as the primary persisted body. The query path aliases `feedback_log_body AS message` so consumers still get a message-like field.

### Memory pipeline model

The memory-related models are the most workflow-oriented:

- `Stage1Output`
  - persisted extraction output for one thread
- `Stage1OutputRef`
  - compact reference used for “removed” diffs
- `Stage1JobClaimOutcome`
  - detailed claim results such as claimed, up-to-date, backoff, exhausted, running
- `Stage1JobClaim`
  - claimed thread + ownership token
- `Phase2JobClaimOutcome`
  - claimed / skipped-not-dirty / skipped-running
- `Phase2InputSelection`
  - `selected`, `previous_selected`, `retained_thread_ids`, `removed`

The schema grows stage-1 outputs into a baseline-tracking store by later migrations:

- `rollout_slug`
- `usage_count`
- `last_usage`
- `selected_for_phase2`
- `selected_for_phase2_source_updated_at`

### Agent job model

The agent-job types are conventional but solid:

- `AgentJobStatus`
  - `Pending`, `Running`, `Completed`, `Failed`, `Cancelled`
- `AgentJobItemStatus`
  - `Pending`, `Running`, `Completed`, `Failed`
- `AgentJob`
  - top-level job with CSV paths, input headers, optional output schema, timestamps
- `AgentJobItem`
  - per-row execution unit with attempts, assigned thread, result JSON, and error fields
- `AgentJobProgress`
  - aggregated counts by item state

The explicit `output_schema_json` TODO in `model/agent_job.rs` shows the subsystem is functional but still evolving toward stronger JSON-schema enforcement.

### Other small models

- `BackfillState` / `BackfillStatus`
  - singleton progress row for rollout metadata backfill
- `DirectionalThreadSpawnEdgeStatus`
  - `Open` / `Closed`, implemented with `strum`
- `RemoteControlEnrollmentRecord`
  - target/account/app-client keyed enrollment data

## End-to-end flows

### Thread ingestion flow

The thread-ingestion path is:

1. caller constructs a `ThreadMetadataBuilder`
2. caller provides parsed `RolloutItem` values
3. `StateRuntime::apply_rollout_items()` loads existing thread metadata if present
4. each rollout item is applied through `apply_rollout_item()`
5. existing Git info is preserved when the new rollout payload is incomplete
6. `updated_at` comes either from an explicit override or the rollout file mtime
7. the runtime upserts the thread row
8. memory mode and dynamic tools are extracted from session metadata and persisted separately

Important invariants:

- the thread row is written before dynamic tools because `thread_dynamic_tools.thread_id` has a foreign key to `threads`
- dynamic tools are write-once in practice; insertion uses `ON CONFLICT ... DO NOTHING`
- thread creation memory mode is only honored on initial creation, not on later upserts

### Thread ordering and pagination flow

The crate uses keyset pagination based on `Anchor { ts }` plus sort key/direction. The subtle design choice is the `thread_updated_at_millis` process-local atomic:

- hot writes in the same second get unique millisecond timestamps
- historical/backfill timestamps that are much older are preserved as-is
- this keeps ordering stable for cursors without requiring a DB read for every update

This is a pragmatic workaround for SQLite rows that might otherwise collide at second precision.

### Log ingestion and query flow

There are two connected pieces:

1. `log_db::start()` installs a tracing layer with an async batch inserter
2. `runtime/logs.rs` persists and queries the resulting rows

For each event:

- span and event visitors extract `thread_id`, formatted fields, and the rendered body
- a process UUID is attached once per process
- events are buffered in an MPSC queue
- the background task flushes every 128 rows or every 2 seconds
- `insert_logs()` writes the batch and prunes over-budget partitions within the same transaction

Retention design:

- one partition per non-null `thread_id`
- one partition per threadless `process_uuid`
- one extra partition for threadless rows whose `process_uuid` is null
- each partition is capped at about 10 MiB or 1,000 rows
- startup maintenance also deletes rows older than 10 days and runs WAL checkpoint + incremental vacuum

The feedback-log query path is especially product-specific:

- it fetches logs for the requested thread ids
- it also includes threadless rows from the latest relevant process UUIDs
- it re-applies an exact whole-line byte cap after formatting the final lines

### Memory pipeline flow

The memory subsystem implements a two-phase pipeline.

#### Stage 1

Stage 1 is per-thread extraction:

1. `claim_stage1_jobs_for_startup()` scans stale eligible threads
2. `try_claim_stage1_job()` uses `BEGIN IMMEDIATE` and a lease-based row in `jobs`
3. successful workers generate `raw_memory` and `rollout_summary`
4. success paths either:
   - upsert a `stage1_outputs` row, or
   - delete it when there is no output
5. stage-1 success enqueues global phase-2 work by advancing the singleton global watermark

The claim logic is careful:

- skips work when stage-1 output is already up-to-date
- skips work when a prior success watermark covers the same source update
- enforces a global running cap
- respects retry backoff and retry exhaustion
- resets retry budget when the source watermark advances

#### Phase 2

Phase 2 is singleton global consolidation:

1. stage-1 completion enqueues or advances the global job watermark
2. `try_claim_global_phase2_job()` checks dirtiness, retries, backoff, and lease ownership
3. the owner builds a selection via `get_phase2_input_selection()`
4. `mark_global_phase2_job_succeeded()` advances the last success watermark and rewrites the exact selected baseline snapshot

What makes this design strong is the baseline snapshot logic:

- the system remembers which exact `stage1_outputs` snapshots were in the last successful global run
- later diffs can distinguish:
  - newly added outputs
  - retained outputs
  - removed outputs
  - regenerated outputs that should count as changed even when the same thread remains selected

This is much more precise than just storing a set of thread ids.

### Agent job flow

The agent-job subsystem is straightforward:

1. `create_agent_job()` inserts the job and all initial items in one transaction
2. runners move items from `Pending` to `Running`
3. a running item can be associated with a thread id
4. results are reported atomically by the assigned thread
5. items move to completed/failed or back to pending
6. top-level job status is updated separately
7. `get_agent_job_progress()` aggregates item counts

One good safety property is `report_agent_job_item_result()` requiring both:

- `status = running`
- `assigned_thread_id = reporting_thread_id`

That prevents late or cross-thread result reports from being accepted silently.

### Backfill flow

The backfill subsystem is a singleton row with lease-like semantics:

1. `ensure_backfill_state_row()` inserts the row if missing
2. `try_claim_backfill(lease_seconds)` claims only if not complete and not freshly running
3. progress is stored through `checkpoint_backfill()`
4. completion stores `last_success_at` and optionally the final watermark

This is intentionally simple because orchestration itself lives elsewhere.

## Database and migration strategy

### Two databases, versioned filenames

The crate keeps separate files for:

- main state DB
- logs DB

That split is one of the key architectural choices in the crate.

### Runtime migrator policy

`src/migrations.rs` wraps the embedded SQLx migrators with `ignore_missing = true`. That means:

- known migrations are still checksum-validated
- the runtime tolerates a database that was already migrated by a newer binary

This is a practical compatibility choice for parallel or staggered binary upgrades.

### Main schema evolution

The main `migrations/` directory shows the story of the crate’s growth:

- `0001_threads.sql`
  - initial thread metadata index
- `0004_thread_dynamic_tools.sql`
  - dynamic tools table
- `0006_memories.sql`
  - stage-1 outputs and generic jobs table
- `0008_backfill_state.sql`
  - singleton backfill state
- `0013_*` and `0020_*`
  - agent fields, model, reasoning effort
- `0014_agent_jobs.sql` and `0015_*`
  - agent job subsystem
- `0016_*` to `0018_*`
  - memory usage and phase-2 snapshot metadata
- `0021_thread_spawn_edges.sql`
  - parent/child thread graph
- `0024_remote_control_enrollments.sql`
  - remote-control persistence
- `0025_thread_timestamps_millis.sql`
  - migration from second precision to millisecond ordering
- `0023_drop_logs.sql`
  - removal of the old logs table from the main state DB

### Logs schema evolution

The logs DB has a short, focused migration chain:

- `0001_logs.sql`
  - dedicated logs table with `estimated_bytes`
- `0002_logs_feedback_log_body.sql`
  - renames the old table and rebuilds it so `feedback_log_body` becomes the persisted body field

That short chain is a sign that the dedicated logs DB is a newer, cleaned-up design.

## Dependencies and their roles

`Cargo.toml` shows a relatively small dependency set for a storage crate.

### Workspace dependency

- `codex-protocol`
  - defines `ThreadId`, `RolloutItem`, `DynamicToolSpec`, and the rollout/session protocol types that this crate persists

This is the most important dependency because it anchors the crate’s domain model to shared protocol types.

### External dependencies

- `anyhow`
  - ergonomic crate-wide error handling
- `chrono`
  - UTC timestamps and cutoff calculations
- `serde` / `serde_json`
  - JSON serialization for tool schemas, agent-job payloads, and memory payloads
- `sqlx`
  - SQLite pools, migrations, row conversion, and dynamic query building
- `tokio`
  - async filesystem access, background tasks, channels, and tests
- `uuid`
  - ownership tokens and process log UUIDs
- `tracing` / `tracing-subscriber`
  - tracing capture and export into SQLite
- `strum`
  - string conversion for graph-edge status
- `log`
  - SQLx log suppression during connection setup
- `clap`, `dirs`, `owo-colors`
  - only needed for the `logs_client` binary

### Dev dependencies

- `codex-git-utils`
  - used by tests that exercise Git metadata handling
- `pretty_assertions`
  - improves test diffs

## Testing coverage

This crate is well-tested for a persistence layer.

### What is covered well

- **Extraction semantics**
  - titles, first-user-message handling, image-only prompts, `TurnContext`, and model/reasoning behavior
- **Thread persistence**
  - creation memory mode, keyset pagination, Git-field preservation, millisecond timestamp allocation, spawn-edge behavior, and override-based updates
- **Logs**
  - dedicated logs DB initialization, schema migration, pruning by size and row count, threadless process behavior, feedback-log rendering, and search filtering
- **Memory pipeline**
  - stage-1 claim semantics, concurrency, retry exhaustion, running-cap throttling, startup scanning, no-output success, retention pruning, selection diffing, pollution handling, phase-2 locking, watermark behavior, and fallback failure handling
- **Backfill**
  - singleton claim behavior, staleness, completion, and legacy DB cleanup
- **Remote control**
  - enrollment round trip and targeted deletion
- **Agent jobs**
  - atomic result reporting and rejection of late reports
- **Tracing layer**
  - SQLite feedback logs match the human-facing formatter shape and flush behavior

### Test-suite assessment

The test names are descriptive and map closely to real product behavior. For this kind of crate, inline async tests are an effective choice because:

- the logic is database-heavy and best tested against real SQLite
- many invariants are transactional rather than purely functional
- concurrency and lease semantics are easier to validate near the implementation

### Remaining test gaps

The main gaps are not correctness gaps so much as integration-boundary gaps:

- no separate integration suite that exercises a higher-level rollout scanner against this crate
- limited direct validation of consumers outside the crate
- no performance/regression tests for very large state DBs or high log throughput

## Design assessment

### Strengths

- **Clear center of gravity**
  - `StateRuntime` is an easy abstraction to understand and use
- **Good persistence boundaries**
  - state and logs are split into separate databases for concurrency and retention reasons
- **Pragmatic migration policy**
  - tolerant migrators reduce upgrade friction across binaries
- **Thoughtful ordering semantics**
  - millisecond allocation solves practical pagination issues without overcomplicating the model
- **Strong transactional design**
  - many state transitions update related tables in a single transaction
- **Product-aware memory design**
  - phase-2 baseline snapshots preserve semantic diffs, not just row existence
- **High-value tests**
  - the tests focus on the riskiest stateful behaviors

### Trade-offs and limitations

- **Very large `memories.rs`**
  - it is cohesive, but 4,600+ lines now combine selection policy, job semantics, retry policy, retention, and tests in one place
- **Very large `threads.rs`**
  - it owns CRUD, filtering, pagination, spawn graphs, dynamic tools, and rollout reconciliation
- **SQL-first implementation**
  - correct and efficient, but harder to refactor because much logic is embedded in query text
- **Some historical layering remains visible**
  - the old main-DB logs migrations still exist historically even though logs now live in a separate DB
- **Public constants do not always map to internal behavior**
  - `SQLITE_HOME_ENV` is exported in `lib.rs` but is not used inside this crate

## Open questions

1. Should `runtime/memories.rs` be split into smaller submodules?
   - The subsystem is coherent, but its current size makes policy changes harder to review.

2. Should `runtime/threads.rs` be split into query/update/graph/dynamic-tool submodules?
   - It has enough responsibilities now that navigability is starting to suffer.

3. Why is `SQLITE_HOME_ENV` exported but unused internally?
   - It may be intended for callers, but that contract is not visible from this crate alone.

4. Should the logs retention window be configurable?
   - `LOG_RETENTION_DAYS` is hard-coded to 10 days, which may or may not match all deployment needs.

5. Should the log-partition retention budgets be configurable?
   - The fixed 10 MiB / 1,000 row caps are sensible, but they encode product policy directly into the storage layer.

6. Should the memory pipeline status strings be formalized into enums at the SQL boundary too?
   - The public claim outcomes are typed, but the shared `jobs` table still uses string statuses such as `running`, `done`, and `error`.

7. Is there a future plan to move some SQL-heavy logic behind typed repositories or query helpers?
   - That could improve readability, though it would trade off directness.

8. Should agent jobs and memory jobs converge on a shared job abstraction?
   - Today they intentionally use different schemas because the workflows differ, but there is some conceptual overlap.

## Bottom line

`codex-state` is not just a thin SQLite wrapper. It is the local persistence backbone for Codex session metadata and several operational workflows. The main architectural ideas are sound:

- keep a fast SQLite index over rollout-derived thread metadata
- isolate tracing logs into a dedicated DB
- encode memory extraction/consolidation as durable leased jobs
- preserve semantic selection history for global memory consolidation
- expose one runtime object that callers can use without knowing the schema details

The crate is strongest where it handles real-world statefulness: migrations, leases, monotonic timestamps, pruning, retries, and snapshot-based diffing. Its main long-term risk is code volume in `threads.rs` and `memories.rs`, not obvious correctness flaws.
