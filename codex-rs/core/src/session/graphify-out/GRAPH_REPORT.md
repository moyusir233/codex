# Graph Report - .  (2026-04-23)

## Corpus Check
- Corpus is ~41,381 words - fits in a single context window. You may not need a graph.

## Summary
- 495 nodes · 1457 edges · 9 communities detected
- Extraction: 61% EXTRACTED · 39% INFERRED · 0% AMBIGUOUS · INFERRED: 569 edges (avg confidence: 0.8)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- [[_COMMUNITY_Turn Token Item|Turn Token Item]]
- [[_COMMUNITY_Turn Context Network|Turn Context Network]]
- [[_COMMUNITY_History Turn Initial|History Turn Initial]]
- [[_COMMUNITY_Permissions Request When|Permissions Request When]]
- [[_COMMUNITY_Mcp User Turn|Mcp User Turn]]
- [[_COMMUNITY_Session Config Turn|Session Config Turn]]
- [[_COMMUNITY_Context Thread Rollout|Context Thread Rollout]]
- [[_COMMUNITY_Shutdown Wait Submit|Shutdown Wait Submit]]
- [[_COMMUNITY_Task Agent Identity|Task Agent Identity]]

## God Nodes (most connected - your core abstractions)
1. `Session` - 120 edges
2. `make_session_and_context()` - 84 edges
3. `make_session_and_context_with_rx()` - 39 edges
4. `submission_loop()` - 34 edges
5. `run_turn()` - 26 edges
6. `thread_rollback()` - 18 edges
7. `try_run_sampling_request()` - 16 edges
8. `Codex` - 15 edges
9. `TurnContext` - 15 edges
10. `thread_rollback_recomputes_previous_turn_settings_and_reference_context_from_replay()` - 13 edges

## Surprising Connections (you probably didn't know these)
- `list_skills()` --calls--> `errors_to_info()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/session/handlers.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/session/mod.rs
- `list_skills()` --calls--> `skills_to_info()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/session/handlers.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/session/mod.rs
- `process_compacted_history_preserves_separate_guardian_developer_message()` --calls--> `make_session_and_context()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/session/tests/guardian_tests.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/session/tests.rs
- `review()` --calls--> `spawn_review_thread()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/session/handlers.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/session/review.rs
- `realtime_conversation_list_voices_emits_builtin_list()` --calls--> `realtime_conversation_list_voices()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/session/tests.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/session/handlers.rs

## Communities

### Community 0 - "Turn Token Item"
Cohesion: 0.03
Nodes (34): Session, fail_agent_identity_registration_emits_error_without_shutdown(), get_base_instructions_no_user_content(), prepend_pending_input_keeps_older_tail_ahead_of_newer_input(), recompute_token_usage_updates_model_context_window(), recompute_token_usage_uses_session_base_instructions(), record_model_warning_appends_user_message(), task_finish_emits_turn_item_lifecycle_for_leftover_pending_user_input() (+26 more)

### Community 1 - "Turn Context Network"
Cohesion: 0.05
Nodes (56): submission_dispatch_span(), abort_gracefully_emits_turn_aborted_only(), abort_regular_task_emits_turn_aborted_only(), build_initial_context_emits_thread_start_skill_warning_on_repeated_builds(), build_initial_context_omits_default_image_save_location_with_image_history(), build_initial_context_omits_default_image_save_location_without_image_history(), build_initial_context_prepends_model_switch_message(), build_initial_context_restates_realtime_start_when_reference_context_is_missing() (+48 more)

### Community 2 - "History Turn Initial"
Cohesion: 0.09
Nodes (50): ActiveReplaySegment, finalize_active_segment(), RolloutReconstruction, Session, assistant_message(), inter_agent_assistant_message(), reconstruct_history_legacy_compaction_without_replacement_history_clears_later_reference_context_item(), reconstruct_history_legacy_compaction_without_replacement_history_does_not_inject_current_initial_context() (+42 more)

### Community 3 - "Permissions Request When"
Cohesion: 0.06
Nodes (43): expect_text_output(), guardian_allows_shell_additional_permissions_requests_past_policy_validation(), guardian_allows_unified_exec_additional_permissions_requests_past_policy_validation(), guardian_subagent_does_not_inherit_parent_exec_policy_rules(), process_compacted_history_preserves_separate_guardian_developer_message(), request_permissions_guardian_review_stops_when_cancelled(), request_permissions_routes_to_guardian_when_reviewer_is_enabled(), shell_handler_allows_sticky_turn_permissions_without_inline_request_permissions_feature() (+35 more)

### Community 4 - "Mcp User Turn"
Cohesion: 0.08
Nodes (34): add_to_history(), clean_background_terminals(), compact(), drop_memories(), dynamic_tool_response(), exec_approval(), get_history_entry_request(), inter_agent_communication() (+26 more)

### Community 5 - "Session Config Turn"
Cohesion: 0.06
Nodes (20): thread_title_from_state_db(), spawn_review_thread(), AppServerClientMetadata, Session, SessionConfiguration, SessionSettingsUpdate, danger_full_access_tool_attempts_do_not_enforce_managed_network(), danger_full_access_turns_do_not_expose_managed_network_proxy() (+12 more)

### Community 6 - "Context Thread Rollout"
Cohesion: 0.17
Nodes (16): thread_rollback(), assistant_message(), attach_rollout_recorder(), fork_startup_context_then_first_turn_diff_snapshot(), record_context_updates_and_set_reference_context_item_persists_baseline_without_emitting_diffs(), record_context_updates_and_set_reference_context_item_persists_full_reinjection_to_rollout(), record_context_updates_and_set_reference_context_item_persists_split_file_system_policy_to_rollout(), record_context_updates_and_set_reference_context_item_reinjects_full_context_after_clear() (+8 more)

### Community 7 - "Shutdown Wait Submit"
Cohesion: 0.1
Nodes (14): Codex, CodexSpawnArgs, CodexSpawnOk, completed_session_loop_termination(), errors_to_info(), PreviousTurnSettings, session_loop_termination_from_handle(), skills_to_info() (+6 more)

### Community 8 - "Task Agent Identity"
Cohesion: 0.15
Nodes (14): Session, cached_agent_task_for_current_identity_clears_stale_task(), curve25519_secret_key_from_signing_key_for_tests(), encrypt_task_id_for_identity(), fake_id_token(), generate_test_signing_key(), make_agent_identity_session_and_context_with_rx(), make_chatgpt_auth() (+6 more)

## Knowledge Gaps
- **11 isolated node(s):** `SessionSettingsUpdate`, `AppServerClientMetadata`, `PreviousTurnSettings`, `CodexSpawnOk`, `CodexSpawnArgs` (+6 more)
  These have ≤1 connection - possible missing edges or undocumented components.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Session` connect `Turn Token Item` to `Turn Context Network`, `History Turn Initial`, `Permissions Request When`, `Mcp User Turn`, `Session Config Turn`, `Context Thread Rollout`, `Shutdown Wait Submit`?**
  _High betweenness centrality (0.266) - this node is a cross-community bridge._
- **Why does `make_session_and_context()` connect `History Turn Initial` to `Turn Token Item`, `Turn Context Network`, `Permissions Request When`, `Mcp User Turn`, `Session Config Turn`, `Context Thread Rollout`, `Shutdown Wait Submit`?**
  _High betweenness centrality (0.104) - this node is a cross-community bridge._
- **Why does `run_turn()` connect `Turn Token Item` to `Turn Context Network`, `History Turn Initial`, `Permissions Request When`, `Session Config Turn`, `Context Thread Rollout`, `Shutdown Wait Submit`, `Task Agent Identity`?**
  _High betweenness centrality (0.040) - this node is a cross-community bridge._
- **Are the 29 inferred relationships involving `make_session_and_context()` (e.g. with `record_initial_history_resumed_bare_turn_context_does_not_hydrate_previous_turn_settings()` and `record_initial_history_resumed_hydrates_previous_turn_settings_from_lifecycle_turn_with_missing_turn_context_id()`) actually correct?**
  _`make_session_and_context()` has 29 INFERRED edges - model-reasoned connections that need verification._
- **Are the 2 inferred relationships involving `make_session_and_context_with_rx()` (e.g. with `.new()` and `request_permissions_guardian_review_stops_when_cancelled()`) actually correct?**
  _`make_session_and_context_with_rx()` has 2 INFERRED edges - model-reasoned connections that need verification._
- **Are the 4 inferred relationships involving `submission_loop()` (e.g. with `.send_event_raw()` and `.spawn_internal()`) actually correct?**
  _`submission_loop()` has 4 INFERRED edges - model-reasoned connections that need verification._
- **Are the 19 inferred relationships involving `run_turn()` (e.g. with `.has_pending_input()` and `.record_context_updates_and_set_reference_context_item()`) actually correct?**
  _`run_turn()` has 19 INFERRED edges - model-reasoned connections that need verification._