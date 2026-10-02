# Resilience and correctness

## Invariants and repeatable external behavior

- `M-STRONG-TYPES`: use the appropriate existing type (`Path`/`PathBuf` for an
  OS path rather than a string) at the point it clarifies the operation. Not
  every public numeric argument needs a wrapper.
- `M-STRONG-TYPES-GUARD`: if a newtype asserts an invariant, private storage
  and checked construction must uphold it; a `pub` tuple field defeats a
  validating `new`. Parsing or conversion from unchecked input must be
  fallible (`TryFrom`/`FromStr`), not infallible `From`. Explicit panicking
  constructors may coexist when users knowingly opt into them.
- `M-MOCKABLE-SYSCALLS`: filesystem, network, clock, entropy, environment and
  other external effects produce failures nondeterministically. When crucial
  edge cases cannot be reproduced, accept an existing I/O core or provide a
  small testable boundary, rather than performing unreplaceable syscalls deep
  in business logic. A real integration test that covers the failure may
  remove the need for a mock. Do not mock predictable in-memory operations.
- `M-TEST-UTIL`: mock controllers or dangerous test-only bypasses visible to
  consumers belong behind a `test-util` feature so production builds cannot
  disable validation. `M-INTEGRATION-TESTS` favors `tests/` for tests
  exercising only a library's public API, keeping implementation modules
  focused.
- `M-BUILD-RESULT`: an interdependent builder validates at
  `build() -> Result<_, _>` instead of requiring a particular setter order or
  fallible setters. Individual fields can still have strong checked types.
- `M-AVOID-STATICS`: correctness-critical library `static` state is not
  necessarily unique across a process—multiple linked versions of a crate can
  have independent globals. Pass a shared handle from the owner if callers
  must agree. A local performance cache that tolerates duplication is
  different.
- `M-NO-GLOB-REEXPORTS`: explicit public re-exports make the exposed surface
  reviewable; platform-dependent forwarding of a single implementation module
  can justify a glob.

## Failure categories and unsafe contracts

- `M-PANIC-IS-STOP`: a panic is not ordinary upstream error signaling; release
  profiles may abort. Invalid user input such as a failed parse should return
  a usable error, not `unwrap` followed by `catch_unwind`.
- `M-PANIC-ON-BUG`, `M-PANIC-MESSAGE`: a detected broken internal invariant or
  caller contract is a programming error; a meaningful panic with relevant
  values may be more appropriate than an error no caller can recover from. A
  parse API, however, is inherently fallible—calling it with bad input is not
  automatically a bug. Useful panic messages aid diagnosis.
- `M-PANIC-CONTINUATION`: caught panics can leave logical state inconsistent
  even when no UB occurred. Do not assume all operations are safe to resume; a
  request boundary may catch a panic to finish other work and initiate
  restart, not normalize it as a routine error.
- `M-UNSAFE`, `M-UNSOUND`: unsafe needs a reason (sound abstraction, FFI,
  *benchmarked* optimization) and an explicit argument for its preconditions.
  A safe function that any safe caller can use to cause UB is unsound, however
  unusual the caller; `unsafe impl Send/Sync` cannot simply bypass trait
  bounds. The Rust-project skill's bundled unsafe reference contains the
  precise soundness lens.
- `M-UNSAFE-IMPLIES-UB`: marking a function `unsafe` denotes caller
  obligations whose violation can cause UB; a destructive but memory-safe
  operation is not thereby an `unsafe fn`.

## Diagnostics

- `M-LOG-NOT-PRINT`: use project telemetry for production-library diagnostics
  rather than `println!`/`dbg!`; stdout is appropriate for a CLI intentionally
  producing user output.
- `M-LOG-STRUCTURED`: record named fields and stable event identity rather
  than preformatting text if the logger supports it; defer formatting when
  practical and omit/redact credentials and sensitive paths. Do not replace an
  installed logging stack solely to comply with a particular syntax.

**Review example:** A library caches authorization state in a `static` under
`foo v1`, while a plugin linked with `foo v2` queries the same logical state.
Check whether version coexistence is possible and consistency is required; if
so pass one handle from the host. A metrics-only cache whose copies may
diverge is not the same problem.

Source provenance:
[resilience](https://microsoft.github.io/rust-guidelines/guidelines/libs/resilience/),
[correctness](https://microsoft.github.io/rust-guidelines/guidelines/correctness/),
[universal
logging](https://microsoft.github.io/rust-guidelines/guidelines/universal/).
Links are optional.
