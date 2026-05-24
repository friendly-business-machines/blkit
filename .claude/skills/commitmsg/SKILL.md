---
name: commitmsg
description: Generate a commit message for the currently staged changes, following the conventions in COMMITS.md. Inspects the staged diff and recent commit log, drafts a message, and prints it — does not run git commit.
---

# Generate a commit message for the currently staged changes

Draft a commit message for the changes the user has already staged (`git add`-ed). Print the
message; do **not** run `git commit` — the user will commit it themselves.

## Steps

1. Run `git diff --cached --stat` to confirm there are staged changes. If the diff is empty,
   tell the user there's nothing staged and stop.
2. Run `git diff --cached` to inspect the full staged diff.
3. Read [COMMITS.md](../../../COMMITS.md) and follow its rules exactly. That file is the
   single source of truth for format, allowed types, subject/body length, and footer policy —
   do not restate or paraphrase its rules here.
4. Output the message in a fenced code block so the user can copy it.

## Notes

- If the staged changes mix unrelated concerns, surface that as a question rather than
  papering over it with a vague subject.
