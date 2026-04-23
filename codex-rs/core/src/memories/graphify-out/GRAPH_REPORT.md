# Graph Report - /data00/home/yangchengrun/codex/codex-rs/core/src/memories  (2026-04-23)

## Corpus Check
- Corpus is ~10,159 words - fits in a single context window. You may not need a graph.

## Summary
- 187 nodes · 434 edges · 11 communities detected
- Extraction: 81% EXTRACTED · 19% INFERRED · 0% AMBIGUOUS · INFERRED: 83 edges (avg confidence: 0.8)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- [[_COMMUNITY_Pipeline Overview|Pipeline Overview]]
- [[_COMMUNITY_Dispatch & Locks|Dispatch & Locks]]
- [[_COMMUNITY_Phase 1 Extraction|Phase 1 Extraction]]
- [[_COMMUNITY_Storage & Artifacts|Storage & Artifacts]]
- [[_COMMUNITY_Phase 2 Consolidation|Phase 2 Consolidation]]
- [[_COMMUNITY_Prompt Rendering|Prompt Rendering]]
- [[_COMMUNITY_Citation Parsing|Citation Parsing]]
- [[_COMMUNITY_Extensions Cleanup|Extensions Cleanup]]
- [[_COMMUNITY_Usage Metrics|Usage Metrics]]
- [[_COMMUNITY_Storage Tests|Storage Tests]]
- [[_COMMUNITY_Sessions|Sessions]]

## God Nodes (most connected - your core abstractions)
1. `run()` - 24 edges
2. `Phase 1: Rollout Extraction (per-thread)` - 14 edges
3. `run()` - 11 edges
4. `Phase 2: Global Consolidation` - 11 edges
5. `sync_rollout_summaries_from_memories()` - 10 edges
6. `memory_root()` - 9 edges
7. `rollout_summaries_dir()` - 9 edges
8. `ensure_layout()` - 8 edges
9. `handle()` - 8 edges
10. `build_consolidation_prompt()` - 8 edges

## Surprising Connections (you probably didn't know these)
- `run()` --calls--> `rebuild_raw_memories_file_from_memories()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/memories/phase2.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/memories/storage.rs
- `run()` --calls--> `sync_rollout_summaries_from_memories()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/memories/phase2.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/memories/storage.rs
- `sync_rollout_summaries_uses_timestamp_hash_and_sanitized_slug_filename()` --calls--> `sync_rollout_summaries_from_memories()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/memories/tests.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/memories/storage.rs
- `artifact_memories_for_phase2()` --calls--> `rollout_summary_file_stem()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/memories/phase2.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/memories/storage.rs
- `rollout_summary_file_stem_sanitizes_and_truncates_slug()` --calls--> `rollout_summary_file_stem()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/memories/storage_tests.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/memories/storage.rs

## Hyperedges (group relationships)
- **Startup Memory Pipeline Core Components** — README_memories_pipeline, README_phase1, README_phase2, README_state_db [INFERRED 0.82]
- **Phase 1 Rollout Extraction Flow عناصر** — README_phase1, README_rollout, README_concurrency_cap, README_secret_redaction, README_stage1_outputs, README_job_leasing [INFERRED 0.78]
- **Phase 2 Global Consolidation Flow** — README_phase2, README_stage1_outputs, README_memory_artifacts, README_consolidation_subagent, README_selection_diff, README_watermark [INFERRED 0.81]

## Communities

### Community 0 - "Pipeline Overview"
Cohesion: 0.07
Nodes (39): Consolidation Agent Restrictions (no approvals/network; local write only), Collaboration Disabled (prevent recursive delegation), Concurrency Cap (parallel model jobs), Internal Consolidation Sub-agent, Ephemeral Session, instructions.md (extension directory marker), Job Leasing/Claiming in State DB, Startup Memories Pipeline (Core) (+31 more)

### Community 1 - "Dispatch & Locks"
Cohesion: 0.18
Nodes (20): clear_memory_root_contents(), clear_memory_roots_contents(), clear_memory_root_contents_preserves_root_directory(), clear_memory_root_contents_rejects_symlinked_root(), completion_watermark_never_regresses_below_claimed_input_watermark(), completion_watermark_uses_claimed_watermark_when_there_are_no_memories(), completion_watermark_uses_latest_memory_timestamp_when_it_is_newer(), dispatch_marks_job_for_retry_when_rebuilding_raw_memories_fails() (+12 more)

### Community 2 - "Phase 1 Extraction"
Cohesion: 0.17
Nodes (23): aggregate_stats(), build_request_context(), claim_startup_jobs(), emit_metrics(), failed(), JobOutcome, JobResult, no_output() (+15 more)

### Community 3 - "Storage & Artifacts"
Cohesion: 0.29
Nodes (15): ensure_layout(), raw_memories_file(), rollout_summaries_dir(), prune_rollout_summaries(), raw_memories_format_error(), rebuild_raw_memories_file(), rebuild_raw_memories_file_from_memories(), retained_memories() (+7 more)

### Community 4 - "Phase 2 Consolidation"
Cohesion: 0.26
Nodes (15): memory_root(), artifact_memories_for_phase2(), Claim, Counters, emit_metrics(), emit_token_usage_metrics(), failed(), get_config() (+7 more)

### Community 5 - "Prompt Rendering"
Cohesion: 0.26
Nodes (12): build_consolidation_prompt(), build_memory_tool_developer_instructions(), build_stage_one_input_message(), parse_embedded_template(), render_memory_extensions_block(), render_phase2_input_selection(), render_removed_input_line(), render_selected_input_line() (+4 more)

### Community 6 - "Citation Parsing"
Cohesion: 0.33
Nodes (8): extract_block(), extract_ids_block(), parse_memory_citation(), parse_memory_citation_entry(), parse_memory_citation_extracts_entries_and_rollout_ids(), parse_memory_citation_supports_legacy_thread_ids(), parse_memory_citation_supports_rollout_ids(), thread_ids_from_memory_citation()

### Community 7 - "Extensions Cleanup"
Cohesion: 0.44
Nodes (8): find_old_extension_resources(), find_old_extension_resources_with_now(), finds_only_old_resources_from_extensions_with_instructions(), PendingExtensionResourceRemoval, remove_extension_resources(), RemovedExtensionResource, resource_timestamp(), memory_extensions_root()

### Community 8 - "Usage Metrics"
Cohesion: 0.54
Nodes (5): emit_metric_for_tool_read(), get_memory_kind(), memories_usage_kinds_from_invocation(), MemoriesUsageKind, shell_command_for_invocation()

### Community 9 - "Storage Tests"
Cohesion: 0.76
Nodes (5): fixed_thread_id(), rollout_summary_file_stem_sanitizes_and_truncates_slug(), rollout_summary_file_stem_uses_uuid_timestamp_and_hash_when_slug_is_empty(), rollout_summary_file_stem_uses_uuid_timestamp_and_hash_when_slug_missing(), stage1_output_with_slug()

### Community 10 - "Sessions"
Cohesion: 1.0
Nodes (1): Session

## Knowledge Gaps
- **25 isolated node(s):** `Memories Pipeline (Core) README`, `Templates Directory: codex-rs/core/templates/memories/`, `Template: stage_one_system.md (canonical, undated)`, `Template: stage_one_input.md (canonical, undated)`, `Template: consolidation.md (canonical, undated)` (+20 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **Thin community `Sessions`** (1 nodes): `Session`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `sample()` connect `Phase 1 Extraction` to `Dispatch & Locks`?**
  _High betweenness centrality (0.153) - this node is a cross-community bridge._
- **Why does `run()` connect `Phase 2 Consolidation` to `Dispatch & Locks`, `Storage & Artifacts`, `Extensions Cleanup`?**
  _High betweenness centrality (0.102) - this node is a cross-community bridge._
- **Why does `rollout_summary_file_stem()` connect `Storage & Artifacts` to `Storage Tests`, `Phase 2 Consolidation`?**
  _High betweenness centrality (0.061) - this node is a cross-community bridge._
- **Are the 13 inferred relationships involving `run()` (e.g. with `start_memories_startup_task()` and `memory_root()`) actually correct?**
  _`run()` has 13 INFERRED edges - model-reasoned connections that need verification._
- **Are the 5 inferred relationships involving `sync_rollout_summaries_from_memories()` (e.g. with `ensure_layout()` and `run()`) actually correct?**
  _`sync_rollout_summaries_from_memories()` has 5 INFERRED edges - model-reasoned connections that need verification._
- **What connects `Memories Pipeline (Core) README`, `Templates Directory: codex-rs/core/templates/memories/`, `Template: stage_one_system.md (canonical, undated)` to the rest of the system?**
  _25 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Pipeline Overview` be split into smaller, more focused modules?**
  _Cohesion score 0.07 - nodes in this community are weakly interconnected._