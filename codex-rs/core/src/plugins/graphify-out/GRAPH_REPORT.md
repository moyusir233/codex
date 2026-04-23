# Graph Report - .  (2026-04-23)

## Corpus Check
- Corpus is ~19,733 words - fits in a single context window. You may not need a graph.

## Summary
- 327 nodes · 996 edges · 12 communities detected
- Extraction: 58% EXTRACTED · 42% INFERRED · 0% AMBIGUOUS · INFERRED: 416 edges (avg confidence: 0.8)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- [[_COMMUNITY_Plugin Plugins Config|Plugin Plugins Config]]
- [[_COMMUNITY_Plugin Config Cache|Plugin Config Cache]]
- [[_COMMUNITY_Curated Sync Plugins|Curated Sync Plugins]]
- [[_COMMUNITY_Plugins Discoverable Plugin|Plugins Discoverable Plugin]]
- [[_COMMUNITY_Source Url Git|Source Url Git]]
- [[_COMMUNITY_Repo Sync Plugins|Repo Sync Plugins]]
- [[_COMMUNITY_Marketplace Add Source|Marketplace Add Source]]
- [[_COMMUNITY_Marketplace Remove Root|Marketplace Remove Root]]
- [[_COMMUNITY_Mentions Collect Explicit|Mentions Collect Explicit]]
- [[_COMMUNITY_Source Config Utc|Source Config Utc]]
- [[_COMMUNITY_Render Plugins Rs|Render Plugins Rs]]
- [[_COMMUNITY_Plugincapabilitysummary|Plugincapabilitysummary]]

## God Nodes (most connected - your core abstractions)
1. `write_file()` - 55 edges
2. `curated_plugins_repo_path()` - 32 edges
3. `PluginsManager` - 31 edges
4. `load_config()` - 24 edges
5. `add_marketplace_sync_with_cloner()` - 21 edges
6. `write_openai_curated_marketplace()` - 20 edges
7. `load_plugins_from_config()` - 16 edges
8. `sync_openai_plugins_repo_via_git()` - 16 edges
9. `write_plugin()` - 15 edges
10. `parse_marketplace_source()` - 14 edges

## Surprising Connections (you probably didn't know these)
- `list_marketplaces_includes_curated_repo_marketplace()` --calls--> `curated_plugins_repo_path()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/plugins/manager_tests.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/plugins/startup_sync.rs
- `list_marketplaces_includes_installed_marketplace_roots()` --calls--> `marketplace_install_root()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/plugins/manager_tests.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/plugins/installed_marketplaces.rs
- `list_marketplaces_uses_config_when_known_registry_is_malformed()` --calls--> `marketplace_install_root()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/plugins/manager_tests.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/plugins/installed_marketplaces.rs
- `list_marketplaces_ignores_installed_roots_missing_from_config()` --calls--> `marketplace_install_root()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/plugins/manager_tests.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/plugins/installed_marketplaces.rs
- `sync_plugins_from_remote_reconciles_cache_and_config()` --calls--> `write_file()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/plugins/manager_tests.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/plugins/test_support.rs

## Communities

### Community 0 - "Plugin Plugins Config"
Cohesion: 0.15
Nodes (52): capability_index_filters_inactive_and_zero_capability_plugins(), capability_summary_sanitizes_plugin_descriptions_to_one_line(), capability_summary_truncates_overlong_plugin_descriptions(), curated_plugin_ids_from_config_keys_reads_latest_codex_home_user_config(), effective_apps_dedupes_connector_ids_across_plugins(), featured_plugin_ids_for_config_defaults_query_param_to_codex(), featured_plugin_ids_for_config_uses_restriction_product_query_param(), init_git_repo() (+44 more)

### Community 1 - "Plugin Config Cache"
Cohesion: 0.05
Nodes (25): CachedFeaturedPluginIds, configured_plugins_from_stack(), configured_plugins_from_user_config_value(), ConfiguredMarketplace, ConfiguredMarketplaceListOutcome, ConfiguredMarketplacePlugin, ConfiguredMarketplaceUpgradeState, featured_plugin_ids_cache_key() (+17 more)

### Community 2 - "Curated Sync Plugins"
Cohesion: 0.1
Nodes (45): activate_curated_repo(), apply_zip_permissions(), curated_plugins_sha_path(), CuratedPluginsBackupArchiveResponse, emit_curated_plugins_startup_sync_counter(), emit_curated_plugins_startup_sync_final_metric(), emit_curated_plugins_startup_sync_metric(), ensure_git_success() (+37 more)

### Community 3 - "Plugins Discoverable Plugin"
Cohesion: 0.23
Nodes (23): list_tool_suggest_discoverable_plugins(), list_tool_suggest_discoverable_plugins_deduplicates_allowlisted_configured_plugin(), list_tool_suggest_discoverable_plugins_does_not_reload_marketplace_per_plugin(), list_tool_suggest_discoverable_plugins_ignores_missing_allowlisted_plugin(), list_tool_suggest_discoverable_plugins_includes_configured_plugin_ids(), list_tool_suggest_discoverable_plugins_normalizes_description(), list_tool_suggest_discoverable_plugins_omits_installed_curated_plugins(), list_tool_suggest_discoverable_plugins_returns_empty_when_plugins_feature_disabled() (+15 more)

### Community 4 - "Source Url Git"
Cohesion: 0.12
Nodes (18): expand_tilde_path(), file_url_source_is_rejected(), github_shorthand_and_git_url_normalize_to_same_source(), is_git_url(), is_ssh_git_url(), local_file_source_is_rejected(), local_path_source_parses(), looks_like_github_shorthand() (+10 more)

### Community 5 - "Repo Sync Plugins"
Cohesion: 0.18
Nodes (22): assert_curated_gmail_repo(), curated_repo_backup_archive_zip_bytes(), curated_repo_zipball_bytes(), has_plugins_clone_dirs(), mount_export_archive(), mount_github_repo_and_ref(), mount_github_zipball(), read_curated_plugins_sha_reads_trimmed_sha_file() (+14 more)

### Community 6 - "Marketplace Add Source"
Cohesion: 0.15
Nodes (20): clone_git_source(), ensure_marketplace_destination_is_inside_install_root(), marketplace_staging_root(), replace_marketplace_root(), run_git(), safe_marketplace_dir_name(), add_marketplace(), add_marketplace_sync() (+12 more)

### Community 7 - "Marketplace Remove Root"
Cohesion: 0.19
Nodes (16): installed_marketplace_roots_from_config(), marketplace_install_root(), resolve_configured_marketplace_root(), MarketplaceRemoveError, MarketplaceRemoveOutcome, MarketplaceRemoveRequest, remove_marketplace(), remove_marketplace_root() (+8 more)

### Community 8 - "Mentions Collect Explicit"
Cohesion: 0.2
Nodes (16): build_connector_slug_counts(), collect_explicit_app_ids(), collect_explicit_plugin_mentions(), collect_tool_mentions_from_messages(), collect_tool_mentions_from_messages_with_sigil(), CollectedToolMentions, collect_explicit_app_ids_dedupes_structured_and_linked_mentions(), collect_explicit_app_ids_from_linked_text_mentions() (+8 more)

### Community 9 - "Source Config Utc"
Cohesion: 0.23
Nodes (10): civil_from_days(), config_sparse_paths(), format_utc_timestamp(), installed_marketplace_root_for_source(), installed_marketplace_root_for_source_propagates_config_read_errors(), installed_marketplace_root_for_source_uses_local_source_root(), InstalledMarketplaceSource, MarketplaceInstallMetadata (+2 more)

### Community 10 - "Render Plugins Rs"
Cohesion: 0.25
Nodes (4): build_plugin_injections(), render_explicit_plugin_instructions(), render_plugins_section(), render_plugins_section_includes_descriptions_and_skill_naming_guidance()

### Community 11 - "Plugincapabilitysummary"
Cohesion: 1.0
Nodes (1): PluginCapabilitySummary

## Knowledge Gaps
- **29 isolated node(s):** `GitHubRepositorySummary`, `GitHubGitRefSummary`, `GitHubGitRefObject`, `CuratedPluginsBackupArchiveResponse`, `FeaturedPluginIdsCacheKey` (+24 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **Thin community `Plugincapabilitysummary`** (2 nodes): `PluginCapabilitySummary`, `.from()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `add_marketplace_sync_with_cloner()` connect `Marketplace Add Source` to `Plugin Plugins Config`, `Source Config Utc`, `Source Url Git`, `Marketplace Remove Root`?**
  _High betweenness centrality (0.084) - this node is a cross-community bridge._
- **Why does `PluginsManager` connect `Plugin Config Cache` to `Plugin Plugins Config`, `Plugins Discoverable Plugin`, `Marketplace Remove Root`?**
  _High betweenness centrality (0.081) - this node is a cross-community bridge._
- **Why does `local_file_source_is_rejected()` connect `Source Url Git` to `Plugin Plugins Config`?**
  _High betweenness centrality (0.034) - this node is a cross-community bridge._
- **Are the 50 inferred relationships involving `write_file()` (e.g. with `load_plugins_from_config()` and `load_plugins_loads_default_skills_and_mcp_servers()`) actually correct?**
  _`write_file()` has 50 INFERRED edges - model-reasoned connections that need verification._
- **Are the 27 inferred relationships involving `curated_plugins_repo_path()` (e.g. with `list_marketplaces_includes_curated_repo_marketplace()` and `sync_plugins_from_remote_reconciles_cache_and_config()`) actually correct?**
  _`curated_plugins_repo_path()` has 27 INFERRED edges - model-reasoned connections that need verification._
- **Are the 15 inferred relationships involving `add_marketplace_sync_with_cloner()` (e.g. with `parse_marketplace_source()` and `marketplace_install_root()`) actually correct?**
  _`add_marketplace_sync_with_cloner()` has 15 INFERRED edges - model-reasoned connections that need verification._
- **What connects `GitHubRepositorySummary`, `GitHubGitRefSummary`, `GitHubGitRefObject` to the rest of the system?**
  _29 weakly-connected nodes found - possible documentation gaps or missing edges._