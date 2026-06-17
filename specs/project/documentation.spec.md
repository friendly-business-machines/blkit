---
name: Documentation
description: Documentation site structure, toolchain, and authoring conventions — compiled from Markdown using Zensical and hosted on GitHub Pages, including auto-generated Go API reference and llms.txt discovery files
targets:
  - ../docs/**/*.md
  - ../docs/**/*.yml
  - ../docs/**/*.yaml
  - ../docs/llms.txt
  - ../docs/llms-full.txt
  - ../scripts/generate-docs.sh
  - ../scripts/generate-llms.sh
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

The eleven examples currently defined in `specs/examples/` are:

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
| `aus-personal-income-tax.spec.md` | Australian personal income tax calculation (FY 2024–25) |

### Page Structure

Each `docs/examples/<name>.md` page is structured as follows:

1. **Business overview** — a plain-English presentation of the process drawn from the corresponding spec: the overview, input/trigger data, the decision rules or process steps, outcomes, and worked examples. The page should reproduce the spec's natural sections so it stands alone on the documentation site, rather than only linking out. Decision-style and process-style examples will not share identical headings.
2. **Implementation** — a self-contained, runnable Go implementation of the example using blkit. While the `decisions` / `processes` packages an example depends on are still being built, this section is a clearly-marked **pending placeholder** (an admonition explaining the dependency and linking to the authoritative spec) instead of code. Pages are published in this pending state; the runnable implementation replaces the placeholder once the required packages land.
3. **Notes** — any caveats, limitations, or points of interest.

### Implementation File Layout

Each example implementation is a Markdown file under `docs/examples/`:

```
docs/examples/<name>.md
```

The Zensical build renders this directly as the example page. A page is published as soon as its file exists — including when its **Implementation** section is still the pending placeholder described above. If the file is missing entirely, the page is excluded from the site and a warning is emitted at build time.

## Reference Section

The Reference section contains the Go API reference, generated from `//` godoc comments in source via `go doc` / `godoc` → Markdown.

Generated Markdown is committed to `docs/reference/` and consumed by Zensical as ordinary source files. The generation step runs in CI on every release.

## LLM Discovery Files

The site publishes two files for large-language-model consumers, following the
[`llms.txt` convention](https://llmstxt.org/):

| File | Purpose |
|---|---|
| `docs/llms.txt` | A concise, curated **index** of the documentation — an H1 site name, a one-line summary, and grouped links to every page with short descriptions. |
| `docs/llms-full.txt` | The **full text** of the documentation concatenated into a single file, so a model can ingest everything in one fetch. |

Both files are generated, committed, and copied verbatim to the site output by
Zensical (non-Markdown files under `docs_dir` are passed through unchanged).
Because the site is served from a project GitHub Pages subpath, they are reached
at `<site_url>llms.txt` and `<site_url>llms-full.txt` (i.e. under `/blkit/`),
not at the domain root.

### Content Requirements

- **`llms.txt`** is derived from `zensical.toml`:
  - An HTML-comment generated-by banner on the first line.
  - An H1 with the configured `site_name`, followed by a blockquote with the
    `site_description`.
  - One H2 section per nav group. Top-level single-page nav entries are grouped
    under a **Documentation** heading; each nav entry whose value is a list
    (e.g. Examples, Reference) becomes its own H2 section.
  - Each link is an absolute URL built from `site_url`, using Zensical's
    directory-URL form (`examples/loan-eligibility.md` → `examples/loan-eligibility/`,
    `index.md` → the section root). Link titles come from the nav label for
    top-level pages and from the page's first H1 for list children. Where a page
    opens with a blockquote summary, it is appended after the link as a
    description.
- **`llms-full.txt`** concatenates the raw Markdown of **every** page in nav
  order, **including** the generated API Reference, each preceded by a
  `<!-- Source: <url> -->` marker. Including the Reference is deliberate: the
  API signatures are the highest-value content for a model assisting with blkit
  code.

### Generation and Staleness

Both files are produced by `scripts/generate-llms.sh` (see below), committed to
`docs/`, and **must not be edited by hand**. Their freshness is enforced the
same way as the Reference Markdown: a pre-commit hook regenerates them when
documentation source or `zensical.toml` changes, and the `docs` CI job
regenerates and diffs them, failing if the committed copy is stale. Because
`llms-full.txt` embeds the generated Reference, generation runs *after*
`scripts/generate-docs.sh`.

## `scripts/generate-llms.sh`

`scripts/generate-llms.sh` is the entry point for regenerating `docs/llms.txt`
and `docs/llms-full.txt`. It reads `zensical.toml` for the site metadata and nav
and reads the referenced pages under `docs/`, then writes the two files. It
requires `python3` with the `tomllib` module (Python 3.11+) to parse the
configuration; it exits non-zero with a structured error message if that
prerequisite is missing or if a nav page cannot be found.

The script is invoked in the same three contexts as `scripts/generate-docs.sh`
(local pre-pull-request / pre-release verification, CI staleness check, and
manual regeneration after editing docs).

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
3. Run `scripts/generate-llms.sh` to regenerate the `llms.txt` discovery files.
4. Fail the build if the regenerated Reference Markdown or `llms.txt` files
   differ from what is committed (a staleness check).
5. Run `zensical build` (or equivalent) to compile the full site.
6. Fail the build if any broken internal links are detected.
7. Publish the compiled site to GitHub Pages only on pushes to the default branch or release tags (not on pull requests).
