# Unsafe-code questions

Read the applicable [Rustonomicon][nomicon] section, especially
[Send and Sync][send-sync] for `unsafe impl` and
[safe/unsafe boundaries][boundaries] for safe wrappers. Consult the
[Rust Reference on undefined behavior][ub] for precise language claims.
The Nomicon warns that some material may be incomplete or outdated; check
disagreements against the Reference.

For each changed unsafe operation, identify the invariant that makes it sound,
who establishes it, and whether safe callers can violate it. Do not infer
unsoundness from the presence of a raw pointer alone: trace access and
ownership. For `unsafe impl Send` or `Sync`, check whether moving or sharing
the actual type across threads can expose unsynchronized mutation. If proof
requires code outside the diff, inspect it or state what remains unverified.
Report a concrete counterexample or missing proof, not a blanket ban on
`unsafe`.

[nomicon]: https://doc.rust-lang.org/nomicon/
[send-sync]: https://doc.rust-lang.org/nomicon/send-and-sync.html
[boundaries]: https://doc.rust-lang.org/nomicon/safe-unsafe-meaning.html
[ub]: https://doc.rust-lang.org/reference/behavior-considered-undefined.html
