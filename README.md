# blkit

**A Go SDK for expressing and executing business rules and processes.**

blkit gives you a typed value system, an expression language, and (on the roadmap) a
full decision-and-process execution stack — so that rules, calculations, and workflows
can be authored as data and run directly inside your Go programs.

📖 **[Read the documentation →](https://friendly-business-machines.github.io/blkit/)**

---

## What is blkit?

Business logic is usually buried in hand-written `if`/`switch` statements, scattered
across services, and impossible to author or audit by anyone who isn't a Go developer.
blkit pulls that logic out into a first-class, executable model:

- **A typed value system** — numbers (exact decimals), strings, booleans, dates, times,
  durations, lists, dictionaries, and ranges — with consistent, null-aware semantics.
- **An expression language (`bl-expr`)** — write rules and calculations as plain strings
  (`aptitude >= 700 and absenceRatio <= 0.40`), compile them once, and evaluate them against
  many inputs.
- **A decision layer (available today)** — model decisions as typed, compile-checked
  nodes (decision tables, expressions, and native Go functions) and wire them into a
  single decision task whose connections the Go compiler verifies.
- **A process layer (in progress)** — execute BPMN-style process graphs with pluggable
  state, message brokers, and a REST/MCP server frontend; being built next.

It's built for **developers**, **AI agents**, **low-code / no-code tools**, and
**transpilers** — anything that needs to *generate* and *run* business logic rather than
hard-code it.

blkit draws on established standards — **BPMN 2.0** for processes, **DMN 1.5** for
decisions, and **FEEL** for the expression language — but it is a practical,
self-contained library, not a conformance implementation of any of them.

## Why blkit?

- **Simple, readable business-logic code.** Your business logic stays manageable and
  transparent thanks to an approachable expression language, and business-friendly building
  blocks like decision tables and process flows.
- **Stop re-solving solved problems.** Exact decimal math, null-aware logic, schema and
  type checking, and a ready-made value system come built in, so you write rules instead
  of the boilerplate around them.
- **Robust, safe, and performant.** Strong typing, well-defined semantics, and a
  parse-once / evaluate-many model give you predictable results and concurrency-safe
  execution at scale.
- **A great developer experience.** Easy to pick up, easy to run locally, and easy to
  deploy — no heavyweight runtime or external services required to get going.
- **Flexible about your final product.** Embed it in a larger Go codebase, stand it up as
  a backend microservice, expose it as an MCP tool, drive it from a TUI — the same engine
  fits whatever shape you need.

**Ideal for:** pricing and discount engines, automated decisioning, tax and fee
calculation, eligibility and approval rules, configurable rules engines, and embedding
user- or AI-authored logic into a product.

## Project status

blkit's **business-friendly expression language** and the **decision layer** built
on it are available today. Its two pluggable infrastructure layers — the **state
store** and the **message broker** — are also implemented, each as a `core`
interface (imported as `bl`) plus conformance-tested backends in their own
`stores/<name>` and `brokers/<name>` modules. The higher-level process, worker,
and server layers that drive them are being built next.

| Component | Status |
|---|---|
| Business-friendly expression language — compiler & evaluator | ✅ Available |
| Decision expressions, tables & native functions | ✅ Available |
| Decision tasks (typed, compile-checked node graphs) | ✅ Available |
| Reference data (static value sources) | ✅ Available |
| State store (in-memory + pluggable backends: Postgres, SQLite, NATS, …) | ✅ Available |
| Message brokers (in-memory + pluggable backends: Redis/Valkey, NATS, RabbitMQ, cloud) | ✅ Available |
| Process execution (BPMN-style graphs) | 🚧 Planned |
| Data contracts & execution context | 🚧 Planned |
| REST server with Server-Sent Events | 🚧 Planned |
| MCP server | 🚧 Planned |

Everything in the **Quickstart** and **Components** sections below works against the
shipped engine. Roadmap features are clearly marked *planned*.

## Install

```bash
go get github.com/friendly-business-machines/blkit
```

Requires **Go 1.22+**.

## Quickstart

Declare your inputs as a struct, compile an expression once, then evaluate it against
that typed input:

```go
package main

import (
	"fmt"

	bl "github.com/friendly-business-machines/blkit/core"
)

// The env struct declares the variables the expression may reference. The
// `expr` tag is the name as written in the source; referencing an undeclared
// name is a compile-time error — the struct *is* the schema.
type Applicant struct {
	Age    bl.BlNumber `expr:"age"`
	Points bl.BlNumber `expr:"points"`
}

func main() {
	// Parse and compile the rule once; reuse it for many evaluations.
	eligible, _ := bl.Expr[Applicant](`age >= 18 and points > 50000`)

	// Build a typed env value.
	age, _ := bl.Number(21)
	points, _ := bl.Number(60000)

	// Evaluate. The result is a typed BlValue (here, a BlBoolean).
	result, _ := eligible.Evaluate(Applicant{Age: age, Points: points})
	fmt.Println(result.String()) // true
}
```

The env struct doubles as the schema: its fields declare every variable the source
may reference. For a variable-free expression use `bl.ExprNoEnv` (shorthand for
`bl.Expr[bl.NoEnv]`). Every result is a `BlValue`, with a canonical `String()`
rendering and a `Type()` tag.

## A worked example: order discounts

Real business rules read naturally as `bl-expr` strings. Here's an order-pricing engine:
several discount rules are evaluated, the matching percentages are collected into a list,
and the single highest discount is applied to the subtotal — with exact decimal money
math.

```go
// The env declares the order fields the rules reference.
type Order struct {
	Tier       bl.BlString `expr:"tier"`
	AccountAge bl.BlNumber `expr:"accountAge"` // months
	Subtotal   bl.BlNumber `expr:"subtotal"`
	ItemCount  bl.BlNumber `expr:"itemCount"`
	Category   bl.BlString `expr:"category"`
	Code       bl.BlString `expr:"code"`
}

// Each rule contributes its discount when it matches, or 0 when it doesn't.
// `max` picks the single best discount — they don't stack.
discount, _ := bl.Expr[Order](`max([
	(if accountAge >= 12 and tier = "gold" then 0.10 else 0),
	(if accountAge < 3 then 0.10 else 0),
	(if itemCount >= 25 then 0.12 else 0),
	(if subtotal > 500 then 0.08 else 0),
	(if category = "furniture" then 0.06 else 0),
	(if code = "WELCOME20" then 0.20 else 0)
])`)

// Final order total — (1 - discount) is exact, no floating-point drift.
total, _ := bl.Expr[Order](`subtotal * (1 - (` + discount.Source() + `))`)
```

Each `BlExpr` is evaluated against the same order env and returns a typed result.
The same rules can also be expressed as a **[decision table](docs/decisions/decision-tables.md)** —
the tabular form business analysts expect — instead of an inline list of
conditionals, and wired into a **[decision task](docs/decisions/decision-tasks.md)**
alongside other nodes.

## Components

blkit is made of three components: **Expressions** for individual rules and calculations,
**Decisions** for composing rules into overall decision logic, and **Processes** for sequencing it
all into multi-step workflows with conditional pathways. The layers build on each other:
expressions are the building blocks of decisions, and decisions are the steps within processes.

### Expressions ✅ *(available today)*

`bl-expr`, a small expression language for writing business rules — built on a proven
[`expr`](https://github.com/expr-lang/expr)-based evaluation engine and a business-focused
type system (exact decimals, dates and durations, null-aware logic). It lives in the
`core` package (imported as `bl`). This is not string-parsing-at-runtime: each expression
is compiled once into bytecode and runs on a stack-based VM, so a `BlExpr` is stateless,
fast, and safe to evaluate against many inputs concurrently — use it directly for a single
rule or calculation, or as the engine the Decisions and Processes components call into.

A few rules that show the kind of business logic `bl-expr` is written for:

```
// Course admission — exact-decimal ratios, no floating-point drift
aptitude >= 700 and absenceRatio <= 0.40

// Tiered classification — a conditional that returns a typed result
if annualSpend > 100000 then "enterprise" else "standard"

// Free shipping — string equality and a numeric threshold together
customerTier = "gold" or orderTotal >= 50

// Fraud screen — `in` membership and null-aware logic: a missing
// priorFlags propagates to null instead of silently passing
orderAmount > 10000 and region in ["EU", "UK"] and priorFlags != null

// Order fulfilment — every line item must be in stock
every line in order.items satisfies line.inStock

// Invoice total — project and roll up a list in one expression
sum(for line in invoice.lines return line.qty * line.unitPrice)
```

Values are arbitrary-precision decimals, full date / time / duration types, lists,
dictionaries and ranges, and every operator is null-aware — missing inputs propagate to
`null` rather than crashing or guessing. Pass a schema to `Expr` and unknown references and
type mismatches are caught at compile time, before any data flows through.

### Decisions ✅ *(available today)*

A declarative decisioning layer, available in `core` now. Each decision is a node
generic over a typed input and output struct: a **decision table** (priority-ordered
rules a business analyst can read), a **decision expression** (named outputs that may
build on one another), or a **decision native function** (the escape hatch — arbitrary
Go). Static constants are supplied as **reference data**. Because every cell, entry,
and field is an expression-language value, decisions inherit the expression layer's
types and exact-decimal semantics.

Nodes compose into a **decision task** — a graph wired by connecting their handles
with `bl.Edge`. The connections are checked by the Go compiler (a mis-typed wire won't
build), and the task derives its node set from the wiring, orders it, and rejects
cycles at program start. A decision task is itself a node, so whole decisions nest
inside larger ones. See the [decision guides](docs/decisions/overview.md).

### Processes 🚧 *(planned)*

An orchestration layer (`blkit.processes`) for multi-step workflows: BPMN-style graphs of
tasks and gateways, where decision tasks call into the decisions layer and native tasks
call your own Go functions. A `Process` is a **stateless definition** — execution state
(the `ExecutionContext` of variables and the `ExecutionHistory` of steps taken) is passed
in and returned out, so one definition can run concurrently for many independent
executions. Execution is asynchronous, built on goroutines and channels. Surrounding
support — data contracts, a pluggable state store (`blkit.data`), message brokers
(a core `MessageBroker` interface plus per-backend `brokers/<name>` modules), and a
REST/SSE server (`blkit.restserver`) — lets processes run as services.

## Architecture

> **Target design — partly implemented.** This section describes how blkit's
> process and service layers are intended to fit together. The expression and
> decision engines ship today, and the two pluggable infrastructure layers below —
> the **state store** and the **message broker** (each a core interface plus
> conformance-tested backends) — are now implemented. The roles that drive them
> (workers, producers, the REST/MCP servers) and the process-execution layer do
> not exist in the codebase yet.

The components above can be embedded directly in a single Go program. To run processes
*as a service* — many instances, across many machines, surviving restarts — blkit fans
out into a few cooperating roles that communicate **only through a message broker**,
never directly with one another. This keeps every role stateless and independently
scalable.

- **Producers (feeders)** — MCP servers, REST/SSE servers, CLI tools, admin UIs. Using
  the producer-side `MessageBroker` API they *feed* new runs into the system (`Submit`),
  respond to a process's request for input, and cancel or terminate. A producer holds no
  execution state: it hands a `StartRequest` to the broker and gets back a
  `ProcessInstanceID`.
- **Message broker** — Redis/Valkey, NATS, RabbitMQ, Azure Service Bus, Google Pub/Sub,
  AWS SQS/SNS, or the in-memory bus built into core for tests and small deployments. The
  `MessageBroker` interface wraps it, presenting a producer-side and a worker-side surface
  over broker-native primitives; each external backend is its own `brokers/<name>` module.
  The broker interface talks only to the broker, never to the state store.
- **Workers** — each `worker.Run` loop registers its **capability set** (the process
  packages linked into its binary), selectively fetches only the jobs it can run, and
  drives each one to completion locally: evaluating the graph, executing tasks as
  goroutines, streaming state to the state store, and publishing instance events back to
  the broker. Workers are stateless and horizontally scalable — add replicas to go faster.
- **State store** — Postgres or another backing store, owned **exclusively by the
  workers**. Execution context and history are persisted here; producers and the broker
  never touch it. Admin and audit reads go straight to the state store.

```mermaid
flowchart LR
    subgraph Clients["Clients"]
        Web["Web app"]
        Mobile["Mobile app"]
        Svc["Third-party service"]
    end

    REST["REST / SSE server<br/><i>built with blkit</i>"]

    Broker[("Message broker<br/>Redis · NATS · Azure SB<br/>Google Pub/Sub")]

    subgraph Workers["Workers"]
        W1["worker<br/><i>built with blkit</i>"]
        W2["worker"]
    end

    Store[("StateStore<br/>Postgres, …")]

    Web -->|"HTTP / SSE"| REST
    Mobile -->|"HTTP / SSE"| REST
    Svc -->|"HTTP / SSE"| REST

    REST -->|"Submit / Cancel"| Broker
    Broker -->|"instance events"| REST

    Broker -->|"FetchJobs"| W1
    Broker -->|"FetchJobs"| W2
    W1 -->|"Report* / events"| Broker
    W2 -->|"Report* / events"| Broker

    W1 -->|"WriteBatch / Save"| Store
    W2 -->|"WriteBatch / Save"| Store

    classDef blkit fill:#dbeafe,stroke:#2563eb,stroke-width:2px,color:#1e3a8a;
    class REST,W1,W2 blkit;
```

The **highlighted** nodes — the REST/SSE server and the workers — are the Go binaries you
build with blkit (the REST server is the batteries-included `blkit.restserver`). The
broker, state store, and client frontends are infrastructure you bring; blkit reaches the
broker and state store through pluggable interfaces.

A single Go module can produce many different worker binaries, each importing only the
process packages it should run. Deploy each as its own fleet and the broker's selective
consumption routes the right work to the right pool — all without any worker knowing about
any other.

### Inside a worker

A single `worker.Run` is a handful of cooperating goroutines. The **fetch loop** pulls
only the jobs this binary can run and spawns one **executor** per job; each executor runs
its process to completion, fanning parallel branches out to **task goroutines**. State
never blocks execution: executors hand off `WriteOp`s to an elastic **writer pool** that
batches them to the `StateStore`, while a **heartbeat goroutine** keeps the worker's lease
alive on the broker.

```mermaid
flowchart LR
    Broker[("Message broker")]
    Store[("StateStore")]

    subgraph Worker["worker.Run"]
        direction TB
        Fetch["Fetch loop<br/><i>1 goroutine · MaxConcurrent permit</i>"]
        Exec["Executor goroutines<br/><i>1 per in-flight job</i><br/>load state · Evaluate · signal outcome"]
        Tasks["Task goroutines<br/><i>parallel branches · TaskConcurrency</i>"]
        Writer["Writer pool<br/><i>elastic · batches WriteOps</i>"]
        Heart["Heartbeat goroutine"]

        Fetch -->|"spawn per job"| Exec
        Exec -->|"fan out"| Tasks
        Exec -->|"enqueue WriteOps"| Writer
    end

    Broker -->|"FetchJobs"| Fetch
    Exec -->|"Report* · events · PostError"| Broker
    Heart -->|"Heartbeat"| Broker

    Exec -->|"LoadState · Save (boundary)"| Store
    Writer -->|"WriteBatch (steps, txns)"| Store

    classDef blkit fill:#dbeafe,stroke:#2563eb,stroke-width:2px,color:#1e3a8a;
    class Fetch,Exec,Tasks,Writer,Heart blkit;
```

## Documentation

The full documentation lives at the
**[blkit documentation portal](https://friendly-business-machines.github.io/blkit/)**
(GitHub Pages): a guide to the expression language and every value type, operator, and
built-in function, plus worked examples (order discounts, product pricing, invoice
processing, tax calculation, and more). The package doc comments and the generated Go
API reference complement it.

## Dependencies

| Package | Purpose |
|---|---|
| [`github.com/shopspring/decimal`](https://github.com/shopspring/decimal) | Arbitrary-precision decimal arithmetic backing `bl.BlNumber`. |
| [`github.com/expr-lang/expr`](https://github.com/expr-lang/expr) | Expression engine foundation, extended with blkit's own type system, operators, and built-in functions. |
| [`github.com/arran4/golang-ical`](https://github.com/arran4/golang-ical) | iCalendar parsing behind the `calendar` value type. |
| [`github.com/teambition/rrule-go`](https://github.com/teambition/rrule-go) | Recurrence-rule (RRULE) expansion for calendar entries. |

## License

blkit is licensed under the GNU Affero General Public License v3.0. See [LICENSE](LICENSE).
