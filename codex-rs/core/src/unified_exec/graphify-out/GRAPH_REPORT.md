# Graph Report - .  (2026-04-23)

## Corpus Check
- Corpus is ~7,902 words - fits in a single context window. You may not need a graph.

## Summary
- 154 nodes · 361 edges · 10 communities detected
- Extraction: 63% EXTRACTED · 37% INFERRED · 0% AMBIGUOUS · INFERRED: 133 edges (avg confidence: 0.8)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- [[_COMMUNITY_Exec Process Env|Exec Process Env]]
- [[_COMMUNITY_Sandbox Output Exited|Sandbox Output Exited]]
- [[_COMMUNITY_Tail Budget Head|Tail Budget Head]]
- [[_COMMUNITY_Utf8 Prefix Split|Utf8 Prefix Split]]
- [[_COMMUNITY_Process Id Processes|Process Id Processes]]
- [[_COMMUNITY_Exec Unified Completed|Exec Unified Completed]]
- [[_COMMUNITY_Process Remote Write|Process Remote Write]]
- [[_COMMUNITY_Set Deterministic Process|Set Deterministic Process]]
- [[_COMMUNITY_Exec Remote Server|Exec Remote Server]]
- [[_COMMUNITY_Noopspawnlifecycle Outputhandles Processhandle|Noopspawnlifecycle Outputhandles Processhandle]]

## God Nodes (most connected - your core abstractions)
1. `UnifiedExecProcess` - 22 edges
2. `UnifiedExecProcessManager` - 16 edges
3. `exec_command_with_tty()` - 13 edges
4. `HeadTailBuffer` - 11 edges
5. `test_session_and_turn()` - 9 edges
6. `exec_command()` - 9 edges
7. `unified_exec_uses_remote_exec_server_when_configured()` - 9 edges
8. `spawn_exit_watcher()` - 9 edges
9. `completed_pipe_commands_preserve_exit_code()` - 8 edges
10. `MockExecProcess` - 7 edges

## Surprising Connections (you probably didn't know these)
- `exec_command_with_tty()` --calls--> `generate_chunk_id()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/unified_exec/mod_tests.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/unified_exec/mod.rs
- `split_valid_utf8_prefix_respects_max_bytes_for_ascii()` --calls--> `split_valid_utf8_prefix_with_max()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/unified_exec/async_watcher_tests.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/unified_exec/async_watcher.rs
- `split_valid_utf8_prefix_avoids_splitting_utf8_codepoints()` --calls--> `split_valid_utf8_prefix_with_max()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/unified_exec/async_watcher_tests.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/unified_exec/async_watcher.rs
- `split_valid_utf8_prefix_makes_progress_on_invalid_utf8()` --calls--> `split_valid_utf8_prefix_with_max()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/unified_exec/async_watcher_tests.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/unified_exec/async_watcher.rs
- `set_deterministic_process_ids_for_tests()` --calls--> `set_deterministic_process_ids_for_tests()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/unified_exec/mod.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/unified_exec/process_manager.rs

## Communities

### Community 0 - "Exec Process Env"
Cohesion: 0.1
Nodes (17): UnifiedExecError, apply_unified_exec_env(), deterministic_process_ids_forced_for_tests(), env_overlay_for_exec_server(), exec_env_policy_from_shell_policy(), exec_server_env_for_request(), exec_server_params_for_request(), exec_server_process_id() (+9 more)

### Community 1 - "Sandbox Output Exited"
Cohesion: 0.21
Nodes (2): ProcessState, UnifiedExecProcess

### Community 2 - "Tail Budget Head"
Cohesion: 0.2
Nodes (8): HeadTailBuffer, chunk_larger_than_tail_budget_keeps_only_tail_end(), draining_resets_state(), fills_head_then_tail_across_multiple_chunks(), head_budget_zero_keeps_only_last_byte_in_tail(), keeps_prefix_and_suffix_when_over_budget(), max_bytes_zero_drops_everything(), push_chunk_preserves_prefix_and_suffix()

### Community 3 - "Utf8 Prefix Split"
Cohesion: 0.17
Nodes (12): emit_exec_end_for_unified_exec(), emit_failed_exec_end_for_unified_exec(), process_chunk(), resolve_aggregated_output(), spawn_exit_watcher(), split_valid_utf8_prefix(), split_valid_utf8_prefix_with_max(), start_streaming_output() (+4 more)

### Community 4 - "Process Id Processes"
Cohesion: 0.25
Nodes (2): generate_chunk_id(), UnifiedExecProcessManager

### Community 5 - "Exec Unified Completed"
Cohesion: 0.38
Nodes (11): completed_commands_do_not_persist_sessions(), exec_command(), multi_unified_exec_sessions(), requests_with_large_timeout_are_capped(), reusing_completed_process_returns_unknown_process(), test_session_and_turn(), TestSpawnLifecycle, unified_exec_pause_blocks_yield_timeout() (+3 more)

### Community 6 - "Process Remote Write"
Cohesion: 0.23
Nodes (5): MockExecProcess, remote_process(), remote_process_waits_for_early_exit_event(), remote_write_closed_stdin_marks_process_exited(), remote_write_unknown_process_marks_process_exited()

### Community 7 - "Set Deterministic Process"
Cohesion: 0.18
Nodes (8): clamp_yield_time(), ExecCommandRequest, ProcessEntry, ProcessStore, set_deterministic_process_ids_for_tests(), UnifiedExecContext, WriteStdinRequest, set_deterministic_process_ids_for_tests()

### Community 8 - "Exec Remote Server"
Cohesion: 0.38
Nodes (7): completed_pipe_commands_preserve_exit_code(), exec_command_with_tty(), remote_exec_server_rejects_inherited_fd_launches(), shell_env(), test_exec_request(), unified_exec_uses_remote_exec_server_when_configured(), UnifiedExecProcessManager

### Community 9 - "Noopspawnlifecycle Outputhandles Processhandle"
Cohesion: 0.4
Nodes (4): NoopSpawnLifecycle, OutputHandles, ProcessHandle, SpawnLifecycle

## Knowledge Gaps
- **9 isolated node(s):** `ExecCommandRequest`, `WriteStdinRequest`, `ProcessEntry`, `SpawnLifecycle`, `NoopSpawnLifecycle` (+4 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **Thin community `Sandbox Output Exited`** (19 nodes): `ProcessState`, `.exited()`, `.failed()`, `process_state.rs`, `UnifiedExecProcess`, `.check_for_sandbox_denial()`, `.check_for_sandbox_denial_with_text()`, `.drop()`, `.exit_code()`, `.fmt()`, `.from_exec_server_started()`, `.from_spawned()`, `.has_exited()`, `.sandbox_type()`, `.signal_exit()`, `.snapshot_output()`, `.spawn_exec_server_output_task()`, `.spawn_local_output_task()`, `.write()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Process Id Processes`** (18 nodes): `.process_failed()`, `generate_chunk_id()`, `.remove()`, `UnifiedExecProcessManager`, `.collect_output_until_deadline()`, `.exec_command()`, `.extend_deadlines_while_paused()`, `.prepare_process_handles()`, `.prune_processes_if_needed()`, `.refresh_process_state()`, `.release_process_id()`, `.store_process()`, `.terminate_all_processes()`, `.unregister_network_approval_for_entry()`, `.wait_for_pause_change()`, `.write_stdin()`, `.failure_message()`, `.terminate()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `UnifiedExecProcess` connect `Sandbox Output Exited` to `Tail Budget Head`, `Utf8 Prefix Split`, `Process Id Processes`, `Exec Remote Server`, `Noopspawnlifecycle Outputhandles Processhandle`?**
  _High betweenness centrality (0.145) - this node is a cross-community bridge._
- **Why does `UnifiedExecProcessManager` connect `Process Id Processes` to `Exec Process Env`, `Exec Remote Server`?**
  _High betweenness centrality (0.077) - this node is a cross-community bridge._
- **Why does `exec_command_with_tty()` connect `Exec Remote Server` to `Exec Process Env`, `Sandbox Output Exited`, `Tail Budget Head`, `Process Id Processes`, `Exec Unified Completed`?**
  _High betweenness centrality (0.074) - this node is a cross-community bridge._
- **Are the 9 inferred relationships involving `exec_command_with_tty()` (e.g. with `.allocate_process_id()` and `.new()`) actually correct?**
  _`exec_command_with_tty()` has 9 INFERRED edges - model-reasoned connections that need verification._
- **What connects `ExecCommandRequest`, `WriteStdinRequest`, `ProcessEntry` to the rest of the system?**
  _9 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Exec Process Env` be split into smaller, more focused modules?**
  _Cohesion score 0.1 - nodes in this community are weakly interconnected._