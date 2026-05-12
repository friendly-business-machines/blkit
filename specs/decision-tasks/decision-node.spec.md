---
name: DecisionNode
description: Abstract base type for all decision nodes — provides common identity, naming, output, and the typed-input declaration surface shared by DecisionTable, LiteralExpression, BoxedContext, Relation, and Invocation
targets:
  - ../decisions/decision_node.go
---

# DecisionNode

`DecisionNode` is the abstract base type for every node in a `DecisionTask`. It carries the common attributes — identity, human-readable name, declared output, and the typed-input declaration surface — that all concrete node types share. Concrete node types are:

- `DecisionTable` — tabular input/output rules with hit policies
- `LiteralExpression` — a single expression body
- `BoxedContext` — an ordered list of named entries with an optional final result
- `Relation` — tabular data that evaluates to a list of contexts
- `Invocation` — a call to a `BusinessKnowledgeModel` with parameter bindings

```go
type DecisionNode struct {
    Id          string
    Name        *string
    Description *string

    // Output variable name — the key under which this node's result is stored
    // in the model's evaluation context. Defaults to Id if not set.
    OutputName *string

    // Internal: the input registry populated by Require*/Optional* calls.
    // Authors do not write to this directly; it is exposed for introspection
    // (e.g. for DecisionTask validation).
}

// Evaluate this node against the provided input variables.
// Called by DecisionTask during graph evaluation; may also be called standalone.
func (d *DecisionNode) Evaluate(input map[string]any) (BlValue, error)
```

Concrete node types embed `DecisionNode` and add their own type-specific fields (`Body` on `LiteralExpression`, `Inputs`/`Rules` on `DecisionTable`, etc.).

---

## Constructing a node

Decision nodes are instantiated via **direct struct literals** — there is no `New*` factory function. Validation (non-empty `Id`, no duplicate `OutputName`, dependency-graph correctness) happens at `DecisionTask.AddNode` and `DecisionTask.Validate` time, not at struct-literal construction.

```go
calc := &LiteralExpression{
    Id:   "monthly_payment",
    Name: "Monthly Payment",
}
```

The constructor-function idiom (below) is the canonical way to build a node — direct struct literals are the building block inside that function.

---

## Typed-input declaration

Each decision node declares its required inputs via per-type `Require*` methods. Each call:

1. Records the input on the node's internal schema (used to derive dependencies and validate against the model's evaluation context).
2. Returns a typed Go reference (`BlNumber`, `BlString`, etc.) that resolves at evaluation time from the surrounding scope.

The captured ref is used directly inside the node's body / rules — there is no separate `Bl.NumberVar("loan_amount")` lookup.

```go
// Inherited by every concrete decision-node type.
func (d *DecisionNode) RequireNumber(name string) BlNumber
func (d *DecisionNode) RequireString(name string) BlString
func (d *DecisionNode) RequireBoolean(name string) BlBoolean
func (d *DecisionNode) RequireDate(name string) BlDate
func (d *DecisionNode) RequireTime(name string) BlTime
func (d *DecisionNode) RequireDateTime(name string) BlDateTime
func (d *DecisionNode) RequireDaysTime(name string) BlDaysTimeDuration
func (d *DecisionNode) RequireYearsMonths(name string) BlYearsMonthsDuration
func (d *DecisionNode) RequireList(name string) BlList
func (d *DecisionNode) RequireContext(name string, schema *ContextContract) BlContext
func (d *DecisionNode) RequireRange(name string) BlRange
func (d *DecisionNode) RequireCalendar(name string) BlCalendar

// Optional variants — same signatures; the ref evaluates to BlNull if the
// input is absent at runtime instead of producing a DecisionEvaluationError.
func (d *DecisionNode) OptionalNumber(name string) BlNumber
// ...etc.
```

`Require*` lazy-initializes the internal input registry on first call, so direct struct literals work without an explicit constructor.

---

## The constructor-function idiom

Each non-trivial decision node is built inside a **dedicated, domain-named Go function** that returns the constructed node. The function's body is the node's "scope" — it owns the typed refs and their use sites. This is the spec's canonical pattern:

```go
func monthlyPaymentCalc() *LiteralExpression {
    calc := &LiteralExpression{
        Id:   "monthly_payment",
        Name: "Monthly Payment",
    }
    loanAmount := calc.RequireNumber("loan_amount")
    rate       := calc.RequireNumber("rate")
    calc.Body = loanAmount.Multiply(rate).Divide(Bl.Number(12))
    return calc
}

func totalInterestCalc() *LiteralExpression {
    calc := &LiteralExpression{
        Id:   "total_interest",
        Name: "Total Interest",
    }
    loanAmount := calc.RequireNumber("loan_amount")  // local; no collision
    rate       := calc.RequireNumber("rate")
    term       := calc.RequireNumber("term_months")
    calc.Body = loanAmount.Multiply(rate).Multiply(term)
    return calc
}
```

Why this is the idiom (and not just a suggestion):

- **No package-level Go-name collisions.** Each function's locals are independent — `loanAmount` in one constructor cannot conflict with `loanAmount` in another, even in the same file.
- **One node per scope.** A reader sees the node's schema and body in a single block.
- **Testable in isolation.** Each constructor is a unit-testable function — call it, inspect the returned node.

Inline construction (passing `&LiteralExpression{...}` directly into `model.AddNode(...)`) is discouraged when the node has more than one or two typed inputs.

Function names are domain-named and **do not use the `new` prefix** — `monthlyPaymentCalc()`, not `newMonthlyPaymentCalc()`. The `new` prefix is reserved for generic factory functions; these are user-authored constructors for specific named nodes.

---

## Identity

- `Id` is a unique identifier within the containing `DecisionTask`. Duplicate ids are rejected by `DecisionTask.Validate()`.
- `Name` is an optional human-readable label (e.g., `"Eligibility Check"`).
- `Description` is optional documentation text.

## Dependencies

The node's dependencies are derived from its `Require*` calls — every required input field name becomes either:

- An `InputData` reference resolved from the model's caller-provided input variables, **or**
- An output reference to another `DecisionNode` whose `OutputName` (or `Id`) matches the field name.

When the model evaluates this node, all dependencies are evaluated first (transitively, in dependency order), and their outputs are merged into the input context available to this node.

A node that calls no `Require*` methods depends only on the empty input set — it is a constant-producing node.

## Output Name

When a node's result is stored in the evaluation context (for downstream nodes to consume), it is keyed by `OutputName`. If `OutputName` is `nil`, the `Id` is used as the key.

## Standalone Evaluation

Any `DecisionNode` can be evaluated independently by calling `.Evaluate(input)` directly, without a containing `DecisionTask`. In this case, the caller is responsible for providing all required input variables — dependency resolution does not occur.

## Edge Cases

- A `DecisionNode` whose `Id` is an empty string is invalid; `DecisionTask.Validate()` rejects it.
- A `DecisionNode` whose declared inputs (via `Require*`) form a circular dependency among nodes is detected by `DecisionTask.Validate()`.
- `OutputName` must be unique within a `DecisionTask`. Duplicate output names are rejected by `DecisionTask.Validate()`.
- Calling `.Evaluate()` with a named input variable not declared via `Require*` / `Optional*` produces a `DecisionEvaluationError`. Only variables declared on the node are accepted.
- Calling the same `Require*` method twice with the same `name` returns the same ref (idempotent registration).
- Calling `Require*` and `Optional*` for the same `name` is an authoring error and produces a `DecisionDefinitionError` at validation time.
- A `Require*` declaration whose evaluation-time value is the wrong type (e.g. a `RequireNumber("x")` declaration where `x` resolves to a string) produces a `BlTypeError` at evaluation time.
