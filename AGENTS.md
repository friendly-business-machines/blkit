# Agent instructions

## Modifying the devcontainer

If you modify `.devcontainer/Dockerfile` you **must** verify the image still builds before finalising your recommended changes.

### Run the test locally

```bash
docker build -f .devcontainer/Dockerfile .
```

## Commit Messages

When creating commits or generating commit messages, follow the conventions in [COMMITS.md](COMMITS.md).

# Agent Rules <!-- tessl-managed -->

@.tessl/RULES.md follow the [instructions](.tessl/RULES.md)
