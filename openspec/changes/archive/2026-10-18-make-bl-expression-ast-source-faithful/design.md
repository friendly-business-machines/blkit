# Design

## Context

See proposal.md for motivation. `expr::Expr` is already a structured public tree, but `expression_parser` constructs private-call encodings for dictionary literals (`__bl_dictionary`), bracket indexing (`__bl_index`), `some` (`__bl_some`), named call arguments (`__bl_named_*`), and `start:end` (`__bl_sequence`). `semantic/types.rs` and `semantic/constraints.rs` recognize these encodings; `decision.rs` traverses dictionary entries in pairs; `codegen/mod.rs` specializes or emits them. `parse` and `expr::expression` expose the tree publicly. Tests compile generated Rust through Cargo; source diagnostics must still identify the original file.

## Goals / Non-Goals

**Goals:** Distinguish authored calls from the five parser-encoded source constructs; preserve their child order and existing validation, typing, error propagation, and execution. Maintain `parse` and `validate` as separate callable steps.

**Non-Goals:** A lossless CST (comments, formatting, or all punctuation), a new whole-program intermediate representation, new source-span guarantees, a changed `.bl` grammar, a Rust AST, or rewrites of type-directed codegen optimizations and fixed Rust project templates. Other existing source-sugar normalization (e.g. `between` into range membership) need not change in this focused refactor.

## Decisions

1. **Use explicit `Expr` variants for each parser-encoded construct, not magic calls.** Store dictionary entries as ordered `(String, Expr)` pairs, index as base/key expressions, `some` as its binding/predicate structure, a sequence as its endpoints, and named arguments as structured call arguments (rather than calls named `__bl_named_*`). Preserve current name/duplicate/argument-order checks in the parser and current dictionary-dependent entry scope. An authored `Expr::Call` must continue to mean an authored function call. Alternative: retain parser encodings and add helper accessors; that would disguise rather than fix the AST contract.

2. **Update source-aware consumers instead of lowering the entire program back to the old representation.** Teach semantic type/constraint traversal, decision dependency analysis, graph reference rewriting, and codegen expression specialization/emission to handle the new variants. Reuse existing type-directed operations and Rust string emission at the codegen boundary; avoid constructing an entire duplicate lowered `Program` solely to preserve old `__bl_*` pattern matches. For cases where a codegen-only helper representation is useful, keep it private to codegen and never return it from `parse`. Alternative: clone and desugar `Program` before validating; that would maintain two parallel AST contracts and risk divergence between `parse`/`validate` and `transpile`.

3. **Preserve compatibility at the `.bl` and generated-runtime boundaries, not the Rust AST-shape boundary.** `Expr` is public, so adding variants and changing `Call` contents may break consumers that exhaustively match it. Keep public `parse`, `expr::expression`, `validate`, and `transpile` signatures, and preserve existing diagnostic paths and rejection behavior. Existing compile-and-execute tests remain the oracle for generated Rust; add parser-shape assertions to ensure the tree is actually source-faithful. No `syn` dependency.

## Risks / Trade-offs

- [Many exhaustive `Expr` matches across semantic, decision, graph, and codegen] → Inventory all consumers before edits; update traversals for nested children, bindings, shadowing, type constraints, and fallibility; let compiler exhaustiveness checks catch omissions and add nested-case tests.
- [Dictionary entries or named options lose ordering/scope semantics] → Keep source order explicit and test dependent dictionary entries, duplicate keys, misplaced/duplicate named arguments, and calendar option behavior.
- [Expression codegen changes observable behavior or source errors] → Assert parse shapes, validation/diagnostic parity, and compile/run generated source for direct and project graph/decision paths; retain existing location strategy rather than silently replacing it.
- [Public Rust API break for exhaustive `Expr` consumers] → Mark the change in the proposal and avoid changing unrelated `Program`/`Type` signatures; no automatic adapter can make exhaustive matches source-compatible.

## Migration Plan

Refactor one expression form at a time with tests, update all downstream consumers for each form, then remove parser-generated magic encodings. No stored data or on-disk migration. If a form regresses, revert that form's AST and consumer changes together; generated projects can be regenerated with the previous transpiler. Run targeted transpiler tests, clippy, and the prescribed full workspace suite before considering the change complete.
