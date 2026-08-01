---
name: <GoTypeOrModuleName>
description: <one line — what this spec covers>
status: <draft | agreed | implemented | superseded>
code:
  - <repo-root-relative file or directory>
# implements: specs/<family>/overview.spec.md    # backend specs only
# superseded-by: specs/<path>.spec.md            # only when status: superseded
---

# {Name}

## Purpose

<!-- All specs. What it is, why it exists, where it sits in the architecture.
     1–3 paragraphs. -->

## Design

<!-- Contract/module: required. Backend: dependency choice & storage/primitive
     mapping only. Decision-per-bullet, each with rationale — what was chosen
     and why the obvious alternative was not.
     Add ###-level subsections only where there is content, in this order:
       ### Structure               — architectural choices, module layout & placement
       ### Dependencies            — technology selection and why
       ### Standards               — alignment with / divergence from BPMN, DMN, FEEL, ISO 8601, …
       ### Errors & Failure Model
       ### Non-Goals               — rejected options, deliberately unsupported behaviour
       ### Principles -->

## API Contract

<!-- Contract: the interface. Module: the full public API. Backend: Config &
     constructor only. Go-pseudocode block per specs/overview.spec.md
     § Interface Specification Format.
     Add ###-level subsections only where there is content:
       ### Configuration & Construction — Config struct, constructors, defaults
       ### Wiring                       — how a caller wires the module into the
                                          wider system: the import path, who
                                          constructs it, and what it is handed to
                                          (e.g. constructing a store and passing
                                          it to worker.Run) -->

```go
```

## Behaviour

<!-- Contract/module: normative semantics — invariants, ordering, concurrency,
     lifecycle. Backend: ONLY the mapping onto this backend's primitives and
     divergences from the contract; link the contract, don't restate it. -->

## Edge Cases

<!-- Bulleted, individually testable statements of boundary behaviour. -->

## Verification

<!-- Contract: the conformance suite. Module: test links. Backend: how the
     suite runs for this backend (embedded / temp dir / testcontainers / DSN
     override).
     Plain links: Verified by [foo_test.go](../../core/foo_test.go). -->
