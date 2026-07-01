---
name: Gateway Nodes
description: Gateway types (ParallelGateway, ExclusiveGateway, InclusiveGateway, JoinGateway) and their constructors (And, Xor, Or, Join). Conditional gateways (Xor, Or) route on typed branch conditions compiled against a Go env struct via bl.Expr[E]; each branch carries its own target node.
targets:
  - ../processes/gateway_nodes.go
---

# Gateway Nodes

Gateways control how tokens fan out, fan in, and route through a process graph. All gateway types implement `ProcessNode`, so they appear in the graph alongside event nodes and task nodes and participate in the same `.To()` chaining mechanism described in [process.spec.md](process.spec.md).

```go
// Gateway constructors — all return ProcessNodes
func And(targets ...ProcessNode) *ParallelGateway
func Xor[E any](branches ...*Branch) *ExclusiveGateway
func Or[E any](branches ...*Branch) *InclusiveGateway
func Join(sources ...ProcessNode) *JoinGateway
```

Each gateway constructor creates a gateway node with its outgoing or incoming edges already wired and returns the gateway. Gateway ids are auto-generated and unique within the process.

`Xor` and `Or` are conditional: they route on branch conditions. Each condition is a blkit expression compiled against a typed Go env struct `E` (the type parameter), and each `Branch` carries the target node it routes to. `And` and `Join` carry no conditions and are unaffected.

## Conditions read data; edges route tokens

A gateway sits at a position in the **control-flow** graph: a token arrives via `.To()`, and the gateway decides where the token goes next. A branch condition only **reads data** — the durable values earlier nodes wrote to the [`ExecutionContext`](../data/execution-context.spec.md). A condition is therefore *not* a data edge to the nodes it reads; it is a typed read-projection of the context at the gateway's position. The env struct `E` names exactly which context values the conditions read, and `ctx.Get` resolves each to the value visible at that point (including across merges and loopbacks).

---

## `And` — ParallelGateway

Parallel (AND) gateway. Activates every outgoing branch concurrently when reached.

```go
type ParallelGateway struct {
    ProcessNode
    Id   string  // auto-generated
    Name *string // nil
}
```

```go
verifyIdentity.To(And(pullCreditReport, checkIncome, runAffordability))
```

`And` returns the `ParallelGateway` node. `And` with a single target is valid — it creates a parallel gateway with one outgoing branch (equivalent to a direct edge, but the gateway node still appears in the graph and in `ExecutionHistory`).

---

## `Xor` — ExclusiveGateway

Exclusive (XOR) gateway. Routes to exactly one outgoing branch — the first whose condition evaluates to `true` (in declaration order). If no condition matches, the default branch is taken.

```go
type ExclusiveGateway struct {
    ProcessNode
    Id       string  // auto-generated
    Name     *string // nil
    Branches []*Branch
}
```

```go
type CreditEnv struct {
    Score bl.BlNumber `expr:"score" ctx:"checkCredit.score"`
}

checkCredit.To(Xor[CreditEnv](
    bl.Branch("approve", `score >= 700`, approveTask),
    bl.DefaultBranch("reject", rejectTask),
))
```

At evaluation time, branches are tested in declaration order. The first matching branch executes. If no branch matches and no default branch is declared, evaluation produces a `GatewayEvaluationError`.

`Xor` returns the `ExclusiveGateway` node.

---

## `Or` — InclusiveGateway

Inclusive (OR) gateway. Routes to **every** outgoing branch whose condition evaluates to `true`. If no condition matches, the default branch fires; if no default is declared, evaluation produces a `GatewayEvaluationError`.

```go
type InclusiveGateway struct {
    ProcessNode
    Id       string  // auto-generated
    Name     *string // nil
    Branches []*Branch
}
```

```go
type NotifyEnv struct {
    WantsEmail bl.BlBoolean `expr:"wants_email" ctx:"notificationCheck.wants_email"`
    WantsSms   bl.BlBoolean `expr:"wants_sms"   ctx:"notificationCheck.wants_sms"`
}

notificationCheck.To(Or[NotifyEnv](
    bl.Branch("email", `wants_email`, sendEmail),
    bl.Branch("sms",   `wants_sms`,   sendSms),
    bl.DefaultBranch("log", logOnly),
))
```

`Or` returns the `InclusiveGateway` node.

---

## `Join` — JoinGateway

Join gateway. Waits for every listed source branch to complete before forwarding flow.

```go
type JoinGateway struct {
    ProcessNode
    Id   string  // auto-generated
    Name *string // nil
}
```

```go
Join(pullCreditReport, checkIncome, runAffordability).To(calculateScore)
```

`Join` returns the `JoinGateway` node. The join waits for all listed source branches to complete regardless of which gateway type created the fanout — `Join(a, b)` after `And(a, b)` is the natural pairing, but `Join` also works with sources that are reached via different fanout paths.

`Join` with a single source is valid — it acts as a pass-through.

---

## Branches

A `Branch` binds a name to a condition and the target node the gateway routes to when that condition matches. Passing branches directly to `Xor` / `Or` makes each branch the single source of truth for both its condition and its destination — there is no separate name-to-node map to keep in sync.

```go
// Branch pairs a condition source with the target it routes to. The source is
// compiled against the gateway's env type E by Xor[E] / Or[E].
func Branch(name string, source string, target ProcessNode) *Branch

// DefaultBranch has no condition — it fires when no other branch matches (Xor)
// or as a fallback when no condition is true (Or).
func DefaultBranch(name string, target ProcessNode) *Branch

type Branch struct {
    Name   string
    Source string      // condition source; "" for a default branch
    Target ProcessNode
    // compiled, type-erased condition; populated by Xor[E] / Or[E]
}
```

`bl.Branch` only records the raw `Source`; the enclosing `Xor[E]` / `Or[E]` compiles it once via [`bl.Expr[E]`](../expressions/bl-expr.spec.md) and retains the type-erased compiled condition on the branch. A default branch has no `Source` and is never compiled.

## The env type `E`

`E` declares, as a concrete Go struct, exactly the context values the gateway's conditions read. Each exported field is one variable:

- **The field type is a `BlValue`** (`bl.BlNumber`, `bl.BlString`, `bl.BlBoolean`, …). Its Go type is what the condition source operates on.
- **`ctx:"node.key"` (required)** is the durable [`ExecutionContext`](../data/execution-context.spec.md) cell the field is populated from at evaluation — the qualified key an earlier node wrote (e.g. `checkCredit.score`).
- **`expr:"name"` (optional)** is the variable name the condition source references. When untagged, the Go field name is used — the same rule [`bl.Expr[E]`](../expressions/bl-expr.spec.md) applies.

At evaluation the gateway builds a fresh `E`, reads each field's `ctx:` cell via `ctx.Get`, assigns it into the field, then runs each compiled condition against that value. Because resolution is parameterised over `ctx`, the same gateway works across replays, suspends, and loopbacks — whatever `ctx` is current supplies the values.

> **Why `[E any]` and not a tighter constraint?** Go generics can constrain a type parameter but cannot say "a struct all of whose fields are `BlValue`" — field types are not expressible in the constraint system. So `[E any]` is the tightest signature available, and the field rules (every field a `BlValue`, every field carrying a `ctx:` tag) are enforced by reflection when `Xor[E]` / `Or[E]` runs. Because gateways live inside package-scope process `var`s, that check fails at program (or test-binary) startup — the same load-time fail-fast as every other definition rule here.

---

## Where type-safety happens

| Moment | What is checked | Raised as |
|--------|-----------------|-----------|
| **Go compile time** | `E` names a real Go type; each `Branch` target is a `ProcessNode`; env field Go types are the types the compiled condition operates on. | Go type error |
| **Construction** (`Xor[E]` / `Or[E]`, then `bl.NewProcess`) — accumulated, panics once | every field of `E` is a `BlValue` carrying a `ctx:` tag; each branch `Source` compiles via `bl.Expr[E]` (syntax, and every referenced variable is a declared field of `E`); branch names are non-empty and unique within the gateway; every branch has a target; each `ctx:"node.key"` path resolves to an output produced by some node reachable on an inbound control path. | `ProcessDefinitionError` |
| **Evaluation** | a condition value that is not a `BlBoolean`; no branch matches and no default branch is declared. | `bl.TypeError`; `GatewayEvaluationError` |

> **The honest caveat.** A condition `Source` and a `ctx:"node.key"` path are strings, so their names resolve against `E` and against the upstream graph **at construction**, not at `go build`. Construction is where that discipline is enforced; it is deterministic and fires before any run — the same envelope the [decision layer](../decision-tasks/decision-node.spec.md#where-type-safety-happens) accepts for expression strings.

---

## Edge Cases

- Gateway ids are auto-generated and unique within the process. They appear in `ProcessGraph.Nodes` and in `ExecutionHistory` steps (e.g. `GATEWAY_RESOLVED`).
- `And` with a single target is valid — it creates a parallel gateway with one branch (equivalent to a direct edge, but the gateway node still appears in the graph).
- `Join` with a single source is valid — it acts as a pass-through.
- `Join` with sources that are not all reachable from the same fanout gateway is valid — `Join` simply waits for all listed nodes to complete.
- Two branches on the same gateway sharing a `Name` collide — a `ProcessDefinitionError`. Branch names need only be unique within their gateway; they no longer have to match any external map.
- A branch `Source` that references a variable not declared in `E`, or that fails to compile, is a `ProcessDefinitionError` at construction.
- A field of `E` that is not a `BlValue`, or that lacks a `ctx:` tag, is a `ProcessDefinitionError` at construction.
- A `ctx:"node.key"` path that no upstream node produces on any inbound control path is a `ProcessDefinitionError` at `bl.NewProcess`.
- An `Xor` gateway with no default branch produces a `GatewayEvaluationError` at runtime if no condition matches.
- An `Or` gateway with no default branch produces a `GatewayEvaluationError` at runtime if no condition evaluates to `true`.
- A condition whose value is not a `BlBoolean` produces a `bl.TypeError` at evaluation.
