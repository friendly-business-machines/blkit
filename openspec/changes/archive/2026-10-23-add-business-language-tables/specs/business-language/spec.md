# Spec Delta

## MODIFIED Requirements

### Requirement: MVP built-in types are fixed
The language SHALL provide the built-in user-facing types `Bool`, `String`, `Number`, `Date`, `DateTime`, `Time`, `DTDuration`, `YMDuration`, `Calendar`, `CalendarEntry`, `List<T>`, `Dictionary`, `Dictionary<T>`, `DictionaryEntry<T>`, `Table<Row>`, nullable `T?`, and `Value`. `Table<Row>` SHALL require `Row` to name a declared dictionary row schema at input/output ports; bare `Table` and `Table<Number>` SHALL NOT be supported port declarations. Tables constructed within `.bl` SHALL infer their schema without a separate authored table type, and SHALL NOT silently convert to or from a list of dictionaries. `T?` SHALL describe a value of type `T` or null without making a declared field optional; fields absent from named dictionary input still fail validation. `Dictionary<T>` SHALL contain string keys and values of one type `T`; `Dictionary` SHALL contain string keys and heterogeneous `Value` values. `DictionaryEntry<T>` SHALL expose `key: String` and `value: T`; entries of a heterogeneous dictionary SHALL have type `DictionaryEntry<Value>`. `Value` SHALL carry the supported scalar, list, dictionary, and table values and require runtime type checks when its contents are used as a specific type. `Date` SHALL represent a Gregorian calendar date, optionally naive, offset-bearing, or IANA-zoned; `Time` SHALL represent a wall-clock time with optional fractional seconds and the same zone choices; `DateTime` SHALL combine both. Serialized temporal inputs SHALL accept their ISO 8601 date/time representations and RFC 9557 `[Zone]` suffixes; invalid date/time, offset, zone, or conflicting zone forms SHALL fail validation. For `Time`, `24:00:00` SHALL be accepted and normalized to `00:00:00`, while values after `24:00:00` and leap seconds SHALL be rejected. `.bl` SHALL provide `date(...)`, `time(...)`, and `datetime(...)` constructors, with valid text or documented component/conversion arguments; constant invalid text SHALL fail at compilation and invalid runtime inputs SHALL report an evaluation error. `dateTime(...)` SHALL no longer be a constructor. Plain quoted text SHALL remain `String`. Comparison of two naive points SHALL use wall-clock order; comparison of two zoned datetimes SHALL use instants; zoned dates SHALL use the documented midnight projection, and zoned times SHALL compare at the evaluation's date in their respective zones. Mixed naive/zoned point comparisons SHALL fail rather than silently infer a zone. The language SHALL continue to use `==` and `!=` for ordinary equality; in an expression, `=` SHALL only bind a supported named function argument, introduce a table output column assignment, or compare two tables (or row dictionaries within a table-row context) as specified by `business-language-tables`. It SHALL NOT compare other values. At the source declaration level, `=` SHALL introduce a named dictionary schema. Explicitly nullable named dictionary fields SHALL accept null on input, and join/union-generated missing-side table cells SHALL retain null through projections and nullable expressions; neither SHALL introduce an authored `null` literal outside range endpoints or authored nullable list elements. `sum` SHALL ignore null numeric elements and return zero for an empty/all-null numeric list; `mean` SHALL ignore null numeric elements and return null for an empty/all-null numeric list. `coalesce(value, fallback)` SHALL provide a non-null value of the same underlying type. Invalid nullable-to-non-null assignments and unsupported nullable operations SHALL fail during `.bl` transpilation.

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
- **WHEN** a typed `DateTime` field receives `"2026-10-02T09:30:00+02:00"`, `"2026-10-02T09:30:00"`, or `"2026-10-02T09:30:00[Europe/Paris]"`
- **THEN** validation accepts each form; two zoned datetimes compare by UTC instant and two naive datetimes by wall clock

#### Scenario: Date and Time input and typed literals
- **WHEN** `Date` receives `"2026-10-02"` or `"2026-10-02+05:30"`, `Time` receives `"09:30:00.250"` or `"09:30:00+02:00"`, or an expression constructs equivalent values with `date(...)` and `time(...)`
- **THEN** the values validate as their declared temporal types, retaining the specified zone kind

#### Scenario: End of day is normalized
- **WHEN** a typed `Time` input or `time(...)` expression receives `"24:00:00"`
- **THEN** it yields `time("00:00:00")` without recording a day advance

#### Scenario: Invalid temporal values
- **WHEN** `Date` receives `"2026-02-30"`, `Time` receives `"24:00:01"` or a leap second, or a temporal constructor has invalid constant text
- **THEN** input validation fails or compilation fails, respectively, with a diagnostic

#### Scenario: Constructor and equality migration
- **WHEN** an expression uses `datetime("2026-10-02T08:30:00Z") == datetime("2026-10-02T09:30:00+01:00")`, `dateTime(...)`, or `a = b` for non-table values outside a table-row context
- **THEN** the first evaluates to `true` and the latter two fail validation

#### Scenario: Unsupported built-in type
- **WHEN** source declares a bare `Table` input or output, `Table<Number>`, `Range`, `Any`, or `Optional<T>`
- **THEN** validation fails with a diagnostic identifying the unsupported type; `Table<Order>` is supported when `Order` is a declared dictionary row schema

#### Scenario: Dictionary ports
- **WHEN** a task accepts `Dictionary<Number>` with JSON object `{"alice":"90","bob":"75"}` or `Dictionary` with JSON object `{"name":"Alice","age":30}`
- **THEN** input validation accepts the matching object and preserves its keys and supported value kinds (a dynamic JSON number is `Number`, a quoted JSON value is `String`); incompatible typed values and non-object inputs fail validation

#### Scenario: Nullable named dictionary fields
- **WHEN** `Order = {id: Number, price: Number?};` is declared and a task accepts an `Order` input with JSON object `{"id":"1","price":null}` or `{"id":"1","price":"20"}`
- **THEN** both values are accepted with the `price` field present, while a missing `price`, a null `id`, or a nonnumeric non-null `price` fails input validation

#### Scenario: Table type and scoped equality
- **WHEN** a `Table<Order>` decision output is constructed from rows matching `Order`, `t = t` is evaluated, or `some r in t satisfies r = row` is evaluated
- **THEN** the declared row shape and table-specific comparisons validate without enabling general `=` equality or changing dictionary/list conversion rules

#### Scenario: Empty and null-bearing numeric aggregates
- **WHEN** `sum` or `mean` consumes a numeric list produced from nullable values, including an empty or all-null list
- **THEN** null elements are ignored, `sum` returns `0` when none remain, and `mean` returns null when none remain; nullable results require an accepting output type or explicit `coalesce`

### Requirement: List iteration expressions can consume dictionary results
`every <name> in <list-or-table> satisfies <Bool-expression>` SHALL evaluate to `Bool` and `for <name> in <list-or-table> return <expression>` SHALL produce a `List<T>` in source order; a table SHALL expose its rows as dictionary bindings in table row order. They SHALL also allow comma-separated additional `name in list-or-table` bindings with Cartesian-product semantics; `some` SHALL use the same binding and predicate rules but return `true` on any match. Each bound name SHALL be visible only in its iteration body and in subsequent binding sources, and SHALL shadow an outer name only within that scope. An empty list or table SHALL make `every` return `true`, `some` return `false`, and `for` return `[]` with its element type inferred from the body or expected output type. Non-list/non-table sources and statically ill-typed predicates SHALL fail validation; errors from evaluated bodies SHALL propagate. Typed dictionary values/entries SHALL retain their value types so existing numeric list aggregates and entry projections work.

#### Scenario: Iterate values and entries
- **WHEN** `scores` is a `Dictionary<Number>` with keys `alice: 90` and `bob: 75`, and `every v in values(scores) satisfies v >= 50` and `for e in getEntries(scores) return e.value > 80` are evaluated
- **THEN** the expressions return `true` and `[true, false]` in key order, and `sum(values(scores))` returns `165`

#### Scenario: Cartesian map and quantifiers
- **WHEN** `.bl` evaluates `for x in [1,2], y in [3,4] return x * y`, `some x in [1,2], y in [3,4] satisfies x + y > 5`, and `every x in [1,2], y in [3,4] satisfies x + y >= 4`
- **THEN** the results SHALL be `[3,4,6,8]`, `true`, and `true`

#### Scenario: Empty list and validation
- **WHEN** an `every`, `some`, or `for` expression consumes an empty typed list in any binding, or an iteration source is a scalar or a predicate body is not `Bool`
- **THEN** `every` returns `true`, `some` returns `false`, `for` returns a typed empty list, and invalid expressions fail validation

#### Scenario: Iterate table rows
- **WHEN** `t` is a `Table` with two `rate` rows and `.bl` evaluates `for row in t return row.rate` and `some row in t satisfies row.rate > 10`
- **THEN** the first produces the two rates in order and the second tests the table's rows without converting the table to a list

### Requirement: Typed lists support positional access, filtering, and projection
Lists SHALL remain ordered, immutable `List<T>` values; literal elements SHALL share one type, and `null` SHALL remain invalid as a list element in authored literals. Lists projected from nullable table columns or produced from table joins or `union(..., "all")` SHALL be permitted to contain null cells without making `null` an authored list literal; their non-null element type and nullability SHALL be retained. `list[index]` SHALL use one-based integral positions, with `-1` denoting the last element; zero, nonintegral, and out-of-bounds positions SHALL report errors. `list[predicate]` SHALL return elements for which a `Bool` predicate holds, in original order; within its predicate only, leading `.item` SHALL refer to the current element. Leading `.item` outside a bracket filter predicate SHALL fail validation. Plain `item` SHALL remain an ordinary name (including decision nodes, context entries, input names and iteration bindings), and numeric positions supplied by it or any other valid name SHALL retain their normal dependencies. Ordinary field access such as `order.item` SHALL retain its meaning. `listOfDictionaries.field` SHALL return a list of that field's values in input order, while ordinary dictionary `.field` access SHALL retain its scalar behavior. Missing fields and ill-typed predicates SHALL fail validation when statically known, otherwise evaluation SHALL report an error; errors inside predicates SHALL propagate. Numeric `mod` SHALL be an infix operator with multiplication-level precedence and the same checked floor-remainder semantics as `modulo`, including zero-divisor errors. An empty list SHALL be typed from context where necessary.

#### Scenario: Index, filter, project
- **WHEN** `.bl` evaluates `[1,2,3,4][1]`, `[1,2,3,4][-2]`, `[1,2,3,4][.item > 2]`, and `[{name: "A"}, {name: "B"}].name`
- **THEN** it SHALL produce `1`, `3`, `[3,4]`, and `["A","B"]` respectively

#### Scenario: Project and summarize orders
- **WHEN** `orders` is `[{id: 1, amount: 100, quantity: 2, region: "NA"}, {id: 2, amount: 150, quantity: 1, region: "EU"}, {id: 3, amount: 75, quantity: 3, region: "NA"}]` of a declared single dictionary type
- **THEN** `sum(orders.amount)` SHALL be `325`, `distinct(orders.region)` SHALL be `["NA","EU"]`, `count(orders[.item.region == "NA"])` SHALL be `2`, and `sum(for o in orders return o.amount * o.quantity)` SHALL be `575`

#### Scenario: Filter using numeric remainder
- **WHEN** `.bl` evaluates `[1,2,3,4][.item mod 2 == 0]`
- **THEN** it SHALL return `[2,4]`; `mod` SHALL use the same checked numeric remainder semantics as `modulo` and reject a zero divisor

#### Scenario: Explicit filter element and ordinary names
- **WHEN** `.bl` evaluates `[1,2][odd(.item)]`, uses a Number-typed input or decision/context entry named `item` as a list index, accesses `order.item`, or evaluates `.item` outside a bracket filter
- **THEN** the filter SHALL return `[1]`, the ordinary names and field access SHALL retain their existing meanings, and the out-of-filter `.item` SHALL fail validation

#### Scenario: Single-type constraint and invalid positions
- **WHEN** a program uses `[1,"two"]`, `[1,null]`, `[1,2][0]`, or `[1,2][3]`
- **THEN** incompatible/null elements SHALL fail validation and invalid positions SHALL report errors rather than yield null or wrap

#### Scenario: Projected nullable table values
- **WHEN** a declared nullable table input or a table produced by a left join has a null `rate` cell and its `rate` column is projected or converted by `.toList()`
- **THEN** the resulting list retains a null in that position with its non-null numeric type, while authored `[null]` remains invalid
