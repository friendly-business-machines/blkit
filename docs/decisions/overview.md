# Overview

> What the decision layer is, the four kinds of decision node, and how typed
> inputs and outputs are wired together into a single, compile-checked decision.

Where [expressions](../expressions/overview.md) compute one value, **decisions**
package business logic into reusable, named units with typed inputs and outputs —
and let you compose those units into a graph that runs as one. The decision layer
is blkit's take on the [DMN](https://www.omg.org/dmn/) standard, built directly on
the expression engine, so every rule, cell, and entry inherits the same types and
exact-decimal arithmetic.

## Typed contracts, in and out

Every decision node declares what it consumes and produces as two ordinary Go
structs — an **input** struct `I` and an **output** struct `O`. Each field is a
`bl.Handle[T]`, a typed slot that carries one [value](../expressions/overview.md):

```go
type LoanInputs struct {
    Age    bl.Handle[bl.BlNumber] `expr:"age"`
    Income bl.Handle[bl.BlNumber] `expr:"income"`
}
type Eligibility struct {
    Eligibility bl.Handle[bl.BlString] `expr:"eligibility"`
}
```

Because the contracts are real Go types, calling a node with the wrong input
shape, or reading an output it does not have, is a **compile error**. The
`expr:"…"` tag names the variable that a node's expressions reference (`age`); the
Go field name (`Age`) is what you connect when wiring nodes together.

## The four kinds of node

A node is generic over its `I` and `O` and exposes a typed
`Evaluate(in I) (O, error)`. Pick the kind that fits how the logic is best
expressed:

| Node | Logic is… | Reach for it when |
|---|---|---|
| [Decision table](decision-tables.md) | a table of rules | rules are best read as rows of conditions → outcomes |
| [Decision expression](decision-expressions.md) | named text expressions | one or more outputs are short formulas, possibly building on each other |
| [Decision native function](decision-native-fn.md) | arbitrary Go | the logic is neither — call a model, an algorithm, or a service |
| [Decision task](decision-tasks.md) | a graph of nodes | you are composing several of the above into one decision |

A [reference data](reference-data.md) value is a static constant a node can read —
not a node itself, but wired in the same way.

## Composing nodes into a task

A [`DecisionTask`](decision-tasks.md) is generic over its own `TaskIn`/`TaskOut`
and holds a graph of nodes wired together by connecting their handles. You connect
an output handle to an input handle with `bl.Edge`, and the set of connections is
the graph:

```go
var loan = bl.NewDecisionTask[LoanInputs, LoanOutputs](bl.DecisionTaskConfig{
    Id: "loan", Name: "Loan Approval",
})

var _ = loan.Graph(
    bl.Edge(loan.In.Age,                 eligibility.In.Age),
    bl.Edge(loan.In.Income,              eligibility.In.Income),
    bl.Edge(eligibility.Out.Eligibility, approval.In.Eligibility),
    bl.Edge(approval.Out.Status,         loan.Out.Status),
)
```

Each `bl.Edge` is type-checked at `go build`: connecting a string output to a
number input, or naming a field that does not exist, will not compile. The task
derives its node set from the edges, orders them so each runs after the nodes it
depends on, and rejects cycles — all when `Graph` is called, at program start.
Evaluating the task threads values through the graph and returns the typed
`TaskOut`.

A `DecisionTask` is itself a node, so a whole decision drops into a larger one
exactly like any other — there is no separate "sub-decision" type.

## Where to next

- **[Decision tables](decision-tables.md)** — rules as rows, with hit policies.
- **[Decision expressions](decision-expressions.md)** — named, inter-dependent outputs.
- **[Decision native functions](decision-native-fn.md)** — the Go escape hatch.
- **[Reference data](reference-data.md)** — static constants.
- **[Decision tasks](decision-tasks.md)** — wiring nodes into one decision.
