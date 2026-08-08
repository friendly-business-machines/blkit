---
name: Project Directory Structure
description: Repository directory layout — Go implementation, docs, scripts, and copier templates
status: implemented
---

# Project Directory Structure

blkit is a Go library. The repository root holds no Go source — the core package lives in `core/`, and the optional infrastructure packages are sibling directories. The following top-level entries are present:

```
blkit/
├── go.mod
├── go.sum
├── core/             # package core, imported as `bl` — value types, expression engine,
│                     #   decision models, process classes, and data contracts in one
│                     #   package (see specs/expressions/, specs/decision-tasks/, etc.)
├── stores/           # One Go module per state-store backend: badger, bbolt, mariadb,
│                     #   mssql, mysql, nats, pebble, postgres, sqlite, turso
├── brokers/          # One Go module per message-broker backend: redis, nats, rabbitmq,
│                     #   azure-service-bus, google-pubsub, aws-sqs-sns
├── worker/           # blkit/worker — worker.Run, ProcessTask lifecycle, writer pool
├── restserver/       # blkit/restserver — HTTP REST + SSE server for the MessageBroker
├── docs/             # Static documentation site source files (Zensical)
├── internal/
│   └── doctest/      # Test driver and external acceptance fixtures for Go code
│                     #   assembled from progressive docs/examples/*.md blocks
├── scripts/          # Automation scripts (create-pull-request.sh, create-release.sh, etc.)
├── copier/           # Copier project templates
├── specs/            # Interface Specifications (this directory)
├── Makefile          # Root-level make targets (make pr, make release, etc.)
├── VERSION           # Current version of the library
├── zensical.toml     # Zensical documentation site configuration
├── README.md         # Project overview and quick-start
├── LICENSE           # Project licence
├── AGENTS.md         # Instructions for AI coding agents working in this repository
├── .gitignore                 # Repository ignore rules for generated and local-only files
├── .pre-commit-config.yaml    # Pre-commit hook configuration (formatter, linter)
└── .github/                   # GitHub Actions workflows and repository configuration
```

Test files live alongside source as `*_test.go`.

The value types, expression engine, decision models, process classes, and data contracts
all live in the `core/` package — `package core`, imported as `bl` (see
[specs/expressions/](../expressions/)). The other top-level code entries
(`blkit/worker`, `blkit/restserver`, and the per-backend modules under `stores/` and
`brokers/`) are separate sibling packages that import `core`.

## `docs/`

Contains the source files for the blkit documentation site, compiled by [Zensical](https://github.com/zensical/zensical) into a static site hosted on GitHub Pages. See [project-documentation.spec.md](project-documentation.spec.md) for full details.

```
docs/
├── index.md                  # Site home page
├── getting-started/          # Orientation for new users, incl. installation
├── tutorials/                # Guided, narrative walkthroughs
├── templates/                # Project scaffolds and boilerplate patterns
├── examples/                 # Focused, self-contained code samples
└── reference/                # API reference (generated from godoc comments)
```

Hand-authored Markdown lives in all directories except `reference/`, whose contents are programmatically generated from source and must not be edited by hand.

Available example implementations are assembled directly from marked, progressive
Go blocks in `docs/examples/*.md`. The test-only driver and external acceptance
fixtures live under `internal/doctest/`; no extracted Go implementation is
committed. See
[project-documentation.spec.md](project-documentation.spec.md#executable-examples-and-verification)
for the source-block and verification contract.

## `internal/doctest/`

Contains the standard-library-only test driver for executable documentation
examples. Its Go-ignored `testdata/<example-name>/example_test.go` fixtures provide
direct business-logic acceptance tests and black-box command tests without
reproducing the Markdown-defined application source. The driver assembles source
and runs the fixtures in temporary directories as part of `go test ./...`.

## `.github/`

Contains GitHub Actions workflow definitions and repository configuration.

```
.github/
├── workflows/
│   ├── pull-request-checks.yml  # PR checks workflow — Go build/format/lint/test + docs job
│   ├── docs.yml     # Docs publish workflow — builds and deploys the Zensical site to GitHub Pages (see project-documentation.spec.md)
│   └── publish.yml  # Publish workflow — triggers pkg.go.dev on GitHub Release creation (see release-workflow.spec.md)
└── CODEOWNERS       # Code ownership rules controlling required reviewers per path
```

## `scripts/`

Contains shell scripts that codify automation workflows:

- `scripts/create-pull-request.sh` — validates the current branch and opens a pull request, invoked by `make pr` or the `/pr` prompt
- `scripts/create-release.sh` — release process (see [release-workflow.spec.md](release-workflow.spec.md))
- `scripts/generate-docs.sh` — regenerates programmatically generated API reference Markdown in `docs/reference/` (see [project-documentation.spec.md](project-documentation.spec.md))
- `scripts/generate-llms-txt.sh` — regenerates the `docs/llms.txt` and `docs/llms-full.txt` LLM discovery files from `zensical.toml` and the docs pages (see [project-documentation.spec.md](project-documentation.spec.md))

## `copier/`

Contains [Copier](https://copier.readthedocs.io/) project templates, enabling developers to scaffold new blkit-based projects from a canonical starting point.
