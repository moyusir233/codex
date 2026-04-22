# codex-utils-template Code Analysis

## Scope

This document analyzes the crate at [/Users/bytedance/project/codex/codex-rs/utils/template](file:///Users/bytedance/project/codex/codex-rs/utils/template) based on:

- crate metadata in [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/utils/template/Cargo.toml)
- implementation and inline unit tests in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/template/src/lib.rs)
- usage guidance in [README.md](file:///Users/bytedance/project/codex/codex-rs/utils/template/README.md)
- representative workspace consumers that parse and render prompt/text templates

The crate is intentionally tiny, but it sits on a sensitive boundary: it validates and renders prompt-like text assets that other crates assume are correct. Its job is not “general templating”; its job is strict, predictable placeholder substitution with failure on ambiguity or mismatch.

## High-Level Responsibilities

`codex-utils-template` owns six concrete responsibilities:

1. Parse a small template language consisting of literal text plus `{{ placeholder }}` interpolation.
2. Support literal brace escaping via `{{{{` and `}}}}`.
3. Reject malformed templates early with structured parse errors and byte offsets.
4. Compile parsed templates into reusable segment lists so callers can render multiple times without reparsing.
5. Enforce strict rendering rules: no missing values, no duplicate variable names, and no extra unused variables.
6. Provide both a reusable parsed API (`Template`) and a one-shot convenience API (`render`).

This crate deliberately does not support loops, conditionals, filters, expression evaluation, or partial templates. That narrow scope is a design feature, not a missing implementation.

## Crate Layout

The crate is fully contained in four files:

- [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/utils/template/Cargo.toml)
  - declares package metadata
  - has no normal dependencies and one dev-dependency
- [src/lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/template/src/lib.rs)
  - contains all production code and all unit tests
- [README.md](file:///Users/bytedance/project/codex/codex-rs/utils/template/README.md)
  - documents the supported syntax and strictness guarantees
- [BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/utils/template/BUILD.bazel)
  - exposes the crate to the workspace Bazel build as `codex_utils_template`

There are no submodules. That is appropriate because the crate has one narrowly scoped concern and only a handful of internal helpers.

## Public API Surface

All public items are defined in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/template/src/lib.rs).

### `TemplateParseError`

Defined at [lib.rs:L13-L46](file:///Users/bytedance/project/codex/codex-rs/utils/template/src/lib.rs#L13-L46).

This enum models syntax-level template failures:

- `EmptyPlaceholder { start }`
  - `{{ }}` or whitespace-only placeholder content
- `NestedPlaceholder { start }`
  - `{{ outer {{ inner }} }}`-style nesting, which the grammar forbids
- `UnmatchedClosingDelimiter { start }`
  - a `}}` appearing outside a placeholder
- `UnterminatedPlaceholder { start }`
  - `{{ ...` without a closing `}}`

The `start` field is a byte offset into the original source string. This is simple and cheap to compute during parsing, and it makes error messages deterministic.

### `TemplateRenderError`

Defined at [lib.rs:L48-L71](file:///Users/bytedance/project/codex/codex-rs/utils/template/src/lib.rs#L48-L71).

This enum models render-time contract violations:

- `DuplicateValue { name }`
  - the caller passed the same variable key more than once
- `ExtraValue { name }`
  - the caller passed a variable not referenced by the template
- `MissingValue { name }`
  - the template requires a value that was not provided

The important design point is that rendering is intentionally exact, not permissive.

### `TemplateError`

Defined at [lib.rs:L73-L107](file:///Users/bytedance/project/codex/codex-rs/utils/template/src/lib.rs#L73-L107).

This is a convenience wrapper that unifies parse and render errors for the one-shot API:

```rust
pub enum TemplateError {
    Parse(TemplateParseError),
    Render(TemplateRenderError),
}
```

It implements `Error`, `Display`, and `From` conversions from both inner enums, which makes `?` ergonomic in the top-level `render` helper.

### `Template`

Defined at [lib.rs:L115-L210](file:///Users/bytedance/project/codex/codex-rs/utils/template/src/lib.rs#L115-L210).

This is the main reusable abstraction:

- `Template::parse(source: &str) -> Result<Self, TemplateParseError>`
  - parses source text into a compiled template
- `Template::placeholders(&self) -> impl ExactSizeIterator<Item = &str>`
  - returns a sorted, deduplicated iterator over placeholder names
- `Template::render<I, K, V>(&self, variables: I) -> Result<String, TemplateRenderError>`
  - renders the compiled template from an iterator of key/value pairs

Internally, `Template` stores:

- `placeholders: BTreeSet<String>`
  - used for deduplicated placeholder discovery and deterministic ordering
- `segments: Vec<Segment>`
  - the compiled sequence of literal and placeholder pieces used during rendering

The private `Segment` enum is defined at [lib.rs:L109-L113](file:///Users/bytedance/project/codex/codex-rs/utils/template/src/lib.rs#L109-L113):

```rust
enum Segment {
    Literal(String),
    Placeholder(String),
}
```

This is a straightforward and idiomatic representation for a small templating engine.

### One-shot `render`

Defined at [lib.rs:L212-L221](file:///Users/bytedance/project/codex/codex-rs/utils/template/src/lib.rs#L212-L221).

```rust
pub fn render<I, K, V>(template: &str, variables: I) -> Result<String, TemplateError>
```

This API parses and renders in one call:

1. `Template::parse(template)?`
2. `.render(variables)`
3. map render errors into `TemplateError`

This is the best entry point for infrequent rendering or tests, while `Template` is better when the same template body is reused many times.

## Internal Flow

The implementation is small enough that the full control flow is easy to reason about.

### 1. Parsing flow

`Template::parse` in [lib.rs:L121-L168](file:///Users/bytedance/project/codex/codex-rs/utils/template/src/lib.rs#L121-L168) performs a single left-to-right scan over the source string.

The parser maintains:

- `cursor`
  - the current byte position
- `literal_start`
  - the start of the currently accumulating literal region
- `segments`
  - the output program
- `placeholders`
  - the unique placeholder set

At each step it checks the current suffix in this order:

1. `{{{{`
   - flush prior literal
   - emit literal `{{`
   - advance by four bytes
2. `}}}}`
   - flush prior literal
   - emit literal `}}`
   - advance by four bytes
3. `{{`
   - flush prior literal
   - delegate to `parse_placeholder`
   - store placeholder in both `placeholders` and `segments`
4. `}}`
   - fail immediately with `UnmatchedClosingDelimiter`
5. otherwise
   - advance by one UTF-8 character

The final trailing literal is appended after the loop.

Two implementation details are worth calling out:

- UTF-8 correctness
  - the parser advances non-delimiter text using `chars().next()` plus `len_utf8()`, so it does not split multibyte characters while scanning
- delimiter precedence
  - escaped delimiters are recognized before normal delimiters, so `{{{{` always means a literal `{{`, not an empty placeholder followed by `{`

### 2. Placeholder parsing flow

`parse_placeholder` lives at [lib.rs:L235-L259](file:///Users/bytedance/project/codex/codex-rs/utils/template/src/lib.rs#L235-L259).

Its algorithm is:

1. start immediately after `{{`
2. scan forward until either `{{`, `}}`, or end of string
3. if a nested `{{` appears, return `NestedPlaceholder`
4. if `}}` appears:
   - trim surrounding whitespace
   - reject empty content
   - return the placeholder name and next cursor position
5. if the scan reaches the end, return `UnterminatedPlaceholder`

This means placeholder content is syntactically simple: it is any non-empty trimmed text that does not contain `{{`. The parser does not restrict names to identifier-like tokens, so names like `user name` or `a.b` are allowed if callers choose to use them.

### 3. Literal coalescing

`push_literal` in [lib.rs:L223-L233](file:///Users/bytedance/project/codex/codex-rs/utils/template/src/lib.rs#L223-L233) merges adjacent literal segments instead of emitting a separate `Segment::Literal` for each chunk.

That matters because escaped braces cause literal flushes mid-scan. Without coalescing, templates with many escapes would produce unnecessarily fragmented segment lists.

### 4. Render flow

`Template::render` in [lib.rs:L174-L209](file:///Users/bytedance/project/codex/codex-rs/utils/template/src/lib.rs#L174-L209) executes in three phases:

1. Normalize inputs with `build_variable_map`
2. Validate exact name matching between template placeholders and provided variables
3. Walk `segments` and build the output string

The validation order is deliberate:

- duplicate values fail first during map construction
- missing placeholder values are checked before extra values
- actual segment rendering performs a defensive second lookup check

The final rendering pass is simple:

```rust
for segment in &self.segments {
    match segment {
        Segment::Literal(literal) => rendered.push_str(literal),
        Segment::Placeholder(name) => rendered.push_str(value_for(name)?),
    }
}
```

There is no lazy substitution and no deferred error handling. If rendering succeeds, the output is complete and fully materialized as a `String`.

### 5. Variable normalization

`build_variable_map` is implemented at [lib.rs:L261-L280](file:///Users/bytedance/project/codex/codex-rs/utils/template/src/lib.rs#L261-L280).

It accepts any `IntoIterator<Item = (K, V)>` where both sides implement `AsRef<str>`. That makes the API flexible enough to accept:

- arrays of `(&str, &str)`
- vectors of `(String, String)`
- mixed borrowed/owned string-like inputs

The function clones keys and values into a `BTreeMap<String, String>`. This provides:

- deterministic key ordering
- duplicate detection via `insert(...).is_some()`
- uniform owned storage for later lookup

Because the crate is used primarily for prompt and text assets, the simplicity of ownership here is more important than micro-optimizing allocations.

## Representative Workspace Usage

Even though this crate is generic, its current workspace role is very concrete: it is the standard “strict prompt/text template” utility.

### Prompt templates in `models-manager`

[collaboration_mode_presets.rs:L7-L16](file:///Users/bytedance/project/codex/codex-rs/models-manager/src/collaboration_mode_presets.rs#L7-L16) parses an embedded collaboration-mode template once into a `LazyLock<Template>`, and [collaboration_mode_presets.rs:L64-L76](file:///Users/bytedance/project/codex/codex-rs/models-manager/src/collaboration_mode_presets.rs#L64-L76) renders it with exactly three variables.

This is a textbook fit for the crate:

- parse once at process lifetime scope
- treat invalid template syntax as a programmer error and panic early
- rely on strict rendering to catch drift between template placeholders and code-provided variables

### HTML template in `login`

[server.rs:L53-L56](file:///Users/bytedance/project/codex/codex-rs/login/src/server.rs#L53-L56) parses an embedded `error.html` page template once for the login callback server.

This shows the crate is not limited to markdown prompts; it is also used for small HTML assets where accidental missing or extra variables would be undesirable.

### Review prompt generation in `core`

[review_prompts.rs:L19-L37](file:///Users/bytedance/project/codex/codex-rs/core/src/review_prompts.rs#L19-L37) defines several static review prompt templates, and [review_prompts.rs:L98-L105](file:///Users/bytedance/project/codex/codex-rs/core/src/review_prompts.rs#L98-L105) renders them with fixed variable sets.

This is a strong architectural signal: upstream crates trust `codex-utils-template` to guard prompt correctness in product-critical flows.

### Memory prompt assembly in `core`

[prompts.rs:L21-L57](file:///Users/bytedance/project/codex/codex-rs/core/src/memories/prompts.rs#L21-L57) wraps `Template::parse` in a helper for embedded markdown templates, and [prompts.rs:L113-L131](file:///Users/bytedance/project/codex/codex-rs/core/src/memories/prompts.rs#L113-L131) uses rendering failures as warning-level fallbacks in a more fault-tolerant path.

That usage is especially informative because it demonstrates two intended integration patterns:

- fail-fast at startup for “must be valid” static templates
- degrade gracefully when a larger feature can fall back to a simpler prompt

### Other consumers

The crate is also referenced in:

- [protocol/src/models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs)
- [core/src/tasks/review.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/tasks/review.rs)

And it is declared as a dependency in:

- [protocol/Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/protocol/Cargo.toml#L23)
- [models-manager/Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/models-manager/Cargo.toml#L29)
- [login/Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/login/Cargo.toml#L23)
- [core/Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/core/Cargo.toml#L76)

That dependency spread confirms this crate is the workspace-standard templating primitive.

## Dependencies

### Direct Cargo dependencies

From [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/utils/template/Cargo.toml):

- normal dependencies
  - none
- dev-dependencies
  - `pretty_assertions` from the workspace, used only in tests

This is an unusually clean dependency profile. The entire production crate relies only on the Rust standard library.

### Standard library building blocks

The key production dependencies are standard-library types:

- `BTreeSet`
  - stores unique placeholder names in sorted order
- `BTreeMap`
  - stores normalized variable bindings in sorted order
- `std::error::Error`
  - provides standard error trait integration
- `fmt`
  - supports human-readable error messages

The absence of `regex`, parser combinators, or third-party templating crates is intentional. The implementation is small enough that a handwritten parser is simpler and easier to audit.

### Bazel integration

[BUILD.bazel:L1-L6](file:///Users/bytedance/project/codex/codex-rs/utils/template/BUILD.bazel#L1-L6) registers the crate as:

```python
codex_rust_crate(
    name = "template",
    crate_name = "codex_utils_template",
)
```

That keeps the crate available to both Cargo and Bazel-based builds with minimal duplication.

## Testing and Verification

All test coverage lives inline in [lib.rs:L282-L442](file:///Users/bytedance/project/codex/codex-rs/utils/template/src/lib.rs#L282-L442).

### Unit test coverage

The 14 unit tests exercise the full public contract:

- successful rendering
  - whitespace-trimmed placeholders
  - repeated placeholder reuse
  - multiline templates
  - adjacent placeholders
  - literal brace escapes
- parse failures
  - empty placeholder
  - unterminated placeholder
  - nested placeholder
  - unmatched closing delimiter
- render failures
  - missing value
  - extra value
  - duplicate value
- wrapper behavior
  - one-shot `render` returns `TemplateError::Parse`
  - one-shot `render` returns `TemplateError::Render`
- metadata behavior
  - placeholder discovery is sorted and unique

Representative examples:

- [render_replaces_placeholders_with_and_without_whitespace](file:///Users/bytedance/project/codex/codex-rs/utils/template/src/lib.rs#L291-L303)
- [render_supports_literal_delimiter_escapes](file:///Users/bytedance/project/codex/codex-rs/utils/template/src/lib.rs#L337-L349)
- [parse_errors_when_placeholder_is_nested](file:///Users/bytedance/project/codex/codex-rs/utils/template/src/lib.rs#L368-L373)
- [render_errors_when_duplicate_value_is_provided](file:///Users/bytedance/project/codex/codex-rs/utils/template/src/lib.rs#L409-L419)

### Test execution

I ran:

```bash
cargo test -p codex-utils-template
```

Result:

- 14 unit tests passed
- 0 doc tests failed

The crate is currently green in isolation.

### Indirect consumer validation

Some higher-level correctness is also exercised indirectly by downstream crates:

- [collaboration_mode_presets_tests.rs](file:///Users/bytedance/project/codex/codex-rs/models-manager/src/collaboration_mode_presets_tests.rs)
  - verifies collaboration-mode placeholders are actually replaced
- [review_prompts.rs tests](file:///Users/bytedance/project/codex/codex-rs/core/src/review_prompts.rs#L132-L180)
  - verifies rendered review prompts are stable

That downstream coverage matters because the real contract of this crate is not just syntax parsing; it is protecting prompt-producing flows from drift.

## Design Notes

### Strengths

- Minimal surface area
  - one type, one convenience function, three small error enums
- Strong correctness guarantees
  - exact-key rendering prevents silent template drift
- Deterministic behavior
  - `BTreeSet` and `BTreeMap` make placeholder discovery and validation ordering stable
- Easy auditability
  - the parser is short, explicit, and free of macro magic
- Reusable compiled form
  - callers can parse once into `LazyLock<Template>` and render repeatedly
- Standard error integration
  - all error types implement `Display` and `Error`

### Important trade-offs

- Strictness over convenience
  - extra values are an error, unlike many templating systems that silently ignore them
- Allocation simplicity over zero-copy design
  - parsing and rendering store owned `String`s rather than borrowing from the source
- Byte offsets over line/column diagnostics
  - simple to compute, but less friendly for humans reading long multiline templates
- Tiny grammar over extensibility
  - no optional placeholders, defaults, loops, or escaping beyond doubled delimiters

### Why the current design fits this workspace

The workspace uses this crate mainly for embedded prompts and small text assets. In that context:

- template bodies are usually static and developer-authored
- rendering variables are usually generated by code, not end-user input
- silent omission of missing data would be dangerous
- over-featured templating would increase attack surface and debugging cost

So the crate’s “strict and boring” design is a good fit.

## Risks and Edge Cases

These are not necessarily bugs, but they are behaviorally important:

1. Placeholder names are arbitrary trimmed strings
   - `{{ user name }}` and `{{a.b}}` are valid names
   - that flexibility is fine, but it means callers must keep naming conventions disciplined themselves

2. Error positions are byte offsets, not character indices
   - this is technically correct for Rust strings
   - it may be less intuitive when templates contain non-ASCII content

3. Validation reports only the first problem
   - rendering returns the first missing, extra, or duplicate name found
   - callers do not get an aggregated list of all mismatches

4. Escaping supports only literal delimiters
   - there is no backslash escape syntax and no way to escape inside placeholder content

5. Rendering always clones inputs into a map
   - efficient enough for current prompt-sized workloads
   - less ideal for very large templates or repeated high-throughput rendering

## Open Questions and Potential Improvements

1. Should diagnostics include line/column information?
   - Current byte offsets are cheap and precise, but line/column may be easier when debugging large markdown or HTML templates.

2. Should placeholder names be more constrained?
   - A stricter grammar such as `[A-Za-z0-9_]+` could catch accidental whitespace or punctuation mistakes earlier, but it would also reduce flexibility.

3. Should render validation aggregate all problems?
   - Returning only the first `MissingValue` or `ExtraValue` is simple, but bulk diagnostics could improve developer ergonomics for larger templates.

4. Should there be a borrowed or prevalidated render path?
   - For hot paths, callers might benefit from a lower-allocation API that accepts a `&BTreeMap<_, _>` or similar, though current usage does not appear performance-sensitive.

5. Should the crate expose a parse-time placeholder list as a concrete collection?
   - `placeholders()` is an iterator, which is ergonomic, but some consumers may want an owned or slice-like view for inspection or schema generation.

6. Should README examples become doc comments?
   - The crate currently has a README example, but no rustdoc example on the public items themselves. Moving or duplicating that example into docs would improve API discoverability.

## Bottom Line

`codex-utils-template` is a compact, well-scoped utility crate that provides strict string templating for workspace prompt and text assets. Its main value is not feature richness; it is its predictability:

- parse embedded templates once
- fail fast on malformed syntax
- render only when provided variables match exactly
- keep the implementation easy to audit

For the Codex workspace, that is the right trade-off. The crate protects several prompt- and HTML-producing flows from silent drift while staying small enough that its correctness story is easy to understand from a single source file.
