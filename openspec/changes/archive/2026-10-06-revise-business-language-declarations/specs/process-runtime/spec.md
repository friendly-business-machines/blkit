# Spec Delta

## MODIFIED Requirements

### Requirement: Registered processes have stable identities
The runtime SHALL register compiler-emitted `.bl` process graphs by namespace, version, and name, and SHALL reject duplicate identities. It SHALL consume the compiled graph rather than construct the process map from separately registered tasks. A start request SHALL identify exactly one registered process and provide a JSON object whose keys correspond exactly to the process start event's declared output ports and whose values satisfy their declared types. A one-port start event SHALL still require an object.

#### Scenario: Start a registered process
- **WHEN** a client starts `orders` version `1.0` process `approve` with valid input such as `{"amount": 3}` for a start event with `output amount: Number;`
- **THEN** the runtime creates a new instance with a unique ID associated with that exact process identity

#### Scenario: Unknown process or invalid input
- **WHEN** a client names an unregistered identity, supplies a scalar instead of an object, omits/adds a port key, or supplies a value incompatible with the port type
- **THEN** no instance starts and the client receives an actionable error

### Requirement: Ready tasks execute with bounded concurrency
The runtime SHALL execute the compiler-emitted explicit graph by advancing named `.bl` decision-task nodes only when all typed inputs bound on the activated route and all upstream control-flow dependencies are available. It SHALL evaluate compiled AND, OR, and XOR routes using start-event values and committed task outputs, propagate only activated branches, and allow independent ready tasks, including tasks from separate instances, to run concurrently subject to a configurable in-flight task limit. A completed task's output and the process's routing progress SHALL be committed as a process-state checkpoint before successor dispatch. A completed instance reaching a typed normal end event SHALL expose its bound result: the direct value for one input port, or a JSON object keyed by input-port name for multiple ports.

#### Scenario: Independent tasks run together
- **WHEN** two independent decision tasks become ready and capacity is available
- **THEN** both can execute without waiting for the other to finish

#### Scenario: Dependencies and capacity gate dispatch
- **WHEN** a task has an unfinished activated flow prerequisite, an unavailable required binding, or the shared in-flight limit is reached
- **THEN** it waits without exceeding the limit and runs only if its instance is still active when ready

#### Scenario: Conditional routes and joins
- **WHEN** an XOR gateway selects one branch or an OR gateway selects several branches
- **THEN** only selected branches run, and their corresponding join waits for the activated incoming branches before advancing

#### Scenario: Task failure
- **WHEN** a decision task reports an execution error
- **THEN** no successors are dispatched, in-flight siblings receive cancellation requests, and the attempt either waits for a policy-permitted checkpointed retry or becomes terminally failed

#### Scenario: Committed sibling output
- **WHEN** one parallel task's result commits before another task fails
- **THEN** the first task's output remains in the checkpoint and that task is not rerun on retry

#### Scenario: Multi-port normal result
- **WHEN** an end event has `input total: Number;` and `input label: String;` and both are bound on the activated route
- **THEN** the completed result is a JSON object with `total` and `label` keys and their bound values

#### Scenario: One-port normal result
- **WHEN** an end event has only `input total: Number;` and receives `3`
- **THEN** the completed result is the JSON number `3`, not an object
