# codex-core-skills crate analysis

## Overview

`codex-core-skills` is the skill discovery, filtering, indexing, rendering, and injection layer used by Codex sessions. It turns on-disk skill bundles into a structured `SkillLoadOutcome`, lets higher layers decide which skills are visible or enabled, renders the prompt-facing skill catalog, resolves explicit and implicit skill usage, and loads full `SKILL.md` bodies when a turn actually invokes a skill.

At a high level, the crate has six major jobs:

1. Discover skill roots from config, repo state, user home, bundled system cache, and optional plugin roots.
2. Parse `SKILL.md` files plus optional `agents/openai.yaml` metadata into `SkillMetadata`.
3. Apply config rules and product restrictions to determine which loaded skills are enabled.
4. Build indexes for implicit invocation detection and render a budgeted “available skills” section for the model prompt.
5. Resolve explicit skill mentions from user input and load matching skill bodies for prompt injection.
6. Surface skill dependencies such as required environment variables, and provide a not-yet-integrated remote skill client.

## Module responsibilities

### `src/lib.rs`

Acts as the public surface. It re-exports the core data model, manager, loader, render helpers, explicit/implicit invocation helpers, and dependency extraction APIs.

### `src/model.rs`

Defines the main data model:

- `SkillMetadata`: normalized skill record with `name`, `description`, optional UI/dependency/policy metadata, owning `scope`, and canonical `path_to_skills_md`.
- `SkillPolicy`: currently supports `allow_implicit_invocation` and product restrictions.
- `SkillLoadOutcome`: loaded skills, parse errors, disabled-path set, per-skill filesystem mapping, and implicit invocation indexes.

Important behavior:

- Skills are considered enabled unless their path lands in `disabled_paths`.
- Implicit invocation is additionally blocked when `policy.allow_implicit_invocation == Some(false)`.
- Product filtering physically removes non-matching skills and trims associated indexes and filesystem mappings.

### `src/loader.rs`

This is the most important module. It:

- Computes skill roots from config layers, repo-local `.codex/skills`, repo `.agents/skills`, `$HOME/.agents/skills`, legacy `$CODEX_HOME/skills`, bundled system cache, admin roots, and plugin roots.
- Walks each root breadth-first with bounded depth and directory-count limits.
- Parses `SKILL.md` YAML frontmatter.
- Loads optional `agents/openai.yaml` metadata and converts it into structured interface/dependency/policy fields.
- Canonicalizes paths, de-duplicates identical skill documents, sorts results by scope/name/path, and stores the backing filesystem per skill.

Notable loader choices:

- Hidden entries are skipped.
- Directory symlinks are followed for repo/user/admin scopes but not for system scope.
- Symlinked `SKILL.md` files are ignored because only real files with `metadata.is_file` are parsed.
- Optional metadata is fail-open: bad `openai.yaml` logs a warning but does not block `SKILL.md` loading.
- Required frontmatter fields are strict; optional metadata fields are lenient and dropped if invalid.

### `src/config_rules.rs`

Builds skill enable/disable rules from user config and session flags:

- Supports selectors by `name` or canonicalized `path`.
- Preserves precedence order across layers.
- Applies rules after loading, producing `disabled_paths`.

The rule model is intentionally simple: the crate keeps all loaded skills in memory and tracks enablement separately instead of deleting disabled skills.

### `src/manager.rs`

Owns lifecycle and caching:

- Installs bundled system skills on startup when enabled.
- Best-effort removes stale bundled skill cache when disabled.
- Loads skills by config-aware cache key or by cwd cache.
- Builds final outcome by calling the loader, applying product restrictions, resolving disabled paths, and generating implicit invocation indexes.

There are two cache strategies:

- `skills_for_config`: safer cache keyed by effective roots + config rules.
- `skills_for_cwd`: legacy/simple cache keyed only by cwd, mainly used when repo filesystem access is available.

### `src/render.rs`

Renders the prompt-facing skill catalog:

- Produces a `<skills_instructions>` section containing a short catalog and usage rules.
- Supports budgets in either approximate tokens or characters.
- Prioritizes prompt inclusion by scope order `System -> Admin -> Repo -> User`.
- Emits telemetry for total skills, kept skills, and whether truncation happened.

This scope order is intentionally different from loader precedence: prompt presentation favors system/admin instructions first, while loading precedence favors repo/user overrides.

### `src/injection.rs`

Handles turn-time skill selection and injection:

- `collect_explicit_skill_mentions` resolves skills from structured `UserInput::Skill` items and `$skill-name` mentions in text.
- `build_skill_injections` reads the full `SKILL.md` bodies for selected skills, emits telemetry, and reports warnings for unreadable files.

Resolution semantics are careful:

- Structured path-based selections are resolved first.
- Text mentions preserve the existing `skills` ordering rather than mention order.
- Plain-name selection only succeeds when the name is unambiguous and does not collide with connector slugs.
- Linked mentions like `[$skill](path)` prefer exact path matches.

### `src/invocation_utils.rs`

Builds and uses indexes for implicit skill invocation:

- Maps each allowed skill to its skill doc path and sibling `scripts/` directory.
- Detects implicit invocation when a command reads a `SKILL.md` file with tools like `cat`, `sed`, `head`, etc.
- Detects implicit invocation when a supported runner executes a script under a skill’s `scripts/` subtree.

This logic operates on canonicalized paths and works with relative or absolute command paths.

### `src/env_var_dependencies.rs`

Extracts `env_var` tool dependencies from mentioned skills so the session layer can prompt the user for missing environment values.

### `src/mention_counts.rs`

Builds exact and case-folded counts of enabled skill names. This supports ambiguity detection and name-vs-connector collision handling.

### `src/system.rs`

Thin wrapper around the `codex-skills` bundled-skill installer plus local cleanup of the cached bundled directory.

### `src/remote.rs`

Implements a low-level remote skill API client:

- `list_remote_skills`
- `export_remote_skill`

It handles ChatGPT auth, query construction, ZIP validation, and safe archive extraction. The file explicitly says this path is intentionally kept for future wiring and is not currently used by active product surfaces.

## Main data flow

### 1. Root discovery

Entry points in `SkillsManager` call `loader::skill_roots(...)`, which gathers roots from:

- user config folder `skills/`
- home-level `.agents/skills`
- bundled cache `skills/.system`
- admin/system config folder `skills/`
- project `.codex/skills`
- repo-local `.agents/skills` from project root to cwd
- plugin roots passed in by the caller
- extra user roots passed to the cwd-loading API

### 2. Skill discovery and parsing

For each root, the loader:

- verifies the root is a directory
- scans breadth-first with limits (`MAX_SCAN_DEPTH = 6`, `MAX_SKILLS_DIRS_PER_ROOT = 2000`)
- parses each `SKILL.md`
- optionally loads `agents/openai.yaml`
- canonicalizes the skill doc path
- stores the filesystem associated with the root

### 3. Filtering and finalization

The manager then:

- filters by product restriction
- resolves disabled paths from config/session rules
- computes implicit invocation indexes using only enabled + implicitly allowed skills

The result is a `SkillLoadOutcome` that still contains disabled skills in `skills`, but marks them via `disabled_paths`.

### 4. Prompt-time catalog rendering

At thread start, the higher-level session code renders only `allowed_skills_for_implicit_invocation()` into the developer prompt catalog. The rendered section is budget-limited and may omit lower-priority skills.

### 5. Turn-time explicit invocation

When a user turn arrives, higher-level session code:

- computes connector-name counts
- resolves explicitly mentioned skills from input
- extracts env-var dependencies
- prompts for missing env vars when the feature flag allows it
- loads matching `SKILL.md` bodies via `build_skill_injections`
- injects those bodies into the model context

### 6. Turn-time implicit invocation analytics

When command execution occurs, higher-level code calls `detect_implicit_skill_invocation_for_command(...)`. If a command reads a skill doc or runs a skill script, the session records telemetry/analytics once per unique skill in that turn.

## Public APIs and what they are for

The most important exported APIs are:

- `SkillsManager`: top-level load/caching orchestrator.
- `SkillsLoadInput`: caller-supplied load context (`cwd`, roots, config stack, bundled-skill flag).
- `load_skills_from_roots(...)`: low-level loader for direct root scanning.
- `render_skills_section(...)`: prompt catalog renderer.
- `collect_explicit_skill_mentions(...)`: turn-time skill selection from user input.
- `build_skill_injections(...)`: loads actual `SKILL.md` contents for selected skills.
- `detect_implicit_skill_invocation_for_command(...)`: command-based implicit invocation detector.
- `collect_env_var_dependencies(...)`: extracts `env_var` requirements from skill metadata.
- `filter_skill_load_outcome_for_product(...)`: removes skills that do not match a product restriction.
- `list_remote_skills(...)` / `export_remote_skill(...)`: unused-for-now remote skill client.

## Important design decisions

### Scope semantics

The crate uses `SkillScope` in two different but deliberate ways:

- Load precedence and dedupe preference: `Repo -> User -> System -> Admin`
- Prompt rendering priority: `System -> Admin -> Repo -> User`

This gives repo-local definitions first-class precedence during discovery while keeping high-level system/admin instructions most visible in prompt metadata.

### Canonical-path identity

Skill identity is path-based. The loader canonicalizes roots and skill docs when possible, which:

- avoids duplicate discovery through symlink aliases
- makes config path selectors stable
- lets exact-path mentions resolve consistently

### Fail-open metadata loading

`SKILL.md` is authoritative. Optional metadata in `agents/openai.yaml` is treated as enrichment, not a hard dependency. Invalid metadata is dropped with warnings instead of breaking the skill.

### Filesystem abstraction

The crate is designed to work against either local disk or an abstract `ExecutorFileSystem`. That is why skill roots retain a filesystem object and `SkillLoadOutcome` keeps a path-to-filesystem mapping for later injection reads.

### Separation between loaded and enabled

The crate does not remove disabled skills from `skills`. Instead, it:

- preserves the full discovered set
- tracks disabled paths separately
- derives enabled subsets where needed

That makes diagnostics and later rule application simpler, but it also means callers must be careful to use enable-aware helpers.

### Conservative implicit invocation

Implicit invocation only indexes allowed skills and uses narrow heuristics:

- supported runners for scripts
- supported readers for direct `SKILL.md` reads

This reduces false positives.

## Metadata and validation rules

The loader supports:

- Required `SKILL.md` frontmatter fields: `name` optional, `description` required in practice because empty values fail validation.
- Optional frontmatter metadata: `metadata.short-description`
- Optional `agents/openai.yaml` sections:
  - `interface`
  - `dependencies.tools`
  - `policy`

Validation behavior:

- Name, description, short description, default prompt, dependency fields, and transport/type strings all have max lengths.
- Interface icon paths must be relative and remain under `assets/`.
- `brand_color` must match `#RRGGBB`.
- Invalid optional fields are ignored rather than rejected.
- Invalid required `SKILL.md` fields produce `SkillError` entries.

## Dependencies and how they are used

### Internal workspace crates

- `codex-config`: config layer stack, root markers, skills config parsing.
- `codex-exec-server`: filesystem abstraction for local/remote repo access.
- `codex-protocol`: `Product`, `SkillScope`, prompt tags, and user-input types.
- `codex-skills`: bundled system skill installation/cache path helpers.
- `codex-utils-absolute-path`: strong absolute path handling.
- `codex-utils-plugins`: plugin namespace detection and mention syntax.
- `codex-utils-output-truncation`: approximate token counting for render budgets.
- `codex-analytics` and `codex-otel`: skill telemetry and analytics.
- `codex-login`: auth for the remote skill API client.

### Third-party crates

- `serde`, `serde_yaml`, `serde_json`, `toml`: config and metadata parsing.
- `tokio`: async filesystem work and blocking ZIP extraction handoff.
- `shlex`: command tokenization for implicit invocation detection.
- `dirs`: home directory discovery.
- `zip`: remote skill export extraction.
- `anyhow`, `tracing`, `dunce`: error reporting, logs, and path normalization.

## Testing coverage

The crate has strong unit coverage around discovery and selection semantics.

### Loader tests cover

- root mapping for user/system/admin/repo scopes
- home `.agents/skills` support
- repo `.codex/skills` and `.agents/skills` traversal
- project-root boundary behavior
- symlink handling and cycle avoidance
- max scan depth
- duplicate path/name behavior
- plugin namespace prefixing
- frontmatter parsing and validation
- interface/dependency/policy metadata loading

### Manager tests cover

- bundled-skill cleanup when disabled
- config-aware cache reuse
- cwd cache behavior with extra roots
- extra root normalization
- enable/disable override precedence across user config and session flags
- bundled-skill exclusion when disabled via config

### Injection and invocation tests cover

- mention parsing for plain and linked mentions
- ambiguity handling
- structured input precedence
- connector-name conflicts
- explicit path resolution and deduplication
- script/doc implicit invocation matching

### Noticeable gaps

- `remote.rs` has no local tests in this crate.
- `build_skill_injections(...)` itself is not directly tested here for read failures, analytics emission, or filesystem selection.
- `collect_env_var_dependencies(...)` has no dedicated tests.
- Render tests exist, but prompt text structure changes are only lightly validated.

## Known limitations and open questions

1. Product gating is incomplete. The code parses and filters by product during loading, but `SkillPolicy` still carries a TODO saying gating should be enforced in selection/injection, not just parsed and stored.
2. Cwd cache is intentionally coarse. `skills_for_cwd(...)` can return cached entries that were built with extra roots, which the tests document as current behavior. That is useful for performance but surprising semantically.
3. Disabled skills remain in `SkillLoadOutcome.skills`. This is workable, but easy for callers to misuse if they iterate `skills` directly instead of using enable-aware helpers.
4. Remote skill support is not wired into an active surface. The API client exists, but the file explicitly says it is future-facing.
5. Implicit invocation heuristics are intentionally narrow. That limits false positives, but may miss legitimate invocations through unsupported runners or file-reading patterns.
6. Optional metadata is fail-open. That protects availability, but silently dropping bad metadata can hide packaging problems unless warnings are monitored.

## Practical summary

If you need to understand the crate quickly, the core path is:

1. `SkillsManager` decides roots and caching.
2. `loader.rs` discovers and parses skills.
3. `config_rules.rs` and product filtering decide enablement.
4. `render.rs` exposes enabled implicit skills to the model.
5. `injection.rs` resolves explicit mentions and loads actual `SKILL.md` contents.
6. `invocation_utils.rs` observes command usage for implicit invocation analytics.

This crate is well-factored, heavily path- and config-driven, and designed to keep skill packaging flexible while preserving enough structure for selection, prompting, and analytics.
