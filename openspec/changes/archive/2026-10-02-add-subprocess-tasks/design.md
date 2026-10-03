# Design

## Context

See proposal.md for motivation and `specs/` for the behavior contract. `src/graph.rs` parses named nodes/links, `src/semantic.rs` resolves task signatures and checks link types, `src/codegen.rs` emits `GraphDefinition`s, and `src/named_runtime.rs` advances serializable `GraphCheckpoint`s. `src/runtime.rs` drives task dispatch for local and distributed workers; the local and PostgreSQL stores persist a whole checkpoint per instance. Project compilation already merges same-namespace/version declarations before validation. Present terminal kinds end an entire instance, and retry/deadline metadata currently live at the instance level.

## Goals / Non-Goals

**Goals:** Keep one externally visible instance and one atomic parent checkpoint, with independent state per child activation; preserve existing task capacity, waits, recovery, and process policy behavior.

**Non-Goals:** Separate child instance IDs/API, cross-scope calls, recursive process calls, handler payloads or child terminal-name matching, compensation, or exactly-once task side effects.

## Decisions

1. **Explicit subprocess node and outcome links.** Parse `node checked = subprocess child(input)` and `link checked -> recovery on error` (also `cancel`/`terminate`). Require one ordinary success link; optional outcome links select exactly one branch and carry no child output. Type-check against `program.processes` rather than treating a process as a task, and check success-only values are unavailable in handler scopes. Follow the existing project merge for cross-file references. Build a process-call adjacency map and reject cycles via DFS before codegen. This is preferable to overloading `task` or introducing generic exception gateways: terminal routing is subprocess-specific and existing source keeps its meaning.

2. **Resolve child graphs from compiled definitions.** Emit the child process identity/typed input evaluator in the subprocess node; use the existing generated definition registry for the same namespace/version in the worker/server. Check that the runtime's registered child definition exists when registering/executing a parent, rather than running a dynamic `.bl` compile or making a REST request. Reject builds that omit a referenced child. Preserve the existing top-level process registration/advertisement API; a worker only claims its advertised parent identity and must carry the compiled child definitions it uses.

3. **Nested checkpoint, shared driver.** Associate a child `GraphCheckpoint` and evaluated child input with each parent subprocess activation ID, not merely each node name, so loops and parallel branches are independent. Advance child graph events through the existing named-graph scheduler; child leaf tasks consume the same shared semaphore as parent leaf tasks. Never reserve a permit while awaiting a whole child. Persist child transitions in the parent checkpoint through the existing conditional checkpoint commit; a child completion and the corresponding parent link selection are one committed transition. Derive parent instance wake eligibility from the earliest active child wait/retry and parent wait; release claims while all active work is waiting. Do not recursively call `execute_named` with a held task permit or create a second store row. Compare with compile-time graph expansion: expansion makes child retries, deadlines, and terminal scope much harder to preserve.

4. **Scope policies and terminal propagation.** On each child activation persist entry time (`queued` origin), first execution time (`first_claimed` origin), retry attempts/first-failure/next-eligible and deadline instant alongside its checkpoint. Child task failures use that activation's retry policy; after it is exhausted, return an execution failure to the parent attempt, whose retry policy then applies to unfinished work. A child deadline produces `Error("timeout")` without retry. A modeled child terminal cancels child-scope in-flight siblings; route it to the matching parent handler if present, otherwise propagate its kind and name up to the top-level instance, stopping its other work. A parent deadline or external cancellation is checked before child advancement and wins against any subsequently arriving child result. Keep top-level store retry/deadline columns for parent policy; nested metadata lives in the checkpoint, so existing API instance schemas need no new child rows.

5. **Use current generated/runtime integration paths.** Extend the shared named executor and its checkpoint serializer rather than implementing subprocesses separately in the local and distributed stores. Local and worker restart/claim paths load the same checkpoint. Preserve old serialized checkpoint defaults and version migration so already-running graphs without child state still resume. Add narrow parser/semantic, generated execution, local recovery, and distributed worker tests, plus a `.bl` example/README entry.

## Risks / Trade-offs

- [Checkpoint growth with nested/parallel activations] → Key child state by activation and retire terminal child state once the parent transition is durably recorded; preserve existing committed-result history semantics.
- [Deadlock if a parent holds a permit while awaiting a child] → Permit only leaf task invocations; test capacity one with a nested subprocess and wait.
- [Lost updates or replay after worker loss] → Commit nested progress under the same parent claim/generation guard and test resume after a committed child task and a pending wait.
- [Misinterpreting scoped cancel as external cancellation] → Distinguish child-modeled terminal events from parent control-plane cancellation and test their races.
- [Top-level vs child deadline mismatch] → Check parent deadline before dispatch/commit and persist child origins; test child timeout catches and parent timeout precedence.

## Migration Plan

Existing `.bl` source and persisted checkpoints without subprocess nodes keep their current behavior. Deploy binaries containing both parent and child compiled definitions; do not mix incompatible binaries under the same namespace/version/process identity (the existing deployment constraint). Roll back by draining active subprocess instances before restoring an older binary that cannot read the new checkpoint shape. No database schema migration or new dependency is required.
