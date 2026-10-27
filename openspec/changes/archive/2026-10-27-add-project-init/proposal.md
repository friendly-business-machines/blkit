# Proposal

## Why

Starting a blkit project currently requires copying a manifest and valid `.bl` source from the README before transpilation can succeed. A project initializer should provide a small standalone starting point without requiring Rust build tools on the authoring machine.

## What Changes

- Add `blkit init [PROJECT_DIR]`, defaulting to the current directory and creating a missing destination directory.
- Create `blkit.toml` using the destination directory's name and the invoked CLI version, with `build_target = "api-worker"` and `persistence = "local"`.
- Include one minimal valid `main.bl` so an otherwise empty project is ready for `blkit transpile`.
- Refuse to overwrite existing scaffold files and preserve unrelated files and ignore rules.
- Default to Git initialization outside an existing Git working tree; support the boolean `--no-vcs` opt-out, avoid nested repositories, and never create a commit.
- Probe `cargo --version` and `rustc --version`; warn when either is missing or unusable without blocking otherwise successful initialization.
- Print explicit transpile, Cargo build, and standalone run instructions. Do not invoke Cargo initialization, dependency resolution, compilation, tool installation, database preparation, or the generated application.
- Expose the command and its boolean `--no-vcs` flag in help, shell completion, and the README.

## Capabilities

### New Capabilities

None; project initialization extends the existing CLI capability.

### Modified Capabilities

- `cli-experience`: Add safe standalone project initialization, opt-out Git setup, advisory Rust toolchain checks, and discoverable next steps while retaining existing command behavior.

## Impact

- `crates/blkit-cli/src/main.rs`: Clap command definition, dispatch, diagnostics, and next-step output; a small CLI-local initializer module may hold filesystem and subprocess operations.
- `crates/blkit-cli/tests/cli.rs` and initializer-focused integration tests: command discovery, filesystem safety, Git behavior, subprocess checks, and initialized-project transpilation/build coverage.
- `README.md`: Replace manual-only onboarding with the initialization workflow while retaining the documented manifest format and manual setup option.
- No changes to the project manifest schema, transpiler target semantics, generated-runtime API, database schemas, or dependency graph are intended. Use the standard library and installed CLI dependencies; Git is required only when a new repository must be created. Cargo and rustc remain optional for initialization.
