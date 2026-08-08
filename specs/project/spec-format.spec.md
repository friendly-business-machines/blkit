---
name: Spec Format
description: The format and lifecycle of blkit's living, module-aligned specification documents — frontmatter schema, spec roles, section skeleton, and authoring rules
status: implemented
---

# Spec Format

## Purpose

Specs in `specs/` are **living documents aligned to code modules** — Go packages,
backend modules, and core types — not one-off feature documents. Each spec is the
authoritative statement of what its module is for, the design decisions behind it,
its public API, and its behaviour, maintained for the life of the project. A spec
is updated in the same change as the code it governs; where spec and code disagree,
the spec is either fixed or the code is wrong.

This document defines the format. The authoring and maintenance workflow lives in
the `spec` skill (`.claude/skills/spec/`).

## Design

- **Module-aligned, not feature-aligned** — every Go package or module is covered
  by exactly one spec. Features change specs; they do not create parallel
  documents.
- **Roles are inferred, not declared** — what a spec is (contract, module spec,
  backend spec, project convention) follows from where it lives and whether it
  `implements:` a contract. There is no `kind`/`type` field to keep in sync.
- **Lifecycle in frontmatter, not prose** — status (`draft`/`agreed`/
  `implemented`/`superseded`) is a machine-checkable field, never a blockquote
  banner in the body.
- **Root-relative code paths** — the `code:` field resolves from the repository
  root, so paths cannot silently rot the way spec-relative `../..` paths do.
- **One authoritative home per fact** — normative content lives in exactly one
  spec; everything else links to it.
- **Self-hosted** — this format definition is itself a spec, versioned with the
  repo and agent-agnostic.

## File layout & naming

```
specs/<area>/<name>.spec.md
```

- Files use lowercase-hyphenated names and the `.spec.md` extension.
- `<area>` groups a family: `expressions/`, `decision-tasks/`, `processes/`,
  `data/`, `state-stores/`, `message-brokers/`, `worker/`, `rest/`, `mcp/`,
  `examples/`, `project/`, plus the repo-wide `specs/overview.spec.md`.
- A family whose members share an interface has an `overview.spec.md` holding
  the shared contract; sibling files hold one implementation each.

## Frontmatter

```yaml
---
name: RedisMessageBroker
description: One line, used for relevance decisions when scanning the spec index
status: implemented      # draft | agreed | implemented | superseded
code:                    # repo-root-relative files or directories
  - brokers/redis/
implements: specs/message-brokers/overview.spec.md   # backend specs only
superseded-by: specs/state-stores/overview.spec.md   # only when status: superseded
---
```

| Field | Required | Meaning |
|---|---|---|
| `name` | always | Short display name, usually the Go type or module name |
| `description` | always | One line; what the spec covers |
| `status` | always | Lifecycle state, see below |
| `code` | code-shaped specs (optional for `project/*`) | Repo-root-relative paths (files or directories) this spec governs |
| `implements` | backend specs | The contract spec this backend implements |
| `superseded-by` | when `status: superseded` | The spec that replaced this one |

### Status lifecycle

| Status | Meaning |
|---|---|
| `draft` | Being shaped; **not authoritative** — do not implement against it |
| `agreed` | Authoritative target; the code may not exist yet (spec-first) |
| `implemented` | Code exists and matches the spec |
| `superseded` | Replaced; follow `superseded-by`. Kept only until inbound references are migrated |

For `draft` and `agreed` specs, `code:` paths name the *intended* location and
need not resolve. For `implemented` specs, every `code:` path must resolve from
the repo root.

## Spec roles

A spec's role — and therefore which sections it needs — is inferred:

| Spec | Role |
|---|---|
| `specs/overview.spec.md` | Repo-wide architecture overview and conventions |
| `specs/project/*` | Project convention or process (testing, releases, this document) |
| `specs/examples/*` | Worked end-to-end example rendered into the docs |
| `specs/<family>/overview.spec.md` | The family's shared **contract**: the interface, its normative behaviour, and its conformance suite |
| Has `implements:` | A **backend spec**: one implementation of a contract, usually its own Go module |
| Anything else | A **module spec**: a self-contained unit — a core type, node family, or package |

## Section skeleton

Sections appear in this canonical order. The matrix shows which are required (✅),
scoped (text), or omitted (—) per role. Architecture overviews, project
conventions, and examples are freeform beyond Purpose — they use whatever
structure fits their content.

| Section | contract | module spec | backend spec |
|---|---|---|---|
| Purpose | ✅ | ✅ | ✅ |
| Design | ✅ | ✅ | deps & mapping |
| API Contract | ✅ the interface | ✅ | Config/ctor only |
| Behaviour | ✅ normative | ✅ | divergences only |
| Edge Cases | ✅ | ✅ | ✅ |
| Verification | ✅ conformance suite | ✅ | how the suite runs |
| Deployment & Operations | — | runnable artifacts only | — |

### Purpose

What it is, why it exists, and where it sits in the architecture. 1–3 paragraphs.

### Design

Decision-per-bullet, each with its rationale — what was chosen and why the obvious
alternative was not. Subsections as applicable, in this order:

- `### Structure` — architectural/structural choices, module layout and placement
- `### Dependencies` — technology selection and why
- `### Standards` — alignment with, or deliberate divergence from, external
  standards (BPMN, DMN, FEEL, ISO 8601, RFC 9557, …)
- `### Errors & Failure Model`
- `### Non-Goals` — rejected options and deliberately unsupported behaviour
- `### Principles`

### API Contract

The public API as a Go-pseudocode block, following the Interface Specification
conventions in [specs/overview.spec.md](../overview.spec.md#interface-specification-format)
— Go-like, not compilable, exported names in `PascalCase`. Subsections as needed:

- `### Configuration & Construction` — Config struct, constructors, defaults
- `### Wiring` — how a caller wires the module into the wider system: the
  import path, who constructs it, and what it is handed to (e.g. constructing a
  store and passing it to `worker.Run`)

### Behaviour

Normative semantics: invariants, ordering, concurrency, lifecycle, scoping.
For a backend spec, this section documents **only** the mapping of the contract
onto the backend's primitives and any divergences — the normative behaviour lives
in the contract spec and is not restated.

### Edge Cases

Bulleted, individually testable statements of boundary behaviour.

### Verification

How conformance is demonstrated: links to the test files covering this spec,
conformance-suite participation, and (for backends) how the suite runs — embedded,
temporary directory, testcontainers, or a `BLKIT_TEST_*_DSN` override. Test links
are plain markdown links: `Verified by [date_test.go](../../core/date_test.go).`
Granular links may also sit inline next to the specific requirement they verify.

### Deployment & Operations

Optional, and only for module specs whose module ships as a **runnable artifact**
(a worker, MCP server, or REST server) rather than a library. Non-normative
operator-facing guidance: packaging the binary into a deployable artifact (e.g. a
container image), deployment configuration, and operational notes (scaling,
graceful-shutdown windows, health checks). Caller-facing construction and handoff
stays in API Contract § Wiring — this section addresses the operator, not the
caller.

## Authoring rules

- **Go surface only.** Errors are `(T, error)`, absence is `nil` / `(nil, nil)` —
  never `raises`, `None`, or idioms from another language.
- **One authoritative home per fact.** Cross-link rather than restate. A contract
  spec may prescribe the section list its backend specs must answer (as
  [message-brokers/overview.spec.md](../message-brokers/overview.spec.md) does
  with its standard questions) — backend specs then follow it.
- **Links are relative and must resolve.** Spec-to-spec and spec-to-code links use
  paths relative to the spec file; only frontmatter `code:` is root-relative.
- **Examples are illustrative, not normative,** unless the spec says otherwise.
- **Specs move with code.** A behaviour or API change updates the spec in the same
  commit/PR; spec-only changes use the `spec` commit type per the
  [commit message conventions](../../.claude/skills/commitmsg/SKILL.md#commit-message-conventions).
- **Superseding:** set `status: superseded` and `superseded-by:`, move any
  still-authoritative content into the successor first, and update inbound links.
  A superseded spec must never be the only home of a live fact.
