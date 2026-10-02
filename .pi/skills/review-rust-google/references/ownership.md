# Don't fight the borrow checker

The course treats ownership as a design constraint to make explicit, not an
obstacle to conceal with clones and shared ownership. Trace each value from
creation through its borrowers, mutation and drop.

## Owned/view pairs and honest costs

- `String` / `&str`: borrow to inspect text; own it to retain or modify
  independently.
- `PathBuf` / `&Path`: accept borrowed paths when only using them during the
  call; own them when storing.
- `Vec<T>` / `&[T]` / `&mut [T]`: a slice communicates read or exclusive
  mutation without forcing a growable collection.

A helper that consumes `String` merely to read it can make every borrowed
caller allocate. Conversely, a method retaining a value cannot safely return a
borrow into temporary input. Check caller constraints and public compatibility
before changing ownership. Hidden `.clone()` can disguise an expensive copy;
make the cost explicit or avoid it if ownership is unnecessary. `Cow<'a, T>`
can represent either a borrowed value or an owned replacement when
mutation/normalization is occasional; do not introduce it for a consistently
owned result.

## Model the ownership graph

- A tree of ownership, with borrowing for transient access, usually makes
  destruction and lifetimes predictable. Split types if one part owns
  long-lived data while another merely views it; don't put borrowed and owning
  modes in a single complicated structure without a caller need.
- A graph with genuine shared owners can use reference counting (`Rc` for one
  thread, `Arc` when sharing across threads). Strong back-edges can form
  cycles that keep memory alive; use weak links for non-owning back-references
  when they solve the actual lifecycle problem.
- Stable indices/handles into an owner-managed collection can avoid
  self-referential borrows and shared ownership. Check deletion, reuse,
  relocation and generation/staleness before recommending indices: an index is
  not a magically valid reference.
- A type holding `&'a T` borrows from a separate owner; the lifetime describes
  the constraint, not how long the data will live. Don't request `'static`,
  leak memory or clone blindly to silence borrow errors. Inspect when the
  owner drops and whether an owned field is the simpler model.

## Mutation and borrowing as invariants

`&mut T` gives exclusive access while the borrow is live; a transaction or
temporary guard can use this to exclude conflicting calls until it ends.
`Cell`/`RefCell` enable interior mutation behind a shared reference when
ordinary borrowing is too restrictive; `RefCell` checks conflicting borrows
*at runtime* and can panic, and is not thread synchronization. Compare a
narrower `&mut` borrowing scope or an owned return before introducing interior
mutability. Sharing a mutable resource across threads requires separate
synchronization.

**Review example:** A registry stores `Rc<Node>` in each node's parent and
child fields. If parents already own children, inspect whether a child only
needs a weak link or a parent-held ID; avoid proposing `Rc` everywhere just to
bypass a borrow error. Verify actual traversal and removal behavior first.

Source provenance: [course overview: borrow
checker](https://google.github.io/comprehensive-rust/idiomatic/welcome.html).
Choices about weak links and handle invalidation are Rust design consequences
of the course's listed strategies, not additional course mandates.
