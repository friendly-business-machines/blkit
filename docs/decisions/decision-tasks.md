# Decision Tasks

> The unit that wires decision nodes into one runnable decision — typed inputs and
> outputs, connections checked by the compiler, evaluated as a graph.

A **decision task** is the container that turns a set of nodes into a single
decision. You give it a typed input and output, wire the nodes together by
connecting their handles, and it runs them in the right order and hands you a typed
result.

A `bl.DecisionTask[TaskIn, TaskOut]` is generic over its own input and output
structs. You build it in two steps: construct it, then wire it with `Graph`.

## Building the nodes

First, the nodes — each a [table](decision-tables.md),
[expression](decision-expressions.md), or
[native function](decision-native-fn.md) — and any
[reference data](reference-data.md):

```go
var minIncome = bl.NewReferenceData(bl.ReferenceDataConfig[bl.BlNumber]{
    Id: "min_income", Value: bl.Number(30000),
})

var eligibility = bl.NewDecisionTable[EligibilityInputs, EligibilityOutputs](/* … */)

var approval = bl.NewDecisionExpression[ApprovalInputs, ApprovalOutputs](bl.DecisionExpressionConfig{
    Id: "approval",
    Entries: bl.Entries{
        "status": `if eligibility = "eligible" then "approved" else "denied"`,
    },
})
```

## Wiring with `Edge` and `Graph`

The task declares its external contract as `TaskIn`/`TaskOut`. Construct the task,
then call `Graph` with the connections — each `bl.Edge` joins a **source** handle
(a task input, a node output, or reference data) to a **destination** handle (a
node input or the task output):

```go
type LoanInputs struct {
    Age    bl.Handle[bl.BlNumber] `expr:"age"`
    Income bl.Handle[bl.BlNumber] `expr:"income"`
}
type LoanOutputs struct {
    Status bl.Handle[bl.BlString] `expr:"status"`
}

var loan = bl.NewDecisionTask[LoanInputs, LoanOutputs](bl.DecisionTaskConfig{
    Id: "loan", Name: "Loan Approval",
})

var _ = loan.Graph(
    bl.Edge(loan.In.Age,                 eligibility.In.Age),
    bl.Edge(loan.In.Income,              eligibility.In.Income),
    bl.Edge(minIncome.Value,             eligibility.In.MinIncome),
    bl.Edge(eligibility.Out.Eligibility, approval.In.Eligibility),
    bl.Edge(approval.Out.Status,         loan.Out.Status),
)
```

`loan.In` and `loan.Out` are the task's own boundary handles. There are no node
lists to maintain: the task **derives** its nodes and reference data straight from
the edges, so each object appears exactly once — in the wiring.

## What the compiler and `Graph` check

Two layers catch mistakes early:

- **At `go build`** — every `bl.Edge` connects two handles of the same type, so
  joining a string output to a number input, or naming a field a node does not
  have, will not compile.
- **At `Graph`** (program start) — the task checks that every node input is wired
  exactly once and every `TaskOut` field is produced, orders the nodes so each runs
  after the ones it depends on, and rejects cycles. A problem panics immediately,
  pinpointing the bad declaration.

The one thing left to evaluation is a value whose *computed* type disagrees with
its declared output — the expression engine is dynamically typed, so that surfaces
as an error from `Evaluate`.

## Evaluating

`Evaluate` threads values through the graph and returns the typed `TaskOut`:

```go
age, _ := bl.Number(30)
income, _ := bl.Number(50000)
out, _ := loan.Evaluate(LoanInputs{
    Age:    bl.NewHandle(age),
    Income: bl.NewHandle(income),
})
// out.Status.Get() == "approved"
```

## A task is a node

Because a `DecisionTask` has a typed `Evaluate` and `In`/`Out` handles, it is
itself a decision node. A whole decision drops into a larger one exactly like any
table or expression — wire its `child.In.*` / `child.Out.*` handles in the parent's
graph. There is no separate sub-decision type; composition is just more wiring.

## Reuse with `Clone`

Build a decision once and reuse it by cloning. A clone shares the original's graph
by reference and takes fresh identity from its config — handy when the same
decision runs in more than one place:

```go
riskCheck := loan.Clone(bl.DecisionTaskConfig{Id: "risk-check", Name: "Risk Check"})
```
