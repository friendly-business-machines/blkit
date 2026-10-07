# Tasks

## 1. Numeric syntax and arithmetic

- [x] 1.1 Add failing parser/type tests in `tests/language.rs` for scientific literals, subtraction/negation, `*`/`/`/`**` precedence, String `+` compatibility, `==` and rejected `=`; implement lexer/AST/type-checking in `src/expr.rs` and `src/semantic/types.rs`, and verify with `cargo test --test language`.
- [x] 1.2 Add generated tests for exact `0.1 + 0.2`, division, powers, unary precedence, `string(1500.50)`, dynamic division by zero and overflow; implement checked numeric arithmetic, fallibility propagation and typed `+` emission in `src/codegen/` and a minimal shared helper if needed; verify with `cargo test --test generated`.
- [x] 1.3 Document arithmetic precedence, decimal precision/error rules, and `==` in the `README.md` expression section; verify the documented `.bl` examples in `tests/generated.rs` pass with `cargo test --test generated`.

## 2. Numeric functions and conversion

- [x] 2.1 Add tests for rounding modes, tie direction, negative and optional scales, floor/ceiling, invalid scales; implement typed call validation and decimal rounding helpers/emission; verify with `cargo test --test language --test generated`.
- [x] 2.2 Add tests for `abs`, `modulo`, `sqrt`, `exp`, `ln`, `log`, predicates, and `clamp`, including negative domains, divisor zero and integral-only predicates; implement typed calls and checked operations with representable exact cases and approximate transcendental results; verify with `cargo test --test language --test generated`.
- [x] 2.3 Add tests for default/localized `number(...)`, malformed grouping/separators, constant validation, runtime errors, and normalized `string(Number)`; implement shared parsing/type-checking/emission without locale guessing; document signatures and invalid-input behavior in `README.md`; verify with `cargo test --test language --test generated`.

## 3. List aggregates

- [x] 3.1 Add failing tests for list-only arity/typing and typed `[]`, min/max/sum/mean/median/product, empty-list identities and undefined cases; implement inference and checked aggregation using existing `Number`; verify with `cargo test --test language --test generated`.
- [x] 3.2 Add tests for sample `stddev` (N−1), small/empty inputs, overflow and deterministic `mode` ties; implement these functions and their runtime errors, document aggregate empty/tie/sample conventions in `README.md`; verify with `cargo test --test language --test generated`.

## 4. Numeric point and interval relations

- [x] 4.1 Add tests for `before`, `after`, `meets`, `metBy` overloads with `Number` and `Range<Number>`, plus existing point functions, included/open and unbounded endpoints, and preserved range/range and Date behavior; update validation and code generation for point overloads; document overloads in `README.md`; verify with `cargo test --test language --test generated`.

## 5. Integration and review

- [x] 5.1 Exercise representative numeric expressions and runtime errors through both a generated decision table and a generated process route/project build; verify the new integration tests and regressions with `cargo test --test generated --test project` and `cargo test --lib`.
- [x] 5.2 Run three independent read-only reviews of the same completed diff using `.pi/skills/review-rust-project/SKILL.md`, `.pi/skills/review-rust-microsoft/SKILL.md`, and `.pi/skills/review-rust-google/SKILL.md` (reviewers must not see each other's findings); have the parent review the three reports and adjudicate every finding, including disagreements, against code and sources, record each acceptance/rejection and why, implement accepted changes, then use the existing ponytail-review skill on the revised diff, address actionable findings, and re-verify affected work with relevant `cargo test` commands. Keep this task unchecked until all stages finish.

### Review adjudication

- **Rust-project P1, Microsoft P1, Google P1 (sqrt max): accepted.** `checked_sqrt` added `MAX + 1` on its first iteration although the square root fits. A nonzero half-value initial estimate now avoids overflow; unit tests first reproduced the error, then passed for both `MAX` and `1e-28`. This is required by the Number math spec, not a language-rule claim.
- **Rust-project P2, Microsoft P2 (public math arity): accepted.** The public `math("sqrt", &[])` and short `modulo`/`clamp` slices panicked despite a `Result` return. Explicit arity checks return `Err` before indexing, as supported by [Rust API Guidelines C-VALIDATE](https://rust-lang.github.io/api-guidelines/dependability.html#c-validate) and [Microsoft M-PANIC-IS-STOP](https://microsoft.github.io/rust-guidelines/guidelines/correctness/#M-PANIC-IS-STOP). Focused tests reproduced and then verified each case. Existing `.bl` call validation remains unchanged.
- **Google P1 (negative-scale round-up): accepted.** Dividing `1e-28` by 10 first rounded the quotient to zero, incorrectly making `roundUp(1e-28, -1)` zero. Remainder-based rounding now preserves the nonzero fraction. The same helper test verified negative `floor` and toward-zero behavior. The [Comprehensive Rust API-design guidance](https://google.github.io/comprehensive-rust/idiomatic/welcome.html#foundations-of-api-design) is contextual advice; the Number rounding spec supplies the behavioral contract.
- There were **no rejected or conflicting findings** across the three independent lenses. The Microsoft run completed with findings in its transcript, but its output artifact was empty after the original runner died; independent Rust-project and Google fallback reports were persisted through the same subagent protocol.
- **Ponytail-review on the revised diff:** `src/codegen/decision.rs` knowledge environment construction manually cloned each key/value; accepted the `shrink` finding and replaced it with `env.clone()` plus `extend`. Other numeric helper/codegen branches implement approved requirements and had no actionable complexity-only deletions. Net: ~3 lines removed.
- **Post-review verification:** the focused Number helper tests passed (4/4); `cargo test --test generated --test language` passed (47/47 and 38/38); the numeric project-build test passed; the remaining library and project suites passed (12/12 and 29/29) with only their two Docker-backed tests skipped. `cargo fmt --all -- --check`, `git diff --check`, and `openspec validate enhance-bl-number --strict` passed. A later full run with Docker available, `cargo test -- --test-threads=1`, passed all 270 tests (0 failed, 0 ignored). A default parallel run failed in PostgreSQL integration tests when concurrent container starts returned `Resource temporarily unavailable`; serializing those tests remains a tooling question, not an outstanding Number implementation task.
