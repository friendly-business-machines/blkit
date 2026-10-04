# Design

## Context

See [proposal.md](proposal.md) for motivation and [specs/business-language/spec.md](specs/business-language/spec.md) for the contract. `src/expr.rs` lexes quoted text without escapes and currently recognizes comparison/Boolean operators only. `src/semantic.rs` validates `Expr` types (`in` currently requires a range); `src/codegen.rs` emits both plain-returning task Rust functions and `Result`-returning decision functions, plus fallible process graph closures. Generated project crates already depend on `blkit` via `src/project.rs`. Decision-table `column matches (...)` is a separate parser construct.

## Goals / Non-Goals

**Goals:** Reuse the existing expression pipeline and generated-crate dependency; give every supported expression context the same typed semantics and observable errors.

**Non-Goals:** General nullable values, generic list membership beyond `List<String>`, new escaping rules for `.bl` literals, a full numeric arithmetic language, arbitrary object/list-to-string serialization, or regex capture metadata beyond nested string lists.

## Decisions

1. **Extend the existing expression parser and type checker.** Tokenize `+` as a String-only infix operator with precedence above comparisons; permit signed numeric literals so e.g. `charAt(s, -1)` can be authored. Reuse the existing `Expr::Binary`, `Expr::Call`, and `Expr::List` AST. Type `in` as either existing range membership or `String in List<String>` depending on its right operand/context, without changing range behavior; type-check each builtin's exact arity, arguments, and output (including `List<List<String>>` for `extract`). Preserve `==`/`!=` and the decision-table unary-test `matches` branch. **Alternative:** a separate string expression evaluator would duplicate parsing and diverge across tasks/graphs/decisions.

2. **Use Rust/stdlib for simple operations and small shared `blkit` helpers for tricky ones.** Generate calls to a public helper module for grapheme indexing, splitting, regex, padding, and integer conversion, so standalone-generated and project-generated code need no direct extra dependencies. Add `unicode-segmentation` for extended grapheme clusters and `regex` for compiled patterns, Unicode-aware search/replace, and supported flags. `Number` is `rust_decimal::Decimal`; convert to bounded integers only where positions/counts require them, checking negative/fractional/overflow cases and allocation limits. Keep literal search/split distinct from regex and use native Unicode case/whitespace operations. **Alternative:** hand-written Unicode boundaries/regexes are error-prone; embedding code directly in generated output repeats it.

3. **Propagate fallible operations as `Result` rather than panic.** Have the expression emitter distinguish infallible and fallible calls so it can emit `?` in a `Result` context. For source-defined tasks containing fallible string expressions, generate `Result<Output, String>` and `Ok` on valid paths; preserve plain return types for unaffected tasks. Propagate errors through task graph wrappers, multi-instance nodes, gateway/loop expressions, and decision knowledge/context/table expressions (decisions already return `Result`). Inspect generated call sites instead of silently discarding errors or changing `.bl` output types. An invalid constant regex or flags literal is rejected by semantic validation using the same rules as runtime; a runtime-supplied invalid regex/flags, bad position or count becomes an execution error. **Alternative:** panic-catching or returning an innocuous value hides business-rule failures; uniformly changing every task signature causes avoidable churn.

4. **Keep predictable edge semantics.** Positions and lengths refer to extended grapheme clusters, with one-based positive indices, negative-from-end indices, and zero invalid for positional arguments. `indexOf` returns zero for not found; substring truncates at end but invalid start fails. `split` uses literal separators (list order breaks ties) and preserves empty fields; empty separators fail. `extract` returns outer lists per regex match and inner participating captures, or the full match when no capture groups exist. `replace` replaces all matches with Rust regex `$1`-style capture interpolation. A missing `substringBefore`/`substringAfter` match produces `""`; `isBlank` uses Unicode whitespace; default pad character is a space and an explicit pad must be one grapheme. Regex operations use the regex engine's character/byte match rules, while position-based functions count graphemes. **Alternative:** byte indexing would split visible characters and contradict the agreed UX.

## Risks / Trade-offs

- [Generated Rust callers of fallible tasks may break] → Only change those signatures, document `Result` handling, and compile generated code and graph call sites in tests.
- [Mixed contexts currently assume emitted expressions cannot fail] → Test task returns/branches, graph routing and loops, decision knowledge/context/table, and standalone/project builds with invalid runtime inputs.
- [Regex match boundaries can cut through a grapheme] → Specify grapheme indexing only for positional APIs; regex behavior follows regex matching, not user-visible positional units.
- [Large pad/repeat/count inputs could exhaust memory or overflow] → Check decimal conversion and sizes, fail cleanly, and test representative bounds without allocating huge strings.

## Migration Plan

Existing `.bl` expressions keep their meaning; users switching to string functions may need to handle `Result` in Rust code calling generated tasks directly. Build generated artifacts again after updating `blkit`. Reverting the source/dependency changes and rebuilding generated crates restores previous behavior; no persisted process/state schema changes are planned.
