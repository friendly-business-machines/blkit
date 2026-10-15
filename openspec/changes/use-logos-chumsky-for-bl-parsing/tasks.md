# Tasks

## 1. Characterize and tokenize the existing language

- [x] 1.1 Add/strengthen tests in `crates/blkit-transpiler/tests/language.rs` and `graph_syntax.rs` for the public AST and accepted/rejected delimiter, enum, quote, comment, range, decision-table, precedence and ordering cases; verify the characterization tests pass on the current parser before replacing it.
- [x] 1.2 Add Logos and Chumsky (including required Pratt feature) as transpiler-only dependencies with `cargo add`; implement a spanned `.bl` token stream that preserves original byte positions, enum indentation and full-line comments, and test strings, exponents, multi-character operators, invalid characters and EOF with targeted transpiler tests.

## 2. Replace the active grammar while preserving outputs

- [x] 2.1 Implement Chumsky expression parsing behind `expr::expression` and `table_condition`, preserving the existing `Expr` lowerings, precedence, contextual unary tests, iteration, named arguments and rejected forms; verify new AST-shape tests and `cargo test -p blkit-transpiler --test language` pass.
- [x] 2.2 Parse namespace/version, records, nested generic types, enum variants, declarations and semicolon/braced bodies with Chumsky over the spanned tokens, retaining `parse(&str) -> Result<Program, String>`; verify syntax and quoting tests plus `cargo test -p blkit-transpiler --test language` pass.
- [x] 2.3 Parse peer, decision-task/table/context/knowledge, and process/flow/bind statements without re-splitting normalized source strings; preserve decision dependency inference and declaration/flow order; verify `cargo test -p blkit-transpiler --test graph_syntax --test graph_validation --test language` passes.
- [x] 2.4 Search callers and retire only unused hand-written lexing, `source_items`, `split_top_level` and inactive legacy parser helpers without removing required public valid-source entry points; verify no active path uses string-based source parsing and focused transpiler tests pass.

## 3. Source provenance and actionable compiler errors

- [x] 3.1 Add a structured internal compiler error with original-source byte spans and source identity plus optional provenance outside the public AST; preserve string-returning public adapters; verify direct-file malformed expression, invalid token, and unclosed-brace tests report correct positions without producing Rust.
- [x] 3.2 Add bounded Chumsky recovery at `;`, `}` and enum variant boundaries, returning errors rather than a successful recovered AST; verify a two-independent-error test reports both without duplicate cascades and emits no Rust.
- [x] 3.3 Preserve per-file provenance while `Project::programs` merges scopes; locate at least unknown types and invalid binding references from the correct source, and avoid fabricated spans for unlocatable global errors; verify direct and two-file project diagnostics tests cover both failures and no successful partial output.

## 4. Present diagnostics to authors

- [x] 4.1 Route direct and project CLI transpilation failures through Miette using original source snippets, path and byte-correct labels; keep accessible narration and plain redirected/`NO_COLOR` stderr; verify `crates/blkit-cli/tests/cli.rs` and `project.rs` cover located and unlocated errors, no ANSI controls or output file on failure.
- [x] 4.2 Document the new compiler diagnostic format and one direct/project invalid-source example in `README.md`; verify the examples produce the documented paths, locations and exit behavior in the CLI tests.

## 5. Integration and independent review

- [x] 5.1 Verify generated Rust and runtime behavior with `cargo test -p blkit-transpiler --test generated`, `cargo test -p blkit-cli --test cli --test project`, and `cargo test --all --workspace -- --test-threads=1` (timeout at least 5,400 seconds); compare build cost and representative parsing time with the pre-migration baseline, and verify generated manifests still exclude transpiler/parser crates.
- [x] 5.2 Run four independent read-only reviews of the same completed diff using `.pi/skills/review-rust-project/SKILL.md`, `.pi/skills/review-rust-microsoft/SKILL.md`, `.pi/skills/review-rust-google/SKILL.md`, and `.pi/skills/rust-skills/SKILL.md` (leonardomso/rust-skills; reviewers must not see each other's findings); have the parent review the four reports and adjudicate every finding (including disagreements) against the code and sources, record which to accept or reject and why, implement accepted changes, then use the existing ponytail-review skill on the revised diff, address its actionable findings, and re-verify affected work. Keep this task unchecked until all stages finish.
