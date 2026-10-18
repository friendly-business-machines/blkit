# Proposal

## Why

The parsed `.bl` expression tree already represents many source constructs directly, but the parser encodes dictionary literals, indexing, `some` iterations, named arguments, and colon sequences as compiler-private `__bl_*` calls. This blurs authored syntax with lowering and forces validation and code generation to recover source constructs from magic names and argument positions.

## What Changes

- Represent those parser-lowered `.bl` expression forms as explicit source-language nodes and preserve their relevant structure (including dictionary key/value order and named-argument order).
- Adapt semantic validation and code generation to consume the new shapes while retaining existing source syntax, validation rules, diagnostics, and generated behavior.
- Keep subsequent type-directed specialization and Rust string emission intact; introducing `syn`, a separate Rust AST, or a new language feature is out of scope.
- **BREAKING (Rust API):** Consumers pattern-matching public `blkit_transpiler::expr::Expr` or the expression trees returned by `parse`/`expr::expression` must handle new variants in place of parser-produced internal calls. `.bl` source compatibility is unchanged.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. This changes the compiler's internal/public Rust AST representation, not the specified `.bl` language behavior. `skip_specs: true` records that no capability delta is needed.

## Impact

`crates/blkit-transpiler/src/expr.rs`, expression consumers in `semantic/`, `decision.rs`, `graph.rs`, and `codegen/`, plus parser/validation/generated-code tests. The public Rust `Expr` representation changes; CLI output, runtime contracts, and generated project behavior must remain equivalent. No new dependencies.
