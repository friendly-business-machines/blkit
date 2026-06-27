# Decision Native Functions

> Back a decision with a hand-written Go function when its logic is beyond — or
> better expressed outside — tables and expressions.

A **decision native function** is the escape hatch of the decision family: its
logic is an ordinary Go function. When a step needs to run a bespoke algorithm,
score a machine-learning model, or call a service, you write it in Go and wrap it
as a node — and it composes with tables and expressions exactly like any other.

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

## Keep it pure where you can

A native function *can* reach outside pure computation — call a service, query a
database — but it is easier to test and reason about when it is a pure function of
its declared inputs. Prefer feeding external data in as inputs (from
[reference data](reference-data.md) or an upstream node) and keeping the body a
calculation.

## Errors, panics, and retries

A non-nil error from the function is returned (tagged with the node id) and, inside
a [task](decision-tasks.md), aborts the decision. A **panic** is recovered and
turned into that same kind of error, so a buggy function never crashes the program.

Set `Retry` to re-run the function on error:

```go
bl.NewDecisionNativeFunction(bl.DecisionNativeFunctionConfig{
    Id:    "credit_score",
    Retry: bl.NewRetryConfig(bl.RetryOpts{MaxRetries: 3, ExponentialBackoff: true}),
}, scoreApplicant)
```

Retry fires on *any* error, so don't combine it with using an error as a deliberate
"no result" signal.

## Running concurrently

Set `Concurrent: true` to let a containing task overlap a slow function (scoring a
large model, say) with independent work. It is purely a scheduling change — the
result is identical to running in order; only the wall-clock timing differs, and
the function must be safe to run alongside the rest of the graph.

## Inside a task

Like every node, it exposes `In`/`Out` handle surfaces and is wired into a
[decision task](decision-tasks.md) with `bl.Edge`.
