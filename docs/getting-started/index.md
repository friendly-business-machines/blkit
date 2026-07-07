# Getting started

> Orientation for new users — what blkit is, when to reach for it, how to
> install it, and how to run your first example.

## What blkit is

blkit is a Go SDK for expressing and executing **business logic** — the rules
and processes that decide what your application does, as opposed to the plumbing
that makes it run. You describe a decision (for example, "is this applicant
eligible for a loan?") or a process (for example, "how does an order move from
placement to delivery?") using blkit's typed building blocks, then execute it
and inspect the result.

The design draws on two established standards:

- **DMN** (Decision Model and Notation) — a way of describing decisions as
  data-driven rules rather than nested `if`/`else` statements.
- **BPMN** (Business Process Model and Notation) — a way of describing
  multi-step processes as a graph of tasks, events, and gateways.

You do **not** need any prior experience with DMN or BPMN to use blkit. If you
can write Go, you can write a blkit decision or process; the standards are
simply where the ideas come from.

## When to use blkit

Business rules change. A discount threshold moves, an approval limit is raised,
a new product tier is added. When that logic is hand-written as procedural code
— deeply nested conditionals spread across services — every change means
re-reading, re-editing, and re-testing code that is easy to get subtly wrong.

blkit is a good fit when:

- your business rules are **numerous, data-driven, or change often**, and you
  want them expressed as structured, inspectable definitions rather than buried
  in control flow;
- you need the **same rule evaluated many times** against different inputs;
- you want decisions and processes that can be **read, reviewed, and reasoned
  about** by people who aren't deep in the implementation.

blkit is probably **not** what you need when the logic is trivial (a single `if`
will do), when you require strict conformance to the DMN or BPMN specifications,
or when you want a visual modelling studio rather than a code library.

## Prerequisites

- **Go 1.22 or later.** Check your version with:

  ```bash
  go version
  ```

  If Go is not installed, follow the official
  [Go installation guide](https://go.dev/doc/install).

## Installation

Add blkit to your module. From within your Go module, run:

```bash
go get github.com/friendly-business-machines/blkit
```

This adds blkit to your `go.mod` and downloads the latest released version.

### Pinning a version

For reproducible builds, pin blkit to a specific release tag:

```bash
go get github.com/friendly-business-machines/blkit@v1.2.3
```

Replace `v1.2.3` with the release you want. You can also pin to a branch or a
commit SHA, though released tags are recommended for production use.

### Importing

The core package is imported as `bl`:

```go
import bl "github.com/friendly-business-machines/blkit/core"
```

This single import provides the whole logic layer — value types, the expression
engine, decision models, process classes, and data contracts. The optional
infrastructure packages (for example a broker module such as
`github.com/friendly-business-machines/blkit/brokers/redis`, or
`blkit/restserver`) are imported under their own paths as they become available.

## Minimal working example

!!! warning "Coming soon"
    The end-to-end "define a decision, execute it, inspect the result" example
    lands once the `decisions` and `processes` packages are available. blkit is
    under active development; today the root `blkit` package (the typed value
    system and expression engine) is implemented, but the decision and process
    execution APIs this example will demonstrate are still being built.

    In the meantime, the [Reference](../reference/blkit.md) section documents the
    expression engine that is available today.

## Next steps

- **[Tutorials](../tutorials/index.md)** — a guided walkthrough of a complete
  use case, start to finish.
- **[Examples](../examples/admission.md)** — a library of focused,
  self-contained feature demonstrations.
- **[Reference](../reference/blkit.md)** — the full Go API reference.
- New to the standards blkit draws from? The
  [DMN](https://www.omg.org/dmn/) and [BPMN](https://www.bpmn.org/) overviews
  are a good primer.
