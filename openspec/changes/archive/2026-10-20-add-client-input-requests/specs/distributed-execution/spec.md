# Spec Delta

## ADDED Requirements

### Requirement: Distributed input responses retain claim and deadline safety
PostgreSQL SHALL persist requests, portable response type metadata, replies, and matching graph checkpoints so an unowned input-waiting instance remains inspectable and is not claimed until a valid response makes work eligible. A worker-owned request/checkpoint transition SHALL be fenced by the current lease and generation. A concurrent response SHALL NOT be lost by a worker's later checkpoint; acceptance and resumption SHALL be serialized with cancellation, deadlines, and terminal transitions. Resuming after an input wait SHALL NOT count as an execution failure or consume a retry merely because the worker released its claim; only workers advertising the same process identity MAY resume it. An instance with parallel runnable work MAY remain claimed while a request is outstanding.

#### Scenario: Lost worker and submitted response
- **WHEN** a worker checkpoints an input request and releases its claim, a client responds, and another capable worker claims the instance
- **THEN** the second worker sees the response and prior committed progress, with no spurious retry or duplicate task result

#### Scenario: Stale owner or competing response
- **WHEN** an old owner tries to checkpoint after losing its lease or two API replicas concurrently submit different replies
- **THEN** the old owner cannot erase a persisted response and at most one client response wins

#### Scenario: No capable worker
- **WHEN** a client answers an input-waiting instance but no worker advertising the exact process identity is available
- **THEN** the response remains durable and the instance waits for a matching worker, subject to its original deadline

### Requirement: Interested API replicas receive selective wake-up hints
For remote SSE streams, an API replica SHALL have a distinct PostgreSQL notification channel and SHALL register only its actively watched instance IDs in shared, expiring interest records. A committed change to an outstanding request or to a watched instance's terminal/cancelled state SHALL notify only currently registered interested replicas with a small instance-ID hint, without requiring the worker to know which API replicas or clients are connected. Unrelated task checkpoints and unwatched instance changes SHALL NOT emit notifications. A notification SHALL NOT substitute for persisted state and SHALL NOT disclose request payloads. Replica interests SHALL be cleaned up when no clients remain or after a replica stops renewing them.

#### Scenario: Two interested replicas
- **WHEN** replicas A and B watch an instance and replica C does not, and a worker commits a new input request
- **THEN** only A and B receive a hint and can send SSE events to their own watching clients

#### Scenario: No watchers or ordinary checkpoint
- **WHEN** no clients watch an instance, or an unrelated task checkpoint advances
- **THEN** no input-request notification is produced for that change

#### Scenario: Replica dies
- **WHEN** a replica exits without deleting its interest records
- **THEN** its registrations expire and cease to be notification targets without affecting the instance or other replicas

### Requirement: Listener recovery repairs missed notifications
Each distributed API replica SHALL establish and commit its LISTEN registration before registering interests and reading initial instance state. Its listener SHALL not hold a long-running transaction while awaiting notifications. On loss of the listener connection, the replica SHALL re-establish LISTEN and resynchronize all instances with locally connected SSE clients from durable state before relying on future hints; clients who stayed connected SHALL also see any state missed during the outage. Repeated, coalesced, or out-of-order hint handling SHALL not produce false request acceptance or duplicate graph progress. Notification queue saturation and connection errors SHALL be reported rather than silently acknowledging uncommitted changes. The listener's dedicated connection SHALL be accounted for separately from the bounded API request pool.

#### Scenario: Change while listener is down
- **WHEN** an input request commits while a replica's PostgreSQL listener is disconnected but its SSE clients remain connected
- **THEN** after LISTEN is restored the replica resynchronizes their instances and delivers the current request state without relying on replay of that notification

#### Scenario: Registration race
- **WHEN** an instance changes while an SSE client is connecting
- **THEN** the client sees that change in its initial snapshot or through a subsequent hint; it cannot remain unaware solely because registration raced with the commit

#### Scenario: Rollback and notification failure
- **WHEN** a request-state transaction rolls back or the database cannot commit its notification-bearing transaction
- **THEN** neither a successful request-state change nor a success acknowledgment is reported for that transaction
