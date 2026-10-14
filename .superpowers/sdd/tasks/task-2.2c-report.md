# Task 2.2c: scoped inline match predicate

Base: `063b1e7`. Added `Expr::InlinePredicate` parsed only as argument 2 of built-in `remove`/`listReplace`; a single identifier binds only the predicate body. Concrete and symbolic typing require a list element and Bool body; replacement must match element type. Decision/context dependency walks exclude the local binder but retain other references. AST traversals now handle the form. No positional/predicate editing evaluation is implemented in this task.

## Verification

- RED: `cargo test -p blkit-transpiler --test language inline_predicate -- --test-threads=1` failed all three new tests before implementation (unsupported syntax).
- GREEN: same command, 3 passed.
- `cargo test -p blkit-transpiler --lib -- --test-threads=1`: 37 passed.
- Preserved generated subsets `generated_item_filters`, `generated_context_item_filters`, `generated_empty_filter`, `generated_list_index_of`, `generated_union_does_not_capture_result_iteration_binding`: 8 passed across five invocations.
- Full language target: 133 passed, 9 failed. Remaining 2.1 RED names: `distinct_accepts_typed_list`, `duplicate_values_accepts_typed_list`, `insert_after_spreads_list_argument`, `insert_before_accepts_one_based_position`, `intersection_accepts_multiple_lists`, `list_functions_reject_incompatible_elements_and_invalid_predicates`, `list_replace_accepts_one_based_position`, `list_set_functions_typecheck_structural_dictionary_elements`, `remove_accepts_one_based_position`.
- `cargo fmt --all`, `cargo clippy --all --workspace`, `git diff --check`: passed; Clippy reported pre-existing warnings in blkit-core and generated dev-harness code. All Cargo commands used `CARGO_TARGET_DIR=/tmp/blkit-list-expressions-recovery-target CARGO_BUILD_JOBS=2`; available /tmp disk space at last check: 743G.

## Limits / next

Editing runtime and the remaining 2.1 cases belong to 2.2d/e/f. Complete generated and full workspace suites were not run per the scoped task; independent review is pending. OpenSpec tasks/checkboxes unchanged.

## Reviewer-fix loop (base `65efb17`)

Reviewed P1/P2 in `98be85e8-324c-4e98-be04-433324756b90_reviewer_output.md`. P1 root cause: `infer_with` created an inline predicate local without `check_name`, while iteration checks its binder and emitter interpolates it as a Rust identifier. P2 root cause: argument parsing greedily treated any `function` token as a predicate even when the following tokens formed a complete ordinary knowledge call. No editing runtime or OpenSpec artifact changes.

RED: `inline_predicate_rejects_reserved_rust_bindings`, `knowledge_calls_shadow_inline_predicate_syntax_without_swallowing_nested_calls`, and `generated_knowledge_remove_accepts_nested_function_call` failed against the original implementation. GREEN: the parser now chooses inline syntax only for a complete `function(identifier)` header followed by a body (not a call continuation); otherwise ordinary nested calls parse normally, including `function(i)` and `function(i + 1)`. Malformed inline forms still error. Concrete inference calls shared `check_name` before introducing the predicate local. The new generated test compiles/executes shadowing knowledge models `remove` and `function` with a forward context-entry dependency.

Validation: focused inline language tests (5 passed); generated knowledge-shadow/dependency (1 passed); transpiler lib (37 passed); preserved generated filters `generated_item_filters`, `generated_context_item_filters`, `generated_empty_filter`, `generated_list_index_of`, `generated_union_does_not_capture_result_iteration_binding` (8 passed); `cargo fmt --all`, `cargo clippy --all --workspace`, `git diff --check` passed. Clippy emitted unchanged blkit-core and generated dev-harness warnings. All Cargo invocations used `CARGO_TARGET_DIR=/tmp/blkit-list-expressions-recovery-target CARGO_BUILD_JOBS=2`.

Self-review: only two parser/semantic behavior changes, one language and one generated regression file; ordinary knowledge calls remain reachable and inline predicate parsing still requires an edit match argument. Existing 2.1 predicate editing runtime tests remain intentionally RED for 2.2e; complete language/generated/full-workspace suites were not run under this scoped fix.

## Re-review P1 checkpoint (base `9ed8547`; blocked on syntax decision)

Added two focused parser/type regressions for `remove(input, function(i) -i < 0)` and `remove(input, function(i) [i] == [i])`. Both are RED at the parser-AST assertion: argument 2 is currently parsed as an ordinary call continuation rather than `InlinePredicate`; type assertions will become reachable after parsing is fixed. Added an ordinary subtraction AST preservation check for `remove(input, function(i) - 1)` to the existing knowledge-call test; it is GREEN. No production code or syntax rule changed.

The ambiguity is real: lexing discards whitespace and `function(i) -i < 0` can also parse as `(function(i) - i) < 0`; `function(i) [i] == [i]` can also parse as `function(i)[i] == [i]`. Current token-only lookahead cannot select intent from these forms without a grammar rule. Possible decisions include explicit inline-body delimiter, whitespace-sensitive adjacency, semantic disambiguation, or an approved precedence rule; do not implement one without approval. Supervisor requested this RED-test checkpoint while the user is asked to choose syntax/precedence. Leave the worktree dirty, without speculative parser implementation or commit. Existing editing-runtime RED cases remain deferred to 2.2e.

Validation: targeted `inline_predicate_accepts_` (2 expected FAIL); all `inline_predicate` language tests (5 PASS, 2 expected FAIL); knowledge call/subtraction test (PASS); nested generated knowledge call (PASS); transpiler library tests (37 PASS); `cargo fmt --all`, `cargo clippy --all --workspace`, `git diff --check` (PASS). Clippy reported existing blkit-core/generated dev-harness warnings. Cargo commands used `CARGO_TARGET_DIR=/tmp/blkit-list-expressions-recovery-target CARGO_BUILD_JOBS=2`; no full-suite run and no files staged.

Self-review: tests independently assert both ambiguous body forms and ordinary subtraction without assuming a parser implementation; remaining risk is the unapproved syntax tie-break. No OpenSpec tasks altered.

## Reserved-function grammar resolution (base `733a2aa`)

The user resolved the checkpoint ambiguity: `function` cannot name a knowledge model; a complete `function(identifier)` header in the remove/listReplace match slot always starts an inline predicate, including unary-minus and list-literal bodies, and cannot fall back to an ordinary call if its body is missing. Kept the two unstaged checkpoint tests/report sections intact; replaced the checkpoint's obsolete ordinary-subtraction expectation with an inline-predicate AST and non-Bool rejection. Semantic knowledge-name validation covers decision declarations; legacy indentation-style decision declarations are rejected by the project parser. Existing custom `remove` knowledge shadowing and forward context-entry dependencies are tested with a legitimate `position(i)` knowledge call instead.

RED before the fix: `function_header_requires_predicate_body_in_edit_match_argument` accepted the missing body; `function_is_not_a_valid_knowledge_declaration_name` accepted the braced declaration. The checkpoint's two unary/list literal parser/type tests were already RED before production edits. GREEN after the fix: six `inline_predicate` language tests, two `function_` language tests, one generated knowledge forward-dependency test, 37 transpiler library tests, and eight preserved generated tests in five focused filters. `cargo fmt --all`, `cargo clippy --all --workspace`, and `git diff --check` passed; Clippy emitted pre-existing blkit-core and generated dev-harness warnings. All Cargo invocations used `CARGO_TARGET_DIR=/tmp/blkit-list-expressions-recovery-target CARGO_BUILD_JOBS=2`; available /tmp disk: 741G. No runtime, OpenSpec, or full-suite changes.

Self-review: removed only the parser lookahead excluding valid body starts; a completed predicate header in the designated argument now requires `parse(0)` to consume a body, so `)` and `,` fail immediately. The shared decision validator rejects `knowledge function` before dependency or code generation. Outside the designated argument, ordinary names and knowledge calls still parse/validate under existing rules; no valid `function` knowledge call survives validation. Existing 2.1 editing/set cases remain intentionally RED for later tasks; complete language/generated/workspace suites are not part of this scoped verification. Independent review pending.
