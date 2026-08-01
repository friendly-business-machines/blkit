---
name: Pull Request Workflow
description: Local pre-pull-request script (scripts/create-pull-request.sh), the make pr target, PR_DESCRIPTION.md lifecycle, the /pr Claude Code slash command, and the GitHub Actions pull-request-checks.yml workflow that runs required status checks on every PR
status: agreed
code:
  - scripts/create-pull-request.sh
  - .github/workflows/pull-request-checks.yml
---

# Pull Request Workflow

The pull request workflow has two components:

1. **Local script** (`scripts/create-pull-request.sh`) — runs checks locally before opening a PR, ensuring nothing broken reaches CI. Invoked via `make pr`, with optional AI assistance from the `/pr` Claude Code slash command.
2. **CI checks** (`.github/workflows/pull-request-checks.yml`) — runs automatically on every open PR via GitHub Actions, enforcing the same quality bar as a required status check.

---

## Phase 1: Local Script

### `scripts/create-pull-request.sh`

The script executes the following steps in order:

1. **Branch check** — verifies the current branch is not `main`. Exits with an error if on `main`.
2. **Clean tree check** — verifies the working tree has no uncommitted changes. Exits with an error if dirty.
3. **Pre-commit hooks** — runs all pre-commit hooks across the entire codebase. Hooks include at minimum: `gofmt` / `goimports` (formatter) and `golangci-lint` (linter).
4. **Build** — runs `go build ./...`.
5. **Tests** — runs `go test ./...`.
6. **Docs regen** — regenerates the godoc-derived Markdown files in `docs/reference/` and diffs them against what is committed, failing if stale.
7. **Docs build** — runs `zensical build` locally and checks for broken links.
8. **Clean tree check (second)** — re-verifies the working tree is clean. Pre-commit hooks may auto-fix formatting, and the docs regen step writes files; if either left uncommitted changes the developer must review, commit, and re-run `make pr`.
9. **PR_DESCRIPTION.md step** (always pauses):
   - If `PR_DESCRIPTION.md` does not exist: prints a prompt and waits until the developer confirms the file has been created.
   - If `PR_DESCRIPTION.md` already exists: prints its contents and waits for the developer to confirm they are happy with them before proceeding.
10. **Open PR** — opens the pull request via the GitHub CLI (`gh pr create`) using the title and body from `PR_DESCRIPTION.md`.
11. **Cleanup** — deletes `PR_DESCRIPTION.md` on successful completion.

### Error Reporting

When the script encounters a failure it prints a structured error block to stderr and exits with a non-zero status. The format is consistent across all failure points so that both humans and AI agents can reliably parse what went wrong and what must be done before re-running.

```
--- BLKIT PR ERROR ---
STEP:   <step name, e.g. "Tests">
FAILED: <one-line description of what failed>
DETAIL: <verbatim output from the failing command, if any>
ACTION: <exact description of what must be fixed and committed before re-running>
----------------------
```

Examples:

```
--- BLKIT PR ERROR ---
STEP:   Branch check
FAILED: Current branch is 'main'
DETAIL:
ACTION: Switch to a feature branch before running make pr.
----------------------
```

```
--- BLKIT PR ERROR ---
STEP:   Tests
FAILED: Go test suite failed
DETAIL: --- FAIL: TestNumberAdd (0.00s)
ACTION: Fix the failing tests in go/, then commit the changes and re-run make pr.
----------------------
```

```
--- BLKIT PR ERROR ---
STEP:   Clean tree check (second)
FAILED: Working tree is dirty after pre-commit hooks ran
DETAIL: M expr/number.go
ACTION: The formatter modified the files listed above. Review them, stage and commit
        the changes, then re-run make pr.
----------------------
```

The `ACTION:` line is the primary signal for an AI agent — it states precisely what to do next. The agent must not re-run the script until the action has been completed and the fix is committed.

### `make pr`

The root `Makefile` exposes a `pr` target that invokes `scripts/pr.sh`:

```makefile
.PHONY: pr
pr:
	@bash scripts/create-pull-request.sh
```

### PR_DESCRIPTION.md

`PR_DESCRIPTION.md` is a transient local file. It is listed in `.gitignore` and must never be committed. Its format:

```
<one-line PR title>

<PR body — markdown, as many paragraphs as needed>
```

The script reads the first line as the PR title and the remainder as the PR body when calling `gh pr create`.

### `/pr` Claude Code Slash Command

The `/pr` slash command (defined in `.claude/commands/pr.md`) provides an AI-assisted authoring layer on top of `scripts/create-pull-request.sh`:

1. Invokes `bash scripts/create-pull-request.sh`.
2. While the script is paused at the `PR_DESCRIPTION.md` step, analyses the commits and diff on the current branch relative to `main`.
3. Generates a draft PR title and description, presenting them to the developer.
4. Iterates interactively — the developer can request revisions — until they approve.
5. Writes the agreed title and description to `PR_DESCRIPTION.md`.
6. Signals to the waiting script that the file is ready, allowing it to proceed and open the PR.

---

## Phase 2: CI Checks

A GitHub Actions workflow (`.github/workflows/pull-request-checks.yml`) fires whenever a pull request targeting `main` is opened, or new commits are pushed to a branch with an open pull request targeting `main`. The results are visible under the **Checks** section of the pull request in the GitHub web UI.

### Go Job

The workflow runs a single `go` job. It runs three sequential stages:

1. **Build / compile** — `go build ./...`. If this fails, the job halts immediately.
2. **Format and lint** (in parallel) — run once the build passes.
3. **Test** — `go test ./...`, runs once format and lint both pass.

The linter operates on compiled artefacts and requires a successful build, so build always precedes format and lint.

#### Format and lint (parallel)

| Formatter | Linter | Type checker |
|---|---|---|
| `gofmt` / `goimports` | `golangci-lint` | (built into compiler) |

### Docs Job

An additional `docs` job runs independently alongside the `go` job:

1. **Regen check** — regenerates the godoc-derived API reference Markdown and diffs it against what is committed in `docs/reference/`. The job fails if there is any discrepancy, ensuring the committed docs are always in sync with the source code.
2. **Site build** — runs `zensical build` to compile the full documentation site.
3. **Link check** — verifies that no internal or external links in the compiled site are broken.

If the regen check fails, steps 2 and 3 do not run.

### Required Status Checks

Both jobs (`go`, `docs`) are configured as **required status checks** in the branch protection rules for `main`. A pull request cannot be merged until every job passes.

Branch protection for `main` additionally requires:

- At least one approving review.
- The branch to be up to date with `main` before merging.
- No direct pushes (all changes must arrive via pull request).
