---
name: commitmsg
description: Generate a commit message for the currently staged changes, following the repository's commit message conventions. Inspects the staged diff and recent commit log, drafts a message, and prints it — does not run git commit.
---

# Generate a commit message for the currently staged changes

Draft a commit message for the changes the user has already staged (`git add`-ed). Print the
message; do **not** run `git commit` — the user will commit it themselves.

## Steps

1. Run `git diff --cached --stat` to confirm there are staged changes. If the diff is empty,
   tell the user there's nothing staged and stop.
2. Run `git diff --cached` to inspect the full staged diff.
3. Draft a message that follows the [commit message conventions](#commit-message-conventions)
   exactly.
4. Output the message in a fenced code block so the user can copy it.

## Notes

- If the staged changes mix unrelated concerns, surface that as a question rather than
  papering over it with a vague subject.

## Commit message conventions

When generating commit messages, follow these rules exactly.

### Structure

```
type(scope): Subject line

Body paragraph(s) if needed.
```

### Rules

<!--- Chris Beam's rules: -->
- Separate subject from body with a blank line
- Limit the subject line to 50 characters
- Capitalize the subject line (begin all subject lines with a capital letter)
- Do not end the subject line with a period
- Use the imperative mood in the subject line
- Wrap the body at 72 characters
- Use the body to explain what and why vs. how
<!--- Conventional commit rules: -->
- Commits MUST be prefixed with a type, which consists of a noun, feat, fix, etc., followed by the OPTIONAL scope, and REQUIRED terminal colon and space
- The type `feat` MUST be used when a commit adds a new feature to your application or library
- The type `fix` MUST be used when a commit fixes a bug in your application or library
- A scope MAY be provided after a type. A scope MUST consist of a noun describing a section of the codebase surrounded by parenthesis, e.g., `fix(parser):`
- A description MUST immediately follow the colon and space after the type/scope prefix. The description is a short summary of the code changes, e.g., `fix: Array parsing issue when multiple spaces were contained in string`
- A longer commit body MAY be provided after the short description, providing additional contextual information about the code changes. The body MUST begin one blank line after the description
- A commit body is free-form and MAY consist of any number of newline separated paragraphs
<!--- Custom rules: -->
- Footers MUST NOT be included
- Types other than `fix` and `feat` are allowed: `build`, `chore`, `ci`, `docs`, `style`, `refactor`, `perf`, `test`, `spec`, `plan`, `rules`, `skill`, `mcp`, `dev`
- The type `docs` relates to Human-facing documentation (READMEs, architecture notes, contributor guides). Does not cover files that govern agent behaviour — use `rules` for those.
<!--- Custom types: --->
- Allowed type `spec`: Changes to specification documents that define intended behaviour
- Allowed type `plan`: Updates to task lists or implementation plans
- Allowed type `rules`: Changes to agent behaviour and constraint files (AGENTS.md, CLAUDE.md, .cursorrules, and any file @-included from them.)
- Allowed type `skill`: Changes to reusable agent workflow definitions (SKILL.md files)
- Allowed type `mcp`: Changes to MCP server configs, tool definitions, or external integrations
- Allowed type `dev`: Changes to local development environment scaffolding that have no bearing on the production build or CI pipeline (i.e. devcontainer.json, Dockerfile.dev, docker-compose.dev.yml, Makefile etc)
