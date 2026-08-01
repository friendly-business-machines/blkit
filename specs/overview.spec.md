---
name: blkit Overview
description: Project-wide conventions, Go naming conventions, Interface Specification format, fluent API design, and async execution model
status: implemented
code:
  - core/
  - brokers/
  - stores/
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

blkit's public API is a single **core package** plus a few optional infrastructure sub-packages. The repository root holds no Go source; the core package lives in `core/`.

- `core` (imported as `bl` — `import bl "github.com/friendly-business-machines/blkit/core"`) — the single logic import. One package holding the whole logic layer, so callers reach all of it through one `bl.` alias:
  - the typed value system and expression engine (`bl.BlNumber`, `bl.BlString`, `bl.BlExpr`, `bl.BlDictionary`, `bl.BlList`, etc.);
  - the decision classes (`bl.DecisionTask`, `bl.DecisionTable`, `bl.DecisionExpression`, `bl.DecisionNativeFunction` — each generic over typed input/output structs; a `bl.DecisionTask` is itself a node, so child decisions compose directly);
  - the process classes (`bl.Process`, `bl.ProcessGraph`, `bl.StartEvent`, `bl.EndEvent`, gateway nodes, tasks);
  - the data contracts and pluggable state store (`bl.InputContract`, `bl.OutputContract`, `bl.ExecutionContext`, `bl.ExecutionHistory`, `bl.StateStore`);
  - the `bl.MessageBroker` interface and its built-in in-memory backend, for submitting process runs, delivering messages, and observing events from outside the worker pool.
- `blkit/brokers/<name>` — one Go module per external message-broker backend (`redis`, `nats`, `rabbitmq`, `azure-service-bus`, `google-pubsub`, `aws-sqs-sns`), each implementing `bl.MessageBroker` with its own client dependency.
- `blkit/worker` — process-execution worker pool.
- `blkit/restserver` — HTTP REST server with Server-Sent Events that exposes processes registered on a `MessageBroker`. Optionally embeds a worker in the same binary.
- `blkit/mcp` — MCP server integration.

The infrastructure sub-packages are imported separately and depend on `core`. They carry heavy, optional dependencies (Redis, NATS, `net/http`), so importing `core` for the logic layer never pulls them in. Within `core`, the value/expression type system is independently usable — callers can construct typed values, build expression trees, and evaluate them directly without involving decisions or processes.

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
import bl "github.com/friendly-business-machines/blkit/core"

type BranchEnv struct {
    Kind bl.BlString `expr:"kind" ctx:"taskA.kind"`
}

var myProcess = bl.NewProcess("my-process", "1.0", []bl.Edge{
    bl.Start("start", "Start", bl.NewInputContract()).To(taskA),
    taskA.To(bl.Xor[BranchEnv](
        bl.Branch("b", `kind = "b"`, taskB),
        bl.DefaultBranch("c", taskC),
    )),
    bl.Join(taskB, taskC).To(taskD),
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
