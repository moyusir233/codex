# CODE_ANALYSIS

## Overview

`codex-execpolicy-legacy` is the original command validation engine used to classify a proposed `execv(3)`-style invocation as:

- `safe`: matched and does not imply file writes
- `match`: matched, but may write files and therefore needs caller-side path authorization
- `forbidden`: explicitly disallowed by policy
- `unverified`: no rule could prove the command safe

The crate exposes both:

- a library for parsing policies and matching commands
- a CLI that emits structured JSON for downstream consumers

The core design is intentionally two-stage:

1. Match a command shape against a policy and label arguments with semantic types.
2. Optionally perform filesystem-aware checks for readable/writeable paths before execution.

## Primary Responsibilities

### 1. Parse policy definitions

The policy language is Starlark-based and is parsed by `PolicyParser`. The parser:

- enables a small set of builtins such as `define_program()`, `flag()`, `opt()`
- injects matcher constants like `ARG_RFILE`, `ARG_WFILE`, `ARG_SED_COMMAND`
- accumulates rules into a `PolicyBuilder`
- compiles the builder into a `Policy`

This allows policy authors to define multiple variants of the same command, such as `printenv` with zero args vs one arg, or `sed` with an inline command vs `-e`.

### 2. Match `ExecCall` values against program specs

The runtime matcher accepts an `ExecCall { program, args }`, finds candidate `ProgramSpec`s for the program, and tries them in order until one matches. Matching performs:

- option and flag recognition
- option-value validation
- positional-argument matching against declared argument patterns
- required-option enforcement
- conversion into a structured `ValidExec`

If a matching spec is marked `forbidden`, the result becomes `MatchedExec::Forbidden` with a concrete cause.

### 3. Annotate arguments with semantic meaning

The engine does not just accept or reject strings. It assigns argument categories such as:

- `ReadableFile`
- `WriteableFile`
- `PositiveInteger`
- `SedCommand`
- `Literal(...)`
- `OpaqueNonFile`
- `Unknown`

This is the key abstraction that lets callers make higher-level safety decisions after syntax matching.

### 4. Perform path-aware exec checks

`ExecvChecker` is the second-stage checker. Given a `ValidExec`, current working directory, and allowed readable/writeable folders, it:

- canonicalizes or absolutizes file arguments
- verifies readable paths stay within allowed readable folders
- verifies writeable paths stay within allowed writeable folders
- prefers a trusted executable from `system_path` if one exists and is executable

### 5. Self-test policy examples

Each `ProgramSpec` can contain `should_match` and `should_not_match` examples. The crate exposes verification helpers that run these examples as lightweight embedded policy tests.

## Public API Surface

The library re-exports the main policy/matching types from `src/lib.rs`:

- `PolicyParser`: parse a Starlark policy into a `Policy`
- `Policy`: hold compiled specs and evaluate `ExecCall`s
- `ExecCall`: raw `{ program, args }` request
- `ProgramSpec`: compiled rule for a single command signature
- `ValidExec`: accepted command plus typed flags/options/args
- `MatchedExec`: either `Match` or `Forbidden`
- `ExecvChecker`: perform folder-based path authorization on a `ValidExec`
- `ArgMatcher` / `ArgType`: policy-side matchers and runtime-side semantic types
- `MatchedArg`, `MatchedOpt`, `MatchedFlag`: structured match results
- `parse_sed_command()`: bespoke validator for narrowly allowed `sed` expressions
- `get_default_policy()`: load and parse the embedded `src/default.policy`

The binary in `src/main.rs` wraps this API behind:

- `check <argv...>`: treat trailing args as an `execv` call
- `check-json <json>`: accept a JSON object with `program` and `args`
- `--policy <path>`: parse a custom Starlark policy
- `--require-safe`: return non-zero exit codes for non-safe outcomes

## Execution Flow

### Library flow

1. A `Policy` is created from `PolicyParser::parse()` or `get_default_policy()`.
2. A caller submits an `ExecCall`.
3. `Policy::check()` applies global forbids first:
   - forbidden program regexes
   - forbidden substrings inside args
4. The policy looks up all `ProgramSpec`s registered for the requested program.
5. Each `ProgramSpec::check()`:
   - scans argv left-to-right
   - matches known flags/options
   - validates option values immediately
   - collects positional args
   - resolves positional args against `ArgMatcher` patterns
   - enforces required options
   - builds `ValidExec`
6. If the spec has a `forbidden` reason, the result is `Forbidden`; otherwise `Match`.
7. If all specs fail, the last error is returned.

### CLI flow

1. Parse CLI args with `clap`.
2. Load the requested policy file or embedded default policy.
3. Convert CLI input into `ExecArg`, then `ExecCall`.
4. Call `policy.check(&exec_call)`.
5. Convert the result into one of four JSON output shapes:
   - `safe`
   - `match`
   - `forbidden`
   - `unverified`
6. Choose exit code:
   - `0` for ordinary success
   - `12` for matched-but-writes-files when `--require-safe` is set
   - `13` for unverified when `--require-safe` is set
   - `14` for forbidden when `--require-safe` is set

## Module Responsibilities

### Core matching

- `src/policy.rs`: compiled policy object, global forbid checks, dispatch to matching specs
- `src/program.rs`: per-program matching logic, required-option handling, forbidden exec results, embedded example verification
- `src/arg_resolver.rs`: resolve positional args against pattern lists, including one variable-arity matcher
- `src/valid_exec.rs`: accepted command representation and write-risk classification

### Policy language

- `src/policy_parser.rs`: Starlark parsing, builtin registration, rule accumulation
- `src/arg_matcher.rs`: Starlark-visible matcher constants and matcher cardinality rules
- `src/opt.rs`: typed option/flag definitions used by policies

### Validation primitives

- `src/arg_type.rs`: runtime validation for literals, positive integers, file-ish args, sed commands
- `src/sed_command.rs`: intentionally tiny safe subset of `sed`
- `src/error.rs`: structured failure types serialized back to callers

### Runtime authorization and interfaces

- `src/execv_checker.rs`: filesystem-aware second-pass authorization
- `src/exec_call.rs`: incoming command model and display implementation
- `src/main.rs`: CLI entrypoint and JSON protocol
- `src/lib.rs`: public re-exports and embedded default policy loader
- `build.rs`: forces rebuild when `src/default.policy` changes

## Default Policy Characteristics

The embedded policy currently covers a small allowlist-oriented command set, including:

- `ls`
- `cat`
- `cp`
- `head`
- `printenv`
- `pwd`
- `rg`
- `sed`
- `which`

Notable policy traits:

- multiple specs per program are supported and used in practice (`printenv`, `sed`)
- system executable preferences are embedded for several tools
- many rules treat file operands as readable or writeable paths rather than opaque strings
- `sed` support is intentionally narrow because of command-execution risk
- `rg` is allowed only in a constrained form, with TODO notes about host-bundled expectations

## Dependencies and Their Role

- `starlark`: policy language parser/evaluator and builtin exposure
- `clap`: CLI parsing
- `serde`, `serde_json`, `serde_with`: JSON output and serialized error/result types
- `regex-lite`: forbidden substring and forbidden program regex matching
- `multimap`: support multiple `ProgramSpec`s per executable name
- `path-absolutize`: canonicalize/absolutize paths in `ExecvChecker`
- `log`, `env_logger`: parser/debug logging
- `anyhow`: CLI and parser convenience errors
- `derive_more`, `allocative`: display and runtime support for Starlark-exposed values
- `tempfile` (dev): filesystem-backed tests for path authorization

## Testing Strategy

### What is covered

- parser and policy sanity through `get_default_policy()`
- positive and negative embedded examples from `default.policy`
- command-specific matching behavior for `cp`, `ls`, `head`, `pwd`, `sed`
- literal argument matching through a custom in-test policy
- direct unit tests for `parse_sed_command()`
- filesystem authorization and trusted executable path selection in `ExecvChecker`

### Test structure

- `tests/all.rs` aggregates integration tests under `tests/suite/`
- `tests/suite/good.rs` verifies all `should_match` examples succeed
- `tests/suite/bad.rs` verifies all `should_not_match` examples fail
- per-command files exercise representative valid and invalid cases
- `src/execv_checker.rs` contains focused unit tests for path checks

### Current state

`cargo test -p codex-execpolicy-legacy` passes locally. The run executed:

- 1 library unit test module (`execv_checker`)
- 32 aggregated integration tests

## Design Observations

### Strengths

- The semantic split between `ArgMatcher` and `ArgType` is clean: policy declarations stay concise while runtime results remain explicit.
- Multiple specs per program handle real command-shape variance without complex parser rules.
- The result model is practical: `safe` vs `match` preserves useful uncertainty instead of flattening everything into a boolean.
- Structured `Error` values make policy failures explainable and machine-readable.
- Embedded `should_match` / `should_not_match` lists are a lightweight and effective way to keep policy intent close to policy definitions.

### Limitations and quirks

- `ProgramSpec` stores `option_bundling` and `combined_format`, but matching logic does not use either field yet.
- `--` is explicitly unsupported.
- Options are recognized even after positional args, so the matcher may accept command shapes the real program rejects.
- Negative numeric values for option arguments are treated as options, not as values, because parsing is purely argv-shape-based.
- `Policy::check()` returns the last matching error among candidate specs, which is simple but can hide why earlier variants failed.
- The `sed` validator intentionally recognizes only a tiny safe subset, which is secure but narrow.
- `ExecvChecker` verifies path containment, not whether files exist or whether execution will succeed.
- On Windows, executable detection is only a placeholder and does not inspect `PATHEXT`.

## Open Questions

1. Should candidate-spec failure reporting be improved? Returning only the last error can make multi-variant programs harder to debug.
2. Should option parsing become faithful to real tool grammars? Current behavior accepts flags after positional args for commands like `ls`.
3. When should `option_bundling` and `combined_format` be fully implemented, and how should ambiguity be resolved for mixed styles?
4. Should `--` support be added so commands can safely express file names beginning with `-`?
5. Should `ExecvChecker` verify more than path prefixes, such as symlink policy, existence, file type, or explicit permission semantics?
6. Should forbidden substring and forbidden regex features be exercised by the default policy or additional tests? They exist in the parser/runtime but are not used by `src/default.policy`.
7. Is README documentation still aligned with the current default policy? The README includes an `applied deploy` forbidden example that is not present in `src/default.policy`.
8. Should the legacy engine continue to grow, or should features now move exclusively into the newer `codex-execpolicy` crate referenced by the README?

## Bottom Line

This crate is a compact, policy-driven command classifier rather than a general shell parser. Its value comes from producing typed, reviewable match results that downstream code can combine with filesystem context. The implementation is intentionally conservative, and several stored-but-unimplemented features show that the design was heading toward richer command grammars before the newer engine superseded it.
