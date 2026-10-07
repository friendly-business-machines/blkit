# Design

## Context

See [proposal.md](proposal.md) and [business-language delta](specs/business-language/spec.md). `Number` is already `rust_decimal::Decimal` throughout compiler and generated Rust. `src/expr.rs` currently lexes decimal literals and signed literals but has no arithmetic precedence; `src/semantic/types.rs` type-checks `String + String` and range functions; `src/codegen/mod.rs` emits `+` as string formatting. Generated decision code and graph closures already support fallible expressions. `src/project.rs` supplies generated crates with `blkit` and `rust_decimal`.

## Goals / Non-Goals

**Goals:** Share one typed numeric expression path across decision tasks, tables, and process graphs, preserving exact decimal arithmetic when representable and propagating errors when not. Reuse the existing `Decimal` and generated `blkit` dependency.

**Non-Goals:** A new numeric type, binary float semantics, general-purpose locale inference, changing `=` into an equality operator, or changing existing decision-table aggregation policies.

## Decisions

1. **Extend the existing AST and precedence parser.** Lex sign as a unary operator in expression context (rather than folding it indiscriminately into digits), distinguish subtraction from negative/exponent-signed literals, and parse `**` right-associatively above unary negation. Reuse `Expr::Binary` and add only the unary form needed for negation; retain existing `String + String`, range, and `between` parsing. Let inference choose numeric addition versus concatenation so emission does not convert numbers to strings accidentally. Reject mixed types and `=` before generation. **Alternative:** A second numeric parser would split the meaning of expressions between graphs and decisions.

2. **Keep decimal operations and tricky builtins in a small shared runtime helper.** Use `rust_decimal` checked operations and rounding strategies for arithmetic, scales, parsing, and aggregates. Use its math support where appropriate, with explicit domain/range checks and exact-case handling for representable results such as integer powers and perfect square roots; transcendental functions produce decimal approximations, not binary-float results exposed as exact. Keep `string(Number)` normalization in the existing conversion path. `sum`/`product` over empty lists need typed empty-list inference. Implement sample `stddev` and deterministic numeric minimum tie-breaking for `mode`. Only introduce additional math support if the installed decimal crate cannot meet a spec scenario; avoid a second numeric representation. **Alternative:** Generating raw Rust operators and `.unwrap()` would panic or overflow on runtime input.

3. **Treat numeric failures like existing fallible string calls.** Extend expression fallibility detection to checked arithmetic, numeric builtins, and `number(...)`; generate `?` through task, knowledge, table, and graph call paths, retaining normal `.bl` port types and unaffected plain Rust signatures. Check literal validity and constant `number(...)` input in semantic validation; dynamic errors propagate as `Result`. Evaluate numeric addition as numeric and String addition as concatenation without eager evaluation or changed ordering of other operators. **Alternative:** A universal `Result` signature for every generated function needlessly breaks existing Rust callers.

4. **Overload only well-defined numeric point/range relations.** Keep existing `BlRange` representation and range/range behavior; type-check point/range `before`, `after`, `meets`, and `metBy` as additional overloads, alongside the existing point predicates. Generate direct comparisons against finite bounds, respecting inclusive endpoints only for membership/starts/finishes. Unbounded required endpoints return false. Do not reinterpret `overlaps`/`coincides` with scalar arguments. **Alternative:** Turning a point into an inclusive singleton range changes interval overlap semantics and complicates empty/open endpoints.

## Risks / Trade-offs

- [Unary minus and scientific notation can collide with `..`, field access, or subtraction] → Parser tests for whitespace-free and spaced forms, negative bounds, nested expressions, and operator precedence.
- [Decimal precision and exact-case math vary by crate method] → Specify stable observable examples, cover checked overflow/domain paths, and verify approximate examples with tolerance rather than exact text.
- [Existing `+` is String-only and generic generated code does not carry resolved types] → Route emission through validated expression types or annotate inference as needed; compile generated String and Number cases in the same suite.
- [Fallibility crosses standalone/project Rust APIs and decision-table aggregation] → Compile generated programs and exercise failures through both direct tasks and process/decision execution; preserve old call signatures when unaffected.
- [Round/aggregate behavior at extremes can silently truncate or allocate heavily] → Bound scale, list arithmetic, and conversion by decimal representability; fail cleanly instead of panicking.

## Migration Plan

Existing valid expressions, `==`/`!=`, and range behavior remain valid. Newly fallible generated Rust functions may return `Result`, as with existing fallible String expressions; rebuild generated crates and update direct Rust callers when their expressions become fallible. No persisted state migration is needed. Roll back by rebuilding from the previous compiler and `blkit` version.
