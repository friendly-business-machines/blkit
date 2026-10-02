# Types, invariants and polymorphism

## Distinguish values and enforce valid construction

A type alias (`type UserId = u64`) is interchangeable with `u64`; a tuple
newtype (`struct UserId(u64)`) is distinct and forwards no methods or
operators automatically. Give its field private visibility and expose only
constructors that establish its promised invariant. Implement comparison when
meaningful; arithmetic on identifiers usually is not. Parse unchecked input
into a validated type once, rather than asking every downstream caller to
revalidate. An extra wrapper is unjustified if no mix-up/invariant exists.

An **extension trait** adds methods to an existing foreign type when wrapping
every instance is the wrong ergonomics. Rust does not permit adding inherent
methods to a foreign type, and method-name collisions can require
disambiguation; check whether an ordinary helper function would be clearer. A
newtype is better when you need a distinct semantic value or enforced
invariant, not merely additional behavior.

## Encode resource and protocol obligations

- **RAII / `Drop`:** own a resource in a value whose destructor releases it on
  scope exit (including early returns; normally also on unwinding). Do not
  rely only on callers remembering `close()`. `Drop` cannot return `Result` or
  `await`; offer an explicit fallible/async close where a caller must see a
  cleanup error, and keep a safe fallback for an unclosed resource.
- **Scope guards / drop bombs:** a guard can complete/undo an action on
  leaving scope; a deliberate panic on an unfinished operation is a sharp
  edge, especially during another panic. Demand evidence that the invariant
  needs it; do not replace normal error handling with a panic.
- **Token types:** an unforgeable value proves that an earlier condition was
  met; an API can require the token for protected work. Keep its
  construction/fields private and check `Default`, deserialization or other
  conversions cannot mint it bypassing validation. A non-`Clone` token
  consumed by value can enforce single-use permission; if the permission must
  belong to a particular owner, check how a token is tied to that owner (e.g.
  a lifetime/brand), not merely that its type matches.
- **Typestate:** when legal operations depend on the current phase, give each
  phase a type; a transition consumes the old value and returns the new one.
  Then methods unavailable in the phase cannot be called, e.g. a serializer
  cannot `finish` until its open struct is closed. For dynamic state, many
  transitions, or values coming from untrusted input, a runtime check and
  `Result` may be clearer.
- **Borrow-enforced invariants:** a guard holding `&mut Resource` can prevent
  another use of the same resource until it is released;
  `OwnedFd`/`BorrowedFd` illustrate how an owned OS resource and a
  lifetime-bound view can prevent use after closure. Check the actual
  ownership and call graph before introducing lifetimes or `PhantomData` for
  an external handle.

## Choose the simplest polymorphism that fits

- Enum + `match`: use for a closed set of variants owned by this library;
  adding variants affects exhaustive matches.
- Trait + generic/`impl Trait` parameter: use when one concrete implementation
  is selected per call. Static dispatch enables specialization, but generic
  signatures and monomorphization can increase complexity or code size.
- Trait object (`&dyn Trait`, `Box<dyn Trait>`): use for a runtime-selected or
  heterogeneous set needing type erasure. Check indirection and object safety.
- Composition: combine capabilities with fields and delegation rather than
  mirroring an inheritance hierarchy in many traits.

A generic *trait parameter* allows distinct implementations for different
parameter types; an *associated type* pins one chosen output/related type per
trait implementation (as in `Iterator::Item`). Check if callers need multiple
`Trait<Other>` implementations or one intrinsic relationship. Macro generation
can reduce repetitive code when traits do not express it cleanly, but the
expanded API should remain understandable; don't invoke a macro for a couple
of straightforward implementations.

**Review example:** Two fixed backends stored as `Box<dyn Engine>` might be
simpler as `enum Engine { A(A), B(B) }`, *if* external users do not implement
`Engine`, runtime heterogeneity is not required, and dispatch code stays
small. This is not a blanket ban on trait objects.

Source provenance: [type-system
overview](https://google.github.io/comprehensive-rust/idiomatic/leveraging-the-type-system.html),
[newtypes](https://google.github.io/comprehensive-rust/idiomatic/leveraging-the-type-system/newtype-pattern.html),
[RAII](https://google.github.io/comprehensive-rust/idiomatic/leveraging-the-type-system/raii.html),
[tokens](https://google.github.io/comprehensive-rust/idiomatic/leveraging-the-type-system/token-types.html),
[typestate](https://google.github.io/comprehensive-rust/idiomatic/leveraging-the-type-system/typestate-pattern.html),
[polymorphism
overview](https://google.github.io/comprehensive-rust/idiomatic/welcome.html).
No source browsing is necessary for these review questions.
