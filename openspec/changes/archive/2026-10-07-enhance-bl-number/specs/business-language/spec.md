# Spec Delta

## ADDED Requirements

### Requirement: Number literals and arithmetic are decimal expressions
`.bl` SHALL accept signed integers, fractional decimal literals, and decimal scientific notation (for example `42`, `-5`, `3.14`, `1500.50`, `1.5e3`, `1.5e-3`) as `Number`. For representable results, `+`, binary `-`, unary `-`, `*`, `/`, and `**` SHALL operate on `Number` without binary floating-point artifacts. Exponentiation SHALL bind tighter than unary negation, which SHALL bind tighter than multiplication/division, which SHALL bind tighter than addition/subtraction; `**` SHALL be right-associative. Numeric `==`, `!=`, `<`, `<=`, `>`, and `>=` SHALL compare values rather than source spelling; `=` SHALL remain invalid. Decimal output through `string(Number)` SHALL omit redundant fractional trailing zeroes and an unnecessary decimal point. Results requiring more precision than `Number` supports SHALL be rounded to representable decimal precision; unrepresentable inputs/results, division by zero, and invalid exponent domains SHALL report validation errors for invalid constants or evaluation errors for runtime inputs, never a panic or silent wrap.

#### Scenario: Exact decimal and normalized output
- **WHEN** `.bl` evaluates `0.1 + 0.2`, `10 / 4`, `3.0 == 3.00`, `2 ** 8`, `9 ** 0.5`, `1.5e3`, and `string(1500.50)`
- **THEN** the results SHALL be `0.3`, `2.5`, `true`, `256`, `3`, `1500`, and `"1500.5"`, respectively

#### Scenario: Arithmetic precedence and signed values
- **WHEN** `.bl` evaluates `10 - 4 * 2`, `-(7)`, `-2 ** 2`, and `2 ** 3 ** 2`
- **THEN** the results SHALL be `2`, `-7`, `-4`, and `512`, respectively

#### Scenario: Invalid calculations and equality alias
- **WHEN** `.bl` evaluates `1 / 0` or an unrepresentable result, or source contains `3.0 = 3.00`
- **THEN** arithmetic SHALL report a calculation error and the `=` expression SHALL fail validation before generation

### Requirement: Number rounding supports decimal scales and explicit tie modes
`round(n, scale)` SHALL alias `roundHalfUp(n, scale)`. `roundUp` SHALL round away from zero; `roundDown` SHALL round toward zero; `roundHalfUp` and `roundHalfDown` SHALL round nearest with exact halfway ties away from and toward zero, respectively; `roundHalfEven` SHALL resolve halfway ties to the even neighbor. `floor(n[, scale])` SHALL round toward negative infinity and `ceiling(n[, scale])` toward positive infinity; omitted scale SHALL mean zero. Scale SHALL be an integral `Number` specifying decimal places, including negative scales for tens/hundreds. Invalid or unrepresentable scales SHALL produce evaluation errors.

#### Scenario: Rounding and ties
- **WHEN** `.bl` evaluates `round(2.345, 2)`, `roundUp(5.1, 0)`, `roundDown(5.9, 0)`, `roundHalfUp(-5.5, 0)`, `roundHalfDown(5.5, 0)`, and `roundHalfEven(2.5, 0)`
- **THEN** the results SHALL be `2.35`, `6`, `5`, `-6`, `5`, and `2`, respectively

#### Scenario: Floor, ceiling, and negative scale
- **WHEN** `.bl` evaluates `floor(-1.56, 1)`, `ceiling(-1.56, 1)`, `floor(1.9)`, and `round(1250, -2)`
- **THEN** the results SHALL be `-1.6`, `-1.5`, `1`, and `1300`, respectively

### Requirement: Number math and predicates are typed
`abs(n)`, `sqrt(n)`, `exp(n)`, `ln(n)`, `log(n[, base])`, and `clamp(n, min, max)` SHALL accept `Number` and return `Number`; omitted `log` base SHALL be 10. `modulo(dividend, divisor)` SHALL use floor-division remainder with the sign of a nonzero divisor. `odd(n)` and `even(n)` SHALL require integral `Number` and return `Bool`; `isPositive(n)`, `isNegative(n)`, and `isZero(n)` SHALL return `Bool`, with zero neither positive nor negative. `clamp` SHALL require `min <= max` and return the nearest inclusive bound or `n` when in range. `sqrt` SHALL reject negative inputs, `ln`/`log` SHALL require positive arguments and a positive base other than 1, and zero divisors, invalid domains, or values outside representable precision/range SHALL produce evaluation errors. Inexact transcendental results SHALL be decimal approximations; exact representable cases SHALL return the exact result.

#### Scenario: Math and clamp
- **WHEN** `.bl` evaluates `abs(-10)`, `modulo(-10, 3)`, `sqrt(16)`, `log(100)`, `log(8, 2)`, `clamp(150, 0, 100)`, and `exp(1)`
- **THEN** the results SHALL be `10`, `2`, `4`, `2`, `3`, `100`, and a decimal approximation of Euler's number, respectively

#### Scenario: Predicates and invalid inputs
- **WHEN** `.bl` evaluates `odd(5)`, `even(2)`, `isPositive(5)`, `isNegative(-3)`, and `isZero(0)`, or evaluates `odd(1.5)`, `sqrt(-1)`, `log(8, 1)`, or `clamp(1, 5, 0)`
- **THEN** the first five results SHALL be `true`; each remaining call SHALL report an evaluation error

### Requirement: Number aggregates consume lists of numbers
`min`, `max`, `sum`, `mean`, `median`, `product`, `stddev`, and `mode` SHALL each take exactly one `List<Number>` and return a `Number`. `median` SHALL average the middle pair for an even-sized list. `stddev` SHALL calculate the sample standard deviation using denominator `N - 1`; it SHALL require at least two values. `mode` SHALL return the most frequent value; if frequencies tie, it SHALL return the numerically smallest tied value. For an empty list `sum` SHALL return zero and `product` SHALL return one; `min`, `max`, `mean`, `median`, and `mode` SHALL report evaluation errors. Decimal results SHALL follow `Number` precision and overflow/error rules.

#### Scenario: Numeric aggregation
- **WHEN** `.bl` evaluates `min([3, 1, 2])`, `max([3, 1, 2])`, `sum([1, 2, 3])`, `mean([1, 2, 3])`, `median([1, 2, 3, 4])`, `product([2, 3, 4])`, `stddev([1, 2, 3])`, and `mode([3, 2, 3, 2])`
- **THEN** the results SHALL be `1`, `3`, `6`, `2`, `2.5`, `24`, `1`, and `2`, respectively

#### Scenario: Empty and undersized lists
- **WHEN** `.bl` evaluates `sum([])`, `product([])`, `mean([])`, or `stddev([5])`
- **THEN** the first two results SHALL be `0` and `1`, and the latter two SHALL report evaluation errors

### Requirement: Number text conversion accepts explicit separators
`number(text)` SHALL parse a `String` containing a signed decimal number with `.` as its decimal separator and no grouping separator, returning `Number`. `number(text, groupingSeparator, decimalSeparator)` SHALL parse a `String` with the supplied distinct, single-character grouping and decimal separators (for example `number("1.500,50", ".", ",")`). Grouping, if present, SHALL divide the integer portion into groups of three digits except for a leading group of one to three digits. Parsing SHALL reject misplaced separators, empty/non-numeric text, unsupported separators, and unrepresentable values; a constant invalid string SHALL fail validation and a runtime string SHALL report an evaluation error.

#### Scenario: Parse default and localized decimals
- **WHEN** `.bl` evaluates `number("1500.50")` and `number("1.500,50", ".", ",")`
- **THEN** both SHALL equal `1500.5`

#### Scenario: Invalid grouping and nonnumeric text
- **WHEN** `.bl` evaluates `number("12.34,50", ".", ",")` or receives `"nope"` as runtime input to `number`
- **THEN** each SHALL fail with a diagnostic or evaluation error as appropriate, rather than returning zero

### Requirement: Runtime-fallible numeric expressions propagate errors
A `.bl` expression involving a numeric operation that can fail for runtime values SHALL report an execution error to its caller, including from decision tasks, decision tables, process conditions, and generated standalone or project builds; it SHALL NOT panic or substitute a plausible result. Named output ports SHALL retain their declared `Number` or `Bool` types. Existing unaffected expressions SHALL preserve their behavior.

#### Scenario: Dynamic division error in a task
- **WHEN** a compiled decision task divides a constant by a runtime `Number` input of zero
- **THEN** the invocation SHALL return an execution error rather than a process panic or a numeric result

## MODIFIED Requirements

### Requirement: Range membership, equality, and interval relations
The `in` operator SHALL test whether a scalar belongs to a range of the same type. `x between a and b` SHALL be equivalent to `x in [a..b]` with finite same-type bounds. Ranges SHALL support `==` and `!=` based on their effective endpoints and inclusion, not textual formatting; no `=` equality operator is added. The functions `before`, `after`, `meets`, `metBy`, `overlaps`, `overlapsBefore`, `overlapsAfter`, and `coincides` SHALL accept two ranges of the same type and return `Bool`; the numeric point overloads for `before`, `after`, `meets`, and `metBy` are defined below. `includes(range, value)` and `during(value, range)` SHALL test membership. `starts(value, range)`/`startedBy(range, value)` SHALL test the included finite start point; `finishes(value, range)`/`finishedBy(range, value)` SHALL test the included finite end point. Incompatible arguments SHALL fail type validation before generation.

`before(A,B)` SHALL mean A's upper bound is strictly below B's lower bound; `after` SHALL be its converse. `meets(A,B)` SHALL mean A's finite upper bound equals B's finite lower bound, irrespective of endpoint inclusion; `metBy` SHALL be its converse. `overlaps(A,B)` SHALL mean the ranges have at least one value in common. `overlapsBefore(A,B)` SHALL mean A starts before B, B starts strictly before A ends, and A ends strictly before B ends; `overlapsAfter` SHALL be the converse. For these ordering relations, unbounded ends compare below/above all finite bounds as appropriate. `coincides` SHALL mean range equality. Empty ranges SHALL NOT overlap any range.

In addition, `before(Number, Range<Number>)` SHALL test whether the point is strictly below a finite lower endpoint; `before(Range<Number>, Number)` SHALL test whether a finite upper endpoint is strictly below the point; `after` SHALL be the converse in each argument order. `meets(Number, Range<Number>)` SHALL test equality to the finite lower endpoint and `meets(Range<Number>, Number)` SHALL test equality to the finite upper endpoint, irrespective of inclusion; `metBy` SHALL be the converse. Existing `during`, `includes`, `starts`, `startedBy`, `finishes`, and `finishedBy` SHALL continue to accept Number points and Number ranges, respecting included endpoints. No point overload is added to the two-range `overlaps`, `overlapsBefore`, `overlapsAfter`, or `coincides` relations. A missing required finite endpoint SHALL make these point `before`/`after`/`meets`/`metBy` checks false.

#### Scenario: Scalar membership and between
- **WHEN** `.bl` evaluates `25 in [18..65]`, `3 in [1..10]`, `10 in [1..10)`, and `5 between 1 and 10`
- **THEN** the results SHALL be `true`, `true`, `false`, and `true`, respectively

#### Scenario: Date, DateTime, and Time membership
- **WHEN** `.bl` evaluates `date("2026-10-02") in [date("2026-10-01")..date("2026-10-03")]`, `dateTime("2026-10-02T08:30:00Z") in [dateTime("2026-10-02T09:00:00+01:00")..null)`, and `time("12:00:00") in (time("09:00:00")..time("17:00:00"))`
- **THEN** all three results SHALL be `true`; the `DateTime` comparison SHALL use instants

#### Scenario: Range equality respects endpoint inclusion
- **WHEN** `.bl` evaluates `[1..5] == [1..5]` and `[1..5] == [1..5)`
- **THEN** the results SHALL be `true` and `false`, respectively

#### Scenario: Ordered relations and shared endpoint
- **WHEN** `.bl` evaluates `before([1..5], [6..10])`, `after([6..10], [1..5])`, `meets([1..5], [5..10])`, and `metBy([5..10], [1..5])`
- **THEN** all four results SHALL be `true`

#### Scenario: Overlap relations
- **WHEN** `.bl` evaluates `overlaps([5..10], [1..6])`, `overlapsBefore([1..5], [4..10])`, and `overlapsAfter([4..10], [1..5])`
- **THEN** all three results SHALL be `true`

#### Scenario: Point relations and coincidence
- **WHEN** `.bl` evaluates `includes([1..10], 5)`, `during(5, [1..10])`, `starts(1, [1..5])`, `startedBy([1..5], 1)`, `finishes(5, [1..5])`, `finishedBy([1..5], 5)`, and `coincides([1..5], [1..5])`
- **THEN** all seven results SHALL be `true`

#### Scenario: Open and unbounded point boundaries
- **WHEN** `.bl` evaluates `starts(1, (1..5])`, `finishes(5, [1..5))`, or `starts(1, (null..5])`
- **THEN** each result SHALL be `false`

#### Scenario: Numeric points before, after, and meeting intervals
- **WHEN** `.bl` evaluates `before(0, [1..5])`, `after(6, [1..5])`, `before([1..5], 6)`, `meets(1, (1..5])`, `metBy([1..5], 1)`, and `before(0, (null..5])`
- **THEN** the first five results SHALL be `true` and the last SHALL be `false`
