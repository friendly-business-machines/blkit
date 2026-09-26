# Design

## Context

The MVP compiler proves that one `.bl` file can become Rust: parse its declarations and process bodies, validate names and types, then emit Rust from the validated program. See proposal.md for motivation.

## Goals / Non-Goals

**Goals:**
- Define a minimal `.bl` syntax for namespace/version declarations, records, enums, lists, and one or more process declarations.
- Build a compiler front end with parse errors and validation diagnostics.
- Enforce a closed-world type model: no Rust imports, Python imports, dynamic `Any`, null values, or external functions.
- Generate Rust only after successful validation.
- Leave runnable checks that compile or validate representative `.bl` examples.

**Non-Goals:**
- No distributed runtime, worker registry, messaging, persistence, REST API, cancellation, or client interaction.
- No embedded Python or custom Rust task ABI.
- No temporal types, ranges, nullable/optional values, first-class functions, loops, async waits, or user-defined helper functions.
- No table declarations, lookups, or query operations; table semantics are deferred.
- No directory or cross-file compilation in this MVP.

## Decisions

### Start with a compiler crate and tiny CLI
Create a Rust compiler crate plus a minimal CLI entry point. The CLI accepts one `.bl` file and emits generated Rust to a target path. Keeping the CLI thin avoids designing directory-wide linking and runtime APIs before the language is proven.

Alternatives considered:
- Runtime interpreter: rejected because the platform goal is Rust generation and compiled business logic.
- Full dev server first: rejected as premature before source validation and code generation exist.

### Use a minimal pipeline: parse AST -> validate -> Rust codegen
Keep parsing separate from semantic validation. Validate the full program before passing its AST to code generation; do not maintain a second typed IR for this MVP. Type safety comes from rejecting invalid source before generation, not from duplicating the representation.

Alternatives considered:
- Separate typed IR: useful when lowering becomes complex, but adds a second representation to maintain now.
- Generate Rust without validation: rejected because it would leave type errors to the Rust compiler.

### Model `Number` as one decimal-backed business numeric type
Expose only `Number` in `.bl`; do not add `Int`. Numeric literals become `Number`, avoiding coercion rules and matching business-rule expectations.

Alternatives considered:
- Separate `Int` and `Decimal`: rejected for MVP because it adds coercion and literal typing complexity without a current business need.
- Binary floating point: rejected for business logic because decimal comparisons must be predictable.

### Keep source closed-world
All names must resolve inside the single `.bl` file or the fixed built-in type set. This protects type safety and keeps Rust/Python interop out of the compiler MVP.

Alternatives considered:
- Import Rust symbols directly: rejected until the language core and task escape hatches are separately designed.

### Diagnostics should be useful before being fancy
Diagnostics must identify the failing construct and reason. Exact source spans are desirable if cheap through the parser choice, but not worth blocking the MVP.

Alternatives considered:
- Rich IDE-grade diagnostics: deferred until the grammar and type system stabilize.

## Risks / Trade-offs

- Parser/grammar churn -> Keep grammar intentionally small and covered by fixture-based checks.
- Decimal implementation choice leaks into generated Rust -> Wrap generated numeric representation behind a small generated/helper type alias where possible.
- Too much runtime sneaks into MVP -> Keep generated Rust focused on callable process functions, not orchestration.

## Migration Plan

No migration is required because there is no existing implementation. Rollback is deleting the new compiler scaffolding and planning artifacts before archive if the approach is abandoned.
