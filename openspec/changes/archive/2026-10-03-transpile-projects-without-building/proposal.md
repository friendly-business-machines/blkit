# Proposal

## Why

A blkit project currently invokes Cargo to compile the generated crate or binary as part of `blkit build`. This couples transpilation to local compilation even when the user intends to build the Rust project elsewhere. The CLI should produce a Rust project; the user should choose when and where to compile it.

## What Changes

- **BREAKING**: Replace `blkit build [PROJECT_DIR]` with `blkit transpile [PROJECT_DIR]`; no `build` alias is planned. The command writes a Cargo project in `.blkit/` for the selected `crate`, `server`, or `worker` target, but does not run `cargo build` or promise a compiled artifact.
- Keep Cargo dependency resolution for custom task crates and `blkit update` for explicit lockfile updates. Cargo must be installed where such projects are transpiled; compiling the generated project is a separate, user-run Cargo step, locally or elsewhere.
- Preserve the direct `blkit SOURCE.bl OUTPUT.rs` invocation and the existing target selection, generated runtime behavior, and extension validation where possible.
- Adapt the completed CLI uplift's command help, completion, progress, diagnostics, tests, and docs to the new operation.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `project-build`: Separate project transpilation from binary/library compilation and document the required Cargo environment and CLI invocation.
- `cli-experience`: Replace the `build` command contract and interactive feedback with `transpile` while preserving the other CLI operations.

## Impact

`src/main.rs`, `src/project.rs`, `tests/cli.rs`, `tests/project.rs`, `README.md`, and the existing project-build specification. The completed CLI uplift's `cli-experience` contract is updated by this change. No new dependencies or runtime architecture are intended.
