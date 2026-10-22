# Proposal

## Why

Clients currently can start a process or answer an outstanding input request only with JSON, so they cannot submit supporting documents or images alongside either request. Storing binary content in the instance database would bloat the durable queue and make it impractical for a distributed worker to access large files.

## What Changes

- Accept one or more file attachments together with the existing JSON input on both the process-start endpoint and responses to outstanding `input_request` tasks, without changing JSON-only clients or `.bl` port validation.
- Store file bytes in an operator-selected object store and persist only attachment metadata and opaque object keys in the local or PostgreSQL database, associated with the instance and (for replies) the request ID. Make attachments available to the worker handling that instance or reply.
- Support a persistent local filesystem store for a single-node deployment and require shared Amazon S3, Google Cloud Storage, or Azure Blob Storage for every distributed API and worker, using the async Rust `object_store` crate.
- **BREAKING:** Distributed API and worker processes must have a configured, usable cloud object store at startup, including when serving JSON-only requests. Single-node binaries use a persistent local directory by default. Add bounded upload handling, failure cleanup, and deployment guidance. Direct client-to-cloud uploads and a new `.bl` file type are not part of this change.

## Capabilities

### New Capabilities

- `process-attachments`: Multipart admission for starts and input replies, durable attachment references, worker retrieval, storage configuration, and lifecycle safety.

### Modified Capabilities

- `process-runtime`: Extend process starts and input-request responses to accept multipart JSON plus attachments while retaining existing JSON-only behavior and validation.
- `project-build`: Generated API and worker targets must carry only the appropriate object-store capabilities and storage configuration.

## Impact

Affected: `blkit-core` HTTP handlers, local and PostgreSQL stores, runtime and distributed worker interfaces, generated project binaries/manifests, dev harness, tests, and README. Adds `object_store` (Apache Arrow Rust); requires a shared cloud container/bucket and credentials for distributed deployments, or a persistent writable directory on a single node. JSON request and response formats remain unchanged when no files are attached.
