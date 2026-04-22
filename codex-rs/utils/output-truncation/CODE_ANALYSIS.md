# codex-utils-output-truncation crate analysis

## Overview

`codex-utils-output-truncation` is a small adapter crate that turns the low-level UTF-8-safe truncation helpers from `codex-utils-string` into protocol-aware output-shaping utilities for Codex tool results. Its main value is not implementing the core slicing algorithm itself, but standardizing how the workspace truncates:

- plain text tool output
- formatted exec output that needs line-count metadata
- multimodal `FunctionCallOutputContentItem` payloads that may mix text and images

The crate surface is concentrated in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/src/lib.rs#L1-L142), with tests in [truncate_tests.rs](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/src/truncate_tests.rs#L1-L281). Packaging is defined in [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/Cargo.toml#L1-L15) and mirrored for Bazel in [BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/BUILD.bazel#L1-L6).

## Concrete responsibilities

1. **Expose a workspace-standard truncation policy**
   - Re-export [TruncationPolicy](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs#L2948-L2980) so higher layers can speak in either byte budgets or approximate token budgets.

2. **Truncate plain strings consistently**
   - Route byte-based and token-based truncation through one API, [truncate_text](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/src/lib.rs#L22-L27), backed by `codex-utils-string`.

3. **Add human-readable truncation metadata**
   - When truncation happens, [formatted_truncate_text](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/src/lib.rs#L12-L20) prepends `Total output lines: N` so callers preserve some structural context even after clipping the payload.

4. **Handle multimodal function-call output**
   - Support whole-payload text merging with [formatted_truncate_text_content_items_with_policy](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/src/lib.rs#L29-L71).
   - Support sequential budget consumption across mixed items with [truncate_function_output_items_with_policy](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/src/lib.rs#L73-L130).

5. **Bridge numeric type boundaries**
   - Provide [approx_tokens_from_byte_count_i64](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/src/lib.rs#L132-L139) for interfaces that still use signed integers.

## Public API

### Re-exports

The crate intentionally re-exports several lower-level helpers instead of forcing callers to depend on two utility crates:

- [TruncationPolicy](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs#L2948-L2980)
- [approx_bytes_for_tokens](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/truncate.rs#L76-L78)
- [approx_token_count](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/truncate.rs#L71-L74)
- [approx_tokens_from_byte_count](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/truncate.rs#L80-L84)

This makes the crate a single import point for most output-budget calculations.

### Native functions

- `formatted_truncate_text(content: &str, policy: TruncationPolicy) -> String`
  - Defined in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/src/lib.rs#L12-L20).
  - Returns the original string when the content fits the policy’s byte-equivalent budget.
  - Otherwise truncates and prefixes the original line count.

- `truncate_text(content: &str, policy: TruncationPolicy) -> String`
  - Defined in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/src/lib.rs#L22-L27).
  - Dispatches to `truncate_middle_chars` for byte budgets and `truncate_middle_with_token_budget(...).0` for token budgets.

- `formatted_truncate_text_content_items_with_policy(items: &[FunctionCallOutputContentItem], policy: TruncationPolicy) -> (Vec<FunctionCallOutputContentItem>, Option<usize>)`
  - Defined in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/src/lib.rs#L29-L71).
  - Merges all text items into one newline-joined string, truncates that merged text when needed, preserves images, and returns `Some(original_token_count)` only when truncation occurred.

- `truncate_function_output_items_with_policy(items: &[FunctionCallOutputContentItem], policy: TruncationPolicy) -> Vec<FunctionCallOutputContentItem>`
  - Defined in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/src/lib.rs#L73-L130).
  - Preserves item structure more faithfully by spending the budget left-to-right across text items while always passing through images.
  - Appends a synthetic summary item like `[omitted 2 text items ...]` when some text items are dropped entirely.

- `approx_tokens_from_byte_count_i64(bytes: i64) -> i64`
  - Defined in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/src/lib.rs#L132-L139).
  - Clamps non-positive input to `0` and saturates on conversion overflow.

## Module structure

- [src/lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/src/lib.rs)
  - Entire crate implementation.
  - Re-exports shared helpers.
  - Implements all public behavior.

- [src/truncate_tests.rs](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/src/truncate_tests.rs)
  - Single unit-test module covering both plain-text and multimodal behavior.

There are no submodules beyond tests, which fits the crate’s narrow scope.

## Main flow

### 1. Plain text truncation

For [truncate_text](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/src/lib.rs#L22-L27):

1. Match on `TruncationPolicy`.
2. For `Bytes(n)`, delegate to [truncate_middle_chars](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/truncate.rs#L6-L9).
3. For `Tokens(n)`, delegate to [truncate_middle_with_token_budget](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/truncate.rs#L11-L36) and discard the optional original-token-count return value.

This keeps the crate policy-aware while leaving UTF-8 boundary correctness to `codex-utils-string`.

### 2. Formatted plain text truncation

For [formatted_truncate_text](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/src/lib.rs#L12-L20):

1. Compare `content.len()` against `policy.byte_budget()`.
2. Return the original content unchanged when it fits.
3. Count original lines with `content.lines().count()`.
4. Truncate via `truncate_text`.
5. Prefix the result with `Total output lines: {total_lines}`.

This function is optimized for human/model-facing summaries where the caller wants a line-count hint only when truncation actually happened.

### 3. Merged multimodal truncation

For [formatted_truncate_text_content_items_with_policy](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/src/lib.rs#L29-L71):

1. Collect only `InputText` items into `text_segments`.
2. Return the original payload unchanged when no text items exist.
3. Join all text segments with `\n` into a single `combined` buffer.
4. Compare `combined.len()` against `policy.byte_budget()`.
5. If it fits, return the original items unchanged.
6. If it does not fit:
   - replace all text items with a single formatted/truncated `InputText`
   - append every original `InputImage` item unchanged after that text item
   - return `Some(approx_token_count(&combined))`

This path intentionally treats the text portion as one logical transcript. It is used in code-mode flows where all-text payloads can be compacted into one readable summary without preserving fine-grained item boundaries.

### 4. Sequential multimodal truncation

For [truncate_function_output_items_with_policy](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/src/lib.rs#L73-L130):

1. Initialize `remaining_budget` in bytes or approximate tokens.
2. Iterate items in original order.
3. For each `InputText` item:
   - compute its cost as `text.len()` for byte budgets or `approx_token_count(text)` for token budgets
   - keep the whole item if it fits
   - otherwise truncate just that item to the remaining budget and then exhaust the budget
   - count fully omitted text items when the remaining budget is zero or truncation yields an empty snippet
4. For each `InputImage` item:
   - always preserve it
   - do not charge it against the budget
5. Append an omission summary text item if any text items were skipped entirely.

This path is more structure-preserving than the merged path and is the safer default when images are interleaved with text or when callers want to keep item ordering meaningful.

## Dependencies and build integration

### Cargo dependencies

From [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/Cargo.toml#L1-L15):

- `codex-protocol`
  - Provides `TruncationPolicy` and `FunctionCallOutputContentItem`.
  - Workspace path is declared in the root [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/Cargo.toml#L159-L159).

- `codex-utils-string`
  - Provides the actual truncation and approximate token/byte helpers.
  - Workspace path is declared in the root [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/Cargo.toml#L198-L198).

- `pretty_assertions`
  - Dev-only dependency used by unit tests for readable diffs.
  - Version comes from the root [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/Cargo.toml#L287-L287).

### Upstream types the crate depends on

- [TruncationPolicy](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs#L2948-L2980)
  - Encapsulates the budget type.
  - Supplies `byte_budget()` and `token_budget()`, which this crate relies on heavily.

- [FunctionCallOutputContentItem](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L1399-L1411)
  - Defines the multimodal output shape with `InputText` and `InputImage`.

### Bazel integration

[BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/BUILD.bazel#L1-L6) registers the crate as `codex_utils_output_truncation`, keeping it available in Bazel-based builds alongside Cargo.

## How the crate is used in the workspace

Representative call sites show the intended split between formatting and structural truncation:

- [tools/mod.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/tools/mod.rs#L70-L105)
  - Uses `truncate_text` and `formatted_truncate_text` for exec output rendered as strings.

- [tools/code_mode/mod.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/tools/code_mode/mod.rs#L236-L252)
  - Uses `formatted_truncate_text_content_items_with_policy` for all-text content-item payloads.
  - Falls back to `truncate_function_output_items_with_policy` when images are present.

- [context_manager/history.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/context_manager/history.rs#L459-L476)
  - Truncates stored function output payloads before they are carried in conversation history.

- [unified_exec/process.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/unified_exec/process.rs#L253-L270)
  - Uses `formatted_truncate_text` to bound sandbox-denied error snippets.

This usage pattern makes the crate the workspace’s protocol-aware “last mile” truncation layer between raw strings and model-facing/tool-facing payloads.

## Testing

The crate currently has one test module, [truncate_tests.rs](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/src/truncate_tests.rs#L1-L281), with 15 unit tests.

Coverage areas:

- **Plain string behavior**
  - Under-limit passthrough for bytes and tokens.
  - Over-limit truncation markers for bytes and tokens.
  - Inclusion of original line counts only when truncation happens.

- **UTF-8 handling**
  - Verifies truncation does not break emoji-heavy input on byte budgets.

- **Multimodal content-item behavior**
  - Preserves original items when under budget.
  - Merges text items and appends images in the formatted path.
  - Preserves empty-leading-text semantics in the merge path.
  - Sequentially truncates across multiple items and reports omitted text items.

- **Numeric conversion**
  - Confirms `approx_tokens_from_byte_count_i64` clamps negative and zero inputs.

Observed status:

- `cargo test -p codex-utils-output-truncation` passes.
- Current local result: 15 passed, 0 failed.

## Design notes

### Positive design choices

- **Clear layering**
  - The crate does not reimplement UTF-8 truncation logic; it wraps [codex-utils-string](file:///Users/bytedance/project/codex/codex-rs/utils/string/src/truncate.rs#L1-L155) and adds protocol-specific behavior.

- **Small public surface**
  - All core behavior fits in one file, which keeps policy semantics easy to audit.

- **Protocol awareness without deep coupling**
  - The crate knows just enough about `FunctionCallOutputContentItem` to preserve images and rewrite text, without owning broader model/message logic.

- **Two distinct multimodal strategies**
  - The merged path favors readability and compactness.
  - The sequential path favors structural fidelity and order preservation.

- **Explicit omission reporting**
  - The summary item in the sequential path avoids silent data loss once the budget is exhausted.

### Trade-offs and caveats

- **Budget checks are approximate for token mode**
  - The crate uses `policy.byte_budget()` and `approx_token_count`, both based on the shared `4 bytes/token` heuristic, so token-mode truncation is intentionally approximate rather than tokenizer-accurate.

- **Formatted outputs exceed the raw truncation budget**
  - `formatted_truncate_text` first truncates the content and then prefixes `Total output lines: ...`, so the final string can exceed the nominal byte/token-equivalent budget.

- **Merged content-item truncation rewrites structure**
  - `formatted_truncate_text_content_items_with_policy` collapses all text items into one and moves all images after that text item, which improves compactness but loses original interleaving.

- **Images are effectively free**
  - Neither multimodal path charges `InputImage` items against the budget. That is likely intentional for text-budgeting, but it means final payload size is not bounded by one global multimodal cap.

- **One API discards original token-count metadata**
  - `truncate_text` uses `truncate_middle_with_token_budget(...).0`, so callers that care about the pre-truncation token estimate must choose a more specialized API.

## Open questions

1. **Should formatted APIs guarantee a hard final-size cap?**
   - [formatted_truncate_text](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/src/lib.rs#L12-L20) adds metadata after truncation, so the final string can exceed the requested budget. That is fine for soft budgeting, but not for strict transport limits.

2. **Should merged multimodal truncation preserve original text/image interleaving?**
   - [formatted_truncate_text_content_items_with_policy](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/src/lib.rs#L57-L68) always emits one text item followed by all images. If consumers rely on positional correspondence between nearby text and images, this rewrite may be too lossy.

3. **Should image items count toward a holistic payload budget?**
   - [truncate_function_output_items_with_policy](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/src/lib.rs#L114-L118) preserves images unconditionally. That matches a text-truncation interpretation, but not a total-payload-budget interpretation.

4. **Should token-aware APIs surface truncation metadata more consistently?**
   - `truncate_text` hides the original token count, while [formatted_truncate_text_content_items_with_policy](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/src/lib.rs#L70-L70) returns one. The asymmetry suggests callers may need more explicit guidance on which API to choose.

5. **Should there be direct tests for the sequential byte-budget path on mixed items?**
   - Current tests cover sequential truncation in token mode, but there is no equivalent mixed-item byte-budget test in [truncate_tests.rs](file:///Users/bytedance/project/codex/codex-rs/utils/output-truncation/src/truncate_tests.rs#L1-L281).

## Summary

`codex-utils-output-truncation` is a thin but useful workspace adapter. It centralizes how Codex turns approximate byte/token budgets into human-readable and protocol-aware truncated outputs, especially for tool results and multimodal function-call payloads. Its implementation is simple and well covered for its size. The main design tension is not low-level correctness, which is delegated well, but semantics: whether truncation is meant to be a soft readability aid or a hard final-size constraint.
