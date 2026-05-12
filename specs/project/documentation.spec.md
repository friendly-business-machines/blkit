---
name: Documentation
description: Documentation site structure, toolchain, and authoring conventions — compiled from Markdown using Zensical and hosted on GitHub Pages, including auto-generated Go API reference
targets:
  - ../docs/**/*.md
  - ../docs/**/*.yml
  - ../docs/**/*.yaml
  - ../scripts/generate-docs.sh
  - ../.github/workflows/docs*.yml
---

# Documentation

blkit project documentation is compiled from Markdown using the [Zensical](https://github.com/zensical/zensical) framework into a static site hosted on GitHub Pages.

## Site Structure

The documentation site is organised into the following top-level sections (at minimum):

| Section | Purpose |
|---|---|
| **Getting Started** | Orientation for new users — what blkit is and a minimal working example |
| **Installation** | Installation instructions (`go get` command, version requirements) |
| **Tutorials** | Guided, narrative walkthroughs that build a working artefact step by step |
| **Templates** | Ready-to-copy project scaffolds and boilerplate patterns for common use cases |
| **Examples** | Focused, self-contained code samples demonstrating specific features or combinations |
| **Reference** | Complete API reference, organised by package and type |

Additional top-level sections may be added as the project grows, but the six sections above are required before the documentation site is considered minimally complete.

## Getting Started Section

The Getting Started section is a single page (or a tightly grouped sequence of no more than three short pages) that gives a new user enough context to understand what blkit is and to run a working example in under ten minutes. It is the first thing most people will read and must stay current with every library release.

### Required Content

The following topics must all be covered, in this order:

#### 1. What blkit is

A concise, jargon-light explanation of blkit's purpose: a Go SDK for executing business rules and processes, drawing inspiration from DMN (Decision Model and Notation) and BPMN (Business Process Model and Notation). The explanation must be accessible to a developer who has not previously worked with DMN or BPMN.

#### 2. When to use blkit

A brief description of the problem blkit solves — specifically, the pain of encoding business logic as procedural code that becomes hard to change when rules evolve — and when blkit is a good fit versus when it is not.

#### 3. Prerequisites

The minimum requirements a developer must meet before following the quick-start steps. At minimum, this covers the required Go version and how to verify it is installed: **Go 1.21 or later**.

#### 4. Installation

The `go get` command to add blkit to a project. This is a brief forward-reference to the Installation section for full options and version pinning guidance.

#### 5. Minimal working example

A self-contained, runnable example that demonstrates the core loop: define a decision or process, execute it with blkit, and inspect the result. The example must:

- use a real-world-flavoured scenario (not `foo`/`bar`) such as a simple loan eligibility check or a shipping cost calculation
- include zero boilerplate not directly relevant to blkit — no framework setup, no database connections, no HTTP server
- produce visible output (e.g. a printed result) so the reader can confirm it ran correctly
- be tested in CI to ensure it compiles and produces the expected output

#### 6. Next steps

Three or four signposts pointing deeper into the documentation:

- Link to **Tutorials** for a guided walkthrough of a complete use case
- Link to **Examples** for a library of focused feature demonstrations
- Link to **Reference** for full API documentation
- Link to the relevant spec repository or a DMN/BPMN overview if the reader wants context on the standards blkit draws from

### Staleness Policy

The Getting Started page(s) must be reviewed and, if necessary, updated as part of every library release. The release checklist (see `specs/project/release-workflow.spec.md`) includes a step requiring confirmation that the minimal working example compiles and runs against the release version.

## Examples Section

The Examples section of the documentation site contains one page per business process example. Each page presents the business process in plain language and then shows a complete, working implementation of that process using blkit.

### Business Process Specs

Each example is defined at the business level in a spec file under `specs/examples/`. These specs describe what the process does — its input data, decision rules or process steps, outcomes, and worked examples — without any reference to code or implementation details. They are the authoritative description of the business problem that the implementation must solve.

The ten examples currently defined in `specs/examples/` are:

| Spec file | Business process |
|---|---|
| `loan-eligibility.spec.md` | Retail bank personal loan eligibility decision |
| `shipping-rate.spec.md` | E-commerce parcel shipping cost calculation |
| `order-fulfillment.spec.md` | E-commerce order handling from placement to delivery |
| `expense-approval.spec.md` | Employee expense claim approval routing |
| `product-pricing.spec.md` | SaaS subscription pricing with compounding discounts |
| `invoice-processing.spec.md` | Accounts payable invoice validation and payment scheduling |
| `employee-onboarding.spec.md` | Parallel new-hire setup across IT, HR, and Facilities |
| `insurance-claim.spec.md` | Motor insurance claim assessment and settlement calculation |
| `customer-discount.spec.md` | Best-discount selection across multiple applicable rules |
| `product-return.spec.md` | Product return request handling and refund or replacement routing |

### Page Structure

Each `docs/examples/<name>.md` page is structured as follows:

1. **Business overview** — a brief plain-English summary of the process, drawn from the corresponding spec.
2. **Implementation** — a self-contained, runnable Go implementation of the example using blkit.
3. **Notes** — any caveats, limitations, or points of interest.

### Implementation File Layout

Each example implementation is a Markdown file under `docs/examples/`:

```
docs/examples/<name>.md
```

The Zensical build renders this directly as the example page. If the implementation file is missing, the page is excluded from the site and a warning is emitted at build time.

## Reference Section

The Reference section contains the Go API reference, generated from `//` godoc comments in source via `go doc` / `godoc` → Markdown.

Generated Markdown is committed to `docs/reference/` and consumed by Zensical as ordinary source files. The generation step runs in CI on every release.

## `scripts/generate-docs.sh`

`scripts/generate-docs.sh` is the entry point for regenerating the API reference Markdown. It runs the godoc-to-Markdown generation tool and writes the output into `docs/reference/`.

The script is invoked in three contexts:

- **Locally** — by `scripts/create-pull-request.sh` and `scripts/create-release.sh` as part of the docs regen verification step, to check that committed reference docs are not stale.
- **CI** — by the `docs` job in the pull request workflow, which diffs the regenerated output against what is committed and fails if there is any discrepancy.
- **Manually** — by a developer who has updated doc comments in source and wants to regenerate and commit the updated reference Markdown.

The script exits with a non-zero status and a structured error message if generation fails.

## Toolchain

- **Zensical** — the static site compiler. Configuration lives in `zensical.toml` at the repository root.
- **GitHub Pages** — hosting. The compiled site is published from the `gh-pages` branch or a configured Pages source.
- **GitHub Actions** — CI/CD. A workflow (`.github/workflows/docs.yml`) builds and publishes the site on every push to the default branch and on every release tag.

## Authoring Conventions

- All hand-authored documentation lives under `docs/`.
- Reference docs under `docs/reference/` are generated — they must not be edited by hand.
- Each page should have a clear, single-sentence purpose stated at the top.
- Code examples in hand-authored pages must be valid Go.
- The Getting Started and Installation pages must be kept current with every library release — they are the highest-traffic entry points and staleness there causes disproportionate confusion.

## CI Requirements

The documentation workflow must:

1. Install Zensical and the godoc-to-Markdown generation toolchain.
2. Run source-to-Markdown generation for the Reference section.
3. Run `zensical build` (or equivalent) to compile the full site.
4. Fail the build if any broken internal links are detected.
5. Publish the compiled site to GitHub Pages only on pushes to the default branch or release tags (not on pull requests).
