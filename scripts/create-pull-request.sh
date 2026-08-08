#!/usr/bin/env bash
#
# create-pull-request.sh
# ======================
#
# This script is blkit's complete local pull-request process. It takes a
# committed feature branch, proves that it meets the same quality bar expected
# by CI, publishes it, and opens a pull request against `main`.
#
# The process deliberately starts with the pull-request description rather than
# the checks. Validation can uncover an issue that takes several attempts to
# fix; capturing the agreed title and body first means that work is not lost
# between attempts. The description remains in `.cache/blkit/` after every
# failure and is removed only after GitHub confirms that the PR was created.
#
# There are two entry paths:
#
#   1. `make pr`
#      Calls this script without arguments. The script asks for a title and a
#      multiline Markdown body, shows the result for confirmation, and reuses an
#      already-saved description after a failed attempt.
#
#   2. `scripts/create-pull-request.sh <description-file>`
#      Used by the Pi `/pr` prompt after the developer has workshopped and
#      approved a description. The supplied file is copied into the same durable
#      cache before any validation starts.
#
# Description files use this intentionally simple format:
#
#   One-line pull request title
#
#   Markdown body, which may contain as many paragraphs as needed.
#
# Once the description is safe, the script performs five phases in order:
#
#   1. Repository safety — require a feature branch and a clean working tree.
#   2. Source validation — run pre-commit, build, and tests.
#   3. Documentation validation — regenerate committed output and build the
#      documentation site in strict mode.
#   4. Publication — authenticate GitHub CLI and push the feature branch.
#   5. PR creation — split the saved description into title/body arguments and
#      call `gh pr create` against `main`.
#
# Phase 1: Repository safety
# --------------------------
# The script first verifies that it is running inside a Git repository. It then
# reads the current branch and rejects both detached HEAD and `main`: pull
# requests must be opened from a named feature branch. Finally, it checks Git's
# porcelain status, including untracked files, and stops if anything other than
# ignored retry state is uncommitted. This establishes that every later check
# examines exactly the commits that will be pushed.
#
# Phase 2: Source validation
# --------------------------
# The configured pre-commit hooks run against the committed difference from
# `main` to `HEAD`. Depending on the changed files, these hooks cover formatting,
# linting, secret scanning, module tidiness, generated documentation, and other
# repository-specific checks. The script then runs `go build ./...` followed by
# `go test ./...` against the root module. Build runs first so compilation errors
# are reported directly rather than being mixed into test output.
#
# Phase 3: Documentation validation
# ---------------------------------
# `scripts/generate-docs.sh` rebuilds the committed Go API reference, then
# `scripts/generate-llms-txt.sh` rebuilds `llms.txt` and `llms-full.txt` from the
# refreshed reference and documentation navigation. Git status is inspected for
# changed or newly generated artifacts; any difference means the generated files
# must be reviewed and committed before retrying. Zensical then builds the whole
# site with `--strict --clean`, proving that a clean render succeeds and that
# links and anchors resolve. A second clean-tree check catches any file changed
# by a formatter, generator, or build step.
#
# Phase 4: Publication
# --------------------
# No network publication happens until every local validation step has passed.
# The script confirms that `gh` is installed and authenticated, then inspects the
# branch's upstream configuration. A new branch is pushed to `origin` with an
# upstream; a previously published branch is pushed to its existing upstream.
# Neither path force-pushes or changes an established tracking relationship.
#
# Phase 5: PR creation
# --------------------
# The first line of the cached description becomes the PR title. The Markdown
# body begins on line three and is copied into a private temporary file for
# `gh pr create --base main`. If GitHub rejects the request, the cached source
# description remains available for another attempt. On success, GitHub's output
# includes the new PR URL, the temporary body file is removed by an exit trap,
# and the cached description is deleted because it is no longer needed for
# recovery.
#
# Every failure uses the same structured error block. Its `ACTION:` line tells a
# developer or coding agent what must happen before retrying. The script never
# bypasses a failed check and never deletes the saved description on failure.
#
# Usage:
#   scripts/create-pull-request.sh [description-file]
#
# Required tools: git, pre-commit, Go, Zensical, and an authenticated GitHub CLI.

# Fail on unhandled errors, unset variables, and failed commands inside
# pipelines. Individual validation commands temporarily relax `errexit` inside
# run_step so their output can be captured and reported consistently.
set -euo pipefail

# Relative description paths are interpreted from the directory in which the
# developer invoked the script, even though all project commands run from the
# repository root. This makes both `make pr` and direct invocation predictable.
readonly CALLER_DIR="$PWD"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly REPO_ROOT

# `.cache/` is already ignored by the repository. Keeping retry state here
# avoids dirtying the working tree while still surviving failed checks and
# repeated invocations.
readonly DESCRIPTION_DIR="${REPO_ROOT}/.cache/blkit"
readonly SAVED_DESCRIPTION="${DESCRIPTION_DIR}/PR_DESCRIPTION.md"

# Print the command-line contract. This is intentionally short; the commentary
# above documents the process and the function bodies document each phase.
usage() {
  cat <<'EOF'
Usage: scripts/create-pull-request.sh [description-file]

The description file must contain the PR title on its first line, followed by a
blank line and the Markdown body. With no argument, the script prompts for the
title and body. The accepted description is saved under .cache/blkit/ until the
pull request is opened successfully.
EOF
}

# Emit one machine- and human-readable failure shape, then stop.
#
# Arguments:
#   $1 — workflow step that failed
#   $2 — concise failure summary
#   $3 — exact action required before retrying
#   $4 — optional command output or repository status details
#
# Keeping ACTION separate from DETAIL matters for agent-driven retries: command
# output explains the problem, while ACTION states the permitted next move.
fail() {
  local step="$1" failed="$2" action="$3" detail="${4:-}"
  {
    echo "--- BLKIT PR ERROR ---"
    printf 'STEP:   %s\n' "$step"
    printf 'FAILED: %s\n' "$failed"
    echo "DETAIL:"
    if [[ -n "$detail" ]]; then
      printf '%s\n' "$detail"
    fi
    printf 'ACTION: %s\n' "$action"
    echo "----------------------"
  } >&2
  exit 1
}

# Run a validation/publication command with live output and structured failure.
#
# The first three arguments are labels consumed by fail(); all remaining
# arguments are the command to execute. `tee` lets the developer watch long
# builds and test suites while retaining their complete output for DETAIL if
# they fail. PIPESTATUS[0] records the wrapped command's status rather than
# tee's status.
run_step() {
  local step="$1" failed="$2" action="$3"
  shift 3

  local output_file status
  output_file="$(mktemp)"
  set +e
  "$@" 2>&1 | tee "$output_file"
  status=${PIPESTATUS[0]}
  set -e

  if ((status != 0)); then
    local detail
    detail="$(cat "$output_file")"
    rm -f "$output_file"
    fail "$step" "$failed" "$action" "$detail"
  fi
  rm -f "$output_file"
}

# Validate the stable on-disk interface shared by `/pr`, `make pr`, and gh.
#
# The title must be non-empty and line two must be blank. The blank separator
# lets the final phase obtain the title from line one and the body from line
# three onward without parsing Markdown. Trailing carriage returns are removed
# while checking so files created by Windows tooling are accepted.
validate_description() {
  local file="$1" title second_line
  [[ -f "$file" ]] || fail \
    "PR description" \
    "Description file not found: ${file}" \
    "Create the description file and rerun the command with its path."
  [[ -s "$file" ]] || fail \
    "PR description" \
    "Description file is empty: ${file}" \
    "Add a title and body, then rerun the command."

  IFS= read -r title <"$file" || true
  title="${title%$'\r'}"
  [[ -n "${title//[[:space:]]/}" ]] || fail \
    "PR description" \
    "The PR title is empty" \
    "Put the PR title on the first line of the description file."

  second_line="$(sed -n '2p' "$file")"
  second_line="${second_line%$'\r'}"
  [[ -z "${second_line//[[:space:]]/}" ]] || fail \
    "PR description" \
    "The title must be followed by a blank line" \
    "Insert a blank second line before the PR body."
}

# Copy an externally prepared description into the retry cache.
#
# Relative paths are resolved against CALLER_DIR, not REPO_ROOT. The copy is
# written to a private temporary file and atomically renamed so an interrupted
# invocation cannot leave behind a partially-written description. If the caller
# already passed the canonical cache file, no copy is necessary.
save_description() {
  local source="$1" source_abs tmp
  source_abs="$source"
  if [[ "$source_abs" != /* ]]; then
    source_abs="${CALLER_DIR}/${source_abs}"
  fi
  validate_description "$source_abs"

  if [[ "$(cd "$(dirname "$source_abs")" && pwd)/$(basename "$source_abs")" == "$SAVED_DESCRIPTION" ]]; then
    return
  fi

  tmp="$(mktemp "${DESCRIPTION_DIR}/.PR_DESCRIPTION.XXXXXX")"
  cp "$source_abs" "$tmp"
  chmod 0600 "$tmp"
  mv "$tmp" "$SAVED_DESCRIPTION"
}

# Collect and confirm a description for the interactive `make pr` path.
#
# A saved description is offered first because its presence normally means a
# previous validation or GitHub step failed. New bodies are entered one line at
# a time and terminated by a line containing only `.`; this avoids depending on
# a particular editor. Rejecting the preview starts collection again, while an
# accepted description remains cached for the rest of the workflow.
collect_description() {
  local answer title line tmp

  if [[ -s "$SAVED_DESCRIPTION" ]]; then
    echo "A saved pull request description already exists:"
    echo
    cat "$SAVED_DESCRIPTION"
    echo
    read -r -p "Reuse this description? [Y/n] " answer
    case "${answer:-y}" in
      y|Y|yes|YES|Yes)
        validate_description "$SAVED_DESCRIPTION"
        return
        ;;
    esac
  fi

  [[ -t 0 ]] || fail \
    "PR description" \
    "No description file was supplied and standard input is not interactive" \
    "Pass a description file: scripts/create-pull-request.sh <description-file>."

  while true; do
    read -r -p "Pull request title: " title
    [[ -n "${title//[[:space:]]/}" ]] && break
    echo "The title cannot be empty." >&2
  done

  echo "Enter the Markdown body. End it with a line containing only a period (.)."
  tmp="$(mktemp "${DESCRIPTION_DIR}/.PR_DESCRIPTION.XXXXXX")"
  {
    printf '%s\n\n' "$title"
    while IFS= read -r line; do
      [[ "$line" == "." ]] && break
      printf '%s\n' "$line"
    done
  } >"$tmp"
  chmod 0600 "$tmp"
  mv "$tmp" "$SAVED_DESCRIPTION"
  validate_description "$SAVED_DESCRIPTION"

  echo
  echo "Pull request description:"
  echo
  cat "$SAVED_DESCRIPTION"
  echo
  read -r -p "Use this description? [Y/n] " answer
  case "${answer:-y}" in
    y|Y|yes|YES|Yes)
      return
      ;;
    *)
      rm -f "$SAVED_DESCRIPTION"
      collect_description
      ;;
  esac
}

# ---------------------------------------------------------------------------
# Command-line parsing
# ---------------------------------------------------------------------------
# Exactly one optional positional argument is supported: the prepared
# description file. More options would obscure the intentionally fixed project
# workflow, so checks and target branch are not configurable here.

if (($# > 1)); then
  usage >&2
  exit 2
fi
if (($# == 1)) && [[ "$1" == "-h" || "$1" == "--help" ]]; then
  usage
  exit 0
fi

# ---------------------------------------------------------------------------
# Phase 0: capture durable PR metadata
# ---------------------------------------------------------------------------
# All subsequent commands run from the repository root. The cache directory is
# private to the current user because a PR body may contain issue or customer
# context that should not be exposed through permissive filesystem defaults.

cd "$REPO_ROOT"
mkdir -p "$DESCRIPTION_DIR"
chmod 0700 "$DESCRIPTION_DIR"

# The argument path is the non-interactive `/pr` route. With no argument we own
# the whole authoring interaction, including reuse after a failed attempt.
if (($# == 1)); then
  save_description "$1"
else
  collect_description
fi

validate_description "$SAVED_DESCRIPTION"
echo "Saved pull request description to .cache/blkit/PR_DESCRIPTION.md."

# ---------------------------------------------------------------------------
# Phase 1: repository safety
# ---------------------------------------------------------------------------
# A PR must come from a named feature branch. Requiring a clean tree ensures the
# checks inspect exactly the committed content that will be pushed; it also
# makes any formatter or generator changes during validation unambiguous.

command -v git >/dev/null 2>&1 || fail \
  "Prerequisites" "git is not installed or not on PATH" \
  "Install git, then rerun the command."
git rev-parse --show-toplevel >/dev/null 2>&1 || fail \
  "Repository check" "The script is not running in a Git repository" \
  "Run the script from a blkit checkout."

branch="$(git branch --show-current)"
[[ -n "$branch" ]] || fail \
  "Branch check" "HEAD is detached" \
  "Switch to a feature branch before rerunning the command."
[[ "$branch" != "main" ]] || fail \
  "Branch check" "Current branch is 'main'" \
  "Switch to a feature branch before rerunning the command."

status="$(git status --porcelain --untracked-files=all)"
[[ -z "$status" ]] || fail \
  "Clean tree check" "Working tree has uncommitted changes" \
  "Commit or intentionally remove the listed changes, then rerun the command." \
  "$status"

# ---------------------------------------------------------------------------
# Phase 2: source validation
# ---------------------------------------------------------------------------
# Pre-commit is run over commits introduced between main and HEAD. This invokes
# the repository's configured formatters, linters, secret scanner,
# module-tidiness checks, and file-specific hooks. Build and tests then exercise
# the root Go module explicitly, matching the Go jobs in PR CI.

command -v pre-commit >/dev/null 2>&1 || fail \
  "Pre-commit hooks" "pre-commit is not installed or not on PATH" \
  "Install pre-commit, then rerun the command."
run_step \
  "Pre-commit hooks" \
  "Pre-commit hooks failed" \
  "Fix and commit the reported issues, then rerun the command." \
  pre-commit run --from-ref main --to-ref HEAD

command -v go >/dev/null 2>&1 || fail \
  "Build" "go is not installed or not on PATH" \
  "Install the repository's required Go version, then rerun the command."
run_step \
  "Build" \
  "Go build failed" \
  "Fix and commit the build errors, then rerun the command." \
  go build ./...
run_step \
  "Tests" \
  "Go test suite failed" \
  "Fix and commit the failing tests, then rerun the command." \
  go test ./...

# ---------------------------------------------------------------------------
# Phase 3: documentation validation
# ---------------------------------------------------------------------------
# Generated reference and LLM-discovery files are committed artifacts. Running
# both generators and then checking git status proves that the committed copies
# match the source. `git status` is used instead of only `git diff` so newly
# generated, previously untracked files are caught too.

run_step \
  "Docs regeneration" \
  "API reference generation failed" \
  "Fix the generation error, then rerun the command." \
  bash scripts/generate-docs.sh
run_step \
  "Docs regeneration" \
  "LLM discovery file generation failed" \
  "Fix the generation error, then rerun the command." \
  bash scripts/generate-llms-txt.sh

generated_diff="$(git status --porcelain -- docs/reference docs/llms.txt docs/llms-full.txt)"
[[ -z "$generated_diff" ]] || fail \
  "Docs regeneration" "Generated documentation is stale" \
  "Review and commit the regenerated files, then rerun the command." \
  "$generated_diff"

# Generation proves freshness; the strict Zensical build separately proves that
# the complete site renders and that its links and anchors resolve.
command -v zensical >/dev/null 2>&1 || fail \
  "Docs build" "zensical is not installed or not on PATH" \
  "Install zensical, then rerun the command."
run_step \
  "Docs build" \
  "The documentation site failed to build" \
  "Fix and commit the documentation errors, then rerun the command." \
  zensical build --strict --clean

# Hooks and generators are allowed to repair files, but repaired files must be
# reviewed and committed rather than silently included in the PR. This second
# clean-tree barrier catches every such mutation before anything is pushed.
status="$(git status --porcelain --untracked-files=all)"
[[ -z "$status" ]] || fail \
  "Clean tree check (second)" \
  "Working tree is dirty after validation" \
  "Review and commit the listed changes, then rerun the command." \
  "$status"

# ---------------------------------------------------------------------------
# Phase 4: publish the feature branch
# ---------------------------------------------------------------------------
# Publication begins only after every local check has passed. Authentication is
# checked explicitly so a missing or expired login produces a useful ACTION
# rather than an opaque failure from the final `gh pr create` command.

command -v gh >/dev/null 2>&1 || fail \
  "Open pull request" "gh is not installed or not on PATH" \
  "Install and authenticate the GitHub CLI, then rerun the command."
run_step \
  "GitHub authentication" \
  "GitHub CLI authentication check failed" \
  "Run 'gh auth login', then rerun the command." \
  gh auth status

# A new local branch needs an upstream on first push. Existing branches use
# their configured upstream, which avoids accidentally changing their tracking
# relationship. Neither path force-pushes.
if ! git rev-parse --abbrev-ref '@{upstream}' >/dev/null 2>&1; then
  run_step \
    "Push branch" \
    "Failed to publish branch '${branch}'" \
    "Resolve the push error, then rerun the command." \
    git push --set-upstream origin "$branch"
else
  run_step \
    "Push branch" \
    "Failed to push branch '${branch}'" \
    "Resolve the push error, then rerun the command." \
    git push
fi

# ---------------------------------------------------------------------------
# Phase 5: create the pull request
# ---------------------------------------------------------------------------
# GitHub CLI accepts title and body separately. The description-file contract
# makes the split deterministic: line one is the title, line two is the blank
# separator, and line three onward is copied to a private temporary body file.

title="$(head -n 1 "$SAVED_DESCRIPTION" | tr -d '\r')"
body_file="$(mktemp)"
trap 'rm -f "$body_file"' EXIT
tail -n +3 "$SAVED_DESCRIPTION" >"$body_file"

run_step \
  "Open pull request" \
  "GitHub CLI failed to create the pull request" \
  "Resolve the GitHub CLI error; the saved description has been preserved for retry." \
  gh pr create --base main --head "$branch" --title "$title" --body-file "$body_file"

# Reaching this line means GitHub returned success. The cached description has
# served its retry purpose and can now be removed. Every earlier exit path leaves
# it untouched for the next invocation.
rm -f "$SAVED_DESCRIPTION"
echo "Pull request created successfully; removed the saved description."
