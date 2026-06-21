---
name: DecisionNode
description: The minimal common interface every decision node satisfies — identity (Id, Name, Description), a declared input contract and output contract (both []Field), and a uniform Evaluate that takes named BlValues and returns named BlValues. This interface is what lets a DecisionTask hold and run a mixed set of DecisionTables and DecisionExpressions.
targets:
  - ../../core/decision_node.go
---

# DecisionNode

`DecisionNode` is the single interface every node in a [`DecisionTask`](decision-task.spec.md) satisfies. It is small and deliberately plain — it exists so that a task can hold a mixed set of node types in one `[]DecisionNode`, inspect each one's typed contract, and run it, without knowing which concrete kind it is.

The two concrete node types are:

- [`DecisionTable`](decision-table.spec.md) — tabular input/output rules with hit policies.
- [`DecisionExpression`](decision-expression.spec.md) — named text-expression entries.

```go
type DecisionNode interface {
    GetId() string
    GetName() string
    GetDescription() string

    // Inputs is the node's input contract: the named, typed variables it
    // consumes from outside itself (task inputs, upstream node outputs, or
    // reference data). Used by NewDecisionTask to wire and type-check edges.
    Inputs() []Field

    // Outputs is the node's output contract: the named, typed values it
    // produces. Output names are unique across the whole DecisionTask.
    Outputs() []Field

    // Evaluate runs the node against a bag of named values and returns a bag of
    // named values keyed by the node's output names. The signature is identical
    // for every node type, which is what lets a DecisionTask drive them
    // uniformly.
    Evaluate(input map[string]BlValue) (map[string]BlValue, error)
}
```

Both concrete types satisfy this interface, so `[]DecisionNode{eligibility, approval, …}` holds nodes with different input and output shapes uniformly.

A static value with identity is **not** a node: it is a [`ReferenceData`](reference-data.spec.md) value source, which carries its `Value` rather than computing one. It has no `Evaluate` method and never appears in `[]DecisionNode`.

---

## Contracts are plain data, not Go generics

A node declares what it consumes and produces as ordinary data — a list of `Field` values (name + type), the same `Field` used by [`Schema`](../expressions/schema.spec.md):

```go
type Field struct {
    Name string
    Type Type
    // (Fields is used for nested dictionary shapes; see schema.spec.md)
}
```

There is **no** type-parameter on the node types and **no** reflected "outputs struct" — a node is configured with a plain `[]Field` for its inputs and a plain `[]Field` for its outputs. This is the whole reason the model is easy to reason about: a node's contract is a value you can read, print, and check, and the single moment type-safety "kicks in" is when those contracts are matched against each other (see [§ Where type-safety happens](#where-type-safety-happens)).

Rules for a contract `[]Field`:

- Every field has a non-empty `Name` and a known `Type` (a `bl.Type` other than `TypeNull`).
- No duplicate names within the same contract list.
- A node's output names must be unique across the whole `DecisionTask` (enforced at task construction), because the task binds every output into one shared, name-keyed evaluation context.

---

## Values flowing through Evaluate are BlValue

`Evaluate` takes and returns `map[string]BlValue`. Every value in those bags is a `BlValue` — a `BlNumber`, `BlString`, `BlBoolean`, `BlList`, `BlDictionary`, and so on. This is not an arbitrary `any`: each `BlValue` carries a `.Type()` tag, and that tag is what makes a value's type checkable against the declared contract types. A bag is keyed by name:

- the **input** bag is keyed by the names in `Inputs()`;
- the **output** bag is keyed by the names in `Outputs()`.

There is no special case for single-output nodes — a node with one output returns a one-entry map. Uniformity over convenience keeps the task's evaluation loop simple.

---

## Where type-safety happens

Decision type-safety is concentrated at **construction**, in two steps, and the mental model is one sentence: *if construction does not complain, the decision is well-formed.*

1. **Node construction** (`NewDecisionTable` / `NewDecisionExpression`) checks the one node it is given: the input and output contracts are well-formed; every expression compiles; and every name an expression references is a declared input or a sibling output. (Compilation is via `bl.Expr`, which — given a schema built from the node's declared inputs — also reports undefined-variable references for free; see [bl-expr.spec.md](../expressions/bl-expr.spec.md).)
2. **Task construction** (`NewDecisionTask`) checks the whole graph: it matches each node's `Inputs()` to a producer (an upstream node `Output`, a task input, or reference data) **by name and declared type**, draws the dependency edges, and rejects cycles, duplicate ids, and unresolved names.

What is **not** checked at construction is whether an expression's *computed* value actually matches its declared type (e.g. an output declared `TypeString` whose expression evaluates to a number). The blkit expression engine is runtime-typed — operators dispatch on operand types at evaluation, not compile time (see [operators in the engine](../expressions/bl-expr.spec.md)) — so a value-versus-declaration mismatch surfaces as a `bl.TypeError` at **evaluation**. Construction guarantees the declarations are mutually consistent; evaluation guarantees the values honour them.

---

## Identity

- `Id` — unique identifier within the containing `DecisionTask`. Duplicate ids are rejected at task construction.
- `Name` — optional human-readable label.
- `Description` — optional documentation text.

These live on each concrete node and are exposed through `GetId` / `GetName` / `GetDescription`.

---

## Evaluation within a task

`NewDecisionTask` stores nodes in topologically-sorted order. At evaluation time the task threads one shared, name-keyed context: it seeds the context with the caller's inputs and the bound reference data, then for each node in order calls `Evaluate` with the context and merges the returned outputs back into it under their output names. Because output names are unique across the task, a downstream node consumes an upstream output simply by declaring an input of that name. See [decision-task.spec.md § Evaluation](decision-task.spec.md#evaluation).

A node can also be evaluated standalone, outside any task, by calling `Evaluate` directly; the caller must supply every value the node's `Inputs()` declare.

---

## Edge Cases

- A node whose `Id` is empty is invalid; `NewDecisionTask` rejects it with `DecisionDefinitionError`.
- A node whose output contract has no fields is invalid (a node must declare at least one output).
- A duplicate name within a node's input or output contract is a `DecisionDefinitionError` at node construction.
- Two nodes producing the same output name in one task collide; `NewDecisionTask` rejects it.
- A node input whose declared type does not match its producer's declared output type is a `DecisionDefinitionError` at task construction.
- A value passed to `Evaluate` whose runtime type disagrees with the declared contract type surfaces as a `bl.TypeError` at evaluation time.
