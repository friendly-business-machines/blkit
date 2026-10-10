# Tasks

## 1. Named dictionary declarations and expression syntax

- [x] 1.1 Add failing parser tests for `Order = {total: Number, blocked: Bool};`, quoted/nested typed fields, cross-file resolution, rejection of `type Order:`, retained `enum Name:` syntax, and all dictionary literal/postfix forms; verify targeted `tests/language.rs` tests fail before implementation.
- [x] 1.2 Extend top-level declaration parsing and expression statement splitting/AST/parser to accept named shapes and all dictionary literal forms without confusing `{}` expression values, peer blocks, lists, or ranges; reject the old `type Name:` form with a migration diagnostic. Verify targeted parser tests from 1.1 pass.
- [x] 1.3 Migrate all checked-in `.bl` examples, embedded test sources, fixtures, and README declarations from `type Name:` to `Name = {field: Type, ...};`, leaving enums unchanged; verify `rg 'type [A-Za-z_]+:' examples tests README.md` finds no legacy examples and `cargo test --test language --test graph_syntax` passes.

## 2. Dictionary types and JSON boundary

- [x] 2.1 Add failing validation and generated-project tests for named shapes (`Order`), required/extra fields, nested typed ports, cross-file declarations, decision-table multi-output dictionaries, AND-join dictionaries, homogeneous and heterogeneous dictionaries, `Dictionary<T>`, `DictionaryEntry<T>`, `Value`, literal-key projections, invalid assignments, and decimal-preserving JSON round trips; verify targeted tests fail first.
- [x] 2.2 Implement unified named/ad hoc dictionary type resolution, decision-table and gateway shape handling, runtime value representation/serialization, typed `Number` versus dynamic JSON number behavior, and generated Rust type mapping; verify new tests and existing `cargo test --test language --test generated --test graph_validation` pass.
- [x] 2.3 Document the `Name = {...};` declaration, migration from `type Name:`, named dictionary port JSON, and dynamic dictionary JSON rules in `README.md`; verify the documented examples compile or are exercised by tests from 2.1.

## 3. Access, inspection, and updates

- [x] 3.1 Add failing compiler/runtime tests for named and ad hoc dictionary dot/bracket access, direct/nested `getValue`, missing/non-dictionary paths, structural equality, sorted `keys`/`values`/`getEntries`, `has`, `size`, and dictionary `isEmpty`; verify targeted tests fail before implementation.
- [x] 3.2 Implement key/path operations, overload resolution, fallible code generation, and deterministic iteration for both named and ad hoc dictionaries with shared runtime helpers; verify tests from 3.1 and existing String/Calendar `isEmpty` and migrated named-field tests pass.
- [x] 3.3 Add failing tests for `dictionaryPut`, nested put, later-wins shallow merge, remove, unchanged original values, invalid path/type behavior, and shape-changing updates to named dictionaries returning ad hoc dictionaries; then implement those transformations and verify success and failure tests pass.
- [x] 3.4 Document dictionary query/update functions, ordering, and missing-path errors in `README.md`; verify examples from `dictionaries-changes.md` are covered by passing generated-code tests.

## 4. List iteration over dictionary results

- [x] 4.1 Add failing parse/validation/runtime tests for `every ... satisfies ...`, `for ... return ...`, scoped/shadowed variables, empty lists, non-list sources, `sum(values(scores))`, sorted entry iteration, and propagation of body errors; verify targeted tests fail first.
- [x] 4.2 Extend parsing, type inference, reference walking, fallibility handling, and Rust emission for scoped list iterations in decision and process expressions; verify tests from 4.1 plus existing list/range/decision tests pass.
- [x] 4.3 Document the two iteration forms and typed dictionary-to-list examples in `README.md`; verify the documented expressions are exercised by passing tests.

## 5. Integration and independent review

- [x] 5.1 Verify all examples in `dictionaries-changes.md` through generated execution and negative cases through validation/runtime tests, run formatting and focused test suites, then run `cargo test --all --workspace -- --test-threads=1` with timeout at least 5,400 seconds; record the results. `cargo fmt --all`, `cargo clippy --all --workspace`, full workspace tests (including 84 generated tests), and `openspec validate add-dictionaries --strict` passed; no legacy declarations remain in checked-in source examples.
- [x] 5.2 Run four independent read-only reviews of the same completed diff using `.pi/skills/review-rust-project/SKILL.md`, `.pi/skills/review-rust-microsoft/SKILL.md`, `.pi/skills/review-rust-google/SKILL.md`, and `.pi/skills/rust-skills/SKILL.md` (reviewers must not see each other's findings); have the parent adjudicate every finding, including disagreements, against code and sources and record accept/reject reasons, implement accepted fixes, then use the existing ponytail-review skill on the revised diff, address actionable findings, and re-verify affected work. Verify the adjudication record exists and affected checks pass; leave this task unchecked until all stages finish. Findings and dispositions are in `review-adjudication.md`; regression tests and full verification passed.
