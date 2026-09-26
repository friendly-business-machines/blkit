# Tasks

## 1. Compiler entry point and syntax

- [x] 1.0 Pin Rust tooling in the devcontainer and install it locally; verify `rustc --version`, `cargo --version`, and `sudo docker build -f .devcontainer/Dockerfile .`.
- [x] 1.1 Create a minimal Rust compiler crate and CLI accepting a `.bl` file and Rust output path; verify `cargo test` and CLI help/usage run.
- [x] 1.2 Parse namespace/version, record and enum declarations, `List<T>` types, and typed process signatures; add valid/invalid parser fixtures and verify parser tests report missing declarations and syntax errors.
- [x] 1.3 Parse process statements and expressions (`if`/`else`, `return`, field/enum access, literals, comparisons, boolean operators, list literals); verify parser tests cover nested branches and malformed expressions.

## 2. Static semantics

- [x] 2.1 Resolve declared and built-in type names with duplicate/unknown-name diagnostics; verify validation tests accept valid records/enums and reject unknown types and unsupported `Table<T>`.
- [x] 2.2 Type-check numeric, string, boolean, enum, field-access, and list expressions plus process return types; verify tests reject mismatched list elements, invalid field/variant references, and wrong return types.
- [x] 2.3 Validate every process branch returns a value of its declared output type; verify tests reject a missing return path and accept both sides of an `if`/`else` that return correctly.

## 3. Rust generation

- [x] 3.1 Generate Rust record/enum definitions and decimal-backed `Number` literals from validated source; verify generated output compiles for representative declarations with `cargo test`.
- [x] 3.2 Generate callable Rust process functions including branching, comparisons, boolean expressions, and typed list values; verify a compiled end-to-end test returns expected results for both sides of a numeric decision and accepts `List<Number>` input.
- [x] 3.3 Ensure invalid input produces diagnostics without writing Rust output; verify CLI tests for parse and type errors leave no generated file.
- [x] 3.4 Document supported `.bl` syntax and CLI usage in a short README example; verify the documented example generates Rust that compiles.

## 4. Integration

- [x] 4.1 Run the full Rust test suite and `openspec validate language-mvp --strict`; verify both commands pass and no deferred table features appear in MVP examples.
