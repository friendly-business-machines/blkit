# Spec Delta

## ADDED Requirements

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
