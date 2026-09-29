# Spec Delta

## MODIFIED Requirements

### Requirement: MVP built-in types are fixed
The language SHALL provide the built-in user-facing types `Bool`, `String`, `Number`, `Date`, `DateTime`, `Time`, and `List<T>`. `Date` SHALL represent a Gregorian calendar date encoded as `YYYY-MM-DD`. `Time` SHALL represent a timezone-free wall-clock time encoded as `HH:MM:SS` with optional fractional seconds, in the range 00:00:00 through 23:59:59.999...; leap seconds and offsets SHALL be rejected. `DateTime` SHALL represent a timezone-aware instant; serialized inputs SHALL accept an RFC 3339 timestamp with an explicit offset and reject missing or invalid offsets. Comparisons of `Date` and `Time` SHALL use chronological order within their respective types; comparisons of `DateTime` SHALL compare instants, not textual representations. `.bl` expressions SHALL provide typed `date("YYYY-MM-DD")`, `time("HH:MM:SS[.fraction]")`, and `dateTime("RFC3339-with-offset")` constructors, rejecting invalid constant text at compile time. These constructors SHALL require string literals; plain quoted text remains `String`.

#### Scenario: Number literal typing
- **WHEN** source uses numeric literals such as `1`, `1000`, or `12.50`
- **THEN** validation treats those literals as `Number` values without requiring an integer type

#### Scenario: Typed list literal
- **WHEN** a value of type `List<Number>` is supplied as `[1, 2.5]`
- **THEN** validation accepts both elements as `Number` values

#### Scenario: List element type mismatch
- **WHEN** a value of type `List<Number>` is supplied as `[1, "two"]`
- **THEN** validation fails with a diagnostic identifying the incompatible element

#### Scenario: DateTime input
- **WHEN** a typed `DateTime` field receives `"2026-10-02T09:30:00+02:00"`
- **THEN** validation accepts it and comparison uses the corresponding instant

#### Scenario: Date and Time input and typed literals
- **WHEN** `Date` receives `"2026-10-02"` or `Time` receives `"09:30:00.250"`, or a `.bl` expression uses `date("2026-10-02")` or `time("09:30:00.250")`
- **THEN** the values SHALL validate with their declared temporal types and sort chronologically

#### Scenario: Invalid temporal values
- **WHEN** `Date` receives `"2026-02-30"`, `Time` receives `"24:00:00"` or `"09:30:00+02:00"`, or a temporal constructor contains invalid text
- **THEN** the input SHALL be rejected or compilation SHALL fail, respectively, with a diagnostic

#### Scenario: Unsupported built-in type
- **WHEN** source uses an unsupported built-in type such as `Table<T>`, `Range`, `Any`, or `Optional<T>`
- **THEN** validation fails with a diagnostic identifying the unsupported type

## ADDED Requirements

### Requirement: Same-type range expressions have explicit boundaries
`.bl` SHALL accept interval expressions `[a..b]`, `(a..b)`, `[a..b)`, and `(a..b]`, where finite endpoints have the same type from `Number`, `Date`, `DateTime`, or `Time`. At least one finite bound SHALL establish the range's type; if both are `null`, a type SHALL be supplied by context (for example, the value tested for membership). `[`/`]` SHALL include the corresponding endpoint, and `(`/`)` SHALL exclude it. The literal `null` SHALL denote an unbounded endpoint only inside a range expression; it SHALL NOT introduce a nullable value type. An omitted bound SHALL have no inclusion semantics. Both bounds MAY be unbounded. Statically reversed finite bounds SHALL fail compilation. Reversed finite bounds discovered only at evaluation SHALL describe an empty range rather than silently swapping bounds. Equal finite bounds SHALL describe a singleton only when both ends are inclusive; otherwise they SHALL describe an empty range.

#### Scenario: Boundary combinations
- **WHEN** `.bl` evaluates `1 in [1..5]`, `1 in (1..5)`, `5 in [1..5)`, and `5 in (1..5]`
- **THEN** the results SHALL be `true`, `false`, `false`, and `true`, respectively

#### Scenario: Unbounded ends
- **WHEN** `.bl` evaluates `18 in [18..null)`, `100 in [18..null)`, a negative `Number` input in `(null..0)`, and `0 in (null..0)`
- **THEN** the results SHALL be `true`, `true`, `true`, and `false`, respectively

#### Scenario: Equal and inverted endpoints
- **WHEN** `.bl` evaluates `1 in [1..1]` and `1 in (1..1]`, or compiles `[5..1]`
- **THEN** the membership results SHALL be `true` and `false`, and the inverted literal SHALL be rejected with a diagnostic

#### Scenario: Dynamically reversed range
- **WHEN** a range formed from two runtime `Number` inputs has a lower bound greater than its upper bound
- **THEN** it SHALL be empty: membership and overlap SHALL return `false`

#### Scenario: Invalid bound type or null outside a range
- **WHEN** a range has a `String` endpoint, mixes `Date` and `DateTime`, both bounds and type context are absent, or `null` is used as a general expression
- **THEN** validation SHALL fail before Rust is emitted

### Requirement: Range membership, equality, and interval relations
The `in` operator SHALL test whether a scalar belongs to a range of the same type. `x between a and b` SHALL be equivalent to `x in [a..b]` with finite same-type bounds. Ranges SHALL support `==` and `!=` based on their effective endpoints and inclusion, not textual formatting; no `=` equality operator is added. The functions `before`, `after`, `meets`, `metBy`, `overlaps`, `overlapsBefore`, `overlapsAfter`, and `coincides` SHALL take two ranges of the same type and return `Bool`. `includes(range, value)` and `during(value, range)` SHALL test membership. `starts(value, range)`/`startedBy(range, value)` SHALL test the included finite start point; `finishes(value, range)`/`finishedBy(range, value)` SHALL test the included finite end point. Incompatible arguments SHALL fail type validation before generation.

`before(A,B)` SHALL mean A's upper bound is strictly below B's lower bound; `after` SHALL be its converse. `meets(A,B)` SHALL mean A's finite upper bound equals B's finite lower bound, irrespective of endpoint inclusion; `metBy` SHALL be its converse. `overlaps(A,B)` SHALL mean the ranges have at least one value in common. `overlapsBefore(A,B)` SHALL mean A starts before B, B starts strictly before A ends, and A ends strictly before B ends; `overlapsAfter` SHALL be the converse. For these ordering relations, unbounded ends compare below/above all finite bounds as appropriate. `coincides` SHALL mean range equality. Empty ranges SHALL NOT overlap any range.

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
