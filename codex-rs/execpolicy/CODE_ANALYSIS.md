# CODE_ANALYSIS

## Scope

This document analyzes the `codex-execpolicy` crate in `/Users/bytedance/project/codex/codex-rs/execpolicy`.

- Crate kind: reusable library plus CLI binary.
- Primary goal: parse Starlark-based exec policy files and evaluate command tokens against prefix-oriented rules.
- Secondary goal: maintain helper APIs for amending policy files and compiling network domain allow/deny lists.

## High-level Responsibilities

The crate currently has six main responsibilities:

1. Parse exec policy files written in a constrained Starlark dialect.
2. Represent rules, matches, decisions, and parser/runtime errors in Rust types.
3. Evaluate one command or multiple commands against prefix rules.
4. Optionally resolve absolute executable paths back to basename rules, gated by `host_executable(...)`.
5. Store and normalize `network_rule(...)` declarations for downstream consumers.
6. Expose a CLI that loads policy files, evaluates a single command, and prints JSON.

The implementation is intentionally split between parsing, runtime policy evaluation, CLI glue, and file-amend helpers:

- `src/parser.rs`: Starlark integration and policy construction.
- `src/policy.rs`: runtime policy representation and match/evaluation logic.
- `src/rule.rs`: rule model, pattern matching, and example validation helpers.
- `src/execpolicycheck.rs`: CLI-facing policy loading and JSON formatting.
- `src/amend.rs`: append-only file mutation helpers for generating policy lines.
- `src/error.rs`, `src/decision.rs`, `src/executable_name.rs`: focused support modules.

## Module Map

### `src/lib.rs`

- Re-exports the crate’s main public surface: `Policy`, `PolicyParser`, `Decision`, `Evaluation`, `ExecPolicyCheckCommand`, rule types, and amend helpers.
- Keeps `executable_name` private, which is appropriate because it is an internal normalization detail.

### `src/main.rs`

- Defines a small Clap-based binary with one subcommand: `check`.
- Delegates directly to `ExecPolicyCheckCommand::run()`.
- Keeps the CLI intentionally thin, which helps keep business logic in the library.

### `src/decision.rs`

- Defines `Decision::{Allow, Prompt, Forbidden}`.
- Derives `Ord`, so severity ordering is encoded directly in enum declaration order.
- `Policy` relies on `max()` over `Decision` to pick the strictest result, which is concise but couples semantics to variant ordering.

### `src/error.rs`

- Central error enum for parsing and validation failures.
- Adds file/range metadata via `ErrorLocation`, `TextRange`, and `TextPosition`.
- Supports attaching locations to custom example-validation errors and extracting locations from Starlark parser/runtime errors.

### `src/rule.rs`

- Defines the pattern language:
  - `PatternToken::Single`
  - `PatternToken::Alts`
  - `PrefixPattern`
- Defines runtime match output:
  - `RuleMatch::PrefixRuleMatch`
  - `RuleMatch::HeuristicsRuleMatch`
- Defines the dynamic `Rule` trait and `RuleRef = Arc<dyn Rule>`.
- Implements `PrefixRule` as the only executable command rule type today.
- Defines network metadata types:
  - `NetworkRule`
  - `NetworkRuleProtocol`
- Contains example-validation helpers used by the parser.

### `src/policy.rs`

- Owns the compiled policy state:
  - `rules_by_program: MultiMap<String, RuleRef>`
  - `network_rules: Vec<NetworkRule>`
  - `host_executables_by_name: HashMap<String, Arc<[AbsolutePathBuf]>>`
- Provides APIs for evaluation, overlay merge, network-domain compilation, and ad hoc rule insertion.
- Implements the exact-match-first and optional basename-fallback flow.

### `src/parser.rs`

- Embeds a Starlark interpreter with custom builtins:
  - `prefix_rule(...)`
  - `network_rule(...)`
  - `host_executable(...)`
- Accumulates results in an internal `PolicyBuilder`.
- Validates `match` / `not_match` examples after each parse call.
- Supports parsing multiple policy files into one accumulated builder.

### `src/execpolicycheck.rs`

- Defines the CLI argument struct `ExecPolicyCheckCommand`.
- Loads one or more files through `PolicyParser`.
- Evaluates the provided command and prints JSON.
- Returns empty `matchedRules` and omits `decision` when nothing matches because the CLI does not pass a heuristics fallback.

### `src/amend.rs`

- Appends `prefix_rule(...)` or `network_rule(...)` lines to a policy file.
- Uses advisory file locking and blocking I/O.
- Deduplicates exact text matches before appending.

### `src/executable_name.rs`

- Normalizes executable lookup keys.
- On Windows, strips common executable suffixes such as `.exe` and `.cmd`.
- On non-Windows systems, uses the raw basename unchanged.

## Public API Surface

The most important public APIs are:

- Parsing:
  - `PolicyParser::new()`
  - `PolicyParser::parse(policy_identifier, policy_contents)`
  - `PolicyParser::build()`
- Evaluation:
  - `Policy::check(...)`
  - `Policy::check_with_options(...)`
  - `Policy::check_multiple(...)`
  - `Policy::check_multiple_with_options(...)`
  - `Policy::matches_for_command(...)`
  - `Policy::matches_for_command_with_options(...)`
- Policy construction and mutation:
  - `Policy::empty()`
  - `Policy::add_prefix_rule(...)`
  - `Policy::add_network_rule(...)`
  - `Policy::set_host_executable_paths(...)`
  - `Policy::merge_overlay(...)`
  - `Policy::compiled_network_domains()`
  - `Policy::get_allowed_prefixes()`
- CLI support:
  - `ExecPolicyCheckCommand`
  - `load_policies(...)`
  - `format_matches_json(...)`
- File-amend helpers:
  - `blocking_append_allow_prefix_rule(...)`
  - `blocking_append_network_rule(...)`

Notable API characteristics:

- The crate exposes concrete policy and rule data structures rather than hiding everything behind a service object.
- Runtime rule polymorphism exists through `Arc<dyn Rule>`, but there is only one command rule implementation today: `PrefixRule`.
- `matches_for_command*` can be used with or without a heuristics fallback; `check*` requires a fallback and therefore always returns a non-empty `Evaluation`.

## Policy Language

The parser currently recognizes three Starlark builtins:

### `prefix_rule(...)`

- Required:
  - `pattern`
- Optional:
  - `decision` with default `allow`
  - `justification`
  - `match`
  - `not_match`

Behavior:

- `pattern` is an ordered token list.
- Each token may be a string or a list of alternative strings.
- The first token may expand into multiple rules if it is an alternatives list.
- Tail alternatives are preserved as `PatternToken::Alts`; they are not expanded cartesian-product style.
- Empty or whitespace-only `justification` is rejected.

### `network_rule(...)`

- Required:
  - `host`
  - `protocol`
  - `decision`
- Optional:
  - `justification`

Behavior:

- `host` is normalized to lowercase and stripped of optional ports / trailing dots.
- Wildcards are explicitly rejected.
- `deny` is accepted as input and mapped to `Decision::Forbidden`.
- The rule is stored, but command evaluation does not currently consult it.

### `host_executable(...)`

- Required:
  - `name`
  - `paths`

Behavior:

- `name` must be a bare executable basename, not a path.
- `paths` must be absolute paths.
- Each path’s basename must normalize to `name`.
- Duplicate paths are deduplicated.
- Later definitions override earlier ones for the same normalized basename.

## End-to-end Flow

### Parse flow

1. `PolicyParser::parse()` configures Starlark `Dialect::Extended` and enables f-strings.
2. The source is parsed into `AstModule`.
3. The evaluator is built with custom globals from `policy_builtins`.
4. Builtins write into `PolicyBuilder` stored in `Evaluator.extra`.
5. `prefix_rule(...)` converts the Starlark pattern into `PatternToken`s, expands first-token alternatives into one `PrefixRule` per head, and records any example validations.
6. `host_executable(...)` validates and stores absolute executable metadata.
7. `network_rule(...)` validates and stores normalized network metadata.
8. After each file is parsed, only newly added example validations are run.

This incremental validation design matters because `PolicyParser` supports repeated `parse(...)` calls before `build()`.

### Evaluation flow

1. `Policy::matches_for_command_with_options(...)` first attempts exact matching on the first command token.
2. If exact matching returns no matches and `resolve_host_executables` is enabled, it attempts basename-based resolution for absolute paths.
3. If basename fallback matches, `resolved_program` is attached to each `RuleMatch`.
4. If no rule matches and a heuristics callback was supplied, a synthetic `HeuristicsRuleMatch` is returned.
5. `Evaluation::from_matches(...)` collapses all rule decisions via `max()` to pick the strictest outcome.

### CLI flow

1. `ExecPolicyCheckCommand::run()` reads all `--rules` files.
2. `load_policies(...)` parses them in order into one `Policy`.
3. The CLI evaluates a single command with `heuristics_fallback = None`.
4. `format_matches_json(...)` emits:
   - `matchedRules`
   - `decision` only if at least one rule matched

The CLI therefore behaves differently from library helpers like `Policy::check(...)`, which require a fallback and always produce a decision.

## Matching Semantics

The matching logic is simple and intentionally prefix-based:

- Rules are bucketed by the first token in a `MultiMap`.
- A `PrefixPattern` matches if:
  - the command has at least as many tokens as the pattern
  - the first token equals the rule head
  - every remaining pattern token matches the corresponding command token
- Matching is prefix-only, so extra command arguments after the matched prefix are allowed.

Example:

```text
pattern = ["git", "commit"]
command = ["git", "commit", "-m", "msg"]
```

This is a match because only the prefix must match.

Decision aggregation rules:

- `Allow < Prompt < Forbidden`
- Any stricter match dominates the final evaluation
- Multiple matching rules are preserved in output rather than reduced to the winner only

This preserves explainability while still producing one final decision.

## Host Executable Resolution

This is one of the more specific design choices in the crate.

Without resolution:

- `/usr/bin/git status` only matches rules whose first token is exactly `/usr/bin/git`.

With `MatchOptions { resolve_host_executables: true }`:

- If exact-path rules do not match, the crate tries basename lookup (`git`).
- If there is no `host_executable("git", ...)` mapping, basename fallback is allowed.
- If there is a mapping, basename fallback is allowed only if the absolute path is listed.
- If the mapping exists but is explicitly empty, basename fallback is blocked.

This creates three policy modes:

1. No mapping: permissive basename fallback.
2. Non-empty mapping: allowlisted basename fallback.
3. Empty mapping: explicitly disable basename fallback.

That behavior is thoroughly tested and seems intentional.

## Example Validation Design

`match` and `not_match` examples act like inline tests for rule definitions.

Important implementation detail:

- Validation is performed against a temporary `Policy` containing only the rules generated by that specific `prefix_rule(...)` call, plus the current `host_executable` map.

Consequences:

- `match` verifies that the examples match the rule being declared.
- `not_match` verifies that the examples do not match that rule’s generated rules.
- Validation does **not** check whether examples might match other, previously defined rules in the full policy set.

This is a coherent design if the goal is “unit test this rule in isolation,” but it is narrower than “validate whole-policy behavior.”

## Dependencies and Why They Matter

### Core dependencies

- `starlark`: powers the policy language and evaluation environment.
- `clap` with `derive`: powers the CLI.
- `serde` / `serde_json`: serialize match results and generated policy lines.
- `multimap`: stores multiple rules per first token without manual `HashMap<Vec<_>>` plumbing.
- `shlex`: tokenizes string examples and safely renders examples back into shell-like text for errors.
- `thiserror`: concise error definitions.
- `codex-utils-absolute-path`: keeps absolute path handling typed and validated.
- `anyhow`: used mostly in CLI and test-facing glue for ergonomic error propagation.

### Test dependencies

- `pretty_assertions`: clearer diffs for rule/evaluation snapshots.
- `tempfile`: isolated temporary files for amendment and parser tests.

## Testing Coverage

I ran the crate test suite successfully with:

```bash
cargo test -p codex-execpolicy
```

Observed result:

- 33 total tests passed.
- 6 unit tests in `src/amend.rs`.
- 27 integration-style tests in `tests/basic.rs`.
- 0 doc tests.

The test suite covers the most important current behaviors:

- prefix matching basics
- decision precedence
- justification propagation and validation
- multi-file parsing
- first-token alternative expansion
- tail-token alternative matching
- inline `match` / `not_match` validation
- heuristics fallback behavior
- network rule parsing and host normalization
- host executable parsing and fallback resolution
- append-only amendment helpers and deduplication

Coverage is strongest around happy-path parsing and resolution semantics. Coverage is lighter around malformed Starlark spans, JSON output shape, and overlay/merge behavior.

## Design Strengths

- Clear separation of parse-time, runtime evaluation, CLI, and amendment logic.
- Minimal CLI wrapper keeps reusable behavior in the library.
- Prefix matching model is easy to reason about and cheap to evaluate.
- `Decision` ordering makes “strictest rule wins” trivial to implement.
- `RuleMatch` preserves all matches, which helps with explainability.
- `host_executable(...)` creates a practical bridge between basename policies and absolute-path commands.
- Inline examples are a strong ergonomics feature for policy authors.

## Design Constraints and Caveats

### 1. Command policy is more mature than network policy

- `network_rule(...)` is parsed, normalized, stored, and can be compiled into domain allow/deny lists.
- It is not part of `Policy::check(...)` command evaluation.
- The CLI does not expose compiled network outputs.

This makes network support real but incomplete from an end-user workflow perspective.

### 2. Runtime rule polymorphism is currently underused

- The trait-based rule model suggests extensibility.
- In practice, only `PrefixRule` exists for command matching.
- Some APIs such as `get_allowed_prefixes()` downcast dynamic rules back into `PrefixRule`.

That is acceptable today, but it shows the abstraction is wider than the current feature set.

### 3. Example validation is isolated, not global

- As noted earlier, examples are validated against just the just-added rules.
- This avoids interference from unrelated rules.
- It also means examples do not protect against accidental shadowing or overlap elsewhere in the merged policy.

### 4. Overlay and parsing semantics differ

- `PolicyParser` merges rules across files and lets later `host_executable` definitions replace earlier ones.
- `Policy::merge_overlay()` appends rules and network rules and extends the host executable map with overlay entries replacing base entries by key.

These are compatible, but there is no direct test coverage for `merge_overlay()`.

### 5. File amendment is append-only and line-text-based

- Deduplication checks whether an identical line already exists.
- Semantically equivalent but differently formatted rules are not deduplicated.
- The helper writes one Starlark call per line and assumes that style.

This is pragmatic, not syntax-tree-aware.

## Open Questions

These are the main questions or follow-up points that stood out during analysis:

1. Should `network_rule(...)` be surfaced by the CLI, or is another crate expected to consume `compiled_network_domains()`?
2. Should `match` / `not_match` support whole-policy validation mode in addition to current per-rule isolation?
3. Should `Policy::merge_overlay()` and `compiled_network_domains()` have dedicated tests to lock down ordering and override behavior?
4. Should JSON output shape be tested explicitly, especially because `RuleMatch` uses serde enum tagging?
5. Should amendment helpers use `create_dir_all()` instead of `create_dir()` in case callers pass deeper missing directory trees?
6. Should there be first-class APIs for parsing directly from files rather than requiring external read-then-parse loops?
7. Should the crate expose a public network evaluation model, or is network policy intentionally just a compilation artifact for another subsystem?

## Key Source References

- Library exports: `/Users/bytedance/project/codex/codex-rs/execpolicy/src/lib.rs`
- CLI entrypoint: `/Users/bytedance/project/codex/codex-rs/execpolicy/src/main.rs`
- Parser and builtins: `/Users/bytedance/project/codex/codex-rs/execpolicy/src/parser.rs`
- Runtime policy evaluation: `/Users/bytedance/project/codex/codex-rs/execpolicy/src/policy.rs`
- Rule model and validation helpers: `/Users/bytedance/project/codex/codex-rs/execpolicy/src/rule.rs`
- CLI command implementation: `/Users/bytedance/project/codex/codex-rs/execpolicy/src/execpolicycheck.rs`
- Amendment helpers: `/Users/bytedance/project/codex/codex-rs/execpolicy/src/amend.rs`
- Main tests: `/Users/bytedance/project/codex/codex-rs/execpolicy/tests/basic.rs`

## Bottom Line

`codex-execpolicy` is a focused policy engine centered on prefix-based command matching with a Starlark authoring layer. Its current design is compact and readable, and the tested behaviors around prefix matching, rule precedence, host executable resolution, and inline example validation are solid. The main architectural gap is that network policy support exists as parsed metadata rather than a fully integrated evaluation path. The other notable nuance is that example validation tests rules in isolation, not the final merged policy.
