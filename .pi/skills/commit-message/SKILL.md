---
name: commit-message
description: Generate a Git commit message for currently staged changes. Use when asked to draft or suggest a commit message for changes already staged with git add.
---

# Generate a commit message for the currently staged changes

Draft a commit message for the changes the user has already staged with
`git add`. Print the message, but do **not** run `git commit`; the user will
commit it themselves.

## Steps

1. Run `git diff --cached --stat` to confirm there are staged changes. If the
   diff is empty, tell the user there is nothing staged and stop.
2. Run `git diff --cached` to inspect the full staged diff.
3. Draft one accurate message that follows the
   [commit message conventions](#commit-message-conventions) exactly. Do not
   infer changes unsupported by the staged diff.
4. Output only the message in a fenced `text` code block so the user can copy
   it.

## Commit message conventions

### Structure

```text
type(scope): Subject line

Body paragraph(s) if needed.
```

The scope is optional. Omit the body when the subject fully explains a small
change.

### Subject rules

- Separate the subject from the body with one blank line.
- Limit the complete subject line, including type and scope, to 50 characters.
- Capitalize the description after the prefix.
- Do not end the subject with a period.
- Use the imperative mood: describe what applying the commit does.
- Prefix every subject with a lowercase type, optional scope, and required
  colon plus space: `type(scope): Description` or `type: Description`.
- Make a scope a concise noun naming the affected codebase section.
- Keep the description specific; do not merely repeat the type or filenames.

### Body rules

- Wrap body text at 72 characters.
- Explain what changed and why, not implementation mechanics that are obvious
  from the diff.
- Use any number of newline-separated paragraphs when context is useful.
- Do not include footers, trailers, sign-offs, or `BREAKING CHANGE` footer
  blocks.

### Types

Use exactly one of these types:

- `feat`: New application or library functionality.
- `fix`: A bug fix in the application or library.
- `build`: Production build system or dependency changes.
- `chore`: Routine maintenance not covered by a more specific type.
- `ci`: Continuous integration configuration or scripts.
- `docs`: Human-facing documentation, such as READMEs, architecture notes, or
  contributor guides. Agent behavior files are `rules`, not `docs`.
- `style`: Formatting-only changes that do not alter behavior.
- `refactor`: Code restructuring without a feature or bug fix.
- `perf`: Performance improvements.
- `test`: Test additions or corrections.
- `spec`: Specification documents defining intended behavior.
- `plan`: Task lists or implementation plans.
- `rules`: Agent behavior or constraint files, including `AGENTS.md`,
  `CLAUDE.md`, `.cursorrules`, and files included from them.
- `skill`: Reusable agent workflow definitions, including `SKILL.md` files.
- `mcp`: MCP server configuration, tool definitions, or external integrations.
- `dev`: Local development environment scaffolding with no effect on the
  production build or CI, such as dev containers, `Dockerfile.dev`,
  `docker-compose.dev.yml`, or a development-only `Makefile`.

Choose the type for the change's primary intent, not merely the kind of file
edited. Use `feat` for a new user-facing capability and `fix` for corrected
behavior even when tests or documentation change alongside it.

## Validation

Before responding, verify:

1. The message accurately covers the supplied change as one coherent commit.
2. The subject follows the required prefix and is at most 50 characters.
3. The description is capitalized, imperative, and has no final period.
4. Any body starts after one blank line and is wrapped at 72 characters.
5. No footer is present.
