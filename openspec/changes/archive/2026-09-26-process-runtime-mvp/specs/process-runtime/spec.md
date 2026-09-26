# Spec Delta

## Purpose

Defines the single-node execution and control contract for versioned, compiled business processes, including durable instance state, graph task scheduling, cancellation, and a development REST API.

## ADDED Requirements

### Requirement: Registered processes have stable identities
The runtime SHALL register compiler-emitted `.bl` process graphs by namespace, version, and name, and SHALL reject duplicate identities. It SHALL consume the compiled graph rather than construct the process map from separately registered tasks. A start request SHALL identify exactly one registered process and provide an input valid for its declared type.

#### Scenario: Start a registered process
- **WHEN** a client starts `orders` version `1.0` process `approve` with valid input
- **THEN** the runtime creates a new instance with a unique ID associated with that exact process identity

#### Scenario: Unknown process or invalid input
- **WHEN** a client names an unregistered identity or supplies input incompatible with its declared input type
- **THEN** no instance starts and the client receives an actionable error

### Requirement: Each instance has a persisted lifecycle
The runtime SHALL persist an instance's identity, process identity, input, status, and available terminal result or failure. Status SHALL distinguish pending, running, cancelling, completed, failed, and cancelled instances. A status lookup SHALL return the recorded state and terminal outcome when one exists.

#### Scenario: Observe a completed process
- **WHEN** a registered compiled process finishes successfully
- **THEN** its status is completed and its typed output is available to the client

#### Scenario: Inspect after restart
- **WHEN** the server is restarted with its existing state store
- **THEN** previously recorded instance identities, inputs, statuses, and terminal outcomes remain available

#### Scenario: Interrupted execution at restart
- **WHEN** the server restarts after stopping while an instance was pending, running, or cancelling
- **THEN** that instance is marked failed with an interruption reason instead of being automatically replayed

### Requirement: Ready tasks execute with bounded concurrency
The runtime SHALL execute the compiler-emitted graph by advancing `.bl` task nodes only when their typed input and upstream control-flow dependencies are available. It SHALL evaluate compiled AND, OR, and XOR routes using available process input and task outputs, propagate only activated branches, and allow independent ready tasks, including tasks from separate instances, to run concurrently subject to a server-wide configurable in-flight task limit. A completed instance SHALL expose the declared process output after its graph completes.

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
- **WHEN** a task fails
- **THEN** the instance becomes failed, no successor tasks are dispatched, and in-flight sibling tasks receive cancellation requests

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
The single-node development binary SHALL provide JSON endpoints to start a registered process, inspect an instance, and request cancellation. It SHALL distinguish successful acceptance, invalid input, unknown identities, and conflicting terminal states with appropriate HTTP responses, and SHALL bind locally by default.

#### Scenario: Start and inspect via HTTP
- **WHEN** a client POSTs valid input to `/processes/{namespace}/{version}/{name}/instances` and then GETs `/instances/{id}`
- **THEN** the start response identifies the instance and the GET response reports its persisted state and result when completed

#### Scenario: Cancel via HTTP
- **WHEN** a client POSTs to `/instances/{id}/cancel` while it is active
- **THEN** the response acknowledges cancellation and subsequent GETs reflect cancelling or cancelled status

#### Scenario: Invalid and missing requests
- **WHEN** input is malformed or an instance ID does not exist
- **THEN** the API returns a client error without starting or modifying an instance
