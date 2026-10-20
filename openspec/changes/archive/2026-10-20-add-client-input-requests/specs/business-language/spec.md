# Spec Delta

## ADDED Requirements

### Requirement: A process may request typed input from a client
A `.bl` process SHALL support a named `input_request` task node with a nonempty literal `prompt` and one or more named, typed `output` response ports. It SHALL be entered through explicit `flow`, continue through an ordinary outgoing `flow` only after a valid response is committed, and expose its response ports to downstream `bind` statements. Response types SHALL be supported `.bl` port types, including named dictionary types resolved in the process scope. The compiler SHALL reject duplicate or unknown ports, unsupported or unresolved types, missing or ambiguous downstream bindings, inaccessible response values, and malformed task-node declarations before generating code. The request SHALL NOT be a blocking `decision_task`, a process start event, or a timed pause.

#### Scenario: Prompt and bind response
- **WHEN** a process declares `input_request review { prompt "Approve this order?"; output approved: Bool; }`, flows through `review`, and binds `review.approved` to a downstream node's `Bool` input
- **THEN** validation succeeds and the downstream node receives the client's typed response after `review` completes

#### Scenario: Invalid response binding
- **WHEN** a request's `Bool` output is bound to a `Number` input, its output is referenced before the request is reached, or its declaration omits a prompt or response port
- **THEN** compilation fails with an actionable source diagnostic and emits no Rust

#### Scenario: Repeated and nested request declarations
- **WHEN** a process loop revisits a request task node or an acyclic subprocess reaches one
- **THEN** the source remains valid and runtime activations retain independent request identities within the parent instance
