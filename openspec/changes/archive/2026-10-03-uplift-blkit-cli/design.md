# Design

## Context

See `proposal.md` for motivation and `specs/cli-experience/spec.md` for the user-facing contract. `src/main.rs` dispatches on raw arguments; `Project::load/build/update` and `blkit::transpile` return `Result<_, String>`. Project builds spawn Cargo and capture its output on failure. `tests/cli.rs` invokes the real binary; `tests/project.rs` exercises target generation and lockfile behavior. The existing `project-build` contract preserves the positional `blkit SOURCE.bl OUTPUT.rs` form.

## Goals / Non-Goals

**Goals:** Confine presentation and argument parsing to the CLI where possible; preserve the existing library and generated-crate behavior. Use all six requested dependencies for distinct, real CLI concerns.

**Non-Goals:** New subcommands, compiler span tracking, changing public library error signatures, progress instrumentation inside the compiler, changes to project target packaging.

## Decisions

1. **Use `clap` for the existing command shape.** Represent `build [PROJECT_DIR]` and `update [PROJECT_DIR]` as subcommands and the legacy source/output pair as top-level positionals; enforce the pair-or-subcommand shape without accepting ambiguous mixed inputs. Use generated help and version. Use `clap_complete`'s shell parser/generator behind a top-level `--completions SHELL` option; completion exits before any I/O on source/project files. Alternative: replace the direct form with a `compile` subcommand, rejected because it breaks the existing invocation.

2. **Adapt errors at the executable boundary.** Keep `Result<_, String>` inside the compiler and project APIs. Define a small CLI error type with `thiserror`, wrap it in `miette` for human-facing contextual reports, and include original parse/validation/Cargo details. Attach filenames/operation context at the boundary; do not fabricate spans for parsers that return strings. Let `clap` own usage errors and exit behavior. Alternative: change every compiler error to a structured diagnostic, rejected as a separate language/compiler project.

3. **Separate human feedback from machine output.** Use `console` for styled terminal-only status text and `indicatif` for one transient spinner around each project operation, on stderr only. Disable both interactive decoration and animation when stderr is not a TTY or styling is disabled (including `NO_COLOR`); clear the spinner before emitting an error. Keep completion stdout unpolluted. Preserve full Cargo error output already included by `Project::build/update`, and avoid adding a fine-grained progress-event API until there are meaningful events to show. Alternative: add phase callbacks to `Project`, rejected as unnecessary for a CLI-only uplift.

4. **Verify at the binary boundary.** Extend `tests/cli.rs` for legacy syntax, help/version, completion, failure context and exit codes, non-TTY clean stderr, and bad argument combinations; rely on `tests/project.rs` for build-target and lockfile regressions. Document user-visible options in README. No new CLI testing dependency is required.

## Risks / Trade-offs

- [Top-level positionals and subcommands can conflict in a parser] → Test all three accepted forms plus partial/mixed/extra operands, preserving the legacy error behavior where possible.
- [Six crates increase dependency footprint for presentation only] → Limit use to the executable, avoid pulling them into generated Cargo manifests, and avoid additional wrappers or progress APIs.
- [Formatted diagnostics could hide underlying errors or emit escape codes in CI] → Preserve original messages and Cargo failures; test redirected stderr and `NO_COLOR` explicitly.
- [Build/update are blocking operations] → One spinner around the call provides feedback without changing `Project` internals; no misleading percentage or phase estimates.

## Migration Plan

No manifest or source migration. The old invocations continue working; help/output presentation becomes richer. If issues arise, revert the CLI entry point and its six dependencies without migrating project files or generated artifacts.
