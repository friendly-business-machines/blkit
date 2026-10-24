# Task 5.3 — parent adjudication and fix-wave ownership

Reviewed all four independent reports against immutable snapshot ebe975887124e45fa67407b9f46b4c894dc06cb3f293450009f12be82c0df6ef. The parent inspected the cited functions and shared-helper callers (task-5.3-callers.txt), approved specs/design, C-VALIDATE primary text and Google's type-system primary text; source provenance is in review-sources/. Guidelines are advisory; the actual runtime/compiler defects and approved requirements justify fixes. No extra types/frameworks/style redesigns are accepted.

## Every finding disposition

| ID | All originating report findings | Decision / reason / severity |
|---|---|---|
| F1 | Rust-project 1; Microsoft 1; Google 4; rust-skills 5 | ACCEPT, Important/P1. request_type recursively expands generic Table row fields while record_cycle only follows bare Named fields. Authored declarations can exhaust compiler stack. Reject unsupported cyclic inline schema expansion before generation; keep supported List<Record> recursion. Rust-project P0 is regraded P1: serious compiler-input crash, not evidence of universal service outage. |
| F2 | Rust-project 2; Microsoft 2; Google 1; rust-skills 3 | ACCEPT, Important/P1. emit_table_cell_type has no Nullable or Generic Table lowering; constructors use new/from_dicts paths that reject nulls. Preserve validated nullable source cells, nested nullable members and declared Table cells through both constructors without permitting authored null literals. |
| F3 | Rust-project 3; Google 2; rust-skills 1 | ACCEPT, Important/P1. toList/toValue adopt expected types rather than known column types; nullable-to-non-null and wrong non-null conversions evade transpilation checks. Infer from known fields, preserve nullable types and validate expected compatibility; retain runtime row-count checks. |
| F4 | Google 3; rust-skills 2 | ACCEPT, Important/P1. Single-output decision validation calls validate_input_nulls, recursively rejecting legitimate null-bearing dictionary/list conversions. Separate output validation from input-only blanket rejection while preserving declared named/table shape checks and existing input validation. |
| F5 | Google 5; rust-skills 7 | ACCEPT, Important/P1. record_cycle ignores transparent Nullable wrappers and admits Node? by-value cycles; generated Option<Node> is infinitely sized. Follow nullable wrappers but preserve real container indirection. |
| F6 | rust-skills 4 | ACCEPT, Important/P1. infer_dependencies/expr_names lack TableJoin and TableSortKey InOrder arms; valid compositions lose node dependencies. Cover the corresponding dependency/name/call walkers and forward references without treating column names as dependencies. |
| F7 | rust-skills 6 | ACCEPT, Important/P1. Selector lists unboundedly extend separately bounded ranges. Enforce cumulative expansion before allocation/extension, including mixed individual indices; explicit resource error, no truncation or accidental per-range-only bound. |
| F8 | rust-skills 8 | ACCEPT, Important (review P2 promoted). table_column_names/static selection retain duplicates while runtime deduplicates; valid declared results are rejected. Normalize first occurrence consistently for bracket/method selection. This is supported syntax, not polish. |
| F9 | rust-skills 9 | ACCEPT, Important (review P2 promoted). table_row_expr does not recurse into Dictionary even though parser/check_table_columns do. Supported dictionary-valued withColumn row expressions cannot infer/lower. Add matching dictionary traversal and compile/execute regression. |
| F10 | Rust-project 4 | ACCEPT, Important/P1. Table sort masks checked comparator errors as Equal, violating total-order assumptions, and fully ranked incompatible temporal values bypass comparisons. Ensure error propagation and compatibility checks, including rank values, with stable bounded work. No panic reproduction claimed; checked std sort contract permits panic for non-total comparator. |
| F11 | Rust-project 5; Microsoft 3 | ACCEPT, Important (review P2 promoted). from_optional retains Some(Cell::Null) although JSON decoding uses None; joins then change behavior on serialization roundtrip. Normalize top-level null once at shared construction boundary; preserve nested null values. Actual semantic inconsistency, not representation taste. |

No findings rejected; duplicate reports consolidated, not discarded. No source-guidance disagreement remains. All 22 reported items map above.

## Safety rulings

- Reject unsupported cyclic inline Table schema generation with an actionable validation diagnostic rather than add a recursive-schema reference architecture. This is the smallest safe behavior for previously crashing inputs. Preserve established List<Record> recursion. Cost if wrong: recursive Table schemas would require an explicitly designed schema-reference extension later.
- Treat F8/F9/F11 as Important because they change supported behavior, regardless of original P2 labels. Cost if wrong: a few extra focused regression fixes, not new feature scope.

## One fix wave, sequential exclusive owners

These are independent source seams, so a single broad writer would blur ownership. No concurrent writers. Each owner uses TDD and commits only its own delta over the preserved pre-existing implementation, never stages whole pre-existing modified files blindly. If hunk staging is unsafe, leave source unstaged and provide a binary patch; the final parent-owned implementation commit can include all approved changes.

| Order | Owner | Findings | Exclusive source/test boundaries | Handoff |
|---|---|---|---|---|
| 1 | runtime worker | F10,F11 | crates/blkit-core/src/table.rs and its unit tests only | stable checked sort API preserved; normalized optional null; focused RED/GREEN logs/report |
| 2 | semantic worker | F1,F3,F5,F6,F8,F9 | transpiler semantic/*, decision.rs walkers; tests/language.rs, tests/table_rows.rs, tests/nullable.rs (no generated.rs) | known conversion types/nullability; safe schema cycle errors; dependency/scope consistency; RED/GREEN logs/report |
| 3 | emission worker | F2,F4,F7 plus generated integration coverage of F9 | transpiler codegen/*; tests/generated.rs only (runtime helper changes require parent coordination) | consume prior semantic decisions; nullable cell lowering/output validation/cumulative selector cap; RED/GREEN logs/report |

Shared contracts: the existing Type::Generic("Nullable", T), Type::Generic("Table", NamedRow), Table::from_optional storage, expected-type compatibility and no authored null syntax remain. Runtime worker does not change public signatures. Semantic worker exposes corrected existing inference; emission consumes it after semantic handoff. All workers reuse /tmp/blkit-add-business-language-tables-target.AwpF302Y with CARGO_INCREMENTAL=0, CARGO_PROFILE_TEST_DEBUG=0, CARGO_PROFILE_DEV_DEBUG=0, inherited by nested builds. No new dependencies/targets, no Docker prune/cleanup. No child subagents, task checkbox edits, sync/archive/merge or main-checkout edits.

After all owners: parent inspects reports and patches; fresh independent scoped re-review of all accepted findings/fix blast radius; Ponytail review of revised whole branch; address actionable findings with TDD/re-review; rerun full workspace suite (timeout>=5400) because implementation changed. Task5.3 remains unchecked through all stages. Then dispatch the separately requested disk investigation before build cleanup and continue authorized sync/archive/integration workflow.
