---
name: DecisionNativeFunction
description: A DecisionNode whose logic is an arbitrary native Go function. It declares its input and output contracts as plain []Field data; Evaluate hands the named input values to the function and returns the named output values it produces. The escape hatch for logic that is neither a table nor an expression — call a model, a service, or any Go code.
targets:
  - ../../core/decision_native_fn.go
---

# DecisionNativeFunction

A `DecisionNativeFunction` is a [`DecisionNode`](decision-node.spec.md) whose logic is a plain Go function. It declares its contracts as plain data (see [decision-node.spec.md § Contracts are plain data](decision-node.spec.md#contracts-are-plain-data-not-go-generics)): `Inputs` is the list of named, typed variables it consumes, and `Outputs` the list of named, typed values it produces. The logic itself is supplied as a Go function, `Fn`. `Evaluate` hands `Fn` the named input values and returns the named output values it produces.

It is the **escape hatch** of the decision family. Where [`DecisionTable`](decision-table.spec.md) expresses logic as tabular rules and [`DecisionExpression`](decision-expression.spec.md) as named text expressions, a `DecisionNativeFunction` expresses it as ordinary Go. When a decision needs something neither form can do — run a bespoke algorithm, or score a PMML or ONNX model — you write it in `Fn`.

Because `Fn` is ordinary Go, it *can* also reach outside pure computation — call an external service, query a database, read from disk. That is technically supported but not especially encouraged: a node that performs I/O or carries side effects is harder to test, cache, and reason about than a pure one, and it makes the decision's result depend on state the framework cannot see. Prefer feeding such data in as declared `Inputs` (resolved from reference data or an upstream node) and keeping `Fn` a pure function of its inputs; reach for in-`Fn` I/O only when that is genuinely impractical.

```go
// NativeFn is the signature of a node's logic: it receives the declared inputs
// keyed by name and returns the declared outputs keyed by name.
type NativeFn func(in map[string]BlValue) (map[string]BlValue, error)

// The node itself is opaque; it is built and validated from a config.
type DecisionNativeFunction struct { /* unexported: id, name, description, fn, inputs, outputs */ }

func NewDecisionNativeFunction(config DecisionNativeFunctionConfig) *DecisionNativeFunction

type DecisionNativeFunctionConfig struct {
    Id          string
    Name        string
    Description string

    // Inputs declares the named, typed variables the function consumes from
    // outside this node (task inputs, upstream node outputs, or reference data).
    Inputs []Field

    // Outputs declares the named, typed values this node produces. Fn must
    // return exactly these names, each of its declared type.
    Outputs []Field

    // Fn is the native logic — typically a named Go function. It receives the
    // declared inputs keyed by name and must return the declared outputs keyed
    // by name.
    Fn NativeFn

    // Concurrent, when true, lets a containing DecisionTask evaluate this node in
    // a goroutine — overlapping its Fn with independent nodes — and join the
    // result before any node that consumes this node's outputs. Default false. It
    // has no effect on a standalone Evaluate; see § Concurrent execution.
    Concurrent bool

    // Retry, when non-nil, re-runs Fn on a non-nil error using the shared
    // bl.RetryConfig (see process.spec.md) — up to MaxRetries attempts and/or for
    // RetryFor, with RetryDelay and optional ExponentialBackoff between attempts.
    // The last error is returned once retries are exhausted. Default nil (no
    // retry). See § Retry.
    Retry *RetryConfig
}

// DecisionNode interface satisfaction.
func (d *DecisionNativeFunction) GetId() string
func (d *DecisionNativeFunction) GetName() string
func (d *DecisionNativeFunction) GetDescription() string
func (d *DecisionNativeFunction) Inputs() []Field
func (d *DecisionNativeFunction) Outputs() []Field

// Concurrent reports whether a containing DecisionTask may evaluate this node in
// a goroutine (see § Concurrent execution).
func (d *DecisionNativeFunction) Concurrent() bool

// Evaluate runs Fn against the input variables and returns a map keyed by this
// node's output names (see decision-node.spec.md). A panic in Fn is recovered and
// returned as an Id-tagged error rather than crashing the program.
func (d *DecisionNativeFunction) Evaluate(input map[string]BlValue) (map[string]BlValue, error)

func (d *DecisionNativeFunction) ToMarkdown() string

// DecisionDefinitionError reports one or more construction problems (shared
// across the decision family).
type DecisionDefinitionError struct {
    Node     string
    Problems []string
}
```

`NewDecisionNativeFunction` validates the input and output contracts — well-formed field names and types, no duplicate name within either list, and a non-empty `Outputs` — and requires a non-nil `Fn`. Every problem is accumulated and raised together as a `DecisionDefinitionError`. The function body itself is opaque to blkit: it is ordinary Go, checked by `go build`, and its logic is the author's responsibility.

---

## Building a DecisionNativeFunction

### Single output

```go
// scoreApplicant runs the model and returns the score keyed by its output name.
func scoreApplicant(in map[string]bl.BlValue) (map[string]bl.BlValue, error) {
    var age    = in["age"].(bl.BlNumber)
    var income = in["income"].(bl.BlNumber)
    var score  = scoreModel(age, income) // any Go: a bespoke algorithm, a PMML/ONNX model, …
    return map[string]bl.BlValue{"score": score}, nil
}

var creditScore = bl.NewDecisionNativeFunction(bl.DecisionNativeFunctionConfig{
    Id:          "credit_score",
    Name:        "Credit Score",
    Description: "Scores an applicant's creditworthiness.",
    Inputs: []bl.Field{
        {Name: "age", Type: bl.TypeNumber},
        {Name: "income", Type: bl.TypeNumber},
    },
    Outputs: []bl.Field{
        {Name: "score", Type: bl.TypeNumber},
    },
    Fn: scoreApplicant,
})

var age, _    = bl.Number(42)
var income, _ = bl.Number(90000)
var result, _ = creditScore.Evaluate(map[string]bl.BlValue{
    "age":    age,
    "income": income,
})
// result is map[string]bl.BlValue{"score": <bl.BlNumber>}
```

`age` and `income` are declared inputs, resolved at task construction from the task's inputs or an upstream node's output. `score` is the produced output; downstream nodes consume it by declaring an input named `score`.

### Multiple outputs

```go
// assessRisk derives the risk band and premium from the score and history.
func assessRisk(in map[string]bl.BlValue) (map[string]bl.BlValue, error) {
    var band, premium = assess(in["score"].(bl.BlNumber), in["history"].(bl.BlList))
    return map[string]bl.BlValue{
        "band":    band,
        "premium": premium,
    }, nil
}

var riskAssessment = bl.NewDecisionNativeFunction(bl.DecisionNativeFunctionConfig{
    Id:   "risk_assessment",
    Name: "Risk Assessment",
    Inputs: []bl.Field{
        {Name: "score", Type: bl.TypeNumber},
        {Name: "history", Type: bl.TypeList},
    },
    Outputs: []bl.Field{
        {Name: "band", Type: bl.TypeString},
        {Name: "premium", Type: bl.TypeNumber},
    },
    Fn: assessRisk,
})
```

`assessRisk` must return every declared output name (`band`, `premium`), each of its declared type.

---

## Validation and type safety

A `DecisionNativeFunction` carries its contracts as plain `[]Field` data, so its safety is *contract-matching*, not name-inference. Following the family rule (see [decision-node.spec.md § Where type-safety happens](decision-node.spec.md#where-type-safety-happens)), the checks concentrate at **construction** — the mental model is *if construction does not complain, the node is well-formed* — with only value-versus-declaration correctness left to evaluation. The function body is opaque to blkit: it is ordinary Go, checked by `go build`, and its internal logic is the author's responsibility.

Go's static typing enforces exactly one thing here: that `Fn` has the `NativeFn` signature — a function accepting and returning a `map[string]BlValue` — so a function of any other shape fails `go build` before the program runs. Every *other* check is a **runtime** one, because a node's contracts are `[]Field` data, outside the Go type system, so a malformed contract is rejected when its constructor runs, never by `go build`. Following the decision-family convention, `NewDecisionNativeFunction` does not return an error — it accumulates every construction problem and **panics once** with a `*DecisionDefinitionError`. Because a node is typically a package-scope `var`, that panic fires during package initialisation, at program (or test-binary) startup — deterministic load-time fail-fast: any run of the program, or any test that merely imports the declaring package, surfaces every construction error.

Four moments, across three phases — compile, runtime init, and runtime — catch four distinct classes of problem:

| Phase | Moment | Trigger | What it catches | Raised as |
|-------|--------|---------|-----------------|-----------|
| **Compile** | Go compilation | `go build` | A `Fn` whose signature is not `func(map[string]BlValue) (map[string]BlValue, error)` — one that accepts or returns anything other than the BlValue map. | Go type error |
| **Runtime init** | Node construction | `NewDecisionNativeFunction` | A malformed contract (an ill-formed `Inputs`/`Outputs` name or type, a duplicate name within either list, or an empty `Outputs`); a nil `Fn`; a `Retry` with neither `MaxRetries` nor `RetryFor` set. | `DecisionDefinitionError` |
| **Runtime init** | Task construction | `NewDecisionTask` | A declared input with no producer of matching name **and** declared type; an output name or `Id` that collides with another node in the task; a cross-node cycle. | `DecisionDefinitionError` |
| **Runtime** | Evaluation | `Evaluate` | A returned value whose runtime type disagrees with its declared output `Field.Type`; a missing or extra output key versus the declared `Outputs`. | `bl.TypeError` |

A non-nil error returned by `Fn` — or a panic inside `Fn`, which `Evaluate` recovers into an equivalent `Id`-tagged error — is a separate outcome, not a type error. `Evaluate` returns it directly, so a deliberate "no decision" or a downstream failure propagates to the caller — see [§ Error handling](#error-handling) for where it lands.

---

## Evaluation

`Evaluate` is stateless: the node is immutable after construction, and `Fn` receives a fresh input bag each call, so concurrency-safety is a property of `Fn` itself. Each call:

1. Passes the supplied `input` map to `Fn`.
2. Runs `Fn`. On success it returns a `map[string]BlValue` of produced values. On a non-nil error: if a `Retry` config is set, `Evaluate` re-runs `Fn` per that config (see [§ Retry](#retry)); once retries are exhausted — or immediately, when no `Retry` is set — the error is returned, tagged with the node `Id`.
3. Validates the returned map against the declared `Outputs`: every declared output must be present and of its declared `Field.Type`; a missing key, an extra key, or a wrong-typed value is a `bl.TypeError`. The validated map is returned.

If `Fn` **panics** instead of returning, `Evaluate` recovers the panic and converts it into an `Id`-tagged error, handled identically to an error `Fn` returned — it is subject to `Retry` and surfaces the same way (see [§ Error handling](#error-handling)). This keeps a buggy `Fn` from crashing the program, and is what makes a `Concurrent` node panic-safe (an unrecovered panic in a goroutine cannot be caught from outside it). The unrecoverable exceptions — fatal runtime conditions (e.g. concurrent map writes, out-of-memory, stack overflow), `os.Exit`, and panics in goroutines `Fn` itself spawns — still crash the process.

Because the function is opaque, blkit checks only the contract at its boundary, never the body. Step 3 is what guarantees a `DecisionNativeFunction` honours its declared `Outputs()` the same way every other node does, so a `DecisionTask` can drive it uniformly.

### Standalone vs. within a task

The input map handed to `Evaluate` is the same map of `Inputs()` values regardless of how the node is driven; only its source differs:

- **Standalone** — with no containing task, the caller supplies every value the node's `Inputs()` declare directly to `Evaluate`.
- **Within a `DecisionTask`** — the task resolves each declared input from the producer it was wired to at task construction (an upstream node output, a task input, or reference data) and passes the assembled map to `Evaluate`.

Either way `Evaluate` behaves identically: it hands the inputs to `Fn` and validates the result.

---

## Retry

By default a non-nil error from `Fn` is returned immediately. Set `Retry` to re-run `Fn` on error instead, using blkit's shared [`RetryConfig`](../processes/process.spec.md) — the same type and semantics processes and the process-layer native-function task already use, so retry behaves consistently across the stack.

```go
var creditScore = bl.NewDecisionNativeFunction(bl.DecisionNativeFunctionConfig{
    Id:      "credit_score",
    Name:    "Credit Score",
    Inputs:  []bl.Field{ /* … */ },
    Outputs: []bl.Field{ /* … */ },
    Fn:      scoreApplicant,
    Retry:   bl.NewRetryConfig(bl.RetryOpts{MaxRetries: 3, ExponentialBackoff: true}),
})
```

On a non-nil error `Evaluate` waits `RetryDelay` (doubling each time when `ExponentialBackoff` is set) and runs `Fn` again, up to `MaxRetries` attempts and/or for `RetryFor`, whichever limit is reached first. The first successful attempt's outputs are validated and returned as usual; if every attempt fails, the **last** error is returned, tagged with the node `Id`. A `Retry` with neither `MaxRetries` nor `RetryFor` set is rejected at construction — that would mean unbounded retries — matching the `RetryConfig` rules in [process.spec.md](../processes/process.spec.md).

Two things to keep in mind:

- **Retry fires on *any* `Fn` error.** A node that uses `Fn`'s error as a deliberate "no result" signal (see [Validation and type safety](#validation-and-type-safety)) should not set `Retry`, or it will re-run the function on that intended outcome.
- **Retry covers a returned error and a recovered panic.** `Evaluate` recovers a panic in `Fn` into an `Id`-tagged error (see [§ Error handling](#error-handling)), which is then retried like any other error. The unrecoverable exceptions — fatal runtime conditions, `os.Exit`, panics in goroutines `Fn` spawns — are not turned into errors and so are not retried.

When the node is also `Concurrent`, the retry loop runs inside its goroutine; the task joins only the final outcome — the first success, or the last error after the budget is exhausted.

---

## Error handling

A `DecisionNativeFunction` does not *resolve* an `Fn` error itself (beyond [retrying](#retry)); it **surfaces** it. Where that error lands depends on how the node is driven — and in a real deployment it is handled declaratively, by composition, not by imperative `if err != nil` code:

- **Standalone** — `Evaluate` returns the `Id`-tagged error to its direct caller, which inspects it like any Go error.
- **Within a `DecisionTask`** — the error aborts the decision: the task stops (no later node runs) and `DecisionTask.Evaluate` returns the same `Id`-tagged error, so the **whole `DecisionTask` fails**. The `Id` identifies which node failed.
- **Within a process** — the usual deployment, where a worker runs a `Process` whose graph contains the `DecisionTask`. The failed-task error reaches blkit's process engine, and from there the **process graph** decides what happens:
  - if the `DecisionTask` has an [`ErrorExitPort`](../processes/task-nodes.spec.md) wired (`bl.WithExitPorts(bl.NewErrorExitPort("…"))`, routed via `task.ExitPort(id).To(…)`), flow follows that port to a recovery branch — this is the declarative "catch";
  - otherwise the error is unhandled and the process instance ends as `ProcessStatusFailed`; how the originating request is then retried, redelivered, or dead-lettered is the worker / message-gateway layer's concern.

So a fully composed decision has a layered, configured error path — node-level [`Retry`](#retry), then a `DecisionTask` `ErrorExitPort`, then process- and worker-level policy — and no call site to hand-write. A node can also stay off the error path entirely by returning a *normal* output (e.g. a `status` value) that downstream logic branches on, instead of returning an error (the deliberate "no result" pattern — which is why such a node should not set `Retry`).

> **Panics are recovered into errors.** `Evaluate` wraps the `Fn` call in a deferred `recover`: a panic inside `Fn` is caught and returned as an `Id`-tagged error, indistinguishable downstream from an error `Fn` returned — so it is subject to `Retry`, surfaces as a `DecisionTask` failure, and is catchable at an `ErrorExitPort`. A buggy `Fn` therefore does **not** crash the program, and a `Concurrent` node is panic-safe (the recover runs inside its goroutine, the only place a goroutine's panic can be caught). The exceptions are genuinely unrecoverable and still crash the process: **fatal runtime conditions** (concurrent map writes, out-of-memory, stack overflow), **`os.Exit`**, and **panics in goroutines `Fn` itself spawns** (a different goroutine, outside `Evaluate`'s reach).

---

## Concurrent execution

By default a node is evaluated synchronously: a containing [`DecisionTask`](decision-task.spec.md) calls its `Evaluate` in topological order and blocks until it returns. Setting `Concurrent: true` lets the task overlap this node with others instead.

When the task reaches a concurrent node, it resolves the node's declared inputs, launches `Evaluate` in a goroutine, and carries on evaluating later nodes that do **not** depend on this one. It joins the goroutine — merging its outputs, or surfacing its error — before evaluating the first later node that consumes one of this node's outputs, and joins any still-running concurrent nodes before assembling the `DecisionResult`.

This is purely a **scheduling** change. Because the join always precedes any consumer, every node sees exactly the context it would under sequential evaluation, so the `DecisionResult` — and any error — is identical; only the wall-clock overlap differs. It is worth turning on for a node whose `Fn` does heavy or slow work (scoring a large model, a long computation) that can run while the task makes progress on independent branches.

The node's input values are captured when the goroutine is launched, so `Fn` never reads the task's evolving shared context concurrently. Beyond that, `Fn` must be safe to run alongside the other nodes' work — the I/O and side-effect guidance from the introduction applies with extra force: keep it a pure function of its inputs, and any shared mutable state or unsynchronised side effect is the author's responsibility.

A standalone `Evaluate` call ignores `Concurrent`: a lone node has nothing to overlap with, so it runs synchronously.

---

## Markdown Rendering

`ToMarkdown()` returns a markdown string with, in order: the node's name as a heading; its `Description`, if set; a `Logic` line identifying the implementation as native Go (there is no source expression to render) and naming the bound `Fn`; and the input and output contracts as two typed tables under `Inputs` and `Outputs` subheadings.

```go
fmt.Println(creditScore.ToMarkdown())
```

Output:

```text
### Credit Score

_Scores an applicant's creditworthiness._

**Logic:** native Go function `scoreApplicant`

#### Inputs

| Name   | Type   |
|--------|--------|
| age    | Number |
| income | Number |

#### Outputs

| Name  | Type   |
|-------|--------|
| score | Number |
```

The `Logic` line names `Fn` by its Go function name, recovered by reflection (`runtime.FuncForPC`): a named function shows its name, an anonymous function literal a synthetic one (`func1`). The full source is not available — a compiled `func` carries no source text — so only the name is shown. The description line appears only when `Description` is set. When the node has `Concurrent: true`, the line appends `· runs concurrently`:

```text
**Logic:** native Go function `scoreApplicant` · runs concurrently
```

---

## Edge Cases

- A `DecisionNativeFunction` whose `Fn` is nil is invalid; `NewDecisionNativeFunction` raises `DecisionDefinitionError`.
- A `DecisionNativeFunction` whose `Outputs` is empty is invalid; `NewDecisionNativeFunction` raises `DecisionDefinitionError`.
- A duplicate name within `Inputs` or within `Outputs` is a `DecisionDefinitionError`.
- `Fn` returning a map missing a declared output, or carrying a key that is not a declared output, is a `bl.TypeError` at evaluation.
- `Fn` returning a value whose runtime type disagrees with its declared output type is a `bl.TypeError` at evaluation.
- `Fn` returning a non-nil error is valid and expected: `Evaluate` returns that error (tagged with the node `Id`) — it is the node's way to signal a runtime failure or a deliberate no-result.
- The function body is the author's responsibility: blkit validates only the contract boundary, never the logic inside `Fn`.
- `Concurrent` defaults to false. A standalone `Evaluate` ignores it and runs synchronously; it only affects scheduling inside a `DecisionTask`.
- A concurrent node produces the same outputs, and surfaces the same errors (joined before any consumer and before the final result), as if it had run sequentially — only the timing differs. Running `Fn` concurrently with the rest of the graph is safe only if `Fn` itself is; that is the author's responsibility.
- `Retry` defaults to nil (no retry). With it set, every non-nil `Fn` error is retried per the `RetryConfig` until `MaxRetries`/`RetryFor` is exhausted, after which the last error is returned (`Id`-tagged). A `Retry` with neither limit set is a `DecisionDefinitionError`.
- `Retry` retries *any* error — including one recovered from a panic — so it should not be combined with using an `Fn` error as a deliberate no-result.
- A panic inside `Fn` is recovered by `Evaluate` and returned as an `Id`-tagged error, so it does not crash the program and flows through `Retry` and the `DecisionTask`'s `ErrorExitPort` like any other `Fn` error. The exceptions — fatal runtime conditions (concurrent map writes, out-of-memory, stack overflow), `os.Exit`, and panics in goroutines `Fn` itself spawns — remain unrecoverable and crash the process.
