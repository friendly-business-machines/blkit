---
name: Project Directory Structure
description: Repository directory layout — Go implementation, docs, scripts, and copier templates
targets:
  - ../**/*.go
  - ../docs/**
  - ../scripts/**
  - ../copier/**
  - ../.pre-commit-config.yaml
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
├── messagegateway/    # blkit/messagegateway — MessageGateway interface and Redis/NATS/in-memory implementations
├── worker/           # blkit/worker — worker.Run, ProcessTask lifecycle, writer pool
├── restserver/       # blkit/restserver — HTTP REST + SSE server for the MessageGateway
├── docs/             # Static documentation site source files (Zensical)
├── scripts/          # Automation scripts (create-pull-request.sh, create-release.sh, etc.)
├── copier/           # Copier project templates
├── specs/            # Interface Specifications (this directory)
├── Makefile          # Root-level make targets (make pr, make release, etc.)
├── VERSION           # Current version of the library
├── zensical.toml     # Zensical documentation site configuration
├── README.md         # Project overview and quick-start
├── LICENSE           # Project licence
├── AGENTS.md         # Instructions for AI coding agents working in this repository
├── .gitignore                 # Ignored files (includes PR_DESCRIPTION.md, RELEASE_NOTES.md, etc.)
├── .pre-commit-config.yaml    # Pre-commit hook configuration (formatter, linter)
└── .github/                   # GitHub Actions workflows and repository configuration
```

Test files live alongside source as `*_test.go`.

The value types, expression engine, decision models, process classes, and data contracts
all live in the `core/` package — `package core`, imported as `bl` (see
[specs/expressions/](../expressions/)). The other top-level code entries
(`blkit/messagegateway`, `blkit/worker`, `blkit/restserver`) are separate sibling packages
that import `core`.

## `docs/`

Contains the source files for the blkit documentation site, compiled by [Zensical](https://github.com/zensical/zensical) into a static site hosted on GitHub Pages. See [documentation.spec.md](documentation.spec.md) for full details.

```
docs/
├── index.md                  # Site home page
├── getting-started/          # Orientation for new users
├── installation/             # Installation instructions
├── tutorials/                # Guided, narrative walkthroughs
├── templates/                # Project scaffolds and boilerplate patterns
├── examples/                 # Focused, self-contained code samples
└── reference/                # API reference (generated from godoc comments)
```

Hand-authored Markdown lives in all directories except `reference/`, whose contents are programmatically generated from source and must not be edited by hand.

## `.github/`

Contains GitHub Actions workflow definitions and repository configuration.

```
.github/
├── workflows/
│   ├── pull-request-checks.yml  # PR checks workflow — Go build/format/lint/test + docs job (see pull-request-workflow.spec.md)
│   ├── docs.yml     # Docs publish workflow — builds and deploys the Zensical site to GitHub Pages (see documentation.spec.md)
│   └── publish.yml  # Publish workflow — triggers pkg.go.dev on GitHub Release creation (see release-workflow.spec.md)
└── CODEOWNERS       # Code ownership rules controlling required reviewers per path
```

## `scripts/`

Contains shell scripts that codify automation workflows:

- `scripts/create-pull-request.sh` — pre-pull-request process (see [pull-request-workflow.spec.md](pull-request-workflow.spec.md))
- `scripts/create-release.sh` — release process (see [release-workflow.spec.md](release-workflow.spec.md))
- `scripts/generate-docs.sh` — regenerates programmatically generated API reference Markdown in `docs/reference/` (see [documentation.spec.md](documentation.spec.md))

## `copier/`

Contains [Copier](https://copier.readthedocs.io/) project templates, enabling developers to scaffold new blkit-based projects from a canonical starting point.
