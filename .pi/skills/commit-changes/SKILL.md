---
name: commit-changes
description: Use when asked to organize Git working-tree changes into reviewable commits, continue committing related changes one at a time, or push after those commits.
---

# Commit changes

Work on the current branch. Group approval, commit-message approval, and
approval of a later conflict resolution are distinct. Never treat an earlier
approval as permission for a changed commit.

1. **Inventory.** Inspect `git status --short --branch`, `git diff`,
   `git diff --cached`, and untracked files. Read the relevant files and history
   to understand intent. Flag secrets, generated files, unrelated changes, and
   pre-existing staged hunks; do not silently include, discard, or unstage them.
2. **Propose groups.** Show the proposed commits in order, with file/hunk
   membership, rationale, and anything excluded or uncertain. Ask the user to
   approve or amend the grouping *before staging*. If there is nothing to
   commit, stop; do not push as part of this workflow.
3. **Commit one group at a time.** Stage only its approved paths/hunks; inspect
   `git diff --cached --check` and the **entire** `git diff --cached` to ensure
   the index contains just this group. Show the staged changes and an exact,
   detailed message; wait for approval of that message before calling
   `git commit`. Follow the **Commit message conventions** section of
   `../commit-message/SKILL.md`; that skill's draft-only stopping rule does not
   apply here. Run relevant checks, commit with the exact approved message,
   then verify the resulting commit and remaining status before moving to the
   next group. Recheck the index if anything changed while waiting.
4. **If a pre-commit hook blocks the commit:** report the failing hook, issue,
   and proposed remedy. Without waiting again, fix issues within the approved
   group's intent, stage only those fixes, rerun checks, and retry the commit
   with the approved message. If remediation changes the commit's meaning,
   makes its message inaccurate, or needs unrelated changes, show the revised
   diff/message and obtain fresh approval first. Do not bypass hooks or discard
   unrelated work. Report and stop if the failure cannot be resolved safely.
5. **Push after all approved groups are committed.** Push the current branch to
   its configured upstream with `git push`; never force-push or invent an
   upstream. If there is no upstream, or auth/policy/network blocks the push,
   report it and ask how to proceed. If the push is rejected because the
   upstream advanced, require a clean working tree, then fetch the configured
   upstream remote and merge with `git merge --ff --no-commit '@{u}'`
   (fast-forward if possible, otherwise merge; **never rebase**). For a
   conflict-free pending merge, show the resulting diff and merge message and
   obtain approval before its commit, then push. For conflicts, diagnose and
   prepare a resolution, show the resulting changes, and ask explicit
   permission to commit the merge **and push**. Check for unresolved files and
   run relevant checks before committing; do not use an automatic merge commit.
   If reconciliation would affect unrelated work, stop and ask first.

Report commit hashes and push outcome. Do not claim a push succeeded until
Git confirms it.
