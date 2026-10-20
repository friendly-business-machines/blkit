# Spec Delta

## ADDED Requirements

### Requirement: Input requests pause graph activations durably
On reaching an input-request task node, the runtime SHALL checkpoint a distinct request identity, prompt, response-port names and types, and its graph activation before exposing the request to clients. Each response SHALL advance only its corresponding activation from the saved graph checkpoint, preserving its existing state. Other ready or in-flight branches MAY continue. When no work can run until additional input arrives, the instance SHALL report `waiting-for-input`, retain its outstanding requests without a clock wake, and release worker capacity. Requests in child processes SHALL retain the parent instance ID and independently scoped activations. On restart, the runtime SHALL restore the saved graph state, outstanding requests, and accepted responses without creating duplicate requests.

#### Scenario: Await and resume
- **WHEN** execution reaches a request with no other runnable work and the client subsequently supplies a valid response
- **THEN** status reports `waiting-for-input` with the request, the worker is not occupied during the wait, and a worker resumes the same instance using the committed response

#### Scenario: Two outstanding activations
- **WHEN** parallel branches or repeated visits reach two input requests, including requests in child processes
- **THEN** each has a different request ID, either may be answered first, and its response cannot satisfy the other

#### Scenario: Other branches keep running
- **WHEN** a request is reached while an independent task is still running
- **THEN** the request becomes visible without cancelling that task; a response received before the task finishes remains available for the graph's next safe advancement

#### Scenario: Restart while awaiting input
- **WHEN** a local server or distributed worker stops and restarts after a request was persisted
- **THEN** the same instance and request IDs remain inspectable, and a later response continues the instance from its persisted checkpoint

### Requirement: Clients can inspect and answer outstanding requests
`GET /instances/{id}` SHALL include outstanding request IDs, prompt, declared response fields and types, and current status. Both local and distributed APIs SHALL accept a JSON object at `POST /instances/{id}/requests/{request_id}/response` only when its keys match that request's declared response ports exactly and its values satisfy their `.bl` types. A distributed API SHALL validate from persisted portable request metadata without loading compiled process code; the worker SHALL also validate before consuming a response. Invalid responses SHALL leave the request outstanding and report a client error. The API SHALL atomically accept at most one response per request and SHALL NOT acknowledge an uncommitted response. An accepted response SHALL remain durable and eventually make an otherwise input-waiting instance eligible for a matching worker. A cancelled, timed-out, or terminal instance SHALL NOT be resumed by a late response. The same HTTP contract SHALL be available from local and distributed servers.

#### Scenario: Submit a valid response
- **WHEN** a client POSTs `{"approved":true}` to an outstanding request with `approved: Bool`
- **THEN** the API accepts it once, persists it, and makes its activation eligible to proceed

#### Scenario: Invalid or duplicate reply
- **WHEN** a client POSTs a missing, extra, or mistyped field, or posts again to a request that already accepted a response
- **THEN** invalid data is rejected without changing the request; an already answered request cannot be overwritten and returns a conflict

#### Scenario: Unknown or stale request
- **WHEN** a client names a nonexistent instance or request, or answers after cancellation, timeout, or completion
- **THEN** the API returns a not-found or conflict response as appropriate and no further graph work is enabled

#### Scenario: Reply races with cancellation
- **WHEN** reply and cancellation or deadline expiration compete for one instance
- **THEN** a single persisted order decides the outcome; once cancellation or timeout wins, the reply cannot advance the graph

### Requirement: Clients can subscribe to current instance state over SSE
Both APIs SHALL expose an SSE stream for an instance, deliver a current persisted snapshot after the stream becomes active, and emit change hints for newly outstanding or resolved input requests and terminal/cancelled outcomes. A stream event SHALL identify the instance and current request state or cause the client to fetch it from `GET /instances/{id}`; events SHALL NOT be the authoritative copy of a request or response. A reconnecting client SHALL receive a fresh snapshot rather than require replay of missed events. The stream SHALL close or report an error for an unknown instance and SHALL NOT expose another instance's data. GET SHALL remain available independently of SSE; clients SHALL NOT need continuous HTTP status polling to observe requests while a healthy stream is connected. Remote clients SHALL use operator-managed authenticated TLS ingress for GET, SSE, and POST, including instance-level access control; the binaries SHALL continue to bind locally by default.

#### Scenario: Initial subscription and reconnect
- **WHEN** a client connects while a request is already outstanding or reconnects after missing an event
- **THEN** it learns the current persisted request state without depending on notification history

#### Scenario: Live request
- **WHEN** an input request is committed while an SSE client watches that instance
- **THEN** a healthy connected client receives an event and can discover the request without continuous status polling

#### Scenario: Unrelated instance
- **WHEN** an event changes a different instance from the one being watched
- **THEN** the client's SSE stream does not disclose that other instance's request or response
