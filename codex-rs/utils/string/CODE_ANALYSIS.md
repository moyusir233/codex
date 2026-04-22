# codex-utils-string code analysis

## Overview

`codex-utils-string` is a small workspace utility crate that centralizes string-focused helpers used across the larger `codex-rs` workspace. The crate is intentionally lightweight: it exposes a handful of UTF-8-aware truncation helpers plus several unrelated but practical string utilities for metric tags, UUID extraction, markdown file-link normalization, and byte-budget slicing.

The crate surface is defined in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/lib.rs#L1-L98) and [truncate.rs](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/truncate.rs#L1-L155). It is packaged as `codex-utils-string` in [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/utils/string/Cargo.toml#L1-L14) and also exposed to Bazel builds via [BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/utils/string/BUILD.bazel#L1-L6).

## Concrete responsibilities

1. **UTF-8-safe truncation**
   - Preserve a prefix and suffix while removing the middle of a string without cutting through UTF-8 code point boundaries.
   - Offer both byte-budget and approximate token-budget entry points.
   - Provide approximate byte/token conversion helpers for callers that reason in model-token units.

2. **Safe prefix slicing**
   - Return a borrowed `&str` prefix that fits within a byte budget and still ends on a character boundary.

3. **Metric tag sanitization**
   - Normalize arbitrary input into a restricted ASCII character set suitable for metric tag values.
   - Collapse invalid content into a stable fallback (`"unspecified"`).

4. **UUID extraction**
   - Find UUID-shaped substrings efficiently with a lazily initialized regex.

5. **Markdown hash-location normalization**
   - Convert markdown fragments like `#L74C3-L76C9` into terminal-oriented suffixes like `:74:3-76:9`.

## Public API

### Re-exported truncation API

Defined in [truncate.rs](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/truncate.rs#L4-L155) and re-exported from [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/lib.rs#L1-L8):

- `truncate_middle_chars(s: &str, max_bytes: usize) -> String`
  - Keeps roughly half the byte budget on the left and half on the right, removes the middle, and inserts a marker like `…21 chars truncated…`.
- `truncate_middle_with_token_budget(s: &str, max_tokens: usize) -> (String, Option<u64>)`
  - Uses a fixed `4 bytes/token` heuristic.
  - Returns `None` when no truncation occurs, otherwise returns `Some(original_token_count)`.
- `approx_token_count(text: &str) -> usize`
  - Ceiling-divides byte length by 4.
- `approx_bytes_for_tokens(tokens: usize) -> usize`
  - Multiplies tokens by 4 with saturation.
- `approx_tokens_from_byte_count(bytes: usize) -> u64`
  - Ceiling-divides byte count by 4 as `u64`.

### Native `lib.rs` API

- `take_bytes_at_char_boundary(s: &str, maxb: usize) -> &str` in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/lib.rs#L9-L24)
  - Scans `char_indices()` until the next code point would exceed `maxb`.
  - Returns a borrowed prefix; does not allocate.
- `sanitize_metric_tag_value(value: &str) -> String` in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/lib.rs#L26-L49)
  - Allows only ASCII alphanumeric plus `.`, `_`, `-`, `/`.
  - Replaces everything else with `_`, trims leading/trailing `_`, falls back to `"unspecified"`, and caps length at 256.
- `find_uuids(s: &str) -> Vec<String>` in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/lib.rs#L51-L63)
  - Uses `OnceLock<regex_lite::Regex>` so the regex is compiled once.
- `normalize_markdown_hash_location_suffix(suffix: &str) -> Option<String>` in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/lib.rs#L65-L98)
  - Accepts `#Lline`, `#LlineCcol`, and range variants with `-`.

## Module structure

- [src/lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/lib.rs)
  - Crate root.
  - Re-exports the truncation module.
  - Defines the non-truncation helpers and unit tests for them.
- [src/truncate.rs](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/truncate.rs)
  - Holds all truncation and byte/token estimation logic.
  - Keeps most helpers private (`truncate_with_byte_estimate`, `split_string`, `split_budget`, `format_truncation_marker`, `removed_units`, `assemble_truncated_output`).
- [src/truncate/tests.rs](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/truncate/tests.rs)
  - Focused white-box tests for the truncation internals.

## Core flow

### Character-budget truncation flow

For `truncate_middle_chars`:

1. [truncate_middle_chars](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/truncate.rs#L6-L9) delegates to `truncate_with_byte_estimate(..., use_tokens = false)`.
2. [truncate_with_byte_estimate](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/truncate.rs#L38-L69):
   - Returns early for empty strings.
   - Returns only a marker when `max_bytes == 0`.
   - Returns the original string when `s.len() <= max_bytes`.
   - Splits the budget evenly with [split_budget](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/truncate.rs#L126-L129).
   - Extracts UTF-8-safe prefix/suffix slices with [split_string](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/truncate.rs#L86-L124).
   - Computes removed character count with [removed_units](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/truncate.rs#L139-L145).
   - Builds the final output with [assemble_truncated_output](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/truncate.rs#L147-L153).

### Token-budget truncation flow

For `truncate_middle_with_token_budget`:

1. [truncate_middle_with_token_budget](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/truncate.rs#L11-L36) returns early for empty strings.
2. It uses [approx_bytes_for_tokens](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/truncate.rs#L76-L78) to convert the token budget into an approximate byte budget.
3. It skips truncation when the input byte length already fits inside that approximate byte budget.
4. Otherwise it truncates through the same shared `truncate_with_byte_estimate` pipeline, but asks the marker to report removed approximate tokens.
5. It computes the original approximate token count with [approx_token_count](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/truncate.rs#L71-L74) and only returns `Some(total_tokens)` when the output actually changed.

### UTF-8 boundary handling

[split_string](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/truncate.rs#L86-L124) is the key correctness function:

- It walks `char_indices()` once.
- It extends the prefix only when the full code point still fits inside the left-side byte budget.
- It starts the suffix only at valid character boundaries at or beyond `len - end_bytes`.
- It counts removed characters only for code points that fall into the middle region.
- It resolves overlapping budgets by forcing `suffix_start >= prefix_end`, which avoids slicing panics and double-including bytes.

## Dependencies and build integration

### Cargo dependencies

From [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/utils/string/Cargo.toml#L1-L14):

- `regex-lite`
  - Only runtime dependency.
  - Used exclusively by `find_uuids`.
- `pretty_assertions`
  - Dev dependency for clearer test diffs.

The concrete workspace versions come from the root [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/Cargo.toml#L287-L294):

- `pretty_assertions = "1.4.1"`
- `regex-lite = "0.1.8"`

### Bazel integration

[BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/utils/string/BUILD.bazel#L1-L6) registers the crate as `codex_rust_crate(name = "string", crate_name = "codex_utils_string")`, which keeps this crate consumable from both Cargo and Bazel-based builds.

## How the crate is used in the workspace

Representative consumers make the responsibilities concrete:

- [utils/output-truncation/lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/src/lib.rs#L1-L120)
  - Re-exports the token estimation helpers.
  - Uses the truncation functions as the workspace-standard mechanism for clipping tool output.
- [core/src/tools/handlers/list_dir.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/tools/handlers/list_dir.rs#L20-L25)
  - Uses `take_bytes_at_char_boundary` to cap rendered directory entries safely.
- [core/src/tools/context.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/tools/context.rs#L20-L23)
  - Uses the same helper for telemetry/log preview shaping.
- [otel/src/metrics/client.rs](file:///Users/bytedance/project/codex/codex-rs/otel/src/metrics/client.rs#L8-L13)
  - Uses `sanitize_metric_tag_value` before attaching tag values to metrics.
- [windows-sandbox-rs/src/setup_error.rs](file:///Users/bytedance/project/codex/codex-rs/windows-sandbox-rs/src/setup_error.rs#L11-L16)
  - Documents that error codes become metric tags, reinforcing why sanitization lives in this crate.
- [tui/src/markdown_render.rs](file:///Users/bytedance/project/codex/codex-rs/tui/src/markdown_render.rs#L1-L13)
  - Uses `normalize_markdown_hash_location_suffix` to translate markdown file links into terminal-friendly file references.

This usage pattern shows that `codex-utils-string` acts as a low-level, dependency-light string primitives crate shared by higher-level subsystems.

## Testing

The crate has two unit-test groups:

- Tests embedded in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/lib.rs#L100-L163)
  - Cover multiple UUID matches.
  - Confirm invalid UUID-like text is ignored.
  - Verify behavior around non-ASCII prefixes and non-overlapping regex matches.
  - Validate metric-tag fallback and invalid-character replacement.
  - Validate markdown location normalization for single locations and ranges.
- Tests in [truncate/tests.rs](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/truncate/tests.rs#L1-L117)
  - Cover empty inputs, zero budgets, prefix-only/suffix-only behavior, overlapping budgets, and UTF-8 boundaries.
  - Verify truncation marker text for both byte-oriented and token-oriented APIs.

Observed test status:

- `cargo test -p codex-utils-string` passes.
- Current result: 17 unit tests passed, 0 failed.

## Design notes

### Positive design choices

- **UTF-8 correctness over raw byte slicing**
  - Every slicing operation is routed through `char_indices()`-derived boundaries.
- **Small public surface**
  - The crate exposes only a few stable helpers and hides composition details.
- **Low dependency footprint**
  - Only `regex-lite` is required at runtime.
- **Cheap hot-path behavior**
  - `take_bytes_at_char_boundary` borrows instead of allocating.
  - `find_uuids` compiles its regex once with `OnceLock`.
- **Cross-cutting reuse**
  - The crate cleanly centralizes logic that would otherwise be duplicated in telemetry, TUI rendering, error reporting, and output truncation paths.

### Trade-offs and caveats

- **Approximate tokens are byte-based, not model-based**
  - The crate intentionally uses a fixed `4 bytes/token` heuristic, which is simple and stable but not tokenizer-accurate.
- **Truncation budgets govern preserved input bytes, not final output size**
  - The marker string is appended after the preserved prefix/suffix budget is chosen.
  - As a result, `truncate_middle_chars` and `truncate_middle_with_token_budget` can produce outputs whose final byte length exceeds the requested budget.
- **The API name `truncate_middle_chars` is somewhat misleading**
  - Its budget is bytes, while its marker reports removed characters.
- **Helper mix is pragmatic rather than domain-pure**
  - The crate groups several unrelated string utilities together because they are small and dependency-light, not because they model a single cohesive domain.

## Open questions

1. **Should truncation APIs guarantee final output fits the requested budget?**
   - The token-budget docs in [truncate.rs](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/truncate.rs#L11-L15) say “at most `max_tokens` approximate tokens”, but the implementation does not reserve space for the marker. If callers rely on a hard cap, the current behavior is surprising.

2. **Is `truncate_middle_chars` the right name for a byte-budget API?**
   - The function truncates based on bytes, not character count. The current name can cause misuse by callers expecting a true character-count budget.

3. **Should `find_uuids` require boundaries around matches?**
   - The regex in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/lib.rs#L53-L62) matches any UUID-shaped substring, even when it is embedded in a longer hex-like token. The existing non-ASCII test demonstrates prefix extraction from a longer tail. That may be useful, but it is less strict than many UUID parsers.

4. **Should markdown location parsing validate digits?**
   - [parse_markdown_hash_location_point](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/lib.rs#L92-L98) only checks for the `L`/`C` structure. Inputs like `#Labc` would normalize successfully even though they are not numeric line references.

5. **Should metric-tag sanitization tests cover length limits and mixed-edge cases?**
   - The current tests validate replacement and fallback behavior but do not exercise the 256-character cap or inputs that trim to only punctuation after replacement.

## Summary

`codex-utils-string` succeeds as a compact shared utility crate. Its strongest design qualities are UTF-8-safe truncation, a minimal runtime dependency set, and reuse across several higher-level workspace components. The main ambiguity is not correctness of slicing, but semantics: several APIs are “approximate” or “best effort” while their names or docs sound stronger than the implementation currently guarantees.
