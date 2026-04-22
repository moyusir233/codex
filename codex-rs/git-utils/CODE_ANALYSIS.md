# codex-git-utils Code Analysis

## Scope and purpose

`codex-git-utils` is a focused infrastructure crate that wraps the system `git` binary for three main product needs:

1. **Repository metadata and trust discovery**
   - Detect whether a path is inside a Git repo.
   - Resolve repo roots and worktree roots.
   - Read branch, commit, remote, change, and diff metadata.

2. **Patch application**
   - Apply or preflight unified diffs with `git apply`.
   - Parse `git apply` output into structured applied/skipped/conflicted path sets.

3. **Undo snapshots (“ghost commits”)**
   - Capture the working tree into an unreferenced commit object.
   - Restore the working tree back to that snapshot later.
   - Preserve pre-existing untracked or ignored content so undo is data-safe.

The crate intentionally prefers shelling out to `git` over binding to libgit2. The design is pragmatic: reuse Git’s own behavior, keep implementation lightweight, and work with the same semantics users see in the CLI.

## Crate structure

Public exports are centralized in `src/lib.rs`, which re-exports the main entry points from module-local implementations.

- `src/apply.rs`
  - Unified diff application and `git apply` output parsing.
- `src/branch.rs`
  - Merge-base computation with logic to prefer an upstream branch when the remote is ahead.
- `src/errors.rs`
  - Shared error type for the synchronous tooling helpers.
- `src/ghost_commits.rs`
  - Snapshot/restore engine used by undo.
- `src/info.rs`
  - Async Git metadata queries and trust-root discovery.
- `src/operations.rs`
  - Shared sync helpers for running Git commands, normalizing paths, and resolving repository scope.
- `src/platform.rs`
  - Cross-platform symlink creation helper.

## Responsibilities by module

### 1. `info.rs`: async repository inspection

This module handles lightweight, read-mostly operations that are naturally async and frequently used by higher-level services.

- `get_git_repo_root`
  - Walks upward looking for `.git` without invoking Git.
  - Fast repo detection for UX and startup checks.
- `collect_git_info`
  - Runs `rev-parse HEAD`, `rev-parse --abbrev-ref HEAD`, and `remote get-url origin` in parallel after confirming the directory is a repo.
  - Produces `GitInfo { commit_hash, branch, repository_url }`.
- `get_git_remote_urls` / `get_git_remote_urls_assume_git_repo`
  - Parses `git remote -v` into a stable `BTreeMap<String, String>`.
- `get_head_commit_hash`, `get_has_changes`, `recent_commits`
  - Small convenience helpers for state surfaces and pickers.
- `default_branch_name`, `current_branch_name`, `local_git_branches`
  - Drive UI and cloud-task branch selection.
- `git_diff_to_remote`
  - Finds the closest remotely reachable base SHA and returns the diff from that SHA to the current working tree.
- `resolve_root_git_project_for_trust`
  - Resolves the **main repo root**, not just the current checkout root, and supports linked worktrees without invoking `git`.

Notable implementation details:

- Read-only Git commands in this module are wrapped in a hardcoded 5-second timeout.
- `GIT_OPTIONAL_LOCKS=0` is set to reduce chances of lock-related stalls.
- Failure behavior is intentionally soft: most functions return `Option<_>` or empty vectors rather than detailed errors.

### 2. `apply.rs`: patch execution and diagnostics

This module is a thin adapter around `git apply`.

- `ApplyGitRequest`
  - Inputs: `cwd`, `diff`, `revert`, `preflight`.
- `apply_git_patch`
  - Writes the diff to a temporary file.
  - Resolves the repository root with `git rev-parse --show-toplevel`.
  - Optionally stages touched files before revert mode.
  - Runs either:
    - `git apply --check ...` for preflight, or
    - `git apply --3way ...` for real apply.
  - Returns `ApplyGitResult` with exit code, categorized paths, stdout/stderr, and a shell-rendered command string for logs.
- `extract_paths_from_patch`
  - Parses `diff --git` headers, including quoted paths and C-style escapes.
- `stage_paths`
  - Best-effort `git add -- <existing paths>` to reduce index mismatch on revert.
- `parse_git_apply_output`
  - Converts unstructured stdout/stderr into:
    - `applied_paths`
    - `skipped_paths`
    - `conflicted_paths`

The parser is the main value-add here. The actual Git invocation is straightforward; the module’s complexity is in turning many Git message variants into a stable machine-readable result.

### 3. `ghost_commits.rs`: snapshot and undo engine

This is the most important and most complex part of the crate.

Its job is to snapshot the current working tree into an unreferenced commit while preserving enough metadata to safely restore later.

Core public types:

- `CreateGhostCommitOptions`
- `RestoreGhostCommitOptions`
- `GhostSnapshotConfig`
- `GhostSnapshotReport`
- `LargeUntrackedDir`
- `IgnoredUntrackedFile`

Core public functions:

- `create_ghost_commit`
- `create_ghost_commit_with_report`
- `capture_ghost_snapshot_report`
- `restore_ghost_commit`
- `restore_ghost_commit_with_options`
- `restore_to_commit`

Key responsibilities:

- Capture tracked changes, deletions, and selected untracked files into a detached commit.
- Preserve ignored or untracked files/directories that existed before the snapshot so undo does not delete user data.
- Exclude oversized untracked files and directories from the snapshot, while still reporting and preserving them.
- Support being called from a repo subdirectory and limiting restore scope to that subtree.

Important design choices:

- Uses a **temporary Git index** via `GIT_INDEX_FILE`, so snapshot creation does not disturb the user’s real index.
- Uses `git read-tree`, `git add --all`, `git write-tree`, and `git commit-tree` plumbing commands rather than creating a real branch or ref.
- Uses `git restore --source <snapshot> --worktree -- <scope>` for restoration and intentionally avoids `--staged` to preserve user-staged changes.
- Stores preexisting untracked file/dir paths in `GhostCommit` so post-restore cleanup only removes newly introduced untracked content.

### 4. `branch.rs`: merge-base helper for review flows

`merge_base_with_head(repo_path, branch)` is a specialized utility for review prompts.

It:

- Ensures the path is a Git repo.
- Resolves `HEAD`.
- Resolves the target branch.
- Checks whether the branch’s upstream is ahead.
- If so, prefers the upstream ref over the local branch when computing merge-base.

This is not a general branch-management API; it exists to improve “review against base branch” behavior when the local tracking branch is stale.

### 5. `operations.rs`: sync execution and path safety layer

This module underpins `branch.rs` and `ghost_commits.rs`.

Important helpers:

- `ensure_git_repository`
- `resolve_head`
- `resolve_repository_root`
- `normalize_relative_path`
- `repo_subdir`
- `run_git_for_status`
- `run_git_for_stdout`
- `run_git_for_stdout_all`

This module does most of the correctness work around:

- mapping Git exit code `128` to `NotAGitRepository`,
- preserving command context in errors,
- rejecting absolute and escaping paths,
- calculating subdirectory-scoped repo prefixes.

### 6. `platform.rs`: symlink creation helper

This is a small portability wrapper:

- Unix: always uses `std::os::unix::fs::symlink`.
- Windows: inspects source metadata and picks `symlink_dir` vs `symlink_file`.

It is exported from `lib.rs`, but in this crate it is auxiliary rather than central.

## Main public APIs

### Metadata and repository helpers

- `collect_git_info(cwd) -> Option<GitInfo>`
- `get_git_repo_root(base_dir) -> Option<PathBuf>`
- `resolve_root_git_project_for_trust(fs, cwd) -> Option<AbsolutePathBuf>`
- `get_git_remote_urls(cwd) -> Option<BTreeMap<String, String>>`
- `get_head_commit_hash(cwd) -> Option<GitSha>`
- `get_has_changes(cwd) -> Option<bool>`
- `recent_commits(cwd, limit) -> Vec<CommitLogEntry>`
- `git_diff_to_remote(cwd) -> Option<GitDiffToRemote>`
- `default_branch_name(cwd) -> Option<String>`
- `current_branch_name(cwd) -> Option<String>`
- `local_git_branches(cwd) -> Vec<String>`

### Patch application

- `apply_git_patch(&ApplyGitRequest) -> io::Result<ApplyGitResult>`
- `extract_paths_from_patch(diff) -> Vec<String>`
- `parse_git_apply_output(stdout, stderr) -> (applied, skipped, conflicted)`
- `stage_paths(git_root, diff) -> io::Result<()>`

### Undo snapshots

- `create_ghost_commit(&CreateGhostCommitOptions) -> Result<GhostCommit, GitToolingError>`
- `create_ghost_commit_with_report(...) -> Result<(GhostCommit, GhostSnapshotReport), GitToolingError>`
- `capture_ghost_snapshot_report(...) -> Result<GhostSnapshotReport, GitToolingError>`
- `restore_ghost_commit(repo_path, &GhostCommit) -> Result<(), GitToolingError>`
- `restore_ghost_commit_with_options(...) -> Result<(), GitToolingError>`
- `restore_to_commit(repo_path, commit_id) -> Result<(), GitToolingError>`

### Branch helper

- `merge_base_with_head(repo_path, branch) -> Result<Option<String>, GitToolingError>`

## End-to-end flows

### Flow A: collect lightweight Git metadata

1. Confirm the directory is a Git repository with `git rev-parse --git-dir`.
2. Run commit, branch, and origin URL queries in parallel.
3. Parse stdout into `GitInfo`.
4. Suppress errors/timeouts by returning `None`.

This is optimized for telemetry, UI, and metadata attachment rather than for deep diagnostics.

### Flow B: apply a patch

1. Resolve repo root from `cwd`.
2. Write the unified diff to a temp file.
3. If revert mode is active and not preflight, best-effort stage existing touched paths.
4. Execute `git apply --check` or `git apply --3way`.
5. Parse command output into applied/skipped/conflicted path sets.
6. Return both raw stdout/stderr and structured results.

This lets higher layers decide whether an apply outcome is success, partial success, or hard failure.

### Flow C: create a ghost commit

1. Validate repo and resolve repo root plus optional subdirectory prefix.
2. Capture porcelain status (`git status --porcelain=2 -z --untracked-files=all`).
3. Split state into:
   - tracked paths,
   - snapshot-worthy untracked files,
   - preserved untracked/ignored files,
   - warning-only large untracked files/dirs.
4. Create a temporary index with `GIT_INDEX_FILE`.
5. Seed it from `HEAD` with `git read-tree` when a parent exists.
6. `git add --all` tracked and selected untracked paths into the temp index.
7. Force-add caller-specified ignored paths.
8. `git write-tree` then `git commit-tree`.
9. Return a `GhostCommit` containing:
   - snapshot commit ID,
   - optional parent,
   - preexisting untracked file list,
   - preexisting untracked dir list.

### Flow D: restore a ghost commit

1. Re-scan current untracked/ignored content before restore.
2. Restore worktree content from the snapshot commit with `git restore`.
3. Remove only new untracked paths that were not present at snapshot time.
4. Preserve:
   - preexisting untracked files,
   - preexisting ignored files,
   - large-file and large-dir exclusions,
   - default ignored directories such as `node_modules` and `.venv`.

This is deliberately conservative and biased toward avoiding data loss.

## Dependency analysis

### Workspace/internal dependencies

- `codex-protocol`
  - Supplies `GitSha` and `GhostCommit`.
  - Important because snapshot identity and Git SHA modeling are part of the shared protocol layer.
- `codex-exec-server`
  - Supplies `ExecutorFileSystem` used by `resolve_root_git_project_for_trust`.
- `codex-utils-absolute-path`
  - Provides `AbsolutePathBuf` used in trust-root discovery.

### External crates

- `tokio`
  - Async process execution and timeouts in `info.rs`.
- `futures`
  - `join_all` for untracked diff generation in `git_diff_to_remote`.
- `regex` + `once_cell`
  - Static regex compilation for `git apply` output parsing.
- `serde`, `schemars`, `ts-rs`
  - Keep `GitInfo` and related protocol-facing types serializable and schema-friendly.
- `tempfile`
  - Temp patch files and temporary index storage.
- `thiserror`
  - Error ergonomics in `GitToolingError`.
- `walkdir`
  - Used in tests and exposed in the shared error enum.

### Architectural note

There are effectively two execution styles in the crate:

- **Async, lossy, timeout-bounded** helpers in `info.rs`
- **Sync, error-rich, correctness-focused** helpers in `operations.rs` and snapshot logic

That split aligns well with the product requirements:

- metadata queries should fail soft,
- snapshot/restore should fail loud and preserve details.

## Testing analysis

### In-crate tests

`apply.rs` tests cover:

- quoted and escaped diff header parsing,
- parser behavior for failed patch output,
- add/modify/revert paths,
- preflight behavior and no-side-effect guarantees.

`branch.rs` tests cover:

- normal merge-base resolution,
- preferring upstream when remote is ahead,
- missing branch handling.

`ghost_commits.rs` tests cover:

- create/restore round trip,
- repositories without an existing `HEAD`,
- custom commit messages,
- force-including ignored files,
- large untracked file exclusion,
- large untracked directory exclusion,
- subdirectory-scoped restore,
- preservation of ignored files/directories and glob matches,
- invalid escaping `force_include` paths.

### Cross-crate tests

`core/src/git_info_tests.rs` exercises behavior that is implemented in `info.rs`, including:

- `collect_git_info` in regular, detached-head, and remote-backed repos,
- `get_has_changes`,
- `recent_commits`,
- `git_diff_to_remote`,
- `resolve_root_git_project_for_trust`, including worktrees.

This is good practical coverage, but it also means part of `git-utils` behavior is validated outside the crate itself.

### Coverage quality

Strong areas:

- undo/snapshot correctness,
- data-preservation edge cases,
- patch parser normalization,
- worktree-aware trust-root resolution.

Weaker areas:

- no direct in-crate tests for every `info.rs` helper,
- timeout behavior is not explicitly validated,
- limited testing around non-UTF-8 git output and platform-specific symlink behavior.

## Design assessment

### Strengths

- **Clear separation of product concerns**
  - metadata lookup, patch application, and undo snapshotting are well isolated.
- **Data safety bias**
  - restore avoids mutating the real index and tries hard not to delete user data.
- **Pragmatic Git plumbing usage**
  - temporary-index snapshot creation is a strong design choice for correctness.
- **Worktree awareness**
  - trust-root resolution handles linked worktrees without requiring a working `git` binary.
- **Structured patch diagnostics**
  - `parse_git_apply_output` makes raw Git output usable by UI and automation.

### Trade-offs and limitations

- **Mixed error model**
  - `info.rs` returns `Option`, while snapshot/branch helpers return `Result`.
  - This is defensible, but it makes the crate feel like two API styles under one roof.
- **System Git dependency**
  - correctness matches user CLI behavior, but all functions depend on an available `git` binary.
- **Hardcoded defaults**
  - timeouts and ignored directory names are embedded in code rather than fully configurable here.
- **Patch application uses `io::Result`**
  - unlike the rest of the sync tooling, it does not use `GitToolingError`, so consumers have a different failure surface.

## Known design patterns

- **Facade via `lib.rs`**
  - a narrow public API over several focused modules.
- **Builder-style options**
  - `CreateGhostCommitOptions` and `RestoreGhostCommitOptions` use fluent setters.
- **Command wrapper**
  - `operations.rs` centralizes Git command execution and error shaping.
- **Temporary-index snapshotting**
  - a Git-specific variant of copy-on-write state capture.
- **Best-effort cleanup/preservation**
  - restore and apply favor preserving user state over strict cleanup.

## Where this crate is used

Key consumers in the workspace:

- `core`
  - ghost snapshots for undo, trust checks, metadata, review prompt merge-base resolution.
- `app-server`
  - remote-context diff generation and trust handling.
- `cloud-tasks` / `cloud-tasks-client`
  - branch selection and patch preflight/apply.
- `chatgpt`
  - local application of task diffs.
- `analytics`, `rollout`, `tui`, `exec`, `secrets`
  - repo discovery and metadata collection.

This confirms the crate is infrastructure-level and shared across both local UX and server/runtime flows.

## Open questions

1. **Should `apply.rs` use `GitToolingError` instead of `io::Result`?**
   - The current API is workable, but it breaks consistency with the other sync modules.

2. **Should timeout values be configurable?**
   - `info.rs` hardcodes a 5-second timeout. Large monorepos or remote filesystem setups may need tuning.

3. **Should `info.rs` expose richer diagnostics for callers that care?**
   - Returning `Option` keeps callers simple, but hides whether failure came from timeout, missing Git, bad cwd, or parse issues.

4. **Should more `info.rs` tests live in this crate?**
   - Current cross-crate coverage is useful but increases coupling to `core`.

5. **Is `GhostSnapshotConfig::disable_warnings` in the right layer?**
   - The config field influences higher-level warning emission, but the crate itself mostly just produces the report.

6. **Should default ignored directory names be configurable at the crate boundary?**
   - Hardcoded entries like `node_modules`, `.venv`, and `dist` are practical, but policy-like.

7. **Should `create_symlink` remain public here?**
   - It is exported, but it does not appear central to the crate’s primary Git responsibilities.

## Bottom line

`codex-git-utils` is a practical Git integration crate with one especially important capability: **safe undo via detached ghost commits built from a temporary index**. The surrounding metadata and patching helpers are smaller, but they support the same overall product goal: let higher-level Codex components understand repository state, apply changes, and recover from them safely.

The crate’s strongest engineering quality is its bias toward **preserving user data and matching real Git behavior**. Its main rough edges are API-style inconsistency and a few policy decisions that are currently hardcoded rather than configurable.
