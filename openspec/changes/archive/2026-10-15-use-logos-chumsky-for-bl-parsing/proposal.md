# Proposal

## Why

`.bl` lexing and parsing currently rely on hand-written tokenization, delimiter splitting, and string-based statement parsers. This makes grammar changes fragile and prevents syntax errors from reliably pointing to the source that caused them. Use established crates to make the front end easier to maintain and diagnostics more useful to authors.

## What Changes

- Replace the hand-written `.bl` lexer with Logos and the active declaration, statement, type, and expression parsers with Chumsky; preserve the documented language, including indented enums and contextual decision-table expressions.
- Carry source locations through parsing and surface actionable syntax and source-located validation errors where the offending construct is known; retain file context for multi-file projects and render them through the CLI's existing Miette dependency.
- Keep the existing compiler AST, semantic rules, emitted Rust, generated-project dependencies, and supported CLI commands compatible for valid `.bl` files; invalid or recovered parses never generate Rust.
- Keep the parser dependencies in the transpiler, not generated `blkit-core` consumers. No new language syntax, runtime behavior, or editor integration is proposed.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `business-language`: Require source-located, actionable diagnostics for malformed `.bl` syntax and source-bound validation failures without changing valid-program semantics.
- `cli-experience`: Render located compiler errors with source context and path while preserving accessible, automation-safe stderr behavior for direct and project transpilation.

## Impact

Primary: `crates/blkit-transpiler/src/{compiler,expr,decision,graph}.rs`, parser-facing validation/provenance in `semantic` and `project`, and `crates/blkit-cli/src/main.rs`. Add Logos and Chumsky only to the transpiler; reuse Miette in the CLI. Exercise language, graph, generated-code, CLI, project, and dev-harness fixtures/tests. Public parser and AST consumers must continue to receive equivalent valid results; error text may change but errors remain failures.
