---
name: Release Workflow
description: Release script (scripts/create-release.sh), the make release target, RELEASE_NOTES.md lifecycle, the publish-on-release CI workflow, and the /release Claude Code slash command
status: agreed
code:
  - scripts/create-release.sh
  - .github/workflows/publish.yml
---

# Release Workflow

The release process is split into two distinct phases:

1. **GitHub Release** — versioning, merging, and creating a GitHub Release. Driven locally by `scripts/create-release.sh` via `make release VERSION=x.y.z`, with optional AI assistance from the `/release` Claude Code slash command.
2. **Package Publishing** — pkg.go.dev picks up the new module version automatically once the GitHub Release tag is pushed; a small GitHub Actions workflow nudges the proxy and verifies the tag.

---

## Phase 1: GitHub Release

### `scripts/create-release.sh`

The script executes the following steps in order:

1. **Branch check** — verifies the current branch is `main`. Exits with an error otherwise.
2. **Pull latest** — runs `git pull` to ensure `main` is up to date.
3. **Clean tree check** — verifies the working tree has no uncommitted changes. Exits with an error if dirty.
4. **Pre-commit hooks** — runs all pre-commit hooks across the entire codebase. Hooks include at minimum `gofmt` / `goimports` for formatting and `golangci-lint` for static analysis.
5. **Build** — runs `go build ./...` and fails if anything does not compile.
6. **Tests** — runs the full Go test suite via `go test ./...`. Any failure aborts the release.
7. **Docs regen** — regenerates all programmatically generated Markdown files in `docs/reference/` (godoc-derived API reference) and diffs them against what is committed, failing if stale.
8. **Docs build** — runs `zensical build` locally and checks for broken links.
9. **Clean tree check (second)** — re-verifies the working tree is clean. Pre-commit hooks may auto-fix formatting and the docs regen step writes files; if either left uncommitted changes the fix must be committed before re-running.
10. **Release branch** — creates and pushes a `release/x.y.z` branch from `main`.
11. **Version bump** — updates the `VERSION` file with the new version. The Go module version is carried by the git tag itself (`vx.y.z`) rather than a field in `go.mod`; no manifest edit is needed.
12. **Commit and push** — commits the version bump on the `release/x.y.z` branch and pushes it.
13. **Open PR** — opens a pull request against `main` via `gh pr create`.
14. **Auto-merge** — enables auto-merge on the PR (`gh pr merge --auto --squash`) so it merges automatically once all required CI checks pass.
15. **Wait for merge** — polls until the PR is merged, checking every 30 seconds. Times out after 30 minutes and exits with an error if the PR has not merged, leaving the release branch and PR open for manual resolution.
16. **RELEASE_NOTES.md step**:
    - If `RELEASE_NOTES.md` does not exist: prints a prompt and waits until the developer confirms the file has been created.
    - If `RELEASE_NOTES.md` already exists: prints its contents and waits for the developer to confirm they are happy with them before proceeding.
17. **GitHub Release** — creates the GitHub Release from `main` via `gh release create vx.y.z --notes-file RELEASE_NOTES.md`. Pushing the `vx.y.z` git tag is what makes the new module version visible to `go get` and pkg.go.dev.
18. **Cleanup** — deletes `RELEASE_NOTES.md` on successful completion.

### Error Reporting

When the script encounters a failure it prints a structured error block to stderr and exits with a non-zero status. The format is consistent across all failure points so that both humans and AI agents can reliably parse what went wrong and what must be done before re-running.

```
--- BLKIT RELEASE ERROR ---
STEP:   <step name, e.g. "Tests">
FAILED: <one-line description of what failed>
DETAIL: <verbatim output from the failing command, if any>
ACTION: <exact description of what must be fixed before re-running>
---------------------------
```

Examples:

```
--- BLKIT RELEASE ERROR ---
STEP:   Branch check
FAILED: Current branch is 'feature/my-feature', expected 'main'
DETAIL:
ACTION: Switch to main and pull the latest changes before running make release.
---------------------------
```

```
--- BLKIT RELEASE ERROR ---
STEP:   Tests
FAILED: Go test suite failed
DETAIL: --- FAIL: TestEvaluate_Suspended (0.01s)
        process_test.go:142: expected status SUSPENDED, got RUNNING
ACTION: Fix the failing test in processes/, merge a fix PR to main, then re-run make release.
---------------------------
```

The `ACTION:` line is the primary signal for an AI agent — it states precisely what to do next. The agent must not re-run the script until the action has been completed.

### `make release`

The root `Makefile` exposes a `release` target that invokes `scripts/create-release.sh`, forwarding the `VERSION` argument:

```makefile
.PHONY: release
release:
	@bash scripts/create-release.sh $(VERSION)
```

Invoked as:

```
make release VERSION=1.2.0
```

### RELEASE_NOTES.md

`RELEASE_NOTES.md` is a transient local file. It is listed in `.gitignore` and must never be committed. Its contents are passed verbatim as the body of the GitHub Release.

### `/release` Claude Code Slash Command

The `/release` slash command (defined in `.claude/commands/release.md`) provides a guided conversational interface on top of `scripts/create-release.sh`:

1. Analyses all commits and merged pull requests since the last release tag.
2. Generates draft release notes and presents them to the developer for review and editing.
3. Writes the agreed release notes to `RELEASE_NOTES.md`.
4. Confirms with the developer that the proposed version number is appropriate given the changes (major/minor/patch semantics).
5. Invokes `bash scripts/create-release.sh x.y.z` with the confirmed version.
6. While the script is paused at the `RELEASE_NOTES.md` step, signals that the file is ready and allows the script to proceed.

---

## Phase 2: Package Publishing

### Publishing CI Workflow

Publishing is triggered by the creation of a GitHub Release and handled by a dedicated GitHub Actions workflow (`.github/workflows/publish.yml`). It does not run on pull requests or pushes — only on the `release` event.

A single version number applies, sourced from the `VERSION` file and the version tag on the GitHub Release.

### Registry

The module is published to [pkg.go.dev](https://pkg.go.dev) automatically — there is no explicit upload step. pkg.go.dev's module proxy indexes new versions when it sees a corresponding git tag (`vx.y.z`).

### Workflow Steps

The workflow runs the following steps in order:

1. **Origin check** — verifies the release tag points to a commit on `main`. Exits with an error otherwise.
2. **Module proxy fetch** — issues a request to `https://proxy.golang.org/<module>/@v/vx.y.z.info` to prompt the proxy to fetch and index the new version. This is best-effort: pkg.go.dev will pick the version up on its next scan regardless, so a failure here does not fail the job.
3. **Verify discoverability** — polls `https://pkg.go.dev/<module>@vx.y.z` until it returns a 200 (with a reasonable timeout). Surfaces a warning, not a hard failure, if the page does not appear within the window — propagation can take longer than the job duration.
