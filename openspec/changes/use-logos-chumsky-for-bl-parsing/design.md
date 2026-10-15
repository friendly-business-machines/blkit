# Design

## Context

See proposal.md for motivation and the two delta specs for observable behavior. The active source path is `compiler::parse` -> `source_items` -> string-based `decision::parse_braced` / `graph::parse_peer` / `graph::parse_source`, with a separate lexer and Pratt parser in `expr.rs`; `type_ref` and `split_top_level` also parse source fragments. `Project::programs` parses files separately before merging declarations by namespace/version. `semantic::validate` and codegen consume the existing public, owned AST. The CLI currently wraps compiler `String` errors in Miette diagnostics without source spans; generated projects depend on `blkit-core`, not on the transpiler. Some legacy parsing entry points remain public but are not used by the active grammar.

## Goals / Non-Goals

**Goals:** Keep one active `.bl` token/grammar pipeline, preserve the AST and generated behavior for valid source, and retain source provenance long enough to report useful syntax and selected validation errors in both single-file and merged-project compilation. Recover only at boundaries that make independent errors meaningful; never compile a recovered AST.

**Non-Goals:** Change `.bl` syntax, runtime semantics, generated dependencies, extension-task behavior, the programmatic AST shape, or all existing semantic error wording. No editor/IDE or incremental parser. Public compatibility for obsolete legacy grammar helpers is not a reason to retain a second production parser; inspect callers before retiring them.

## Decisions

1. **Lex original input with Logos into spanned tokens; parse those tokens with Chumsky.** Lex identifiers, decimal/scientific numeric text, unescaped quoted text, punctuation/operators, and invalid characters as source byte ranges. Preserve line boundaries/leading indentation for colon-style enums and only skip full-line `#` comments outside strings. Multi-character operators must win over prefixes, and keywords used as names in other contexts must be handled as contextual grammar, not indiscriminately reserved lexemes. Adapt `(token, span)` pairs to a Chumsky spanned input and carry an end-of-file position. Alternatives: extending `source_items` retains the current delimiter fragility; Chumsky-only character lexing gives up a dedicated lexer without simplifying the grammar.

2. **Parse active declarations and expressions into the same AST in source order.** Chumsky grammar covers the braced declarations, nested decision nodes and graph statements, generic type references, quoted schema keys, and indented enum bodies directly instead of parsing normalized statement strings. Build expressions with explicit precedence/associativity (`**` right-associative, tighter than unary minus) and explicit branches for dictionary/list/range ambiguity, iteration, `between`, named calendar arguments, and table-only unary `matches`. Preserve current internal expression lowerings (`__bl_dictionary`, `__bl_index`, named-argument wrappers) and decision dependency inference so `semantic` and codegen do not need a redesign. Retain public `parse`, `expr::expression`, and `transpile` results as compatible adapters around the new internals; only retire inactive legacy helpers after inspecting their callers. Alternative: keep a hand-written Pratt parser over Logos tokens, but that would leave the requested parser migration incomplete.

3. **Keep diagnostics/provenance outside the public AST.** An internal parse result carries the owned `Program` plus original-file source and byte spans for declarations, type references, expressions, and flow/bind statements. Project grouping merges provenance alongside programs, retaining original file identities and statement order; errors in one file must never be mapped to another. Internal parsing/validation returns structured errors with optional source identity/span and useful messages; existing public `Result<_, String>` entry points format those errors for compatibility. Route the CLI's direct and project transpile paths through structured results, adapting them to the already installed Miette renderer with source excerpts and readable, non-graphical stderr. If a semantic error cannot be mapped with certainty, report the correct file/project and message without a fake span. Alternative: add spans to every public AST node; that breaks pattern matches and forces unrelated semantic/codegen changes.

4. **Recover carefully, fail closed.** Treat unrecognized lexical text as a located error token so later statements can be inspected. Recover at statement `;`, declaration `}`, and enum variant line/`;` boundaries, retaining independent errors while suppressing duplicate cascades. The parser result is successful only if no lexing or parsing errors were collected; semantic validation and generation run only on clean programs. Unknown types and bad process bindings receive located validation messages; unrelated validation still fails with correct source/project context. Alternatives: stop at first syntax error (less helpful), or always emit a recovered AST (unsafe to validate/generate).

## Risks / Trade-offs

- **Enum indentation and full-line comments** -> preserve newlines, blank-line behavior and source offsets; test inline versus quoted `#`, variant layout, and comments around declarations.
- **Grammar drift and error cascades** -> characterize accepted and rejected forms before swapping, add AST-equivalence/behavior tests for precedence, ranges, rules, iteration, graph ordering, missing delimiters and legacy syntax, and recover only at tested boundaries.
- **Provenance after cross-file merging** -> pair original source references with declaration/statement provenance before grouping; test unknown type and bad binding in the *second* source file.
- **Added build time/dependencies and deeply nested input** -> confine Logos/Chumsky to the transpiler, check nested-input failure behavior, and compare CLI compile and parsing time to baseline; no performance claim without measurement.
- **Existing CLI string APIs and output conventions** -> keep public string adapters, add Miette rendering tests under redirected stderr and `NO_COLOR`, and ensure no output file is created after errors.

## Migration Plan

1. Add characterization tests for existing AST/language acceptance and negative cases, including isolated diagnostics for direct and multi-file sources.
2. Introduce Logos tokens and Chumsky expression parsing behind the current public APIs; test parsing and AST shape before replacing the active declaration/graph/decision parsing path.
3. Replace the active file parser, remove now-unused hand-written lexical/statement parsing, preserve semantic/codegen interfaces, and carry provenance through project grouping.
4. Add structured error and Miette presentation through both CLI paths, with targeted located validation and recovery tests; update any documented diagnostic examples.
5. Run focused suites, generated-code and CLI/project tests, then full workspace tests and performance/build-cost comparison. No persisted `.bl` or runtime data migration is needed; rollback is a code revert, leaving source files and generated-project dependency contracts unchanged.
