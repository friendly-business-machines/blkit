# Tasks

## 1. `.bl` task and graph syntax

- [x] 1.1 Parse `.bl` task bodies and process maps with named task nodes, typed links, and explicit paired AND/OR/XOR gateways; verify parser fixtures accept sequential and nested fork/join maps while legacy one-body process tests still pass.
- [x] 1.2 Parse gateway conditions over process input and upstream task outputs, including ordered XOR branches and OR/XOR fallback routes; verify malformed links, conditions, and missing fallbacks produce parser diagnostics.
- [x] 1.3 Document the supported graph syntax with a small `.bl` example in README; verify it parses and the existing `examples/approve.bl` still compiles.

## 2. Static graph validation and code generation

- [x] 2.1 Validate task names, reachability, acyclic links, structured split/join pairing, typed task input/output links, and typed process output on all routes; verify tests reject unknown nodes, cycles, unreachable outputs, and mismatched links.
- [x] 2.2 Type-check gateway `Bool` conditions and path availability of referenced outputs, plus AND record/XOR common-type/OR `List<T>` join results; verify tests reject unavailable references, incompatible branches, and non-boolean conditions.
- [x] 2.3 Emit compiled `.bl` task logic, gateway predicates, typed boundary adapters, and an executable graph definition (not a Rust-authored map); verify generated Rust compiles and exposes namespace/version/process identity while legacy generated modules remain standalone.

## 3. Durable instance records

- [x] 3.1 Add a local file-backed instance store using a pinned `turso` crate version, with unique IDs, versioned identity, JSON input/output, statuses, failure reason, timestamps, and explicit transactions for related writes; verify tests commit and reopen pending and terminal instances without a database server.
- [x] 3.2 On server startup, mark persisted nonterminal instances failed/interrupted without replay; verify reopen tests retain acknowledged results and fail previously running/cancelling instances, and an abrupt-exit test recovers acknowledged writes.
- [x] 3.3 Exercise concurrent instance writes against the pinned Turso version; verify all committed transitions remain readable after reopen and no acknowledged status disappears.

## 4. Compiled-graph execution

- [x] 4.1 Register compiler-emitted process definitions and run activated task nodes with typed inputs/outputs, gateway route selection, and correct AND/OR/XOR joins; verify tests exercise all three gateways, ordered XOR matching, OR fallback, two simultaneous OR branches, and typed join results.
- [x] 4.2 Use a configurable shared in-flight limit while allowing concurrent activated tasks and independent instances; verify barrier-based tests show parallelism with capacity and never exceed a smaller configured limit.
- [x] 4.3 Stop advancement on task failure and request cancellation of in-flight siblings; verify graph tests show no downstream task starts after failure.

## 5. Cancellation and races

- [x] 5.1 Persist cancellation before halting gateway/task dispatch and invoking cancellation on every in-flight task; verify tests cover cancellation before dispatch, two running sibling tasks, no successor advancement, and late completions.
- [x] 5.2 Serialize cancellation/completion/failure races and make repeated requests safe without overwriting terminal outcomes; verify race tests cover both event orders and cancellation of sibling tasks.

## 6. Single-node dev server

- [x] 6.1 Compile a source-defined `.bl` graph example into a separate single-binary dev server with locally bound JSON start/status/cancel routes; verify HTTP tests cover valid requests, 400/404/409 cases, gateway-selected output, and unchanged compiler CLI tests.
- [x] 6.2 Document build/run steps, local Turso database path, concurrency limit, curl requests, restart behavior, and cooperative-cancellation limits in README; verify the documented graph example starts an instance and returns its result.

## 7. Integration verification

- [x] 7.1 Run the full Rust test suite and `openspec validate process-runtime-mvp --strict`; verify both pass and that no Rust-authored process map, distributed coordination, or external Rust/Python task ABI was added.
