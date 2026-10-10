# decision-models Specification

## Purpose

Defines typed, source-authored decision requirement graphs in `.bl` for reusable business rules, including evaluation of complete decision-table hit policies without a separate DMN runtime.

## Requirements

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
A braced decision table SHALL support multiple typed output columns; each matching rule SHALL provide every column and the table's result SHALL be a single named dictionary-valued node output (or a list of named dictionaries for multi-result policies). The output dictionary's named schema SHALL declare fields corresponding to all output columns with matching types; the old `type Name:` declaration SHALL be invalid. SUM, MIN, and MAX SHALL aggregate a single `Number` output column; COUNT SHALL produce a `Number` match count. The compiler SHALL reject unsupported aggregation/output combinations, including SUM, MIN, or MAX over multiple output columns. A table SHALL permit a type-correct no-match default for its policy result; absent a default, multi-result policies return an empty list and COUNT returns zero, while SUM/MIN/MAX report a decision evaluation error. Qualified node-output references SHALL identify the table's result port, not its internal output-column names.

#### Scenario: Multi-column result
- **WHEN** a table declares a `Number` price and a `String` tier, its result port uses a matching `Quote = {price: Number, tier: String};` schema, and a rule yields both columns
- **THEN** its result port contains a typed dictionary with both columns; RULE ORDER returns a list of such dictionaries

#### Scenario: Numeric aggregation
- **WHEN** three matching numeric rules return 2, 3, and 5 under COLLECT SUM
- **THEN** the result is `Number` 10; COLLECT COUNT on the same matches returns `Number` 3

#### Scenario: Invalid aggregate shape
- **WHEN** COLLECT MIN is declared on a two-output table
- **THEN** validation rejects it before generation

### Requirement: Decision-table rules support range and comparison unary tests
A decision-table rule condition SHALL accept `column matches (test, test, ...)`, where `column` is a declared input column and each test is a range expression or a comparison test using `<`, `<=`, `>`, `>=`, `==`, or `!=` followed by a scalar expression. Each test SHALL compare the column value with a same-type range or scalar. A rule SHALL match the unary-test list if ANY test matches; it SHALL preserve existing full-`Bool` rule conditions, including `and`/`or` composition with a `matches` condition. A unary-test list SHALL be permitted only in decision-table rule conditions, not as a free-standing `.bl` expression or a rule output. Lists of tests SHALL NOT change the table's hit-policy behavior or rule declaration order. Empty or ill-typed lists SHALL fail validation before code generation.

#### Scenario: Alternatives of ranges
- **WHEN** a table declares `input amount: Number = input` and uses `rule amount matches ([2..5], (2..5]) -> 1`
- **THEN** input `2` SHALL match, input `5` SHALL match, and input `6` SHALL not match

#### Scenario: Comparison mixed with a range
- **WHEN** a table uses `rule amount matches (< 10, [20..30]) -> 1`
- **THEN** input `9` or `25` SHALL match, while `15` SHALL not match

#### Scenario: Temporal unary tests
- **WHEN** a table declares a `Date` input column and uses `rule day matches ([date("2026-01-01")..date("2026-01-31")], >= date("2026-12-01")) -> 1`
- **THEN** a date in January or December 2026 SHALL match and a February date SHALL not match; equivalent same-type `DateTime` and `Time` tests SHALL also validate

#### Scenario: Existing Boolean rules and policies remain valid
- **WHEN** an existing rule uses `rule amount > 100 -> 5` or a table has more than one matching rule after using unary tests
- **THEN** Boolean rules SHALL retain their current behavior and multiple matches SHALL follow the table's declared hit policy

#### Scenario: Invalid unary tests
- **WHEN** a table rule tests a `Number` column against a `Date` range, uses `matches ()`, or a non-table expression uses a comma-separated unary-test list
- **THEN** compilation SHALL fail with a diagnostic and SHALL emit no Rust for that file

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
