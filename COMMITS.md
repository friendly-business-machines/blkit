# Commit Message Format

When generating commit messages, follow these rules exactly.

## Structure

```
type(scope): Subject line

Body paragraph(s) if needed.
```

## Rules

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
- Types other than `fix` and `feat` are allowed: `build`, `chore`, `ci`, `docs`, `style`, `refactor`, `perf`, `test`
