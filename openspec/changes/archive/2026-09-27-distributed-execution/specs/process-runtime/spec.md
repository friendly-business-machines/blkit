# Spec Delta

## ADDED Requirements

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

## MODIFIED Requirements

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
