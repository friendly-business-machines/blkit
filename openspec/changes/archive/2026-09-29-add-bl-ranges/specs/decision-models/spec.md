# Spec Delta

## ADDED Requirements

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
