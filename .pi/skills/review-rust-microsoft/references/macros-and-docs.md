# Macros, documentation and applications

## Macros have an observable contract

- `M-MACRO-LAST-RESORT`, `M-EXAMPLE-OVER-PROC`: write ordinary
  functions/traits first. For genuinely repetitive syntax, a `macro_rules!`
  macro is usually easier to inspect and compile than a proc macro when it can
  express the same operation. A macro should make its expansion unsurprising.
- `M-MACROS-DONT-LIE`: a macro should not secretly change the kind of item or
  function signature (e.g. make a plain `fn` async and require a hidden
  parameter). The generated behavior should correspond to what a reader sees
  at the invocation.
- `M-MACRO-HELPERS`, `M-MACRO-MAIN-CRATE`: expansion paths must resolve in the
  caller's crate without making callers add unadvertised dependencies. A
  library can re-export necessary external items through a hidden `_private`
  module and generate fully qualified paths through its main crate. Check
  renaming/namespace assumptions according to the supported public API, not
  speculative environments.
- `M-MACRO-VERSION-PIN`: a crate and its companion proc-macro crate should use
  an exact version pairing when expansion refers to that crate's internals;
  otherwise compatible-looking semver ranges can pair incompatible generated
  code. This is distinct from an independently consumed macro crate.
- `M-PROC-IMPLIED-ITEMS`: generated public types/names that aren't apparent in
  source can collide with user names or break re-exports. Prefer explicit,
  predictable emitted items. `M-PROC-IMPL` favors placing substantial
  transformation logic/tests in an ordinary implementation crate where it is
  easier to exercise; don't split a trivial macro merely for the sake of a
  pattern.

## Documentation and app-specific decisions

- `M-FIRST-DOC-SENTENCE`, `M-MODULE-DOCS`: a public doc comment should begin
  with a concise sentence describing the item, while meaningful module docs
  explain the subsystem and its usage. A code listing without behavior or
  constraints is not much help.
- `M-CANONICAL-DOCS`: important behavior, error, panic and safety contracts
  should be findable where users look in rustdoc. Organize complex docs around
  actual usage rather than creating boilerplate sections for every item.
- `M-APP-ERROR`: an application may use an opaque reporting error (e.g.
  anyhow-like) at its outer boundary; a library with callers needing recovery
  should retain useful typed distinctions. Don't require the same error
  representation in both layers.
- `M-FFI-TRANSLATES`: an FFI boundary should translate between Rust and
  foreign representations; putting all business logic in FFI glue makes it
  harder to test and reuse. Apply only where the changed code actually crosses
  an FFI boundary.

**Review example:** A proc macro emits `impl ::third_party::Trait for Item`,
but a user depending only on the advertised crate cannot compile because it
does not directly depend on `third_party`. If the public crate owns this
integration, route the emitted path through its hidden re-export; check the
expansion and downstream build before filing a finding.

Source provenance:
[macros](https://microsoft.github.io/rust-guidelines/guidelines/macros/),
[documentation](https://microsoft.github.io/rust-guidelines/guidelines/docs/),
[applications](https://microsoft.github.io/rust-guidelines/guidelines/apps/),
[FFI](https://microsoft.github.io/rust-guidelines/guidelines/ffi/). Links are
optional.
