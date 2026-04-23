# Graph Report - /data00/home/yangchengrun/codex/codex-rs/core/src/agent  (2026-04-23)

## Corpus Check
- Corpus is ~13,252 words - fits in a single context window. You may not need a graph.

## Summary
- 326 nodes · 993 edges · 8 communities detected
- Extraction: 70% EXTRACTED · 30% INFERRED · 0% AMBIGUOUS · INFERRED: 297 edges (avg confidence: 0.8)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- [[_COMMUNITY_Agent Name Pool|Agent Name Pool]]
- [[_COMMUNITY_Control Tests|Control Tests]]
- [[_COMMUNITY_AgentControl Runtime|AgentControl Runtime]]
- [[_COMMUNITY_Registry & Slots|Registry & Slots]]
- [[_COMMUNITY_Role Tests|Role Tests]]
- [[_COMMUNITY_Role Layering|Role Layering]]
- [[_COMMUNITY_Mailbox|Mailbox]]
- [[_COMMUNITY_Agent Resolver|Agent Resolver]]

## God Nodes (most connected - your core abstractions)
1. `Agent (role)` - 102 edges
2. `AgentControl` - 37 edges
3. `text_input()` - 26 edges
4. `apply_role_to_config()` - 20 edges
5. `test_config_with_cli_overrides()` - 15 edges
6. `build()` - 15 edges
7. `AgentRegistry` - 14 edges
8. `write_role_config()` - 13 edges
9. `resume_agent_from_rollout_does_not_reopen_closed_descendants()` - 12 edges
10. `resume_closed_child_reopens_open_descendants()` - 11 edges

## Surprising Connections (you probably didn't know these)
- `agent_nickname_candidates()` --calls--> `resolve_role_config()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/agent/control.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/agent/role.rs
- `test_config_with_cli_overrides()` --calls--> `build()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/agent/control_tests.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/agent/role.rs
- `test_config_with_cli_overrides()` --calls--> `build()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/agent/role_tests.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/agent/role.rs
- `apply_role_defaults_to_default_and_leaves_config_unchanged()` --calls--> `apply_role_to_config()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/agent/role_tests.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/agent/role.rs
- `apply_role_returns_error_for_unknown_role()` --calls--> `apply_role_to_config()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/agent/role_tests.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/agent/role.rs

## Hyperedges (group relationships)
- **Agent Name Pool** — agent_names_euclid, agent_names_archimedes, agent_names_ptolemy, agent_names_hypatia, agent_names_avicenna, agent_names_averroes, agent_names_aquinas, agent_names_copernicus, agent_names_kepler, agent_names_galileo, agent_names_bacon, agent_names_descartes, agent_names_pascal, agent_names_fermat, agent_names_huygens, agent_names_leibniz, agent_names_newton, agent_names_halley, agent_names_euler, agent_names_lagrange, agent_names_laplace, agent_names_volta, agent_names_gauss, agent_names_ampere, agent_names_faraday, agent_names_darwin, agent_names_lovelace, agent_names_boole, agent_names_pasteur, agent_names_maxwell, agent_names_mendel, agent_names_curie, agent_names_planck, agent_names_tesla, agent_names_poincare, agent_names_noether, agent_names_hilbert, agent_names_einstein, agent_names_raman, agent_names_bohr, agent_names_turing, agent_names_hubble, agent_names_feynman, agent_names_franklin, agent_names_mcclintock, agent_names_meitner, agent_names_herschel, agent_names_linnaeus, agent_names_wegener, agent_names_chandrasekhar, agent_names_sagan, agent_names_goodall, agent_names_carson, agent_names_carver, agent_names_socrates, agent_names_plato, agent_names_aristotle, agent_names_epicurus, agent_names_cicero, agent_names_confucius, agent_names_mencius, agent_names_zeno, agent_names_locke, agent_names_hume, agent_names_kant, agent_names_hegel, agent_names_kierkegaard, agent_names_mill, agent_names_nietzsche, agent_names_peirce, agent_names_james, agent_names_dewey, agent_names_russell, agent_names_popper, agent_names_sartre, agent_names_beauvoir, agent_names_arendt, agent_names_rawls, agent_names_singer, agent_names_anscombe, agent_names_parfit, agent_names_kuhn, agent_names_boyle, agent_names_hooke, agent_names_harvey, agent_names_dalton, agent_names_ohm, agent_names_helmholtz, agent_names_gibbs, agent_names_lorentz, agent_names_schrodinger, agent_names_heisenberg, agent_names_pauli, agent_names_dirac, agent_names_bernoulli, agent_names_godel, agent_names_nash, agent_names_banach, agent_names_ramanujan, agent_names_erdos, agent_names_jason [EXTRACTED 0.90]

## Communities

### Community 0 - "Agent Name Pool"
Cohesion: 0.04
Nodes (102): Agent (role), Ampere, Anscombe, Aquinas, Archimedes, Arendt, Aristotle, Averroes (+94 more)

### Community 1 - "Control Tests"
Cohesion: 0.14
Nodes (55): AgentControlHarness, append_message_records_assistant_message(), assistant_message(), completion_watcher_notifies_parent_when_child_is_missing(), get_status_returns_not_found_for_missing_thread(), get_status_returns_not_found_without_manager(), get_status_returns_pending_init_for_new_thread(), has_subagent_notification() (+47 more)

### Community 2 - "AgentControl Runtime"
Cohesion: 0.09
Nodes (13): agent_matches_prefix(), agent_nickname_candidates(), AgentControl, default_agent_nickname_list(), keep_forked_rollout_item(), ListedAgent, LiveAgent, render_input_preview() (+5 more)

### Community 3 - "Registry & Slots"
Cohesion: 0.14
Nodes (25): ActiveAgents, AgentMetadata, AgentRegistry, exceeds_thread_spawn_depth_limit(), format_agent_nickname(), next_thread_spawn_depth(), session_depth(), SpawnReservation (+17 more)

### Community 4 - "Role Tests"
Cohesion: 0.26
Nodes (26): apply_role_to_config(), build(), apply_empty_explorer_role_preserves_current_model_and_reasoning_effort(), apply_explorer_role_sets_model_and_adds_session_flags_layer(), apply_role_defaults_to_default_and_leaves_config_unchanged(), apply_role_does_not_materialize_default_sandbox_workspace_write_fields(), apply_role_ignores_agent_metadata_fields_in_user_role_file(), apply_role_preserves_active_profile_and_model_provider() (+18 more)

### Community 5 - "Role Layering"
Cohesion: 0.32
Nodes (16): apply_role_to_config_inner(), build_config_layer_stack(), build_from_configs(), build_next_config(), config_file_contents(), configs(), deserialize_effective_config(), existing_layers() (+8 more)

### Community 6 - "Mailbox"
Cohesion: 0.31
Nodes (6): Mailbox, mailbox_assigns_monotonic_sequence_numbers(), mailbox_drains_in_delivery_order(), mailbox_tracks_pending_trigger_turn_mail(), MailboxReceiver, make_mail()

### Community 7 - "Agent Resolver"
Cohesion: 0.83
Nodes (2): register_session_root(), resolve_agent_target()

## Knowledge Gaps
- **Thin community `Agent Resolver`** (4 nodes): `register_session_root()`, `resolve_agent_target()`, `agent_resolver.rs`, `agent_resolver.rs`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `AgentControl` connect `AgentControl Runtime` to `Control Tests`?**
  _High betweenness centrality (0.089) - this node is a cross-community bridge._
- **Why does `AgentRegistry` connect `Registry & Slots` to `AgentControl Runtime`?**
  _High betweenness centrality (0.025) - this node is a cross-community bridge._
- **Are the 102 inferred relationships involving `Agent (role)` (e.g. with `Euclid` and `Archimedes`) actually correct?**
  _`Agent (role)` has 102 INFERRED edges - model-reasoned connections that need verification._
- **Are the 16 inferred relationships involving `apply_role_to_config()` (e.g. with `apply_role_defaults_to_default_and_leaves_config_unchanged()` and `apply_role_returns_error_for_unknown_role()`) actually correct?**
  _`apply_role_to_config()` has 16 INFERRED edges - model-reasoned connections that need verification._
- **Are the 2 inferred relationships involving `test_config_with_cli_overrides()` (e.g. with `.new()` and `build()`) actually correct?**
  _`test_config_with_cli_overrides()` has 2 INFERRED edges - model-reasoned connections that need verification._
- **Should `Agent Name Pool` be split into smaller, more focused modules?**
  _Cohesion score 0.04 - nodes in this community are weakly interconnected._
- **Should `Control Tests` be split into smaller, more focused modules?**
  _Cohesion score 0.14 - nodes in this community are weakly interconnected._