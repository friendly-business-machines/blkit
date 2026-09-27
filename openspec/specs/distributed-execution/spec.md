# distributed-execution Specification

## Purpose

Defines PostgreSQL-backed distributed execution of compiled, versioned process instances by multiple worker binaries, with exclusive per-instance claims, durable recovery, and graceful version-aware rollouts.

## Requirements

### Requirement: Workers advertise compiled capabilities and remain independently live
Each running worker-binary process SHALL register a unique worker identity, advertise the namespace/version/process identities linked into that binary, and renew a heartbeat while live. A worker SHALL execute multiple instances concurrently up to a configurable local task limit; it SHALL claim only instances whose exact process identity it advertises. The system SHALL NOT require `.bl` compilation by a running worker.

#### Scenario: Matching version is claimable
- **WHEN** a worker advertises `orders/1.0/decide` and an instance of that exact identity is queued
- **THEN** that worker may claim it, subject to capacity

#### Scenario: Old version remains queued
- **WHEN** only workers advertising `orders/2.0/decide` are live and an `orders/1.0/decide` instance is queued
- **THEN** the old-version instance remains visible and unclaimed until a capable worker is available

#### Scenario: One worker runs several instances
- **WHEN** several eligible instances are queued and a worker has spare capacity
- **THEN** it can run more than one concurrently without assigning any instance to two current owners

### Requirement: PostgreSQL holds durable queued and claimed instance records
The distributed mode SHALL use a shared PostgreSQL store for instances, queued work, attempt/retry metadata, process checkpoints, and worker claims. Claiming SHALL retain the instance record and atomically record an owner, claim generation, and expiring lease; it SHALL NOT remove the instance from the durable queue. At most one current owner SHALL advance an instance at a time. A separate message broker SHALL NOT be required.

#### Scenario: Exclusive claim
- **WHEN** two eligible workers attempt to claim the same queued instance concurrently
- **THEN** only one receives the current claim, while the other can claim different eligible work

#### Scenario: Claim persists
- **WHEN** an instance is claimed and its worker stops unexpectedly
- **THEN** its input, claim state, and checkpoint remain in PostgreSQL for recovery

### Requirement: Lost claims are reconciled and stale owners fenced
An expired owner lease SHALL be detected without cooperation from the lost worker and recorded as a failed execution attempt. If retry limits permit, the instance SHALL become claimable after its scheduled retry delay; otherwise it SHALL become terminally failed. Worker-owned process-state updates SHALL succeed only while that worker holds the current valid claim. A delayed former worker SHALL NOT overwrite a newer claim, checkpoint, cancellation, or terminal state.

#### Scenario: Lost worker with retries remaining
- **WHEN** a worker lease expires while it owns an instance with retry budget remaining
- **THEN** the failed attempt is recorded, the last committed checkpoint is retained, and a matching live worker can claim the scheduled retry

#### Scenario: Lost worker without retries
- **WHEN** a worker lease expires and no retry is allowed
- **THEN** the instance becomes terminally failed with a worker-loss reason

#### Scenario: Stale worker result
- **WHEN** an old worker submits a task result after its lease expired and another generation owns the instance
- **THEN** that result cannot change the instance or its checkpoint

### Requirement: Completed work survives worker loss and task errors
A worker's process executor SHALL durably record completed task results and process routing/activation state before dispatching dependent work. Checkpoints SHALL identify completed nodes, typed outputs, selected routes, active branches, and join progress sufficient to resume the same compiled graph. Independent concurrent completions SHALL be serialized so a task already committed before another task's failure is not repeated. In-progress/uncommitted work MAY run again; external effects are not guaranteed exactly once.

#### Scenario: Parallel completion before error
- **WHEN** B and C run concurrently, B's output is checkpointed, and C encounters an execution error
- **THEN** a retry restores B's completion/output and reruns C without rerunning B

#### Scenario: Failure before checkpoint
- **WHEN** a task finishes but its completion was not committed before its worker was lost
- **THEN** a retry may execute that task again

#### Scenario: Process routing survives takeover
- **WHEN** an OR gateway selected two branches and one completed before owner loss
- **THEN** a new owner restores the selected branches and completed output without reevaluating a different route or waiting for inactive branches

### Requirement: Distributed retries obey the source-declared limits
Execution errors from tasks or process execution and loss of an owning worker SHALL fail the current attempt, stop further advancement, signal in-flight sibling tasks when the owner is live, and schedule retry from the latest committed checkpoint when permitted. Retry count means additional attempts beyond the initial run; `retry for` begins on the first execution failure, and both time and attempt limits SHALL be enforced. The first retry SHALL wait at least `retry delay`, and later waits SHALL grow exponentially. An intentional terminal `error`, `cancel`, or `terminate` SHALL NOT retry.

#### Scenario: Recover execution error
- **WHEN** a task reports an execution error with a retry permitted
- **THEN** the instance remains nonterminal until its next eligible attempt, preserving committed results

#### Scenario: Retry budget exhausted
- **WHEN** either the additional-attempt limit or retry-time window prevents another attempt
- **THEN** the instance is terminally failed with the last execution-failure reason

#### Scenario: Business error does not retry
- **WHEN** the graph reaches a named `error` node
- **THEN** the instance records that business-error outcome without consuming or scheduling a retry

### Requirement: Workers drain without abandoning old versions
A worker asked to drain SHALL stop claiming new instances, continue heartbeating and executing its current claims until each finishes or is released for a scheduled retry, and then unregister/exit. Removing an older process version SHALL NOT route its instances to a worker containing only a newer version; instances without a capable worker SHALL remain queued and inspectable.

#### Scenario: Version rollout
- **WHEN** old-version workers drain while new-version workers start
- **THEN** the old workers finish or release their owned attempts and the new workers claim only matching new-version instances

#### Scenario: Pending old-version work
- **WHEN** an old-version instance remains queued after the last capable worker exits
- **THEN** it remains queued for an old-version worker and is not silently executed as a new version
