# Decision Native Functions

> Back a decision with a hand-written Go function when its logic is beyond — or
> better expressed outside — tables and expressions.

A **decision native function** is the escape hatch of the decision family: its
logic is an ordinary Go function. When a step needs to run a bespoke algorithm, a
calculation, or a model that is simply too hard to express as a table or
expression, you write it in Go and wrap it as a node — and it composes with tables
and expressions exactly like any other.

It is for **pure computation**, not I/O. Anything that calls a service, queries a
database, or touches storage belongs in a process-layer native-function task
instead — that is where retries, timeouts, and concurrency live. A decision native
function deliberately has none of those: it runs its function exactly once,
synchronously.

A `bl.DecisionNativeFunction[I, O]` is generic over a typed input struct `I` and
output struct `O`. The function is passed to the constructor, so `I` and `O` are
inferred from it.

## Writing one

```go
type ScoreInputs struct {
    Age    bl.Handle[bl.BlNumber] `expr:"age"`
    Income bl.Handle[bl.BlNumber] `expr:"income"`
}
type ScoreOutputs struct {
    Score bl.Handle[bl.BlNumber] `expr:"score"`
}

func scoreApplicant(in ScoreInputs) (ScoreOutputs, error) {
    score := model(in.Age.Get(), in.Income.Get()) // any Go you like
    return ScoreOutputs{Score: bl.NewHandle(score)}, nil
}

var creditScore = bl.NewDecisionNativeFunction(bl.DecisionNativeFunctionConfig{
    Id:          "credit_score",
    Name:        "Credit Score",
    Description: "Scores an applicant's creditworthiness.",
}, scoreApplicant)
```

Because `I` and `O` are real structs, the body reads `in.Age.Get()` and returns
`ScoreOutputs{…}` with full Go type-checking — there is no "missing output" to get
wrong. The function body itself is yours; blkit only checks the contract at its
edges.

## Keep it pure

A native function *can* reach outside pure computation — call a service, query a
database — but that is not what it is for: such work belongs in a process-layer
native-function task. Keep the body a pure function of its declared inputs. It is
easier to test and reason about, and you feed external data in as inputs (from
[reference data](reference-data.md) or an upstream node) rather than fetching it
inside the function.

## Errors and panics

A non-nil error from the function is returned (tagged with the node id) and, inside
a [task](decision-tasks.md), aborts the decision. A **panic** is recovered and
turned into that same kind of error, so a buggy function never crashes the program.

The function runs **exactly once** — there is no retry. Retrying is a property of
fallible I/O, which belongs in a native-function task, not a decision. (Returning
an error as a deliberate "no result" signal works cleanly for the same reason:
nothing re-runs the function behind your back.)

## Inside a task

Like every node, it exposes `In`/`Out` handle surfaces and is wired into a
[decision task](decision-tasks.md) with `bl.Edge`.
