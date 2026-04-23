# Graph Report - .  (2026-04-23)

## Corpus Check
- 80 files · ~75,603 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 1361 nodes · 3669 edges · 26 communities detected
- Extraction: 74% EXTRACTED · 26% INFERRED · 0% AMBIGUOUS · INFERRED: 943 edges (avg confidence: 0.8)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- [[_COMMUNITY_Tool Result Kind|Tool Result Kind]]
- [[_COMMUNITY_Exec Command Parse|Exec Command Parse]]
- [[_COMMUNITY_Constructor Meriyah Umd|Constructor Meriyah Umd]]
- [[_COMMUNITY_Agent Multi Rejects|Agent Multi Rejects]]
- [[_COMMUNITY_Approval Network Sandbox|Approval Network Sandbox]]
- [[_COMMUNITY_Shell Response Command|Shell Response Command]]
- [[_COMMUNITY_Exec Tool Kernel|Exec Tool Kernel]]
- [[_COMMUNITY_Exec Shell Policy|Exec Shell Policy]]
- [[_COMMUNITY_Js Repl Image|Js Repl Image]]
- [[_COMMUNITY_Tool Tools Mcp|Tool Tools Mcp]]
- [[_COMMUNITY_Agent Kind Handle|Agent Kind Handle]]
- [[_COMMUNITY_Search Tool Build|Search Tool Build]]
- [[_COMMUNITY_Tool Rs Kind|Tool Rs Kind]]
- [[_COMMUNITY_Csv Item Job|Csv Item Job]]
- [[_COMMUNITY_Server Resource Call|Server Resource Call]]
- [[_COMMUNITY_Shell Snapshot Maybe|Shell Snapshot Maybe]]
- [[_COMMUNITY_List Dir Entries|List Dir Entries]]
- [[_COMMUNITY_Sandbox Context Approval|Sandbox Context Approval]]
- [[_COMMUNITY_Search Limit Computer|Search Limit Computer]]
- [[_COMMUNITY_Image Code Mode|Image Code Mode]]
- [[_COMMUNITY_Plan Handle Update|Plan Handle Update]]
- [[_COMMUNITY_Run Search Limit|Run Search Limit]]
- [[_COMMUNITY_Mcp Rs Mcphandler|Mcp Rs Mcphandler]]
- [[_COMMUNITY_Tooleventctx New|Tooleventctx New]]
- [[_COMMUNITY_Codemodeimagedetail Protocol|Codemodeimagedetail Protocol]]
- [[_COMMUNITY_Code Mode Functioncalloutputcontentitem|Code Mode Functioncalloutputcontentitem]]

## God Nodes (most connected - your core abstractions)
1. `invocation()` - 63 edges
2. `function_payload()` - 60 edges
3. `thread_manager()` - 50 edges
4. `can_run_js_repl_runtime_tests()` - 42 edges
5. `me()` - 42 edges
6. `j()` - 38 edges
7. `JsReplManager` - 38 edges
8. `de()` - 33 edges
9. `lt()` - 31 edges
10. `fe()` - 29 edges

## Surprising Connections (you probably didn't know these)
- `parse_arguments_handles_empty_and_json()` --calls--> `parse_arguments()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/tools/handlers/mcp_resource_tests.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/tools/code_mode/wait_handler.rs
- `telemetry_preview_truncates_by_bytes()` --calls--> `telemetry_preview()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/tools/context_tests.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/tools/context.rs
- `telemetry_preview_truncates_by_lines()` --calls--> `telemetry_preview()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/tools/context_tests.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/tools/context.rs
- `build_specs_with_discoverable_tools()` --calls--> `build_tool_search_entries()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/tools/spec.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/tools/tool_search_entry.rs
- `handler_from_tools()` --calls--> `build_tool_search_entries()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/tools/handlers/tool_search.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/tools/tool_search_entry.rs

## Communities

### Community 0 - "Tool Result Kind"
Cohesion: 0.02
Nodes (47): CloseAgentArgs, CloseAgentResult, Handler, custom_tool_calls_can_derive_text_from_content_items(), custom_tool_calls_should_roundtrip_as_custom_outputs(), exec_command_tool_output_formats_truncated_response(), function_payloads_remain_function_outputs(), log_preview_uses_content_items_when_plain_text_is_missing() (+39 more)

### Community 1 - "Exec Command Parse"
Cohesion: 0.03
Nodes (69): ApplyPatchHandler, convert_apply_patch_hunks_to_protocol(), effective_patch_permissions(), file_paths_for_action(), format_update_chunks_for_progress(), hunk_source_path(), intercept_apply_patch(), approval_keys_include_move_destination() (+61 more)

### Community 2 - "Constructor Meriyah Umd"
Cohesion: 0.13
Nodes (76): _(), ae(), at(), B(), be(), bt(), c(), ce (+68 more)

### Community 3 - "Agent Multi Rejects"
Cohesion: 0.09
Nodes (92): build_agent_resume_config_clears_base_instructions(), build_agent_spawn_config_preserves_base_user_instructions(), close_agent_submits_shutdown_and_returns_previous_status(), expect_text_output(), function_payload(), handler_rejects_non_function_payloads(), history_contains_inter_agent_communication(), install_role_with_model_override() (+84 more)

### Community 4 - "Approval Network Sandbox"
Cohesion: 0.03
Nodes (49): ActiveNetworkApproval, ActiveNetworkApprovalCall, allows_network_approval_flow(), begin_network_approval(), build_blocked_request_observer(), build_network_policy_decider(), DeferredNetworkApproval, finish_deferred_network_approval() (+41 more)

### Community 5 - "Shell Response Command"
Cohesion: 0.03
Nodes (29): AbortedToolOutput, ApplyPatchToolOutput, CallToolResult, content_items_to_code_mode_result(), ExecCommandToolOutput, function_tool_response(), FunctionToolOutput, McpToolOutput (+21 more)

### Community 6 - "Exec Tool Kernel"
Cohesion: 0.05
Nodes (42): build_exec_result_content_items(), EmitImageRequest, EmitImageResult, ensure_node_version(), ExecResultMessage, ExecToolCalls, format_stderr_tail(), HostToKernel (+34 more)

### Community 7 - "Exec Shell Policy"
Cohesion: 0.04
Nodes (44): build_sandbox_command(), NodeVersion, node_version_parses_v_prefix_and_suffix(), SandboxAttempt<'a>, ApprovalKey, ShellRequest, ShellRuntime, ShellRuntimeBackend (+36 more)

### Community 8 - "Js Repl Image"
Cohesion: 0.09
Nodes (58): ApplyPatchArgumentDiffConsumer, diff_consumer_does_not_stream_json_tool_call_arguments(), diff_consumer_sends_next_update_after_buffer_interval(), diff_consumer_streams_apply_patch_changes(), write_permissions_for_paths_keep_dirs_outside_workspace_root(), write_permissions_for_paths_skip_dirs_already_writable_under_workspace_root(), write_permissions_for_paths(), emitted_image_content_item() (+50 more)

### Community 9 - "Tool Tools Mcp"
Cohesion: 0.06
Nodes (56): ToolRegistryBuilder, build_tool_call_uses_namespace_for_registry_name(), js_repl_tools_only_allows_js_repl_source_calls(), js_repl_tools_only_blocks_direct_tool_calls(), js_repl_tools_only_blocks_namespaced_js_repl_tool(), mcp_parallel_support_uses_exact_payload_server(), parallel_support_does_not_match_namespaced_local_tool_names(), ToolCall (+48 more)

### Community 10 - "Agent Kind Handle"
Cohesion: 0.04
Nodes (38): Handler, Handler, FollowupTaskArgs, handle_message_string_tool(), handle_message_submission(), message_content(), MessageDeliveryMode, SendMessageArgs (+30 more)

### Community 11 - "Search Tool Build"
Cohesion: 0.06
Nodes (63): applyReplacements(), buildMarkCommittedExpression(), buildModuleSource(), canonicalizePath(), canReadCommittedBinding(), clearLocalFileModuleCaches(), collectBindings(), collectCommittedBindings() (+55 more)

### Community 12 - "Tool Rs Kind"
Cohesion: 0.05
Nodes (28): CodeModeExecuteHandler, build_enabled_tools(), build_freeform_tool_payload(), build_function_tool_payload(), build_nested_router(), build_nested_tool_payload(), call_nested_tool(), CodeModeService (+20 more)

### Community 13 - "Csv Item Job"
Cohesion: 0.08
Nodes (38): active_item_status(), ActiveJobItem, AgentJobFailureSummary, AgentJobProgressUpdate, BatchJobHandler, build_runner_options(), build_worker_prompt(), csv_escape() (+30 more)

### Community 14 - "Server Resource Call"
Cohesion: 0.1
Nodes (29): call_tool_result_from_content(), emit_tool_call_begin(), emit_tool_call_end(), handle_list_resource_templates(), handle_list_resources(), handle_read_resource(), ListResourcesArgs, ListResourcesPayload (+21 more)

### Community 15 - "Shell Snapshot Maybe"
Cohesion: 0.17
Nodes (28): build_codex_proxy_git_ssh_command_exports(), build_override_exports(), build_override_exports_for_keys(), build_proxy_env_exports(), is_valid_shell_variable_name(), join_shell_blocks(), maybe_wrap_shell_lc_with_snapshot(), shell_single_quote() (+20 more)

### Community 16 - "List Dir Entries"
Cohesion: 0.12
Nodes (18): collect_entries(), DirEntry, DirEntryKind, format_entry_component(), format_entry_line(), format_entry_name(), list_dir_slice_with_policy(), ListDirArgs (+10 more)

### Community 17 - "Sandbox Context Approval"
Cohesion: 0.14
Nodes (8): ApplyPatchRequest, ApplyPatchRuntime, file_system_sandbox_context_omits_legacy_equivalent_policy(), file_system_sandbox_context_uses_active_attempt(), guardian_review_request_includes_patch_context(), no_sandbox_attempt_has_no_file_system_context(), wants_no_sandbox_approval_granular_respects_sandbox_flag(), send()

### Community 18 - "Search Limit Computer"
Cohesion: 0.25
Nodes (10): computer_use_tool_search_uses_larger_limit(), default_limit_for_bucket(), expanded_search_keeps_non_computer_use_servers_at_default_limit(), handler_from_tools(), limit_results_by_bucket(), mixed_search_results_coalesce_mcp_namespaces(), non_computer_use_query_keeps_default_limit_with_computer_use_tools_installed(), numbered_tools() (+2 more)

### Community 19 - "Image Code Mode"
Cohesion: 0.18
Nodes (5): code_mode_result_returns_image_url_object(), ViewImageArgs, ViewImageDetail, ViewImageHandler, ViewImageOutput

### Community 20 - "Plan Handle Update"
Cohesion: 0.22
Nodes (4): handle_update_plan(), parse_update_plan_arguments(), PlanHandler, PlanToolOutput

### Community 21 - "Run Search Limit"
Cohesion: 0.39
Nodes (5): rg_available(), run_search_handles_no_matches(), run_search_respects_limit(), run_search_returns_results(), run_search_with_glob_filter()

### Community 22 - "Mcp Rs Mcphandler"
Cohesion: 0.5
Nodes (1): McpHandler

### Community 23 - "Tooleventctx New"
Cohesion: 1.0
Nodes (1): ToolEventCtx<'a>

### Community 24 - "Codemodeimagedetail Protocol"
Cohesion: 1.0
Nodes (1): CodeModeImageDetail

### Community 25 - "Code Mode Functioncalloutputcontentitem"
Cohesion: 1.0
Nodes (1): codex_code_mode::FunctionCallOutputContentItem

## Knowledge Gaps
- **98 isolated node(s):** `ToolEventCtx`, `ToolEventCtx<'a>`, `ToolEventStage`, `ToolEventFailure`, `ExecCommandInput` (+93 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **Thin community `Mcp Rs Mcphandler`** (4 nodes): `mcp.rs`, `McpHandler`, `.handle()`, `.kind()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Tooleventctx New`** (2 nodes): `ToolEventCtx<'a>`, `.new()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Codemodeimagedetail Protocol`** (2 nodes): `CodeModeImageDetail`, `.into_protocol()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Code Mode Functioncalloutputcontentitem`** (2 nodes): `codex_code_mode::FunctionCallOutputContentItem`, `.into_protocol()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `parse_arguments()` connect `Exec Command Parse` to `Tool Result Kind`, `Agent Kind Handle`, `Tool Rs Kind`, `Csv Item Job`, `Server Resource Call`, `List Dir Entries`, `Image Code Mode`?**
  _High betweenness centrality (0.071) - this node is a cross-community bridge._
- **Why does `handle()` connect `Csv Item Job` to `Tool Result Kind`, `Exec Command Parse`, `Search Tool Build`, `Tool Tools Mcp`?**
  _High betweenness centrality (0.068) - this node is a cross-community bridge._
- **Why does `telemetry_preview()` connect `Shell Response Command` to `Search Tool Build`, `Agent Multi Rejects`?**
  _High betweenness centrality (0.045) - this node is a cross-community bridge._
- **Are the 2 inferred relationships involving `invocation()` (e.g. with `.new()` and `.default()`) actually correct?**
  _`invocation()` has 2 INFERRED edges - model-reasoned connections that need verification._
- **What connects `ToolEventCtx`, `ToolEventCtx<'a>`, `ToolEventStage` to the rest of the system?**
  _98 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Tool Result Kind` be split into smaller, more focused modules?**
  _Cohesion score 0.02 - nodes in this community are weakly interconnected._
- **Should `Exec Command Parse` be split into smaller, more focused modules?**
  _Cohesion score 0.03 - nodes in this community are weakly interconnected._