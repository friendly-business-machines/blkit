# Spec Delta

## MODIFIED Requirements

### Requirement: Registered processes have stable identities
The runtime SHALL register compiler-emitted `.bl` process graphs by namespace, version, and name, and SHALL reject duplicate identities. It SHALL consume the compiled graph rather than construct the process map from separately registered tasks. A local start request SHALL identify exactly one registered process and provide a JSON object whose keys correspond exactly to the process start event's declared output ports and whose values satisfy their declared types; a one-port start event SHALL still require an object. A distributed API SHALL instead accept syntactically valid JSON only after finding a recent, non-draining worker advertising the exact namespace, version, and process identity in PostgreSQL. It SHALL queue the input without linking executable process definitions or performing typed port validation; a worker claiming that instance SHALL validate the input and initialize its compiled-graph checkpoint before dispatching any tasks.

#### Scenario: Start a registered process
- **WHEN** a client starts `orders` version `1.0` process `approve` with valid input such as `{"amount": 3}` for a start event with `output amount: Number;`
- **THEN** the runtime creates a new instance with a unique ID associated with that exact process identity

#### Scenario: Unknown process or invalid input
- **WHEN** a client names an unregistered identity, supplies a scalar instead of an object, omits/adds a port key, or supplies a value incompatible with the port type to a local server
- **THEN** no instance starts and the client receives an actionable error

#### Scenario: No capable distributed worker
- **WHEN** a client starts an identity for which PostgreSQL has no non-draining worker with a sufficiently recent heartbeat advertising that exact identity
- **THEN** the distributed API reports unavailability and creates no instance

#### Scenario: Invalid input admitted to a distributed worker
- **WHEN** a live matching worker was advertised at admission but the client supplies a well-formed JSON value incompatible with the process start ports
- **THEN** the distributed API returns an instance ID and the claiming worker records a terminal failed instance with an actionable input error, without dispatching process tasks or retrying invalid input

#### Scenario: Worker vanishes after admission
- **WHEN** an advertised worker stops after the distributed API accepts a request
- **THEN** the instance remains queued and inspectable for a future worker advertising the same identity; admission does not guarantee immediate execution

### Requirement: Development server exposes process control over REST
The single-node development binary and distributed REST service SHALL provide JSON endpoints to start a process, inspect an instance, and request cancellation. The local server SHALL distinguish successful acceptance, invalid input, unknown identities, conflicting terminal states, waiting retries, completed results, modeled business errors, and execution failures with appropriate HTTP responses and status fields. The distributed API SHALL reject malformed JSON before creating an instance, report absence of a live capable worker as unavailable without creating an instance, and report typed input failures through the accepted instance's persisted status after worker validation. Both binaries SHALL bind locally by default; distributed remote access SHALL require an operator-managed authenticated ingress.

#### Scenario: Start and inspect via HTTP
- **WHEN** a client POSTs valid input to `/processes/{namespace}/{version}/{name}/instances` and then GETs `/instances/{id}`
- **THEN** the start response identifies the instance and the GET response reports its persisted state and result when completed

#### Scenario: Cancel via HTTP
- **WHEN** a client POSTs to `/instances/{id}/cancel` while it is active, queued, or waiting for retry
- **THEN** the response acknowledges cancellation, advances no more work, and subsequent GETs report cancelling or cancelled status

#### Scenario: Invalid and missing requests
- **WHEN** the client POSTs malformed JSON, the local server receives a typed-invalid start input, or an instance ID does not exist
- **THEN** the API returns a client error without starting or modifying an instance

#### Scenario: Distributed typed-input error
- **WHEN** a distributed API accepts syntactically valid JSON for an advertised process but the worker finds its start ports or types invalid
- **THEN** the API has returned an instance ID and subsequent GETs show a failed instance with the validation reason

#### Scenario: Distinct business and execution errors
- **WHEN** one instance reaches a named business-error node and another exhausts execution retries
- **THEN** status reports the first as a business error with its node name and the second as failed with an execution reason

## ADDED Requirements

### Requirement: A local worker polls durable pending work in the API process
A local API+worker binary SHALL validate process identity and typed input before admitting an instance, persist it as pending work, and run an in-process worker that polls and claims eligible persisted instances rather than starting execution directly from the HTTP request. The worker SHALL enforce the configured in-flight task concurrency limit, checkpoint progress before dependent work, honor cancellation, retries, waits, and deadlines, and recover pending or interrupted work according to existing local recovery rules after restart. It SHALL use the local store without a separate worker executable or PostgreSQL.

#### Scenario: Request queues work locally
- **WHEN** a client starts a valid process through a local API+worker binary
- **THEN** the request returns an instance ID after durable admission and an in-process worker claims and executes it asynchronously within the concurrency limit

#### Scenario: Local validation remains immediate
- **WHEN** a local API receives a process identity or typed input that is invalid
- **THEN** it returns an actionable error without creating an instance or queue entry

#### Scenario: Local worker restarts
- **WHEN** the local API+worker process stops with pending or in-progress work and is restarted against the same local store
- **THEN** pending work remains claimable and in-progress work resumes only according to existing checkpoint and retry rules without replaying completed tasks

#### Scenario: Cancel before claim
- **WHEN** a locally queued instance is cancelled before its worker claims it
- **THEN** no task starts and the instance remains cancelled
