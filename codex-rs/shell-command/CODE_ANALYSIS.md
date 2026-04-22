# shell-command crate analysis

## Overview

`codex-shell-command` is a small but high-leverage support crate that turns raw shell argv vectors into three kinds of product behavior:

1. **Human-readable command summaries** via `parse_command`.
2. **Read-only/safe-command detection** via `is_safe_command`.
3. **Dangerous-command detection** via `is_dangerous_command`.

The crate sits between low-level shell execution and higher-level policy/UI code. It does not execute commands itself. Instead, it classifies commands well enough for:

- approval policy decisions,
- event logging and user-visible summaries,
- identifying obviously dangerous commands,
- preserving shell-specific semantics when commands are wrapped in `bash -lc`, `zsh -lc`, or PowerShell.

The crate is intentionally conservative. When parsing is ambiguous, dynamic, or shell features become too expressive, it falls back to `Unknown` or `unsafe`.

## Package shape

### Cargo / build

- `Cargo.toml` defines the crate as `codex-shell-command`.
- Main external dependencies:
  - `codex-protocol`: provides the shared `ParsedCommand` enum returned by `parse_command`.
  - `tree-sitter` + `tree-sitter-bash`: structural parsing of bash/zsh command strings.
  - `shlex`: token splitting/joining for normalized summaries.
  - `serde` + `serde_json` + `base64`: request/response protocol for the persistent PowerShell parser process.
  - `regex` + `url` + `once_cell`: Windows dangerous-command heuristics, especially URL launch detection.
  - `which` + `codex-utils-absolute-path`: locating `powershell.exe` / `pwsh.exe`.
- `BUILD.bazel` includes `src/command_safety/powershell_parser.ps1` as `compile_data`, which matters because the Rust code embeds and launches that script for PowerShell AST parsing.

### Module map

- `src/lib.rs`
  - public surface and re-exports.
- `src/parse_command.rs`
  - command summarization and normalization into `ParsedCommand`.
- `src/bash.rs`
  - bash/zsh extraction plus tree-sitter-based shell parsing helpers.
- `src/powershell.rs`
  - PowerShell command extraction, UTF-8 prefixing, executable discovery.
- `src/shell_detect.rs`
  - identifies `bash`, `zsh`, `sh`, `pwsh`, `powershell`, and `cmd` by basename / path stem.
- `src/command_safety/is_safe_command.rs`
  - allowlist-based read-only safety checks.
- `src/command_safety/is_dangerous_command.rs`
  - narrow dangerous-command detection and shared git helper logic.
- `src/command_safety/windows_safe_commands.rs`
  - Windows-specific safe PowerShell parsing/classification.
- `src/command_safety/windows_dangerous_commands.rs`
  - Windows-specific dangerous PowerShell/CMD/browser heuristics.
- `src/command_safety/powershell_parser.rs`
  - long-lived child process wrapper for PowerShell AST parsing.
- `src/command_safety/powershell_parser.ps1`
  - actual PowerShell-side AST flattener.

## Public API and responsibilities

### Root exports

- `is_safe_command(command: &[String]) -> bool`
  - re-export of `command_safety::is_safe_command::is_known_safe_command`.
  - Answers: “is this command conservative-read-only and safe to auto-approve?”
- `is_dangerous_command(command: &[String]) -> bool`
  - re-export of `command_safety::is_dangerous_command::command_might_be_dangerous`.
  - Answers: “is this command suspicious enough to force stronger handling?”

### `parse_command` module

- `parse_command(command: &[String]) -> Vec<ParsedCommand>`
  - Main summarizer.
  - Produces one or more semantic summaries:
    - `Read { cmd, name, path }`
    - `ListFiles { cmd, path }`
    - `Search { cmd, query, path }`
    - `Unknown { cmd }`
  - Deduplicates consecutive identical summaries.
  - If any segment becomes `Unknown`, the whole result collapses to a single `Unknown` summary of the original command/script. This is a deliberate “do not overclaim understanding” rule.
- `extract_shell_command(command: &[String]) -> Option<(&str, &str)>`
  - Returns shell executable + inline script for bash/zsh or PowerShell wrappers.
- `shlex_join(tokens: &[String]) -> String`
  - Stable, user-facing rendering of normalized commands.

### `bash` module

- `extract_bash_command`
  - Recognizes `bash|zsh|sh` with `-c` / `-lc`.
- `try_parse_shell`
  - Parses shell source with tree-sitter-bash.
- `parse_shell_lc_plain_commands`
  - Returns a sequence of plain commands from a shell script if the script only uses allowed syntax.
- `parse_shell_lc_single_command_prefix`
  - Special-case parser for here-doc shell scripts when only the executable prefix is needed.

### `powershell` module

- `extract_powershell_command`
  - Recognizes PowerShell invocations and extracts the script body.
- `prefix_powershell_script_with_utf8`
  - Prepends `[Console]::OutputEncoding=[System.Text.Encoding]::UTF8;` unless already present.
- `try_find_powershell_executable_blocking`
- `try_find_pwsh_executable_blocking`
  - Locate working PowerShell executables and verify they run.

## Core flow

### 1. Command summarization flow

`parse_command` follows this rough pipeline:

1. Try shell-aware parsing first.
   - `parse_shell_lc_commands` recognizes `bash -lc` / `zsh -lc`.
   - It uses tree-sitter to accept only “word-only” command sequences with safe connectors (`&&`, `||`, `;`, `|`).
2. If the command is a PowerShell wrapper, strip the outer shell and summarize the script as `Unknown`.
   - This is intentionally less ambitious than the safety subsystem.
3. Normalize plain argv.
   - Drop wrappers like `yes |`, `no |`, and `bash -c/-lc`.
   - Split on connectors.
4. Track `cd` segments.
   - Later `Read` paths are rewritten relative to the inferred working directory.
5. Summarize each main command heuristically.
   - `rg`, `grep`, `git grep`, `fd`, `find`, `ag`, `ack`, `pt` -> `Search`
   - `ls`, `eza`, `exa`, `tree`, `du`, `git ls-files`, `rg --files` -> `ListFiles`
   - `cat`, `bat`, `less`, `more`, `head`, `tail`, `awk`, `nl`, `sed -n ...` -> `Read`
   - everything else -> `Unknown`
6. Simplify noisy helpers.
   - Drops or collapses `cd`, `true`, formatting helpers (`head`, `tail`, `wc`, `nl`, etc.) in pipeline contexts.
7. If ambiguity remains, return `Unknown`.

This produces summaries that are optimized for UX rather than exact shell semantics.

### 2. Bash parsing flow

The bash parser is the most important structural primitive in the crate.

- It parses the script into a tree-sitter AST.
- It walks the full tree and rejects any named node outside a small allowlist.
- It also rejects punctuation/operators outside a safe token set.
- Accepted command words may include:
  - bare words,
  - numbers,
  - single-quoted strings,
  - double-quoted strings with only literal string content,
  - concatenations of literal pieces such as `-g"*.rs"`.
- Rejected constructs include:
  - redirects,
  - subshells,
  - command substitution,
  - variable expansion,
  - process substitution,
  - control flow,
  - parse-error states.

That same primitive is reused by both `parse_command` and the Unix safe/dangerous checks, so bash parsing is a shared trust boundary.

### 3. Safe-command flow

`is_known_safe_command` uses an allowlist model:

1. Normalize `zsh` to `bash` for equivalent Unix behavior.
2. Ask the Windows PowerShell-specific classifier first.
3. Check direct argv safety with `is_safe_to_call_with_exec`.
4. If it is a `bash -lc` / `zsh -lc` script, parse the shell script into plain commands and require every command in the sequence to be individually safe.

Important characteristics:

- Safety is defined as “read-only enough to auto-approve”, not “cannot do harm under any environment”.
- The allowlist covers common read-oriented commands such as `cat`, `grep`, `head`, `tail`, `ls`, `pwd`, `which`, `rg`, selected `git` subcommands, and constrained `sed -n`.
- Some commands are safe only under argument restrictions:
  - `base64` becomes unsafe with output-file options.
  - `find` becomes unsafe with `-exec`, `-delete`, or file-writing options.
  - `rg` becomes unsafe with external-command options like `--pre`.
  - `git` becomes unsafe with config/repository override flags or mutating branch usage.

### 4. Dangerous-command flow

`command_might_be_dangerous` is narrower and more targeted than the safe-command check.

- On Unix-like platforms it currently focuses on obvious cases such as:
  - `rm -f`
  - `rm -rf`
  - `sudo <dangerous command>`
- It also checks parsed `bash -lc` / `zsh -lc` sequences so a dangerous inner plain command is still detected.
- On Windows it adds heuristics for:
  - PowerShell `Start-Process` / `Invoke-Item` / ShellExecute-style URL launches,
  - browser or Explorer launches with URLs,
  - `cmd /c start https://...`,
  - force delete patterns like `Remove-Item -Force`, `del /f`, `rmdir /s /q`.

The “dangerous” predicate is therefore a high-signal blocklist, not a complete model of all mutation.

## PowerShell design

Windows PowerShell handling is the most specialized part of the crate.

### Why it exists

PowerShell syntax is too rich to safely parse with simple token splitting. The crate therefore uses a real PowerShell parser process for the safe-command path.

### Safe path

- Rust launches a persistent child PowerShell process running `powershell_parser.ps1`.
- Requests are JSON lines containing:
  - a monotonic request id,
  - a base64-encoded UTF-16LE script.
- The PowerShell script uses `[System.Management.Automation.Language.Parser]::ParseInput(...)`.
- It lowers only AST shapes that can be flattened into literal argv-like words.
- Unsupported constructs return `unsupported`, not a guess.
- Rust caches one parser process per executable path (`powershell.exe` vs `pwsh.exe`) because they may accept different syntax.

This is a strong design choice: the crate pays complexity to avoid hand-rolled PowerShell parsing for the “safe” decision.

### Dangerous path

`windows_dangerous_commands.rs` is deliberately lighter weight:

- It uses best-effort parsing/shlex-style splitting instead of the AST bridge.
- The goal is not full classification, only spotting dangerous URL launches and force-delete patterns.

That means the PowerShell safe path is structurally stronger than the PowerShell dangerous path.

## Heuristic command model in `parse_command`

The largest file, `src/parse_command.rs`, implements a heuristic summary engine rather than a formal shell interpreter.

Key behaviors:

- Uses command-family-specific parsers instead of one generic parser.
- Shortens displayed paths with `short_display_path` to keep summaries concise.
- Preserves grep queries verbatim rather than shortening slash-containing patterns.
- Treats pipeline formatting tools as auxiliary and tries to surface the “primary” command.
- Rewrites read paths after `cd`, even across shell-wrapped scripts.
- Uses connector splitting to summarize multi-step command sequences in execution order.

This makes the output appropriate for UI/event logs, but it is intentionally lossy.

## Dependency roles

### Parsing and token handling

- `tree-sitter`, `tree-sitter-bash`
  - structural bash parsing and syntax rejection.
- `shlex`
  - shell-like token splitting/joining for normalized summaries.

### PowerShell bridge

- `serde`, `serde_json`
  - request/response framing for the PowerShell parser child.
- `base64`
  - transports UTF-16LE PowerShell source safely through JSON/stdin.

### Pattern matching and URL checks

- `regex`
  - trims/sanitizes URL-like PowerShell/CMD tokens.
- `url`
  - validates `http` / `https` launch targets.
- `once_cell`
  - cached regex initialization for Windows dangerous-command logic.

### Environment / executable discovery

- `which`
  - locate PowerShell executables in `PATH`.
- `codex-utils-absolute-path`
  - normalize discovered executable paths.

### Shared protocol type

- `codex-protocol`
  - defines the `ParsedCommand` enum consumed by higher-level crates.

## Testing profile

The crate is heavily test-driven, with **177 unit tests** across the source tree.

Approximate breakdown:

- `parse_command.rs`: 79 tests
- `windows_dangerous_commands.rs`: 39 tests
- `bash.rs`: 28 tests
- `is_safe_command.rs`: 16 tests
- `windows_safe_commands.rs`: 8 tests
- `powershell.rs`: 4 tests
- `is_dangerous_command.rs`: 2 tests
- `powershell_parser.rs`: 1 Windows-only test

What the tests emphasize:

- parser edge cases and regressions,
- shell wrapper handling,
- quoting and concatenation behavior,
- git/ripgrep/find safety restrictions,
- Windows-specific dangerous command patterns,
- PowerShell AST parsing and wrapper recognition.

This test mix strongly suggests the crate evolves by regression-driven patching of real command examples.

## Design assessment

### Strengths

- **Conservative by default**: ambiguous inputs degrade to `Unknown`/unsafe instead of overclassification.
- **Real syntax parsing where it matters most**: bash uses tree-sitter; PowerShell safety uses the actual PowerShell parser.
- **Good separation of concerns**:
  - summarization,
  - safe classification,
  - dangerous classification.
- **Policy-aware heuristics**: git, ripgrep, find, and sed handling encode security-relevant argument semantics instead of just command names.
- **Strong regression coverage**: the large test suite is the main safety net for a heuristic-heavy crate.

### Tradeoffs / limitations

- `parse_command.rs` is very large and centralizes many unrelated heuristics, which makes local reasoning and extension harder.
- Summarization and safety share some concepts but not one common intermediate representation, so logic is duplicated.
- The PowerShell safe path is AST-based, but the dangerous path is mostly token/regex-based.
- Nontrivial shell semantics are intentionally not modeled, which is good for safety but can reduce summary fidelity.
- The crate is highly tuned to a known command vocabulary; unfamiliar tools default to `Unknown`.

## Open questions

1. **Should `parse_command.rs` be split by command family?**
   - Example candidates: read commands, search commands, list commands, shell wrappers, simplification passes.

2. **Should safe/dangerous/summarization share a common parsed intermediate form?**
   - Today each subsystem partially re-derives structure.

3. **Should PowerShell dangerous detection also use the AST parser?**
   - That would reduce mismatch between the safe and dangerous paths, though at a runtime/complexity cost.

4. **Should the Unix dangerous-command model expand beyond `rm -f/-rf`?**
   - Current coverage is intentionally narrow but may miss other clearly destructive patterns.

5. **Should PowerShell summarization become richer than `Unknown`?**
   - The crate already has strong PowerShell parsing infrastructure for safety; some of that could potentially drive better `ParsedCommand` summaries.

6. **Should path-shortening be configurable?**
   - `short_display_path` is good for UX but can hide context in large monorepos.

7. **Should there be property tests or corpus/fuzz tests for shell parsing?**
   - The current suite is strong on regression examples but less explicit about adversarial generation.

## Bottom line

This crate is best understood as a **security- and UX-oriented command classifier**. Its architecture combines:

- structural parsing for shell wrappers,
- targeted allowlists/blocklists for approval decisions,
- heuristic semantic summaries for UI,
- extensive regression tests driven by real command patterns.

The code is intentionally conservative and operationally pragmatic. The main maintainability risk is not correctness drift in one algorithm, but the growing amount of handwritten heuristic logic concentrated in `parse_command.rs` and the partial duplication between Windows/Unix and safe/dangerous/summarization paths.
