# Design

## Context

The root `blkit-transpiler` package currently contains `src/lib.rs`, `src/main.rs`, three `src/bin/` executables, and `build.rs`. That build script compiles `examples/graph.bl` for the demo binaries on every package build, including a selected `blkit` CLI build. The root manifest also enables all server/worker/storage features of `blkit-core` for the transpiler package. Tests in root `tests/` mix compiler, CLI, runtime, and process-launching concerns. `src/project.rs` and several tests derive the local core path from the old root `CARGO_MANIFEST_DIR`; generated projects must still pin the same `blkit-core` version and work outside the checkout when a matching core release is available.

## Goals / Non-Goals

**Goals:**
- Make CLI, transpiler, runtime, and development harness separate Cargo package boundaries with a workspace-only root and CLI-only default build.
- Keep `blkit` as the sole executable selected for distribution; avoid compiling the example process or demo executables when building it.
- Preserve CLI commands, generated project behavior and core dependency contract, demo executables, and full workspace test coverage.

**Non-Goals:**
- Change the `.bl` language, process runtime, generated-project target choices, or API protocols.
- Publish the private CLI/transpiler/harness packages to crates.io or require Cargo for ordinary source transpilation.
- Rework the core's feature model or create another implementation of the demo runtime.

## Decisions

### Package ownership and dependency direction

Use a root workspace-only `Cargo.toml` with members `crates/blkit-cli`, `crates/blkit-transpiler`, `crates/blkit-core`, and `tools/dev-harness`; set `default-members` to the CLI package. Move `src/main.rs` to the private `blkit-cli` package, retaining bin name `blkit`, and compiler, semantic, graph, decision, codegen, expression, and project modules to the private `blkit-transpiler` package, retaining the `blkit_transpiler` Rust library name. Keep `blkit-core` at its current path and available for generated projects. The CLI depends on the transpiler; the transpiler depends on core's minimal feature set; the harness depends on the transpiler for build-time compilation and core's runtime features for its binaries/tests. Keep package versions aligned so CLI version checking and generated core pins retain their meaning.

Alternative: retain the root package and gate demo binaries with features. This still invokes the root build script for CLI builds and leaves CLI/compiler ownership mixed. A single package with more module directories likewise changes appearance, not build isolation.

### Example and demo build ownership

Move `build.rs`, `blkit-dev`, `blkit-api`, `blkit-worker`, and their compiled `graph.bl` fixture into `tools/dev-harness`; keep the other documentation examples in the root `examples/` directory. The harness build script calls the transpiler crate as a build dependency rather than importing the compiler source files by `#[path]`. Only the harness generates `OUT_DIR/graph.rs`, and its bins include that output as before. The API/worker remain separately executable for manual use and process-level tests. Building the root default target or `cargo build -p blkit-cli --release --bin blkit` never selects the harness/build script. `cargo test --all --workspace -- --test-threads=1` deliberately still includes it.

Alternative: ship demo code in generated projects only and delete the repo binaries. This would break existing manual and process-level testing without improving the requested package boundary.

### Test and dependency migration

Move tests to packages according to what they actually exercise: CLI and CLI-invoking project tests to `blkit-cli`, parser/generator/validation tests to `blkit-transpiler`, core runtime/temporal/store tests to `blkit-core`, and API/worker process-launching and cross-component HTTP tests to `dev-harness`. Keep `CARGO_BIN_EXE_blkit` and `CARGO_BIN_EXE_blkit-{api,worker}` consumers in the same packages as their corresponding binaries. Update fixture `include_str!` paths and generated temporary-project core paths from each new manifest location. Put test-only dependencies on the packages whose tests use them; the CLI production dependency graph must not enable core's API, worker, or persistence features. Tests that compile nested generated projects still use the real core crate and must not rely on the old root being a package.

### Generated-project core location

Update local checkout detection in the relocated transpiler's `project.rs` to use the actual sibling `../blkit-core` manifest path. In a checkout, generated manifests still use a matching exact core version plus a valid local path; for a distributed CLI where the build-time checkout path is absent, they still request the matching core release without a path. This preserves the existing dependency contract and does not add a CLI/transpiler dependency to generated projects.

## Risks / Trade-offs

- [Moved manifest roots invalidate relative fixtures, test-generated Cargo manifests, and README paths] -> Update every reference and verify direct-source and all generated target builds, not only compiler unit tests.
- [Cargo features unify across selected workspace packages during full-suite runs] -> Verify CLI isolation with a CLI-only build rather than inferring it from `--workspace` tests; inspect selected build targets/features if needed.
- [The CLI artifact may embed a build-time checkout path used to locate core for project generation] -> Preserve existing fallback when that checkout path is unavailable; test both checkout and standalone CLI behavior without changing core version pinning.
- [Repackaging process-launching tests may inadvertently stop compiling the demo binaries] -> Keep those tests and their `CARGO_BIN_EXE_*` references together in the harness, and run the complete workspace suite.

## Migration Plan

Move sources and tests into their owning packages, replace the root manifest with a workspace-only manifest, and update dependency features and relative paths as one coherent migration. Verify the CLI-only distribution build, CLI and generated-project behavior, demo binaries, and full workspace tests; update README and developer commands. This changes only repository/build layout, not persisted data or externally supported CLI flags, so rollback is a revert of the package migration.
