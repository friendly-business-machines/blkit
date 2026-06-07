---
name: bl.BlExpr
description: The blkit expression language — a string-based expression syntax (based on DMN FEEL) parsed and evaluated into Bl* values. This is the hub spec covering the engine API, operators, control flow, unary tests, lexical rules, semantics, and the Go layer that extends expr-lang/expr; each data type's literals, functions, and Go implementation are detailed in its own spoke spec.
targets:
  - ../../expr_engine.go
---

# The blkit expression language

This spec defines **blkit's expression language**: a compact, business-readable syntax for writing
expressions as text — `age >= 18 and income > 50000`, `sum(lineItems.amount)`,
`if score >= 700 then "approve" else "review"` — that evaluate to blkit value types (`bl.BlNumber`,
`bl.BlString`, `bl.BlList`, …, each documented in its own spoke spec; see [§ Data types](#data-types)).

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
same source syntax (e.g. a `bl.BlNumber` renders as `42`, a `bl.BlString` as `"foo"`).

---

## Using the engine

```go
// host-side (Go)
// Expr compiles a source string once, optionally type-checking it against a
// declared schema. The returned bl.BlExpr can be evaluated repeatedly.
func Expr(source string, schema BlSchema) (BlExpr, error)

// BlSchema declares the variable names and types an expression may reference, so
// type errors surface at parse time. Pass nil to skip static checking. See
// schema.spec.md for the full type definition.

// BlExpr is a compiled source-text expression.
type BlExpr interface {
    Evaluate(input BlValue) (BlValue, error)
    Source() string
}
```

A `bl.BlExpr` is a compiled expression: parse a source string once with `bl.Expr`, then `Evaluate` it
repeatedly. The input is a single `bl.BlValue` — typically a `bl.BlDictionary` whose keys match the
declared schema's fields. The result is a `bl.BlValue` (see [§ Engine internals](#engine-internals-go)
for the bridging rules and host accessors). There are no `bl.Number(…)`-style expression factories.

```go
// host-side (Go)
var schema, _ = bl.Schema(
    bl.Field{Name: "age",    Type: bl.TypeNumber},
    bl.Field{Name: "income", Type: bl.TypeNumber},
)
var eligible, _ = bl.Expr("age >= 18 and income > 50000", schema)

var age,    _ = bl.Number(21)
var income, _ = bl.Number(60000)
var inputs, _ = bl.Dictionary(map[string]bl.BlValue{
    "age":    age,
    "income": income,
})
var result, _ = eligible.Evaluate(inputs)
// result is the bl.BlBoolean true
```

`[@test] ../../expr_engine_test.go`

---

## Data types

Every value belongs to one of the following types. Each has a literal or constructor syntax.

| Type | `Bl*` type | Literal / constructor | Example |
|---|---|---|---|
| number | `bl.BlNumber` | digits, optional `.` and `-` | `42`, `3.14`, `-5`, `1500.50` |
| string | `bl.BlString` | double quotes | `"hello"` |
| boolean | `bl.BlBoolean` | keywords | `true`, `false` |
| date | `bl.BlDate` | `date(...)` | `date("2025-03-28")` |
| time | `bl.BlTime` | `time(...)` | `time("11:45:30")`, `time("11:45:30+02:00")` |
| date-time | `bl.BlDateTime` | `datetime(...)` | `datetime("2025-03-28T11:45:30")` |
| days-time duration | `bl.BlDaysTimeDuration` | `dtDuration(...)` | `dtDuration("P4DT12H")` |
| years-months duration | `bl.BlYearsMonthsDuration` | `ymDuration(...)` | `ymDuration("P1Y6M")` |
| list | `bl.BlList` | `[ ... ]` | `[1, 2, 3]` |
| dictionary | `bl.BlDictionary` | `{ ... }` | `{name: "Alice", age: 30}` |
| range | `bl.BlRange` | interval notation | `[1..10]`, `(1..10)`, `[1..10)` |
| null | `bl.BlNull` | keyword | `null` |

- **Numbers** are arbitrary-precision decimals — never floats; precision is preserved through all
  arithmetic (see [number.spec.md](number.spec.md)).
- The blkit-specific types `bl.BlCalendar` ([calendar.spec.md](calendar.spec.md)) and `bl.BlTable`
  ([table.spec.md](table.spec.md)) have **no literal syntax** in v1; they are produced
  programmatically and may be referenced as variables.

`[@test] ../../expr_data_types_test.go`

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
with a `bl.BlSchema`, are type-checked at parse time. A reference to a name that is neither in scope
nor declared is a parse error (see [§ Errors and null](#errors-and-null)).

`[@test] ../../expr_variables_test.go`

### Name resolution: `isDefined`

`isDefined(x)` is a built-in that reports whether the engine could resolve `x` to a value. It
returns `true` when `x` resolves (including when it resolves to `bl.BlNull`) and `false` only when
the name is unbound at evaluation time. It is the only way for an expression to distinguish "the
caller supplied this name with a null value" from "the caller supplied no value at all."

```
// expression-language
isDefined(applicant)             // → true if `applicant` is bound, even if its value is null
isDefined(applicant.middleName)  // → true (path access on a bound dictionary always resolves;
                                 //   a missing key resolves to bl.BlNull, which is still "defined")
isDefined(undeclaredName)        // → false (only when there is no binding)
```

`isDefined` operates at the resolution layer, not the value layer — to test whether a value *is*
`bl.BlNull` once resolved, use `isNull(x)` ([null.spec.md § Testing for null](null.spec.md#testing-for-null)).
To supply a fallback when a value resolves to `bl.BlNull`, use `getOrElse(x, default)`
([null.spec.md § Default for null](null.spec.md#default-for-null)).

Because `isDefined` distinguishes "unbound name" from "bound to anything (including `bl.BlNull`)", it
cannot be expressed as a normal `bl.BlValue → bl.BlValue` impl — by the time a normal impl runs, the
argument has already been resolved and unbound names are a parse error. Instead, the engine's AST
patcher (see [§ Patchers](#patchers-ast-rewriting)) intercepts `isDefined(name)` calls before
resolution and rewrites them to a lookup against the input value plus declared `bl.BlSchema` bindings.
The impl is registered in `expr_engine.go` alongside the other engine-level options.

`[@test] ../../expr_is_defined_test.go`

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

`[@test] ../../expr_arithmetic_test.go`

---

## Comparison

The operators `<`, `<=`, `>`, `>=`, `=`, `!=` compare numbers, strings, and temporal/duration
values; `=` and `!=` apply to all types. `x between a and b` is shorthand for `x >= a and x <= b`.
The result is a `bl.BlBoolean` (or `null` when operands are incomparable).

```
// expression-language
5 < 10                             // → true
10 >= 10                           // → true
"abc" = "abc"                      // → true
date("2025-01-01") < date("2025-06-01")   // → true
5 between 1 and 10                 // → true
```

`[@test] ../../expr_comparison_test.go`

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

`[@test] ../../expr_boolean_logic_test.go`

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

`[@test] ../../expr_string_expressions_test.go`

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

`[@test] ../../expr_conditional_test.go`

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

`[@test] ../../expr_ranges_test.go`

---

## Sequences: the `:` operator

`start:end` materialises a numeric **`bl.BlList`** running from `start` to `end` inclusive in
steps of `1` — a strict-list counterpart to the range `[start..end]` (which stays as a
`bl.BlRange` for containment / interval-algebra purposes). The shorthand is sugar for
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
{a: 5:10}                    // bl.ParseError — bare `:` in a dict value position
```

`[@test] ../../expr_sequences_test.go`

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

For a `bl.BlCalendar` right operand, `point in calendar` is **patcher-lowered** to
`contains(calendar, point)` and inherits its semantics — see
[calendar.spec.md § Operators](calendar.spec.md#operators) for the zone-kind and
cross-temporal-kind rules. The left operand for calendar membership must be a `bl.BlDate` or
`bl.BlDateTime`; a range left operand → `bl.TypeError` (use the explicit
`overlaps(c, r)` / `entriesIn(c, r)` for range-on-calendar queries).

`[@test] ../../expr_membership_test.go`

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
pattern when a decision-table row's cells are checked against repeated input data. The
host API is the same `bl.BlExpr` returned by `bl.Expr`; the only difference is the
constructor accepts the unary-test grammar and an `inputType` (the type the implicit `?`
will hold).

```go
// host-side (Go)

// bl.UnaryTest compiles a unary-test source string. inputType is the type the
// implicit ? will hold at evaluation time; the engine uses it to type-check the
// test body and verifies the result is a bl.BlBoolean (or bl.BlNull). Pass
// bl.TypeAny for inputs whose type isn't known statically (e.g. when the test
// must accept the wildcard "-" against any value).
func UnaryTest(source string, inputType Type) (BlExpr, error)
```

The returned `bl.BlExpr` is evaluated like any other: pass the input value directly
(no need to wrap into a dictionary, since there's only the single implicit `?`).

Worked example:

```go
// host-side (Go) — compile-once, test-many.
var atLeast18, _ = bl.UnaryTest(">= 18", bl.TypeNumber)
var isUrgent,  _ = bl.UnaryTest(`contains(?, "urgent")`, bl.TypeString)
var inRange,   _ = bl.UnaryTest("[18..65]", bl.TypeNumber)
var wildcard,  _ = bl.UnaryTest("-", bl.TypeAny)

// Evaluate against typed inputs. The engine has verified at construction that
// every result will be bl.BlBoolean (true / false) or bl.BlNull (when null
// propagation kicks in — see null.spec.md § Propagation).
var n21, _    = bl.Number(21)
var ok,  _    = atLeast18.Evaluate(n21)                          // → bl.BlBoolean(true)

var noteIn    = bl.String("urgent notice")
var ok2, _    = isUrgent.Evaluate(noteIn)                        // → bl.BlBoolean(true)

var n70, _    = bl.Number(70)
var ok3, _    = inRange.Evaluate(n70)                            // → bl.BlBoolean(false)
var ok4, _    = wildcard.Evaluate(n70)                           // → bl.BlBoolean(true) — wildcard matches anything
var ok5, _    = wildcard.Evaluate(bl.Null())                     // → bl.BlBoolean(true) — even Null
```

The fallible cases:

- **Parse-time errors** — `bl.UnaryTest` returns `(nil, bl.ParseError)` if the source
  string isn't a valid unary test. Common causes: unknown identifier, malformed range,
  type mismatch between the test body and the declared `inputType` (e.g. `>= 18` with
  `bl.TypeString`), or a body whose result isn't a boolean (e.g. `?` alone with
  `bl.TypeNumber`).
- **Evaluation-time errors** — `Evaluate(input)` returns `(nil, bl.TypeError)` if the
  supplied `input`'s type doesn't match the compiled `inputType`. This shouldn't happen
  with correct host code, but the runtime check catches mismatches.
- **`bl.BlNull` results** — a test against a null input returns `bl.BlNull`, not an
  error. Wildcards (`-`) explicitly return `true` for null; all other tests propagate
  null per the standard rules (see [null.spec.md](null.spec.md)).

The engine's pre-validation guarantees the result type, but `Evaluate` returns
`bl.BlValue` like any other `bl.BlExpr`; callers that need a `bl.BlBoolean` value can
type-assert the result.

The decision-table runtime ([decision-table.spec.md](../decision-tasks/decision-table.spec.md))
uses this API internally: at table-construction time, each cell's unary-test source is
compiled once via `bl.UnaryTest` and the resulting `bl.BlExpr` is cached on the cell. At
evaluation time, the column-input value is fed through `Evaluate(input)`, and the
per-cell booleans are combined per the table's hit policy.

**Relationship to `bl.Expr`.** `bl.UnaryTest` is a constructor variant of `bl.Expr`:
it shares the same parse / patch / type-check / compile pipeline, with two extra steps
in front. A **source normaliser** rewrites the unary-test forms into ordinary
expressions referencing `?` (e.g. `< 10` → `? < 10`, `2, 3` → `? = 2 or ? = 3`, `-` →
`true`, `[18..65]` → `? in [18..65]`, `contains(?, "urgent")` left as-is), and a
single-field `bl.BlSchema` declaring `?` with type `inputType` is supplied for the
type-check step. The result is an ordinary `bl.BlExpr` whose evaluation receives the
input value bound to `?`. The separate constructor exists so the unary-test grammar
(the left-implicit and comma-disjunction forms) doesn't have to be valid
plain-expression syntax — those forms only parse in unary-test mode. `Source()`
returns the original (un-normalised) string the caller supplied.

`[@test] ../../expr_unary_tests_test.go`

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

`[@test] ../../expr_list_expressions_test.go`

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

Each loop result is a `bl.BlList`.

`[@test] ../../expr_for_test.go`

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

`[@test] ../../expr_quantified_test.go`

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

`[@test] ../../expr_dictionaries_test.go`

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

`[@test] ../../expr_components_test.go`

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

`[@test] ../../expr_functions_test.go`

---

## Type checking: `instance of`

`x instance of T` tests a value's type, where `T` is a type name from
[§ Data types](#data-types). Returns a `bl.BlBoolean`.

```
// expression-language
42 instance of number              // → true
"x" instance of number             // → false
date("2025-01-01") instance of date    // → true
```

`[@test] ../../expr_instance_of_test.go`

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
| String | `substring`, `stringLength`, `upperCase`, `contains`, `matches`, `replace`, `split`, `stringJoin`, `pattern` (precompiled `bl.BlRegex`), … | [string.spec.md](string.spec.md) |
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

`[@test] ../../expr_precedence_test.go`

---

## Errors and null

- **`null` propagation** — most operations involving `null` produce `null`
  ([null.spec.md](null.spec.md)); the exceptions are the short-circuit boolean cases (see
  [§ Boolean logic](#boolean-logic)).
- **Missing dictionary key** → `null`, not an error.
- **Division by zero** → `null`.
- **Parse / type-check errors** — malformed syntax, an unknown variable (when a `bl.BlSchema` is given),
  or a static type mismatch — are returned by `bl.Expr` as a `bl.ParseError`.
  `[@test] ../../expr_parse_error_test.go`
- **Evaluation errors** — a type mismatch only detectable with concrete inputs (e.g. comparing
  incompatible types) — are returned by `Evaluate` as a `bl.TypeError`.
  `[@test] ../../expr_eval_error_test.go`

---

## Engine internals (Go)

The engine is built on [`github.com/expr-lang/expr`](https://github.com/expr-lang/expr), which
provides lexing, parsing, optional static type-checking, compilation, and a sandboxed evaluation VM.
`expr` supplies **none** of FEEL's types, functions, or syntax — blkit builds those on top. This
section is the **authoritative Go-implementation contract** shared by every spoke; each spoke's
*Go implementation* section then gives only its own concrete value type, host API, and registrations,
referring back here for the mechanics.

All code lives in the **root `blkit`** package (Go module path
`github.com/friendly-business-machines/blkit`). Callers conventionally import as `bl`, so
call sites read `bl.Expr(...)`, `bl.Number(...)`, etc.

### Package layout

| File | Contents |
|---|---|
| `expr_engine.go` | `Expr`, `bl.BlExpr`, `Type`; the option-assembly, operator binding, patcher install, and the input/output bridge. `bl.BlSchema` lives in `expr_schema.go`. |
| `expr_value.go` | The `bl.BlValue` interface, the `bl.BlNull` singleton, and shared helpers (null propagation, equality, wrapping). |
| `expr_errors.go` | `ParseError`, `TypeError`, `RegexError`, `CalendarRangeError`. |
| `expr_patch.go` | The `ast.Visitor` patcher(s) for FEEL-only syntax. |
| `expr_<type>.go` | One per value type (`expr_number.go`, `expr_string.go`, `expr_date.go`, …): the `Bl*` value type, its exported host API, its unexported `…Options()` registrations, and its backing impl funcs. |
| `expr_*_test.go` | Tests — the `[@test]` targets throughout these specs. |

### Visibility & naming conventions

- **Exported** (the public API): the value types (`bl.BlNumber`, `bl.BlString`, …); their host
  constructors (`Number`, `Date`, `Today`, …) and accessors (`ToNativeFloat`, `ToNativeString`,
  `CompareTo`, …); the engine surface (`Expr`, `bl.BlExpr`, `bl.BlSchema`, `bl.BlValue`, `Type`); and the
  error types.
- **Unexported** (package-internal): built-in implementation funcs (suffix `Fn`, e.g. `dayNameFn`);
  operator implementation funcs (e.g. `addNumbers`, `ltDates`); each type's `…Options()` assembler;
  the patcher; and the bridge helpers (`wrap`/`unwrap`).

### Engine entry points (`expr_engine.go`)

```go
// host-side (Go)
// All identifiers below live in package blkit. Callers conventionally
// import as `bl` (so call sites read bl.Expr, bl.Number, etc.).

func Expr(source string, schema BlSchema) (BlExpr, error)

// bl.BlExpr is a compiled expression (wraps a *vm.Program).
type BlExpr interface {
    Evaluate(input BlValue) (BlValue, error)
    Source() string
}

// bl.BlSchema declares parse-time variable names and types. See schema.spec.md.

// Type identifies a language type for parse-time checking and `instance of`.
type Type int
const (
    TypeNull Type = iota
    TypeNumber; TypeString; TypeBoolean
    TypeDate; TypeTime; TypeDateTime
    TypeDaysTimeDuration; TypeYearsMonthsDuration
    TypeList; TypeDictionary; TypeRange; TypeTable; TypeCalendar
    TypeRegex
    TypeAny
)
```

**Pipeline.** `bl.Expr` runs: **normalise** (source-level fixups `expr`'s lexer needs — see Operators)
→ **parse** (`expr`'s parser) → **patch** (`expr.Patch`, rewrite FEEL-only syntax) → **type-check**
(against the `bl.BlSchema`) → **compile** to a `*vm.Program`. `Evaluate` takes a `bl.BlValue` input
(typically a `bl.BlDictionary` for multi-variable expressions, or a single value for unary-test
`bl.BlExpr`s where the schema declares `?`), runs the program on the sandboxed VM, and returns the
result as a `bl.BlValue`.

```go
// host-side (Go)
func Expr(source string, schema BlSchema) (BlExpr, error) {
    program, err := expr.Compile(normalise(source), buildOptions(schema)...)
    if err != nil {
        return nil, &ParseError{Source: source, Err: err}
    }
    return &compiled{program: program, source: source}, nil
}

// buildOptions assembles every spoke's registrations, the operator bindings,
// the patcher, and the typed environment.
func buildOptions(schema BlSchema) []expr.Option {
    opts := []expr.Option{expr.Env(envType(schema))}
    for _, reg := range typeRegistrations { // numberOptions, stringOptions, dateOptions, …
        opts = append(opts, reg()...)
    }
    opts = append(opts, operatorBindings()...) // the expr.Operator(...) lines
    opts = append(opts, expr.Patch(newFeelPatcher()))
    return opts
}
```

### The `bl.BlValue` contract (`expr_value.go`)

Every `Bl*` value type implements `bl.BlValue`, so they pass through the VM as `any`:

```go
// host-side (Go)
type BlValue interface {
    Type() Type                // the language type tag
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

### Bridging native ↔ `Bl*` (`expr_value.go`)

`wrap` converts host inputs to `Bl*`; `unwrap` is the inverse for results that cross back out.

```go
// host-side (Go)
func wrap(v any) (BlValue, error)   // native Go → Bl*
func unwrap(v BlValue) any          // Bl* → native (used by host code)
```

| Native Go input | Wrapped as |
|---|---|
| `int`, `int64`, `float64`, `decimal.Decimal`, decimal `string` via `Number(...)` | `bl.BlNumber` |
| `string` | `bl.BlString` |
| `bool` | `bl.BlBoolean` |
| `[]any` | `bl.BlList` |
| `map[string]any` | `bl.BlDictionary` |
| `time.Time` | `bl.BlDate` / `bl.BlDateTime` (per precision) |
| `time.Duration` | `bl.BlDaysTimeDuration` |
| `nil` / absent input key | `Null` |
| an already-`bl.BlValue` | itself |

`bl.BlNumber` stays an arbitrary-precision decimal inside the VM — **never** collapsed to `float64`.
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
            new(func(bl.BlDate) bl.BlString), new(func(bl.BlDateTime) bl.BlString)),
        expr.Function("addBusinessDays", addBusinessDaysFn,
            new(func(bl.BlDate, bl.BlNumber, bl.BlCalendar) bl.BlDate),
            new(func(bl.BlDate, bl.BlNumber, bl.BlCalendar, bool) bl.BlDate)),
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
  a registered function overloaded over `bl.BlNumber`/duration types.
- **`and` / `or`.** Short-circuit logical operators over Go `bool`; our operands are wrapped `Bl*`
  values that may be `bl.BlNull`. The patcher rewrites them into calls to the three-valued funcs in
  [boolean.spec.md](boolean.spec.md) (`blAnd`/`blOr`). `not(x)` is an ordinary `expr.Function`.

### Patchers (`expr_patch.go`)

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

`bl.BlSchema` is translated to an `expr.Env` (a `map[string]any` of zero-value `Bl*` exemplars per
field) so references are type-checked at parse time. Nested `Fields` are walked recursively so
that member-access expressions (`applicant.address.postalCode`) type-check against the declared
shape. Errors:

```go
// host-side (Go)
type ParseError struct { Source string; Err error } // from Expr (parse/type-check)
type TypeError  struct { /* op, types */ }           // from Evaluate (runtime type mismatch)
type RegexError struct { Pattern string; Err error } // bad regex in matches/replace/extract
type CalendarRangeError struct { /* date, bounds */ }// business-day iteration past validity
```

`[@test] ../../expr_engine_internal_test.go`

---

## Integration points

These are forward-looking notes; the referenced specs are **not** modified by this document.

- **`LiteralExpression`** ([literal-expression.spec.md](../decision-tasks/literal-expression.spec.md))
  could accept a source string compiled with `Expr` as its expression body.
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
    `ParseError`. This avoids the runtime-dispatch cost of a polymorphic constructor for a
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

- An empty source string is a `ParseError`.
- An expression that evaluates to `null` is a valid result.
- A compiled `bl.BlExpr` needs no input for expressions that reference no variables (`1 + 1`,
  `date("2025-01-01")`) — pass `nil` to `Evaluate`.
- A list index out of range returns `null`; a missing dictionary key returns `null`.

`[@test] ../../expr_edge_cases_test.go`
