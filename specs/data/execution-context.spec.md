---
name: ExecutionContext
description: The shared variable store for a process execution — an append-only log of transactions, where each transaction is a node's atomic batch of variable assignments, and reads are projections that fold the log into current state
targets:
  - ../data/context.go
---

# ExecutionContext

An `ExecutionContext` is an **append-only log of `Transaction`s** maintained throughout a process execution. Each transaction is a node's atomic batch of variable assignments. Reads are projections — methods that fold the log into the current state of the context. A single `ExecutionContext` is shared across the whole process tree (a sub-process operates on a [scoped view](#sub-process-scoping) of the same log).

```go
type TransactionStatus int

const (
    StatusPending   TransactionStatus = iota // appended, not yet visible to other nodes
    StatusCommitted                          // node completed successfully; visible to siblings (visibility may be gated by ancestor scopes)
    StatusAborted                            // node failed; never becomes visible
)

type Transaction struct {
    CommitNumber int               // 1-indexed monotonic sequence; assigned at append time
    NodeID       string            // hierarchical (dotted) node path; e.g. "validate" at top level, "verify-step.check-docs" inside a sub-process
    ExecutionID  string            // groups transactions from the same execution; loopbacks/iterations get distinct ids
    Status       TransactionStatus // Pending, Committed, or Aborted
    Timestamp    time.Time         // when this transaction was appended
    Values       map[string]any    // one or more key → Bl value assignments, applied atomically
}

type ExecutionContext struct { ... }

// The raw transaction log — append-only, chronological, with monotonic commit numbers.
// Includes Pending and Aborted transactions; reads filter by visibility (see Atomic Commit and Visibility).
func (c *ExecutionContext) Transactions() []Transaction { ... }

// Append a transaction to the log as Pending. Assigned the next CommitNumber.
func (c *ExecutionContext) Record(nodeID string, executionID string, values map[string]any) { ... }

// Mark all Pending transactions for nodeID as Committed (called when the node successfully completes).
// For SubProcessTask nodes, this also cascades visibility of internal commits to the parent scope.
func (c *ExecutionContext) Commit(nodeID string) { ... }

// Mark all Pending transactions for nodeID as Aborted (called when the node fails). Aborted transactions
// remain in the log for audit but are never surfaced by reads.
func (c *ExecutionContext) Abort(nodeID string) { ... }

// Highest commit number in the log; 0 when the log is empty
func (c *ExecutionContext) CurrentCommit() int { ... }

// Projections that fold the log into current state. Reads honor visibility — Pending and Aborted
// transactions are excluded, and committed transactions in unfinished sub-process scopes are excluded
// from readers outside those scopes.
func (c *ExecutionContext) Get(path ...string) any               // no args: full nested BlDictionary mirroring the NodeID hierarchy. One arg ("node_id.key" or deeper): drill in.
func (c *ExecutionContext) Latest() []Transaction                // one entry per NodeID — its latest committed transaction visible to this scope

// Read-only view filtered to transactions with CommitNumber ≤ commit. All reads on the view are time-travelled.
func (c *ExecutionContext) AsOf(commit int) *ExecutionContext { ... }

// Sub-process scoping — restrict read/write access to NodeIDs nested under prefix.
func (c *ExecutionContext) Scope(prefix string) *ExecutionContext { ... }

// View used by the worker currently executing nodeID. Reads include the executor's own Pending
// transactions for nodeID (so loop iteration N can read iteration N−1's value before the node commits).
// Other readers do not see those Pending writes.
func (c *ExecutionContext) AsExecutor(nodeID string) *ExecutionContext { ... }

// Inspection
func (c *ExecutionContext) String() string { ... }
func (c *ExecutionContext) ToMarkdown() string { ... }  // structured markdown report of metadata + the full transaction log including Pending/Aborted

// Process metadata (read-only)
func (c *ExecutionContext) ProcessID() string { ... }
func (c *ExecutionContext) ProcessInstanceID() string { ... }      // unique id for this process instance (sub-process scopes report their own)
func (c *ExecutionContext) ParentProcessInstanceID() *string { ... } // nil at the root; set on sub-process scopes
```

---

## Structure

The context is an append-only chronological list of `Transaction` entries. Each transaction represents one node committing its outputs (or, for the start node, the initial submission input).

- **`commit_number`** — a 1-indexed monotonic sequence number assigned when the transaction is appended. The first transaction is `1`; each subsequent transaction is `prior + 1`. Used as a stable handle for time-travel reads via `AsOf(n)`.
- **`node_id`** — the id of the node that produced the transaction. For sub-process nodes, this is a dotted path (`parent-task-id.child-node-id`).
- **`execution_id`** — a unique hash identifier for the node execution that produced this transaction (e.g. `"b7e2a4f1"`). When a node executes multiple times (loopbacks, loops, multi-instance), each execution has a distinct `execution_id`.
- **`timestamp`** — when this transaction was committed (typically the moment the node's execution completed).
- **`values`** — one or more key → `Bl` value assignments applied atomically. A transaction may carry a single value or many; all assignments in one transaction share the `commit_number`, `node_id`, `execution_id`, and `timestamp`.

The first transaction (commit 1) is always the start node's submission — keyed under the `StartId` passed to `store.NewExecutionState(...)` — carrying the initial input variables.

### Example

For a process `start → validate → review → publish → end`:

```
#1 [start] (a3f8c1d2, 2026-04-03T10:00:00Z)
  applicant_name = "Alice"
  loan_amount = 250000

#2 [validate] (b7e2a4f1, 2026-04-03T10:00:01Z)
  is_valid = true
  validation_notes = "All fields present"

#3 [review] (c9d0b3e5, 2026-04-03T10:00:05Z)
  review_status = "approved"
  reviewer = "Bob"

#4 [publish] (d1f4c8a6, 2026-04-03T10:00:06Z)
  published_at = date("2026-04-03")
```

Four transactions, in commit order. Each transaction's `commit_number` (rendered as `#N`) is its position in the log. Each transaction's `node_id` matches the node that produced it; the `Values` map holds that node's outputs. The start node's transaction is no different in shape from any other — it just happens to be first and produced by `store.NewExecutionState(...)` rather than by task execution.

After the example above, `ctx.CurrentCommit()` returns `4`.

---

## Variable Access

Reads are **projections** of the log. `Get` is the primary projection — it folds the log into the current state, returning either the whole state or a value at a specific path.

```go
// Whole current state — a nested bl.BlDictionary mirroring the NodeID hierarchy
state := ctx.Get()
//   bl.BlDictionary{
//     "start":    bl.BlDictionary{"applicant_name": "Alice", "loan_amount": 250000},
//     "validate": bl.BlDictionary{"is_valid": true, ...},
//     "review":   bl.BlDictionary{"review_status": "approved", ...},
//   }

// Drill in by dotted path
name := ctx.Get("start.applicant_name")            // "Alice"
status := ctx.Get("review.review_status")          // "approved"
```

### Whole-state projection (no args)

`Get()` (no args) returns the latest state as a single `bl.BlDictionary` whose structure mirrors the NodeID hierarchy:

- Each top-level key is a NodeID segment; dotted NodeIDs become nested `bl.BlDictionary`s along their dotted path.
- Each leaf node's value is its latest `Transaction.Values`, surfaced as a `bl.BlDictionary`.
- Sub-process variables appear nested under the SubProcessTask's NodeID — `verify-step.check-docs`'s values are reachable as `state["verify-step"]["check-docs"]`.

### Drill-in projection (one arg)

`Get(path)` splits `path` on `.` and tries every (node_id, remainder) split, **longest node_id first**. For each candidate, if a transaction with that `NodeID` exists, it resolves the remainder against the latest such transaction's `Values` map (drilling into nested `bl.BlDictionary` values for further segments). The first split that resolves wins. If nothing matches, `Get` returns `nil` / `None`.

### `Latest()` for provenance

`Get()` projects values without provenance. When the caller needs the originating `CommitNumber`, `ExecutionID`, and `Timestamp` per node (for audit, debugging, or computing diffs), `Latest()` returns one `Transaction` per `NodeID` — the most recent commit, with its full `Values` map and metadata. Iterate `Transactions()` for the full chronological log including superseded commits.

### Expressions

In blkit expressions (gateway conditions, input/output mappings), variables are referenced using dot notation. The leading segment(s) name a node; the remainder names a key (and optionally drills into a nested value):

```go
// Gateway condition referencing a node's output
bl.StringVar("review.review_status").Equals(bl.String("approved"))

// Input mapping pulling from a previous node
bl.BooleanVar("validate.is_valid")
```

---

## Time Travel

Every transaction carries a 1-indexed `CommitNumber` assigned at append time. `CurrentCommit()` returns the highest committed number; `AsOf(commit)` returns a **read-only view** of the context filtered to transactions with `CommitNumber ≤ commit`. All read methods (`Get`, `Latest`, `Transactions`) project from that prefix of the log.

```go
// Snapshot of state right after `validate` ran but before `review` committed
historical := ctx.AsOf(2)
historical.Get("validate.is_valid")            // true
historical.Get("review.review_status")         // nil — review hadn't committed yet
historical.CurrentCommit()                     // 2 — within the view, the head is the cutoff
historical.Transactions()                      // commits 1..2

// Diff helper: latest transactions per node up to two different cutoffs
before := ctx.AsOf(prevCommit).Latest()
after  := ctx.Latest()
```

`AsOf` composes with `Scope`:

```go
ctx.AsOf(42).Scope("verify-step").Get("check-docs.docs_valid")
```

The view is read-only. `Record()` on an `AsOf` view produces an error — appending into a historical prefix is meaningless because the next commit number is already taken. Callers wanting to extend the log do so on the live context.

Why a view instead of `Get(path, commit)`? In Go, mixing a variadic string `path` with an optional `int commit` doesn't compose cleanly, and the view applies time-travel semantics uniformly across all reads — `Transactions()` and `Latest()` time-travel for free.

---

## Atomic Commit and Visibility

A node's writes are an **atomic transaction**: the node may call `Record()` zero or more times during execution, but its outputs only become visible to other nodes when the node successfully completes. On failure, the writes are aborted — never visible. Sub-process tasks act as nested transactions: their internal nodes' commits are visible *within* the sub-process scope as they happen, but invisible to readers outside the sub-process until the SubProcessTask itself commits.

### Lifecycle

The runtime drives the lifecycle — the worker calls these as part of its evaluate–execute cycle; see [process.spec.md](../processes/process.spec.md#evaluation):

1. Worker dispatches node `N` and begins execution.
2. During execution, `N` may call `ctx.Record(N, execID, values)` one or more times. Each call appends a Pending transaction.
3. On success: the worker calls `ctx.Commit(N)` — all `N`'s Pending transactions transition to Committed.
4. On failure: the worker calls `ctx.Abort(N)` — all `N`'s Pending transactions transition to Aborted.

`Aborted` transactions remain in the log for audit but are filtered out of all read projections. The append-only invariant is preserved — nothing is ever removed.

### Loops, multi-instance, and atomic commit

Each loop iteration / multi-instance instance appends its own Pending transaction. **All** of a node's pending transactions stay Pending until the node fully completes (NODE_COMPLETED — i.e. the final iteration succeeds). They then transition to Committed atomically. If any iteration fails terminally (node aborts), every Pending transaction for that node aborts together — earlier iterations are not partially exposed.

This means: if `assess` runs three loop iterations and the third fails, none of the three iterations' values become visible to subsequent nodes.

### Self-reads during execution

The active worker for node `N` needs to read its own in-progress writes — for example, a loop iteration `N+1`'s body / condition typically references `bl.StringVar("assess.risk_score")` to see iteration `N`'s output. The worker uses `ctx.AsExecutor(N)`, a view whose reads include `N`'s Pending transactions in addition to all normally-visible ones. Other readers (gateways evaluating in parallel, sibling nodes) use the regular `ctx`, where `N`'s pending writes remain invisible.

### Visibility cascade for sub-processes

A reader at scope `S` sees a transaction `T` (with NodeID `N`) iff:

1. `N` is under `S` (i.e. `N == S`, or `S == ""`, or `N` starts with `S + "."`).
2. `T.Status == Committed`.
3. **Every NodeID prefix of `N` between `S` and `N`** has been Committed. Concretely: take each dot-bounded prefix of `N` whose length is greater than `S`'s; each such prefix must have been the target of a successful `Commit(prefix)` call.

This is what makes sub-process atomicity work:

- Inside `verify-step` (reader scope `S = "verify-step"`): `T.NodeID = "verify-step.check-docs"` is visible as soon as `Commit("verify-step.check-docs")` happens. The outer `Commit("verify-step")` is not required because the reader is already inside that scope.
- At the root (reader scope `S = ""`): the same `T` is *not* visible until both `Commit("verify-step.check-docs")` *and* `Commit("verify-step")` have happened. The first makes it visible inside the sub-process; the second cascades visibility to the outside.

If a SubProcessTask aborts, no `Commit("verify-step")` is ever called; even if internal nodes individually committed, none of their transactions become visible to the parent scope. (They remain visible inside the sub-process scope's view, which is useful for inspection of failed sub-process runs.)

`AsOf(commit)` time-travel composes with visibility — only commits that happened at or before the cutoff are considered. A SubProcessTask whose `Commit` happened *after* the cutoff is treated as un-cascaded at that point in time.

---

## Durability and the StateStore

The canonical way to obtain an `ExecutionContext` is via `StateStore.NewExecutionState(...)` (fresh) or `StateStore.LoadExecutionState(...)` (resume). Both factories return a context wired to the store's writer channel; the durability behaviour described below depends on that wiring. Direct construction (e.g. for unit tests with no backing store) yields an un-wired context whose mutations are in-memory only.

The in-memory log described above is the **live** copy used by the executing worker. In addition, every mutation on a wired context is mirrored to the configured `StateStore` (see [state-store.spec.md](state-store.spec.md)) as a durable event:

- `Record(nodeID, executionID, values)` enqueues an `OpRecordTransaction` write op carrying the freshly-appended `Transaction` (with its assigned `CommitNumber` and `Timestamp`).
- `Commit(nodeID)` enqueues an `OpUpdateStatus` write op identifying the affected `CommitNumber`s and the new status (`Committed`).
- `Abort(nodeID)` enqueues a symmetric `OpUpdateStatus` write op with status `Aborted`.

The enqueue is non-blocking from the caller's perspective — the runtime's writer pool (see [worker.spec.md](../worker/worker.spec.md#writer-pool)) drains the queue in the background and calls `StateStore.WriteBatch`. Hot-path reads (`Get`, `Latest`, `Transactions`, `AsOf`, `Scope`, `AsExecutor`) operate exclusively on the live in-memory log and never touch the backend.

When a worker picks up a process instance, the in-memory `ExecutionContext` is reconstructed by calling `StateStore.LatestContext(processInstanceID)`. Persistent backends sort the durable events by `(Timestamp, CommitNumber)` before folding them into the rebuilt log — so arrival order at the backend does not need to be preserved. Visibility cascade and time-travel semantics described above are unchanged because they continue to operate on the live in-memory log; they simply see a deterministically-replayed view of what happened.

`Flush(processInstanceID)` is a durability barrier — workers call it on root-scope `Commit` / `Abort` and at evaluation boundaries to guarantee that prior writes have landed before returning control.

---

## Loopbacks

When a loopback causes a node to execute multiple times, each execution appends a **new transaction** with a distinct `execution_id` and later `timestamp`. The log preserves every commit — earlier transactions are never overwritten. Reads project the latest commit per `(node_id, key)`, so subsequent nodes naturally see the newest value.

```
#3 [review] (c9d0b3e5, 2026-04-03T10:00:05Z)
  review_status = "needs_revision"

#4 [revise] (e2a7d9c4, 2026-04-03T10:00:10Z)
  content = "revised draft v2"

#5 [review] (f5b1e8a3, 2026-04-03T10:00:15Z)              # second execution of review
  review_status = "approved"
```

After the second execution of `review`:

```go
// Get() returns the value from the latest matching transaction
ctx.Get("review.review_status")            // "approved"

// Both review transactions remain in the log; iterate Transactions() to see them.
// For richer step-by-step history (NODE_SCHEDULED / NODE_STARTED / etc.), see ExecutionHistory.
```

---

## Sub-process scoping

There is **one** `ExecutionContext` per top-level submission. A `SubProcessTask` does **not** maintain its own context that gets copied at completion — it operates on a **scoped view** of the same shared log.

When the engine starts a `SubProcessTask` whose NodeID is `verify-step`, it hands the sub-process worker `parentCtx.Scope("verify-step")`. Inside that scope:

- `scope.Record("check-docs", execID, {docs_valid: true})` writes a transaction with `NodeID="verify-step.check-docs"` to the shared log.
- `scope.Get("check-docs.docs_valid")` resolves to `parentCtx.Get("verify-step.check-docs.docs_valid")`.
- `scope.Transactions()` returns only transactions whose `NodeID` is `verify-step` or has the prefix `verify-step.`, with the prefix stripped — the sub-process cannot read parent-only variables (e.g. `validate.is_valid`).
- `scope.ProcessInstanceID()` returns the sub-process's own instance id; `scope.ParentProcessInstanceID()` returns the parent's. Metadata travels with the scope; the underlying log is shared.

NodeIDs are hierarchical: nesting is conveyed entirely by the dotted `node_id` path stored on each transaction.

### Example

For a parent process `start → verify-step → end`, where `verify-step` is a `SubProcessTask` running `start → check-docs → check-identity → end`:

```
#1 [start] (..., 2026-04-03T10:00:00Z)
  applicant_name = "Alice"

#2 [verify-step.start] (..., 2026-04-03T10:00:00Z)
  document_url = "https://..."

#3 [verify-step.check-docs] (..., 2026-04-03T10:00:02Z)
  docs_valid = true

#4 [verify-step.check-identity] (..., 2026-04-03T10:00:05Z)
  identity_confirmed = true
  confidence = 0.95
```

From any scope, the dotted-path access works:

```go
ctx.Get("verify-step.start.document_url")                  // "https://..."
ctx.Get("verify-step.check-docs.docs_valid")               // true
ctx.Get("verify-step.check-identity.confidence")           // 0.95
```

From inside the sub-process's scope (`scope := ctx.Scope("verify-step")`):

```go
scope.Get("start.document_url")                            // "https://..."
scope.Get("check-docs.docs_valid")                         // true
scope.Get("validate.is_valid")                             // nil — outside the scope
```

### Nested sub-processes

When a sub-process itself runs another sub-process, scopes compose. For a chain: parent → `kyc-step` (SubProcessTask) → `sanctions-check` (SubProcessTask), where `sanctions-check` runs `start → screen → end`:

```
#1 [start] (..., 2026-04-03T10:00:00Z)
  applicant_name = "Alice"

#2 [kyc-step.start] (..., 2026-04-03T10:00:00Z)
  applicant_name = "Alice"

#3 [kyc-step.verify-identity] (..., 2026-04-03T10:00:02Z)
  identity_confirmed = true

#4 [kyc-step.sanctions-check.start] (..., 2026-04-03T10:00:03Z)
  applicant_name = "Alice"

#5 [kyc-step.sanctions-check.screen] (..., 2026-04-03T10:00:05Z)
  sanctions_clear = true
  match_count = 0
```

```go
ctx.Get("kyc-step.sanctions-check.screen.sanctions_clear")        // true
ctx.Get("kyc-step.sanctions-check.screen.match_count")            // 0

// Inside sanctions-check's worker scope:
inner := ctx.Scope("kyc-step").Scope("sanctions-check")
// equivalent to ctx.Scope("kyc-step.sanctions-check")
inner.Get("screen.sanctions_clear")                               // true
```

The dotted `node_id` path mirrors the process nesting — arbitrarily deep. `Get` resolves correctly because it tries longest-node-id splits first.

---

## Loop Context

When a node has a `LoopConfig`, each iteration appends its own `Transaction` with a distinct `execution_id` and later `timestamp`. All of them stay `Pending` until the node fully completes (the loop terminates), at which point they all transition to `Committed` together — sibling nodes never see partial loop progress. While the loop is running, the node's own loop body / condition still reads its prior iterations via `ctx.AsExecutor(nodeID)` (see [Atomic Commit and Visibility](#atomic-commit-and-visibility)). After commit, `Get()` returns the value from the latest iteration's transaction — the same behaviour as loopbacks.

### Example

A `risk-check` task with `LoopConfig(condition=..., max_iterations=3)` that runs twice:

```
#3 [risk-check] (a1b2c3d4, 2026-04-03T10:00:01Z)
  risk_score = "indeterminate"

#4 [risk-check] (e5f6a7b8, 2026-04-03T10:00:02Z)
  risk_score = "low"
```

```go
ctx.Get("risk-check.risk_score")              // "low" (latest iteration)
```

Both iterations remain in `Transactions()` for audit purposes.

---

## Multi-Instance Context

When a node has a `MultiInstanceConfig`, each instance appends its own `Transaction` with a distinct `execution_id` and later `timestamp`. The output is a `bl.BlList` that accumulates — each instance's transaction contains the results from all instances completed so far, in collection order. As with loops, every instance's transaction stays `Pending` until the node fully completes (all instances finish); they then all transition to `Committed` atomically. If any instance fails terminally, the node aborts and *every* instance transaction aborts together. `Get()` (after commit) returns the value from the latest transaction (the fully accumulated list).

### Example

A `risk-check` task with `MultiInstanceConfig(collection=..., element_variable="applicant")` running over three applicants:

```
#3 [risk-check] (m1n2o3p4, 2026-04-03T10:00:01Z)
  result = [
    {risk_score: "low", applicant_id: "A1"},
  ]

#4 [risk-check] (q5r6s7t8, 2026-04-03T10:00:02Z)
  result = [
    {risk_score: "low", applicant_id: "A1"},
    {risk_score: "high", applicant_id: "A2"},
  ]

#5 [risk-check] (u9v0w1x2, 2026-04-03T10:00:03Z)
  result = [
    {risk_score: "low", applicant_id: "A1"},
    {risk_score: "high", applicant_id: "A2"},
    {risk_score: "medium", applicant_id: "A3"},
  ]
```

```go
ctx.Get("risk-check.result")                  // bl.BlList of 3 BlDictionaries (latest accumulated)
```

All iteration transactions are preserved in `Transactions()` for audit purposes.

---

## Variable Types

All values stored in transaction `Values` are `Bl` values:

- Primitives: `bl.BlString`, `bl.BlNumber`, `bl.BlBoolean`, `bl.BlNull`
- Dates and times: `bl.BlDate`, `bl.BlTime`, `bl.BlDateTime`, `BlDuration`
- Collections: `bl.BlList`, `bl.BlDictionary`, `bl.BlTable` (a list of uniformly-keyed `bl.BlDictionary` rows; see [table.spec.md](../expressions/table.spec.md))
- Ranges: `bl.BlRange`

---

## Inspection

Two inspection methods produce human-readable representations of the log. Both render every transaction in commit order, including `Pending` and `Aborted` entries (so failed runs stay debuggable). Status is shown so it's clear what's visible vs not.

### `String()`

A compact plain-text form for log lines and CLI output:

```go
fmt.Println(ctx.String())
```

Output:

```
#1 [start] (a3f8c1d2, 2026-04-03T10:00:00Z)
  applicant_name: bl.BlString = "Alice"
  loan_amount: bl.BlNumber = 250000

#2 [validate] (b7e2a4f1, 2026-04-03T10:00:01Z)
  is_valid: bl.BlBoolean = true
  validation_notes: bl.BlString = "All fields present"

#3 [review] (c9d0b3e5, 2026-04-03T10:00:05Z)
  review_status: bl.BlString = "needs_revision"

#4 [revise] (e2a7d9c4, 2026-04-03T10:00:10Z)
  content: bl.BlString = "revised draft v2"

#5 [review] (f5b1e8a3, 2026-04-03T10:00:15Z)
  review_status: bl.BlString = "approved"
```

`Committed` transactions render without a status marker. `Pending` and `Aborted` transactions append `(pending)` or `(aborted)` after the timestamp — for example, a failed review attempt mid-run might appear as:

```
#3 [review] (c9d0b3e5, 2026-04-03T10:00:05Z) (aborted)
  review_status: bl.BlString = "error"
```

Loopback iterations appear as multiple transactions for the same `node_id`. For nested values (`bl.BlDictionary`, `bl.BlList`), the output uses indented `Bl` literal syntax:

```
#1 [start] (a3f8c1d2, 2026-04-03T10:00:00Z)
  applicant: bl.BlDictionary = {
    name: bl.BlString = "Alice",
    age: bl.BlNumber = 34,
    address: bl.BlDictionary = {
      city: bl.BlString = "Melbourne",
      country: bl.BlString = "AU"
    }
  }
  scores: bl.BlList = [720, 680, 750]
```

### `ToMarkdown()`

A structured markdown report of the **current visible context** — the committed values for each node, hierarchically organized by `NodeID` (so sub-process nodes nest under their parent task), with each node's values shown as a bullet list. Useful for embedding in PR descriptions, debugging UIs, or CLI tools that render markdown.

```go
fmt.Println(ctx.ToMarkdown())
```

#### Shape

- A top-level `# ExecutionContext` heading.
- A metadata bullet list: `process_id`, `process_instance_id`, `parent_process_instance_id` (if set), and `current_commit`.
- One section per node that has at least one visible transaction. The heading level reflects the node's depth in the NodeID hierarchy (`##` for top-level nodes, `###` for one level deep, etc.).
- Under each section, the node's latest committed `Values` rendered as a bullet list of `key: Bl-literal` lines. Nested `bl.BlDictionary` / `bl.BlList` values render as indented `Bl` literals across multiple lines.
- A node with no values of its own (e.g. a `SubProcessTask` that just orchestrates children) appears as a heading followed directly by its child sections.

#### Ordering

Sections are listed **in the order each node's execution began** — using the minimum `CommitNumber` among the node's own transactions and the transactions of any node nested under it as the ordering key. This means a sub-process appears at the position of its first internal commit, with its children listed within in the same order.

Pending and Aborted transactions are excluded — `ToMarkdown()` shows the committed projected state. For an audit-style view of every transaction (including failed ones), use `String()`. For a snapshot at an earlier commit, use `ctx.AsOf(n).ToMarkdown()`.

#### Example — linear process

For the linear process from [Example](#example) (`start → validate → review → publish`):

```
# ExecutionContext

- process_id: simple-review
- process_instance_id: pi-001
- current_commit: 4

## start

- applicant_name: "Alice"
- loan_amount: 250000

## validate

- is_valid: true
- validation_notes: "All fields present"

## review

- review_status: "approved"
- reviewer: "Bob"

## publish

- published_at: date("2026-04-03")
```

#### Example — sub-process

For the sub-process example from [Sub-process scoping](#sub-process-scoping):

```
# ExecutionContext

- process_id: onboarding
- process_instance_id: pi-005
- current_commit: 4

## start

- applicant_name: "Alice"

## verify-step

### start

- document_url: "https://..."

### check-docs

- docs_valid: true

### check-identity

- identity_confirmed: true
- confidence: 0.95
```

The `verify-step` heading has no values directly under it because the SubProcessTask itself didn't `Record` — its children (`start`, `check-docs`, `check-identity`) carry the data. Their order within `verify-step` reflects the order in which each first committed.

#### Example — nested values

For a node whose values include nested `bl.BlDictionary` / `bl.BlList`:

```
## start

- applicant: {
    name: "Alice",
    age: 34,
    address: {
      city: "Melbourne",
      country: "AU"
    }
  }
- scores: [720, 680, 750]
```

Indentation follows the `Bl` literal syntax used elsewhere in the spec.

#### Example — bl.BlTable values

When a value is a [`bl.BlTable`](../expressions/table.spec.md), it renders as a proper markdown table beneath its key, using `bl.BlTable.ToMarkdown()`. Columns are aligned for plain-text readability.

```
## quote

- line_items:

  | product   | quantity | unit_price |
  |-----------|----------|------------|
  | "Widget"  | 2        | 9.99       |
  | "Gadget"  | 1        | 19.99      |
  | "Sprocket"| 5        | 3.50       |

- total: 64.48
```

A scalar value, list, or context still renders inline (or with `Bl`-literal indentation for nested cases). Only `bl.BlTable` values get table rendering — a plain `bl.BlList[bl.BlDictionary]` that hasn't been promoted to a `bl.BlTable` falls back to the `Bl`-literal form.

#### Example — loopbacks and loops

A loopback or loop node only appears **once** in the markdown — under its single heading — with its latest committed values. The full per-iteration history is preserved in `Transactions()` and rendered by `String()`; `ToMarkdown()` is intentionally a current-state view.

---

## Process Metadata

The context exposes read-only metadata. Each scope reports its own metadata; the underlying log is shared.

- `process_id` — the id of the `Process` definition for this scope.
- `process_instance_id` — a unique identifier for this scope's execution run. The root scope's id is generated when the engine begins executing the submission; sub-process scopes have their own.
- `parent_process_instance_id` — `None` for the root scope; set to the parent's `process_instance_id` for sub-process scopes.

---

## Edge Cases

- `get(path)` returns `None` (not an error) if no transaction matches the path. When multiple transactions match the same `(node_id, key)` (loopbacks, loops, multi-instance), `get()` returns the value from the **latest** one — i.e. the matching transaction with the largest `CommitNumber`.
- `record(...)` always appends — it never overwrites an existing transaction, even when called with the same `node_id` and `execution_id` as a prior call. Each append assigns the next `commit_number` (`CurrentCommit() + 1`) and starts in `Status: Pending`.
- `record(...)` with an empty `values` map is valid but useless — it appends a transaction that contributes no readable values. Most callers omit such calls.
- `commit(nodeID)` transitions every Pending transaction whose `NodeID == nodeID` to Committed. Already-Committed or Aborted transactions for that nodeID are unaffected. Calling `commit(nodeID)` when there are no Pending transactions for the node is a no-op (the node successfully completed without writing values).
- `abort(nodeID)` transitions every Pending transaction whose `NodeID == nodeID` to Aborted. Already-Committed or Aborted transactions for that nodeID are unaffected. Aborted transactions remain in `Transactions()` for audit and appear in `String()`/`ToMarkdown()` output, but are excluded from `Get`, `Latest`, and the visibility cascade entirely.
- `commit` / `abort` only act on the writing node's own transactions — they do not affect descendants. A SubProcessTask's `commit` cascades visibility of internal commits to the parent scope but does not change those internal transactions' Status (they're already Committed at the inner scope).
- If a SubProcessTask aborts, its internal Committed transactions remain visible *inside* the sub-process scope's view (useful for inspecting the failed run) but are invisible to readers at any ancestor scope.
- `AsExecutor(nodeID)` returns a view whose reads include `nodeID`'s own Pending transactions in addition to all normally-visible commits. Every other read API (`Transactions`, `Get`, `Latest`) on a non-executor view filters Pending out. The executor view is intended for the active worker only; it is not a substitute for `Commit`.
- A reader at scope `S` cannot see Pending transactions belonging to nodes outside its own active execution — even if those nodes are *its* descendants. Visibility requires a successful `Commit`.
- `CurrentCommit()` returns `0` when the log is empty. The first transaction is commit `1`.
- `commit_number` is dense and gap-free (1, 2, 3, …). Commit numbers are scoped to the shared log — sub-process scopes (`Scope(prefix)`) see the same global numbering, not a re-numbering within their subtree.
- `AsOf(commit)` with `commit` ≥ `CurrentCommit()` returns a view equivalent to the live context (read-only). With `commit ≤ 0`, it returns a view over an empty log.
- `AsOf(commit)` returns a read-only view: calling `Record()` on it produces a `ContextReadOnlyError`. Callers that need to extend the log do so on the live context.
- `AsOf` and `Scope` compose in either order — `ctx.AsOf(n).Scope(p)` and `ctx.Scope(p).AsOf(n)` produce equivalent views.
- `Get()` (no args) returns an empty `bl.BlDictionary` when the log is empty. `Latest()` returns an empty list.
- `Get()` (no args) and `Get(path)` always project from the latest transaction per NodeID. Superseded transactions (loopbacks, loop iterations, multi-instance iterations) are not surfaced — use `Transactions()` to see them.
- A node whose latest transaction's `Values` is empty appears in `Get()`'s nested context as an empty `bl.BlDictionary` under its NodeID path.
- If two NodeIDs collide on the projection path — e.g. a node `verify-step` records its own values *and* a sub-node `verify-step.check-docs` exists — the parent NodeID's keys and the child NodeID's nested context share the same `bl.BlDictionary` level. A key collision (the parent's `Values` contains `"check-docs"` *and* a child node has NodeID `verify-step.check-docs`) is invalid; `Get()` resolves the nested NodeID path and the parent's colliding key is masked. In practice, `SubProcessTask` nodes do not Record their own values, so this collision does not arise.
- `to_string()` on an empty context returns an empty string.
- `process_instance_id` is unique per scope. Each call to `store.NewExecutionState(...)` produces a distinct `process_instance_id` for the root scope. Each sub-process scope also gets its own unique `process_instance_id`.
- `parent_process_instance_id` is `None` at the root; for a sub-process scope, it is the `process_instance_id` of the scope that contains the `SubProcessTask` which created it.
- The start node's NodeID is the `StartId` passed to `store.NewExecutionState(...)` (by convention, `"start"` for processes with a single start node).
- `Scope(prefix)` returns a view backed by the same underlying log. Writes inside the scope are visible to ancestor scopes (with the scope prefix prepended to NodeIDs); reads inside the scope are restricted to NodeIDs equal to `prefix` or beginning with `prefix + "."`.
- `Scope(prefix)` chains: `ctx.Scope("a").Scope("b")` is equivalent to `ctx.Scope("a.b")`. Each level in the chain reports its own metadata.
- `get()` with a multi-segment path tries every `(node_id, remainder)` split, longest node_id first. The first split for which a transaction's `NodeID` matches and the remainder resolves against its `Values` map wins. This handles both nested sub-process NodeIDs and drilling into `bl.BlDictionary` values.
- Loopback, loop, and multi-instance iterations each produce their own `Transaction` with a distinct `execution_id` and timestamp. `Get()` returns the value from the latest one. All transactions remain in the log.
- Keys within a transaction's `Values` map should be simple names (no dots). Dotted nesting is conveyed by the `NodeID`, not by embedding dots in keys; reserving dots for `NodeID` boundaries keeps `Get` resolution unambiguous.
