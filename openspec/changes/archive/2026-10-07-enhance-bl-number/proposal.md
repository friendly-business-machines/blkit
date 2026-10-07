# Proposal

## Why

`.bl` has a decimal-backed `Number` type, but numeric expressions are largely limited to literals, comparisons, and range tests. Business rules need arithmetic, predictable rounding and statistics, and locale-aware text conversion without leaving the typed expression language.

## What Changes

- Add signed and scientific-notation number literals; decimal arithmetic (`+`, `-`, unary `-`, `*`, `/`, `**`) with precedence and exact decimal behavior where representable. Preserve `==`/`!=` for equality (no `=` alias), other comparisons, `between`, and range membership.
- Add rounding, numeric math, predicates, clamping, and list-based numeric aggregate functions, including **sample** standard deviation.
- Extend point-versus-interval functions to accept `Number` points where meaningful while retaining range-versus-range behavior.
- Add `number(text[, groupingSeparator, decimalSeparator])` and normalized decimal text output; report invalid text, domains, precision/overflow, and undefined calculations as evaluation errors rather than silently substituting results.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `business-language`: Extend `Number` expression syntax, operations, builtins, conversion, and numeric point/range relation behavior.

## Impact

The existing expression lexer/parser (`src/expr.rs`), type inference (`src/semantic/types.rs`), Rust emitter (`src/codegen/mod.rs`), and generated call/error paths are affected. A shared numeric runtime helper may be needed for fallible decimal operations; existing `rust_decimal` should be reused. Add source-level and generated-Rust regression coverage in `tests/language.rs` and `tests/generated.rs`. The change does not add a new user-facing type or alter persisted process schemas.
