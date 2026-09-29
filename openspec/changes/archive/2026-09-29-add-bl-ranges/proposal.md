# Proposal

## Why

`.bl` can compare scalar values but cannot express bounded or open-ended numeric and temporal intervals or reuse them in decision-table rules. Supporting ranges and their relations makes eligibility and threshold rules concise without introducing a separate rules runtime.

## What Changes

- Add same-type ranges for `Number`, `Date`, `DateTime`, and `Time` with inclusive/exclusive endpoints and `null` as an unbounded-end marker, plus typed membership, `between`, `==`/`!=`, and the requested interval relations.
- Introduce `Date` and `Time` built-ins with ISO-format serialization and typed literals; retain the existing offset-aware `DateTime` semantics.
- Add comma-separated OR unary tests in decision-table rules only, including ranges and ordinary comparison tests, while keeping existing Boolean rule conditions valid.
- Reject malformed, incompatible, or statically inverted range expressions during compilation; treat dynamically inverted bounds as empty ranges. Generate deterministic Rust for accepted expressions and table rules.
- Preserve `==` as equality syntax; no general-purpose nullable type, FEEL runtime, or standalone unary-test lists in ordinary expressions.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `business-language`: Add `Date` and `Time` built-ins and same-type range expressions, membership, equality, and interval-relation semantics to `.bl` expressions.
- `decision-models`: Add range and comparison unary-test alternatives to decision-table rule conditions.

## Impact

Touches the expression lexer/parser, semantic validation and type inference, decision-table rule parser, Rust generation, compiler/generated-code tests, and `.bl` language documentation. No new dependencies or runtime services; existing decision-table Boolean rule syntax remains valid.
