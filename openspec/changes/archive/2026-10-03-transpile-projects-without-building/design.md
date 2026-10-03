# Design

## Context

See `proposal.md` and `specs/project-build/spec.md`. Currently `src/main.rs` routes `build` to `Project::build()`. In `src/project.rs`, `build()` generates `.blkit/Cargo.toml` and Rust sources via `generate()`, then invokes `cargo build`; `programs()` invokes `cargo metadata` when sources reference external task crates, and `update()` invokes `cargo update`. The generated library and optional worker/server entry point already form a Cargo project. `uplift-blkit-cli` is complete and its `cli-experience` spec is synced; the CLI now uses structured argument parsing and presentation while preserving `build` until this change is applied.

## Goals / Non-Goals

**Goals:** Expose generation as a complete CLI operation independent of compilation, retain dependency resolution and explicit lock updates, and let users build the generated project wherever they operate a Rust toolchain.

**Non-Goals:** Bundle Cargo, remove Cargo metadata resolution for custom tasks, change `blkit.toml` target values or the generated runtime, or make compilation failures detectable at transpilation time.

## Decisions

1. **Replace the project command instead of adding an alias.** Expose `transpile [PROJECT_DIR]` in the post-uplift CLI parser, help, and completions; remove `build`. Keep `update [PROJECT_DIR]` and direct `SOURCE.bl OUTPUT.rs`. Rename the public project operation to `Project::transpile()` and reuse the existing `generate()` path, dropping only the final `cargo build` execution. Alternative: keep a `build` alias or keep `Project::build()` compiling; rejected because the command/API would keep implying or performing compilation.

2. **Retain generated Cargo project structure.** Continue writing `.blkit/Cargo.toml`, `src/lib.rs`, scope modules and, for worker/server, the selected `src/bin/*` entry point. Transpilation succeeds after validation and generation, with no `target/` requirement or binary success claim. The user runs `cargo build --manifest-path .blkit/Cargo.toml` locally or after transferring the project with its referenced local dependencies to the build environment. Alternative: introduce a new output layout or generate a standalone Rust file; rejected because `.blkit` already is a complete Cargo project.

3. **Keep Cargo for dependency work, not compilation.** Referenced external tasks still use `cargo metadata` to locate task descriptors, honoring `.blkit/Cargo.lock` when present; `blkit update` still runs `cargo update`. Built-in-only transpilation needs no Cargo. An existing lockfile can be committed and reused; when none exists, dependency resolution or the user's eventual Cargo build may create it. Missing Cargo and unresolved extension errors remain actionable. Rust callable compatibility is verified by the user's Cargo build, not inferred by transpilation. Alternative: bundle Cargo or implement registry resolution ourselves; rejected as additional toolchain maintenance and duplicated Cargo behavior.

4. **Adjust tests at the boundary.** Change project tests that previously relied on `Project::build()` to call transpilation and then explicitly run Cargo only when verifying executable/link behavior, lockfile behavior, or runtime behavior. Add a test that transpilation alone creates sources/manifest but never produces `target/` in a fresh isolated project; use a fake Cargo on PATH for an extension-bearing test to distinguish permitted `metadata`/`update` from forbidden `build`. Update CLI tests for `transpile`, removed `build`, and the uplift's help/completion/progress behavior. Preserve tests for cross-file validation and direct single-file transpilation.

## Risks / Trade-offs

- [A generated project can fail during a later user-run Cargo build] → Document the separate failure boundary; keep explicit Cargo-build integration tests for generated crate, worker, server, and extension callables without making compilation part of the transpilation command.
- [Moving `.blkit` to another machine can break local path dependencies or the generated blkit source path] → Document that the build environment must have the referenced local dependencies and an available blkit crate; do not promise a self-contained portable directory.
- [Old clients use `blkit build` or `Project::build()`] → Treat command and library rename as breaking, document migration, and reject the old command rather than silently changing its semantics.

## Migration Plan

With `uplift-blkit-cli` complete, replace uses of `blkit build [PROJECT_DIR]` with `blkit transpile [PROJECT_DIR]`, followed by a user-run `cargo build --manifest-path PROJECT_DIR/.blkit/Cargo.toml` when a compiled artifact is wanted; update library callers to `Project::transpile()` and run Cargo themselves. Keep `blkit update` for dependency refresh. No project manifest migration is required. Rollback restores the old CLI/library operation and its embedded Cargo build behavior.
