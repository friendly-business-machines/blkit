---
description: Workshop a pull request description, validate the branch, and open the PR
argument-hint: "[additional instructions]"
---
# Initiate a pull request

Start by inspecting the commits and complete diff between the current branch and
`main`. Workshop an outcome-focused pull request title and Markdown description
with me. Revise them until I explicitly approve the exact title and body; do not
run the pre-PR checks before that approval.

After approval, create `.cache/blkit/` and write the title as the first line of
`.cache/blkit/PR_DESCRIPTION.md`, followed by a blank line and the approved body.
Then run:

```sh
bash scripts/create-pull-request.sh .cache/blkit/PR_DESCRIPTION.md
```

The file is intentionally retained if validation or PR creation fails, so address
the reported `ACTION:` and rerun the same command. Never alter the approved
content without asking me. Report the pull request URL when creation succeeds.

Additional instructions: ${ARGUMENTS:-none}
