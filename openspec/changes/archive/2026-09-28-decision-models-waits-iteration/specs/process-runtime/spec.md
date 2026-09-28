# Spec Delta

## ADDED Requirements

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
