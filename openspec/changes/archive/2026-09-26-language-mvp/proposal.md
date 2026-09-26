# Proposal

## Why

The project needs to validate its core bet: business logic can be written as human-readable `.bl` scripts, statically checked, transpiled to Rust, and compiled into executable behavior. Starting with a small language MVP keeps the platform honest before adding runtime distribution, messaging, persistence, or custom task integrations.

## What Changes

- Add the first `.bl` language capability for closed-world, type-safe business-domain scripts.
- Support a tiny process wrapper with typed input and output.
- Support declared record types, enums, lists, and a minimal expression/branching core.
- Generate Rust from validated `.bl` source as the compiler output.
- Defer tables and exclude distributed runtime behavior, persistence, messaging, Python tasks, Rust imports, dynamic typing, nullability, and temporal types from this MVP.

## Capabilities

### New Capabilities
- `business-language`: Defines the MVP `.bl` language surface, type system, validation behavior, and Rust code generation expectations.

### Modified Capabilities
- None.

## Impact

- Introduces the initial compiler/transpiler architecture: parser, AST, name/type checking, and Rust code generation from a fully validated AST.
- Establishes `.bl` as a closed-world language using only business-domain types declared in `.bl` plus built-in `Bool`, `String`, `Number`, and `List<T>`.
- May add Rust project structure and dependencies during implementation, including a decimal representation for `Number` if needed.
