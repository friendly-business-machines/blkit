---
name: DecisionNativeFunction
description: A generic DecisionNode[I, O] whose logic is an arbitrary native Go function func(I) (O, error) over concrete input/output structs of bl.Handle fields. Evaluate hands the function the typed input struct and returns the typed output struct it produces. The escape hatch for logic that is neither a table nor an expression — call a model, a service, or any Go code, with compile-time-typed inputs and outputs.
targets:
  - ../../core/decision_native_fn.go
---

# DecisionNativeFunction

A `DecisionNativeFunction[I, O]` is a [`DecisionNode[I, O]`](decision-node.spec.md) whose logic is a plain Go function `func(I) (O, error)`. Like every node it declares its contracts as concrete Go structs whose fields are `bl.Handle` variables (see [decision-node.spec.md § Contracts are concrete Go structs](decision-node.spec.md#contracts-are-concrete-go-structs)): `I`'s fields are the variables it consumes, `O`'s the values it produces. `Evaluate` hands the function the typed input struct and returns the typed output struct.

It is the **escape hatch** of the decision family. Where [`DecisionTable`](decision-table.spec.md) expresses logic as tabular rules and [`DecisionExpression`](decision-expression.spec.md) as named text expressions, a `DecisionNativeFunction` expresses it as ordinary Go — and because `I` and `O` are concrete structs, the function body reads `in.Age.Get()` and returns `Out{Score: bl.NewHandle(score)}` with full Go type-checking. When a decision needs something neither other form can do — run a bespoke algorithm, or score a PMML or ONNX model — you write it in `Fn`.

Because `Fn` is ordinary Go, it *can* also reach outside pure computation — call an external service, query a database, read from disk. That is technically supported but not especially encouraged: a node that performs I/O or carries side effects is harder to test, cache, and reason about than a pure one, and it makes the decision's result depend on state the framework cannot see. Prefer feeding such data in as declared inputs (resolved from reference data or an upstream node) and keeping `Fn` a pure function of its inputs; reach for in-`Fn` I/O only when that is genuinely impractical.

```go
// The node is opaque apart from its In/Out port surfaces; it is built from a
// config plus the typed function. I and O are inferred from Fn, so call sites
// need no explicit type arguments.
type DecisionNativeFunction[I, O any] struct {
    In  I // input port surface for wiring
    Out O // output port surface for wiring
    // unexported: id, name, description, fn, concurrent, retry
}

func NewDecisionNativeFunction[I, O any](
    config DecisionNativeFunctionConfig,
    fn func(I) (O, error),
) *DecisionNativeFunction[I, O]

type DecisionNativeFunctionConfig struct {
    Id          string
    Name        string
    Description string

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

// DecisionNode[I, O] interface satisfaction.
func (d *DecisionNativeFunction[I, O]) GetId() string
func (d *DecisionNativeFunction[I, O]) GetName() string
func (d *DecisionNativeFunction[I, O]) GetDescription() string
func (d *DecisionNativeFunction[I, O]) Inputs() []Field  // reflected from I
func (d *DecisionNativeFunction[I, O]) Outputs() []Field // reflected from O

// Concurrent reports whether a containing DecisionTask may evaluate this node in
// a goroutine (see § Concurrent execution).
func (d *DecisionNativeFunction[I, O]) Concurrent() bool

// Evaluate runs Fn against the typed input struct and returns the typed output
// struct. A panic in Fn is recovered and returned as an Id-tagged error rather
// than crashing the program.
func (d *DecisionNativeFunction[I, O]) Evaluate(in I) (O, error)

func (d *DecisionNativeFunction[I, O]) ToMarkdown() string

// DecisionDefinitionError reports one or more construction problems (shared
// across the decision family).
type DecisionDefinitionError struct {
    Node     string
    Problems []string
}
```

`NewDecisionNativeFunction` reflects over `I` and `O` to validate the contracts (the shared reflection contract: every field an exported `bl.Handle[BlValue]`, valid identifier names, no duplicate within a struct, `O` non-empty) and requires a non-nil `fn`. Every problem is accumulated and raised together as a `DecisionDefinitionError`. The function body itself is opaque to blkit: it is ordinary Go, checked by `go build`, and its logic is the author's responsibility.

---

## Building a DecisionNativeFunction

### Single output

```go
type ScoreInputs struct {
    Age    bl.Handle[bl.BlNumber] `expr:"age"`
    Income bl.Handle[bl.BlNumber] `expr:"income"`
}
type ScoreOutputs struct {
    Score bl.Handle[bl.BlNumber] `expr:"score"`
}

// scoreApplicant runs the model and returns the typed output struct.
func scoreApplicant(in ScoreInputs) (ScoreOutputs, error) {
    var score = scoreModel(in.Age.Get(), in.Income.Get()) // any Go: a bespoke algorithm, a PMML/ONNX model, …
    return ScoreOutputs{Score: bl.NewHandle(score)}, nil
}

var creditScore = bl.NewDecisionNativeFunction(bl.DecisionNativeFunctionConfig{
    Id:          "credit_score",
    Name:        "Credit Score",
    Description: "Scores an applicant's creditworthiness.",
}, scoreApplicant)
// I, O are inferred from scoreApplicant — no explicit [ScoreInputs, ScoreOutputs].

var age, _    = bl.Number(42)
var income, _ = bl.Number(90000)
var out, _    = creditScore.Evaluate(ScoreInputs{
    Age:    bl.NewHandle(age),
    Income: bl.NewHandle(income),
})
// out.Score.Get() is a bl.BlNumber
```

`age` and `income` are this node's input variables; inside a task they are fed from a task input or an upstream node's output by wiring a producer's output handle to `creditScore.In.Age` / `creditScore.In.Income`. `score` is the produced output; downstream nodes consume it by wiring `creditScore.Out.Score` to one of their inputs.

### Multiple outputs

```go
type RiskInputs struct {
    Score   bl.Handle[bl.BlNumber] `expr:"score"`
    History bl.Handle[bl.BlList]   `expr:"history"`
}
type RiskOutputs struct {
    Band    bl.Handle[bl.BlString] `expr:"band"`
    Premium bl.Handle[bl.BlNumber] `expr:"premium"`
}

// assessRisk derives the risk band and premium from the score and history.
func assessRisk(in RiskInputs) (RiskOutputs, error) {
    var band, premium = assess(in.Score.Get(), in.History.Get())
    return RiskOutputs{
        Band:    bl.NewHandle(band),
        Premium: bl.NewHandle(premium),
    }, nil
}

var riskAssessment = bl.NewDecisionNativeFunction(bl.DecisionNativeFunctionConfig{
    Id:   "risk_assessment",
    Name: "Risk Assessment",
}, assessRisk)
```

The Go compiler guarantees `assessRisk` returns a `RiskOutputs` with both declared fields, each of its declared type — there is no runtime "missing output" class as there was with a map return.

---

## Validation and type safety

A `DecisionNativeFunction` gets its safety from the family's three moments (see [decision-node.spec.md § Where type-safety happens](decision-node.spec.md#where-type-safety-happens)). Because `I` and `O` are concrete structs, **more moves to Go compile time** here than for any other node kind: the function body's reads (`in.Age.Get()`) and writes (`Out{Score: …}`) are type-checked, and so is the shape of every result. The reflection contract (handles, names) and a non-nil `Fn` are checked at construction; the body itself is opaque to blkit — ordinary Go, the author's responsibility.

Following the decision-family convention, `NewDecisionNativeFunction` does not return an error — it accumulates every construction problem and **panics once** with a `*DecisionDefinitionError`. Because a node is typically a package-scope `var`, that panic fires during package initialisation, at program (or test-binary) startup — deterministic load-time fail-fast.

What each moment catches, across three phases — compile, runtime init, and runtime:

| Phase | Moment | Trigger | What it catches | Raised as |
|-------|--------|---------|-----------------|-----------|
| **Compile** | Go compilation | `go build` | A caller passing the wrong input struct to `Evaluate`, or reading an output field that does not exist; an `Fn` whose signature is not `func(I) (O, error)`; a body reading or writing a handle field of the wrong type; a `bl.Edge` connecting one of this node's handles to a handle of a different type. | Go type error |
| **Runtime init** | Node construction | `NewDecisionNativeFunction` | A non-struct `I`/`O`; an unexported field; a field that is not a `bl.Handle[BlValue]`; an invalid or duplicated variable name within a struct; an empty `O`; a nil `Fn`; a `Retry` with neither `MaxRetries` nor `RetryFor` set. | `DecisionDefinitionError` |
| **Runtime** | Evaluation | `Evaluate` | Not a type error: `Fn` returning a non-nil error, or panicking (recovered into an `Id`-tagged error). The result's shape and field types are already guaranteed by `O`; an output handle the body leaves unset reads as `bl.BlNull`. | `error` (Id-tagged) |

A non-nil error returned by `Fn` — or a panic inside `Fn`, which `Evaluate` recovers into an equivalent `Id`-tagged error — is a separate outcome from any contract problem. `Evaluate` returns it directly, so a deliberate "no decision" or a downstream failure propagates to the caller — see [§ Error handling](#error-handling) for where it lands.

---

## Evaluation

`Evaluate` is stateless: the node is immutable after construction, and `Fn` receives a fresh typed input each call, so concurrency-safety is a property of `Fn` itself. Each call:

1. Passes the supplied input struct `I` to `Fn`.
2. Runs `Fn`. On success it returns the produced `O`. On a non-nil error: if a `Retry` config is set, `Evaluate` re-runs `Fn` per that config (see [§ Retry](#retry)); once retries are exhausted — or immediately, when no `Retry` is set — the error is returned, tagged with the node `Id`.

If `Fn` **panics** instead of returning, `Evaluate` recovers the panic and converts it into an `Id`-tagged error, handled identically to an error `Fn` returned — it is subject to `Retry` and surfaces the same way (see [§ Error handling](#error-handling)). This keeps a buggy `Fn` from crashing the program, and is what makes a `Concurrent` node panic-safe (an unrecovered panic in a goroutine cannot be caught from outside it). The unrecoverable exceptions — fatal runtime conditions (e.g. concurrent map writes, out-of-memory, stack overflow), `os.Exit`, and panics in goroutines `Fn` itself spawns — still crash the process.

Because the function is opaque, blkit checks only its contract — guaranteed at the boundary by `I`/`O` — never the body.

### Standalone vs. within a task

`Evaluate` behaves identically however the node is driven; only the source of its inputs differs:

- **Standalone** — the caller builds a typed `I` with value-carrying handles (`bl.NewHandle(...)`) and passes it directly.
- **Within a `DecisionTask`** — the task populates this node's input handles from the producers wired to them at construction, runs the node, and routes its output handles onward (see [decision-task.spec.md § Evaluation](decision-task.spec.md#evaluation)).

---

## Retry

By default a non-nil error from `Fn` is returned immediately. Set `Retry` to re-run `Fn` on error instead, using blkit's shared [`RetryConfig`](../processes/process.spec.md) — the same type and semantics processes and the process-layer native-function task already use, so retry behaves consistently across the stack.

```go
var creditScore = bl.NewDecisionNativeFunction(bl.DecisionNativeFunctionConfig{
    Id:    "credit_score",
    Name:  "Credit Score",
    Retry: bl.NewRetryConfig(bl.RetryOpts{MaxRetries: 3, ExponentialBackoff: true}),
}, scoreApplicant)
```

On a non-nil error `Evaluate` waits `RetryDelay` (doubling each time when `ExponentialBackoff` is set) and runs `Fn` again, up to `MaxRetries` attempts and/or for `RetryFor`, whichever limit is reached first. The first successful attempt's output is returned as usual; if every attempt fails, the **last** error is returned, tagged with the node `Id`. A `Retry` with neither `MaxRetries` nor `RetryFor` set is rejected at construction — that would mean unbounded retries — matching the `RetryConfig` rules in [process.spec.md](../processes/process.spec.md).

Two things to keep in mind:

- **Retry fires on *any* `Fn` error.** A node that uses `Fn`'s error as a deliberate "no result" signal should not set `Retry`, or it will re-run the function on that intended outcome.
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

By default a node is evaluated synchronously: a containing [`DecisionTask`](decision-task.spec.md) calls its `Evaluate` in dependency order and blocks until it returns. Setting `Concurrent: true` lets the task overlap this node with others instead.

When the task reaches a concurrent node, it populates the node's input handles, launches `Evaluate` in a goroutine, and carries on evaluating later nodes that do **not** depend on this one. It joins the goroutine — routing its outputs onward, or surfacing its error — before evaluating the first later node that consumes one of this node's outputs, and joins any still-running concurrent nodes before assembling the `DecisionResult`.

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
- A `DecisionNativeFunction` whose `O` has no fields is invalid; `NewDecisionNativeFunction` raises `DecisionDefinitionError`.
- An `I`/`O` field that is not a `bl.Handle[BlValue]`, or an unexported field, is a `DecisionDefinitionError`.
- A duplicate variable name within `I` or within `O` is a `DecisionDefinitionError`. Names need not be unique across nodes.
- `Fn` returning a non-nil error is valid and expected: `Evaluate` returns that error (tagged with the node `Id`) — it is the node's way to signal a runtime failure or a deliberate no-result.
- An output handle that `Fn` leaves unset reads as `bl.BlNull`; the result shape and field types are otherwise guaranteed by `O` at compile time.
- The function body is the author's responsibility: blkit validates only the contract boundary, never the logic inside `Fn`.
- `Concurrent` defaults to false. A standalone `Evaluate` ignores it and runs synchronously; it only affects scheduling inside a `DecisionTask`.
- A concurrent node produces the same outputs, and surfaces the same errors (joined before any consumer and before the final result), as if it had run sequentially — only the timing differs. Running `Fn` concurrently with the rest of the graph is safe only if `Fn` itself is; that is the author's responsibility.
- `Retry` defaults to nil (no retry). With it set, every non-nil `Fn` error is retried per the `RetryConfig` until `MaxRetries`/`RetryFor` is exhausted, after which the last error is returned (`Id`-tagged). A `Retry` with neither limit set is a `DecisionDefinitionError`.
- `Retry` retries *any* error — including one recovered from a panic — so it should not be combined with using an `Fn` error as a deliberate no-result.
- A panic inside `Fn` is recovered by `Evaluate` and returned as an `Id`-tagged error, so it does not crash the program and flows through `Retry` and the `DecisionTask`'s `ErrorExitPort` like any other `Fn` error. The exceptions — fatal runtime conditions (concurrent map writes, out-of-memory, stack overflow), `os.Exit`, and panics in goroutines `Fn` itself spawns — remain unrecoverable and crash the process.
