# Proposal

## Why

The current root Cargo package mixes the distributable `blkit` CLI, transpiler library, and repository-only API/worker demos. Its build script compiles the example process even when building only the CLI, and the shared package boundary obscures which code is shipped.

## What Changes

- Make the root a workspace-only manifest, with separate `crates/blkit-cli`, `crates/blkit-transpiler`, `crates/blkit-core`, and `tools/dev-harness` packages.
- Keep `blkit` as the only distributable repository executable. The CLI and transpiler packages are private (`publish = false`); generated user projects continue to depend on `blkit-core`, not the CLI or transpiler.
- Move the example graph build script and repository-only demo API/worker/server binaries into the dev harness, so a CLI-only build does not compile the example or build the demo targets.
- Move and update tests, fixtures, project-generation paths, manifest dependencies, and documentation for the new layout while preserving the CLI and generated-project behavior.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `cli-experience`: Require an independently buildable/distributable CLI whose build excludes the repository demo harness, without changing supported invocations.

## Impact

Cargo workspace and package manifests, `src/`, `build.rs`, `src/bin/`, tests and examples, project-generation core-path discovery, and documented developer build commands. No changes to the `.bl` language, generated process/runtime contract, or user-visible CLI commands are intended. Full workspace tests still include the dev harness.
