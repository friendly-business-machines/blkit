# process-runtime Specification

## Purpose

Defines the single-node execution and control contract for versioned, compiled business processes, including durable instance state, graph task scheduling, cancellation, and a development REST API.

## Requirements

### Requirement: Registered processes have stable identities
The runtime SHALL register compiler-emitted `.bl` process graphs by namespace, version, and name, and SHALL reject duplicate identities. It SHALL consume the compiled graph rather than construct the process map from separately registered tasks. A start request SHALL identify exactly one registered process and provide an input valid for its declared type.

#### Scenario: Start a registered process
- **WHEN** a client starts `orders` version `1.0` process `approve` with valid input
- **THEN** the runtime creates a new instance with a unique ID associated with that exact process identity

#### Scenario: Unknown process or invalid input
- **WHEN** a client names an unregistered identity or supplies input incompatible with its declared input type
- **THEN** no instance starts and the client receives an actionable error

### Requirement: Each instance has a persisted lifecycle
The runtime SHALL persist an instance's identity, process identity, input, status, current owner when applicable, committed graph checkpoint, retry/attempt metadata, and available terminal result or failure. Status SHALL distinguish pending, running, retry-waiting, cancelling, completed, business-error, failed, cancelled, and terminated instances. A status lookup SHALL return the recorded state, named business-error node when applicable, and terminal outcome when one exists. A failed attempt while retry remains SHALL NOT be reported as a terminally failed instance.

#### Scenario: Observe a completed process
- **WHEN** a registered compiled process finishes successfully
- **THEN** its status is completed and its typed output is available to the client

#### Scenario: Inspect after restart
- **WHEN** the server is restarted with its existing state store
- **THEN** previously recorded instance identities, inputs, checkpoints, retry state, statuses, and terminal outcomes remain available

#### Scenario: Interrupted execution at restart
- **WHEN** the local server restarts after stopping while an instance was pending, running, or cancelling
- **THEN** pending work remains queued, a running attempt is recorded as interrupted and resumes from its checkpoint only if retries permit, and accepted cancellation cannot turn into a successful completion

#### Scenario: Retryable attempt is visible
- **WHEN** an execution error occurs with retries remaining
- **THEN** status distinguishes the waiting instance from a terminal failure and retains the last attempt's failure reason

### Requirement: Ready tasks execute with bounded concurrency
The runtime SHALL execute the compiler-emitted explicit graph by advancing named `.bl` task nodes only when their typed input and upstream control-flow dependencies are available. It SHALL evaluate compiled AND, OR, and XOR routes using process input and committed task outputs, propagate only activated branches, and allow independent ready tasks, including tasks from separate instances, to run concurrently subject to a configurable in-flight task limit. A completed task's output and the process's routing progress SHALL be committed as a process-state checkpoint before successor dispatch. A completed instance SHALL expose the declared process output after its graph reaches a typed normal `end`.

#### Scenario: Independent tasks run together
- **WHEN** two independent tasks become ready and capacity is available
- **THEN** both can execute without waiting for the other to finish

#### Scenario: Dependencies and capacity gate dispatch
- **WHEN** a task has an unfinished activated prerequisite or the shared in-flight limit is reached
- **THEN** it waits without exceeding the limit and runs only if its instance is still active when ready

#### Scenario: Conditional routes and joins
- **WHEN** an XOR gateway selects one branch or an OR gateway selects several branches
- **THEN** only selected branches run, and their corresponding join waits for the activated incoming branches before advancing

#### Scenario: Task failure
- **WHEN** a task reports an execution error
- **THEN** no successors are dispatched, in-flight siblings receive cancellation requests, and the attempt either waits for a policy-permitted checkpointed retry or becomes terminally failed

#### Scenario: Committed sibling output
- **WHEN** one parallel task's result commits before another task fails
- **THEN** the first task's output remains in the checkpoint and that task is not rerun on retry

### Requirement: Cancellation halts graph advancement and signals running tasks
The runtime SHALL accept a cancellation request for a pending, running, or cancelling instance. On acceptance it SHALL persist the cancellation state before any further task dispatch or gateway advancement, prevent further graph token advancement, and invoke cancellation on all `.bl` task nodes already in flight. Task completions received after acceptance SHALL NOT dispatch successors or turn the instance into completed. Cancellation is cooperative and does not guarantee rollback of work already performed by a task.

#### Scenario: Cancel before dispatch
- **WHEN** a pending instance is cancelled
- **THEN** it becomes cancelled and none of its tasks start

#### Scenario: Cancel during parallel tasks
- **WHEN** an instance with two in-flight tasks is cancelled
- **THEN** both receive cancellation calls, no further tasks are dispatched, and the instance resolves to cancelled

#### Scenario: Completion races with cancellation
- **WHEN** a task completes as a cancellation request is accepted
- **THEN** serialized state ordering decides which event is first; after cancellation wins, that completion cannot advance a token or overwrite cancelled status

#### Scenario: Repeated or terminal cancellation
- **WHEN** cancellation is repeated for an already cancelling/cancelled instance, or requested for an already completed/failed instance
- **THEN** repeated cancellation is safe and terminal completion/failure is not overwritten

### Requirement: Development server exposes process control over REST
The single-node development binary and distributed REST service SHALL provide JSON endpoints to start a registered process, inspect an instance, and request cancellation. They SHALL distinguish successful acceptance, invalid input, unknown identities, conflicting terminal states, waiting retries, completed results, modeled business errors, and execution failures with appropriate HTTP responses and status fields. Both binaries SHALL bind locally by default; distributed remote access SHALL require an operator-managed authenticated ingress.

#### Scenario: Start and inspect via HTTP
- **WHEN** a client POSTs valid input to `/processes/{namespace}/{version}/{name}/instances` and then GETs `/instances/{id}`
- **THEN** the start response identifies the instance and the GET response reports its persisted state and result when completed

#### Scenario: Cancel via HTTP
- **WHEN** a client POSTs to `/instances/{id}/cancel` while it is active, queued, or waiting for retry
- **THEN** the response acknowledges cancellation, advances no more work, and subsequent GETs report cancelling or cancelled status

#### Scenario: Invalid and missing requests
- **WHEN** input is malformed or an instance ID does not exist
- **THEN** the API returns a client error without starting or modifying an instance

#### Scenario: Distinct business and execution errors
- **WHEN** one instance reaches a named business-error node and another exhausts execution retries
- **THEN** status reports the first as a business error with its node name and the second as failed with an execution reason

### Requirement: Local development honors source-defined recovery
The local single-node development server SHALL execute the same compiled named graph, terminal nodes, checkpoints, and source-declared retry policy as distributed mode. It SHALL persist enough process state to resume from the last committed checkpoint after a restart, counting interrupted work as an execution failure; it SHALL NOT replay completed tasks. The local store SHALL NOT require PostgreSQL.

#### Scenario: Dev-server restart with retry
- **WHEN** the local server stops while a task is in progress after an earlier task was checkpointed and retry limits permit
- **THEN** restart retains the earlier task's output and schedules only unfinished work after the configured delay

#### Scenario: Dev-server restart without retry
- **WHEN** the local server restarts while an instance was running and its policy permits no retry
- **THEN** the instance is marked terminally failed with an interruption reason and its committed checkpoint remains inspectable

### Requirement: Terminal events and execution failures have distinct outcomes
When a compiled graph reaches a normal `end`, the instance SHALL complete with its typed output. Reaching a named `error` SHALL produce a business-error terminal outcome with the node's name, distinct from execution failure; `cancel` SHALL produce a cancelled outcome and `terminate` SHALL produce a terminated outcome. `error`, `cancel`, and `terminate` SHALL stop process advancement, signal in-flight sibling tasks, and SHALL NOT consume or schedule execution retries. An `end` SHALL complete only when the graph's active work has satisfied its required joins. External cancellation SHALL continue to result in cancelled status.

#### Scenario: Named business error
- **WHEN** routing reaches a named `error` node while another branch is active
- **THEN** the instance records the business-error node name, signals in-flight siblings, and does not retry

#### Scenario: Termination halts parallel work
- **WHEN** a graph reaches a `terminate` node while a sibling task is in flight
- **THEN** no successor starts, the sibling receives a cancellation request, and the instance becomes terminated

#### Scenario: Normal end needs joined work
- **WHEN** a branch finishes but another activated branch required by its join is still running
- **THEN** the instance does not complete until the join and typed `end` are reached

### Requirement: Intermediate waits resume durably without worker occupation
On reaching a `pause for` node, the runtime SHALL checkpoint a single wake instant computed from entry time plus its declared duration. On reaching a `pause until` node, it SHALL evaluate the typed timestamp once, checkpoint that instant, and treat an instant in the past as immediately eligible. The instance SHALL expose a waiting state and wake time, release any worker claim while waiting, and resume the same activation at or after eligibility without external input. After restart or retry, a checkpointed wait SHALL NOT be reevaluated or extended. Cancellation SHALL remain available while waiting.

#### Scenario: Delay survives restart
- **WHEN** an instance checkpoints a ten-minute pause and its server restarts five minutes later
- **THEN** it remains waiting for the original wake instant and continues only when that instant arrives

#### Scenario: Past timestamp
- **WHEN** `pause until` evaluates to an instant earlier than entry time
- **THEN** the instance continues without an unnecessary wait

### Requirement: Each repeated graph activation is independently checkpointed
The runtime SHALL distinguish repeated activations of the same node, concurrent multi-instance invocations, and gateway-join generations so a committed earlier visit does not suppress a later one. Task-loop bounds SHALL stop further iterations at the first exceeded limit with a `task-iteration-limit` business-error outcome; a sequential or parallel multi-instance task SHALL return results in input order and an empty input list SHALL return an empty result list. A process deadline SHALL take precedence over new work; retries SHALL resume only unfinished invocations, and cancellation SHALL stop all outstanding invocations. Parallel invocations SHALL respect the shared in-flight task limit.

#### Scenario: Back-edge visits task twice
- **WHEN** a route visits the same task twice and the first result is committed
- **THEN** the second activation runs and stores its own result without replacing or replaying the first activation

#### Scenario: Parallel multi-instance failure
- **WHEN** one of three parallel invocations fails after another was checkpointed
- **THEN** retry preserves the committed invocation and reruns only uncommitted work

#### Scenario: Task-loop bound
- **WHEN** a loop condition remains true at its configured maximum
- **THEN** the task is not invoked again and the instance ends with a `task-iteration-limit` business-error outcome

### Requirement: Process deadlines terminate all active work
For a configured process deadline, the runtime SHALL measure elapsed wall-clock time from the persisted queue time or persisted first-claim time, as selected by the source policy. Queue-origin deadlines SHALL apply even before a worker is available; first-claim deadlines SHALL begin only at the first successful claim and SHALL NOT reset on retries, pauses, handoffs, or restarts. On expiry, the instance SHALL become `business-error` with reserved terminal name `timeout`, without an authored error node or retry; no further tokens SHALL advance and in-flight tasks SHALL receive cancellation signals. Timeout SHALL be reconciled while waiting or queued as well as while running, and competing completion/cancellation/timeout transitions SHALL have a single persisted winner.

#### Scenario: Queue-origin expiry without worker
- **WHEN** a queue-origin deadline passes before an instance is claimed
- **THEN** reconciliation reports a timeout business error without running its graph

#### Scenario: First-claim duration survives takeover
- **WHEN** an instance first claimed at time T loses its worker, retries, and is reclaimed
- **THEN** its deadline remains measured from T rather than the new claim time

#### Scenario: Timeout interrupts parallel work
- **WHEN** the configured deadline expires with two tasks in flight
- **THEN** no successor is dispatched, both tasks receive cancellation requests, and the instance reports `business-error` and terminal name `timeout`

### Requirement: Custom asynchronous tasks obey graph execution semantics
The local server and distributed worker SHALL await I/O-capable custom tasks without blocking the async executor or consuming capacity beyond the configured in-flight task limit. Successful outputs SHALL be checked against their declared `.bl` output type before checkpointing or dispatching downstream nodes; task errors and invalid outputs SHALL be execution failures subject to the existing source-defined retry policy. Cancellation and deadlines SHALL prevent further graph advancement and signal in-flight tasks cooperatively, but SHALL NOT guarantee undoing external effects. A custom task whose external effect completed before its output was committed MAY run again after interruption.

#### Scenario: Concurrent custom I/O tasks
- **WHEN** two independent custom tasks are ready and capacity is available
- **THEN** both may wait on I/O concurrently without blocking unrelated task execution

#### Scenario: Invalid custom task result
- **WHEN** a custom task returns data incompatible with its declared `.bl` output type
- **THEN** the attempt fails without checkpointing that result or dispatching successors, following the process retry policy

#### Scenario: Cancel in-flight custom task
- **WHEN** cancellation is accepted while a custom task awaits external I/O
- **THEN** it is signalled and cannot commit a later result or advance the graph, even if its external effect already happened

#### Scenario: Interrupted external effect
- **WHEN** a custom task performs an external effect but its worker is lost before its result is committed
- **THEN** a policy-permitted retry may invoke that task again; exactly-once external effects are not guaranteed
