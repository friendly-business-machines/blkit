# Spec Delta

## MODIFIED Requirements

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
