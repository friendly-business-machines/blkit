# Spec Delta

## ADDED Requirements

### Requirement: Process starts accept optional multipart attachments
The existing process-start route SHALL continue to accept JSON-only requests with unchanged input validation and response shape. It SHALL additionally accept multipart requests containing exactly one JSON `input` part and one or more `files` parts; the JSON part SHALL obey the same process identity and typed start-port rules as an ordinary JSON request. Files SHALL be supplementary attachments, not extra `.bl` input ports. The local API SHALL reject invalid typed input before admitting the instance; the distributed API SHALL retain its existing worker-side typed validation after accepting syntactically valid JSON. A malformed multipart request or missing JSON input SHALL not admit an instance.

#### Scenario: Start with input and two files
- **WHEN** a client POSTs multipart data with `input` containing `{"amount":3}` and two `files` parts to a registered process
- **THEN** the instance is accepted with the same JSON input and two separately stored attachments

#### Scenario: Existing JSON client
- **WHEN** a client POSTs `{"amount":3}` as `application/json` without files
- **THEN** the request follows the existing JSON-only start contract without any new required fields

#### Scenario: Invalid multipart body
- **WHEN** a client sends a missing or duplicate `input` part, invalid JSON, or an unsupported part
- **THEN** the API returns a client error without creating an instance

### Requirement: Input-request replies accept optional multipart attachments
The existing `POST /instances/{id}/requests/{request_id}/response` route SHALL continue to accept JSON-only responses with unchanged field validation and conflict behavior. It SHALL additionally accept multipart data containing exactly one JSON `response` part and one or more `files` parts. The `response` JSON SHALL have exactly the declared response ports and compatible values, validated by the API in both local and distributed modes using its existing persisted request metadata; the worker SHALL validate it again before consumption. Files SHALL be supplementary to the typed response, not extra `.bl` ports. A reply and its attachment references SHALL be accepted together at most once; a malformed body, invalid response, unknown or already answered request, cancelled/terminal instance, or expired deadline SHALL NOT commit either a reply or attachment references.

#### Scenario: Answer with a file
- **WHEN** a client POSTs multipart data with `response` containing `{"approved":true}` and a `files` part to an outstanding request with `approved: Bool`
- **THEN** the response is accepted once, the file is associated with that request, and the same graph activation becomes eligible to resume

#### Scenario: Existing JSON response
- **WHEN** a client POSTs `{"approved":true}` as `application/json` without files
- **THEN** the existing reply validation and acceptance behavior remains unchanged

#### Scenario: Invalid or late multipart reply
- **WHEN** a multipart reply has a duplicate or missing `response` part, invalid typed JSON, or is posted to an answered or cancelled request
- **THEN** the API returns the existing kind of client error or conflict and does not change the accepted reply or attachments
