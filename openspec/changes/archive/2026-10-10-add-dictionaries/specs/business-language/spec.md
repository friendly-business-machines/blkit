# Spec Delta

## MODIFIED Requirements

### Requirement: Domain types are declared in source
The language SHALL support named fixed-shape dictionary declarations of the form `Order = {total: Number, blocked: Bool};`, enum declarations, and typed list values using only `.bl` source-defined domain types and built-in types. A named dictionary SHALL use comma-separated identifier or quoted-string keys and field types, require its declared keys at typed boundaries, reject unknown keys and incompatible values, and be usable wherever a domain type is accepted. Its values SHALL use the same dictionary access, equality, and inspection operations as ad hoc dictionaries. A declaration using `type Order:` SHALL be invalid. Enum declarations SHALL retain their existing form. In a project build, a declaration MAY reference domain types defined in another file with the same namespace and process version. A standalone single-file compilation SHALL continue to resolve declarations from that file alone.

#### Scenario: Valid domain declarations
- **WHEN** a source declares `Order = {total: Number, tags: List<String>};` and an enum type
- **THEN** validation accepts the declarations when all referenced types exist in its compilation scope

#### Scenario: Shared domain type
- **WHEN** one `.bl` file defines `Order` and another file in the same project namespace and version uses `Order` in a process signature
- **THEN** project validation accepts the type regardless of source-file order

#### Scenario: Unknown type reference
- **WHEN** a declaration references a type that is neither built in nor declared in its compilation scope
- **THEN** validation fails with a diagnostic identifying the unknown type

#### Scenario: Old record declaration rejected
- **WHEN** a source contains `type Order:` followed by indented field declarations
- **THEN** compilation fails with a syntax diagnostic directing migration to `Order = { ... };`

#### Scenario: Named dictionary value checked
- **WHEN** an `Order = {total: Number, blocked: Bool};` port receives JSON `{"total":"1200","blocked":false}`, a JSON value missing `blocked`, or a JSON value with an extra `other` key
- **THEN** the complete value is accepted and the incomplete/extra-key values are rejected

### Requirement: MVP built-in types are fixed
The language SHALL provide the built-in user-facing types `Bool`, `String`, `Number`, `Date`, `DateTime`, `Time`, `DTDuration`, `YMDuration`, `Calendar`, `CalendarEntry`, `List<T>`, `Dictionary`, `Dictionary<T>`, `DictionaryEntry<T>`, and `Value`. `Dictionary<T>` SHALL contain string keys and values of one type `T`; `Dictionary` SHALL contain string keys and heterogeneous `Value` values. `DictionaryEntry<T>` SHALL expose `key: String` and `value: T`; entries of a heterogeneous dictionary SHALL have type `DictionaryEntry<Value>`. `Value` SHALL carry the supported scalar, list, and dictionary values and require runtime type checks when its contents are used as a specific type. `Date` SHALL represent a Gregorian calendar date, optionally naive, offset-bearing, or IANA-zoned; `Time` SHALL represent a wall-clock time with optional fractional seconds and the same zone choices; `DateTime` SHALL combine both. Serialized temporal inputs SHALL accept their ISO 8601 date/time representations and RFC 9557 `[Zone]` suffixes; invalid date/time, offset, zone, or conflicting zone forms SHALL fail validation. For `Time`, `24:00:00` SHALL be accepted and normalized to `00:00:00`, while values after `24:00:00` and leap seconds SHALL be rejected. `.bl` SHALL provide `date(...)`, `time(...)`, and `datetime(...)` constructors, with valid text or documented component/conversion arguments; constant invalid text SHALL fail at compilation and invalid runtime inputs SHALL report an evaluation error. `dateTime(...)` SHALL no longer be a constructor. Plain quoted text SHALL remain `String`. Comparison of two naive points SHALL use wall-clock order; comparison of two zoned datetimes SHALL use instants; zoned dates SHALL use the documented midnight projection, and zoned times SHALL compare at the evaluation's date in their respective zones. Mixed naive/zoned point comparisons SHALL fail rather than silently infer a zone. The language SHALL continue to use `==` and `!=` for equality; in an expression, `=` SHALL only bind a supported named function argument, never compare values. At the source declaration level, `=` SHALL introduce a named dictionary schema.

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
- **WHEN** an expression uses `datetime("2026-10-02T08:30:00Z") == datetime("2026-10-02T09:30:00+01:00")`, `dateTime(...)`, or `a = b`
- **THEN** the first evaluates to `true` and the latter two fail validation

#### Scenario: Unsupported built-in type
- **WHEN** source uses an unsupported built-in type such as `Table<T>`, `Range`, `Any`, or `Optional<T>`
- **THEN** validation fails with a diagnostic identifying the unsupported type

#### Scenario: Dictionary ports
- **WHEN** a task accepts `Dictionary<Number>` with JSON object `{"alice":"90","bob":"75"}` or `Dictionary` with JSON object `{"name":"Alice","age":30}`
- **THEN** input validation accepts the matching object and preserves its keys and supported value kinds (a dynamic JSON number is `Number`, a quoted JSON value is `String`); incompatible typed values and non-object inputs fail validation

### Requirement: Gateway conditions and joins are typed
The language SHALL support named, explicitly connected AND, OR, and XOR split and join gateways in a `.bl` process map. Each gateway SHALL use a kind-specific braced declaration; `flow` statements SHALL carry typed branch conditions, ordered fallbacks, and AND branch labels as applicable; typed `bind` statements SHALL carry required values separately. AND splits SHALL activate all outgoing branches; XOR splits SHALL activate the first matching branch in outgoing-flow declaration order; OR splits SHALL activate every matching branch. Conditions SHALL be `Bool` expressions that can reference start-event outputs and upstream task outputs available on every route to that gateway. XOR and OR splits SHALL provide a fallback when no condition matches. Joins SHALL account for the branches activated for that instance: AND waits for all incoming branches and combines their results into a declared named dictionary, XOR accepts the selected branch's result of a common type, and OR waits for every selected branch and produces a `List<T>` of compatible branch results in split-flow declaration order. Every route to a normal end event SHALL bind compatible values to its required input ports.

#### Scenario: Conditions use task output and input
- **WHEN** an XOR gateway's `flow` condition uses a completed decision-task output and a start-event output
- **THEN** validation accepts its `Bool` condition and selects exactly one branch

#### Scenario: Inclusive parallel routing
- **WHEN** two OR conditions match
- **THEN** both branches activate and the OR join waits for both, but not for inactive branches

#### Scenario: Invalid reference or route
- **WHEN** a condition uses an unavailable output or a normal end-event input cannot be bound on a reachable route
- **THEN** validation fails before generation

#### Scenario: Multiple XOR conditions match
- **WHEN** multiple XOR conditions match
- **THEN** the first matching outgoing flow in declaration order is selected

#### Scenario: Inclusive join output
- **WHEN** an OR gateway selects two branches of the same declared output type
- **THEN** its join produces a typed list in split-flow declaration order

### Requirement: String conversion, joining, and literal transformations are typed
The language SHALL provide `string(from)` for `String`, `Number`, `Bool`, `Date`, `Time`, and `DateTime`, producing text in each type's existing human-readable serialized form (Boolean `true`/`false`, decimal number text, and the documented temporal formats); it SHALL reject lists and dictionaries. `stringJoin(List<String>, String) -> String` SHALL place the separator between elements, returning `""` for an empty list. The following SHALL accept a `String` and return a `String`: `upperCase`, `lowerCase`, `trim`, `trimLeading`, `trimTrailing`, and `reverse`. Case changes SHALL use Unicode casing, trims SHALL remove Unicode whitespace only at the indicated edge(s), and reverse SHALL preserve each visible character's internal code-point order.

#### Scenario: Conversion and join
- **WHEN** `.bl` evaluates `"order-" + string(123)`, `stringJoin(["a", "b"], ", ")`, and `stringJoin([], ",")`
- **THEN** it SHALL produce `"order-123"`, `"a, b"`, and `""`

#### Scenario: Transformations use Unicode text
- **WHEN** `trim(" é ")`, `upperCase("é")`, and `reverse("éx")` are evaluated
- **THEN** they SHALL produce `"é"`, `"É"`, and `"xé"`, respectively, preserving the combining accent with `e`

### Requirement: Declarations and statements have explicit delimiters
A `.bl` source SHALL end every statement, including namespace/version, named dictionary schema, variant, port, expression, rule, `flow`, and `bind` statements, with `;`. A declaration or control block SHALL close with `}` rather than indentation or a terminating semicolon, except that a named dictionary schema SHALL use `Name = {field: Type, ...};` with comma-separated members and one trailing semicolon. Processes and peer graph nodes SHALL have braced bodies, including empty bodies. Decision-task bodies and their kind-specific decision nodes SHALL be braced. Enum declarations MAY retain their current colon/indentation layout and semicolon-terminated variants. The old `type Name:` declaration SHALL be rejected. Whitespace and newlines alone SHALL NOT terminate statements. A semicolon inside a quoted literal SHALL NOT terminate a statement.

#### Scenario: Braces and semicolons
- **WHEN** a source uses `namespace demo;`, `version "1.0";`, `Order = {total: Number, blocked: Bool};`, and `process p { flow start -> done; }` with braced peer nodes
- **THEN** the source passes delimiter validation

#### Scenario: Missing delimiter
- **WHEN** a `flow` lacks `;`, a braced declaration lacks `}`, a dictionary schema lacks its trailing `;`, or a declaration uses `type Order:`
- **THEN** compilation fails with a syntax diagnostic and emits no Rust

## ADDED Requirements

### Requirement: Dictionary literals and key access
`.bl` SHALL accept `{}` and comma-separated `{key: expression}` literals with identifier or quoted-string keys, including special-character keys. Dictionary literal entries SHALL be evaluated in declaration order; a later value expression MAY refer to an earlier identifier key in the same literal, and a duplicate key SHALL fail validation. Heterogeneous or nested literals SHALL be usable as `Dictionary` values, while homogeneous literals MAY be used as `Dictionary<T>` when all values match `T`. A literal supplied as a named dictionary value SHALL match the named schema's required keys and their types; an inferred anonymous literal SHALL NOT silently acquire an unrelated named identity. `d.key` and `d["key"]` SHALL access string-keyed dictionary values; bracket keys MAY be runtime `String` expressions. Dot and bracket access SHALL chain through nested dictionaries and named dictionary values. Statically invalid fields on named dictionary types SHALL remain compilation errors; missing dictionary keys, attempts to traverse a non-dictionary value, or invalid dynamic value operations SHALL report evaluation errors, not silently return null.

#### Scenario: Literals and dependent entries
- **WHEN** `{name: "Alice", age: 30}`, `{"my key": 1}`, `{a: 1, b: {c: 2}}`, and `{a: 2, b: a * 2}` are evaluated
- **THEN** the values include their declared keys, and the last expression has `a = 2` and `b = 4`

#### Scenario: Nested and quoted key access
- **WHEN** `{a: {b: 3}}.a.b`, `applicant.address.postcode`, and `applicant["my key"]` are evaluated against matching dictionary inputs
- **THEN** the values at those keys are returned, including `3` for the literal path

#### Scenario: Invalid dictionary access
- **WHEN** a dictionary literal repeats a key, a dynamic lookup uses a missing key, or a path attempts to traverse a scalar
- **THEN** duplicate keys fail compilation and invalid runtime paths report an evaluation error

### Requirement: Dictionary equality, inspection, and transformations
Dictionary `==` and `!=` SHALL compare keys and nested values structurally without considering insertion order. `getValue(dictionary, key)` and `getValue(dictionary, [key1, ...])` SHALL retrieve a scalar or nested path; `getEntries(dictionary)` SHALL return entries containing `.key` and `.value`. `keys(dictionary)` SHALL return `List<String>` in Unicode code-point order; `values(dictionary)` and `getEntries(dictionary)` SHALL use that same order. `has(dictionary, key)` SHALL check direct key presence; `size(dictionary)` SHALL return a `Number` count, and `isEmpty(dictionary)` SHALL test whether there are any entries without changing its existing String or Calendar overloads. `dictionaryPut(dictionary, key-or-path, value)` SHALL return a new dictionary with that key/path replaced or inserted; `dictionaryMerge(List<Dictionary>)` SHALL return a new shallow union with later inputs winning on duplicate keys; `dictionaryRemove(dictionary, key)` SHALL return a new dictionary without that direct key. Nested put paths SHALL be nonempty and require every intermediate key to exist and contain a dictionary. Missing get paths or invalid intermediate put paths SHALL report evaluation errors. All transformations SHALL leave their inputs unchanged. They SHALL also accept named dictionary inputs; a transformation that could add or remove fields SHALL return a dictionary value rather than claiming the named fixed shape still holds. For a named heterogeneous dictionary, `values` SHALL return `List<Value>` and `getEntries` SHALL return `List<DictionaryEntry<Value>>`.

#### Scenario: Structural equality
- **WHEN** `{a: 1, b: 2} == {b: 2, a: 1}` and `{a: 1} != {a: 2}` are evaluated
- **THEN** both return `true`

#### Scenario: Lookup and sorted inspection
- **WHEN** `getValue({foo: 123}, "foo")`, `getValue({x: 1, y: {z: 0}}, ["y", "z"])`, `getEntries({foo: 123})`, `keys({b: 2, a: 1})`, and `values({b: 2, a: 1})` are evaluated
- **THEN** they yield `123`, `0`, `[{key: "foo", value: 123}]`, `["a", "b"]`, and `[1, 2]`, respectively

#### Scenario: Presence and size
- **WHEN** `has({a: 1}, "a")`, `size({a: 1, b: 2})`, and `isEmpty({})` are evaluated
- **THEN** they return `true`, `2`, and `true`

#### Scenario: Named dictionaries use dictionary operations
- **WHEN** `Order = {total: Number, blocked: Bool};` is declared and an `Order` value is used with `.total`, `keys`, `getValue`, and `dictionaryPut`
- **THEN** `.total` and a literal-key `getValue` yield `Number`, `keys` yields sorted keys, and `dictionaryPut` yields a new dictionary without changing the original `Order`

#### Scenario: Non-mutating updates
- **WHEN** `dictionaryPut({x: 1}, "y", 2)`, `dictionaryPut({x: 1, y: {z: 0}}, ["y", "z"], 2)`, `dictionaryMerge([{x: 1}, {y: 2}])`, and `dictionaryRemove({a: 1, b: 2}, "a")` are evaluated
- **THEN** they return `{x: 1, y: 2}`, `{x: 1, y: {z: 2}}`, `{x: 1, y: 2}`, and `{b: 2}` without mutating the original values

#### Scenario: Invalid paths and arguments
- **WHEN** `getValue({a: 1}, "missing")`, `dictionaryPut({a: 1}, ["a", "b"], 2)`, or `getValue({a: 1}, [])` is evaluated
- **THEN** each reports an evaluation error; invalid argument types are rejected at compilation when known

### Requirement: List iteration expressions can consume dictionary results
`every <name> in <list> satisfies <Bool-expression>` SHALL evaluate to `Bool` and `for <name> in <list> return <expression>` SHALL produce a `List<T>` in source-list order. Each bound name SHALL be visible only in its iteration body and SHALL shadow an outer name only within that body. An empty list SHALL make `every` return `true` and `for` return `[]` with its element type inferred from the body or expected output type. Non-list sources and statically ill-typed predicates SHALL fail validation; errors from evaluated bodies SHALL propagate. Typed dictionary values/entries SHALL retain their value types so existing numeric list aggregates and entry projections work.

#### Scenario: Iterate values and entries
- **WHEN** `scores` is a `Dictionary<Number>` with keys `alice: 90` and `bob: 75`, and `every v in values(scores) satisfies v >= 50` and `for e in getEntries(scores) return e.value > 80` are evaluated
- **THEN** the expressions return `true` and `[true, false]` in key order, and `sum(values(scores))` returns `165`

#### Scenario: Empty list and validation
- **WHEN** an `every` or `for` expression consumes an empty typed list, or an iteration source is a scalar or an `every` body is not `Bool`
- **THEN** `every` returns `true`, `for` returns a typed empty list, and the invalid expressions fail validation
