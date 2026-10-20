# distributed-execution Specification

## Purpose

Defines PostgreSQL-backed distributed execution of compiled, versioned process instances by multiple worker binaries, with exclusive per-instance claims, durable recovery, and graceful version-aware rollouts.

## Requirements

### Requirement: Workers advertise compiled capabilities and remain independently live
Each running worker-binary process SHALL register a unique worker identity, advertise the namespace/version/process identities linked into that binary, and renew a heartbeat while live. A worker SHALL execute multiple instances concurrently up to a configurable local task limit; while eligible matching instances remain queued and the worker is not draining, it SHALL claim replacement work when capacity becomes available without waiting for unrelated running instances to finish. It SHALL NOT exceed its configured local task limit or claim instances whose exact process identity it does not advertise. The system SHALL NOT require `.bl` compilation by a running worker.

#### Scenario: Matching version is claimable
- **WHEN** a worker advertises `orders/1.0/decide` and an instance of that exact identity is queued
- **THEN** that worker may claim it, subject to capacity

#### Scenario: Old version remains queued
- **WHEN** only workers advertising `orders/2.0/decide` are live and an `orders/1.0/decide` instance is queued
- **THEN** the old-version instance remains visible and unclaimed until a capable worker is available

#### Scenario: One worker runs several instances
- **WHEN** several eligible instances are queued and a worker has spare capacity
- **THEN** it can run more than one concurrently without assigning any instance to two current owners

#### Scenario: Freed capacity is reused during a long-running instance
- **WHEN** a worker is executing a long-running instance, another claimed instance finishes, and eligible matching work is queued
- **THEN** the worker claims replacement work while the long-running instance is still active, without exceeding its configured task limit

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
A worker asked to drain SHALL stop claiming new instances, including replacement work as capacity becomes available, continue heartbeating and executing its current claims until each finishes or is released for a scheduled retry, and then unregister/exit. Removing an older process version SHALL NOT route its instances to a worker containing only a newer version; instances without a capable worker SHALL remain queued and inspectable.

#### Scenario: Version rollout
- **WHEN** old-version workers drain while new-version workers start
- **THEN** the old workers finish or release their owned attempts and the new workers claim only matching new-version instances

#### Scenario: Pending old-version work
- **WHEN** an old-version instance remains queued after the last capable worker exits
- **THEN** it remains queued for an old-version worker and is not silently executed as a new version

#### Scenario: Drain during active work
- **WHEN** a worker receives a drain request while it owns instances and one completes with eligible backlog still queued
- **THEN** it does not claim replacement work, renews and finishes or releases its remaining claims, and unregisters after the last claim is released

### Requirement: Distributed waits and deadlines are persisted and claim-safe
PostgreSQL SHALL persist each waiting instance's wake instant, process deadline origin and deadline instant when known, and first-claim instant. Waiting instances SHALL release their worker claim and SHALL NOT be claimable until wake eligibility; eligible instances SHALL be claimable only by workers advertising their exact compiled process identity. The control and worker reconciliation paths SHALL enforce queue-origin deadlines even when no capable worker is present and SHALL enforce first-claim deadlines after the first successful claim. A timeout or cancellation SHALL fence a former owner from advancing the graph, and its terminal outcome SHALL NOT become a retryable execution failure.

#### Scenario: Wait through worker shutdown
- **WHEN** a worker checkpoints a wait and stops before wake time
- **THEN** the same instance remains waiting and an eligible matching worker can claim it at or after the persisted wake instant

#### Scenario: No matching worker during queue-origin deadline
- **WHEN** a queued old-version instance exceeds its queue-origin deadline while only new-version workers exist
- **THEN** control-plane reconciliation records its timeout without running it on a new-version worker

#### Scenario: Stale completion after timeout
- **WHEN** a previously claimed worker submits a completion after the deadline has been committed
- **THEN** the completion cannot overwrite the timeout or dispatch a successor

### Requirement: Repeated activation recovery remains fenced
Distributed checkpoints SHALL persist separate identities and progress for repeated node visits, join generations, and multi-instance items. Checkpoint and completion writes SHALL remain fenced by the current unexpired lease and claim generation. Completed invocations SHALL not be rerun after worker loss, while uncommitted invocations remain at-least-once.

#### Scenario: Interrupted multi-instance batch
- **WHEN** a parallel batch commits two item results and its owner loses the claim before a third commits
- **THEN** the new owner resumes only unfinished items and returns the final result in original input order

### Requirement: Remote roles use bounded PostgreSQL connection pools
Each PostgreSQL-backed API and worker role SHALL serve concurrent store operations through a pool with a positive, bounded maximum rather than requiring all operations of that role to wait for one shared client. The default maximum SHALL be four connections per role; a Rust consumer SHALL be able to set an explicit maximum while retaining the existing `PostgresStore::connect` call shape. Generated remote executables SHALL accept independent optional `BLKIT_API_DB_POOL_MAX` and `BLKIT_WORKER_DB_POOL_MAX` positive-integer environment settings for their respective roles, without changing existing positional arguments. API-only and split API binaries SHALL use the API budget, worker-only and split worker binaries SHALL use the worker budget, and a combined remote API/worker binary SHALL use separate API and worker budgets. A role SHALL NOT maintain more simultaneously managed PostgreSQL sessions than its maximum; it SHALL NOT require all allowed connections to be open at startup. Local-persistence and reusable-crate targets SHALL NOT gain a PostgreSQL dependency solely for pooling.

#### Scenario: Concurrent API calls
- **WHEN** an API role has a maximum of two and one store operation is waiting on PostgreSQL while another independent operation can complete on a different session
- **THEN** the second operation can complete without waiting for the first session, and the API role uses no more than two sessions

#### Scenario: Concurrent worker persistence
- **WHEN** a worker role has a maximum of two and one store operation occupies a PostgreSQL session while an independent worker store operation can complete on another
- **THEN** the independent operation can complete without waiting for the first session, and the worker role uses no more than two sessions

#### Scenario: Combined roles under load
- **WHEN** a generated combined remote binary runs concurrent API requests and worker persistence with distinct configured API and worker maxima
- **THEN** the roles draw from independent pools bounded by their own maxima; exhausting the worker budget does not consume API connections

#### Scenario: Defaults and overrides
- **WHEN** a generated remote binary starts without pool settings, or starts with positive integer settings for its active roles
- **THEN** each active role uses a default maximum of four or its configured maximum, independently of the worker task limit and CPU count

### Requirement: PostgreSQL pool failures preserve distributed safety
Generated remote executables SHALL reject a nonpositive or malformed pool limit for any active role with a diagnostic that identifies the setting. PostgreSQL-backed store initialization SHALL still establish a usable connection and perform required schema setup before reporting startup success. Connection checkout SHALL have a bounded wait and SHALL report an error if capacity or a working connection is unavailable; a failed or discarded connection SHALL NOT permanently consume pool capacity. Pooling SHALL preserve single-connection transaction ownership and the existing claim/drain serialization, lease renewal, checkpoint fencing, cancellation, and recovery semantics; it SHALL NOT acknowledge a database operation that was not committed.

#### Scenario: Invalid configuration or unavailable startup database
- **WHEN** an active remote role is configured with zero or an invalid maximum, or the database is unavailable during store initialization
- **THEN** startup fails visibly instead of running with an unlimited or uninitialized pool

#### Scenario: Saturated or disconnected pool
- **WHEN** all sessions in a role's pool remain checked out beyond its checkout wait or a session is lost
- **THEN** a timed-out checkout reports an error rather than waiting indefinitely or pretending it committed; a lost session is replaced when possible or the operation reports an error, without permanently losing pool capacity

#### Scenario: Claim and drain on different sessions
- **WHEN** a claim and drain overlap on different PostgreSQL sessions for the same worker
- **THEN** a committed drain does not permit a later claim, while already owned claims remain fenced and can renew and finish or release under the existing drain rules

#### Scenario: Contended active worker
- **WHEN** worker persistence and maintenance compete for the bounded worker pool while claims are active
- **THEN** the worker continues to enforce its task limit and existing lease/generation safety rather than double-claiming or committing a stale update

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
