---
name: blkit Overview
description: Project-wide conventions, Go naming conventions, Interface Specification format, fluent API design, and async execution model
targets:
  - ../**/*.go
---

# blkit Overview

blkit is a Go SDK for expressing and executing business logic. It provides business-logic abstraction layers, giving developers, AI agents, low-code/no-code tools, and transpilers a library of types and functions for building executable business logic directly in code.

## Go Naming Conventions

blkit follows standard Go conventions:

| Concept | Convention |
|---|---|
| Type | `Process` (exported) |
| Method | `Evaluate()` (exported) |
| Method (multi-word) | `ToMarkdown()` (exported) |
| Field | `Id` (exported) |
| Package | `blkit` |

### Namespace Structure

blkit's public API spans the **root package** plus several sub-packages:

- `blkit` (the root package, imported as `bl`) — typed value system and expression engine (`bl.BlNumber`, `bl.BlString`, `bl.BlExpr`, `bl.BlDictionary`, `bl.BlList`, etc.). Unlike the others, this is **not** a sub-package — its code is the root-level `expr_*.go` files.
- `blkit.processes` — process execution classes (`Process`, `ProcessGraph`, `StartEvent`, `EndEvent`, gateway nodes, tasks)
- `blkit.decisions` — decision classes (`DecisionTask`, `DecisionTable`, `LiteralExpression`, `BoxedContext`, `Relation`, `Invocation`, `BusinessKnowledgeModel`)
- `blkit.data` — data contracts, execution context, and the pluggable state store (`InputContract`, `OutputContract`, `ExecutionContext`, `ExecutionHistory`, `StateStore`)
- `blkit.messagegateway` — producer-side typed client SDK (`MessageGateway` interface, `RedisMessageGateway`, `NATSMessageGateway`, `InMemoryMessageGateway`) for submitting process runs, delivering messages, and observing events from outside the worker pool
- `blkit.restserver` — HTTP REST server with Server-Sent Events that exposes processes registered on a `MessageGateway`. Optionally embeds a worker in the same binary.

The value/expression type system lives in the **root `blkit` package** (imported as `bl`, the root-level `expr_*.go` files), not a sub-package. It is consumed by decision models but is independently usable — callers can construct typed values, build expression trees, and evaluate them directly without involving decisions.

## Interface Specification Format

Each spec file covering a public type includes a pseudocode block that defines the type's public API. The pseudocode uses Go-like notation — it is **not** compilable Go source code, but follows Go conventions for types, methods, and naming. Exported names use `PascalCase` as standard.

```go
// Pseudocode conventions:
// - *T                      nullable (pointer) parameter or return type
// - (T, error)              value-or-error return
// - map[string]any          Variables — string keys to any serialisable value
// - func NewFoo(...) *Foo   constructor / factory function
// - func (f *Foo) Bar() T   method with receiver
// - ...                     marks a method body (stub)
// - interface               marks an interface type
```

## Fluent Interface

blkit implements a fluent interface: method chains read as meaningful, human-friendly expressions.

Process graphs are declared by chaining `.to()` calls from a `start()` node through tasks and gateways to an `end()` node. The chain expressions are passed as a list to the `graph` parameter of `Process.create()`. Gateways are `ProcessNode`s — `and_()`, `or_()`, `xor()`, and `join()` each create a gateway node that is passed into `.to()` like any other node. `Process.create()` walks the edges to discover the full graph structure and stores the result as a `ProcessGraph`.

```go
myProcess := bl.NewProcess("my-process", "1.0", []Edge{
    bl.Start("start", "Start", bl.NewInputContract()).To(taskA),
    taskA.To(Xor(conditions, map[string]ProcessNode{"b": taskB, "c": taskC})),
    Join(taskB, taskC).To(taskD),
    taskD.To(bl.End("done", "Done")),
})
```

Once the graph is built, `.evaluate()` walks it, dispatching ready tasks as goroutines, until the process completes, suspends waiting for an external event, or fails.

## Async Execution

`.evaluate()` is asynchronous. The Go implementation uses goroutines and channels. Callers must await the result.

Tasks in a process may themselves perform async work (e.g. calling an external API, reading a database). The execution engine awaits each task before advancing.

## Error Handling

Errors during execution are surfaced through the async return type — they do not silently swallow exceptions. Unhandled errors during a task result in the process transitioning to a faulted state, which is captured in the execution history.

## Statefulness

A `Process` instance is a stateless definition — it holds the graph, metadata, and configuration, but no execution state. Execution state lives externally:

- The `ExecutionContext` (process variables) is passed into `evaluate()` and returned in the `EvaluationResult`.
- The `ExecutionHistory` (a record of every step taken) is passed into `evaluate()` for re-evaluations and extended in the result.

This design allows a single `Process` instance to be evaluated concurrently for many independent process executions. Workers create no per-execution copies of the process — they pass different context/history pairs to the same instance.

## Extensibility

Projects built on blkit extend it by authoring a custom Go package implementing domain logic, algorithms, and third-party integrations. This package is a standard module dependency. `NativeFunctionTask` nodes in a process definition hold a direct reference to a Go function, which receives the `ExecutionContext` and returns a `bl.BlValue`.

## Key Go Dependencies

| Package | Purpose |
|---|---|
| [`github.com/shopspring/decimal`](https://github.com/shopspring/decimal) | Arbitrary-precision decimal arithmetic backing `bl.BlNumber`. Ensures exact results for decimal operations (`0.1 + 0.2 = 0.3`). |
| [`github.com/expr-lang/expr`](https://github.com/expr-lang/expr) | Expression language engine underpinning blkit's value/expression system (the root `blkit` package). blkit extends it with its own type system (`bl.BlValue`), operators, and built-in function registrations. |

## Influences

blkit's design draws from established standards — BPMN 2.0 for process execution, DMN 1.4 for decision models, and FEEL for the expression language and type system — but it is not a conformance implementation of any of these standards. The API is designed to be practical and self-contained rather than spec-compliant.
