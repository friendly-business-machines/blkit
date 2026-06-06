---
name: BlExpr
description: The blkit expression language — a string-based expression syntax (based on DMN FEEL) parsed and evaluated into Bl* values. This is the hub spec covering the engine API, operators, control flow, unary tests, lexical rules, semantics, and the Go layer that extends expr-lang/expr; each data type's literals, functions, and Go implementation are detailed in its own spoke spec.
targets:
  - ../../expr/engine.go
  - ../../expr/expr.go
---

# The blkit expression language

This spec defines **blkit's expression language**: a compact, business-readable syntax for writing
expressions as text — `age >= 18 and income > 50000`, `sum(lineItems.amount)`,
`if score >= 700 then "approve" else "review"` — that evaluate to blkit value types (`BlNumber`,
`BlString`, `BlList`, …, each documented in its own spoke spec; see [§ Data types](#data-types)).

Expressions are **strings**, parsed and evaluated at runtime — there is no programmatic
expression-builder API. The language is based on **DMN 1.4 FEEL** (Friendly Enough Expression
Language) and reproduces its syntax and semantics as the v1 baseline. The runtime is built on
[`github.com/expr-lang/expr`](https://github.com/expr-lang/expr), which blkit extends with its own
value types, functions, and operators — see [§ Engine internals](#engine-internals-go).

This document is the **hub**: it specifies the engine API, operators, control flow, unary tests,
lexical rules, semantics, and the shared Go extension layer. Each data type's literals, type-specific
functions, and Go implementation live in that type's **spoke** spec (e.g. [number.spec.md](number.spec.md),
[string.spec.md](string.spec.md)).

Throughout, examples use the notation `expression  // → result`, where the result is shown in the
same source syntax (e.g. a `BlNumber` renders as `42`, a `BlString` as `"foo"`).

---

## Using the engine

```go
// host-side (Go)
// Expr compiles a source string once, optionally type-checking it against a
// declared environment. The returned BlExpr can be evaluated repeatedly.
func (Bl) Expr(source string, env BlEnv) (BlExpr, error)

// BlEnv declares the variable names and types an expression may reference, so
// type errors surface at parse time. Pass nil to skip static checking.
type BlEnv map[string]BlType

// BlExpr is a compiled source-text expression.
type BlExpr interface {
    Evaluate(input map[string]any) (BlValue, error)
    Source() string
}
```

A `BlExpr` is a compiled expression: parse a source string once with `Bl.Expr`, then `Evaluate` it
repeatedly. Input variables are supplied as **native Go values** (`int`, `float64`, `string`, `bool`,
slices, maps, `time.Time`, …) which the engine wraps into the corresponding `Bl*` value; the result
is a `BlValue` (see [§ Engine internals](#engine-internals-go) for the bridging rules and host
accessors). There are no `Bl.Number(…)`-style expression factories.

```go
// host-side (Go)
var env = BlEnv{"age": BlTypeNumber, "income": BlTypeNumber}
var eligible, _ = Bl.Expr("age >= 18 and income > 50000", env)

var result, _ = eligible.Evaluate(map[string]any{
    "age":    21,
    "income": 60000,
})
// result is the BlBoolean true
```

`[@test] ../../expr/engine_test.go`

---

## Data types

Every value belongs to one of the following types. Each has a literal or constructor syntax.

| Type | `Bl*` type | Literal / constructor | Example |
|---|---|---|---|
| number | `BlNumber` | digits, optional `.` and `-` | `42`, `3.14`, `-5`, `1500.50` |
| string | `BlString` | double quotes | `"hello"` |
| boolean | `BlBoolean` | keywords | `true`, `false` |
| date | `BlDate` | `date(...)` | `date("2025-03-28")` |
| time | `BlTime` | `time(...)` | `time("11:45:30")`, `time("11:45:30+02:00")` |
| date-time | `BlDateTime` | `datetime(...)` | `datetime("2025-03-28T11:45:30")` |
| days-time duration | `BlDaysTimeDuration` | `dtDuration(...)` | `dtDuration("P4DT12H")` |
| years-months duration | `BlYearsMonthsDuration` | `ymDuration(...)` | `ymDuration("P1Y6M")` |
| list | `BlList` | `[ ... ]` | `[1, 2, 3]` |
| dictionary | `BlDictionary` | `{ ... }` | `{name: "Alice", age: 30}` |
| range | `BlRange` | interval notation | `[1..10]`, `(1..10)`, `[1..10)` |
| null | `BlNull` | keyword | `null` |

- **Numbers** are arbitrary-precision decimals — never floats; precision is preserved through all
  arithmetic (see [number.spec.md](number.spec.md)).
- The blkit-specific types `BlCalendar` ([calendar.spec.md](calendar.spec.md)) and `BlTable`
  ([table.spec.md](table.spec.md)) have **no literal syntax** in v1; they are produced
  programmatically and may be referenced as variables.

`[@test] ../../expr/data_types_test.go`

### Numbers

```
// expression-language
42            // → 42
3.14          // → 3.14
-5            // → -5
1500.50       // → 1500.5
```

Scientific notation (`1.2e3`) is also accepted. Hexadecimal literals (`0xff`) are a Drools/KIE
vendor extension and are **not** supported.

### Strings

String literals use double quotes. Strings are immutable Unicode sequences
([string.spec.md](string.spec.md)).

```
// expression-language
"hello"               // → "hello"
"line1\nline2"        // → two lines
"quote: \""           // → quote: "
```

### Temporal values

Temporal values are created with constructor functions, accepting either an ISO 8601 string or
numeric components (see [§ Conversion functions](#conversion-functions)).

```
// expression-language
date("2025-03-28")                       // → a date
date(2025, 3, 28)                        // → a date
time("11:45:30+02:00")                   // → a time with offset
datetime("2025-03-28T11:45:30")          // → a date-time
dtDuration("P4DT12H")                      // → days-time duration (4 days, 12 hours)
ymDuration("P1Y6M")                        // → years-months duration (1 year, 6 months)
```

---

## Variables

A bare name references an input variable. Names follow conventional identifier rules — letters,
digits, and underscores, **no spaces** — and are case-sensitive. (This is a deliberate divergence
from FEEL, which allows multi-word names with embedded spaces; it also applies to built-in function
names. See [§ Relationship to FEEL](#relationship-to-feel-and-future-direction).)

```
// expression-language
age                   // value of the `age` variable
loanAmount * 12       // identifier used in arithmetic
```

Variables are supplied at evaluation time via the `input` map and, when an expression is compiled
with a `BlEnv`, are type-checked at parse time. A reference to a name that is neither in scope nor
declared is a parse error (see [§ Errors and null](#errors-and-null)).

`[@test] ../../expr/variables_test.go`

### Name resolution: `isDefined`

`isDefined(x)` is a built-in that reports whether the engine could resolve `x` to a value. It
returns `true` when `x` resolves (including when it resolves to `BlNull`) and `false` only when
the name is unbound at evaluation time. It is the only way for an expression to distinguish "the
caller supplied this name with a null value" from "the caller supplied no value at all."

```
// expression-language
isDefined(applicant)             // → true if `applicant` is bound, even if its value is null
isDefined(applicant.middleName)  // → true (path access on a bound dictionary always resolves;
                                 //   a missing key resolves to BlNull, which is still "defined")
isDefined(undeclaredName)        // → false (only when there is no binding)
```

`isDefined` operates at the resolution layer, not the value layer — to test whether a value *is*
`BlNull` once resolved, use `isNull(x)` ([null.spec.md § Testing for null](null.spec.md#testing-for-null)).
To supply a fallback when a value resolves to `BlNull`, use `getOrElse(x, default)`
([null.spec.md § Default for null](null.spec.md#default-for-null)).

Because `isDefined` distinguishes "unbound name" from "bound to anything (including `BlNull`)", it
cannot be expressed as a normal `BlValue → BlValue` impl — by the time a normal impl runs, the
argument has already been resolved and unbound names are a parse error. Instead, the engine's AST
patcher (see [§ Patchers](#patchers-ast-rewriting)) intercepts `isDefined(name)` calls before
resolution and rewrites them to a lookup against the input map plus declared `BlEnv` bindings.
The impl is registered in `engine.go` alongside the other engine-level options.

`[@test] ../../expr/is_defined_test.go`

---

## Arithmetic

The operators `+`, `-`, `*`, `/`, `**` (exponent), and unary `-` operate on numbers, with decimal
precision preserved. `+` and `-` also apply to temporal/duration combinations.

```
// expression-language
2 + 3                              // → 5
10 - 4                             // → 6
3 * 4                              // → 12
10 / 4                             // → 2.5
2 ** 8                             // → 256
-(7)                               // → -7

date("2025-01-01") + ymDuration("P1Y")    // → date("2026-01-01")
dtDuration("P1D") + dtDuration("PT12H")      // → dtDuration("P1DT12H")
```

`null` propagates: `null + 1 // → null`. Division by zero yields `null`.

`[@test] ../../expr/arithmetic_test.go`

---

## Comparison

The operators `<`, `<=`, `>`, `>=`, `=`, `!=` compare numbers, strings, and temporal/duration
values; `=` and `!=` apply to all types. `x between a and b` is shorthand for `x >= a and x <= b`.
The result is a `BlBoolean` (or `null` when operands are incomparable).

```
// expression-language
5 < 10                             // → true
10 >= 10                           // → true
"abc" = "abc"                      // → true
date("2025-01-01") < date("2025-06-01")   // → true
5 between 1 and 10                 // → true
```

`[@test] ../../expr/comparison_test.go`

---

## Boolean logic

`and`, `or`, and `not(...)` use **three-valued logic** — `true`, `false`, and `null` — matching
[boolean.spec.md](boolean.spec.md) and [null.spec.md](null.spec.md). `and`/`or` short-circuit so a
definite result is returned even when the other operand is `null`.

```
// expression-language
true and false                    // → false
true or false                     // → true
not(true)                         // → false

false and null                    // → false   (short-circuit)
true or null                      // → true    (short-circuit)
true and null                     // → null
null or false                     // → null
```

`[@test] ../../expr/boolean_logic_test.go`

---

## String expressions

Strings concatenate with `+`. Concatenation is **string-only**: to join a non-string value, convert
it first with `string(...)`.

```
// expression-language
"foo" + "bar"                     // → "foobar"
"order-" + string(123)            // → "order-123"
```

Inspection and transformation use the [§ String functions](#string-functions).

```
// expression-language
upperCase("aBc")                 // → "ABC"
contains("foobar", "oob")         // → true
substring("foobar", 3, 2)         // → "ob"
```

`[@test] ../../expr/string_expressions_test.go`

---

## Conditional

`if c then a else b` evaluates `a` when `c` is `true`, otherwise `b`. A `null` or non-boolean
condition takes the `else` branch. The two branches may have different types; the result type is
their union.

```
// expression-language
if 5 < 10 then "low" else "high"           // → "low"
if 12 < 10 then "low" else "high"          // → "high"
if null then "low" else "high"             // → "high"

if age >= 18 then "adult" else "minor"     // depends on `age`
```

Conditionals nest:

```
// expression-language
if score >= 750 then "prime"
else if score >= 650 then "standard"
else "subprime"
```

`[@test] ../../expr/conditional_test.go`

---

## Ranges and intervals

A range is a bounded interval with open `( )` or closed `[ ]` boundaries on each side. Ranges are
used for membership tests (with `in`) and the [§ Range functions](#range-functions). See
[range.spec.md](range.spec.md).

```
// expression-language
[1..10]      // 1 to 10, both inclusive
(1..10)      // 1 to 10, both exclusive
[1..10)      // 1 inclusive, 10 exclusive
(1..10]      // 1 exclusive, 10 inclusive
```

Ranges work over numbers and ordered temporal values:

```
// expression-language
[date("2025-01-01")..date("2025-12-31")]
```

`[@test] ../../expr/ranges_test.go`

---

## Sequences: the `:` operator

`start:end` materialises a numeric **`BlList`** running from `start` to `end` inclusive in
steps of `1` — a strict-list counterpart to the range `[start..end]` (which stays as a
`BlRange` for containment / interval-algebra purposes). The shorthand is sugar for
`seq(start, end, 1)`; the parser lowers `start:end` to `seq(start, end, 1)` at patch time, so
the two forms are exactly equivalent. For a non-default step (or for clarity in dense
expressions), call `seq(start, end, step)` explicitly. Full semantics — auto-reverse on
`start > end`, fractional steps, the zero-step rejection — live in
[list.spec.md § Sequence constructor](list.spec.md#sequence-constructor-seq-and-the--operator).

```
// expression-language
5:10                 // → [5, 6, 7, 8, 9, 10]
10:5                 // → [10, 9, 8, 7, 6, 5]    (auto-reversed)
1.5:3.5              // → [1.5, 2.5, 3.5]        (fractional)
-2:2                 // → [-2, -1, 0, 1, 2]      (unary minus binds tightest, so this is (-2):2)
seq(0, 1, 0.25)      // → [0, 0.25, 0.5, 0.75, 1]   (non-default step requires seq)
```

**Precedence.** `:` binds tighter than `+` / `-` but looser than `*` / `/`, so
`1+2:5*2` parses as `(1+2):(5*2)` → `[3, 4, 5, 6, 7, 8, 9, 10]`. It matches the position of
the range constructor `..` in the precedence ladder (see [§ Operator
precedence](#operator-precedence)).

**Dictionary literals.** `:` is also the dictionary key/value separator inside `{...}`. To
avoid ambiguity, a bare sequence `start:end` is **not allowed in dictionary value
positions** — the first `:` after a key is always the dict separator, and any further `:`
inside that value is a parse error. Use `seq(start, end)` (or parenthesise:
`{a: (5:10)}`) when you need a sequence as a dictionary value.

```
// expression-language
{a: seq(5, 10)}              // OK — explicit function call
{a: (5:10)}                  // OK — parenthesised sequence expression
{a: 5:10}                    // BlParseError — bare `:` in a dict value position
```

`[@test] ../../expr/sequences_test.go`

---

## Membership: the `in` operator

`x in y` tests whether `x` is a member of a list, falls within a range, or is covered by a
calendar.

```
// expression-language
5 in [1, 2, 3, 4, 5]               // → true
3 in [1..10]                       // → true
10 in [1..10)                      // → false   (upper bound exclusive)
"US" in ["US", "CA", "MX"]         // → true
date("2025-12-25") in ukHolidays   // → true   (calendar membership; see calendar.spec.md)
```

For a `BlCalendar` right operand, `point in calendar` is **patcher-lowered** to
`contains(calendar, point)` and inherits its semantics — see
[calendar.spec.md § Operators](calendar.spec.md#operators) for the zone-kind and
cross-temporal-kind rules. The left operand for calendar membership must be a `BlDate` or
`BlDateTime`; a range left operand → `BlTypeError` (use the explicit
`overlaps(c, r)` / `entriesIn(c, r)` for range-on-calendar queries).

`[@test] ../../expr/membership_test.go`

---

## Unary tests

A **unary test** is a condition evaluated against an implicit input value. Unary tests are how
decision-table input entries are written (see
[decision-table.spec.md](../decision-tasks/decision-table.spec.md)); the tested column value is
supplied implicitly.

| Form | Matches when input… | Example |
|---|---|---|
| value | equals the value | `"valid"` |
| `<` `<=` `>` `>=` | compares true | `< 10`, `>= date("2020-04-06")` |
| interval | falls in the range | `[2..5]`, `(2..5]` |
| comma list | equals any (disjunction) | `2, 3, 4`, `< 10, > 50` |
| `not(...)` | does **not** match the inner test | `not("valid")`, `not(2, 3)` |
| `?` expression | the expression (with `?` = input) is true | `contains(?, "good")`, `endsWith(?, "@blkit.io")` |
| `-` | always (wildcard) | `-` |

```
// expression-language
< 10                  // input < 10
[18..65]              // 18 <= input <= 65
"low", "medium"       // input = "low" or input = "medium"
not(0)                // input != 0
contains(?, "urgent") // the input string contains "urgent"
-                     // matches anything
```

### The `?` placeholder

Inside a unary test, `?` is the **implicit input** — the value the test is being evaluated
against. Simple tests like `< 10` or `"valid"` use the input implicitly (the engine inserts
the comparison or equality), but when the test body needs to call a built-in on the input,
`?` is the addressable name:

```
// expression-language
contains(?, "urgent")               // input.contains("urgent")
endsWith(?, "@blkit.io")            // input.endsWith("@blkit.io")
?.year >= 2025                      // input is a date; check its year
isPublicHoliday(?, ukHolidays)      // input is a date; check against a calendar
```

For implicit-input forms (`< 10`, `"valid"`, `[18..65]`, etc.), writing `?` explicitly is
redundant — `< 10` and `? < 10` are equivalent. Use `?` when the input needs to be passed as
a function argument or accessed via a path / component.

### Host-side usage

Host Go code compiles a unary test once and evaluates it against many inputs — typical
pattern when a decision-table row's cells are checked against repeated input data. The host
API mirrors `Bl.Expr` but adds an `inputType` parameter so the engine can
type-check the test body at parse time (e.g. `< 10` is only valid when the input is a
`BlNumber`).

```go
// host-side (Go)

// Bl.UnaryTest compiles a unary-test source string. inputType is the type the
// implicit ? will hold at evaluation time; the engine uses it to type-check the
// test body. Pass BlTypeAny for inputs whose type isn't known statically (e.g.
// when the test must accept the wildcard "-" against any value).
func (Bl) UnaryTest(source string, inputType BlType) (BlUnaryTest, error)

// BlUnaryTest is a compiled unary-test expression, evaluated repeatedly against
// different inputs.
type BlUnaryTest interface {
    Test(input BlValue) (BlBoolean, error)   // → true / false / null
    Source() string                          // the original source string
}
```

Worked example:

```go
// host-side (Go) — compile-once, test-many.
var atLeast18, _ = Bl.UnaryTest(">= 18", BlTypeNumber)
var isUrgent,  _ = Bl.UnaryTest(`contains(?, "urgent")`, BlTypeString)
var inRange,   _ = Bl.UnaryTest("[18..65]", BlTypeNumber)
var wildcard,  _ = Bl.UnaryTest("-", BlTypeAny)

// Test against typed inputs. The result is a BlBoolean (true / false) on success,
// or BlNull if the comparison would propagate null (e.g. numeric ordering against
// a missing operand — see null.spec.md § Propagation).
var n21, _    = Number(21)
var ok,  _    = atLeast18.Test(n21)                              // → BlBoolean(true)
var noteIn    = String("urgent notice")
var ok2, _    = isUrgent.Test(noteIn)                            // → BlBoolean(true)
var n70, _    = Number(70)
var ok3, _    = inRange.Test(n70)                                // → BlBoolean(false)
var ok4, _    = wildcard.Test(n70)                               // → BlBoolean(true) — wildcard matches anything
var ok5, _    = wildcard.Test(Null())                            // → BlBoolean(true) — even Null
```

The fallible cases:

- **Parse-time errors** — `Bl.UnaryTest` returns `(nil, BlParseError)` if the source string
  isn't a valid unary test. Common causes: unknown identifier, malformed range, type mismatch
  between the test body and the declared `inputType` (e.g. `>= 18` with `BlTypeString`).
- **Evaluation-time errors** — `Test(input)` returns `(zero-value, BlTypeError)` if the
  supplied `input`'s type doesn't match the compiled `inputType`. This shouldn't happen with
  correct host code, but the runtime check catches mismatches (e.g. an input wrongly bridged
  from a `map[string]any` value).
- **`BlNull` results** — a test against a null input returns `BlNull`, not an error.
  Wildcards (`-`) explicitly return `true` for null; all other tests propagate null per the
  standard rules (see [null.spec.md](null.spec.md)).

The decision-table runtime ([decision-table.spec.md](../decision-tasks/decision-table.spec.md))
uses this API internally: at table-construction time, each cell's unary-test source is
compiled once via `Bl.UnaryTest` and the resulting `BlUnaryTest` is cached on the cell. At
evaluation time, the column-input value is fed through `Test(input)`, and the per-cell
booleans are combined per the table's hit policy.

**Relationship to `Bl.Expr`.** `Bl.UnaryTest` is internally implemented on top of the same
`expr-lang/expr` pipeline `Bl.Expr` uses, with two extra steps in front: a **source
normaliser** rewrites the unary-test forms into ordinary expressions referencing `?` (e.g.
`< 10` → `? < 10`, `2, 3` → `? = 2 or ? = 3`, `-` → `true`, `[18..65]` → `? in [18..65]`,
`contains(?, "urgent")` left as-is), and a **typed environment** of `BlEnv{"?": inputType}`
is supplied to the parse / patch / type-check / compile chain. The result is functionally a
`BlExpr` whose evaluation receives `?` from `Test(input)` rather than from a host
`map[string]any`. The separate public entry point exists so the test grammar (the
left-implicit and comma-disjunction forms above) doesn't have to be valid plain-expression
syntax — those forms only parse in unary-test mode.

`[@test] ../../expr/unary_tests_test.go`

---

## Lists

### Literals and indexing

Lists are ordered and heterogeneous. Indexing is **1-based**; negative indexes count from the end;
an out-of-range index yields `null`.

```
// expression-language
[1, 2, 3, 4]                       // → [1, 2, 3, 4]
[1, 2, 3, 4][1]                    // → 1     (first element)
[1, 2, 3, 4][-1]                   // → 4     (last element)
[1, 2, 3, 4][5]                    // → null  (out of range)
[[1, 2], [3, 4]]                   // nested lists
```

### Filtering

`a[c]` filters list `a` by condition `c`. Inside the filter, the current element is bound to
`item`.

```
// expression-language
[1, 2, 3, 4][item > 2]             // → [3, 4]
[1, 2, 3, 4][even(item)]           // → [2, 4]
```

### Projection

Accessing a field on a list of dictionaries projects that field across every element.

```
// expression-language
[{name: "Alice", age: 30}, {name: "Bob", age: 34}].name   // → ["Alice", "Bob"]
[{name: "Alice", age: 30}, {name: "Bob", age: 34}].age     // → [30, 34]
```

List operations are covered by the [§ List functions](#list-functions).

`[@test] ../../expr/list_expressions_test.go`

---

## Loops: `for … return`

`for x in xs return e` evaluates `e` once per element of `xs`, collecting the results into a new
list.

```
// expression-language
for x in [1, 2, 3] return x * 2          // → [2, 4, 6]
```

**Multiple iterators** pair each element of the first list with each element of the second
(every combination), iterating the rightmost fastest:

```
// expression-language
for x in [1, 2], y in [3, 4] return x * y    // → [3, 4, 6, 8]
```

A loop may iterate a numeric range:

```
// expression-language
for x in 0..8 return 2 ** x              // → [1, 2, 4, 8, 16, 32, 64, 128, 256]
```

The keyword **`partial`** refers to the list of results accumulated so far, enabling running
computations:

```
// expression-language
for i in 1..10 return if i <= 2 then 1 else partial[-1] + partial[-2]
// → [1, 1, 2, 3, 5, 8, 13, 21, 34, 55]   (Fibonacci)
```

Each loop result is a `BlList`.

`[@test] ../../expr/for_test.go`

---

## Quantified expressions: `some` / `every`

`some x in xs satisfies c` is `true` when `c` holds for at least one element; `every x in xs
satisfies c` is `true` when `c` holds for all of them.

```
// expression-language
some x in [1, 2, 3] satisfies x > 2           // → true
every x in [1, 2, 3] satisfies x >= 1         // → true
every x in [1, 2, 3] satisfies x > 2          // → false

some order in orders satisfies order.total > 1000
```

**Multiple iterators** pair each element of the first list with each element of the second
(same as `for`), and the condition is tested against every combination:

```
// expression-language
some x in [1, 2], y in [3, 4] satisfies x + y > 5     // → true   (2 + 4 > 5)
every x in [1, 2], y in [3, 4] satisfies x + y >= 4   // → true   (every pair has sum ≥ 4)
```

`[@test] ../../expr/quantified_test.go`

---

## Dictionaries

A dictionary is an ordered map of named entries. Keys may be unquoted names or strings; values may be
any type. Later entries can reference earlier ones in the same literal.

```
// expression-language
{}                                 // → empty dictionary
{a: 1, b: 2}                       // → {a: 1, b: 2}
{"a": 1, "b": 2}                   // → {a: 1, b: 2}   (quoted keys)
{a: 1, b: {c: 2}}                  // → nested dictionary
{a: 2, b: a * 2}                   // → {a: 2, b: 4}   (b references a)
```

### Path access

The dot operator navigates into a dictionary. Chains traverse nested dictionaries; a missing key yields
`null`.

```
// expression-language
{a: 2}.a                           // → 2
{a: {b: 3}}.a.b                    // → 3
{a: 1}.b                           // → null
applicant.address.postcode         // navigate input variables
```

See [dictionary.spec.md](dictionary.spec.md). Dictionary manipulation uses the
[§ Dictionary functions](#dictionary-functions).

`[@test] ../../expr/dictionaries_test.go`

---

## Accessing components

The dot operator also reads named **components** of temporal, duration, and range values — not just
dictionary entries. This is the standard FEEL way to pull a field out of a date, time, duration, or
range.

```
// expression-language
date("2025-03-28").year            // → 2025
date("2025-03-28").month           // → 3
date("2025-03-28").day             // → 28
date("2025-03-28").weekday         // → 5    (Friday; Monday = 1)

time("11:45:30+02:00").hour        // → 11
time("11:45:30+02:00").minute      // → 45
time("11:45:30+02:00").offset // → dtDuration("PT2H")

dtDuration("P1DT2H3M4S").days        // → 1
dtDuration("P1DT2H3M4S").hours       // → 2
dtDuration("P1DT2H3M4S").minutes     // → 3
ymDuration("P1Y6M").years            // → 1
ymDuration("P1Y6M").months           // → 6

[1..10].start                      // → 1
[1..10].end                        // → 10
[1..10).endIncluded               // → false
```

Available components follow the relevant `Bl*` type spec (e.g. [date.spec.md](date.spec.md),
[range.spec.md](range.spec.md)).

`[@test] ../../expr/components_test.go`

---

## Function invocation

Functions (built-ins or in-scope functions) are invoked with **positional** or **named**
arguments.

```
// expression-language
substring("foobar", 3, 2)                                  // positional → "ob"
substring(string: "foobar", startPosition: 3, length: 2)  // named → "ob"
```

### User-defined functions

FEEL inline functions have the form `function(params) body`:

```
// expression-language
function(x, y) x + y
sort([3, 1, 2], function(a, b) a < b)      // → [1, 2, 3]
```

Inline functions are a v1 feature; the engine's `BlFunc` value type is consumed by every
higher-order built-in in the library (`sort`, the predicate forms of `remove` /
`listReplace`, etc.). To keep the semantics simple and the engine surface auditable, blkit
restricts the form to a minimal subset of what FEEL allows:

- **Anonymous only.** The syntactic form is always `function(params) body`. There is no
  named-function-definition form (no `let f = function(...) ...`, no top-level `fun`
  declarations) — FEEL itself doesn't have one. Functions exist purely as anonymous values
  inside expressions.
- **First-class as values, but not addressable by name.** A function value can be passed as
  an argument to a higher-order built-in, stored as a dictionary value, or returned from an
  `if`/`for`/`some` expression. It cannot be looked up by name from outside the expression
  (host code that wants a callable should register an `expr.Function` instead — see [§
  Registering built-in functions](#registering-built-in-functions)).
- **No recursion.** The body cannot reference the function itself — there's no name to refer
  to, since the function is anonymous, and the engine does not insert an implicit
  self-reference. For recursive-style computation, use the bounded `for i in 1..n return …`
  form ([§ Loops: for … return](#loops-for--return)), which accumulates results
  iteratively and inherits the engine's step limit.
- **Pure, bounded execution.** The body sees only its parameters and the surrounding lexical
  scope; it has no I/O, no access to host state, no mutable references. The function shares
  the engine VM's existing step / stack / recursion limits (see [§ Environment &
  errors](#environment--errors)), so a runaway body terminates with a `BlEvalError` rather
  than hanging.

These constraints fall out naturally from `expr-lang/expr`'s sandboxed VM model — the engine
already disallows I/O, host calls, and arbitrary recursion at the VM layer. The restrictions
above are stated for clarity at the language-spec level so callers understand the surface
they're getting, not to add engine work.

`[@test] ../../expr/functions_test.go`

---

## Type checking: `instance of`

`x instance of T` tests a value's type, where `T` is a type name from
[§ Data types](#data-types). Returns a `BlBoolean`.

```
// expression-language
42 instance of number              // → true
"x" instance of number             // → false
date("2025-01-01") instance of date    // → true
```

`[@test] ../../expr/instance_of_test.go`

---

## Built-in function library

The language ships a large built-in function library. Each function is documented in the spec for
the type it operates on or produces — that spoke gives its full signature, worked examples, edge
cases, and Go registration. This catalogue is the index:

| Group | Representative functions | Spoke |
|---|---|---|
| Conversion | `string`, `number`, `date`, `time`, `datetime`, `duration` | the target type's spoke ([number](number.spec.md), [string](string.spec.md), [date](date.spec.md), …) |
| Boolean | `not` | [boolean.spec.md](boolean.spec.md) |
| Null | `isNull`, `getOrElse` | [null.spec.md](null.spec.md) |
| Resolution | `isDefined` | this spec ([§ Name resolution](#name-resolution-isdefined)) |
| String | `substring`, `stringLength`, `upperCase`, `contains`, `matches`, `replace`, `split`, `stringJoin`, `pattern` (precompiled `BlRegex`), … | [string.spec.md](string.spec.md) |
| Numeric | `decimal`, `floor`, `ceiling`, `round*`, `abs`, `modulo`, `sqrt`, `log`, `ln`, `exp`, `odd`, `even`, … | [number.spec.md](number.spec.md) |
| List | `count`, `min`, `max`, `sum`, `mean`, `sublist`, `append`, `concatenate`, `union`, `distinct`, `flatten`, `sort`, `seq` (and the `:` sequence operator), … | [list.spec.md](list.spec.md) |
| Dictionary | `getValue`, `getEntries`, `dictionaryPut`, `dictionaryMerge` | [dictionary.spec.md](dictionary.spec.md) |
| Temporal | `now`, `today`, `lastDayOfMonth`, `addBusinessDays`, `is*`, … (calendar properties such as `.dayName`, `.monthName` are dot accessors, not function calls — see [date.spec.md § Calendar properties](date.spec.md#calendar-properties)) | [date](date.spec.md) / [time](time.spec.md) / [datetime](datetime.spec.md) |
| Duration | `ymDuration`, `dtDuration`, `ymDurationBetween`, `dtDurationBetween`, components, `abs`, `isNegative`, `round*` (overloaded) | [days_time_duration](days_time_duration.spec.md) / [years_months_duration](years_months_duration.spec.md) |
| Range (interval algebra) | `before`, `after`, `meets`, `overlaps`, `includes`, `during`, `starts`, `finishes`, `coincides` | [range.spec.md](range.spec.md) |
| Table | `table`, `project`, `columns`, `rows`, `distinct` | [table.spec.md](table.spec.md) |
| Calendar (host-constructed) | `entries`, `find`, `contains`, `overlaps`, `entriesFor`, `entriesIn`, `validFrom` / `validTo` / `validRange`, `calendarDrop` / `calendarKeep` / `calendarMerge` | [calendar.spec.md](calendar.spec.md) |

Multi-word names are lowerCamelCase ([§ Relationship to FEEL](#relationship-to-feel-and-future-direction)).
Built-ins that exceed the DMN standard (blkit extensions, e.g. `clamp`, `padStart`, `addBusinessDays`)
are flagged as such in their spoke. Every built-in is registered with `expr` via the mechanism in
[§ Engine internals](#engine-internals-go).

---

## Operator precedence

From lowest to highest binding:

1. `or`
2. `and`
3. comparison — `< <= > >= = !=`, `between`, `in`, `instance of`
4. additive — `+`, `-`
5. sequence — `:` (see [§ Sequences](#sequences-the--operator))
6. multiplicative — `*`, `/`
7. exponent — `**`
8. unary — `-`, `not(...)`
9. postfix — path (`.`), filter/index (`[ ]`), invocation (`( )`)

Parentheses `( )` group sub-expressions explicitly.

```
// expression-language
2 + 3 * 4             // → 14    (* binds tighter than +)
(2 + 3) * 4           // → 20
a or b and c          // → a or (b and c)
1 + 2:5 * 2           // → [3, 4, 5, 6, 7, 8, 9, 10]   (: between additive and multiplicative)
```

`[@test] ../../expr/precedence_test.go`

---

## Errors and null

- **`null` propagation** — most operations involving `null` produce `null`
  ([null.spec.md](null.spec.md)); the exceptions are the short-circuit boolean cases (see
  [§ Boolean logic](#boolean-logic)).
- **Missing dictionary key** → `null`, not an error.
- **Division by zero** → `null`.
- **Parse / type-check errors** — malformed syntax, an unknown variable (when a `BlEnv` is given),
  or a static type mismatch — are returned by `Bl.Expr` as a `BlParseError`.
  `[@test] ../../expr/parse_error_test.go`
- **Evaluation errors** — a type mismatch only detectable with concrete inputs (e.g. comparing
  incompatible types) — are returned by `Evaluate` as a `BlTypeError`.
  `[@test] ../../expr/eval_error_test.go`

---

## Engine internals (Go)

The engine is built on [`github.com/expr-lang/expr`](https://github.com/expr-lang/expr), which
provides lexing, parsing, optional static type-checking, compilation, and a sandboxed evaluation VM.
`expr` supplies **none** of FEEL's types, functions, or syntax — blkit builds those on top. This
section is the **authoritative Go-implementation contract** shared by every spoke; each spoke's
*Go implementation* section then gives only its own concrete value type, host API, and registrations,
referring back here for the mechanics.

All code lives in the repo-root **`expr`** package (Go module path
`github.com/friendly-business-machines/blkit/expr`).

### Package layout

| File | Contents |
|---|---|
| `expr/engine.go` | The `Bl` entry namespace, `Bl.Expr`, `BlExpr`, `BlEnv`, `BlType`; the option-assembly, operator binding, patcher install, and the input/output bridge. |
| `expr/value.go` | The `BlValue` interface, the `BlNull` singleton, and shared helpers (null propagation, equality, wrapping). |
| `expr/errors.go` | `BlParseError`, `BlTypeError`, `BlRegexError`, `BlCalendarRangeError`. |
| `expr/patch.go` | The `ast.Visitor` patcher(s) for FEEL-only syntax. |
| `expr/<type>.go` | One per type (`number.go`, `string.go`, `date.go`, …): the `Bl*` value type, its exported host API, its unexported `…Options()` registrations, and its backing impl funcs. |
| `expr/*_test.go` | Tests — the `[@test]` targets throughout these specs. |

### Visibility & naming conventions

- **Exported** (the public API): the value types (`BlNumber`, `BlString`, …); their host
  constructors (`Number`, `Date`, `Today`, …) and accessors (`ToNativeFloat`, `ToNativeString`,
  `CompareTo`, …); the engine surface (`Bl`, `BlExpr`, `BlEnv`, `BlValue`, `BlType`); and the error
  types.
- **Unexported** (package-internal): built-in implementation funcs (suffix `Fn`, e.g. `dayNameFn`);
  operator implementation funcs (e.g. `addNumbers`, `ltDates`); each type's `…Options()` assembler;
  the patcher; and the bridge helpers (`wrap`/`unwrap`).

### Engine entry points (`engine.go`)

```go
// host-side (Go)
// Bl is the package entry namespace (a zero-size value), so callers write
// expr.Bl.Expr(...).
type blEngine struct{}
var Bl blEngine

func (blEngine) Expr(source string, env BlEnv) (BlExpr, error)

// BlExpr is a compiled expression (wraps a *vm.Program).
type BlExpr interface {
    Evaluate(input map[string]any) (BlValue, error)
    Source() string
}

type BlEnv map[string]BlType

// BlType identifies a language type for parse-time checking and `instance of`.
type BlType int
const (
    BlTypeNull BlType = iota
    BlTypeNumber; BlTypeString; BlTypeBoolean
    BlTypeDate; BlTypeTime; BlTypeDateTime
    BlTypeDaysTimeDuration; BlTypeYearsMonthsDuration
    BlTypeList; BlTypeDictionary; BlTypeRange; BlTypeTable; BlTypeCalendar
    BlTypeRegex
    BlTypeAny
)
```

**Pipeline.** `Bl.Expr` runs: **normalise** (source-level fixups `expr`'s lexer needs — see Operators)
→ **parse** (`expr`'s parser) → **patch** (`expr.Patch`, rewrite FEEL-only syntax) → **type-check**
(against the `BlEnv`) → **compile** to a `*vm.Program`. `Evaluate` wraps the input map into `Bl*`
values, runs the program on the sandboxed VM, and unwraps the result.

```go
// host-side (Go)
func (blEngine) Expr(source string, env BlEnv) (BlExpr, error) {
    program, err := expr.Compile(normalise(source), buildOptions(env)...)
    if err != nil {
        return nil, &BlParseError{Source: source, Err: err}
    }
    return &compiled{program: program, source: source}, nil
}

// buildOptions assembles every spoke's registrations, the operator bindings,
// the patcher, and the typed environment.
func buildOptions(env BlEnv) []expr.Option {
    opts := []expr.Option{expr.Env(envType(env))}
    for _, reg := range typeRegistrations { // numberOptions, stringOptions, dateOptions, …
        opts = append(opts, reg()...)
    }
    opts = append(opts, operatorBindings()...) // the expr.Operator(...) lines
    opts = append(opts, expr.Patch(newFeelPatcher()))
    return opts
}
```

### The `BlValue` contract (`value.go`)

Every `Bl*` value type implements `BlValue`, so they pass through the VM as `any`:

```go
// host-side (Go)
type BlValue interface {
    Type() BlType                // the language type tag
    Equal(other BlValue) BlValue // three-valued: BlBoolean or BlNull (see null.spec.md)
    String() string              // canonical literal-form rendering
    isBlValue()                  // sealing method — only this package's types implement BlValue
}

// The null value (see null.spec.md).
type BlNull struct{}
func Null() BlNull
```

Type-specific host accessors (`ToNativeFloat`, `Hour`, `ToArray`, …) are declared on the concrete
types in their spokes, not on the interface.

### Bridging native ↔ `Bl*` (`value.go`)

`wrap` converts host inputs to `Bl*`; `unwrap` is the inverse for results that cross back out.

```go
// host-side (Go)
func wrap(v any) (BlValue, error)   // native Go → Bl*
func unwrap(v BlValue) any          // Bl* → native (used by host code)
```

| Native Go input | Wrapped as |
|---|---|
| `int`, `int64`, `float64`, `decimal.Decimal`, decimal `string` via `Number(...)` | `BlNumber` |
| `string` | `BlString` |
| `bool` | `BlBoolean` |
| `[]any` | `BlList` |
| `map[string]any` | `BlDictionary` |
| `time.Time` | `BlDate` / `BlDateTime` (per precision) |
| `time.Duration` | `BlDaysTimeDuration` |
| `nil` / absent input key | `Null` |
| an already-`BlValue` | itself |

`BlNumber` stays an arbitrary-precision decimal inside the VM — **never** collapsed to `float64`.
`Null` propagates per [null.spec.md](null.spec.md).

### Registering built-in functions

`expr.Function(name, fn, signatures…)` takes an impl in `expr`'s calling convention plus one or more
typed signatures used by the checker (multiple = overloads). Each spoke's `…Options()` assembles its
registrations:

```go
// host-side (Go)
// expr's required impl shape:
type exprFn = func(args ...any) (any, error)

// dateOptions is unexported; the engine calls it from buildOptions.
func dateOptions() []expr.Option {
    return []expr.Option{
        expr.Function("dayName", dayNameFn,
            new(func(BlDate) BlString), new(func(BlDateTime) BlString)),
        expr.Function("addBusinessDays", addBusinessDaysFn,
            new(func(BlDate, BlNumber, BlCalendar) BlDate),
            new(func(BlDate, BlNumber, BlCalendar, bool) BlDate)),
        // … one entry per function in the spoke's § Built-in functions
    }
}

// Backing impl (unexported, suffix Fn), in expr's calling convention:
func dayNameFn(args ...any) (any, error) { /* type-asserts args, returns BlValue */ }
```

A small set of generic adapters (`typed1`, `typed2`, `typed3`, `variadic`) wrap statically-typed Go
funcs into `exprFn` and supply the matching signature pointer, so most impls are written with real
types and the `args ...any` boilerplate is confined to the adapters.

### Operators

`expr.Operator(op, fnNames…)` overloads a **binary** operator: the listed functions (each registered
via `expr.Function`) are tried by operand type. Binding is centralised in `operatorBindings()` because
one operator spans many types; each spoke contributes the named funcs.

```go
// host-side (Go)
func operatorBindings() []expr.Option {
    return []expr.Option{
        expr.Operator("+",  "addNumbers", "concatStrings",
                            "addDateYM", "addDateDT", "addDateTimeDur",
                            "addTimeDur", "addYMDuration", "addDTDuration"),
        expr.Operator("-",  "subNumbers", "subDates", "subDateTimes",
                            "subDateDur", "subDateTimeDur",
                            "subYMDuration", "subDTDuration"),
        expr.Operator("*",  "mulNumbers", "scaleYMDuration", "scaleDTDuration"),
        expr.Operator("/",  "divNumbers", "divYMDuration", "divDTDuration"),
        expr.Operator("**", "powNumber"),
        expr.Operator("<",  "ltNumbers", "ltStrings", "ltDates", "ltDateTimes",
                            "ltTimes", "ltYMDuration", "ltDTDuration"),
        // <=, >, >=, ==, != likewise
    }
}
```

Three operator concerns are **not** handled by `expr.Operator`:

- **`=` (single-equals).** FEEL uses `=`/`!=`; `expr`'s lexer expects `==`. The `normalise` step
  rewrites a single `=` to `==` (leaving `==`, `<=`, `>=`, `!=` untouched) before parsing.
- **Unary `-`.** `expr.Operator` is binary-only; the patcher rewrites unary `-x` into `negate(x)`,
  a registered function overloaded over `BlNumber`/duration types.
- **`and` / `or`.** Short-circuit logical operators over Go `bool`; our operands are wrapped `Bl*`
  values that may be `BlNull`. The patcher rewrites them into calls to the three-valued funcs in
  [boolean.spec.md](boolean.spec.md) (`blAnd`/`blOr`). `not(x)` is an ordinary `expr.Function`.

### Patchers (`patch.go`)

FEEL constructs absent from `expr`'s grammar are produced by an `expr` patcher (`ast.Visitor` via
`expr.Patch`) that rewrites the parsed tree before compilation:

- interval/range membership with open/closed boundaries (`x in [a..b)`, range literals);
- unary tests (decision-table input entries);
- `for…return`, `some/every…satisfies`, `if…then…else`;
- the boolean connectives `and`/`or` and unary `-` (above);
- **component access** — `x.year`, `d.minutes`, `r.start` resolve to accessor-function calls
  (`dateYear(x)`, …) because `Bl*` values are opaque structs, not reflectable maps; dictionary member
  access (`d.key`) lowers to `getValue(d, "key")`;
- the **sequence operator** `start:end` lowers to `seq(start, end, 1)` (see
  [§ Sequences](#sequences-the--operator)); the patcher also enforces the "no bare `:` in a
  dictionary value position" rule by rejecting any `:` it finds inside a dict-literal value
  expression that isn't parenthesised.

Forbidding spaces in identifiers (see [§ Relationship to FEEL](#relationship-to-feel-and-future-direction))
removes what would otherwise be the hardest rewrite — multi-word names colliding with `and`/`or` —
and lets identifiers ride directly on `expr`'s lexer.

### Environment & errors

`BlEnv` is translated to an `expr.Env` (a `map[string]any` of zero-value `Bl*` exemplars per type) so
references are type-checked at parse time. Errors:

```go
// host-side (Go)
type BlParseError struct { Source string; Err error } // from Bl.Expr (parse/type-check)
type BlTypeError  struct { /* op, types */ }           // from Evaluate (runtime type mismatch)
type BlRegexError struct { Pattern string; Err error } // bad regex in matches/replace/extract
type BlCalendarRangeError struct { /* date, bounds */ }// business-day iteration past validity
```

`[@test] ../../expr/engine_internal_test.go`

---

## Integration points

These are forward-looking notes; the referenced specs are **not** modified by this document.

- **`LiteralExpression`** ([literal-expression.spec.md](../decision-tasks/literal-expression.spec.md))
  could accept a source string compiled with `Bl.Expr` as its expression body.
- **`DecisionTable`** ([decision-table.spec.md](../decision-tasks/decision-table.spec.md)) rule
  predicates and input entries could be authored as unary tests (see [§ Unary tests](#unary-tests)),
  parsed against each column's type.

---

## Relationship to FEEL and future direction

The v1 language is based on DMN 1.4 FEEL, drawing on the standard plus the well-documented
implementations below. FEEL is the starting point, not a permanent contract: blkit diverges where it
improves the language. Each divergence is recorded here with the FEEL behaviour, the blkit
behaviour, and the rationale.

**Divergences from FEEL (v1):**

- **No spaces in identifiers.** FEEL permits multi-word names with embedded spaces, for both
  variables and built-in functions (`loan amount`, `string length`, `day of week`). blkit forbids
  spaces — identifiers are letters, digits, and underscores. *Rationale:* removes FEEL's hardest
  parsing ambiguity (longest-match names colliding with the `and`/`or` keywords), matches
  conventional programming identifiers, and lets the grammar sit directly on `expr`'s lexer.
- **lowerCamelCase built-in names.** As a direct consequence of the no-spaces rule, every multi-word
  FEEL function name is spelled in lowerCamelCase: `string length` → `stringLength`, `day of year` →
  `dayOfYear`, and `date and time` → `datetime` (a recognised compound, kept whole). *Rationale:*
  lowerCamelCase is the convention used by `expr`'s own built-ins (`hasPrefix`, `sortBy`, `toJSON`)
  and matches the casing of the host Go `Bl*` methods, so blkit functions read as native on both
  layers. The catalogue and the spokes use these spellings.
- **Renamed for clarity.** A handful of FEEL function names are renamed where the literal
  lowerCamelCase translation reads awkwardly or hides the function's role:
  - FEEL's `years and months duration(from, to)` becomes `ymDurationBetween(from, to)` — the
    `Between` suffix makes the date-difference role explicit and matches the parallel
    `dtDurationBetween` for the sibling duration type.
  - FEEL's polymorphic `duration(string)` (which returns either kind depending on the literal's
    designators) is split into typed sibling constructors **`ymDuration(string)`** and
    **`dtDuration(string)`**. Each accepts only its kind's designators and has a single typed
    return, so downstream usage is statically checkable; a wrong-kind string is a
    `BlParseError`. This avoids the runtime-dispatch cost of a polymorphic constructor for a
    case (variable input strings) that's rare in practice.

  The catalogue and spokes use the renamed forms.

**Open decisions:**

- Which vendor extensions (the non-standard built-ins flagged in the spokes, e.g. `dateAdd`,
  `intersection`) are in scope.

### References

- OMG DMN 1.4 specification — the FEEL grammar and standard built-in function library.
- Trisotech Digital Enterprise Suite FEEL docs — standard FEEL function reference and temporal/list
  extensions.
- Drools / KIE [DMN FEEL handbook](https://kiegroup.github.io/dmn-feel-handbook/) — function
  reference and numeric-literal extensions.
- Camunda FEEL language guide — function signatures and worked examples.

---

## Edge cases

- An empty source string is a `BlParseError`.
- An expression that evaluates to `null` is a valid result.
- A compiled `BlExpr` needs no input for expressions that reference no variables (`1 + 1`,
  `date("2025-01-01")`) — pass `nil` to `Evaluate`.
- A list index out of range returns `null`; a missing dictionary key returns `null`.

`[@test] ../../expr/edge_cases_test.go`
