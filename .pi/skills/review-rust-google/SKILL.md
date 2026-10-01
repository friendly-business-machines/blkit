---
name: review-rust-google
description: Use when reviewing Rust changes for idiomatic API, ownership, type-system, and polymorphism choices using Google's Comprehensive Rust course.
---

# Google Idiomatic Rust Code Review

Review the changed Rust code, read-only, through
[Comprehensive Rust: Idiomatic Rust][course]. It is an opinionated course
under active development, **not** a normative checklist. This pass covers
design judgments a formatter or Clippy run cannot settle; avoid duplicating
the other source-focused reviews.

Inspect surrounding callers before suggesting changes:

- [Foundations of API design][api]: Is the call site clear and is the public
  behavior predictable?
- [Don't fight the borrow checker][borrowing]: Does an API hide an unnecessary
  clone or obscure whether a value is borrowed or owned? Confirm that a borrow
  serves actual callers and assess compatibility before changing a public
  return type.
- [Leveraging the type system][types]: Would a type enforce an actual domain
  invariant, or would typestate/newtypes merely add complexity?
- [Polymorphism][polymorphism]: Is the current choice of enum, trait, or
  generic the simplest one for real use cases?

For each finding report `file:line`, concrete caller impact, minimal change,
and the relevant course section URL. Present course advice as a contextual
suggestion, not a language rule or a demand for advanced patterns. If no
idiomatic issue is evidenced, return no findings.

[course]: https://google.github.io/comprehensive-rust/idiomatic/welcome.html
[api]: https://google.github.io/comprehensive-rust/idiomatic/welcome.html#foundations-of-api-design
[borrowing]: https://google.github.io/comprehensive-rust/idiomatic/welcome.html#dont-fight-the-borrow-checker
[types]: https://google.github.io/comprehensive-rust/idiomatic/leveraging-the-type-system.html
[polymorphism]: https://google.github.io/comprehensive-rust/idiomatic/polymorphism.html
