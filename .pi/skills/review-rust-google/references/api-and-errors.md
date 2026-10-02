# Foundations of API design and error handling

The course prioritizes predictable behavior **at the call site**: someone
reading a call should understand what happens without opening the definition.

## Names and the standard-library vocabulary

- Prefer familiar names and conventions (`push`, `is_empty`) over a second
  dialect (`push_back`, `empty`) when behavior is equivalent. Check local
  consistency and whether changing a public name would break users.
- Reach for the existing std vocabulary of collection, conversion and I/O
  types/traits before inventing a parallel API. Predictability comes from what
  callers already know, not from implementing every trait indiscriminately.
- Make ownership and failure visible: a caller should not discover a hidden
  allocation, implicit clone, unexpected panic, or side effect only after
  inspecting an implementation. Look at *actual* uses, not a hypothetical
  style ideal.

## Documentation that teaches the contract

Useful rustdoc describes semantics a signature cannot: accepted input, output,
ownership implications, failure conditions, important side effects and a
realistic usage example. A comment that only restates the function name adds
no information. Don't demand verbose docs for obvious private helpers; note a
finding when a public behavior or caller constraint is otherwise unclear.

## Errors: absence, recovery, reporting

- Use `Option<T>` for ordinary absence without a failure reason; `Result<T,E>`
  when the caller needs to distinguish or communicate a failure. A panic is
  not an alternative to a recoverable input error.
- Scope error types to the consumers that handle them. A specific
  library/domain error enables a caller to match on recoverable cases; at an
  application/reporting boundary, added context and an opaque report can be
  more useful than a deep enum exposed everywhere. Avoid promoting incidental
  dependency errors to a stable public contract accidentally.
- When an error crosses a boundary (e.g. reading bytes → parsing a config →
  starting a service), add the operation and relevant path/identifier so the
  report says *what failed*. Preserve the cause through
  `std::error::Error::source` where applicable; don't replace every error with
  an uninformative string.
- If fatal transport failure and a recoverable per-item rejection are both
  genuine outcomes, model the distinction explicitly; the course illustrates
  `Result<Result<T, RecoverableError>, FatalError>`. This is an example,
  **not** a demand to nest every `Result`.
- The course mentions `thiserror` to reduce boilerplate for typed errors and
  `anyhow` for reporting. Neither crate is mandatory: follow dependencies
  already in use and choose by the needs of real callers.

**Review example:** A library returns `Option<Config>` for both “config not
found” and “config malformed.” If callers need to repair malformed input, use
an error-bearing path for parse failure and reserve absence for the missing
file. Check whether callers deliberately treat both the same before changing a
public return type.

Source provenance: [course overview: foundations and error
handling](https://google.github.io/comprehensive-rust/idiomatic/welcome.html),
[foundations](https://google.github.io/comprehensive-rust/idiomatic/foundations-api-design.html).
This file contains the usable guidance; links are not required steps.
