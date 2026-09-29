# Tasks

## 1. Temporal scalar types

- [x] 1.1 Add failing compiler and generated-Rust tests for `Date`/`Time` input and output, typed date/time/datetime constructors, ordering, invalid ISO input and missing offsets; verify they fail for the missing support with `cargo test --test language --test generated`.
- [x] 1.2 Implement Date/Time type validation, constructors, generated aliases and serialization using the installed `chrono`; verify the tests from 1.1 pass with `cargo test --test language --test generated`.
- [x] 1.3 Document Date/Time wire formats and typed literals beside the existing DateTime documentation in `README.md`; verify examples in the documentation compile and generated-code tests cover the formats with `cargo test --test generated`.

## 2. Range syntax and membership

- [x] 2.1 Add failing parser/type-validation tests for all four boundary forms, unbounded `null`, `Number`/`Date`/`DateTime`/`Time` bounds, decimal/list/grouping ambiguity, mixed bounds and inverted constants; verify failures with `cargo test --test language`.
- [x] 2.2 Implement range parsing and internal type inference without a public `Range<T>` type; verify 2.1 and existing language tests pass with `cargo test --test language`.
- [x] 2.3 Add failing generated-Rust tests for inclusive/exclusive membership, unbounded ends, `between`, equality, dynamically inverted/empty ranges and DateTime instant ordering; implement a shared range helper/emission and verify with `cargo test --test generated`.
- [x] 2.4 Document range syntax, typed bounds and the `null` limitation in `README.md`; verify the illustrated `.bl` snippets are exercised by `cargo test --test generated`.

## 3. Interval relations

- [x] 3.1 Add failing compiler and generated-Rust tests for `before`, `after`, `meets`, `metBy`, `overlaps`, `overlapsBefore`, `overlapsAfter`, `includes`, `during`, `starts`, `startedBy`, `finishes`, `finishedBy`, and `coincides`, including open/unbounded, type-mismatch and adjacent-date cases; verify expected failures with `cargo test --test language --test generated`.
- [x] 3.2 Add typed built-in relation resolution and generated evaluation by reusing the shared range operations; verify all relations and existing knowledge-model calls with `cargo test --test language --test generated`.
- [x] 3.3 Document argument order and endpoint semantics for relations in `README.md`; verify documented examples through `cargo test --test generated`.

## 4. Decision-table unary tests

- [x] 4.1 Add failing decision-table tests for `matches` lists of ranges and comparison tests, Boolean composition, temporal inputs, invalid empty/mixed-type lists, and unchanged hit policies/output-column parsing; verify expected failures with `cargo test --test language --test generated`.
- [x] 4.2 Parse table-only `column matches (tests...)` and lower alternatives to typed Boolean conditions before existing table evaluation; verify 4.1 and prior table cases with `cargo test --test language --test generated`.
- [x] 4.3 Document unary-test table syntax and its decision-table-only restriction beside the pricing example in `README.md`; verify example rules with `cargo test --test generated`.

## 5. Integration check

- [x] 5.1 Run `cargo test` and `openspec validate add-bl-ranges --strict`; inspect failures and verify existing `.bl` examples still compile and execute.
