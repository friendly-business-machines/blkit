# Spec Delta

## ADDED Requirements

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
