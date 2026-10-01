---
name: review-rust-microsoft
description: Use when reviewing Rust changes through Microsoft's Pragmatic Rust Guidelines for resilience, correctness, and maintainability.
---

# Microsoft Pragmatic Rust Code Review

Review the changed Rust code, read-only, using the
[Pragmatic Rust Guidelines checklist][checklist] as a source of **applicable**
questions, not a compliance mandate. This is a separate lens from the
Rust-project API review; don't repeat its API or Clippy/formatting findings.

Prioritize decisions a lint cannot make:

- Does a newtype that promises an invariant enforce it at construction and
  through public fields? [M-STRONG-TYPES-GUARD][strong-types].
- Are external effects testable where failure modes otherwise cannot be
  exercised? [M-MOCKABLE-SYSCALLS][mockable]. Don't demand a mock layer when a
  real integration test covers the case.
- Are expected input failures reported rather than panicking, and actual
  broken internal invariants handled deliberately? [M-PANIC-IS-STOP][panic]
  and [M-PANIC-ON-BUG][panic-bug].
- Is a proposed hot-path optimization supported by profiling?
  [M-HOTPATH][hotpath].

For findings give `file:line`, the violated invariant or observable cost,
the applicable guideline ID/URL, and the smallest justified change.
Distinguish Microsoft's recommendations from Rust language requirements.
Its [golden rule][golden-rule] favors the rationale over literal compliance:
do not demand extra crates, builders, mocks, allocators, or type machinery
without a concrete issue. If no rule applies, return no findings.

[checklist]: https://microsoft.github.io/rust-guidelines/guidelines/checklist/
[strong-types]: https://microsoft.github.io/rust-guidelines/guidelines/libs/resilience/#M-STRONG-TYPES-GUARD
[mockable]: https://microsoft.github.io/rust-guidelines/guidelines/libs/resilience/#M-MOCKABLE-SYSCALLS
[panic]: https://microsoft.github.io/rust-guidelines/guidelines/correctness/#M-PANIC-IS-STOP
[panic-bug]: https://microsoft.github.io/rust-guidelines/guidelines/correctness/#M-PANIC-ON-BUG
[hotpath]: https://microsoft.github.io/rust-guidelines/guidelines/performance/#M-HOTPATH
[golden-rule]: https://microsoft.github.io/rust-guidelines/#the-golden-rule
