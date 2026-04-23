# Graph Report - /data00/home/yangchengrun/codex/codex-rs/core/src/config  (2026-04-23)

## Corpus Check
- Corpus is ~35,533 words - fits in a single context window. You may not need a graph.

## Summary
- 560 nodes · 2199 edges · 12 communities detected
- Extraction: 57% EXTRACTED · 43% INFERRED · 0% AMBIGUOUS · INFERRED: 936 edges (avg confidence: 0.8)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- [[_COMMUNITY_Config Loading Tests|Config Loading Tests]]
- [[_COMMUNITY_Config Editing|Config Editing]]
- [[_COMMUNITY_Config IO & Paths|Config IO & Paths]]
- [[_COMMUNITY_Config Layering|Config Layering]]
- [[_COMMUNITY_Edit Document Model|Edit Document Model]]
- [[_COMMUNITY_Permissions Profiles|Permissions Profiles]]
- [[_COMMUNITY_Network Proxy Spec|Network Proxy Spec]]
- [[_COMMUNITY_Config Service|Config Service]]
- [[_COMMUNITY_Agent Roles|Agent Roles]]
- [[_COMMUNITY_Managed Features|Managed Features]]
- [[_COMMUNITY_Schema Docs|Schema Docs]]
- [[_COMMUNITY_Schema Tests|Schema Tests]]

## God Nodes (most connected - your core abstractions)
1. `ConfigEditsBuilder` - 27 edges
2. `load_global_mcp_servers()` - 22 edges
3. `Config` - 20 edges
4. `NetworkProxySpec` - 17 edges
5. `ConfigDocument` - 15 edges
6. `load_agent_roles()` - 13 edges
7. `ConfigBuilder` - 13 edges
8. `compile_permission_profile()` - 13 edges
9. `ConfigService` - 12 edges
10. `ManagedFeatures` - 12 edges

## Surprising Connections (you probably didn't know these)
- `toml_value_to_item_handles_nested_config_tables()` --calls--> `toml_value_to_item()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/config/service_tests.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/config/service.rs
- `test_precedence_fixture_with_gpt3_profile()` --calls--> `resolve_mcp_oauth_credentials_store_mode()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/config/config_tests.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/config/mod.rs
- `test_precedence_fixture_with_zdr_profile()` --calls--> `resolve_mcp_oauth_credentials_store_mode()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/config/config_tests.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/config/mod.rs
- `test_precedence_fixture_with_gpt5_profile()` --calls--> `resolve_mcp_oauth_credentials_store_mode()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/config/config_tests.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/config/mod.rs
- `managed_config_overrides_oauth_store_mode()` --calls--> `deserialize_config_toml_with_base()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/config/config_tests.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/config/mod.rs

## Hyperedges (group relationships)
- **Config schema generation workflow** — schema_configtoml_type, schema_config_toml_path, schema_committed_schema_file [INFERRED 0.74]
- **Schema regeneration trigger on ConfigToml changes** — schema_configtoml_type, schema_nested_config_types, schema_just_write_config_schema [EXTRACTED 0.86]

## Communities

### Community 0 - "Config Loading Tests"
Cohesion: 0.06
Nodes (129): accepts_amazon_bedrock_aws_profile_override(), active_project_does_not_match_configured_alias_for_canonical_cwd(), add_dir_override_extends_workspace_writable_roots(), cli_override_sets_compact_prompt(), cli_override_takes_precedence_over_profile_sandbox_mode(), config_defaults_to_file_cli_auth_store_mode(), config_honors_explicit_file_oauth_store_mode(), config_loads_allow_login_shell_from_toml() (+121 more)

### Community 1 - "Config Editing"
Cohesion: 0.06
Nodes (59): load_global_mcp_servers_accepts_legacy_ms_field(), replace_mcp_servers_round_trips_entries(), replace_mcp_servers_serializes_cwd(), replace_mcp_servers_serializes_disabled_flag(), replace_mcp_servers_serializes_env_sorted(), replace_mcp_servers_serializes_env_vars(), replace_mcp_servers_serializes_required_flag(), replace_mcp_servers_serializes_sourced_env_vars() (+51 more)

### Community 2 - "Config IO & Paths"
Cohesion: 0.12
Nodes (50): agent_role_file_metadata_overrides_config_toml_metadata(), agent_role_file_name_takes_precedence_over_config_key(), agent_role_file_without_developer_instructions_is_dropped_with_warning(), agent_role_relative_config_file_resolves_against_config_toml(), agent_role_without_description_after_merge_is_dropped_with_warning(), approvals_reviewer_can_be_set_in_config_without_guardian_approval(), approvals_reviewer_can_be_set_in_profile_without_guardian_approval(), approvals_reviewer_defaults_to_manual_only_without_guardian_feature() (+42 more)

### Community 3 - "Config Layering"
Cohesion: 0.08
Nodes (36): AgentRoleConfig, apply_managed_filesystem_constraints(), apply_requirement_constrained_value(), Config, ConfigOverrides, constrain_mcp_servers(), deserialize_config_toml_with_base(), ensure_no_inline_bearer_tokens() (+28 more)

### Community 4 - "Edit Document Model"
Cohesion: 0.14
Nodes (26): apply(), apply_blocking(), array_from_env_vars(), array_from_iter(), ConfigDocument, ConfigEdit, ensure_table_for_read(), ensure_table_for_write() (+18 more)

### Community 5 - "Permissions Profiles"
Cohesion: 0.14
Nodes (38): compile_filesystem_access_path(), compile_filesystem_path(), compile_filesystem_permission(), compile_network_sandbox_policy(), compile_permission_profile(), compile_read_write_glob_path(), compile_scoped_filesystem_path(), compile_scoped_filesystem_pattern() (+30 more)

### Community 6 - "Network Proxy Spec"
Cohesion: 0.11
Nodes (19): apply_exec_policy_network_rules(), NetworkProxySpec, StartedNetworkProxy, StaticNetworkProxyReloader, allow_only_requirements_do_not_create_deny_constraints_in_full_access(), build_state_with_audit_metadata_threads_metadata_to_state(), danger_full_access_keeps_managed_allowlist_and_denylist_fixed(), deny_only_requirements_do_not_create_allow_constraints_in_full_access() (+11 more)

### Community 7 - "Config Service"
Cohesion: 0.16
Nodes (18): apply_merge(), clear_path(), compute_override_metadata(), ConfigService, ConfigServiceError, create_empty_user_layer(), find_effective_layer(), first_overridden_edit() (+10 more)

### Community 8 - "Agent Roles"
Cohesion: 0.3
Nodes (18): agent_role_config_from_toml(), agents_toml_from_layer(), collect_agent_role_files(), discover_agent_roles_in_dir(), load_agent_roles(), load_agent_roles_without_layers(), merge_missing_role_fields(), normalize_agent_role_description() (+10 more)

### Community 9 - "Managed Features"
Cohesion: 0.24
Nodes (10): feature_requirements_normalize_runtime_feature_mutations(), explicit_feature_settings_in_config(), feature_requirements_display(), ManagedFeatures, normalize_candidate(), parse_feature_requirements(), validate_explicit_feature_settings_in_config_toml(), validate_feature_requirements_in_config_toml() (+2 more)

### Community 10 - "Schema Docs"
Cohesion: 0.25
Nodes (9): Committed schema file: core/config.schema.json, Config JSON Schema (document), ~/.codex/config.toml, ConfigToml type, Editor integration, Generated JSON Schema for config.toml, Command: just write-config-schema, Nested config types (included in ConfigToml) (+1 more)

### Community 11 - "Schema Tests"
Cohesion: 0.67
Nodes (2): config_schema_matches_fixture(), trim_single_trailing_newline()

## Knowledge Gaps
- **4 isolated node(s):** `~/.codex/config.toml`, `Editor integration`, `Nested config types (included in ConfigToml)`, `Rationale: commit generated schema to support editor integration`
  These have ≤1 connection - possible missing edges or undocumented components.
- **Thin community `Schema Tests`** (4 nodes): `schema_tests.rs`, `config_schema_matches_fixture()`, `schema_tests.rs`, `trim_single_trailing_newline()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `ConfigEditsBuilder` connect `Config Editing` to `Edit Document Model`?**
  _High betweenness centrality (0.051) - this node is a cross-community bridge._
- **Why does `Config` connect `Config Layering` to `Config Loading Tests`, `Config IO & Paths`?**
  _High betweenness centrality (0.035) - this node is a cross-community bridge._
- **Why does `NetworkProxySpec` connect `Network Proxy Spec` to `Config Layering`?**
  _High betweenness centrality (0.022) - this node is a cross-community bridge._
- **Are the 18 inferred relationships involving `load_global_mcp_servers()` (e.g. with `.new()` and `.get()`) actually correct?**
  _`load_global_mcp_servers()` has 18 INFERRED edges - model-reasoned connections that need verification._
- **What connects `~/.codex/config.toml`, `Editor integration`, `Nested config types (included in ConfigToml)` to the rest of the system?**
  _4 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Config Loading Tests` be split into smaller, more focused modules?**
  _Cohesion score 0.06 - nodes in this community are weakly interconnected._
- **Should `Config Editing` be split into smaller, more focused modules?**
  _Cohesion score 0.06 - nodes in this community are weakly interconnected._