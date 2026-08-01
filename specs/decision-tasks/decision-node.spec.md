---
name: DecisionNode
description: The generic interface every decision node satisfies — identity (Id, Name, Description), a typed input struct I and output struct O whose fields are bl.Handle values, and a typed Evaluate(in I) (O, error). Nodes declare their contracts as concrete Go structs reflected to []Field; a DecisionTask wires them into a compile-checked netlist by connecting their In/Out handle surfaces. A DecisionTask is itself a DecisionNode.
status: implemented
code:
  - core/decision_node.go
---

# DecisionNode

`DecisionNode[I, O]` is the interface every node in a [`DecisionTask`](decision-task.spec.md) satisfies. A node is generic over two concrete Go structs — an **input** struct `I` and an **output** struct `O` — and exposes a typed `Evaluate(in I) (O, error)`. Because every node carries the same shape, a task can hold a mixed set of node kinds, wire them together, and run them, while each kind keeps its own input and output types.

The concrete node kinds are:

- [`DecisionTable`](decision-table.spec.md) — tabular input/output rules with hit policies.
- [`DecisionExpression`](decision-expression.spec.md) — named text-expression entries.
- [`DecisionNativeFunction`](decision-native-fn.spec.md) — an arbitrary native Go function (the escape hatch for logic that is neither a table nor an expression).
- A [`DecisionTask`](decision-task.spec.md) **is itself** a `DecisionNode` (see [§ A DecisionTask is a node](#a-decisiontask-is-a-node)), so a whole child decision composes into a larger one as a single node — there is no separate wrapper type.

```go
type DecisionNode[I, O any] interface {
    GetId() string
    GetName() string
    GetDescription() string

    // Evaluate runs the node against a typed input struct and returns a typed
    // output struct. Identical in shape for every node kind, which is what lets a
    // DecisionTask drive them uniformly.
    Evaluate(in I) (O, error)
}
```

Each node value also exposes two **port surfaces** — `node.In` (an `I`-shaped surface) and `node.Out` (an `O`-shaped surface) — whose fields are the typed [handles](#handlet--the-typed-io-field) used to wire the node (see [§ Port surfaces](#port-surfaces)).

A static value with identity is **not** a node: it is a [`ReferenceData`](reference-data.spec.md) value source, which carries its constant rather than computing one. It exposes a single `.Value` handle so it can be wired, but it has no `Evaluate` and never runs.

---

## Contracts are concrete Go structs

A node declares what it consumes and produces as two concrete Go structs. Every exported field is one variable of the node's contract:

```go
type MembershipInputs struct {
    Age    bl.Handle[bl.BlNumber] `expr:"age"`
    Points bl.Handle[bl.BlNumber] `expr:"points"`
}
type Eligibility struct {
    Eligibility bl.Handle[bl.BlString] `expr:"eligibility"`
}
```

The constructor *reflects* over `I` and `O` — Go's `reflect` package lets code read a struct's fields at runtime — to discover the declared variables, their types, and their names. This is the **shared reflection contract** every node kind obeys; each failure is reported as a `DecisionDefinitionError`:

- `I` and `O` must be structs, and every field must be **exported** (an unexported field cannot be a variable);
- **every field must be a `bl.Handle[T]` whose `T` is a `BlValue`** (see [§ Handle\[T\]](#handlet--the-typed-io-field)). A field of any other Go type — a bare `bl.BlNumber`, an `int`, a plain struct — is rejected;
- **every variable name must be a valid expr identifier** — a letter or `_` followed by letters, digits, or `_`. The name is the field's `expr:"…"` tag, or its Go field name when untagged. A malformed tag (`expr:"full name"`, `expr:"1st"`) is rejected; an untagged field always passes, since a Go field name is a valid identifier;
- no variable name may be duplicated **within** the same struct. Names need **not** be unique across nodes: wiring connects specific handles, not names (see [§ Where type-safety happens](#where-type-safety-happens)), so two nodes may both produce an output called `result`.

The `expr:"…"` tag is *optional* and only renames a variable — a struct of plainly-named fields needs no tags. It is the name an expression node's sources reference (`age`, `eligibility`); the **Go field name** (`Age`, `Eligibility`) is what the wiring netlist uses (`node.In.Age`, `node.Out.Eligibility`).

> **Why a runtime contract, not a compile-time one?** Go's generics can constrain a *type parameter* but cannot say "a struct all of whose fields are `bl.Handle[BlValue]`" — field types are not expressible in the constraint system. So `[I, O any]` is the tightest signature available, and the `Handle` rule is enforced by reflection when the constructor runs. Because a node is typically a package-scope `var`, that check fails at program (or test-binary) startup — the same load-time fail-fast as every other contract rule here.

For wiring and rendering, a node still advertises its contracts as plain data: `Inputs()` and `Outputs()` each return a `[]Field` (the same `Field` used by [`Schema`](../expressions/schema.spec.md)), reflected from `I` and `O`.

```go
type Field struct {
    Name string
    Type Type
}
```

---

## Handle[T] — the typed I/O field

A `bl.Handle[T]` is one input or output field. It plays two roles:

- **At wiring time** it is a typed connection point. A node's constructor reflects over `I`/`O` and stamps each handle with its owning node and field name, so `eligibility.Out.Eligibility` is a `Handle[BlString]` that identifies exactly that output. Connecting two handles with [`bl.Edge`](decision-task.spec.md#wiring) requires their `T` to match, so a mis-typed connection is a **Go compile error**.
- **At evaluation time** it carries the value. `Evaluate` reads each input handle's value and writes each output handle's value; `Handle[T].Get()` returns the `T`.

```go
type Handle[T BlValue] struct { /* unexported: node, field, value */ }

// NewHandle wraps a value in a handle — used to build a typed input struct when
// calling Evaluate standalone. Inside a task, the engine populates handles itself.
func NewHandle[T BlValue](value T) Handle[T]

func (h Handle[T]) Get() T              // the value (evaluation time)
func (h *Handle[T]) Bind(node, field string) // stamp identity (construction time; called by the node constructor)
```

`Bind` is how the constructor stamps identity by reflection: the I/O *field* is exported, so the constructor calls `Bind` on each field even though `Handle`'s own state is unexported. No code generation and no parallel "port struct" is involved — the one struct you declare is both the value contract and the wiring surface.

---

## Port surfaces

A constructed node exposes its handles for wiring through two surfaces:

- `node.In` — a value of type `I` whose handles are stamped with this node's id and each input field name.
- `node.Out` — a value of type `O`, stamped likewise for the outputs.

```go
eligibility.In.Age          // Handle[BlNumber] — this node's "age" input
eligibility.Out.Eligibility // Handle[BlString] — this node's "eligibility" output
```

These surfaces hold no values — they are the handle templates the netlist connects (see [decision-task.spec.md § Wiring](decision-task.spec.md#wiring)). The actual values flow through fresh `I`/`O` structs each time `Evaluate` runs.

---

## Evaluation, typed and erased

`Evaluate(in I) (O, error)` is the **typed** entry point: pass a fully-typed input struct, get a fully-typed output struct. It is what you call to unit-test a node standalone, and the level at which the Go compiler checks a caller.

Inside a `DecisionTask`, nodes have different `I`/`O` types, so they cannot share one Go slice as `DecisionNode[I, O]`. The task therefore stores them through a small **non-generic** view — the node set it derives from the graph edges — and drives each through an **internal run-thunk** that reads the node's input handles from the task's shared value environment, calls the typed `Evaluate`, and writes the output handles back. The erasure is entirely internal: every contract a user touches — the structs, the handles, the edges — stays typed. This mirrors how a generic [`ReferenceData[T]`](reference-data.spec.md) is held through the non-generic `ReferenceValue` view.

---

## A DecisionTask is a node

A `DecisionTask[TaskIn, TaskOut]` has a typed `Evaluate(in TaskIn) (TaskOut, error)` and `In`/`Out` port surfaces, so it **satisfies `DecisionNode[TaskIn, TaskOut]`**. A complete decision therefore composes into a larger one exactly like any other node — you wire its `child.In.*` / `child.Out.*` handles in the parent's graph. There is no dedicated sub-task wrapper; the old name-remapping a wrapper would provide is simply *which handles you connect*. See [decision-task.spec.md § A DecisionTask is a node](decision-task.spec.md#a-decisiontask-is-a-node).

---

## Where type-safety happens

Decision type-safety now lands in three places, and the mental model is one sentence: *if it compiles and construction does not complain, the decision is well-formed.*

1. **Go compile time.** Two things move all the way to `go build`:
   - a caller passing the wrong input struct to `Evaluate`, or reading an output field that does not exist, fails to compile;
   - the **netlist** — every `bl.Edge(src, dst)` connects two `Handle[T]`, so a connection whose endpoint types differ, or that names a non-existent field, is a compile error (see [decision-task.spec.md § Wiring](decision-task.spec.md#wiring)).
2. **Construction.** Each node constructor (`NewDecisionTable`, `NewDecisionExpression`, `NewDecisionNativeFunction`) checks what lives outside the Go type system: the reflection contract above, plus any kind-specific structure — for the expression-bearing kinds, **compiling every expression string** and checking that each name it references is a declared variable. `task.Graph(...)` then checks the whole graph: every node input is wired exactly once, types line up (already guaranteed by `bl.Edge`, re-asserted), node ids are unique, and the edge graph is **acyclic**. Every problem is accumulated and the constructor **panics once** with a `*DecisionDefinitionError` — load-time fail-fast at package init / `go test`.
3. **Evaluation.** What is **not** checked earlier is whether an expression's *computed* value matches its declared output type (e.g. an output declared `Handle[BlString]` whose expression evaluates to a number). The blkit expression engine is runtime-typed — operators dispatch on operand types at evaluation (see [bl-expr.spec.md](../expressions/bl-expr.spec.md)) — so a value-versus-declaration mismatch surfaces as a `bl.TypeError` at evaluation.

> **The honest caveat.** Wiring *between* nodes is compile-checked, but the **expression strings inside** a table or expression node (`if eligibility = "eligible" …`) still resolve their variable names against the node's contract **at construction**, not at `go build` — a string is not a Go identifier. Construction is where that name discipline is enforced; it is deterministic and fires before any run.

---

## Identity

- `Id` — unique identifier within the containing `DecisionTask`. Duplicate ids are rejected at task construction, because a handle is stamped with its node id and two nodes sharing an id would make handles ambiguous.
- `Name` — optional human-readable label.
- `Description` — optional documentation text.

These live on each concrete node and are exposed through `GetId` / `GetName` / `GetDescription`.

---

## Edge Cases

- A node whose `Id` is empty is invalid; `task.Graph(...)` rejects it with `DecisionDefinitionError`.
- A node whose output struct `O` has no fields is invalid (a node must declare at least one output).
- An `I` or `O` field that is not a `bl.Handle[T]` (a bare value, or a non-`BlValue` `T`) is a `DecisionDefinitionError` at node construction.
- An unexported `I`/`O` field is a `DecisionDefinitionError` — every field must be an exported handle variable.
- A duplicate variable name within one struct is a `DecisionDefinitionError`. Names are **not** required to be unique across nodes — wiring is by handle, not by name.
- A variable name (field name, or `expr` tag) that is not a valid expr identifier is a `DecisionDefinitionError`.
- Two nodes sharing an `Id` collide; `task.Graph(...)` rejects it.
- A `bl.Edge` connecting two handles of different `T`, or naming a field that does not exist, is a **Go compile error** — not a construction error.
- A value produced by a node whose runtime type disagrees with its declared output handle surfaces as a `bl.TypeError` at evaluation time.
