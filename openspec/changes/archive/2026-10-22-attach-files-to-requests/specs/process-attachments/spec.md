# Spec Delta

## Purpose

Allows clients to attach files to process starts and input-request replies while keeping binary content outside instance databases and available to the worker executing the process.

## ADDED Requirements

### Requirement: Attached files are persisted outside the database
For an accepted multipart process start or input-request reply, the API SHALL store each file's bytes in the configured object store before acknowledging acceptance and SHALL persist an attachment record associated with the instance and, for replies, the input request ID, containing an opaque object key, original filename, declared media type, size, and attachment order. The local and PostgreSQL instance databases SHALL NOT store the file bytes. Attachment metadata and object keys SHALL NOT be treated as caller-supplied storage locations.

#### Scenario: Attach multiple documents
- **WHEN** a client submits a valid process-start request with two files
- **THEN** the API returns an accepted instance ID only after both objects and their ordered references are durable, and the database contains references rather than their bytes

#### Scenario: Attach documents to an input reply
- **WHEN** a client supplies a valid reply with two files to an outstanding input request
- **THEN** the reply and both ordered attachment references commit together, without storing file bytes in the database

#### Scenario: JSON-only request
- **WHEN** a client starts a process or answers an input request using the existing JSON format
- **THEN** it is accepted without attachments or an object-store upload

### Requirement: Workers can retrieve committed instance attachments
A worker with access to the configured store SHALL be able to list persisted start attachments and attachments belonging to an accepted input reply separately, and read their bytes using server-managed object keys, including after a restart or a handoff to another matching worker. Attachment metadata SHALL be available without altering the process start-port JSON schema or including file bytes in instance status, checkpoints, SSE notifications, or process results.

#### Scenario: Different worker claims the instance
- **WHEN** a distributed API accepts a file-bearing instance and a separate matching worker claims it later
- **THEN** that worker can retrieve every attached file from shared storage using the instance's durable metadata

#### Scenario: Restart on one node
- **WHEN** the local server restarts with the same database and persistent attachment directory
- **THEN** its worker can retrieve both start and accepted reply attachments belonging to a previously admitted instance

#### Scenario: Reply is consumed
- **WHEN** a worker consumes an accepted input response
- **THEN** it can still identify the files belonging to that request during the instance's lifetime

### Requirement: Storage configuration is deployment-safe
A single-node server SHALL initialize a persistent local filesystem store by default, in a directory alongside its local database unless the operator configures a different persistent root, and SHALL fail startup if that directory is unusable. Every distributed API and worker role SHALL require a configured Amazon S3, Google Cloud Storage, or Azure Blob Storage location at startup, regardless of whether any incoming request contains files. Distributed roles SHALL fail startup with an actionable diagnostic when configuration is missing, invalid, or the selected store cannot be initialized; a distributed worker SHALL NOT depend on the API replica's local disk. Object keys SHALL be unique and independent of untrusted filenames, and uploads SHALL remain private to the deployment's configured storage credentials and authenticated ingress.

#### Scenario: Distributed backend configured consistently
- **WHEN** an API and worker are configured for the same cloud bucket or container and credentials permit writing and reading respectively
- **THEN** a file uploaded through the API is available to that worker without passing file bytes through PostgreSQL

#### Scenario: Missing distributed store
- **WHEN** a distributed API or worker starts without cloud object storage configured, even if clients intend to send only JSON
- **THEN** startup fails with an actionable diagnostic and the role does not serve or claim work

#### Scenario: Invalid store settings
- **WHEN** a deployment selects an unsupported backend or omits a required bucket or container, or its selected store fails initialization
- **THEN** its active role fails startup instead of accepting or claiming work with unusable attachment references

#### Scenario: Single-node default directory
- **WHEN** a single-node server starts with a writable local database and no attachment-specific settings
- **THEN** it uses a persistent directory alongside that database and can accept files after a restart

#### Scenario: Unwritable local directory
- **WHEN** a single-node server cannot write to its configured or default attachment directory
- **THEN** startup fails with a diagnostic instead of accepting JSON-only work

### Requirement: Failed or oversized uploads do not admit incomplete instances
The API SHALL enforce documented finite limits on file count, individual file size, total upload size, and multipart parsing on both endpoints; it SHALL reject invalid or oversized requests without admitting an instance or accepting a response. If an upload or database commit fails, or a competing answer, cancellation, or timeout prevents accepting the reply, the API SHALL report failure rather than success and attempt to remove any objects written for that request. A worker SHALL never see a committed attachment reference to an upload known to have failed.

#### Scenario: Oversized attachment
- **WHEN** a client uploads a file exceeding the configured limit
- **THEN** the API rejects the request and no instance is admitted

#### Scenario: Partial upload or database failure
- **WHEN** storing one of several files fails, or storing attachment references fails after uploading files
- **THEN** the API does not acknowledge the instance or reply as accepted, and best-effort cleanup removes the written objects

#### Scenario: Input reply loses a race
- **WHEN** an upload completes but another reply, cancellation, or deadline has already prevented acceptance of that input response
- **THEN** the reply and its attachment records are not committed and the uploaded objects are cleaned up best-effort
