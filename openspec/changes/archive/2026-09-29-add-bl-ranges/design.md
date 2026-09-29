# Design

## Context

See proposal.md. `src/expr.rs` parses lists, grouped expressions and function calls; `src/semantic.rs` infers scalar types and resolves knowledge-model calls; `src/codegen.rs` emits both standalone Rust functions and graph closures. `src/decision.rs` parses decision-table rules as ordinary Boolean expressions. `chrono` already supplies offset-aware `DateTime`; `Date` and `Time` do not yet exist. The behavioral contract is in `specs/business-language/spec.md` and `specs/decision-models/spec.md`.

## Goals / Non-Goals

**Goals:** Reuse the existing expression/type/codegen pipeline for ranges in tasks, decisions and graph conditions; keep existing Boolean table rules and hit policies unchanged.

**Non-Goals:** A publicly declared/serialized `Range<T>` type, nullable values outside range bounds, a FEEL interpreter, or timezone conversion for `Time`.

## Decisions

1. **Represent ranges as typed expression values, not public source types.** Extend the parser with a range expression containing optional bound expressions and endpoint flags. Detect `..` before decimal or field-access lexing; distinguish a range beginning with `(` from grouping via the range delimiter and matching closure. Use an internal range type of `Number`, `Date`, `DateTime` or `Time`. Validate both finite endpoints against that type, inferring a doubly-unbounded range only from its use; reject ambiguous, mixed-type and statically reversed bounds. This avoids introducing JSON serialization or changing process signatures. Alternative: add `Range<T>` to the public type system, which would require an input/output format unrelated to the examples.

2. **Use existing `chrono` and typed literal constructors.** Extend scalar type validation, generated aliases and serialization for `Date` (`NaiveDate`) and `Time` (`NaiveTime`); reuse existing `DateTime<FixedOffset>`. Validate `date(...)`, `time(...)` and `dateTime(...)` string-literal constructors at compile time. A plain string never implicitly becomes a temporal value. DateTime ordering compares instants, while Date/Time ordering uses calendar/wall-clock values. Alternative: parse strings opportunistically, which would make type errors and ambiguous timezones hard to diagnose.

3. **Share range semantics across expression contexts.** Lower range operations to one small generated-Rust helper over ordered same-type endpoints (optional bound + inclusion flags), rather than duplicating comparison logic across functions and table rules. Ignore inclusion on missing bounds for equality. Model empty equal/open or dynamically reversed ranges explicitly; membership and intersection check for an actual value (including the `Date` calendar-day granularity), rather than assuming every ordered pair contains an intermediate value. Implement converse relations by reversing arguments; `between` lowers to inclusive membership. The helper uses no new dependency. Alternative: generate expanded comparisons at each use site, which duplicates error-prone endpoint rules.

4. **Restrict comma-separated unary tests to table rule conditions.** Accept `rule amount matches (< 10, [20..30]) -> result` via a table-condition parsing path; each list item becomes an ordinary typed Boolean comparison of the named input column with its scalar/range operand. OR these alternatives before composing with any surrounding Boolean condition. Preserve `rule amount > 100 -> result` and the existing rule ordering/hit-policy evaluator. Parse the `matches (...)` list with balanced delimiters so commas inside calls cannot be mistaken for test separators or output columns. Ordinary expression parsing continues to reject bare unary-test lists. Alternative: introduce general-purpose unary-test values, unnecessary outside tables.

## Risks / Trade-offs

- **Parentheses, dots and commas are already significant tokens** → Lexer/parser tests distinguish decimals, field access, list literals, grouped conditions, range bounds and table outputs before adding generation.
- **Ranges of `Date` are discrete, while `Number`/`DateTime` are ordered differently** → Test intersection at adjacent/open calendar dates and equality across DateTime offsets, not only simple numeric intervals.
- **Invalid dynamic bound order in non-fallible generated task functions** → Treat a dynamic inversion as an empty range; compile-time constant inversions remain diagnostics. No public function return-type change.
- **New scalar types affect generated JSON input and output** → Test typed record/process/decision round trips and rejection of malformed dates, leap seconds, offset-bearing times and offset-free datetimes.

## Migration Plan

No source migration for existing `.bl` files: the original Boolean decision-table rules, numeric literals, lists and `==` syntax remain valid. Generated binaries must be rebuilt for new `.bl` sources; reverting this change requires removing the new syntax from those sources before rebuilding.
