# decision-models Specification

## Purpose

Defines typed, source-authored decision requirement graphs in `.bl` for reusable business rules, including evaluation of complete decision-table hit policies without a separate DMN runtime.

## Requirements

### Requirement: Decision models contain typed acyclic dependencies
A `.bl` decision model SHALL declare typed input and output and named dependencies between decisions. Decisions SHALL be implemented by literal expressions, decision tables, or boxed contexts. The compiler SHALL reject missing references, incompatible input/output types, duplicate names, unavailable dependencies, and cycles within a decision model. Decision graph evaluation SHALL use `.bl` expressions rather than FEEL and SHALL be deterministic for the same input.

#### Scenario: Multiple dependent decisions
- **WHEN** a decision model calculates eligibility and then uses that decision to calculate a discount
- **THEN** validation accepts the dependency graph and the final decision returns its declared type

#### Scenario: Cyclic decision dependencies
- **WHEN** two decisions depend on each other
- **THEN** compilation fails with a diagnostic identifying the decision cycle

### Requirement: Boxed contexts and business knowledge models are reusable
A boxed context SHALL evaluate named entries in dependency order and return a typed final expression. A business knowledge model SHALL declare typed parameters and a typed result, be callable by decisions and contexts in the model, and validate argument and return types. Dependency cycles including recursive knowledge-model calls SHALL be rejected.

#### Scenario: Reuse calculation
- **WHEN** two decisions invoke the same knowledge model with valid typed arguments
- **THEN** both can use its result without duplicating its definition

#### Scenario: Invalid context reference
- **WHEN** a context entry refers to an unavailable or cyclic entry
- **THEN** validation reports the offending reference before code is emitted

### Requirement: Decision tables cover all DMN hit policies
A decision table SHALL support UNIQUE, ANY, FIRST, PRIORITY, RULE ORDER, OUTPUT ORDER, and COLLECT hit policies and COLLECT SUM, MIN, MAX, and COUNT aggregations. Input columns SHALL be typed `.bl` expressions over available inputs and dependencies, and rules SHALL be evaluated against their computed input-column values. Rule conditions and output expressions SHALL be type checked. Rules SHALL be considered in declaration order: UNIQUE rejects more than one match; ANY accepts multiple matches only when their full output values agree; FIRST returns the first match; PRIORITY returns the highest-priority match; RULE ORDER returns all matches in rule order; OUTPUT ORDER returns all matches in priority order with rule order breaking ties; COLLECT without aggregation returns all matches in rule order. Priority-based policies SHALL require an explicit output priority order; evaluation SHALL report an unranked matching output as an error. When a single-result policy has no match, it SHALL return a type-correct declared default if one exists, otherwise report a decision evaluation error.

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
A table SHALL support multiple typed output columns; each matching rule SHALL provide every column and the result SHALL be a typed record (or a list of those records for multi-result policies). SUM, MIN, and MAX SHALL aggregate a single `Number` output column; COUNT SHALL produce a `Number` match count. The compiler SHALL reject unsupported aggregation/output combinations, including SUM, MIN, or MAX over multiple output columns. A table SHALL permit a type-correct no-match default for its policy result; absent a default, multi-result policies return an empty list and COUNT returns zero, while SUM/MIN/MAX report a decision evaluation error.

#### Scenario: Multi-column result
- **WHEN** a table declares a `Number` price and a `String` tier and a matching rule yields both
- **THEN** its result contains both typed columns, and RULE ORDER returns a list of such results

#### Scenario: Numeric aggregation
- **WHEN** three matching numeric rules return 2, 3, and 5 under COLLECT SUM
- **THEN** the result is `Number` 10; COLLECT COUNT on the same matches returns `Number` 3

#### Scenario: Invalid aggregate shape
- **WHEN** COLLECT MIN is declared on a two-output table
- **THEN** validation rejects it before generating Rust
