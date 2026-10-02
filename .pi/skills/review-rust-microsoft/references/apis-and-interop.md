# Universal, interoperability and UX guidance

Use these *Pragmatic Rust Guidelines* only for changed behavior and
demonstrable caller impact. `M-UPSTREAM-GUIDELINES` defers to Rust-project
API/style/soundness guidance; the separate Rust-project review covers that
material.

## Universal choices

- `M-PUBLIC-DEBUG`, `M-PUBLIC-DISPLAY`: public types should be diagnosable
  with `Debug`; user-readable values (especially errors) benefit from
  `Display`. Custom formatting must redact secrets; deriving `Debug` on a
  credential holder may leak them. Don't turn a non-displayable internal
  marker into prose without a need.
- `M-WEASEL-WORDS`, `M-SHORT-NAMES`, `M-REGULAR-FN`: use names that say the
  specific job rather than `Manager`/`Factory` by default; context in the
  module can avoid repetitive prefixes. Prefer a module-level function for
  unrelated computation over a type's associated function. Check existing API
  compatibility before renaming.
- `M-DOCUMENTED-MAGIC`: explain why a timeout/limit/threshold was chosen and
  what changes if it moves. A named constant with rationale beats a comment
  merely restating the numeric value.
- `M-LINT-OVERRIDE-EXPECT`: a narrowly scoped `#[expect(..., reason = "...")]`
  surfaces when the warning no longer occurs; `#[allow]` can still fit
  generated code. `M-STATIC-VERIFICATION` recommends compiler lints, Clippy,
  formatting, and suitable dependency/unsafe checks, but these tools are
  separate from a judgment-based code review.

## Interoperability and stable public boundaries

- `M-TYPES-SEND`: public futures commonly need `Send` for multithreaded
  runtimes. If a future holds `Rc` (or another `!Send` value) across `.await`,
  it may become unusable by callers that spawn it. Do not demand `Send` on a
  type deliberately restricted to a local executor; check the target runtime.
- `M-DONT-LEAK-TYPES`, `M-FOREIGN-REEXPORTS`: a third-party type in a public
  signature ties the library's compatibility to that dependency; prefer stable
  std/core types where adequate. Ecosystem integration, relevant features and
  umbrella crates are legitimate exceptions. Avoid re-exporting arbitrary
  foreign types merely as aliases; re-export technical companion crates when
  that is the intended public path.
- `M-ESCAPE-HATCHES`: a safe native-handle wrapper may need a documented
  `unsafe from_native` for interop that cannot satisfy its invariants
  automatically, plus clear ownership-transfer/accessors. Do not remove safety
  obligations just to make FFI convenient.
- `M-IMPL-ASREF`, `M-IMPL-RANGEBOUNDS`, `M-IMPL-IO`: a borrowed, one-shot
  input may accept `impl AsRef<Path>` or `impl Read` rather than force a
  `PathBuf` or `File`; ranges can accept `RangeBounds` if partial/unbounded
  ranges make sense. Prefer a concrete type when ownership, inference,
  performance or callers warrant it. Generic input bounds need not infect
  stored public types.

## Usable abstraction and error surfaces

- `M-SIMPLE-ABSTRACTIONS`, `M-AVOID-WRAPPERS`: avoid making users carry nested
  generics and `Arc<Mutex<Box<...>>>` when private implementation detail can
  be hidden behind a simple named public type. Public smart pointers are fine
  if sharing is the point or measured performance justifies them.
- `M-DI-HIERARCHY`: for an async dependency, start with a concrete type; if
  consumers genuinely supply different implementations, use a generic trait
  bound, then consider a trait object when runtime type erasure is required.
  An internal native/mock pair may be a private enum. Object safety, async
  method support and wrapper exposure matter; do not ban all trait objects.
- `M-ERRORS-CANONICAL-STRUCTS`, `M-FROM-ERROR`: the guidelines favor
  situation-specific error structs carrying a cause and a backtrace when
  useful. Repeated conversion of owned error types in each caller can be
  centralized with `From` and `?`; `map_err` remains right when adding local
  context or converting a foreign type. A full error framework/backtrace in
  every tiny app is not a useful review demand.
- `M-INIT-BUILDER`, `M-BUILD-RESULT`: a builder helps when optional inputs
  yield many construction permutations; simple types should keep simple
  constructors. If constraints cross fields, accept settings in setters and
  return `Result` at `build()` instead of making every setter fail. Validated
  field types can still enforce local invariants earlier.
- `M-SERVICES-CLONE`, `M-ESSENTIAL-FN-INHERENT`: a heavyweight reusable
  service may expose cheap shared-ownership cloning without making callers
  clone a hidden inner graph; ordinary owned values needn't become `Arc`s.
  Core functionality should remain discoverable as inherent methods rather
  than usable only when an extension trait is imported.
- `M-ASYNC-FN`, `M-COLLECTION-TRAITS`: an ordinary asynchronous operation can
  be `async fn` when both forms work; returning `impl Future` deliberately is
  appropriate for hot-future sizing and some trait use. A real collection
  should expose useful iterator/construction traits and truthful `size_hint`,
  not implement every possible adapter regardless of semantics.

**Review example:** A public `async fn serve(client: Rc<dyn Client>)` retained
across awaits may simultaneously constrain `Send` and force every caller to
know an implementation detail. Inspect actual callers: a concrete client,
generic bound, or internal enum could be simpler. If callers select
heterogeneous implementations dynamically or run only locally, the original
may be justified.

Source provenance:
[universal](https://microsoft.github.io/rust-guidelines/guidelines/universal/),
[interoperability](https://microsoft.github.io/rust-guidelines/guidelines/libs/interop/),
[library UX](https://microsoft.github.io/rust-guidelines/guidelines/libs/ux/).
The substance is above; URLs are optional.
