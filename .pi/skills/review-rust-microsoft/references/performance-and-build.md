# Performance and build guidance

## Performance is a measured trade-off

- `M-THROUGHPUT`, `M-HOTPATH`: for throughput-sensitive software, identify hot
  operations and profile CPU/allocation cost before trading clarity, safety or
  dependencies for speed. Batching can reduce per-item work, task-switching
  and contention; latency and scale constraints still matter. Do not file a
  speed finding just because a type uses `String`, `HashMap` or a lock.
- `M-MEM-REUSE`, `M-INITIAL-CAPACITY`: measured allocation churn in a repeated
  path can justify reusing a buffer (e.g. clear/re-fill a caller-owned value).
  If the final size is known, preallocate or prefer iterator `collect()` where
  its size hint already handles capacity. Avoid manual allocation tricks for
  cold paths.
- `M-BOX-DST`, `M-SHRINK-TO-FIT`: frequently instantiated, long-lived
  immutable sequences may save memory as `Box<[T]>`/`Box<str>` or by shrinking
  excess capacity after construction. Don't demand conversion for mutable or
  rarely instantiated values; a conversion can itself cost allocation/copying.
- `M-AVOID-INDIRECTION`, `M-FAST-HASHER`: nested `Arc`s can hurt locality on a
  hot read; a faster non-cryptographic hash function is only appropriate for
  trusted internal keys where collision attacks are irrelevant. Require
  hot-path evidence; default `HashMap` is safer for untrusted input.
- `M-LOG-OVERHEAD`: verbose or allocating telemetry inside a hot item loop may
  dominate throughput. Prefer a cheap batch event or sampled diagnostics where
  investigation still works.
- `M-YIELD-POINTS`: CPU-bound async work without `.await` can monopolize an
  executor; break lengthy processing into chunks with cooperative yields.
  Async I/O with natural await points need not add artificial yields; size of
  task and runtime matter.
- `M-ASYNC-STACK-SIZE`: an `async fn`'s parameters and locals held across
  `.await` form part of the future; very large or very many futures can
  consume significant space and incur copies when scheduled. Measure with
  `size_of_val`/profiling first. If material, shorten live ranges, process
  large inputs before creating the future, or explicitly return a future from
  a normal function; don't rewrite ordinary async code speculatively.

## Building crates people can use

- `M-OOBE`: libraries intended to be portable should build on supported
  targets without extra undeclared system tools, environment variables or
  network downloads by default. Generated sources can be shipped in the
  published crate; gate platform-only dependencies appropriately. Explicitly
  platform-specific crates are an exception.
- `M-SYS-CRATES`: for a native wrapper, the guidelines favor reproducible
  native build steps, verifiable bundled sources, and pregenerated bindings
  when possible. Don't require every consumer to have a global native
  toolchain or network access unless the target contract says so.
- `M-FEATURES-ADDITIVE`: Cargo unifies features across dependents. Turning on
  a feature must not remove a public item or disable another feature; feature
  `std` may add capabilities instead of a `no-std` feature that negates them.
  Dependent features should enable their prerequisites. Check relevant
  combinations/platforms, not just default features.
- `M-MSRV`, `M-LATEST-EDITION`, `M-CARGO-WORKSPACE`: new crates should target
  the latest stable edition when feasible; declare a library MSRV and raise it
  conservatively when features require it. Related crates can inherit shared
  dependency versions, metadata and lints from a workspace. Check existing
  compatibility commitments before recommending an edition/MSRV change.

**Review example:** A new feature `fast` uses `#[cfg(not(feature = "fast"))]`
to remove `pub fn safe_mode()`. Downstream dependencies can enable `fast` for
the entire graph and unexpectedly break another caller. Preserve the API or
move mutually exclusive choices to runtime/explicit separate crates. By
contrast, adding an optional new method behind a feature is additive.

Source provenance:
[performance](https://microsoft.github.io/rust-guidelines/guidelines/performance/),
[building](https://microsoft.github.io/rust-guidelines/guidelines/libs/building/),
[project](https://microsoft.github.io/rust-guidelines/guidelines/project/). No
browsing required.
