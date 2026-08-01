---
name: ExecutionHistory
description: The structure of execution history for a process instance — a flat chronological record of execution steps with node-grouped rendering for inspection
status: agreed
code:
  - core/execution_history.go
---

# ExecutionHistory

The execution history for a process instance is a flat, chronological list of `ExecutionStep`s — every scheduling decision, task start/completion, gateway resolution, and process lifecycle event. This is the source of truth for what happened during execution.

For display purposes, `to_markdown()` renders a **node-grouped** view by grouping steps by `node_id`, with process-level steps (PROCESS_STARTED, PROCESS_COMPLETED, PROCESS_FAILED) shown outside the node sections. The examples below show this rendered format. Steps where a token currently resides are marked with `◀ token`.

For the `ExecutionContext` variable store, see [execution-context.spec.md](execution-context.spec.md). For the `evaluate()` algorithm, see [process.spec.md](../processes/process.spec.md). For the pluggable state store that persists this history, see [state-store.spec.md](state-store.spec.md).

---

## Structure

```go
type ExecutionHistory struct {
    ProcessID             string
    ProcessVersion        string
    ProcessInstanceID     string
    ProcessInstanceStatus ProcessStatus // RUNNING, SUSPENDED, COMPLETED, or FAILED
    PublishedAt           *time.Time    // when the start-command for this process instance was published to the MessageBroker; nil for runs invoked directly via Process.Evaluate() without a broker
    StartedAt             *time.Time    // when Evaluate() first ran (nil if still queued)
    CompletedAt           *time.Time    // when the process reached a terminal state (COMPLETED or FAILED) and is no longer being retried
    EvaluationCount       int           // how many times Evaluate() has been called for this process instance (does not include task executions or sub-process evaluations)
    Steps                 []ExecutionStep // flat chronological list of all steps
}

func NewExecutionHistory(
    processID string,
    processVersion string,
    processInstanceID string,
    enqueuedAt time.Time,
) *ExecutionHistory { ... }

// Append steps to the history
func (h *ExecutionHistory) Record(steps ...ExecutionStep) { ... }

// Serialization
func (h *ExecutionHistory) ToJSON() string { ... }
func (h *ExecutionHistory) ToTable() string { ... }
func (h *ExecutionHistory) ToMarkdown() string { ... }

// Deserialization
func ExecutionHistoryFromJSON(jsonStr string) (*ExecutionHistory, error) { ... }
func ExecutionHistoryFromTable(tableStr string) (*ExecutionHistory, error) { ... }

// Internal — groups steps by node_id for rendering
func (h *ExecutionHistory) nodeHistories() []NodeHistory { ... }
```

### Methods

- **`record(*steps)`** — appends one or more steps to the `steps` list. For process-level steps, updates metadata: `PROCESS_STARTED` sets `started_at` (if not already set), `PROCESS_COMPLETED` sets `completed_at` and `process_instance_status` to `COMPLETED`, `PROCESS_FAILED` sets `completed_at` and `process_instance_status` to `FAILED`. If any step carries a previously unseen non-None `evaluation_id`, increments `evaluation_count`. Called by `evaluate()` (which passes the batch of steps it produced) and by the worker when recording task execution steps.

- **`_node_histories()`** — groups node-level steps by `node_id`, returning a list of `NodeHistory` objects ordered by the position of each group's first step in the flat `steps` list. Process-level steps (`PROCESS_STARTED`, `PROCESS_COMPLETED`, `PROCESS_FAILED`) are excluded regardless of whether they carry a `node_id` — they are rendered separately. Used by `to_markdown()` and `to_table()` for their grouped/display views.

### Node-Grouped View

`_node_histories()` produces the node-grouped view used by `to_markdown()`. Each group becomes a `NodeHistory`:

```go
type NodeHistory struct {
    NodeID   string
    NodeName *string          // human-readable name (if set on the node)
    NodeType string           // "StartEvent", "NativeFunctionTask", "ExclusiveGateway", etc.
    Steps    []ExecutionStep  // chronological steps for this node
}
```

Groups are ordered by the position of their first step in the flat `steps` list. A node only appears if it became involved in the execution — nodes on branches that were never reached are absent. Process-level steps (`PROCESS_STARTED`, `PROCESS_COMPLETED`, `PROCESS_FAILED`) are rendered outside the Nodes section even though they carry a `node_id`.

### Serialization

- **`to_json()`** — returns a JSON string representation of the full execution history. The JSON structure contains a top-level object with process metadata fields and a flat `steps` array. Each step includes a computed `sequence_number` (1-indexed position) and all non-null fields (`execution_id`, `type`, `node_id`, `node_name`, `node_type`, `start_timestamp`, `end_timestamp`, `selected_paths`, `error`, `iteration`, `instance`). DateTime values are serialized as ISO 8601 strings.

- **`to_table()`** — returns a flat tabular string representation of the execution history. Each row represents a single step. Columns: `seq` (computed 1-indexed position), `execution_id`, `node_id`, `node_type`, `type`, `start_timestamp`, `end_timestamp`. The table is preceded by a header section with the process metadata fields (ordered: `process_instance_id`, `process_id`, `process_version`, then remaining fields). Columns for `iteration`, `instance`, `selected_paths`, and `error` are included only when at least one step has a non-null value. Suitable for CLI output and log files.

- **`to_markdown()`** — returns the node-grouped format shown in the examples throughout this spec. Calls `_node_histories()` to group steps by node. Process-level steps are rendered outside the Nodes section. This is the human-readable representation used for inspection and debugging. Token markers (`◀ token`) are included for histories where the process is still `RUNNING`.

- **`from_json(json_str)`** — reconstructs an `ExecutionHistory` from a JSON string produced by `to_json()`. Returns a fully hydrated instance with all steps restored. Raises `ValueError` if the JSON is malformed or missing required fields.

- **`from_table(table_str)`** — reconstructs an `ExecutionHistory` from a table string produced by `to_table()`. Parses the header section for process metadata and each table row for step data. Raises `ValueError` if the table is malformed.

### Ordering

Steps are ordered by their position in the `steps` list. The `record()` method appends steps in the order they are received. The **sequence number** displayed in rendered output (e.g. `[1]`, `[2]`) is the 1-indexed position in the list — it is not stored on the step itself. When parallel tasks record steps concurrently, the StateStore serializes appends and assigns ordering.

Each step carries an `evaluation_id` that identifies which `evaluate()` call produced it. Steps recorded during task execution (`NODE_STARTED`, `NODE_COMPLETED`, `NODE_ITERATION_COMPLETED`, `NODE_FAILED`) have `evaluation_id: None`.

---

## ExecutionStep

Each `ExecutionStep` captures a single event in the execution timeline — a node scheduled, a node started, a node completed, a gateway resolved, a task failed, etc.

```go
type ExecutionStep struct {
    // Identifies which task execution produced this step — the ProcessTask.ExecutionID
    // for steps recorded by Evaluate(), or a unique task execution id for steps
    // recorded during task execution. Steps from the same Evaluate() call share the
    // same ExecutionID. Steps from the same task execution share the same ExecutionID.
    ExecutionID string

    // Which Evaluate() call produced this step (nil for task execution steps)
    EvaluationID *string

    // The node this step pertains to. Set on all step types except PROCESS_FAILED:
    // - PROCESS_STARTED: the start node id (NodeType: "StartEvent")
    // - PROCESS_COMPLETED: the end node id (NodeType: "EndEvent")
    // - PROCESS_FAILED: nil (failure may not be attributable to a single node)
    // - All other types: the node that was scheduled, started, completed, resolved, etc.
    NodeID   *string
    NodeName *string
    NodeType *string // "StartEvent", "NativeFunctionTask", "ExclusiveGateway", etc.

    // What happened
    Type ExecutionStepType

    // Timestamps — each step uses exactly one: StartTimestamp for start-phase events,
    // EndTimestamp for completion-phase events. No step carries both.
    StartTimestamp *time.Time // for PROCESS_STARTED, NODE_SCHEDULED, NODE_STARTED
    EndTimestamp   *time.Time // for NODE_COMPLETED, NODE_ITERATION_COMPLETED, NODE_FAILED, GATEWAY_RESOLVED, PROCESS_COMPLETED, PROCESS_FAILED

    // For gateways: the ids of the nodes on the selected outgoing paths
    SelectedPaths []string

    // For failed steps: the error
    Error *ExecutionError

    // For loop iterations: which iteration produced this step (1-indexed)
    Iteration *int

    // For multi-instance: which instance produced this step (1-indexed, matches collection order)
    Instance *int
}

type ExecutionStepType int

const (
    StepNodeScheduled          ExecutionStepType = iota // Evaluate() identified this node as ready and enqueued a task
    StepNodeStarted                                     // a worker picked up the task and began execution
    StepNodeIterationCompleted                          // a loop iteration or multi-instance instance completed successfully (not the final one)
    StepNodeCompleted                                   // execution finished successfully (final iteration/instance for loops/multi-instance)
    StepNodeFailed                                      // execution failed (error in the Error field)
    StepGatewayResolved                                 // a gateway evaluated its condition(s)
    StepProcessStarted                                  // process execution began
    StepProcessCompleted                                // process reached an EndEvent successfully
    StepProcessFailed                                   // process terminated due to an unhandled error
)

type ExecutionError struct {
    Message    string
    Code       *string
    Type       string  // e.g. "NativeFunctionError", "GatewayEvaluationError"
    StackTrace *string // available in development environments; omitted in production
}
```

---

## Step Recording Rules

Steps are recorded by the worker during the evaluate–execute loop:

1. **`PROCESS_STARTED`** — recorded by `evaluate()` once when the execution history is empty (initial evaluation). `node_id` is set to the start node id selected for this execution. Uses `start_timestamp`.
2. **`NODE_SCHEDULED`** — recorded by `evaluate()` when it reaches a task node and dispatches it. This marks the task as dispatched in the history, which is essential for idempotent evaluation. Uses `start_timestamp` (the moment the task was dispatched).
3. **`NODE_STARTED`** — recorded by the worker when it begins executing a task. Uses `start_timestamp`.
4. **`NODE_ITERATION_COMPLETED`** — recorded by the worker after a loop iteration or multi-instance instance completes successfully, when further iterations/instances remain. Uses `end_timestamp`. Carries `iteration` (for loops) or `instance` (for multi-instance).
5. **`NODE_COMPLETED`** — recorded by `evaluate()` for non-task nodes (start events, end events). Recorded by the worker after a task returns successfully (or after the final loop iteration / final multi-instance instance). Uses `end_timestamp`.
6. **`GATEWAY_RESOLVED`** — recorded by `evaluate()` when it reaches a gateway and evaluates its conditions, capturing which paths were selected. Uses `end_timestamp`.
7. **`NODE_FAILED`** — recorded by the worker if a task throws or returns an error. The `error` field is populated. Uses `end_timestamp`.
8. **`PROCESS_COMPLETED`** — recorded by `evaluate()` when an `EndEvent` has been reached and all active paths are complete. `node_id` is set to the end node id that consumed the final token. Uses `end_timestamp`.
9. **`PROCESS_FAILED`** — recorded by `evaluate()` when a `NODE_FAILED` step is not recovered by an error boundary event and the process halts. `node_id` is `None`. Uses `end_timestamp`.

Nodes on non-taken gateway branches do not appear in the execution history at all — there is no "skipped" step type.

---

## Replaying History

`ExecutionHistory` provides a chronological audit trail but does not itself support replay. Replaying a process from a specific step requires enqueuing a new initial `ProcessTask` (or calling `Process.Evaluate` directly with fresh state from `store.NewExecutionState`) with the desired input.

---

## Durability and the StateStore

The canonical way to obtain an `ExecutionHistory` is via `StateStore.NewExecutionState(...)` (fresh) or `StateStore.LoadExecutionState(...)` (resume). Both factories return a history wired to the store's writer channel; the durability behaviour below depends on that wiring. Direct construction yields an un-wired history whose `Record(...)` calls do not persist.

The in-memory `ExecutionHistory` object held by the worker is mirrored to the configured `StateStore` (see [state-store.spec.md](state-store.spec.md)) one event at a time. Every call to `Record(step)` on a wired history enqueues an `OpRecordStep` write op on the writer pool, which drains in the background and calls `StateStore.WriteBatch`.

Each `ExecutionStep` carries its own `start_timestamp` / `end_timestamp` and is self-describing for ordering purposes — the durable backend does not need to preserve arrival order. On worker pickup, `ExecutionHistory` is reconstructed via `StateStore.Get(processInstanceID)`, which sorts the durable events by timestamp and replays them into a fresh `ExecutionHistory`. `Save(processInstanceID, history)` persists the history-level metadata (`process_instance_status`, `evaluation_count`, `started_at`, `completed_at`) at evaluation boundaries.

---

## Who Records What

All steps are recorded by the same worker that owns the process task. The worker alternates between two phases:

- **Evaluation phase** — the worker calls `evaluate()`, which appends steps for non-task nodes (`PROCESS_STARTED`, `NODE_COMPLETED` for events, `GATEWAY_RESOLVED`, `PROCESS_COMPLETED`, `PROCESS_FAILED`) and `NODE_SCHEDULED` for tasks it dispatches. All steps from the same `evaluate()` call share the same `execution_id` (= ProcessTask.execution_id) and the same `evaluation_id`.
- **Task execution phase** — the worker executes tasks locally and records `NODE_STARTED` before execution, then `NODE_COMPLETED` (or `NODE_ITERATION_COMPLETED` / `NODE_FAILED`) after execution. All steps from the same task execution share the same `execution_id` and have `evaluation_id: None`.

`evaluate()` records `NODE_SCHEDULED` when it reaches a task node. This is essential for idempotent evaluation — without a `NODE_SCHEDULED` record, a re-evaluation after a crash and re-enqueue would see no evidence that the task was already dispatched and would dispatch it again.

---

## Token Markers

Steps where a token currently resides are marked with `◀ token`. A node holds a token when:

- Its most recent step is `NODE_SCHEDULED` or `NODE_STARTED` — the task is dispatched and waiting for (or being executed by) a worker.
- Its most recent step is `NODE_COMPLETED` but its successor has not yet been scheduled by a later evaluation — the node finished but the token hasn't advanced yet (e.g. blocked at a parallel join).

When the process completes or fails, all tokens are consumed and no markers appear.

---

## Linear Process

For a process `start → validate → review → end`:

```go
type StepInputs struct{}
type StepOutputs struct{ Status bl.BlString }

var validate = bl.NewNativeFunctionTask(NativeFunctionTaskOpts[StepInputs, StepOutputs]{
    Id: "validate", Name: "Validate",
    Fn: func(in *StepInputs) (StepOutputs, error) { /* body */ },
})
var review = bl.NewNativeFunctionTask(NativeFunctionTaskOpts[StepInputs, StepOutputs]{
    Id: "review", Name: "Review",
    Fn: func(in *StepInputs) (StepOutputs, error) { /* body */ },
})

var simpleReview = bl.NewProcess("simple-review", "1.0", ProcessOpts{
    Graph: []ProcessNode{
        bl.Start("start", "Start", bl.NewInputContract()).To(validate),
        validate.To(review),
        review.To(bl.End("done", "Done")),
    },
})
```

### After initial evaluation

A worker picks up the initial `ProcessTask` and calls `evaluate()`. The history is empty, so a token is placed at the start node. `evaluate()` walks through the start event and stops at `validate`, dispatching it.

```
process_id: "simple-review"
process_version: "1.0"
process_instance_id: "pi-001"
process_instance_status: RUNNING
enqueued_at: 2026-04-03T09:59:59Z
started_at: 2026-04-03T10:00:00Z
completed_at: None
evaluation_count: 1

[1] PROCESS_STARTED
    execution_id: "eval-001"
    node_id: "start"
    start_timestamp: 2026-04-03T10:00:00Z

Nodes:
  start (StartEvent):
    [2] NODE_COMPLETED
      execution_id: "eval-001"
      end_timestamp: 2026-04-03T10:00:00Z

  validate (NativeFunctionTask):
    [3] NODE_SCHEDULED  ◀ token
      execution_id: "eval-001"
      start_timestamp: 2026-04-03T10:00:00Z
```

### After validate completes

Tokens are still unconsumed, so the worker re-enqueues a `ProcessTask`. On the next run, `evaluate()` sees `validate` completed, walks through to `review`, and stops.

```
process_id: "simple-review"
process_version: "1.0"
process_instance_id: "pi-001"
process_instance_status: RUNNING
enqueued_at: 2026-04-03T09:59:59Z
started_at: 2026-04-03T10:00:00Z
completed_at: None
evaluation_count: 2

[1] PROCESS_STARTED
    execution_id: "eval-001"
    node_id: "start"
    start_timestamp: 2026-04-03T10:00:00Z

Nodes:
  start (StartEvent):
    [2] NODE_COMPLETED
      execution_id: "eval-001"
      end_timestamp: 2026-04-03T10:00:00Z

  validate (NativeFunctionTask):
    [3] NODE_SCHEDULED
      execution_id: "eval-001"
      start_timestamp: 2026-04-03T10:00:00Z
    [4] NODE_STARTED
      execution_id: "act-001"
      start_timestamp: 2026-04-03T10:00:00Z
    [5] NODE_COMPLETED
      execution_id: "act-001"
      end_timestamp: 2026-04-03T10:00:01Z

  review (NativeFunctionTask):
    [6] NODE_SCHEDULED  ◀ token
      execution_id: "eval-002"
      start_timestamp: 2026-04-03T10:00:02Z
```

### After review completes (final)

Tokens are still unconsumed, so the worker re-enqueues a `ProcessTask` again. On the next run, `evaluate()` sees `review` completed, walks through the end event, and records process completion.

```
process_id: "simple-review"
process_version: "1.0"
process_instance_id: "pi-001"
process_instance_status: COMPLETED
enqueued_at: 2026-04-03T09:59:59Z
started_at: 2026-04-03T10:00:00Z
completed_at: 2026-04-03T10:00:06Z
evaluation_count: 3

[1] PROCESS_STARTED
    execution_id: "eval-001"
    node_id: "start"
    start_timestamp: 2026-04-03T10:00:00Z

Nodes:
  start (StartEvent):
    [2] NODE_COMPLETED
      execution_id: "eval-001"
      end_timestamp: 2026-04-03T10:00:00Z

  validate (NativeFunctionTask):
    [3] NODE_SCHEDULED
      execution_id: "eval-001"
      start_timestamp: 2026-04-03T10:00:00Z
    [4] NODE_STARTED
      execution_id: "act-001"
      start_timestamp: 2026-04-03T10:00:00Z
    [5] NODE_COMPLETED
      execution_id: "act-001"
      end_timestamp: 2026-04-03T10:00:01Z

  review (NativeFunctionTask):
    [6] NODE_SCHEDULED
      execution_id: "eval-002"
      start_timestamp: 2026-04-03T10:00:02Z
    [7] NODE_STARTED
      execution_id: "act-002"
      start_timestamp: 2026-04-03T10:00:02Z
    [8] NODE_COMPLETED
      execution_id: "act-002"
      end_timestamp: 2026-04-03T10:00:05Z

  done (EndEvent):
    [9] NODE_COMPLETED
      execution_id: "eval-003"
      end_timestamp: 2026-04-03T10:00:06Z

[10] PROCESS_COMPLETED
     execution_id: "eval-003"
     node_id: "done"
     end_timestamp: 2026-04-03T10:00:06Z
```

---

## Parallel Branches

For a process with an AND split and join: `start → AND(check-credit, check-income) → JOIN → decide → end`:

```go
type StepInputs struct{}
type StepOutputs struct{ Status bl.BlString }

var checkCredit = bl.NewNativeFunctionTask(NativeFunctionTaskOpts[StepInputs, StepOutputs]{
    Id: "check-credit", Name: "Check Credit",
    Fn: func(in *StepInputs) (StepOutputs, error) { /* body */ },
})
var checkIncome = bl.NewNativeFunctionTask(NativeFunctionTaskOpts[StepInputs, StepOutputs]{
    Id: "check-income", Name: "Check Income",
    Fn: func(in *StepInputs) (StepOutputs, error) { /* body */ },
})
decide := decisionTemplate.Clone(DecisionTaskOpts{
    Id:             "decide",
    Name:           "Decide",
    InputMappings:  bl.NewVariableMapping(),
    OutputMappings: bl.NewVariableMapping(),
})

parallelChecks := bl.NewProcess("parallel-checks", "1.0", ProcessOpts{
    Graph: []Edge{
        bl.Start("start", "Start", bl.NewInputContract()).To(And(checkCredit, checkIncome)),
        Join(checkCredit, checkIncome).To(decide),
        decide.To(bl.End("done", "Done")),
    },
})
```

### After initial evaluation

`evaluate()` walks through the start event and the AND gateway, placing tokens on both branches.

```
process_id: "parallel-checks"
process_version: "1.0"
process_instance_id: "pi-002"
process_instance_status: RUNNING
enqueued_at: 2026-04-03T09:59:59Z
started_at: 2026-04-03T10:00:00Z
completed_at: None
evaluation_count: 1

[1] PROCESS_STARTED
    execution_id: "eval-010"
    node_id: "start"
    start_timestamp: 2026-04-03T10:00:00Z

Nodes:
  start (StartEvent):
    [2] NODE_COMPLETED
      execution_id: "eval-010"
      end_timestamp: 2026-04-03T10:00:00Z

  and-split (ParallelGateway):
    [3] GATEWAY_RESOLVED
      execution_id: "eval-010"
      end_timestamp: 2026-04-03T10:00:00Z
      selected_paths: ["check-credit", "check-income"]

  check-credit (NativeFunctionTask):
    [4] NODE_SCHEDULED  ◀ token
      execution_id: "eval-010"
      start_timestamp: 2026-04-03T10:00:00Z

  check-income (NativeFunctionTask):
    [5] NODE_SCHEDULED  ◀ token
      execution_id: "eval-010"
      start_timestamp: 2026-04-03T10:00:00Z
```

### After check-income completes (join blocked)

`check-income` completes first. `evaluate()` checks the join — `check-credit` is still in progress, so the join is not satisfied. No new steps are recorded by this evaluation.

```
process_id: "parallel-checks"
process_version: "1.0"
process_instance_id: "pi-002"
process_instance_status: RUNNING
enqueued_at: 2026-04-03T09:59:59Z
started_at: 2026-04-03T10:00:00Z
completed_at: None
evaluation_count: 2

[1] PROCESS_STARTED
    execution_id: "eval-010"
    node_id: "start"
    start_timestamp: 2026-04-03T10:00:00Z

Nodes:
  start (StartEvent):
    [2] NODE_COMPLETED
      execution_id: "eval-010"
      end_timestamp: 2026-04-03T10:00:00Z

  and-split (ParallelGateway):
    [3] GATEWAY_RESOLVED
      execution_id: "eval-010"
      end_timestamp: 2026-04-03T10:00:00Z
      selected_paths: ["check-credit", "check-income"]

  check-credit (NativeFunctionTask):
    [4] NODE_SCHEDULED  ◀ token
      execution_id: "eval-010"
      start_timestamp: 2026-04-03T10:00:00Z

  check-income (NativeFunctionTask):
    [5] NODE_SCHEDULED
      execution_id: "eval-010"
      start_timestamp: 2026-04-03T10:00:00Z
    [6] NODE_STARTED
      execution_id: "act-011"
      start_timestamp: 2026-04-03T10:00:00Z
    [7] NODE_COMPLETED  ◀ token
      execution_id: "act-011"
      end_timestamp: 2026-04-03T10:00:03Z
```

### After check-credit completes (join fires)

Both branches have now completed. The join is satisfied. `evaluate()` walks through the join and stops at `decide`.

```
process_id: "parallel-checks"
process_version: "1.0"
process_instance_id: "pi-002"
process_instance_status: RUNNING
enqueued_at: 2026-04-03T09:59:59Z
started_at: 2026-04-03T10:00:00Z
completed_at: None
evaluation_count: 3

[1] PROCESS_STARTED
    execution_id: "eval-010"
    node_id: "start"
    start_timestamp: 2026-04-03T10:00:00Z

Nodes:
  start (StartEvent):
    [2] NODE_COMPLETED
      execution_id: "eval-010"
      end_timestamp: 2026-04-03T10:00:00Z

  and-split (ParallelGateway):
    [3] GATEWAY_RESOLVED
      execution_id: "eval-010"
      end_timestamp: 2026-04-03T10:00:00Z
      selected_paths: ["check-credit", "check-income"]

  check-credit (NativeFunctionTask):
    [4] NODE_SCHEDULED
      execution_id: "eval-010"
      start_timestamp: 2026-04-03T10:00:00Z
    [8] NODE_STARTED
      execution_id: "act-012"
      start_timestamp: 2026-04-03T10:00:01Z
    [9] NODE_COMPLETED
      execution_id: "act-012"
      end_timestamp: 2026-04-03T10:00:05Z

  check-income (NativeFunctionTask):
    [5] NODE_SCHEDULED
      execution_id: "eval-010"
      start_timestamp: 2026-04-03T10:00:00Z
    [6] NODE_STARTED
      execution_id: "act-011"
      start_timestamp: 2026-04-03T10:00:00Z
    [7] NODE_COMPLETED
      execution_id: "act-011"
      end_timestamp: 2026-04-03T10:00:03Z

  decide (DecisionTask):
    [10] NODE_SCHEDULED  ◀ token
      execution_id: "eval-013"
      start_timestamp: 2026-04-03T10:00:06Z
```

### After decide completes (final)

`decide` completes. `evaluate()` walks through the end event and records process completion.

```
process_id: "parallel-checks"
process_version: "1.0"
process_instance_id: "pi-002"
process_instance_status: COMPLETED
enqueued_at: 2026-04-03T09:59:59Z
started_at: 2026-04-03T10:00:00Z
completed_at: 2026-04-03T10:00:08Z
evaluation_count: 4

[1] PROCESS_STARTED
    execution_id: "eval-010"
    node_id: "start"
    start_timestamp: 2026-04-03T10:00:00Z

Nodes:
  start (StartEvent):
    [2] NODE_COMPLETED
      execution_id: "eval-010"
      end_timestamp: 2026-04-03T10:00:00Z

  and-split (ParallelGateway):
    [3] GATEWAY_RESOLVED
      execution_id: "eval-010"
      end_timestamp: 2026-04-03T10:00:00Z
      selected_paths: ["check-credit", "check-income"]

  check-credit (NativeFunctionTask):
    [4] NODE_SCHEDULED
      execution_id: "eval-010"
      start_timestamp: 2026-04-03T10:00:00Z
    [8] NODE_STARTED
      execution_id: "act-012"
      start_timestamp: 2026-04-03T10:00:01Z
    [9] NODE_COMPLETED
      execution_id: "act-012"
      end_timestamp: 2026-04-03T10:00:05Z

  check-income (NativeFunctionTask):
    [5] NODE_SCHEDULED
      execution_id: "eval-010"
      start_timestamp: 2026-04-03T10:00:00Z
    [6] NODE_STARTED
      execution_id: "act-011"
      start_timestamp: 2026-04-03T10:00:00Z
    [7] NODE_COMPLETED
      execution_id: "act-011"
      end_timestamp: 2026-04-03T10:00:03Z

  decide (DecisionTask):
    [10] NODE_SCHEDULED
      execution_id: "eval-013"
      start_timestamp: 2026-04-03T10:00:06Z
    [11] NODE_STARTED
      execution_id: "act-014"
      start_timestamp: 2026-04-03T10:00:06Z
    [12] NODE_COMPLETED
      execution_id: "act-014"
      end_timestamp: 2026-04-03T10:00:07Z

  done (EndEvent):
    [13] NODE_COMPLETED
      execution_id: "eval-015"
      end_timestamp: 2026-04-03T10:00:08Z

[14] PROCESS_COMPLETED
     execution_id: "eval-015"
     node_id: "done"
     end_timestamp: 2026-04-03T10:00:08Z
```

### Interleaving

When parallel branches are active, `NODE_COMPLETED` steps from different branches may arrive in any order. The history reflects the actual completion order — notice how `check-income` completed (step 7) before `check-credit` (step 9), even though `check-credit` was dispatched first (step 4). The worker waits for all parallel tasks to complete, then re-evaluates to advance past the join.

---

## Exclusive Gateway

For a process with an XOR split: `start → assess-risk → XOR(approve, reject) → end`:

```go
type StepInputs struct{}
type StepOutputs struct{ Status bl.BlString }

assessRisk := riskTemplate.Clone(DecisionTaskOpts{
    Id:             "assess-risk",
    Name:           "Assess Risk",
    InputMappings:  bl.NewVariableMapping(),
    OutputMappings: bl.NewVariableMapping(),
})
var approve = bl.NewNativeFunctionTask(NativeFunctionTaskOpts[StepInputs, StepOutputs]{
    Id: "approve", Name: "Approve",
    Fn: func(in *StepInputs) (StepOutputs, error) { /* body */ },
})
var reject = bl.NewNativeFunctionTask(NativeFunctionTaskOpts[StepInputs, StepOutputs]{
    Id: "reject", Name: "Reject",
    Fn: func(in *StepInputs) (StepOutputs, error) { /* body */ },
})

type RiskEnv struct {
    RiskLevel bl.BlString `expr:"risk_level" ctx:"assess-risk.risk_level"`
}

riskDecision := bl.NewProcess("risk-decision", "1.0", ProcessOpts{
    Graph: []Edge{
        bl.Start("start", "Start", bl.NewInputContract()).To(assessRisk),
        assessRisk.To(Xor[RiskEnv](
            bl.Branch("approve", `risk_level = "low"`, approve),
            bl.DefaultBranch("reject", reject),
        )),
        approve.To(bl.End("done", "Done")),
        reject.To(bl.End("done", "Done")),
    },
})
```

The initial evaluation follows the standard pattern — `evaluate()` walks through start and stops at `assess-risk`. After `assess-risk` completes with `risk_level: "low"`, the re-evaluation walks through the XOR gateway:

### After assess-risk completes (gateway resolves)

```
process_id: "risk-decision"
process_version: "1.0"
process_instance_id: "pi-003"
process_instance_status: RUNNING
enqueued_at: 2026-04-03T09:59:59Z
started_at: 2026-04-03T10:00:00Z
completed_at: None
evaluation_count: 2

[1] PROCESS_STARTED
    execution_id: "eval-020"
    node_id: "start"
    start_timestamp: 2026-04-03T10:00:00Z

Nodes:
  start (StartEvent):
    [2] NODE_COMPLETED
      execution_id: "eval-020"
      end_timestamp: 2026-04-03T10:00:00Z

  assess-risk (DecisionTask):
    [3] NODE_SCHEDULED
      execution_id: "eval-020"
      start_timestamp: 2026-04-03T10:00:00Z
    [4] NODE_STARTED
      execution_id: "act-021"
      start_timestamp: 2026-04-03T10:00:00Z
    [5] NODE_COMPLETED
      execution_id: "act-021"
      end_timestamp: 2026-04-03T10:00:03Z

  xor-split (ExclusiveGateway):
    [6] GATEWAY_RESOLVED
      execution_id: "eval-022"
      end_timestamp: 2026-04-03T10:00:04Z
      selected_paths: ["approve"]

  approve (NativeFunctionTask):
    [7] NODE_SCHEDULED  ◀ token
      execution_id: "eval-022"
      start_timestamp: 2026-04-03T10:00:04Z
```

The `GATEWAY_RESOLVED` step captures which path was selected. `reject` does not appear in the history — non-taken branches are simply absent.

---

## Task Loop

For a process `start → assess → end`, where `assess` has a loop that re-evaluates while the risk score is indeterminate:

```go
assess := riskTemplate.Clone(DecisionTaskOpts{
    Id:             "assess",
    Name:           "Assess Risk",
    InputMappings:  bl.NewVariableMapping(),
    OutputMappings: bl.NewVariableMapping(),
})
assess.Loop = bl.NewLoopConfig(
    bl.StringVar("assess.risk_score").Equals(bl.String("indeterminate")),
    LoopOpts{MaxIterations: 3},
)

riskCheck := bl.NewProcess("risk-check", "1.0", ProcessOpts{
    Graph: []Edge{
        bl.Start("start", "Start", bl.NewInputContract()).To(assess),
        assess.To(bl.End("done", "Done")),
    },
})
```

### After initial evaluation

```
process_id: "risk-check"
process_version: "1.0"
process_instance_id: "pi-007"
process_instance_status: RUNNING
enqueued_at: 2026-04-03T09:59:59Z
started_at: 2026-04-03T10:00:00Z
completed_at: None
evaluation_count: 1

[1] PROCESS_STARTED
    execution_id: "eval-050"
    node_id: "start"
    start_timestamp: 2026-04-03T10:00:00Z

Nodes:
  start (StartEvent):
    [2] NODE_COMPLETED
      execution_id: "eval-050"
      end_timestamp: 2026-04-03T10:00:00Z

  assess (DecisionTask):
    [3] NODE_SCHEDULED  ◀ token
      execution_id: "eval-050"
      start_timestamp: 2026-04-03T10:00:00Z
```

### After assess completes (2 loop iterations — final)

The worker executes the loop locally. Iteration 1 returns `risk_score: "indeterminate"` — the loop condition is true, so the task executes again. Iteration 2 returns `risk_score: "low"` — the loop condition is false, so the loop terminates. Intermediate iterations are recorded as `NODE_ITERATION_COMPLETED`; only the final iteration is `NODE_COMPLETED`.

```
process_id: "risk-check"
process_version: "1.0"
process_instance_id: "pi-007"
process_instance_status: COMPLETED
enqueued_at: 2026-04-03T09:59:59Z
started_at: 2026-04-03T10:00:00Z
completed_at: 2026-04-03T10:00:03Z
evaluation_count: 2

[1] PROCESS_STARTED
    execution_id: "eval-050"
    node_id: "start"
    start_timestamp: 2026-04-03T10:00:00Z

Nodes:
  start (StartEvent):
    [2] NODE_COMPLETED
      execution_id: "eval-050"
      end_timestamp: 2026-04-03T10:00:00Z

  assess (DecisionTask):
    [3] NODE_SCHEDULED
      execution_id: "eval-050"
      start_timestamp: 2026-04-03T10:00:00Z
    [4] NODE_STARTED
      execution_id: "act-051"
      start_timestamp: 2026-04-03T10:00:00Z
    [5] NODE_ITERATION_COMPLETED (iteration 1 — loop condition true)
      execution_id: "act-051"
      iteration: 1
      end_timestamp: 2026-04-03T10:00:01Z
    [6] NODE_COMPLETED (iteration 2 — loop condition false, complete)
      execution_id: "act-051"
      iteration: 2
      end_timestamp: 2026-04-03T10:00:02Z

  done (EndEvent):
    [7] NODE_COMPLETED
      execution_id: "eval-052"
      end_timestamp: 2026-04-03T10:00:03Z

[8] PROCESS_COMPLETED
    execution_id: "eval-052"
    node_id: "done"
    end_timestamp: 2026-04-03T10:00:03Z
```

All loop iterations share the same `execution_id` — they are part of a single task execution. The `iteration` field (1-indexed) identifies which iteration produced the step. Intermediate iterations use `NODE_ITERATION_COMPLETED`; the final iteration uses `NODE_COMPLETED`, signalling that the task is done and the token can advance.

---

## Multi-Instance Task

For a process `start → send → end`, where `send` runs once per recipient in a collection:

```go
type SendInputs struct{ Recipient bl.BlDictionary }
type SendOutputs struct{ Delivered bl.BlBoolean }

var send = bl.NewNativeFunctionTask(NativeFunctionTaskOpts[SendInputs, SendOutputs]{
    Id: "send", Name: "Send Notification",
    InputBindings: func(in SendInputs) []ParameterBinding {
        return []ParameterBinding{
            bl.Bind(in.Recipient, bl.MultiInstanceItem[bl.BlDictionary]()),
        }
    },
    Fn: func(in *SendInputs) (SendOutputs, error) { /* body */ },
    MultiInstance: bl.NewMultiInstanceConfig(
        bl.ListVar("start.recipients"),
        MultiInstanceOpts{ElementVariable: "recipient", IsSequential: false},
    ),
})

var notifyAll = bl.NewProcess("notify-all", "1.0", ProcessOpts{
    Graph: []ProcessNode{
        bl.Start("start", "Start", bl.NewInputContract()).To(send),
        send.To(bl.End("done", "Done")),
    },
})
```

### After initial evaluation

```
process_id: "notify-all"
process_version: "1.0"
process_instance_id: "pi-008"
process_instance_status: RUNNING
enqueued_at: 2026-04-03T09:59:59Z
started_at: 2026-04-03T10:00:00Z
completed_at: None
evaluation_count: 1

[1] PROCESS_STARTED
    execution_id: "eval-060"
    node_id: "start"
    start_timestamp: 2026-04-03T10:00:00Z

Nodes:
  start (StartEvent):
    [2] NODE_COMPLETED
      execution_id: "eval-060"
      end_timestamp: 2026-04-03T10:00:00Z

  send (NativeFunctionTask):
    [3] NODE_SCHEDULED  ◀ token
      execution_id: "eval-060"
      start_timestamp: 2026-04-03T10:00:00Z
```

### After send completes (3 parallel instances — final)

The worker dispatches all 3 instances concurrently in separate goroutines. Each instance completion is recorded. Intermediate instances use `NODE_ITERATION_COMPLETED`; the final instance uses `NODE_COMPLETED`. The order reflects actual completion time, not collection order.

```
process_id: "notify-all"
process_version: "1.0"
process_instance_id: "pi-008"
process_instance_status: COMPLETED
enqueued_at: 2026-04-03T09:59:59Z
started_at: 2026-04-03T10:00:00Z
completed_at: 2026-04-03T10:00:04Z
evaluation_count: 2

[1] PROCESS_STARTED
    execution_id: "eval-060"
    node_id: "start"
    start_timestamp: 2026-04-03T10:00:00Z

Nodes:
  start (StartEvent):
    [2] NODE_COMPLETED
      execution_id: "eval-060"
      end_timestamp: 2026-04-03T10:00:00Z

  send (NativeFunctionTask):
    [3] NODE_SCHEDULED
      execution_id: "eval-060"
      start_timestamp: 2026-04-03T10:00:00Z
    [4] NODE_STARTED
      execution_id: "act-061"
      start_timestamp: 2026-04-03T10:00:00Z
    [5] NODE_ITERATION_COMPLETED (instance 2 of 3)
      execution_id: "act-061"
      instance: 2
      end_timestamp: 2026-04-03T10:00:01Z
    [6] NODE_ITERATION_COMPLETED (instance 1 of 3)
      execution_id: "act-061"
      instance: 1
      end_timestamp: 2026-04-03T10:00:02Z
    [7] NODE_COMPLETED (instance 3 of 3 — all instances complete)
      execution_id: "act-061"
      instance: 3
      end_timestamp: 2026-04-03T10:00:03Z

  done (EndEvent):
    [8] NODE_COMPLETED
      execution_id: "eval-062"
      end_timestamp: 2026-04-03T10:00:04Z

[9] PROCESS_COMPLETED
    execution_id: "eval-062"
    node_id: "done"
    end_timestamp: 2026-04-03T10:00:04Z
```

All instances share the same `execution_id`. The `instance` field (1-indexed, matching collection order) identifies which instance produced the step. For parallel multi-instance (`is_sequential: false`), instances complete in non-deterministic order — notice instance 2 completed before instance 1. For sequential multi-instance (`is_sequential: true`), instances complete in collection order (1, 2, 3). Intermediate instances use `NODE_ITERATION_COMPLETED`; the last instance to finish uses `NODE_COMPLETED`, signalling that the task is done and the token can advance.

---

## Loopbacks

For a process with a loopback: `start → review → XOR(approved → end, needs_revision → revise → review)`:

```go
type StepInputs struct{}
type StepOutputs struct{ Status bl.BlString }

var review = bl.NewNativeFunctionTask(NativeFunctionTaskOpts[StepInputs, StepOutputs]{
    Id: "review", Name: "Review",
    Fn: func(in *StepInputs) (StepOutputs, error) { /* body */ },
})
var revise = bl.NewNativeFunctionTask(NativeFunctionTaskOpts[StepInputs, StepOutputs]{
    Id: "revise", Name: "Revise",
    Fn: func(in *StepInputs) (StepOutputs, error) { /* body */ },
})

type ReviewEnv struct {
    Status bl.BlString `expr:"status" ctx:"review.status"`
}

docReview := bl.NewProcess("doc-review", "1.0", ProcessOpts{
    Graph: []Edge{
        bl.Start("start", "Start", bl.NewInputContract()).To(review),
        review.To(Xor[ReviewEnv](
            bl.Branch("approved", `status = "approved"`, bl.End("done", "Done")),
            bl.DefaultBranch("needs_revision", revise),
        )),
        revise.To(review),
    },
})
```

This example shows two iterations of the review loop before the process completes.

### After initial evaluation

```
process_id: "doc-review"
process_version: "1.0"
process_instance_id: "pi-004"
process_instance_status: RUNNING
enqueued_at: 2026-04-03T09:59:59Z
started_at: 2026-04-03T10:00:00Z
completed_at: None
evaluation_count: 1

[1] PROCESS_STARTED
    execution_id: "eval-030"
    node_id: "start"
    start_timestamp: 2026-04-03T10:00:00Z

Nodes:
  start (StartEvent):
    [2] NODE_COMPLETED
      execution_id: "eval-030"
      end_timestamp: 2026-04-03T10:00:00Z

  review (NativeFunctionTask):
    [3] NODE_SCHEDULED  ◀ token
      execution_id: "eval-030"
      start_timestamp: 2026-04-03T10:00:00Z
```

### After review completes (needs revision — loopback to revise)

`evaluate()` walks through the XOR gateway. The condition `review.status = "approved"` is false, so the default path to `revise` is selected. The token loops back.

```
process_id: "doc-review"
process_version: "1.0"
process_instance_id: "pi-004"
process_instance_status: RUNNING
enqueued_at: 2026-04-03T09:59:59Z
started_at: 2026-04-03T10:00:00Z
completed_at: None
evaluation_count: 2

[1] PROCESS_STARTED
    execution_id: "eval-030"
    node_id: "start"
    start_timestamp: 2026-04-03T10:00:00Z

Nodes:
  start (StartEvent):
    [2] NODE_COMPLETED
      execution_id: "eval-030"
      end_timestamp: 2026-04-03T10:00:00Z

  review (NativeFunctionTask):
    [3] NODE_SCHEDULED
      execution_id: "eval-030"
      start_timestamp: 2026-04-03T10:00:00Z
    [4] NODE_STARTED
      execution_id: "act-031"
      start_timestamp: 2026-04-03T10:00:00Z
    [5] NODE_COMPLETED
      execution_id: "act-031"
      end_timestamp: 2026-04-03T10:00:03Z

  xor-split (ExclusiveGateway):
    [6] GATEWAY_RESOLVED
      execution_id: "eval-032"
      end_timestamp: 2026-04-03T10:00:04Z
      selected_paths: ["revise"]

  revise (NativeFunctionTask):
    [7] NODE_SCHEDULED  ◀ token
      execution_id: "eval-032"
      start_timestamp: 2026-04-03T10:00:04Z
```

### After revise completes (review is ready again)

`evaluate()` determines `review` is ready because `revise` (step 9, `end_timestamp: T10:00:08Z`) completed more recently than `review`'s last execution (step 5, `end_timestamp: T10:00:03Z`). A fresh token is placed on `review`.

```
process_id: "doc-review"
process_version: "1.0"
process_instance_id: "pi-004"
process_instance_status: RUNNING
enqueued_at: 2026-04-03T09:59:59Z
started_at: 2026-04-03T10:00:00Z
completed_at: None
evaluation_count: 3

[1] PROCESS_STARTED
    execution_id: "eval-030"
    node_id: "start"
    start_timestamp: 2026-04-03T10:00:00Z

Nodes:
  start (StartEvent):
    [2] NODE_COMPLETED
      execution_id: "eval-030"
      end_timestamp: 2026-04-03T10:00:00Z

  review (NativeFunctionTask):
    [3] NODE_SCHEDULED
      execution_id: "eval-030"
      start_timestamp: 2026-04-03T10:00:00Z
    [4] NODE_STARTED
      execution_id: "act-031"
      start_timestamp: 2026-04-03T10:00:00Z
    [5] NODE_COMPLETED
      execution_id: "act-031"
      end_timestamp: 2026-04-03T10:00:03Z
    [10] NODE_SCHEDULED  ◀ token
      execution_id: "eval-034"
      start_timestamp: 2026-04-03T10:00:09Z

  xor-split (ExclusiveGateway):
    [6] GATEWAY_RESOLVED
      execution_id: "eval-032"
      end_timestamp: 2026-04-03T10:00:04Z
      selected_paths: ["revise"]

  revise (NativeFunctionTask):
    [7] NODE_SCHEDULED
      execution_id: "eval-032"
      start_timestamp: 2026-04-03T10:00:04Z
    [8] NODE_STARTED
      execution_id: "act-033"
      start_timestamp: 2026-04-03T10:00:04Z
    [9] NODE_COMPLETED
      execution_id: "act-033"
      end_timestamp: 2026-04-03T10:00:08Z
```

### After review completes (approved — process completes)

The XOR gateway now selects the `done` path. The token is consumed by the end event.

```
process_id: "doc-review"
process_version: "1.0"
process_instance_id: "pi-004"
process_instance_status: COMPLETED
enqueued_at: 2026-04-03T09:59:59Z
started_at: 2026-04-03T10:00:00Z
completed_at: 2026-04-03T10:00:13Z
evaluation_count: 4

[1] PROCESS_STARTED
    execution_id: "eval-030"
    node_id: "start"
    start_timestamp: 2026-04-03T10:00:00Z

Nodes:
  start (StartEvent):
    [2] NODE_COMPLETED
      execution_id: "eval-030"
      end_timestamp: 2026-04-03T10:00:00Z

  review (NativeFunctionTask):
    [3] NODE_SCHEDULED
      execution_id: "eval-030"
      start_timestamp: 2026-04-03T10:00:00Z
    [4] NODE_STARTED
      execution_id: "act-031"
      start_timestamp: 2026-04-03T10:00:00Z
    [5] NODE_COMPLETED
      execution_id: "act-031"
      end_timestamp: 2026-04-03T10:00:03Z
    [10] NODE_SCHEDULED
      execution_id: "eval-034"
      start_timestamp: 2026-04-03T10:00:09Z
    [11] NODE_STARTED
      execution_id: "act-035"
      start_timestamp: 2026-04-03T10:00:09Z
    [12] NODE_COMPLETED
      execution_id: "act-035"
      end_timestamp: 2026-04-03T10:00:12Z

  xor-split (ExclusiveGateway):
    [6] GATEWAY_RESOLVED
      execution_id: "eval-032"
      end_timestamp: 2026-04-03T10:00:04Z
      selected_paths: ["revise"]
    [13] GATEWAY_RESOLVED
      execution_id: "eval-036"
      end_timestamp: 2026-04-03T10:00:13Z
      selected_paths: ["done"]

  revise (NativeFunctionTask):
    [7] NODE_SCHEDULED
      execution_id: "eval-032"
      start_timestamp: 2026-04-03T10:00:04Z
    [8] NODE_STARTED
      execution_id: "act-033"
      start_timestamp: 2026-04-03T10:00:04Z
    [9] NODE_COMPLETED
      execution_id: "act-033"
      end_timestamp: 2026-04-03T10:00:08Z

  done (EndEvent):
    [14] NODE_COMPLETED
      execution_id: "eval-036"
      end_timestamp: 2026-04-03T10:00:13Z

[15] PROCESS_COMPLETED
     execution_id: "eval-036"
     node_id: "done"
     end_timestamp: 2026-04-03T10:00:13Z
```

Key observations:

- `review` was dispatched twice — steps 3–5 and steps 10–12, with different `execution_id` pairs for the scheduling (from evaluate()) and start/completion (from task execution). The sequence number jumps (5 → 10) show that other nodes were involved in between.
- `xor-split` resolved twice — step 6 (selected `revise`) and step 13 (selected `done`). Both resolutions appear in its section.
- `done` only appears once it is actually reached (step 14). It is absent from earlier snapshots where the gateway selected a different path.
- `revise` was scheduled, started, and completed once (steps 7–9). On the second loop iteration, the gateway selected `done` instead, so `revise` receives no further steps.

---

## SubProcessTasks

For a parent process `start → verify-id → end`, where `verify-id` is a `SubProcessTask` that runs a separate process `start → check-docs → check-identity → end`:

The parent and child processes have **separate execution histories**, each with their own `process_instance_id` and node lists. The parent's history treats the subprocess as a single task — a `NODE_SCHEDULED` / `NODE_STARTED` / `NODE_COMPLETED` sequence.

### Parent history — after initial evaluation

```
process_id: "onboarding"
process_version: "1.0"
process_instance_id: "pi-005"
process_instance_status: RUNNING
enqueued_at: 2026-04-03T09:59:59Z
started_at: 2026-04-03T10:00:00Z
completed_at: None
evaluation_count: 1

[1] PROCESS_STARTED
    execution_id: "eval-040"
    node_id: "start"
    start_timestamp: 2026-04-03T10:00:00Z

Nodes:
  start (StartEvent):
    [2] NODE_COMPLETED
      execution_id: "eval-040"
      end_timestamp: 2026-04-03T10:00:00Z

  verify-id (SubProcessTask):
    [3] NODE_SCHEDULED  ◀ token
      execution_id: "eval-040"
      start_timestamp: 2026-04-03T10:00:00Z
```

The child process runs with its own execution history under a separate `process_instance_id`.

### Parent history — after subprocess completes (final)

```
process_id: "onboarding"
process_version: "1.0"
process_instance_id: "pi-005"
process_instance_status: COMPLETED
enqueued_at: 2026-04-03T09:59:59Z
started_at: 2026-04-03T10:00:00Z
completed_at: 2026-04-03T10:00:13Z
evaluation_count: 2

[1] PROCESS_STARTED
    execution_id: "eval-040"
    node_id: "start"
    start_timestamp: 2026-04-03T10:00:00Z

Nodes:
  start (StartEvent):
    [2] NODE_COMPLETED
      execution_id: "eval-040"
      end_timestamp: 2026-04-03T10:00:00Z

  verify-id (SubProcessTask):
    [3] NODE_SCHEDULED
      execution_id: "eval-040"
      start_timestamp: 2026-04-03T10:00:00Z
    [4] NODE_STARTED
      execution_id: "act-041"
      start_timestamp: 2026-04-03T10:00:00Z
    [5] NODE_COMPLETED
      execution_id: "act-041"
      end_timestamp: 2026-04-03T10:00:12Z

  done (EndEvent):
    [6] NODE_COMPLETED
      execution_id: "eval-042"
      end_timestamp: 2026-04-03T10:00:13Z

[7] PROCESS_COMPLETED
    execution_id: "eval-042"
    node_id: "done"
    end_timestamp: 2026-04-03T10:00:13Z
```

### Child history (separate `process_instance_id`)

The child process has its own complete execution history with its own node list. It shares the parent's `ExecutionContext`, operating on a scoped view rooted at the `SubProcessTask`'s NodeID — its writes append transactions whose `NodeID` is prefixed with `verify-id.` in the shared log. The scope reports `parent_process_instance_id` set to the parent's `process_instance_id` (see [execution-context.spec.md](execution-context.spec.md#sub-process-scoping)).

---

## Failed Task

When a task throws an error, the worker records `NODE_FAILED`. The subsequent re-evaluation records `PROCESS_FAILED`.

```
process_id: "simple-review"
process_version: "1.0"
process_instance_id: "pi-006"
process_instance_status: FAILED
enqueued_at: 2026-04-03T09:59:59Z
started_at: 2026-04-03T10:00:00Z
completed_at: 2026-04-03T10:00:03Z
evaluation_count: 2

[1] PROCESS_STARTED
    execution_id: "eval-070"
    node_id: "start"
    start_timestamp: 2026-04-03T10:00:00Z

Nodes:
  start (StartEvent):
    [2] NODE_COMPLETED
      execution_id: "eval-070"
      end_timestamp: 2026-04-03T10:00:00Z

  validate (NativeFunctionTask):
    [3] NODE_SCHEDULED
      execution_id: "eval-070"
      start_timestamp: 2026-04-03T10:00:00Z
    [4] NODE_STARTED
      execution_id: "act-071"
      start_timestamp: 2026-04-03T10:00:00Z
    [5] NODE_FAILED
      execution_id: "act-071"
      end_timestamp: 2026-04-03T10:00:02Z
      error: {
        message: "Connection timeout",
        code: "TIMEOUT",
        type: "NativeFunctionError",
        stack_trace: "..."
      }

[6] PROCESS_FAILED
    execution_id: "eval-072"
    end_timestamp: 2026-04-03T10:00:03Z
```

---

## Edge Cases

- A node only appears in the history if it became involved in the execution. Nodes on branches that were never reached do not appear.
- Nodes are ordered by the position of their first step in the flat `steps` list. New nodes are appended to the bottom as they become involved.
- A node visited multiple times (via loopbacks) has all its steps in a single section. The position jumps (e.g. steps 3–5, then 10–12 for `review` in the loopback example) show that other nodes were involved in between.
- The sequence number shown in rendered output (`[1]`, `[2]`, etc.) is the 1-indexed position of the step in the `steps` list — it is not stored on the step. The flat chronological step list can be reconstructed by merging all node step lists and sorting by rendered sequence number.
- `execution_id` identifies which task execution produced the step. Steps from the same `evaluate()` call share the same `execution_id` (= ProcessTask.ExecutionID). Steps from the same task execution share the same `execution_id` (a unique task execution id). NODE_SCHEDULED (from evaluate()) and NODE_STARTED/NODE_COMPLETED (from task execution) have **different** `execution_id` values.
- Parallel branch `NODE_COMPLETED` steps may arrive in any order. The worker re-evaluates after all parallel tasks complete.
- A `NODE_SCHEDULED` step is always followed eventually by a `NODE_STARTED` step, then by a `NODE_COMPLETED` or `NODE_FAILED` step. The SCHEDULED step has a different `execution_id` (from evaluate()) than the STARTED/COMPLETED/FAILED steps (from task execution). There is no guarantee about how many other steps (on other nodes) appear between them.
- An evaluation that finds nothing to advance (e.g. a parallel join still waiting) appends zero steps. The evaluation still occurred.
- `GATEWAY_RESOLVED` steps include `selected_paths` — the ids of the nodes on the chosen outgoing paths. The gateway's `node_type` in the step identifies the gateway kind (ExclusiveGateway, ParallelGateway, InclusiveGateway).
- Nodes on non-taken gateway branches do not appear in the execution history. Only nodes that are actually scheduled, started, or completed receive steps. The `GATEWAY_RESOLVED` step's `selected_paths` field records which branches were taken.
- `PROCESS_STARTED` carries `node_id` set to the start node id. `PROCESS_COMPLETED` carries `node_id` set to the end node id that consumed the final token. `PROCESS_FAILED` has `node_id: None`.
- Sub-process histories are stored independently under their own `process_instance_id`. The parent history treats the sub-process task as a `NODE_SCHEDULED` / `NODE_STARTED` / `NODE_COMPLETED` sequence within its node section.
- `evaluation_id` is stored on each `ExecutionStep` recorded during that evaluation, allowing the node-grouped history to be regrouped into evaluation boundaries when needed.
- `evaluation_count` counts only calls to `evaluate()` on this process instance. Task executions and sub-process evaluations (which have their own `ExecutionHistory`) are not counted.
- Each step uses exactly one timestamp field: `start_timestamp` for start-phase events (PROCESS_STARTED, NODE_SCHEDULED, NODE_STARTED) and `end_timestamp` for completion-phase events (NODE_COMPLETED, NODE_ITERATION_COMPLETED, NODE_FAILED, GATEWAY_RESOLVED, PROCESS_COMPLETED, PROCESS_FAILED). No step carries both.
- `NODE_ITERATION_COMPLETED` is used for intermediate loop iterations and multi-instance instances. The final iteration/instance uses `NODE_COMPLETED`. The task is not complete (and the token does not advance) until `NODE_COMPLETED` is recorded.
- `PublishedAt` is `nil` for runs invoked directly via `Process.Evaluate(...)` without a `MessageBroker` — there is no start-command to publish in that path. When set, it records the moment the start-command for this instance was published to the broker (e.g. by `MessageBroker.Submit`).

---

## Serialization Formats

The examples below use the completed linear process (pi-001) from the Linear Process section.

### JSON (`to_json()`)

```json
{
  "process_id": "simple-review",
  "process_version": "1.0",
  "process_instance_id": "pi-001",
  "process_instance_status": "COMPLETED",
  "enqueued_at": "2026-04-03T09:59:59Z",
  "started_at": "2026-04-03T10:00:00Z",
  "completed_at": "2026-04-03T10:00:06Z",
  "evaluation_count": 3,
  "steps": [
    {"sequence_number": 1, "execution_id": "eval-001", "node_id": "start", "node_type": "StartEvent", "type": "PROCESS_STARTED", "start_timestamp": "2026-04-03T10:00:00Z"},
    {"sequence_number": 2, "execution_id": "eval-001", "node_id": "start", "node_type": "StartEvent", "type": "NODE_COMPLETED", "end_timestamp": "2026-04-03T10:00:00Z"},
    {"sequence_number": 3, "execution_id": "eval-001", "node_id": "validate", "node_name": "Validate", "node_type": "NativeFunctionTask", "type": "NODE_SCHEDULED", "start_timestamp": "2026-04-03T10:00:00Z"},
    {"sequence_number": 4, "execution_id": "act-001", "node_id": "validate", "node_name": "Validate", "node_type": "NativeFunctionTask", "type": "NODE_STARTED", "start_timestamp": "2026-04-03T10:00:00Z"},
    {"sequence_number": 5, "execution_id": "act-001", "node_id": "validate", "node_name": "Validate", "node_type": "NativeFunctionTask", "type": "NODE_COMPLETED", "end_timestamp": "2026-04-03T10:00:01Z"},
    {"sequence_number": 6, "execution_id": "eval-002", "node_id": "review", "node_name": "Review", "node_type": "NativeFunctionTask", "type": "NODE_SCHEDULED", "start_timestamp": "2026-04-03T10:00:02Z"},
    {"sequence_number": 7, "execution_id": "act-002", "node_id": "review", "node_name": "Review", "node_type": "NativeFunctionTask", "type": "NODE_STARTED", "start_timestamp": "2026-04-03T10:00:02Z"},
    {"sequence_number": 8, "execution_id": "act-002", "node_id": "review", "node_name": "Review", "node_type": "NativeFunctionTask", "type": "NODE_COMPLETED", "end_timestamp": "2026-04-03T10:00:05Z"},
    {"sequence_number": 9, "execution_id": "eval-003", "node_id": "done", "node_type": "EndEvent", "type": "NODE_COMPLETED", "end_timestamp": "2026-04-03T10:00:06Z"},
    {"sequence_number": 10, "execution_id": "eval-003", "node_id": "done", "node_type": "EndEvent", "type": "PROCESS_COMPLETED", "end_timestamp": "2026-04-03T10:00:06Z"}
  ]
}
```

Steps include only non-null fields — `node_name`, `selected_paths`, `error`, `iteration`, and `instance` are omitted when null.

### Table (`to_table()`)

```
process_instance_id: pi-001
process_id: simple-review
process_version: 1.0
process_instance_status: COMPLETED
enqueued_at: 2026-04-03T09:59:59Z
started_at: 2026-04-03T10:00:00Z
completed_at: 2026-04-03T10:00:06Z
evaluation_count: 3

seq  execution_id  node_id    node_type            type               start_timestamp       end_timestamp
---  ------------  ---------  -------------------  -----------------  --------------------  --------------------
  1  eval-001      start      StartEvent           PROCESS_STARTED    2026-04-03T10:00:00Z
  2  eval-001      start      StartEvent           NODE_COMPLETED                           2026-04-03T10:00:00Z
  3  eval-001      validate   NativeFunctionTask   NODE_SCHEDULED     2026-04-03T10:00:00Z
  4  act-001       validate   NativeFunctionTask   NODE_STARTED       2026-04-03T10:00:00Z
  5  act-001       validate   NativeFunctionTask   NODE_COMPLETED                           2026-04-03T10:00:01Z
  6  eval-002      review     NativeFunctionTask   NODE_SCHEDULED     2026-04-03T10:00:02Z
  7  act-002       review     NativeFunctionTask   NODE_STARTED       2026-04-03T10:00:02Z
  8  act-002       review     NativeFunctionTask   NODE_COMPLETED                           2026-04-03T10:00:05Z
  9  eval-003      done       EndEvent             NODE_COMPLETED                           2026-04-03T10:00:06Z
 10  eval-003      done       EndEvent             PROCESS_COMPLETED                        2026-04-03T10:00:06Z
```

The table is a flat chronological view ordered by position in the `steps` list. `PROCESS_FAILED` steps have empty `node_id` and `node_type` columns. Columns for `iteration`, `instance`, `selected_paths`, and `error` are included only when at least one step has a non-null value.

### Markdown (`to_markdown()`)

`to_markdown()` produces the node-grouped format shown throughout this spec — the same format used in the examples above. Internally it calls `_node_histories()` to group steps by node. For a `RUNNING` process, `◀ token` markers are included on steps where tokens currently reside.

### Deserialization

- **`from_json(json_str)`** — round-trips with `to_json()`. Parses the JSON structure and reconstructs the full `ExecutionHistory` with all `ExecutionStep` objects. Raises `ValueError` if the JSON is malformed or missing required fields.

- **`from_table(table_str)`** — round-trips with `to_table()`. Parses the header section for process metadata and each table row for step data. Raises `ValueError` if the table is malformed.
