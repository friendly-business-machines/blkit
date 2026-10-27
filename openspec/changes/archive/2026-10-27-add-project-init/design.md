# Design

## Context

See `proposal.md` for the motivation and `specs/cli-experience/spec.md` for the behavior contract. A design is warranted because safe filesystem creation, Git subprocesses, toolchain probes, and command dispatch need explicit failure boundaries.

Observed implementation constraints:

- `crates/blkit-cli/src/main.rs` owns Clap parsing, generated completions, operation dispatch, and contextual plain-text diagnostics. Help/completion exit before executing an operation. Invalid arguments are deliberately redacted for database credential safety; preserve that behavior.
- `crates/blkit-transpiler/src/project.rs::Project::load` reads `[project]`, checks the CLI version exactly, accepts `api-worker` only with an explicit persistence setting, and requires a `.bl` source for this target.
- The existing generator already creates the local API/worker executable and Cargo package under `.blkit`, using the project name with hyphens replaced by underscores for Rust imports. `prepare_manifest` writes `.blkit/.gitignore`, allowing `.gitignore` and `Cargo.lock` while ignoring other generated content.
- Local binaries already default to `blkit.db`, 32 concurrent tasks, and `127.0.0.1:3000`, without PostgreSQL preparation or cloud storage credentials.
- Existing CLI tests execute `CARGO_BIN_EXE_blkit` in temporary directories with child-local `PATH` overrides. There is no existing initializer or VCS helper to reuse, and no explicit minimum Rust version to enforce.

## Goals / Non-Goals

**Goals:**

- Produce inputs accepted by the existing local executable target rather than introduce a second project layout.
- Protect user files and make failures distinguish required setup from optional build-tool availability.
- Keep subprocess and filesystem behavior testable with the standard library and current dependencies.

**Non-Goals:**

- A template catalog, interactive wizard, target-selection flags, project-name override, or additional VCS implementations.
- Cargo scaffolding, lockfile generation, compilation, a new build/run command, dependency installation, or `blkit doctor`.
- Changing global Git configuration, creating commits, rewriting existing ignore files, or preparing remote services.
- Expanding compiler compatibility validation or changing `Project::load` acceptance rules.

## Decisions

### 1. Keep initialization CLI-local and separate from transpilation

Add an `init` operation with an optional destination and a boolean `--no-vcs` flag. Git is the only supported VCS and is enabled by default; the flag skips it entirely. Do not add a value-taking VCS selector or shell-specific value-completion code. A small private module in `crates/blkit-cli/src/` can implement the filesystem/subprocess work; wire it into the existing `CliError` flow rather than add a new error framework or public library API. Help and completion come from the same Clap definitions.

Do not call `Project::load` before creation: it requires files that do not yet exist. Validate the generated fixture through the real loader/transpiler in tests. Do not call the generator during initialization: `.blkit` remains the output of a separate explicit `blkit transpile`.

Alternative rejected: `cargo init` would introduce a root Cargo manifest and Rust entry point unrelated to `.bl` authoring, require Cargo prematurely, and compete with existing generated output.

### 2. Use a minimal static starter and a directory-derived package name

The manifest contains only `[project]` with `name`, `blkit`, `build_target`, and `persistence`. Use the CLI's compile-time package version rather than a hard-coded release. Resolve the effective destination so `.` and trailing path separators yield its actual directory name. For a missing path, use its intended final component and resolve existing ancestors as needed; validate before writing scaffold files.

**Proposed implementation defaults:** retain a valid directory-derived name exactly, including hyphens. Reject names that cannot serve as the generated Cargo package and Rust crate import after the generator's existing hyphen-to-underscore conversion; include reserved identifiers in that validation. Report that the user should choose or rename the destination instead of silently inventing a different name. No new `--name` option or shared manifest-validation refactor is needed.

Use a fixed starter namespace `example`, process version `"1"`, and an `echo` process connecting a numeric start port directly to an end port. This is already supported by the source pattern in `tests/cli.rs` and avoids adding task dependencies or converting package names into language identifiers. The `.bl` process version is intentionally distinct from the toolchain version.

Alternative rejected: deriving every source identifier from the directory would require escaping, normalization, and keyword handling with no benefit to the starter.

### 3. Preflight conflicts and create files exclusively

Before creating either scaffold file or initializing Git, validate the destination/name and check both output paths with symlink-aware metadata so dangling links also count as conflicts. An existing unrelated file, `.gitignore`, Cargo project, or repository is not itself a conflict. Create missing directories as necessary, then open scaffold files with exclusive creation rather than truncating writes. This preserves no-overwrite behavior if another process creates a path after preflight.

**Proposed failure policy:** known preflight conflicts leave scaffold files and Git metadata untouched. Unexpected later I/O failures may leave newly created directories or a partial scaffold; report the failing operation and do not claim success. Do not recursively delete the destination or attempt broad rollback in a directory that may contain user data. This is not a transactional multi-file initializer.

Alternative rejected: a generic staging/rollback framework is disproportionate to two files and risks deleting unrelated work. Exclusive creation and accurate partial-failure reporting provide the necessary data protection.

### 4. Recognize existing worktrees without requiring Git

Unless `--no-vcs` is supplied, inspect the effective destination and its ancestors for `.git` metadata, recognizing both repository directories and linked-worktree gitfiles. Perform detection on resolved filesystem paths so symlinked parents do not accidentally cause a nested repository. Do not scan or change unrelated child repositories. With an existing working tree, skip Git entirely; with `--no-vcs`, skip all Git operations.

When a new repository is needed, preflight that Git can be executed before writing scaffold files, then invoke `git init` with the destination as its working directory after the two scaffold files are written. Use direct argument passing, not shell interpolation. Ensure repository-redirection environment settings such as `GIT_DIR` and `GIT_WORK_TREE` cannot cause the subprocess to initialize a different location. Preserve normal user configuration such as the chosen initial branch. Missing Git or failed repository creation is a required-operation error with `--no-vcs` recovery guidance, not an advisory warning. Never run `git add` or `git commit`.

Do not generate or alter a root `.gitignore`. The existing `.blkit/.gitignore` remains generated by transpilation. Documentation should retain the distinction between source/lockfiles that belong in VCS and local runtime databases/attachment data that do not.

Alternatives rejected: invoking `git rev-parse` as the only detector would require Git even inside an existing working tree; adding a Git library would expand dependencies for simple setup. Supporting other VCS tools is outside the agreed scope.

### 5. Limit tool checks to bounded advisory version probes

After required scaffold/VCS operations succeed, invoke `cargo --version` and `rustc --version` separately from the destination, without a shell. Only successful execution matters; do not parse version strings or introduce an invented minimum compiler version. Missing executables, nonzero status, spawn/wait errors, and timeouts each become a concise named warning on stderr. Continue checking the other tool after a warning.

**Proposed implementation default:** bound each probe to five seconds using standard-library child polling, terminate and reap a timed-out child, and discard child output instead of copying arbitrary wrapper diagnostics into CLI output. No timeout option or reusable process-runner abstraction is needed. Set `RUSTUP_AUTO_INSTALL=0` for probes so rustup-managed commands do not install an absent active toolchain merely to answer an availability check. This behavior is documented in the [rustup environment-variable reference](https://rust-lang.github.io/rustup/environment-variables.html). Test this child environment with fake tools.

Report that Rust tools are needed for the later Cargo build; their absence does not prevent authoring or transpiling this starter. Successful probes do not establish target support, system linker availability, dependency availability, or compatible compiler versions. No checks for Docker, PostgreSQL clients, cloud CLIs, linkers, or other speculative prerequisites are added.

Alternative rejected: probing presence on `PATH` alone misses broken rustup shims; compiling a smoke project would require a functioning toolchain and potentially network access during initialization.

### 6. Print actionable steps without claiming a verified build

On success, identify the initialized project and show the separate steps: enter its directory if needed, run `blkit transpile`, run `cargo build --manifest-path .blkit/Cargo.toml`, then run the generated `<name>-api-worker` executable with its existing loopback defaults. Format displayed paths safely for the documented shell and handle spaces; mention the existing `CARGO_TARGET_DIR` override rather than promise the default binary location unconditionally. Runtime instructions should retain the README's loopback/access-control warning.

Use existing plain diagnostics/output conventions. Tool warnings belong on stderr and must remain readable when redirected or color is disabled. Do not add a spinner, machine-readable output format, or extra verbosity flags.

## Risks / Trade-offs

- [Advisory tool availability is weaker than build validation] -> Say so explicitly; exercise a real initialized-project Cargo build and a local echo request during implementation verification, not during user initialization.
- [A late I/O or Git failure can leave partial new files] -> Preflight predictable failures, use exclusive creation, preserve user data, report the failure, and document manual cleanup rather than promise atomicity.
- [Directory names can pass the current manifest loader but fail as generated Rust imports] -> Reject unusable names in init and test common hyphenated names, reserved identifiers, invalid names, and non-UTF-8 paths where supported, without widening this into a loader refactor.
- [Existing `.bl` files can conflict with the starter] -> Preserve all unrelated files; guarantee immediate transpilation for otherwise empty destinations, not arbitrary pre-existing source collections.
- [Worktree-marker detection is not a full Git repository audit] -> Handle ordinary repositories and linked worktrees; do not claim to repair malformed metadata or support bare repositories as authoring worktrees.
- [Version checks execute user-selected tools on PATH] -> Use fixed arguments, suppress automatic rustup installation, cap probe waits, and do not interpret child output as commands or compatibility evidence.
- [Local application data is not protected by `.blkit/.gitignore` when stored outside `.blkit`] -> Keep the existing documentation warning not to commit runtime data; do not silently alter user ignore rules.
- [A generated build can require matching runtime sources/releases and platform build prerequisites] -> Retain existing README caveats; do not claim probes prove dependency resolution, linking, or cross-machine portability.

## Migration Plan

This is an additive CLI command with no manifest, source-language, runtime, or database migration. Existing projects continue to use the current commands unchanged. Ship the help/completion/documentation updates with the command. Rollback removes initialization support; any created project remains a normal manually authorable blkit project accepted by the existing transpiler.
