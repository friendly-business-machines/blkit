# Proposal

## Why

The blkit CLI currently parses arguments and prints errors by hand. As its existing project commands mature, consistent help, diagnostics, completion, and interactive feedback would make them easier to use without changing what blkit builds.

## What Changes

- Replace handwritten argument parsing for `build`, `update`, and `SOURCE.bl OUTPUT.rs` with structured parsing and generated help/version text while retaining the existing invocations.
- Offer shell completion for the existing interface through an option, not a new subcommand.
- Provide readable, contextual errors and terminal-only styling/progress for long project operations; keep redirected output clean and failures nonzero.
- Add the requested crates `clap`, `clap_complete`, `console`, `indicatif`, `miette`, and `thiserror` for their respective CLI roles. Do not add new commands or change project manifests, generated targets, or `.bl` language semantics.

## Capabilities

### New Capabilities

- `cli-experience`: Help, completion, diagnostics, and terminal feedback for existing blkit CLI invocations.

### Modified Capabilities

None. Existing `project-build` requirements, including direct single-file transpilation and all three build targets, remain in force.

## Impact

- CLI entry point (`src/main.rs`), and potentially the project command boundary in `src/project.rs` to expose progress without changing its public outcomes; CLI tests and README usage.
- Cargo dependencies/lockfile gain the six requested crates; generated project and runtime APIs remain unchanged.
