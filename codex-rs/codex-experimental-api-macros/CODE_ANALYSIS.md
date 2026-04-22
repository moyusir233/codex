# codex-experimental-api-macros crate analysis

## Scope

This document analyzes the crate at `/Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros`.

This crate is intentionally tiny, but it sits on an important compatibility boundary for the workspace. It owns a single derive macro, `ExperimentalApi`, that turns plain Rust data types into two things:

1. runtime experimental-gating checks via an `ExperimentalApi` trait impl, and
2. static field registrations used by schema/export filtering.

The crate itself does not define the trait or registry types. Instead, it generates code against contracts provided by the consuming crate, currently `codex-app-server-protocol`.

## High-level responsibility

The crate has one core job:

- expose `#[derive(ExperimentalApi)]` for structs and enums, declared in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/src/lib.rs#L16-L28)

That single macro expands into several concrete behaviors:

- inspect `#[experimental("reason")]` field or variant attributes
- inspect `#[experimental(nested)]` field attributes
- generate `crate::experimental_api::ExperimentalApi` impls
- generate `::inventory::submit!` registrations for experimental struct fields
- generate a per-type `EXPERIMENTAL_FIELDS` constant for struct types
- reject unions with a compile error

In practice, this makes the macro crate the glue between authoring-time annotations on protocol types and downstream runtime/export behavior in `codex-app-server-protocol`.

## File and build layout

- [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/Cargo.toml#L1-L16)
  - declares a `proc-macro` library
  - depends only on `proc-macro2`, `quote`, and `syn`
- [src/lib.rs](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/src/lib.rs)
  - contains the entire implementation
- [BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/BUILD.bazel#L1-L7)
  - mirrors the Cargo role as a Rust proc-macro target

There are no crate-local integration tests, `tests/` directories, or `#[cfg(test)]` modules inside this crate.

## Public API surface

The public surface is deliberately minimal:

- `#[derive(ExperimentalApi)]` from [derive_experimental_api](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/src/lib.rs#L16-L28)
- helper attribute `#[experimental("...")]`
- helper attribute `#[experimental(nested)]`

The derive is only valid on:

- structs, handled by [derive_for_struct](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/src/lib.rs#L30-L158)
- enums, handled by [derive_for_enum](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/src/lib.rs#L160-L193)

It explicitly rejects unions in [derive_experimental_api](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/src/lib.rs#L19-L27).

## Consuming-crate contract

The macro is small because it assumes the consumer provides a fixed integration surface.

Generated code refers to:

- `crate::experimental_api::ExperimentalApi`
- `crate::experimental_api::ExperimentalField`
- `::inventory::submit!`

Those contracts are supplied by [experimental_api.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/experimental_api.rs#L4-L32) in `codex-app-server-protocol`, which defines the trait, the `ExperimentalField` registry entry type, and the `inventory::collect!` registry.

This is an important design choice: the macro crate is not a standalone experimental-gating framework. It is a code generator specialized to the protocol crate’s naming and registry conventions.

## How the derive works

### 1. Parse and dispatch

The entrypoint [derive_experimental_api](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/src/lib.rs#L16-L28) parses the incoming token stream as `syn::DeriveInput` and dispatches on `syn::Data`:

- `Data::Struct` -> [derive_for_struct](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/src/lib.rs#L30-L158)
- `Data::Enum` -> [derive_for_enum](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/src/lib.rs#L160-L193)
- `Data::Union` -> compile error

### 2. Attribute decoding

Attribute parsing is split into two helpers:

- [experimental_reason / experimental_reason_attr](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/src/lib.rs#L195-L205)
  - recognizes `#[experimental("reason")]`
- [has_nested_experimental / experimental_nested_attr](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/src/lib.rs#L207-L218)
  - recognizes `#[experimental(nested)]`

The two forms are mutually distinguished by parsing the attribute payload either as a `LitStr` or as an `Ident`.

### 3. Struct expansion

For structs, [derive_for_struct](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/src/lib.rs#L30-L158) walks every field and builds three token lists:

- `checks`
  - runtime logic for `ExperimentalApi::experimental_reason(&self)`
- `experimental_fields`
  - `ExperimentalField` values used in the generated constant
- `registrations`
  - `inventory::submit!` calls so the consuming crate can discover fields globally

For a field with `#[experimental("reason")]`, the macro generates:

- a presence test
- an early return of `Some(reason)` if the field is considered “in use”
- an `ExperimentalField { type_name, field_name, reason }` registration

For a field with `#[experimental(nested)]`, the macro generates a recursive call to `ExperimentalApi::experimental_reason` on that field value instead of assigning its own reason.

The final expansion for structs includes:

- zero or more `inventory::submit!` registrations
- an inherent associated constant
- an impl of `crate::experimental_api::ExperimentalApi`

Relevant code lives in [derive_for_struct](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/src/lib.rs#L143-L156):

```rust
impl #name {
    pub(crate) const EXPERIMENTAL_FIELDS: &'static [crate::experimental_api::ExperimentalField] =
        #experimental_fields;
}

impl crate::experimental_api::ExperimentalApi for #name {
    fn experimental_reason(&self) -> Option<&'static str> {
        #checks
    }
}
```

### 4. Enum expansion

Enums are simpler. [derive_for_enum](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/src/lib.rs#L160-L193) generates a `match self` where each variant returns:

- `Some(reason)` if the variant has `#[experimental("reason")]`
- `None` otherwise

This derive path does not:

- inspect fields inside enum variants
- support `#[experimental(nested)]` on enum fields
- register variant metadata in `inventory`

So enum support is purely runtime gating, not schema-field registration.

### 5. Presence heuristics

The most subtle logic is how the macro decides whether a field is “present enough” to count as experimental.

That logic lives in:

- [experimental_presence_expr](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/src/lib.rs#L244-L253)
- [index_presence_expr](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/src/lib.rs#L255-L258)
- [presence_expr_for_access](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/src/lib.rs#L260-L277)
- [presence_expr_for_ref](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/src/lib.rs#L279-L293)
- [option_inner / is_vec_like / is_map_like / is_bool / type_last_ident](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/src/lib.rs#L295-L329)

The rules are:

- `Option<T>` counts only when `Some(...)` is present and the inner value is itself present
- `Vec<T>` counts when non-empty
- `HashMap<K, V>` and `BTreeMap<K, V>` count when non-empty
- `bool` counts only when `true`
- every other type counts as present unconditionally

This gives the derive a pragmatic “first meaningful usage” behavior rather than a pure syntactic “field exists on the type” behavior.

## Field-name registration behavior

Schema filtering needs the serialized property name, so the macro computes field names through:

- [field_serialized_name](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/src/lib.rs#L220-L224)
- [snake_to_camel](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/src/lib.rs#L226-L242)

For named struct fields, the macro:

- takes the Rust identifier, such as `mock_experimental_field`
- converts it to camelCase, such as `mockExperimentalField`
- stores that name in `ExperimentalField.field_name`

Tuple-struct fields use numeric indexes as strings, such as `"0"` or `"1"`.

This registered metadata is later consumed by schema/export filtering in [export.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/export.rs#L243-L453).

## End-to-end flow in the workspace

The macro crate is easiest to understand in the full workspace flow:

### 1. Protocol types opt into derive

`codex-app-server-protocol` derives the macro on types such as:

- [ProfileV2](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L661-L683)
- [Config](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L762-L800)
- [ConfigReadResponse / ConfigRequirements / ConfigRequirementsReadResponse](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L882-L965)
- [ThreadStartParams / ThreadStartResponse / ThreadResumeParams / ThreadResumeResponse / ThreadForkParams / ThreadForkResponse](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L2876-L3140)

### 2. Runtime gating uses the generated trait impls

The consumer trait is defined in [experimental_api.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/experimental_api.rs#L4-L32), and request-level logic in [common.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/common.rs#L35-L69) and [common.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/common.rs#L158-L189) consults `ExperimentalApi::experimental_reason(...)` to determine whether a request or notification requires the experimental capability.

### 3. Schema export uses the generated field registrations

The inventory registry returned by [experimental_fields](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/experimental_api.rs#L22-L27) is consumed by:

- TypeScript filtering in [export.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/export.rs#L243-L395)
- JSON schema filtering in [export.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/export.rs#L397-L453)

That means this small macro crate directly influences:

- runtime request validation and capability gating
- generated TypeScript type shape
- generated JSON schema shape

## Dependencies

The direct dependencies in [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/Cargo.toml#L10-L13) are standard proc-macro tooling:

- `proc-macro2`
  - stable token representation and `Span`
- `quote`
  - code generation
- `syn` with `full` and `extra-traits`
  - AST parsing for `DeriveInput`, fields, attributes, and types

Notably absent:

- no dependency on `inventory`
- no dependency on the protocol crate
- no dependency on `serde`

That keeps the proc-macro crate light, but the generated code still requires the consumer to have `inventory` and the expected `crate::experimental_api` module available.

## Testing story

This crate has no local tests, so its behavior is validated indirectly in the consumer crate.

The most important downstream coverage is:

- [experimental_api.rs tests](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/experimental_api.rs#L58-L172)
  - enum variant support
  - nested option support
  - nested vector support
  - nested map support
- [v2.rs tests](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L7929-L8259)
  - real protocol structs and request types using the derive
  - nested experimental propagation through request params
  - concrete reasons such as `askForApproval.granular` and `config/read.approvalsReviewer`
- [export.rs tests](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/export.rs#L2628-L2722)
  - schema and TypeScript filtering behavior based on `ExperimentalField`

This is good end-to-end coverage, but it also means regressions in the macro crate are only caught through another crate’s test suite.

## Design observations

### Strengths

- focused scope
  - the crate does one job and keeps its code size small
- practical expansion model
  - one derive drives both runtime gating and export-time field filtering
- low compile-time dependency surface
  - no heavy internal framework beyond `syn`/`quote`
- useful nested propagation
  - `#[experimental(nested)]` composes well with consumer-side `ExperimentalApi` impls for `Option`, `Vec`, `HashMap`, and `BTreeMap` in [experimental_api.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/experimental_api.rs#L34-L55)
- explicit unsupported case
  - unions fail loudly instead of silently producing nonsense

### Trade-offs

- consumer coupling
  - generated code is hardcoded to `crate::experimental_api`, so the derive is not reusable without that exact module shape
- heuristic presence model
  - `bool` uses `true` as presence, collections use non-empty, and other types always count as present; this is useful but opinionated
- asymmetric type support
  - structs get field registration and nested traversal, enums only get variant-level reason matching
- test indirection
  - the macro crate cannot be validated in isolation today

## Open questions

1. Does field-name registration correctly handle non-camelCase serde output?
   - The macro always converts named fields with [snake_to_camel](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/src/lib.rs#L226-L242).
   - But derived types such as [ProfileV2](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L661-L683) and [Config](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L762-L800) use `#[serde(rename_all = "snake_case")]`.
   - That suggests registered field names like `approvalsReviewer` may not match generated schema/TS property names like `approvals_reviewer`.
   - The current export tests in [export.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/export.rs#L2628-L2722) only exercise camelCase-style field names.

2. Should malformed `#[experimental(...)]` attributes produce hard errors instead of being ignored?
   - [experimental_reason_attr](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/src/lib.rs#L199-L205) treats parse failures as `None`.
   - [experimental_nested_attr](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/src/lib.rs#L211-L218) also quietly returns `false` on parse failure.
   - That means typos such as a non-string reason or misspelled nested marker are easy to miss.

3. Is the generated `EXPERIMENTAL_FIELDS` constant still needed?
   - The constant is generated in [derive_for_struct](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/src/lib.rs#L146-L149).
   - A workspace search only finds the generation site, not any reads.
   - If it is dead output, removing it would reduce expansion size and simplify the generated API.

4. Should enums support nested-field inspection?
   - [derive_for_enum](file:///Users/bytedance/project/codex/codex-rs/codex-experimental-api-macros/src/lib.rs#L160-L193) only looks at variant attributes.
   - Stable variants that carry nested experimental payloads currently do not appear to participate in recursive detection.
   - That may be intentional, but it is a meaningful semantic limitation compared with struct support.

5. Should this crate gain direct trybuild-style tests?
   - Right now the main confidence comes from downstream tests in `codex-app-server-protocol`.
   - Local compile-pass and compile-fail tests would make the macro contract clearer and would catch parsing/expansion regressions closer to the source.

## Bottom line

`codex-experimental-api-macros` is a small but high-leverage proc-macro crate. It converts lightweight annotations on protocol types into the workspace’s experimental API control plane:

- runtime `experimental_reason()` checks
- global experimental field registration
- downstream schema and TypeScript pruning

Its implementation is straightforward and readable, especially for a proc-macro crate. The main architectural caveat is that it is not generic infrastructure; it is tightly coupled to `codex-app-server-protocol` conventions, and some of its most important contracts, especially field-name mapping and attribute validation, are implicit rather than type-checked.
