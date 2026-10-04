# Proposal

## Why

Business authors can declare `String` fields but cannot yet compose, search, transform, or extract text inside `.bl` expressions. These operations should work in tasks, process expressions, and decision models without requiring external Rust tasks.

## What Changes

- Add string concatenation, string-list membership, scalar-to-string conversion, joining, and the requested typed string functions for searching, slicing, case/whitespace changes, splitting, extraction, padding, and repetition.
- Use one-based positions and visible Unicode characters (grapheme clusters); negative position arguments count from the right. Keep `==` and `!=` case-sensitive; do not introduce `=`.
- Add regex search, replacement, and grouped extraction with flags `i`, `m`, and `s`, with validation of constant patterns and errors for invalid dynamic patterns.
- **BREAKING (generated Rust API):** functions whose expressions can fail at runtime may return `Result<Output, String>` rather than plain `Output`; direct Rust callers of those generated functions must handle errors. Existing `.bl` source syntax and declared output types stay unchanged.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `business-language`: extend typed expressions with string operators/functions and error/Unicode semantics while preserving range membership and equality syntax.

## Impact

Expression lexing/parsing, semantic validation, Rust generation for tasks/graphs/decisions, runtime string helpers, generated-call boundaries, tests, and language documentation. Add regex and Unicode grapheme support to the `blkit` crate; generated crates reuse helpers through their existing `blkit` dependency.
