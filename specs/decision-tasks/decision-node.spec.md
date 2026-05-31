---
name: DecisionNode
description: Common interface for every decision node — the shared identity surface (Id, Name, Description) and evaluation contract satisfied by the generic DecisionTable, LiteralExpression, BoxedContext, Relation, and Invocation types.
targets:
  - ../decisions/decision_node.go
---

# DecisionNode

`DecisionNode` is the interface every node in a `DecisionTask` satisfies. The five concrete node types are each generic over a caller-supplied outputs struct that declares the node's typed outputs:

- `DecisionTable[Outputs]` — tabular input/output rules with hit policies
- `LiteralExpression[Outputs]` — a single expression body
- `BoxedContext[Outputs]` — an ordered list of named entries with an optional final result
- `Relation[Outputs]` — tabular data that evaluates to a list of contexts
- `Invocation[Outputs]` — a call to a `BusinessKnowledgeModel` with parameter bindings

```go
type DecisionNode interface {
    GetId() string
    GetName() *string
    GetDescription() string
    Evaluate(input map[string]any) (BlValue, error)
}
```

Every concrete generic node type implements this interface regardless of its `Outputs` type parameter, so `[]DecisionNode{eligibility, approval, ...}` accepts nodes with different output shapes uniformly.

---

## Outputs structs

Each concrete node's typed outputs are declared by a caller-defined struct. **Naming convention:** the struct name ends in `Outputs` (e.g., `EligibilityOutputs`, `ApprovalOutputs`, `RiskScoreOutputs`).

```go
type EligibilityOutputs struct {
    Risk  BlString   // output column "risk"
    Score BlNumber   // output column "score"
}
```

Rules for the outputs struct:

- **Every exported field is an output.** No filter — the caller put it there, so it is an output.
- A field's static type must implement `BlValue` (`BlString`, `BlNumber`, `BlBoolean`, `BlDate`, `BlTime`, `BlDateTime`, `BlDaysTimeDuration`, `BlYearsMonthsDuration`, `BlList`, `BlDictionary`, `BlRange`, `BlCalendar`). A non-`BlValue` field type is a `DecisionDefinitionError` at construction time.
- The column name defaults to the lowercase field name; override with a `bl:"name"` struct tag.
- Single-output nodes use an outputs struct with exactly one field. Multi-output nodes use one field per output column.

When the constructor runs, every outputs-struct field is populated with a typed handle that downstream nodes reference to consume this node's output.

---

## Constructing a node

Each concrete type has a generic constructor: `NewDecisionTable[Outputs]`, `NewLiteralExpression[Outputs]`, `NewBoxedContext[Outputs]`, `NewRelation[Outputs]`, `NewInvocation[Outputs]`. The caller passes opts containing the node's logic (rules, body, entries, rows, bindings) and the type parameter pins the outputs struct.

```go
var eligibility = NewDecisionTable[EligibilityOutputs](DecisionTableOpts{
    Id:        "eligibility",
    Name:      "Eligibility Check",
    HitPolicy: HitPolicyUnique,
    Inputs:    []TableInput{ /* ... */ },
    Rules:     []Rule{ /* ... */ },
})
```

The constructor performs the following steps:

1. Allocate and populate the underlying node value from `opts`.
2. Reflect on the `Outputs` type parameter. For every exported field:
   - Verify the field's static type implements `BlValue`. Reject otherwise.
   - Derive the output's name (lowercased field name, or the `bl:"name"` tag if present).
   - Register the output on the underlying node.
   - Allocate a typed handle and assign it into the corresponding field of the returned value's `Outputs`.
3. Validate node-internal consistency (e.g., every rule output references a declared output; no duplicate output names on this node).
4. Return the constructed `*DecisionTable[Outputs]` (or analogous for other node types). Validation failures raise a `DecisionDefinitionError`.

The exact return type for the example above is `*DecisionTable[EligibilityOutputs]`. Access patterns:

```go
eligibility.Outputs.Risk      // BlString  — typed handle
eligibility.Outputs.Score     // BlNumber  — typed handle
eligibility.GetId()           // "eligibility"
eligibility.Evaluate(input)   // standalone evaluation
```

---

## Cross-node references

A downstream node references upstream outputs by reading the typed `Bl*` field through `.Outputs.X`. The field's static type carries through the Go compiler — type mismatches are caught at compile time.

```go
type ApprovalOutputs struct {
    Status BlString
}

var approval = NewLiteralExpression[ApprovalOutputs](LiteralExpressionOpts{
    Id:   "approval",
    Name: "Loan Approval",
    Body: Bl.If(
        eligibility.Outputs.Risk.Equals(Bl.String("high")),
        Bl.String("review"),
        Bl.String("approved"),
    ),
})
```

`NewDecisionTask` derives the dependency graph by walking each node's expression trees (`Rules`, `Body`, `Entries`, `Rows`, `Bindings`) and collecting output handles. Each handle carries a pointer to its source node, so producer→consumer edges are unambiguous and string-free.

---

## Identity

- `Id` — unique identifier within the containing `DecisionTask`. Duplicate ids are rejected.
- `Name` — optional human-readable label.
- `Description` — optional documentation text.

These fields live on each concrete node's struct and are exposed through the interface getter methods (`GetId`, `GetName`, `GetDescription`).

---

## Evaluation

A `DecisionNode` is evaluated by calling `Evaluate(input)`:

- **Single-output case** (outputs struct has exactly one field): returns that field's `BlValue` directly.
- **Multi-output case**: returns a `BlDictionary` keyed by the declared output names.

Within a `DecisionTask`, the runtime evaluates nodes in topologically sorted dependency order and stores each node's result in the evaluation context under the node's `Id`.

---

## Standalone Evaluation

Any decision node can be evaluated independently by calling `.Evaluate(input)` without a containing `DecisionTask`. The caller is responsible for providing all input variables referenced by the node's expressions. Dependency resolution against other nodes does not occur in standalone mode — references to other nodes' outputs must already be resolvable from the supplied `input`.

---

## Edge Cases

- A `DecisionNode` whose `Id` is an empty string is invalid; `NewDecisionTask` rejects it with `DecisionDefinitionError`.
- An outputs-struct field whose type does not implement `BlValue` is a `DecisionDefinitionError` at construction time.
- An outputs-struct with no exported fields is a `DecisionDefinitionError` (a node must declare at least one output).
- An outputs-struct field whose `bl:"name"` tag duplicates another field's effective name (within the same node) is a `DecisionDefinitionError`.
- Output names must be unique across the whole `DecisionTask`. Collisions are rejected at task construction.
- Decision nodes forming a circular dependency among themselves are detected by `NewDecisionTask` and rejected with `DecisionDefinitionError`.
- Calling `.Evaluate()` in standalone mode with an input variable not referenced by any expression on the node is silently ignored.
- A required input variable that is missing at evaluation time resolves to `BlNull`.
- A node body that produces a value whose runtime type disagrees with the declared outputs-struct field type produces a `BlTypeError` at evaluation time.
