# Spec Delta

## MODIFIED Requirements

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
