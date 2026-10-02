# Selected Rust Reference rules for source-grounded claims

Use this when a review finding depends on a *language rule*, not for general
style. The Reference documents behavior; API preference alone is not a
language violation. These are selected high-value rules, not the whole
language. For a rule absent here, either gather trustworthy evidence
separately or state uncertainty—never cite an unexamined Reference section as
proof.

## Privacy and implementations

- [Visibility/privacy](https://doc.rust-lang.org/reference/visibility-and-privacy.html):
  visibility is constrained by the containing modules. `pub` on a member does
  not by itself make an inaccessible path usable from another crate.
  `pub(crate)` allows access anywhere in the current crate; `pub(super)` in
  the parent module; `pub(in path)` in the specified ancestor module. For API
  findings, trace *reachability* through modules/re-exports as well as the
  item's own visibility.
- [Implementations/coherence](https://doc.rust-lang.org/reference/items/implementations.html):
  inherent methods can only be added to a type defined in the same crate.
  Trait implementations are restricted by the orphan/coherence rules: a local
  trait or an appropriately local type is needed, and overlapping
  implementations are rejected. A local extension trait can add method syntax
  to a foreign type, but a foreign trait on an unrelated foreign type is not
  generally allowed; generic/covered-type details can be subtle, so check them
  before asserting a specific impl is legal.

## Representation and foreign calls

- [Type layout](https://doc.rust-lang.org/reference/type-layout.html): a
  default `repr(Rust)` struct does not guarantee C field order or padding
  layout. `repr(C)` gives a C-compatible representation for the *outer item*
  under the target's ABI; nested Rust-layout fields do not acquire C layout
  just because the outer struct does. `repr(packed)` can make a field
  misaligned; forming an ordinary reference to a packed field may therefore be
  invalid, so use an appropriate raw/unaligned access pattern. Do not promise
  a stable layout for an unconstrained Rust type merely because `size_of`
  happens to match on one build.
- [External
  blocks](https://doc.rust-lang.org/reference/items/external-blocks.html) and
  [unsafe keyword](https://doc.rust-lang.org/reference/unsafe-keyword.html):
  in the 2024 edition external blocks must be declared `unsafe extern`; the
  declaration asserts signatures are correct. Imported functions are unsafe to
  call unless individually declared `safe`, which itself needs a genuine
  guarantee. The ABI string controls calling convention; `"C"` and `"system"`
  can differ on targets such as 32-bit Windows. `"C-unwind"`/`"system-unwind"`
  have different unwinding semantics; check whether either side can unwind and
  whether the boundary permits it.
- [Panic and UB](https://doc.rust-lang.org/reference/panic.html): Rust can
  unwind or abort depending on configuration; no design may rely on catchable
  unwinding in a build that aborts. A wrong ABI or unwinding through an ABI
  not allowing it may be UB; an ordinary panic is not by itself UB. The [UB
  list](https://doc.rust-lang.org/reference/behavior-considered-undefined.html)
  includes data races, invalid values, invalid pointer access and broken
  aliasing, but is not exhaustive.

**Example:** A changed FFI function uses `extern "C" fn` for a foreign
callback documented to unwind. Do not report merely “FFI unsafe”; inspect the
declared ABI on both sides and actual unwind path. If it can cross a
non-unwinding ABI, identify that exact path and correct the ABI/contain the
unwind; if it cannot unwind, no issue is shown by the signature alone.

These links identify provenance. The summaries here are the locally usable
rules; contested corner cases still require fresh evidence rather than
extrapolation.
