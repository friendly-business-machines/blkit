---
name: review-rust-project
description: Use when reviewing Rust changes against Rust-project API, language, or unsafe-code guidance.
---

# Rust Project Code Review

Review the changed Rust code, read-only. This is a **source-focused** review,
not a second review of change requirements, test coverage, Clippy output, or
formatting.

- For any changed public function, type, or trait (`pub`), read
  [references/api.md](references/api.md) and inspect its callers and
  surrounding types.
- When a finding depends on a *language rule*, consult the relevant section
  of the [Rust Reference](https://doc.rust-lang.org/reference/) before
  asserting it. The Reference is not a general style checklist.
- Independently, when the change contains `unsafe`, `unsafe impl`, or an
  unsafe abstraction, read [references/unsafe.md](references/unsafe.md).
  Otherwise do not load the Rustonomicon. A change can require both reference
  files.

For each actionable finding, report `file:line`, the observed behavior, its
consequence, a minimal correction, and the exact applicable source URL/section.
Distinguish a guideline recommendation from a language or soundness
requirement. Do not claim a source supports a finding unless you checked it;
if you cannot check a disputed rule, state the uncertainty. If no Rust code or
no applicable source-grounded issues are found, say so. Do not propose
unrelated redesigns.
