---
name: DecisionNativeFunction
description: A generic DecisionNode[I, O] whose logic is an arbitrary native Go function func(I) (O, error) over concrete input/output structs of bl.Handle fields. Evaluate hands the function the typed input struct and returns the typed output struct it produces. The escape hatch for a decision's pure computation that is neither a table nor an expression — a bespoke algorithm, calculation, or model, with compile-time-typed inputs and outputs. It runs Fn exactly once, with no retry or concurrent-execution config; fallible I/O belongs in a process-layer native-function task instead.
status: implemented
code:
  - core/decision_native_fn.go
---

# DecisionNativeFunction

A `DecisionNativeFunction[I, O]` is a [`DecisionNode[I, O]`](decision-node.spec.md) whose logic is a plain Go function `func(I) (O, error)`. Like every node it declares its contracts as concrete Go structs whose fields are `bl.Handle` variables (see [decision-node.spec.md § Contracts are concrete Go structs](decision-node.spec.md#contracts-are-concrete-go-structs)): `I`'s fields are the variables it consumes, `O`'s the values it produces. `Evaluate` hands the function the typed input struct and returns the typed output struct.

It is the **escape hatch** of the decision family. Where [`DecisionTable`](decision-table.spec.md) expresses logic as tabular rules and [`DecisionExpression`](decision-expression.spec.md) as named text expressions, a `DecisionNativeFunction` expresses it as ordinary Go — and because `I` and `O` are concrete structs, the function body reads `in.Age.Get()` and returns `Out{Score: bl.NewHandle(score)}` with full Go type-checking. When a decision needs something neither other form can do — run a bespoke algorithm, a calculation, or a model (a pressure-relief-valve equation, a PMML or ONNX scorer) — you write it in `Fn`.

## Intended use: pure computation, not I/O

A `DecisionNativeFunction` is meant for an **algorithm, calculation, or model** that is simply too hard to express as a table or expression — a self-contained computation over its declared inputs. That is the whole reason it exists.

Because `Fn` is ordinary Go, nothing *stops* it from reaching outside pure computation — calling an external service, querying a database, reading or writing storage — but that is **not the intent**. Work like that belongs in a process-layer **native-function task** ([native-function-task.spec.md](../processes/native-function-task.spec.md)), which is built for it: it runs against an `ExecutionContext`, and it carries the operational controls such work needs — retry, timeout, and concurrency. A `DecisionNativeFunction` deliberately carries **none** of those:

- **No retry.** A decision native function runs its `Fn` exactly once; there is no retry config. Retrying is a property of fallible I/O, which belongs in a native-function task. (If a node uses an `Fn` error as a deliberate "no result" signal, single-shot evaluation is exactly what you want anyway.)
- **No concurrent execution.** A decision native function is always evaluated synchronously, in dependency order, by its containing task. There is no way to mark it to run in a goroutine. Overlapping slow I/O with other work is, again, a native-function-task concern.

So keep `Fn` a pure function of its inputs: feed any external data in as declared inputs (resolved from reference data or an upstream node) rather than fetching it inside `Fn`. A pure node is easier to test, cache, and reason about, and it keeps the decision's result from depending on state the framework cannot see.

```go
// The node is opaque apart from its In/Out port surfaces; it is built from a
// config plus the typed function. I and O are inferred from Fn, so call sites
// need no explicit type arguments.
type DecisionNativeFunction[I, O any] struct {
    In  I // input port surface for wiring
    Out O // output port surface for wiring
    // unexported: id, name, description, fn
}

func NewDecisionNativeFunction[I, O any](
    config DecisionNativeFunctionConfig,
    fn func(I) (O, error),
) *DecisionNativeFunction[I, O]

type DecisionNativeFunctionConfig struct {
    Id          string
    Name        string
    Description string
}

// DecisionNode[I, O] interface satisfaction.
func (d *DecisionNativeFunction[I, O]) GetId() string
func (d *DecisionNativeFunction[I, O]) GetName() string
func (d *DecisionNativeFunction[I, O]) GetDescription() string
func (d *DecisionNativeFunction[I, O]) Inputs() []Field  // reflected from I
func (d *DecisionNativeFunction[I, O]) Outputs() []Field // reflected from O

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
    Age   bl.Handle[bl.BlNumber] `expr:"age"`
    Steps bl.Handle[bl.BlNumber] `expr:"steps"`
}
type ScoreOutputs struct {
    Score bl.Handle[bl.BlNumber] `expr:"score"`
}

// scoreFitness runs the model and returns the typed output struct.
func scoreFitness(in ScoreInputs) (ScoreOutputs, error) {
    var score = scoreModel(in.Age.Get(), in.Steps.Get()) // any Go: a bespoke algorithm, a PMML/ONNX model, …
    return ScoreOutputs{Score: bl.NewHandle(score)}, nil
}

var fitnessScore = bl.NewDecisionNativeFunction(bl.DecisionNativeFunctionConfig{
    Id:          "fitness_score",
    Name:        "Fitness Score",
    Description: "Scores a member's cardio fitness.",
}, scoreFitness)
// I, O are inferred from scoreFitness — no explicit [ScoreInputs, ScoreOutputs].

var age, _   = bl.Number(42)
var steps, _ = bl.Number(90000)
var out, _   = fitnessScore.Evaluate(ScoreInputs{
    Age:   bl.NewHandle(age),
    Steps: bl.NewHandle(steps),
})
// out.Score.Get() is a bl.BlNumber
```

`age` and `steps` are this node's input variables; inside a task they are fed from a task input or an upstream node's output by wiring a producer's output handle to `fitnessScore.In.Age` / `fitnessScore.In.Steps`. `score` is the produced output; downstream nodes consume it by wiring `fitnessScore.Out.Score` to one of their inputs.

### Multiple outputs

```go
type ZoneInputs struct {
    Score   bl.Handle[bl.BlNumber] `expr:"score"`
    History bl.Handle[bl.BlList]   `expr:"history"`
}
type ZoneOutputs struct {
    Zone   bl.Handle[bl.BlString] `expr:"zone"`
    Target bl.Handle[bl.BlNumber] `expr:"target"`
}

// assessFitness derives the training zone and target from the score and history.
func assessFitness(in ZoneInputs) (ZoneOutputs, error) {
    var zone, target = assess(in.Score.Get(), in.History.Get())
    return ZoneOutputs{
        Zone:   bl.NewHandle(zone),
        Target: bl.NewHandle(target),
    }, nil
}

var zoneAssessment = bl.NewDecisionNativeFunction(bl.DecisionNativeFunctionConfig{
    Id:   "zone_assessment",
    Name: "Zone Assessment",
}, assessFitness)
```

The Go compiler guarantees `assessFitness` returns a `ZoneOutputs` with both declared fields, each of its declared type — there is no runtime "missing output" class as there was with a map return.

---

## Validation and type safety

A `DecisionNativeFunction` gets its safety from the family's three moments (see [decision-node.spec.md § Where type-safety happens](decision-node.spec.md#where-type-safety-happens)). Because `I` and `O` are concrete structs, **more moves to Go compile time** here than for any other node kind: the function body's reads (`in.Age.Get()`) and writes (`Out{Score: …}`) are type-checked, and so is the shape of every result. The reflection contract (handles, names) and a non-nil `Fn` are checked at construction; the body itself is opaque to blkit — ordinary Go, the author's responsibility.

Following the decision-family convention, `NewDecisionNativeFunction` does not return an error — it accumulates every construction problem and **panics once** with a `*DecisionDefinitionError`. Because a node is typically a package-scope `var`, that panic fires during package initialisation, at program (or test-binary) startup — deterministic load-time fail-fast.

What each moment catches, across three phases — compile, runtime init, and runtime:

| Phase | Moment | Trigger | What it catches | Raised as |
|-------|--------|---------|-----------------|-----------|
| **Compile** | Go compilation | `go build` | A caller passing the wrong input struct to `Evaluate`, or reading an output field that does not exist; an `Fn` whose signature is not `func(I) (O, error)`; a body reading or writing a handle field of the wrong type; a `bl.Edge` connecting one of this node's handles to a handle of a different type. | Go type error |
| **Runtime init** | Node construction | `NewDecisionNativeFunction` | A non-struct `I`/`O`; an unexported field; a field that is not a `bl.Handle[BlValue]`; an invalid or duplicated variable name within a struct; an empty `O`; a nil `Fn`. | `DecisionDefinitionError` |
| **Runtime** | Evaluation | `Evaluate` | Not a type error: `Fn` returning a non-nil error, or panicking (recovered into an `Id`-tagged error). The result's shape and field types are already guaranteed by `O`; an output handle the body leaves unset reads as `bl.BlNull`. | `error` (Id-tagged) |

A non-nil error returned by `Fn` — or a panic inside `Fn`, which `Evaluate` recovers into an equivalent `Id`-tagged error — is a separate outcome from any contract problem. `Evaluate` returns it directly, so a deliberate "no decision" or a downstream failure propagates to the caller — see [§ Error handling](#error-handling) for where it lands.

---

## Evaluation

`Evaluate` is stateless: the node is immutable after construction, and `Fn` receives a fresh typed input each call, so concurrency-safety is a property of `Fn` itself. Each call:

1. Passes the supplied input struct `I` to `Fn`.
2. Runs `Fn` **exactly once**. On success it returns the produced `O`. On a non-nil error it returns that error immediately, tagged with the node `Id` — there is no retry (see [§ Intended use](#intended-use-pure-computation-not-io)).

If `Fn` **panics** instead of returning, `Evaluate` recovers the panic and converts it into an `Id`-tagged error, handled identically to an error `Fn` returned, and surfaces the same way (see [§ Error handling](#error-handling)). This keeps a buggy `Fn` from crashing the program. The unrecoverable exceptions — fatal runtime conditions (e.g. concurrent map writes, out-of-memory, stack overflow), `os.Exit`, and panics in goroutines `Fn` itself spawns — still crash the process.

Because the function is opaque, blkit checks only its contract — guaranteed at the boundary by `I`/`O` — never the body.

### Standalone vs. within a task

`Evaluate` behaves identically however the node is driven; only the source of its inputs differs:

- **Standalone** — the caller builds a typed `I` with value-carrying handles (`bl.NewHandle(...)`) and passes it directly.
- **Within a `DecisionTask`** — the task populates this node's input handles from the producers wired to them at construction, runs the node, and routes its output handles onward (see [decision-task.spec.md § Evaluation](decision-task.spec.md#evaluation)).

---

## Error handling

A `DecisionNativeFunction` does not *resolve* an `Fn` error itself; it **surfaces** it. Where that error lands depends on how the node is driven — and in a real deployment it is handled declaratively, by composition, not by imperative `if err != nil` code:

- **Standalone** — `Evaluate` returns the `Id`-tagged error to its direct caller, which inspects it like any Go error.
- **Within a `DecisionTask`** — the error aborts the decision: the task stops (no later node runs) and `DecisionTask.Evaluate` returns the same `Id`-tagged error, so the **whole `DecisionTask` fails**. The `Id` identifies which node failed.
- **Within a process** — the usual deployment, where a worker runs a `Process` whose graph contains the `DecisionTask`. The failed-task error reaches blkit's process engine, and from there the **process graph** decides what happens:
  - if the `DecisionTask` has an [`ErrorExitPort`](../processes/task-nodes.spec.md) wired (`bl.WithExitPorts(bl.NewErrorExitPort("…"))`, routed via `task.ExitPort(id).To(…)`), flow follows that port to a recovery branch — this is the declarative "catch";
  - otherwise the error is unhandled and the process instance ends as `ProcessStatusFailed`; how the originating request is then retried, redelivered, or dead-lettered is the worker / message-broker layer's concern.

So a fully composed decision has a layered, configured error path — a `DecisionTask` `ErrorExitPort`, then process- and worker-level policy — and no call site to hand-write. A node can also stay off the error path entirely by returning a *normal* output (e.g. a `status` value) that downstream logic branches on, instead of returning an error (the deliberate "no result" pattern).

> **Panics are recovered into errors.** `Evaluate` wraps the `Fn` call in a deferred `recover`: a panic inside `Fn` is caught and returned as an `Id`-tagged error, indistinguishable downstream from an error `Fn` returned — so it surfaces as a `DecisionTask` failure and is catchable at an `ErrorExitPort`. A buggy `Fn` therefore does **not** crash the program. The exceptions are genuinely unrecoverable and still crash the process: **fatal runtime conditions** (concurrent map writes, out-of-memory, stack overflow), **`os.Exit`**, and **panics in goroutines `Fn` itself spawns** (a different goroutine, outside `Evaluate`'s reach).

---

## Markdown Rendering

`ToMarkdown()` returns a markdown string with, in order: the node's name as a heading; its `Description`, if set; a `Logic` line identifying the implementation as native Go (there is no source expression to render) and naming the bound `Fn`; and the input and output contracts as two typed tables under `Inputs` and `Outputs` subheadings.

```go
fmt.Println(fitnessScore.ToMarkdown())
```

Output:

```text
### Fitness Score

_Scores a member's cardio fitness._

**Logic:** native Go function `scoreFitness`

#### Inputs

| Name  | Type   |
|-------|--------|
| age   | Number |
| steps | Number |

#### Outputs

| Name  | Type   |
|-------|--------|
| score | Number |
```

The `Logic` line names `Fn` by its Go function name, recovered by reflection (`runtime.FuncForPC`): a named function shows its name, an anonymous function literal a synthetic one (`func1`). The full source is not available — a compiled `func` carries no source text — so only the name is shown. The description line appears only when `Description` is set.

---

## Edge Cases

- A `DecisionNativeFunction` whose `Fn` is nil is invalid; `NewDecisionNativeFunction` raises `DecisionDefinitionError`.
- A `DecisionNativeFunction` whose `O` has no fields is invalid; `NewDecisionNativeFunction` raises `DecisionDefinitionError`.
- An `I`/`O` field that is not a `bl.Handle[BlValue]`, or an unexported field, is a `DecisionDefinitionError`.
- A duplicate variable name within `I` or within `O` is a `DecisionDefinitionError`. Names need not be unique across nodes.
- `Fn` returning a non-nil error is valid and expected: `Evaluate` returns that error (tagged with the node `Id`) — it is the node's way to signal a runtime failure or a deliberate no-result.
- An output handle that `Fn` leaves unset reads as `bl.BlNull`; the result shape and field types are otherwise guaranteed by `O` at compile time.
- The function body is the author's responsibility: blkit validates only the contract boundary, never the logic inside `Fn`.
- `Fn` runs exactly once per `Evaluate`: a `DecisionNativeFunction` has no retry and no concurrent-execution config, by design (see [§ Intended use](#intended-use-pure-computation-not-io)). Fallible I/O that needs retry or overlap belongs in a process-layer [native-function task](../processes/native-function-task.spec.md).
- A panic inside `Fn` is recovered by `Evaluate` and returned as an `Id`-tagged error, so it does not crash the program and flows through the `DecisionTask`'s `ErrorExitPort` like any other `Fn` error. The exceptions — fatal runtime conditions (concurrent map writes, out-of-memory, stack overflow), `os.Exit`, and panics in goroutines `Fn` itself spawns — remain unrecoverable and crash the process.
