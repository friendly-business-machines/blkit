---
name: Gateway Nodes
description: Gateway types (ParallelGateway, ExclusiveGateway, InclusiveGateway, JoinGateway) and their fluent constructors (And, Xor, Or, Join), plus GatewayConditions for conditional fanouts
targets:
  - ../processes/gateway_nodes.go
---

# Gateway Nodes

Gateways control how tokens fan out, fan in, and route through a process graph. All gateway types implement `ProcessNode`, so they appear in the graph alongside event nodes and task nodes and participate in the same `.To()` chaining mechanism described in [process.spec.md](process.spec.md).

```go
// Gateway constructors — all return ProcessNodes
func And(targets ...ProcessNode) *ParallelGateway
func Xor(conditions *GatewayConditions, branches map[string]ProcessNode) *ExclusiveGateway
func Or(conditions *GatewayConditions, branches map[string]ProcessNode) *InclusiveGateway
func Join(sources ...ProcessNode) *JoinGateway
```

Each gateway constructor creates a gateway node with its outgoing or incoming edges already wired and returns the gateway. Gateway ids are auto-generated and unique within the process.

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
    Id         string  // auto-generated
    Name       *string // nil
    Conditions *GatewayConditions
}
```

```go
conditions := NewGatewayConditions(
    NewBranch("approve", Bl.NumberVar("checkCredit.score").GreaterThanOrEqual(Bl.Number(700))),
    DefaultBranch("reject"),
)

checkCredit.To(Xor(conditions, map[string]ProcessNode{
    "approve": approveTask,
    "reject":  rejectTask,
}))
```

At evaluation time, branches are tested in declaration order. The first matching branch executes. If no branch matches and no default is declared, evaluation produces a `GatewayEvaluationError`.

`Xor` returns the `ExclusiveGateway` node.

---

## `Or` — InclusiveGateway

Inclusive (OR) gateway. Routes to **every** outgoing branch whose condition evaluates to `true`. If no condition matches, the default branch fires; if no default is declared, evaluation produces a `GatewayEvaluationError`.

```go
type InclusiveGateway struct {
    ProcessNode
    Id         string  // auto-generated
    Name       *string // nil
    Conditions *GatewayConditions
}
```

```go
conditions := NewGatewayConditions(
    NewBranch("email", Bl.BooleanVar("notificationCheck.wants_email")),
    NewBranch("sms",   Bl.BooleanVar("notificationCheck.wants_sms")),
    DefaultBranch("log"),
)

notificationCheck.To(Or(conditions, map[string]ProcessNode{
    "email": sendEmail,
    "sms":   sendSms,
    "log":   logOnly,
}))
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

## GatewayConditions and Branches

`GatewayConditions` defines the routing rules for `Xor` and `Or` gateways. Each `Branch` maps a name to a blkit expression that evaluates to a boolean.

```go
func NewGatewayConditions(branches ...*Branch) *GatewayConditions

func NewBranch(name string, condition BlExpr) *Branch
func DefaultBranch(name string) *Branch
// A default branch has no condition — it fires when no other branch matches (Xor)
// or always fires as a fallback (Or).

type Branch struct {
    Name      string
    Condition BlExpr // nil for default branches
}
```

Branch names must match the keys in the `branches` map passed to `Xor` or `Or`. A mismatch — a name in `GatewayConditions` not found in the `branches` map, or vice versa — is detected during process validation and produces a `ProcessDefinitionError`.

---

## Edge Cases

- Gateway ids are auto-generated and unique within the process. They appear in `ProcessGraph.Nodes` and in `ExecutionHistory` steps (e.g. `GATEWAY_RESOLVED`).
- `And` with a single target is valid — it creates a parallel gateway with one branch (equivalent to a direct edge, but the gateway node still appears in the graph).
- `Join` with a single source is valid — it acts as a pass-through.
- `Join` with sources that are not all reachable from the same fanout gateway is valid — `Join` simply waits for all listed nodes to complete.
- Branch names in `GatewayConditions` must match the keys in the `Xor` / `Or` branches map. A mismatch is a `ProcessDefinitionError`.
- A `GatewayConditions` with no default branch in an `Xor` gateway produces a `GatewayEvaluationError` at runtime if no condition matches.
- A `GatewayConditions` with no default branch in an `Or` gateway produces a `GatewayEvaluationError` at runtime if no condition evaluates to `true`.
