# codex-utils-stream-parser Code Analysis

## Scope

This document analyzes the crate at `utils/stream-parser/`, with emphasis on:

- [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/Cargo.toml#L1-L11)
- [README.md](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/README.md#L1-L97)
- [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/lib.rs#L1-L23)
- all source modules under `src/`
- inline unit tests in each module
- representative workspace consumers in `core/`

The crate is small, but it sits on a sensitive streaming boundary. Its job is to consume partially delivered assistant output, preserve only text that is safe to render immediately, and extract hidden control/payload markup without requiring callers to reconstruct the entire response first.

## High-Level Responsibilities

The crate owns six concrete responsibilities:

1. Define a minimal trait, `StreamTextParser`, for incremental text parsing with explicit end-of-stream flushing.
2. Provide a generic inline-tag parser that can hide literal tags while extracting their contents across arbitrary chunk boundaries.
3. Specialize that generic parser for memory citations using `<oai-mem-citation>...</oai-mem-citation>`.
4. Provide a line-oriented block parser for `<proposed_plan>` sections that must appear on their own line.
5. Compose citation parsing and plan-block parsing into a higher-level assistant-output parser that matches Codex assistant streaming semantics.
6. Adapt any text parser to raw byte streams where UTF-8 code points may be split across chunk boundaries.

The practical effect is:

- UI and protocol layers can stream visible assistant text incrementally.
- hidden metadata can be extracted without leaking markup into rendered output.
- callers can use either one-shot helpers or long-lived stateful parsers depending on whether input is already buffered or still streaming.

## Crate Layout

The public surface is re-exported from [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/lib.rs#L1-L23):

- [stream_text.rs](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/stream_text.rs#L1-L36)
  - defines the core `StreamTextChunk<T>` result and `StreamTextParser` trait
- [inline_hidden_tag.rs](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/inline_hidden_tag.rs#L1-L208)
  - generic inline hidden-tag parser
- [citation.rs](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/citation.rs#L1-L179)
  - citation-specific wrapper plus `strip_citations`
- [tagged_line_parser.rs](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/tagged_line_parser.rs#L1-L199)
  - internal line-oriented block-tag state machine
- [proposed_plan.rs](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/proposed_plan.rs#L1-L212)
  - proposed-plan block parser and one-shot helpers
- [assistant_text.rs](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/assistant_text.rs#L1-L130)
  - composition layer for “assistant text” semantics
- [utf8_stream.rs](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/utf8_stream.rs#L1-L333)
  - byte-stream adapter with UTF-8 boundary handling

This split is sensible:

- `stream_text.rs` defines the common protocol.
- `inline_hidden_tag.rs` and `tagged_line_parser.rs` implement the two reusable parsing primitives.
- `citation.rs`, `proposed_plan.rs`, and `assistant_text.rs` encode repository-specific markup policy.
- `utf8_stream.rs` handles transport-level byte concerns orthogonally from markup parsing.

## Public API Surface

### `StreamTextChunk<T>` and `StreamTextParser`

The foundational abstraction lives in [stream_text.rs](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/stream_text.rs#L1-L36).

- `StreamTextChunk<T>` contains:
  - `visible_text: String`
  - `extracted: Vec<T>`
- `is_empty()` returns whether both outputs are empty.
- `StreamTextParser` requires:
  - `push_str(&mut self, chunk: &str) -> StreamTextChunk<Self::Extracted>`
  - `finish(&mut self) -> StreamTextChunk<Self::Extracted>`

This is a strong base abstraction for streaming because it makes the hidden state explicit and forces every implementation to define EOF behavior. The explicit `finish()` is particularly important because several parsers intentionally buffer ambiguous suffixes until the stream is complete.

### `InlineHiddenTagParser<T>`

Defined in [inline_hidden_tag.rs](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/inline_hidden_tag.rs#L26-L208).

Public data types:

- `InlineTagSpec<T>`: literal opener/closer pair plus logical tag identity
- `ExtractedInlineTag<T>`: extracted tag identity plus captured content
- `InlineHiddenTagParser<T>`: streaming parser over one or more literal inline tags

Key behavior:

- supports multiple literal tag types
- matches tags case-sensitively and non-nested
- preserves visible text outside tags
- extracts the interior text of hidden tags
- buffers suffixes that might be the start of an opener or closer split across chunks
- auto-closes an active tag at EOF and emits its buffered content

The most important implementation details are:

- `find_next_open()` chooses the earliest opener in `pending`, preferring the longest opener when multiple openers start at the same byte offset; that avoids short-prefix ambiguity such as `<a>` versus `<ab>` ([inline_hidden_tag.rs](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/inline_hidden_tag.rs#L72-L88)).
- `longest_suffix_prefix_len()` computes how much trailing text must remain buffered because it could still become the start of a delimiter in the next chunk ([inline_hidden_tag.rs](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/inline_hidden_tag.rs#L200-L208)).
- while a tag is active, normal visible output is suppressed and bytes are appended to the tag body until a literal close delimiter is found ([inline_hidden_tag.rs](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/inline_hidden_tag.rs#L128-L171)).

This module is the core “incremental inline markup” engine of the crate.

### `CitationStreamParser` and `strip_citations`

Defined in [citation.rs](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/citation.rs#L14-L76).

API:

- `CitationStreamParser::new()`
- `impl StreamTextParser<Extracted = String>`
- `strip_citations(text: &str) -> (String, Vec<String>)`

The parser is intentionally thin. It delegates almost everything to `InlineHiddenTagParser`, then maps `ExtractedInlineTag<CitationTag>` into bare `String` payloads. That keeps the generic parser reusable while giving workspace callers a simpler citation-specific type.

Notable semantics:

- citations are hidden from `visible_text`
- extracted citations are returned in encounter order
- incomplete citations auto-close on `finish()`
- malformed nested citations are not supported and degrade to literal-text behavior after the first close tag

### `ProposedPlanParser`, `ProposedPlanSegment`, and helpers

Defined in [proposed_plan.rs](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/proposed_plan.rs#L15-L115).

API:

- `ProposedPlanParser::new()`
- `ProposedPlanSegment`
  - `Normal(String)`
  - `ProposedPlanStart`
  - `ProposedPlanDelta(String)`
  - `ProposedPlanEnd`
- `strip_proposed_plan_blocks(text: &str) -> String`
- `extract_proposed_plan_text(text: &str) -> Option<String>`

This parser is different from citations in one important way: `<proposed_plan>` is treated as a line-scoped control block, not an inline substring. It uses `TaggedLineParser` internally because plan tags only count when the trimmed line equals the tag by itself.

That design preserves two outputs simultaneously:

- `visible_text`: normal assistant text with plan blocks removed
- `extracted`: an ordered event stream that retains both visible text and plan transitions

The ordered `ProposedPlanSegment` stream is a good design choice because it lets downstream code reconstruct exact sequencing if needed, while still exposing a convenience `visible_text` projection for common consumers.

### `AssistantTextStreamParser`

Defined in [assistant_text.rs](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/assistant_text.rs#L20-L73).

API:

- `AssistantTextStreamParser::new(plan_mode: bool)`
- `push_str(&mut self, chunk: &str) -> AssistantTextChunk`
- `finish(&mut self) -> AssistantTextChunk`
- `AssistantTextChunk`
  - `visible_text`
  - `citations`
  - `plan_segments`

This is the highest-level parser in the crate. It encodes repository policy for streamed assistant output:

- always strip and collect citations first
- optionally parse proposed-plan markup from the post-citation visible text
- emit a single combined result object per chunk

The order matters. Because citations are removed before plan parsing, a citation inside a proposed-plan block becomes invisible to the plan parser and appears separately in `citations`. That exact sequencing is validated in [assistant_text.rs tests](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/assistant_text.rs#L75-L129).

### `Utf8StreamParser<P>`

Defined in [utf8_stream.rs](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/utf8_stream.rs#L40-L178).

API:

- `Utf8StreamParser::new(inner)`
- `push_bytes(&mut self, chunk: &[u8]) -> Result<StreamTextChunk<P::Extracted>, Utf8StreamParserError>`
- `finish(&mut self) -> Result<StreamTextChunk<P::Extracted>, Utf8StreamParserError>`
- `into_inner(self) -> Result<P, Utf8StreamParserError>`
- `into_inner_lossy(self) -> P`
- `Utf8StreamParserError`
  - `InvalidUtf8 { valid_up_to, error_len }`
  - `IncompleteUtf8AtEof`

This adapter is careful about error semantics:

- partial UTF-8 sequences are buffered without emitting text
- a truly invalid chunk rolls back the entire pushed chunk so the wrapped parser never observes a partial prefix from that chunk
- `finish()` rejects an incomplete code point at EOF

That rollback behavior is an important design quality. It prevents the wrapped parser from entering a logically irreversible state based on bytes that later prove invalid.

## Internal Flow

The crate is easiest to understand as four related state-machine flows.

### 1. Inline hidden-tag flow

Implemented in [InlineHiddenTagParser::push_str](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/inline_hidden_tag.rs#L118-L174).

Flow:

1. Append the new chunk into `pending`.
2. If an inline tag is currently active:
   - search for its close delimiter in `pending`
   - if found, append preceding text to `active.content`, emit an extracted tag, and drain through the close delimiter
   - if not found, append everything except the longest possible close-prefix suffix into `active.content`, keep the ambiguous suffix buffered, and stop
3. If no tag is active:
   - search for the next opener across configured specs
   - if found, emit the preceding bytes as visible text, drain the opener, and activate that tag
   - if not found, emit everything except the longest possible opener-prefix suffix as visible text and keep the ambiguous suffix buffered
4. On `finish()`:
   - if a tag is active, emit it as auto-closed with all remaining buffered content
   - otherwise flush any buffered visible text

This parser is byte-indexed over `String`, but it remains Unicode-safe because it only slices at positions returned by `str::find()` or validated by `needle.is_char_boundary(k)`.

### 2. Tagged line flow

Implemented in [TaggedLineParser](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/tagged_line_parser.rs#L21-L199).

Flow:

1. Process input character-by-character.
2. While `detect_tag` is true, accumulate into `line_buffer` instead of emitting immediately.
3. For the trimmed-left current line prefix:
   - if it is empty or still a prefix of some open/close tag, keep buffering
   - once it can no longer become a tag, downgrade the buffered text to normal text and stop “tag detection” for the rest of that line
4. On newline, decide whether the completed line is:
   - an opening tag line
   - a closing tag line
   - ordinary text
5. While a tag is active, ordinary text is emitted as `TagDelta(tag, text)`.
6. On `finish()`, any buffered line is re-evaluated, and an active tag is auto-closed.

This parser exists because block tags have stricter semantics than inline tags. It intentionally avoids recognizing `<proposed_plan>` inside arbitrary prose such as `"  <proposed_plan> extra"`, which the tests confirm remains ordinary text.

### 3. Assistant text composition flow

Implemented in [AssistantTextStreamParser](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/assistant_text.rs#L24-L73).

Flow:

1. Each chunk first goes through `CitationStreamParser`.
2. The resulting citation-free `visible_text` is then:
   - returned directly when `plan_mode == false`
   - passed into `ProposedPlanParser` when `plan_mode == true`
3. Extracted citations are attached to the final `AssistantTextChunk`.
4. On `finish()`:
   - citation parser flushes first
   - plan parser flushes second if enabled
   - the two outputs are merged into one final chunk

This composition is narrow but valuable. It keeps core session logic from having to manage multiple parser instances and ordering rules at each streaming call site.

### 4. UTF-8 byte adaptation flow

Implemented in [Utf8StreamParser::push_bytes](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/utf8_stream.rs#L61-L109) and [finish](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/utf8_stream.rs#L111-L149).

Flow:

1. Append incoming bytes to `pending_utf8`.
2. Attempt `std::str::from_utf8(&pending_utf8)`.
3. If fully valid:
   - pass all decoded text to the inner parser
   - clear buffered bytes
4. If invalid with a concrete `error_len`:
   - restore `pending_utf8` to its previous length
   - return `InvalidUtf8`
5. If invalid only because the end is incomplete:
   - if zero valid bytes exist, return an empty chunk and keep buffering
   - otherwise feed only the valid prefix to the inner parser and retain the trailing incomplete bytes
6. On `finish()`, incomplete trailing UTF-8 becomes an explicit error.

This separation of byte buffering from markup parsing keeps all text parsers defined purely over `&str`, which is a clean layering decision.

## Dependency Analysis

The runtime dependency story is intentionally minimal.

### Runtime dependencies

[Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/Cargo.toml#L1-L11) declares no non-std runtime dependencies at all.

That is a strong fit for the crate’s role:

- no regex engine
- no parser combinator framework
- no streaming abstraction dependency
- no UTF-8 helper crate

The crate relies entirely on standard-library string/byte handling and small hand-written state machines.

### Dev dependencies

- `pretty_assertions` in [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/Cargo.toml#L10-L11)
  - used to make sequence and string diffs easier to inspect in unit tests

Architecturally, this is a very good dependency profile for a low-level utility crate. It minimizes compile-time cost and makes parser behavior easier to audit because the core logic is local.

## Testing Coverage

The crate has broad inline unit-test coverage for its size. Running `cargo test -p codex-utils-stream-parser` passes with 27 tests.

### `citation.rs` tests

[citation.rs tests](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/citation.rs#L78-L179) cover:

- citation tags split across chunk boundaries
- buffering of partial opener prefixes
- auto-closing an unterminated citation at EOF
- preserving a non-matching partial tag prefix as visible text
- one-shot `strip_citations`
- lack of nested-tag support

These tests validate the edge cases most likely to occur in real streamed model output.

### `inline_hidden_tag.rs` tests

[inline_hidden_tag.rs tests](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/inline_hidden_tag.rs#L210-L323) cover:

- multiple tag types
- non-ASCII delimiters
- longest-opener disambiguation
- constructor panics for empty delimiters

This is good evidence that the generic parser is meant as a reusable primitive, not just a citation-only helper.

### `tagged_line_parser.rs` tests

[tagged_line_parser.rs tests](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/tagged_line_parser.rs#L201-L248) cover:

- buffering an incomplete tag line across chunks
- rejecting tag-shaped lines with extra trailing text

The coverage is narrow but aligned with the parser’s core policy decision: tags must stand alone on the trimmed line.

### `proposed_plan.rs` tests

[proposed_plan.rs tests](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/proposed_plan.rs#L117-L212) cover:

- streaming visible text plus ordered plan segments
- non-tag lines that resemble a plan tag
- EOF auto-closing for unterminated plan blocks
- one-shot stripping and extraction helpers

These tests validate both the parser itself and the convenience APIs most callers will use.

### `assistant_text.rs` tests

[assistant_text.rs tests](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/assistant_text.rs#L75-L129) cover:

- citation parsing across seed/delta boundaries
- plan parsing after citation stripping

These are especially important because they verify the ordering contract between subparsers rather than only the behavior of each parser in isolation.

### `utf8_stream.rs` tests

[utf8_stream.rs tests](file:///Users/bytedance/project/codex/codex-rs/utils/stream-parser/src/utf8_stream.rs#L180-L333) cover:

- split UTF-8 code points across chunks
- rollback on invalid continuation bytes
- rollback when invalid bytes follow a valid prefix in the same chunk
- EOF behavior for incomplete code points
- `into_inner()` vs `into_inner_lossy()`

For a byte-stream adapter, this is the right kind of coverage: state retention, recovery, and EOF correctness.

## Representative Consumers

Although the crate is generic enough to stand alone, current workspace consumers show its real role.

### One-shot hidden-markup stripping

[stream_events_utils.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/stream_events_utils.rs#L64-L87) uses:

- `strip_citations`
- `strip_proposed_plan_blocks`

This path is for already-buffered assistant text where the caller wants a clean visible string, optionally plus a parsed memory citation.

### Stateful per-item assistant streaming

[turn.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/session/turn.rs#L1370-L1409) stores one `AssistantTextStreamParser` per response item ID in `AssistantMessageStreamParsers`.

That consumer confirms the intended usage pattern:

- parser instances are long-lived per streamed assistant item
- seed text and subsequent deltas go through the same parser state
- `finish_item()` flushes buffered suffixes and removes parser state when the item completes

### Completed-item plan extraction

[turn.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/session/turn.rs#L1736-L1751) uses `extract_proposed_plan_text` and then `strip_citations` on the extracted plan text before completing plan-mode state.

This shows the crate serves both streaming consumers and post-hoc analysis of completed content.

## Design Assessment

Overall, the crate is well designed for a tricky problem: parsing malformed-or-partial streamed text without rendering hidden markup too early.

### Strengths

- Narrow abstractions
  - one tiny trait, a small result type, and a few focused parsers
- Correct buffering model
  - ambiguous suffixes remain buffered until disambiguated by later chunks or EOF
- Good composition
  - generic primitives power repository-specific parsers without leaking complexity into callers
- Minimal dependency footprint
  - everything important is visible in local code
- Clear failure behavior
  - UTF-8 errors are explicit and rollback avoids partial parser corruption
- Good tests for streaming edge cases
  - especially chunk-splitting, incomplete delimiters, and malformed UTF-8

### Tradeoffs and constraints

- Tag matching is literal and case-sensitive
  - no normalization, no escaping, no attribute parsing
- Inline tags are non-nested
  - malformed nested structures degrade in a predictable but not HTML-like way
- Plan tags are line-based
  - `<proposed_plan>` must occupy the trimmed line alone
- EOF auto-closes active hidden blocks
  - practical for incomplete model output, but it also means malformed streams are partially accepted instead of rejected
- `AssistantTextStreamParser` composes only citations and plan blocks
  - adding more hidden syntaxes would likely require either another composition layer or a more declarative pipeline

## Open Questions

The current implementation looks solid, but a few design questions are worth documenting.

1. Should auto-closing at EOF remain the default for all hidden constructs?
   - This is convenient for model output, but it can mask malformed markup and may not be appropriate for every future parser.

2. Should nested inline tags ever be supported?
   - Today they are explicitly unsupported, and the citation tests lock in that behavior. If upstream output becomes richer, `InlineHiddenTagParser` may need a stack-based model.

3. Should `TaggedLineParser` expose a public API?
   - It is currently crate-private, but it already represents a useful generic primitive for line-scoped control blocks.

4. Should the crate add a parser reset/reuse API?
   - Current parsers are long-lived until `finish()`, but there is no explicit `reset()` for pooled or reused parser instances.

5. Should `extract_proposed_plan_text()` return the last plan block, all plan blocks, or reject multiple blocks explicitly?
   - The current implementation effectively keeps the content from the most recently started plan block.

6. Is the current scanning strategy sufficient for very large pending buffers or many tag specs?
   - The present implementation is simple and probably correct for small tag sets, but it is not optimized like a finite automaton or prefix table.

7. Should byte offsets in `Utf8StreamParserError::InvalidUtf8` be documented more explicitly as offsets within the buffered parser state rather than necessarily the original transport chunk?
   - The current docs are decent, but recovery code may care about exact interpretation.

## Bottom Line

`codex-utils-stream-parser` is a focused streaming-text utility crate built around explicit parser state and explicit EOF flushing. Its main value is not just “remove some tags”; it provides the policy and mechanics needed to safely consume assistant output that arrives:

- incrementally
- with hidden control markup
- with arbitrary chunk boundaries
- and sometimes as raw bytes rather than already-decoded UTF-8 text

The crate’s strongest design choice is that it separates concerns cleanly:

- generic parsing primitives for inline and line-scoped hidden markup
- thin repository-specific wrappers for citations and proposed plans
- a composition layer for assistant output semantics
- a transport adapter for UTF-8 chunking

That separation keeps the implementation understandable, testable, and easy to integrate into higher-level streaming code in `core/`.
