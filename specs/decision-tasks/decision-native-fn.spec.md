---
name: DecisionNativeFunction
description: A DecisionNode whose logic is an arbitrary native Go function. Declares its input and output contracts as plain []Field data; Evaluate hands the named input values to the function and returns the named output values it produces. The escape hatch for logic that is neither a table nor an expression — call a model, a service, or any Go code.
targets:
  - ../../core/decision_native_fn.go
---

# DecisionNativeFunction

A `DecisionNativeFunction` is a [`DecisionNode`](decision-node.spec.md) whose logic is a plain Go function. It declares its contracts as plain data (see [decision-node.spec.md § Contracts are plain data](decision-node.spec.md#contracts-are-plain-data-not-go-generics)): `Inputs` is the list of named, typed variables it consumes, and `Outputs` is the list of named, typed values it produces. `Evaluate` hands the function the named input values and returns the named output values it produces.

It is the **escape hatch** of the decision family. Where [`DecisionTable`](decision-table.spec.md) expresses logic as tabular rules and [`DecisionExpression`](decision-expression.spec.md) as named text expressions, a `DecisionNativeFunction` expresses it as ordinary Go. When a decision needs something neither form can do — score a PMML or ONNX model, call an external service, run a bespoke algorithm — you implement it in the function body. There are no special PMML/ONNX/model body forms: a native function that does the work is the one general mechanism.

```go
type NativeFn func(in map[string]BlValue) (map[string]BlValue, error)

type DecisionNativeFunction struct {
    Id          string
    Name        string
    Description string

    Fn NativeFn // the native logic; receives the input bag, returns the output bag

    // inputs / outputs hold the declared contracts; exposed via the
    // DecisionNode interface methods Inputs() / Outputs().
    inputs  []Field
    outputs []Field
}

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

    // Fn is the native logic. It receives the declared inputs keyed by name and
    // must return the declared outputs keyed by name.
    Fn NativeFn
}

// DecisionNode interface satisfaction.
func (d *DecisionNativeFunction) GetId() string
func (d *DecisionNativeFunction) GetName() string
func (d *DecisionNativeFunction) GetDescription() string
func (d *DecisionNativeFunction) Inputs() []Field
func (d *DecisionNativeFunction) Outputs() []Field

// Evaluate the function against the input variables, returning a map keyed by
// this node's output names (see decision-node.spec.md).
func (d *DecisionNativeFunction) Evaluate(input map[string]BlValue) (map[string]BlValue, error)

// Render as a markdown string
func (d *DecisionNativeFunction) ToMarkdown() string
```

`NewDecisionNativeFunction` validates the input and output contracts (well-formed names and types, no duplicates, non-empty `Outputs`) and that `Fn` is non-nil. A malformed contract or a nil `Fn` is a `DecisionDefinitionError`.

---

## Building a DecisionNativeFunction

### Single output

```go
var creditScore = bl.NewDecisionNativeFunction(bl.DecisionNativeFunctionConfig{
    Id:   "credit_score",
    Name: "Credit Score",
    Inputs: []bl.Field{
        {Name: "age", Type: bl.TypeNumber},
        {Name: "income", Type: bl.TypeNumber},
    },
    Outputs: []bl.Field{
        {Name: "score", Type: bl.TypeNumber},
    },
    Fn: func(in map[string]bl.BlValue) (map[string]bl.BlValue, error) {
        var age = in["age"].(bl.BlNumber)
        var income = in["income"].(bl.BlNumber)
        // arbitrary Go: score a PMML/ONNX model, call a service, run an algorithm…
        var score = scoreModel(age, income)
        return map[string]bl.BlValue{"score": score}, nil
    },
})

var result, err = creditScore.Evaluate(map[string]bl.BlValue{
    "age":    bl.Number(42),
    "income": bl.Number(90000),
})
// result is map[string]bl.BlValue{"score": <bl.BlNumber>}
```

`age` and `income` are declared inputs, resolved at task construction from the task's inputs or an upstream node's output. `score` is the produced output; downstream nodes consume it by declaring an input named `score`.

### Multiple outputs

```go
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
    Fn: func(in map[string]bl.BlValue) (map[string]bl.BlValue, error) {
        var band, premium = assess(in["score"].(bl.BlNumber), in["history"].(bl.BlList))
        return map[string]bl.BlValue{
            "band":    band,
            "premium": premium,
        }, nil
    },
})
```

`Fn` must return every declared output name (`band`, `premium`), each of its declared type.

---

## Validation and type safety

A `DecisionNativeFunction` carries its own contracts as plain data, so safety is contract-matching, not name-inference. Following the family rule (see [decision-node.spec.md § Where type-safety happens](decision-node.spec.md#where-type-safety-happens)), the checks are concentrated at **construction** — the mental model is *if construction does not complain, the node is well-formed* — with only value-versus-declaration correctness left to evaluation. The function body itself is opaque to the framework: it is ordinary Go, type-checked by `go build`, and its internal logic is the author's responsibility.

These are **runtime** checks, not Go compile-time ones. "Construction" means the moment the `NewDecisionNativeFunction` (or `NewDecisionTask`) constructor executes; a `DecisionNativeFunction`'s contracts are `[]Field` data, outside the Go type system, so a malformed node is rejected when its constructor runs, never by `go build`.

`NewDecisionNativeFunction` does not return an error: following the decision-family convention, it accumulates every construction-time problem and **panics once** with a `*DecisionDefinitionError` (see [decision-task.spec.md](decision-task.spec.md), which documents the same convention). Because a `DecisionNativeFunction` is typically declared as a package-scope `var` — including inside a package the application author writes — its construction runs during that package's **initialisation**, when the program (or its test binary) starts, before `main`. A malformed node therefore aborts the program at startup: the panic lists each problem and the stack trace pins the offending declaration. This is not compile-time safety, but it is deterministic **load-time fail-fast** — any run of the program, or any test that merely imports the declaring package, surfaces every construction error.

Three moments catch three distinct classes of problem:

| Moment | Trigger | What it catches | Raised as |
|--------|---------|-----------------|-----------|
| **Node construction** | `NewDecisionNativeFunction` | A malformed contract (an ill-formed `Inputs`/`Outputs` name or type, a duplicate name within either list, or an empty `Outputs`); a nil `Fn`. | `DecisionDefinitionError` |
| **Task construction** | `NewDecisionTask` | A declared input with no producer of matching name **and** declared type; an output name or `Id` that collides with another node in the task; a cross-node cycle. | `DecisionDefinitionError` |
| **Evaluation** | `Evaluate` | A returned value whose runtime type disagrees with its declared output `Field.Type`; a missing or extra output key versus the declared `Outputs`. | `bl.TypeError` |

A non-nil error returned by `Fn` is a separate, expected outcome — not a type error. `Evaluate` returns it directly (tagged with the node `Id`), so a deliberate "no decision" or downstream failure propagates to the caller.

---

## Evaluation

`Evaluate` is stateless: the `DecisionNativeFunction` is immutable after construction, and the function receives a fresh input bag each call, so concurrency-safety is a property of `Fn` itself.

1. The supplied `input` map is passed to `Fn` as its `in` argument.
2. `Fn` runs and returns a `map[string]BlValue` of produced values (or an error, which `Evaluate` returns directly).
3. The returned map is validated against the declared `Outputs`: every declared output must be present and of its declared `Field.Type`; a missing key, an extra key, or a value of the wrong type is a `bl.TypeError`. The validated map is returned.

Because the function is opaque, the framework cannot check its body — only the contract at its boundary. Step 3 is what guarantees a `DecisionNativeFunction` honours its declared `Outputs()` the same way every other node does, so a `DecisionTask` can drive it uniformly.

### Standalone vs. within a task

The input map handed to `Evaluate` is the same map of `Inputs()` values regardless of how the node is driven; only its source differs:

- **Standalone** — with no containing task, the caller supplies every value the node's `Inputs()` declare directly to `Evaluate`.
- **Within a `DecisionTask`** — the task resolves each declared input from the producer it was wired to at task construction (an upstream node output, a task input, or reference data) and passes the assembled map to `Evaluate`.

Either way `Evaluate` behaves identically: it hands the inputs to `Fn` and validates the result.

---

## Markdown Rendering

`ToMarkdown()` returns a markdown string showing the node's name, its input and output contracts, and a marker that the body is native Go (there is no source expression to render).

```go
fmt.Println(creditScore.ToMarkdown())
```

Output:

```text
### Credit Score

_Body: native Go function_

| Input  | Type   |
|--------|--------|
| age    | Number |
| income | Number |

| Output | Type   |
|--------|--------|
| score  | Number |
```

---

## Edge Cases

- A `DecisionNativeFunction` whose `Fn` is nil is invalid; `NewDecisionNativeFunction` raises `DecisionDefinitionError`.
- A `DecisionNativeFunction` whose `Outputs` is empty is invalid; `NewDecisionNativeFunction` raises `DecisionDefinitionError`.
- A duplicate name within `Inputs` or within `Outputs` is a `DecisionDefinitionError`.
- `Fn` returning a map missing a declared output, or carrying a key that is not a declared output, is a `bl.TypeError` at evaluation.
- `Fn` returning a value whose runtime type disagrees with its declared output type is a `bl.TypeError` at evaluation.
- `Fn` returning a non-nil error is valid and expected: `Evaluate` returns that error (tagged with the node `Id`) — it is the node's way to signal a runtime failure or a deliberate no-result.
- The function body is the author's responsibility: the framework validates only the contract boundary, never the logic inside `Fn`.
