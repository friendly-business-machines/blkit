# Spec Delta

## ADDED Requirements

### Requirement: Subprocess calls execute durably within one instance
The runtime SHALL execute a subprocess as a child graph within its parent's instance ID, without creating or exposing a separate child instance. It SHALL checkpoint child progress, selected routes, completed outputs, waits, retry state, and deadline origin with the parent state before dispatching dependent work. Repeated or concurrent activations of one subprocess node SHALL have independent child state. Local and distributed execution SHALL resume unfinished child work from that checkpoint without replaying committed results; in-flight/uncommitted external effects MAY repeat as with ordinary tasks. Child work SHALL share the parent instance's in-flight task limit and SHALL release worker capacity while waiting.

#### Scenario: Resume after child wait
- **WHEN** a child checkpoints completed work and a pending wait, then the worker or local server restarts
- **THEN** the parent keeps its instance identity, resumes the wait at its original wake instant, and does not replay committed child work

#### Scenario: Concurrent child activations
- **WHEN** two branches call the same child process concurrently
- **THEN** each call retains its own progress and output, under the shared in-flight task limit

### Requirement: Subprocess policies and outcomes are scoped
A child SHALL retain its own retry and deadline policies while executing in the parent instance. Its retry window and deadline origins SHALL be persisted from entry into the subprocess node (`queued`) or first execution of that child activation (`first_claimed`), not from the parent's start; they SHALL NOT reset across worker handoffs, waits, or child retries. A child's deadline expiry SHALL yield its `timeout` error outcome without retry, and a child's modeled `error`, `cancel`, or `terminate` SHALL stop its child scope and signal in-flight child work. A matching parent handler SHALL advance its configured exceptional route; an unhandled outcome SHALL end the whole parent instance with the same outcome and terminal name where applicable, stopping sibling work. Child execution failures SHALL use the child's retry policy first; once exhausted, they SHALL fail the parent attempt under its retry policy. Parent-initiated cancellation and parent deadline expiry SHALL stop all active children regardless of handlers and take precedence over later child completions.

#### Scenario: Child error is handled
- **WHEN** a child reaches a named `error` with a matching `on error` handler
- **THEN** the child stops, the parent follows the handler route, and the parent is not reported as a business-error instance

#### Scenario: Child deadline is catchable
- **WHEN** a child deadline expires before its parent deadline and the parent has an `on error` route
- **THEN** the child stops with `timeout` and the parent follows the error handler without resetting the parent's deadline

#### Scenario: Child execution retry is exhausted
- **WHEN** a child task fails and exhausts the child's configured retries
- **THEN** the parent attempt fails and follows its own retry policy, preserving committed child work

#### Scenario: External cancellation overrides handler
- **WHEN** cancellation of the parent is accepted while a child is running
- **THEN** child work receives cancellation signals, no child handler route runs, and the parent resolves to cancelled

## MODIFIED Requirements

### Requirement: Terminal events and execution failures have distinct outcomes
When a top-level compiled graph reaches a normal `end`, the instance SHALL complete with its typed output. Reaching a named `error` SHALL produce a business-error terminal outcome with the node's name, distinct from execution failure; `cancel` SHALL produce a cancelled outcome and `terminate` SHALL produce a terminated outcome. `error`, `cancel`, and `terminate` SHALL stop graph advancement, signal in-flight sibling work in their scope, and SHALL NOT consume or schedule execution retries. A child graph's terminal SHALL first be offered to its parent's matching subprocess handler; without one it propagates to the parent. An `end` SHALL complete only when the graph's active work has satisfied its required joins. External cancellation SHALL continue to result in cancelled status.

#### Scenario: Named business error
- **WHEN** routing reaches a named `error` node while another branch is active
- **THEN** work in that process scope stops, its in-flight siblings receive cancellation requests, and the outcome is not retried

#### Scenario: Termination halts parallel work
- **WHEN** a top-level graph reaches a `terminate` node while a sibling task is in flight
- **THEN** no successor starts, the sibling receives a cancellation request, and the instance becomes terminated

#### Scenario: Normal end needs joined work
- **WHEN** a branch finishes but another activated branch required by its join is still running
- **THEN** the instance does not complete until the join and typed `end` are reached

#### Scenario: Caught child terminal does not end parent
- **WHEN** a child reaches `terminate` and its parent has an `on terminate` link
- **THEN** the child scope stops, the parent's handler route continues, and unrelated parent sibling work is not cancelled
