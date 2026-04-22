# codex-cloud-tasks-mock-client analysis

## Scope

- Crate manifest: [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/Cargo.toml#L1-L19)
- Public facade: [lib.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/lib.rs#L1-L3)
- Full implementation: [mock.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L1-L203)
- Bazel target: [BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/BUILD.bazel#L1-L6)
- Interface implemented by this crate: [cloud-tasks-client/api.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-client/src/api.rs#L133-L170)

## Responsibilities

- Provide a zero-state in-memory mock implementation of the `CloudBackend` trait through [MockClient](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L17-L21).
- Supply deterministic-enough task data for local UI work and tests, especially task listing, diff retrieval, apply flows, and alternate-attempt flows in [mock.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L21-L161).
- Generate lightweight `DiffSummary` metadata from embedded unified diffs using [count_from_unified](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L177-L203).
- Exercise environment-aware task filtering by returning different task sets for `None`, `"env-A"`, and `"env-B"` in [list_tasks](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L22-L73).
- Support the cloud-tasks application’s debug/mock startup path, where `CODEX_CLOUD_TASKS_MODE=mock` swaps in this backend in [cloud-tasks/src/lib.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L43-L58).

## Public API

- The crate exports exactly one type from [lib.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/lib.rs#L1-L3): `MockClient`.
- `MockClient` implements the full `CloudBackend` trait required by consumers in [api.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-client/src/api.rs#L133-L170).
- Implemented operations in [mock.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L21-L161):
  - `list_tasks`: returns canned task rows and derived diff summaries.
  - `get_task_summary`: looks up a task from the global task list.
  - `get_task_diff`: returns a canned unified diff string.
  - `get_task_messages`: returns a canned assistant-output fallback message.
  - `get_task_text`: returns canned prompt/message/turn metadata.
  - `apply_task`: reports successful application without mutating anything.
  - `apply_task_preflight`: reports successful dry-run validation.
  - `list_sibling_attempts`: exposes one alternate attempt for `T-1000`.
  - `create_task`: fabricates a timestamp-based local task id.

## Module layout

- [lib.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/lib.rs#L1-L3) is only a facade that re-exports `MockClient`.
- [mock.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L1-L203) contains all behavior:
  - trait implementation in [mock.rs:L21-L161](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L21-L161)
  - canned diff fixtures in [mock_diff_for](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L163-L175)
  - diff-summary parsing in [count_from_unified](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L177-L203)
- [BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/BUILD.bazel#L1-L6) shows that Bazel treats this as a normal Rust crate with no special data files or test fixtures.

## Runtime flow

- Task listing flow:
  - [list_tasks](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L22-L73) picks a canned row set based on `env`.
  - Each row is converted to a `TaskId`, then mapped to a synthetic unified diff via [mock_diff_for](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L163-L175).
  - The diff is summarized into added/removed line counts via [count_from_unified](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L177-L203).
  - A `TaskSummary` is assembled with `Utc::now()`, environment metadata, `is_review = false`, and a canned `attempt_total` in [mock.rs:L48-L72](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L48-L72).
- Detail flow:
  - [get_task_summary](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L75-L84) reuses `list_tasks(None, None, None)` and searches the returned vector.
  - [get_task_diff](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L86-L88) always returns `Some(diff)`.
  - [get_task_text](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L96-L105) returns a single canned prompt, message, turn id, and completed attempt status.
- Attempt flow:
  - [list_sibling_attempts](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L131-L147) returns one sibling only for `T-1000`, including a diff and `attempt_placement = Some(1)`.
  - This directly supports downstream attempt aggregation in [collect_attempt_diffs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L304-L345).
- Apply flow:
  - [apply_task_preflight](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L117-L129) always returns `ApplyStatus::Success` with `applied = false`.
  - [apply_task](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L107-L115) always returns `ApplyStatus::Success` with `applied = true`.
  - Both methods ignore `diff_override`, so alternate-attempt apply code paths are structurally exercised but not behaviorally validated.
- Creation flow:
  - [create_task](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L149-L160) ignores all inputs and returns `task_local_<timestamp_millis>`.

## Data and behavior details

- The mock is stateless:
  - `MockClient` is an empty struct with `Clone` and `Default` in [mock.rs:L17-L18](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L17-L18).
  - No method mutates shared state or stores created/applied tasks.
- Embedded task catalog:
  - Global/default tasks: `T-1000`, `T-1001`, `T-1002` in [mock.rs:L35-L39](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L35-L39).
  - Environment-specific tasks: `env-A -> T-2000`, `env-B -> T-3000/T-3001` in [mock.rs:L29-L34](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L29-L34).
- Embedded diff fixtures:
  - `T-1000` edits `README.md` and inserts one extra line in [mock.rs:L165-L167](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L165-L167).
  - `T-1001` removes one line from `core/src/lib.rs` in [mock.rs:L168-L170](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L168-L170).
  - All other ids, including unknown ids, fall back to adding `CONTRIBUTING.md` in [mock.rs:L171-L173](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L171-L173).
- Diff summarization strategy:
  - First tries structured parsing with `diffy::Patch::from_str` in [mock.rs:L178-L187](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L178-L187).
  - Falls back to line-prefix counting while skipping diff headers in [mock.rs:L188-L202](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L188-L202).
  - This makes the crate resilient to malformed or simplified diffs without pulling in a larger patch model.

## Dependencies

- Direct dependencies from [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/Cargo.toml#L15-L19):
  - `async-trait`: enables the async trait implementation for `CloudBackend`.
  - `chrono`: supplies `Utc::now()` and timestamp-based ids.
  - `codex-cloud-tasks-client`: provides the trait and all shared task/apply data types.
  - `diffy`: parses unified diffs for `DiffSummary` derivation.
- The dependency surface is intentionally small. This crate does not depend on HTTP, filesystem, or serialization libraries directly because it only fabricates in-process values.

## Downstream usage

- Debug backend selection:
  - [init_backend](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L43-L58) instantiates `MockClient` when `CODEX_CLOUD_TASKS_MODE=mock` under debug builds.
- Integration-style test coverage from another crate:
  - [env_filter.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/tests/env_filter.rs#L1-L39) verifies that `list_tasks` varies by environment.
- Attempt-flow test coverage from another crate:
  - [collect_attempt_diffs_includes_sibling_attempts](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L2343-L2355) relies on the mock to return the primary diff plus one sibling attempt for `T-1000`.

## Testing

- This crate contains no `#[cfg(test)]` module or integration tests of its own.
- Effective coverage is indirect and consumer-driven:
  - environment-specific listing is checked in [env_filter.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/tests/env_filter.rs#L1-L39)
  - sibling-attempt aggregation is checked in [cloud-tasks/src/lib.rs:L2343-L2355](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks/src/lib.rs#L2343-L2355)
- Notably untested inside or outside this crate:
  - `count_from_unified` fallback behavior on malformed diffs
  - `get_task_summary` miss behavior for environment-only task ids
  - `apply_task` and `apply_task_preflight` handling of `diff_override`
  - `create_task` id format stability and uniqueness assumptions

## Design observations

- Good fit for interface-driven development:
  - The crate implements the entire `CloudBackend` abstraction, so consumers can swap between HTTP and mock backends without extra adapters.
- Very low operational complexity:
  - No I/O, no runtime configuration, and no shared mutable state keep the mock cheap and predictable.
- Intentionally behavior-light:
  - The mock validates control-flow integration more than backend correctness. It is strongest for UI development and smoke tests, not for semantic backend simulation.
- Some realism is deliberately present:
  - per-environment task sets
  - computed diff summaries instead of hard-coded counts
  - alternate-attempt support for one task
- Some realism is deliberately absent:
  - pagination and limits
  - persistent apply/create state transitions
  - task-not-found handling for all entry points
  - message/diff consistency across task ids

## Open questions

- Should [get_task_summary](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L75-L84) be able to resolve environment-specific ids such as `T-2000` and `T-3000`? Today it only searches the global `env = None` list, so those ids fail even though `list_tasks(Some(...))` returns them.
- Should [get_task_diff](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L86-L88) return an error for unknown task ids instead of silently mapping everything non-special to the `CONTRIBUTING.md` diff?
- Should [get_task_messages](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L90-L94) and [get_task_text](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L96-L105) vary by task so consumers can exercise both “diff available” and “messages only” detail views more realistically?
- Should [apply_task](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L107-L115) and [apply_task_preflight](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L117-L129) inspect `diff_override` and optionally simulate conflict/partial outcomes, especially because downstream code has alternate-attempt selection logic?
- Should [list_tasks](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L22-L73) honor `limit` and `cursor` so pagination code can be tested without introducing a heavier fake?
- Should timestamps be deterministic in tests? Using `Utc::now()` in [mock.rs:L57](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L57) and [mock.rs:L140](file:///Users/bytedance/project/codex/codex-rs/cloud-tasks-mock-client/src/mock.rs#L140) makes outputs non-stable across runs.

## Overall assessment

- This crate is a thin but useful contract test double for `CloudBackend`.
- Its strongest value is enabling local development and smoke testing of the cloud-tasks UX without auth, network, or backend availability.
- Its main limitation is that it models happy-path shape, not lifecycle state or backend edge cases.
