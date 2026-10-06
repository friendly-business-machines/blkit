# Spec Delta

## ADDED Requirements

### Requirement: Decision nodes use specific keywords and explicit typed outputs
Inside a braced `decision_task <name> { ... }`, each decision-node declaration SHALL use its specific kind keyword (`literal_expression`, `decision_table`, or `context`), a name, and a braced body. A decision task SHALL declare named, typed `input` and `output` ports in its body. Each decision node SHALL expose its result through a named, typed output; the task SHALL map each task output to a compatible decision-node output using `output <name>: <Type> = <node>.<port>;`. A reference to `<node>` without a port SHALL be permitted only if the node has exactly one output. Neither the task nor its nodes SHALL use positional argument lists. All body statements SHALL end in `;`.

#### Scenario: Single-output shorthand
- **WHEN** `literal_expression compute { output result: Number; expression amount; }` is the only producing node and a task declares `output result: Number = compute;`
- **THEN** the task output resolves to `compute.result`

#### Scenario: Multi-output reference
- **WHEN** a node exposes `price` and `tier` and the task maps a compatible output to `node.price`
- **THEN** validation resolves the selected output; using bare `node` is rejected as ambiguous

#### Scenario: Invalid result mapping
- **WHEN** a task output references a missing node or port or an incompatible type
- **THEN** validation fails before code generation

## MODIFIED Requirements

### Requirement: Decision models contain typed acyclic dependencies
A `.bl` `decision_task` SHALL declare typed named inputs and outputs and kind-specific braced decision nodes, with dependencies inferred from typed node-output references in node expressions and task output mappings. No explicit decision `link` or dependency-edge declaration SHALL be accepted. Decisions SHALL be implemented by literal expressions, decision tables, or boxed contexts. The compiler SHALL reject missing references, incompatible input/output types, duplicate names, unavailable dependencies, and cycles within a decision task. Evaluation SHALL use `.bl` expressions rather than FEEL and SHALL be deterministic for the same input. The old `decision ...:` and `node ... = literal|table|context` forms SHALL fail parsing.

#### Scenario: Multiple dependent decisions
- **WHEN** a decision task calculates eligibility in one node and a later node references the first node's compatible typed output
- **THEN** validation infers the dependency and evaluates the first node before the second without an explicit dependency statement

#### Scenario: Cyclic decision dependencies
- **WHEN** two decision nodes reference each other's outputs
- **THEN** compilation fails with a diagnostic identifying the decision cycle

#### Scenario: Missing dependency
- **WHEN** a decision node references a missing or unavailable node output
- **THEN** compilation fails before emitting Rust

### Requirement: Boxed contexts and business knowledge models are reusable
A braced `context` decision node SHALL evaluate named, semicolon-terminated entries in dependency order and expose a typed result. Business knowledge definitions inside a `decision_task` SHALL use braced bodies and named typed inputs/outputs rather than positional task signatures; they SHALL remain callable by decisions and contexts, with argument and return types validated. Dependency cycles including recursive knowledge calls SHALL be rejected. These knowledge definitions SHALL NOT introduce another process task kind.

#### Scenario: Reuse calculation
- **WHEN** two decision nodes invoke the same typed knowledge definition
- **THEN** both can use its result without duplicating its definition

#### Scenario: Invalid context reference
- **WHEN** a context entry refers to an unavailable or cyclic entry
- **THEN** validation reports the offending reference before code generation

### Requirement: Decision tables cover all DMN hit policies
A braced `decision_table` node SHALL support UNIQUE, ANY, FIRST, PRIORITY, RULE ORDER, OUTPUT ORDER, and COLLECT hit policies and COLLECT SUM, MIN, MAX, and COUNT aggregations. Typed input columns, output columns, rules, priorities, and defaults SHALL be semicolon-terminated statements in its body; its evaluated result SHALL be exposed as a typed node output. Input columns SHALL use typed `.bl` expressions over available task inputs and inferred decision dependencies. Rule conditions and outputs SHALL be type checked. Rules SHALL be considered in declaration order: UNIQUE rejects more than one match; ANY accepts multiple matches only when their full output values agree; FIRST returns the first match; PRIORITY returns the highest-priority match; RULE ORDER returns all matches in rule order; OUTPUT ORDER returns all matches in priority order with rule order breaking ties; COLLECT without aggregation returns all matches in rule order. Priority-based policies SHALL require explicit output priority order; an unranked matching output SHALL error. A single-result policy with no match SHALL use a declared type-correct default or report an evaluation error.

#### Scenario: Multiple matching rules
- **WHEN** two different-output rules match under UNIQUE or ANY
- **THEN** evaluation reports a policy violation rather than silently selecting one

#### Scenario: Priority versus declaration order
- **WHEN** two rules match and the later rule has higher declared priority
- **THEN** PRIORITY chooses the later rule, FIRST chooses the earlier rule, and OUTPUT ORDER returns both in priority order

#### Scenario: No matching rule
- **WHEN** no rules match a table with a configured default result
- **THEN** evaluation returns the configured typed default

### Requirement: Tables support multiple output columns and aggregations
A braced decision table SHALL support multiple typed output columns; each matching rule SHALL provide every column and the table's result SHALL be a single typed record-valued node output (or a list of records for multi-result policies). SUM, MIN, and MAX SHALL aggregate a single `Number` output column; COUNT SHALL produce a `Number` match count. The compiler SHALL reject unsupported aggregation/output combinations, including SUM, MIN, or MAX over multiple output columns. A table SHALL permit a type-correct no-match default for its policy result; absent a default, multi-result policies return an empty list and COUNT returns zero, while SUM/MIN/MAX report a decision evaluation error. Qualified node-output references SHALL identify the table's result port, not its internal output-column names.

#### Scenario: Multi-column result
- **WHEN** a table declares a `Number` price and a `String` tier and a matching rule yields both
- **THEN** its result port contains a typed record of both columns; RULE ORDER returns a list of such records

#### Scenario: Numeric aggregation
- **WHEN** three matching numeric rules return 2, 3, and 5 under COLLECT SUM
- **THEN** the result is `Number` 10; COLLECT COUNT on the same matches returns `Number` 3

#### Scenario: Invalid aggregate shape
- **WHEN** COLLECT MIN is declared on a two-output table
- **THEN** validation rejects it before generation
