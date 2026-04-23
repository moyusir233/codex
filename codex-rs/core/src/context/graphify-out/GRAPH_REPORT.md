# Graph Report - .  (2026-04-23)

## Corpus Check
- Corpus is ~1,921 words - fits in a single context window. You may not need a graph.

## Summary
- 61 nodes · 81 edges · 11 communities detected
- Extraction: 81% EXTRACTED · 19% INFERRED · 0% AMBIGUOUS · INFERRED: 15 edges (avg confidence: 0.8)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- [[_COMMUNITY_Shell Environment Context|Shell Environment Context]]
- [[_COMMUNITY_Turn Context Item|Turn Context Item]]
- [[_COMMUNITY_Detects Fragment User|Detects Fragment User]]
- [[_COMMUNITY_Contextual User Fragment|Contextual User Fragment]]
- [[_COMMUNITY_Turn Aborted Rs|Turn Aborted Rs]]
- [[_COMMUNITY_Subagent Notification Rs|Subagent Notification Rs]]
- [[_COMMUNITY_User Shell Command|User Shell Command]]
- [[_COMMUNITY_Contextualuserfragment Fragmentregistration Fragmentregistrationproxy|Contextualuserfragment Fragmentregistration Fragmentregistrationproxy]]
- [[_COMMUNITY_Skill Instructions Rs|Skill Instructions Rs]]
- [[_COMMUNITY_User Instructions Rs|User Instructions Rs]]
- [[_COMMUNITY_Networkcontext New Environment|Networkcontext New Environment]]

## God Nodes (most connected - your core abstractions)
1. `EnvironmentContext` - 10 edges
2. `fake_shell_name()` - 8 edges
3. `equals_except_shell_compares_cwd()` - 4 edges
4. `equals_except_shell_compares_cwd_differences()` - 4 edges
5. `is_standard_contextual_user_text()` - 4 edges
6. `parse_visible_hook_prompt_message()` - 4 edges
7. `SkillInstructions` - 3 edges
8. `TurnAborted` - 3 edges
9. `serialize_workspace_write_environment_context()` - 3 edges
10. `serialize_environment_context_with_network()` - 3 edges

## Surprising Connections (you probably didn't know these)
- `detects_hook_prompt_fragment_and_roundtrips_escaping()` --calls--> `parse_visible_hook_prompt_message()`  [INFERRED]
  /data00/home/yangchengrun/codex/codex-rs/core/src/context/contextual_user_message_tests.rs → /data00/home/yangchengrun/codex/codex-rs/core/src/context/contextual_user_message.rs

## Communities

### Community 0 - "Shell Environment Context"
Cohesion: 0.45
Nodes (8): equals_except_shell_compares_cwd(), equals_except_shell_compares_cwd_differences(), equals_except_shell_ignores_shell(), fake_shell_name(), serialize_environment_context_with_network(), serialize_environment_context_with_subagents(), serialize_read_only_environment_context(), serialize_workspace_write_environment_context()

### Community 1 - "Turn Context Item"
Cohesion: 0.4
Nodes (1): EnvironmentContext

### Community 2 - "Detects Fragment User"
Cohesion: 0.29
Nodes (1): detects_hook_prompt_fragment_and_roundtrips_escaping()

### Community 3 - "Contextual User Fragment"
Cohesion: 0.43
Nodes (5): is_contextual_user_fragment(), is_memory_excluded_contextual_user_fragment(), is_standard_contextual_user_text(), parse_visible_hook_prompt_message(), FragmentRegistrationProxy<T>

### Community 4 - "Turn Aborted Rs"
Cohesion: 0.5
Nodes (1): TurnAborted

### Community 5 - "Subagent Notification Rs"
Cohesion: 0.5
Nodes (1): SubagentNotification

### Community 6 - "User Shell Command"
Cohesion: 0.5
Nodes (1): UserShellCommand

### Community 7 - "Contextualuserfragment Fragmentregistration Fragmentregistrationproxy"
Cohesion: 0.5
Nodes (3): ContextualUserFragment, FragmentRegistration, FragmentRegistrationProxy

### Community 8 - "Skill Instructions Rs"
Cohesion: 0.67
Nodes (1): SkillInstructions

### Community 9 - "User Instructions Rs"
Cohesion: 0.67
Nodes (1): UserInstructions

### Community 10 - "Networkcontext New Environment"
Cohesion: 0.67
Nodes (1): NetworkContext

## Knowledge Gaps
- **3 isolated node(s):** `FragmentRegistration`, `FragmentRegistrationProxy`, `ContextualUserFragment`
  These have ≤1 connection - possible missing edges or undocumented components.
- **Thin community `Turn Context Item`** (10 nodes): `EnvironmentContext`, `.body()`, `.diff_from_turn_context_item()`, `.equals_except_shell()`, `.from_turn_context()`, `.from_turn_context_item()`, `.network_from_turn_context()`, `.network_from_turn_context_item()`, `.new()`, `.with_subagents()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Detects Fragment User`** (7 nodes): `classifies_memory_excluded_fragments()`, `detects_agents_instructions_fragment()`, `detects_environment_context_fragment()`, `detects_hook_prompt_fragment_and_roundtrips_escaping()`, `detects_subagent_notification_fragment_case_insensitively()`, `ignores_regular_user_text()`, `contextual_user_message_tests.rs`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Turn Aborted Rs`** (4 nodes): `turn_aborted.rs`, `TurnAborted`, `.body()`, `.new()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Subagent Notification Rs`** (4 nodes): `subagent_notification.rs`, `SubagentNotification`, `.body()`, `.new()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `User Shell Command`** (4 nodes): `user_shell_command.rs`, `UserShellCommand`, `.body()`, `.new()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Skill Instructions Rs`** (3 nodes): `skill_instructions.rs`, `SkillInstructions`, `.body()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `User Instructions Rs`** (3 nodes): `user_instructions.rs`, `UserInstructions`, `.body()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Networkcontext New Environment`** (3 nodes): `NetworkContext`, `.new()`, `environment_context.rs`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `parse_visible_hook_prompt_message()` connect `Contextual User Fragment` to `Shell Environment Context`, `Detects Fragment User`?**
  _High betweenness centrality (0.107) - this node is a cross-community bridge._
- **Why does `detects_hook_prompt_fragment_and_roundtrips_escaping()` connect `Detects Fragment User` to `Contextual User Fragment`?**
  _High betweenness centrality (0.071) - this node is a cross-community bridge._
- **Why does `SkillInstructions` connect `Skill Instructions Rs` to `Shell Environment Context`?**
  _High betweenness centrality (0.029) - this node is a cross-community bridge._
- **Are the 2 inferred relationships involving `equals_except_shell_compares_cwd()` (e.g. with `.new()` and `.from()`) actually correct?**
  _`equals_except_shell_compares_cwd()` has 2 INFERRED edges - model-reasoned connections that need verification._
- **Are the 2 inferred relationships involving `equals_except_shell_compares_cwd_differences()` (e.g. with `.new()` and `.from()`) actually correct?**
  _`equals_except_shell_compares_cwd_differences()` has 2 INFERRED edges - model-reasoned connections that need verification._
- **What connects `FragmentRegistration`, `FragmentRegistrationProxy`, `ContextualUserFragment` to the rest of the system?**
  _3 weakly-connected nodes found - possible documentation gaps or missing edges._