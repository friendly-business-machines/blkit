---
name: Documentation
description: Documentation site structure, toolchain, and authoring conventions — compiled from Markdown using Zensical and hosted on GitHub Pages, including auto-generated Go API reference and llms.txt discovery files
status: implemented
code:
  - docs/
  - internal/doctest/
  - scripts/generate-docs.sh
  - scripts/generate-llms-txt.sh
  - .github/workflows/docs.yml
  - .github/workflows/pull-request-checks.yml
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
| **Expressions** | User guide to the expression language — an overview page plus one page per language area (numbers, strings, dates and times, lists, …) |
| **Decisions** | User guide to the decision components (decision expressions, tables, native functions, sub-decisions, reference data, decision tasks) |
| **Processes** | User guide to the process components (tasks, gateways, processes) |
| **State Stores** | User guide to the pluggable state-store backends — an overview plus one page per backend |
| **Message Brokers** | User guide to the pluggable message-broker backends — an overview plus one page per backend |
| **Architecture** | Internal design of blkit's subsystems — one deep-dive chapter per subsystem |

The first six sections in the table are required before the documentation site is considered minimally complete. **Expressions**, **Decisions**, **Processes**, **State Stores**, and **Architecture** are additional sections that grow as the corresponding capabilities are documented: the four user-guide sections (Expressions, Decisions, Processes, State Stores) describe how to *use* each component, while the Architecture section describes how each is *built*.

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

Like the Architecture section, the Examples section has no landing page of its own; the nav group header is a non-clickable grouping over the example pages. Each example page's H1 is the business process name alone, with no `Example:` prefix, so the nav sidebar stays uncluttered.

### Business Process Specs

Each example is defined at the business level in a spec file under `specs/examples/`. These specs describe what the process does — its input data, decision rules or process steps, outcomes, and worked examples — without any reference to code or implementation details. They are the authoritative description of the business problem that the implementation must solve.

The eleven examples currently defined in `specs/examples/` are:

| Spec file | Business process |
|---|---|
| `admission.spec.md` | University undergraduate course admission decision |
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
2. **Implementation** — progressive, runnable Go for every part of the example that current blkit APIs can express. Functionality requiring an unfinished module has no Go block yet and is identified in ordinary prose without preventing available code on the same page from being compiled and tested.
3. **Notes** — any caveats, limitations, or points of interest.

### Implementation File Layout

Each example implementation is a Markdown file under `docs/examples/`:

```
docs/examples/<name>.md
```

The Zensical build renders this directly as the example page. A page is published as soon as its file exists, whether or not current blkit APIs can yet express all of its behavior. If the file is missing entirely, the page is excluded from the site and a warning is emitted at build time.

### Executable Examples and Verification

Every implemented Go fragment in an example page is a first-class CI check.
Functionality whose blkit API is not yet available has no Go fragment; the page
may still explain that unavailable portion in prose. Go implementation remains
solely in the Markdown page: no generated or hand-copied `.go` implementation is
committed elsewhere in the repository.

#### Progressive source blocks

A page teaches available implementation progressively. Each step may explain a
design choice and then contribute another fragment of the program in a marked Go
fence. The outer four-backtick `markdown` fence below only displays the required
authoring syntax inside this spec; an example page contains only the inner
three-backtick Go fence.

````markdown
``` { .go .blkit-example title="main.go" }
// The next fragment of the application.
```
````

The `.blkit-example` class is the executable-source marker. Ordinary `go` fences
without that class are illustrative and are not part of the executable program.
Arbitrary custom fence options are not used because Zensical does not preserve
them reliably; custom classes are supported and render as normal highlighted Go.

The test driver concatenates the bodies of all `.blkit-example` Go fences in
document order, separated by newlines, to form one temporary `main.go`. Every
marked fence on a page must use `title="main.go"`. Version 1 deliberately supports
one generated Go source file per example; multi-file tangling is a non-goal until
a real example requires it. The driver may add Go `//line` directives between
fragments so compiler diagnostics identify the originating Markdown path and
line, but it must not otherwise transform the source.

The assembled source must be a complete `package main` application. Its
`main()` accepts input from an external boundary appropriate to the example,
such as command-line arguments or standard input; it must not contain hardcoded
acceptance cases or expected results. Reusable business logic is kept in
functions that the external tests can call directly. This keeps the displayed
code representative of an application a reader could adapt rather than turning
`main()` into a test driver.

A page with one or more marked fences has one matching external acceptance test
fixture. A page without marked fences has no fixture and is skipped by the Go
example runner. A fixture without marked source, or marked source without a
fixture, is invalid. An unclosed marked fence, a marked non-Go fence, a marked
fence with another title, or source that does not parse and compile is also a
test failure.

There is no separate page status or completeness metadata. Missing blkit
functionality is represented by the absence of source for that behavior, not by
pseudocode. Tests cover only behavior implemented by the marked blocks and must
not claim that omitted process or worker side effects occurred.

#### External acceptance tests

Tests are intentionally outside the Markdown so the reader-facing application
contains no embedded test cases. They live under the test driver's Go-ignored
fixture tree (`testdata/` is not discovered as a package by `go test ./...`):

```
internal/doctest/testdata/<example-name>/example_test.go
```

These files contain tests only. They must not reproduce, wrap, or complete the
implementation from the Markdown page. At test time the driver copies the
matching test into the temporary directory beside the assembled `main.go` and
runs them as one `package main`.

Every example containing marked source is verified at both levels:

1. **Direct logic acceptance tests** call the assembled application's reusable
   business-logic functions. They are table-driven and cover every worked row in
   the corresponding `specs/examples/<name>.spec.md`, including returned values
   and documented errors.
2. **Black-box application tests** execute the built temporary command with
   externally supplied input and verify its exit status, standard output, and
   relevant standard error. At least one worked scenario is exercised through
   this boundary, proving that `main()` connects application input and output to
   the tested business logic.

Top-level direct tests use the `TestLogic` name or prefix; black-box tests use
`TestCommand`. The driver builds the assembled command before running tests and
provides its path in `BLKIT_EXAMPLE_BINARY`, so command tests execute the exact
source extracted from the page. Acceptance values live in the business spec and
external tests, never in the executable Go fences. The documentation page may
still present worked inputs and outputs in its explanatory prose or tables.

#### Test-driver behaviour

The standard-library-only driver lives in `internal/doctest/` and is itself
reached by `go test ./...`. It discovers every `docs/examples/*.md` page and
matches each page containing marked source to
`internal/doctest/testdata/<name>/example_test.go`. For each tested page it:

1. extracts and assembles the marked fragments in a fresh temporary directory;
2. parses and compiles the assembled package;
3. builds the temporary application binary;
4. runs the external direct and black-box tests with
   `BLKIT_EXAMPLE_BINARY` set; and
5. reports failures against the example page, preserving Markdown line
   attribution where Go tooling permits it.

Missing or extra test fixtures, duplicate page identities, extraction errors,
compile errors, build errors, absent direct or command tests, runtime failures,
and assertion failures all fail the suite. Temporary source and binaries are
removed by the test lifecycle and are never committed or used as Zensical input.
Pages without marked source are ignored.

Repository-authored Markdown and its external tests are trusted executable code.
Pull requests changing either receive the same review and CI treatment as Go
source changes; the driver does not execute Markdown obtained from untrusted
runtime input.

The approach was selected after disposable feasibility checks of native Go
`Example` blocks, complete `package main` blocks with embedded or adjacent
expected output, Zensical Markdown Exec, progressive source tangling, and
external tests. Progressive tangling with external direct and black-box tests
was chosen because it keeps the portal tutorial-shaped, keeps production logic
single-sourced in Markdown, keeps acceptance data out of the displayed program,
and still verifies both the business API and the runnable application boundary.

## Reference Section

The Reference section contains the Go API reference, generated from `//` godoc comments in source via `go doc` / `godoc` → Markdown.

Generated Markdown is committed to `docs/reference/` and consumed by Zensical as ordinary source files. The generation step runs in CI on every release.

The section covers every user-facing buildable package in the workspace, not only the `core` module. Internal test/tooling packages such as `internal/doctest` are excluded. Because each state-store backend under `stores/<name>/` and each message-broker backend under `brokers/<name>/` is its own Go module (see the State Stores and Message Brokers Sections below), `go list ./...` on its own does not reach them; `scripts/generate-docs.sh` additionally discovers each backend module through the workspace and documents it. The `core` package is written as `reference/blkit.md`; each state-store module as `reference/stores-<name>.md` (e.g. `reference/stores-postgres.md`) and each broker module as `reference/brokers-<name>.md` (e.g. `reference/brokers-redis.md`). All of these pages are wired into the Reference nav group.

## Expressions Section

The Expressions section is the user guide to blkit's expression language — the same engine documented internally in the Architecture section, but presented from the author's point of view. It opens with an orientation **overview** page and is then organised as one page per area of the language, each combining prose, syntax, and worked snippets. Each per-area page's behaviour is defined authoritatively by the matching spec under `specs/expressions/`; the `overview.md` page has no single matching spec — it is an orientation page that introduces what the language is (mirroring the Architecture chapter's framing) and catalogues the closed set of value types, linking out to each area's page.

Pages: `overview.md`, `numbers.md`, `strings.md`, `booleans-and-logic.md`, `dates-and-times.md`, `lists.md`, `dictionaries.md`, `ranges.md`, `tables.md`.

## Decisions Section

The Decisions section is the user guide to blkit's decision components. Its pages are published as placeholders until the decisions package lands; each page's behaviour is defined authoritatively by the matching spec under `specs/decision-tasks/`.

Pages: `decision-expressions.md`, `decision-tables.md`, `decision-native-fn.md`, `sub-decisions.md`, `reference-data.md`, `decision-tasks.md`.

## Processes Section

The Processes section is the user guide to blkit's process components — tasks, gateways, and whole processes. Its pages are published as placeholders until the process and worker packages land.

Pages: `tasks.md`, `gateways.md`, `processes.md`.

## State Stores Section

The State Stores section is the user guide to blkit's pluggable state-store backends — where a process instance's state (the values its tasks produce and its execution history) is kept. It describes how to *use* and *choose* a backend, not the internal design.

It opens with an orientation **overview** page (what a state store is, what every backend does, how to choose one, and the shared conformance guarantee) and is then organised as one page per backend. Each page's behaviour is defined authoritatively by the matching spec under `specs/state-stores/`: the overview by `overview.spec.md`, and each backend page by its `<name>-state-store.spec.md`. The pages are hand-authored user-guide prose, so they omit the specs' status metadata, Go interface details, and test links; those live in the specs and (for the API surface) in the generated Reference pages, which each backend page links to.

Pages: `overview.md`, `in-memory.md`, `postgres.md`, `mssql.md`, `mysql.md`, `mariadb.md`, `nats.md`, `sqlite.md`, `bbolt.md`, `badger.md`, `pebble.md`.

The in-memory backend is built into core, so its API appears in `reference/blkit.md`; every other backend is a separate module with its own `reference/stores-<name>.md` page (see the Reference Section above).

## Message Brokers Section

The Message Brokers section is the user guide to blkit's pluggable message-broker backends — the channel over which clients and workers exchange messages (worker registration, the process job queue, start and cancel requests, input requests and responses, and process outcomes). It describes how to *use* and *choose* a backend, not the internal design, and stresses that the broker carries *messages about* a run while the run's state lives in the state store.

It opens with an orientation **overview** page (what a broker does and does not do, how to choose one, and the shared conformance guarantee) and is then organised as one page per backend. Each page's behaviour is defined authoritatively by the matching spec under `specs/message-brokers/`: the overview by `overview.spec.md`, and each backend page by its `<name>-message-broker.spec.md`. The pages are hand-authored user-guide prose, so they omit the specs' status metadata, Go interface details, and test links; those live in the specs and (for the API surface) in the generated Reference pages, which each backend page links to.

Pages: `overview.md`, `in-memory.md`, `redis.md`, `nats.md`, `rabbitmq.md`, `azure-service-bus.md`, `google-pubsub.md`, `aws-sqs-sns.md`.

The in-memory backend is built into core, so its API appears in `reference/blkit.md`; every other backend is a separate module with its own `reference/brokers-<name>.md` page (see the Reference Section above). The three cloud-managed backends (Azure Service Bus, Google Pub/Sub, AWS SQS/SNS) additionally ship a pluggable `RegistryStore` for the worker registry, timers, and last-event records, documented on the same page.

## Architecture Section

The Architecture section explains how blkit is built internally — the design of the engine behind the public API. It complements the Reference section (*what* the API is) and the Examples section (*how* to use it) by documenting *why the machinery is shaped the way it is*: the pipelines, intermediate representations, and design decisions. Its audience is contributors and anyone debugging behaviour that only makes sense with knowledge of the internal layers.

The section is a set of chapter pages, one per subsystem; it has no landing page of its own, so the nav group header is a non-clickable grouping. Each chapter is a self-contained deep-dive of hand-authored prose (unlike the generated Reference section). A chapter may be published as a clearly-marked placeholder before the subsystem it documents is implemented.

| Page | Subsystem |
|---|---|
| `docs/architecture/expressions.md` | The expression engine — how blkit extends the Expr (`expr-lang/expr`) project into a FEEL-like language: the compilation pipeline (normalise → parse → patch → compile → run), the value system, and the typed API |
| `docs/architecture/worker-pools.md` | The worker-pool execution model (placeholder until the process and worker packages land) |

Decisions, processes, and further subsystems will each gain a chapter as those packages land.

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
    directory-URL form (`examples/admission.md` → `examples/admission/`,
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

Both files are produced by `scripts/generate-llms-txt.sh` (see below), committed to
`docs/`, and **must not be edited by hand**. Their freshness is enforced the
same way as the Reference Markdown: a pre-commit hook regenerates them when
documentation source or `zensical.toml` changes, and the `docs` CI job
regenerates and diffs them, failing if the committed copy is stale. Because
`llms-full.txt` embeds the generated Reference, generation runs *after*
`scripts/generate-docs.sh`.

## `scripts/generate-llms-txt.sh`

`scripts/generate-llms-txt.sh` is the entry point for regenerating `docs/llms.txt`
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

The generation tool targets GitHub-flavored Markdown and defensively backslash-escapes parentheses in headings and link text (e.g. `func \(BlBoolean\) IsNull`). GitHub renders `\(` as a bare `(`, but Zensical's Markdown engine leaves the backslash in heading plain text, so the literal slashes would otherwise leak into the rendered page. The script therefore post-processes each generated file to un-escape parentheses (`\(` → `(`, `\)` → `)`). This is scoped to parentheses only: they carry no Markdown meaning in the positions they appear here, whereas brackets, angle brackets and backticks are load-bearing inside link text and code spans and must remain escaped.

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
3. Run `scripts/generate-llms-txt.sh` to regenerate the `llms.txt` discovery files.
4. Fail the build if the regenerated Reference Markdown or `llms.txt` files
   differ from what is committed (a staleness check).
5. Assemble every marked Go implementation in the Examples section, run its
   external direct and black-box tests, and verify the behavior represented by
   that source against the corresponding business-process spec.
6. Run `zensical build` (or equivalent) to compile the full site.
7. Fail the build if any broken internal links are detected.
8. Publish the compiled site to GitHub Pages only on pushes to the default branch or release tags (not on pull requests).
