# Unary Tests

> A compact syntax for writing a condition about a single value — `> 5`,
> `[18..65]`, `"gold"` — where the value being tested is supplied implicitly.

A **unary test** is a condition evaluated against one implicit input value. Where
an ordinary expression spells out both sides of a comparison (`age >= 18`), a
unary test leaves the input unwritten and states only the condition (`>= 18`).
The value to test is fed in from the outside.

This is the shorthand used to write the input cells of a decision table: each
cell is a unary test, and the column's value for the row being evaluated is the
implicit input. A cell that reads `[18..65]` means "the input is between 18 and
65"; a cell that reads `"gold", "platinum"` means "the input is gold or
platinum".

## The forms

| Form | Matches when the input… | Example |
|---|---|---|
| **value** | equals the value | `"valid"`, `42` |
| **comparison** `<` `<=` `>` `>=` | compares true | `< 10`, `>= date("2020-04-06")` |
| **interval** | falls in the range | `[2..5]`, `(2..5]` |
| **comma list** | equals any of them (an "or") | `2, 3, 4`, `< 10, > 50` |
| **`not(...)`** | does *not* match the inner test | `not("valid")`, `not(2, 3)` |
| **leading-dot path** | the path on the input compares true | `.status = "active"`, `.amount > 100` |
| **`?` expression** | the expression (with `?` = input) is true | `contains(?, "good")` |
| **wildcard `-`** | always — matches anything | `-` |

```
// expression-language
< 10                   // input < 10
[18..65]               // 18 <= input <= 65
"low", "medium"        // input = "low" or input = "medium"
not(0)                 // input is not 0
contains(?, "urgent")  // the input string contains "urgent"
-                      // matches anything
```

A comma list is a disjunction — the test passes if the input matches **any**
entry — and the entries can be different forms: `< 10, > 50` matches an input
below 10 or above 50. `not(...)` inverts whatever test it wraps, including a
comma list: `not(2, 3)` matches any input that is neither 2 nor 3.

## Which forms apply to which types

The **comparison** and **interval** forms need an ordered value type — numbers,
strings, and the temporal types. The **value**, **comma list**, `not(...)`, `?`
expression, and **wildcard** forms work against any type, including booleans,
lists, dictionaries, tables, and ranges.

So `[18..65]` is valid for a number input but not for a boolean one, whereas
`"valid"`, `not(x)`, `-`, and the `?` forms apply everywhere.

## The `?` placeholder

For the simple forms (`< 10`, `"valid"`, `[18..65]`), the input is referenced
implicitly — you never name it. When the condition needs to **pass the input to a
function**, `?` is the name that stands for it:

```
// expression-language
contains(?, "urgent")            // input.contains("urgent")
endsWith(?, "@blkit.io")         // input ends with the domain
?.year >= 2025                   // input is a date; check its year
isPublicHoliday(?, ukHolidays)   // input is a date; check against a calendar
```

Writing `?` for the implicit forms is allowed but redundant — `< 10` and
`? < 10` are the same test. A **leading dot** is shorthand for path access on the
input: `.status = "active"` is exactly `?.status = "active"`, and
`.applicant.income >= 50000` walks a nested path. Reach for an explicit `?` when
the input is an argument to a function (`contains(?, "x")`); use the leading dot
when you're projecting a field out of it.

Because a `?` expression can be any expression that yields a boolean, the unary
test can express anything the language can — the full power of the type pages
([Lists](lists.md), [Dictionaries](dictionaries.md), [Tables](tables.md)) is
available inside one.

## Tests over structured inputs

When the input is a list, dictionary, or table, the `?` form carries the query:

```
// expression-language — input is a list
count(?) > 0                            // non-empty
listContains(?, "urgent")               // membership
count(?[item.amount > 100]) >= 1        // at least one matching element
every x in ? satisfies x.qty > 0        // all elements pass
some  x in ? satisfies x.is_priority    // some element passes
```

```
// expression-language — input is a dictionary
.tier = "gold"                          // field equality (leading-dot shorthand)
.applicant.income >= 50000              // nested path
has(?, "approver")                      // key presence
size(?) > 0                             // non-empty
```

```
// expression-language — input is a table
count(?) > 0                            // non-empty
some  r in ? satisfies r.amount > 1000  // any row matches
every r in ? satisfies r.status = "ok"  // every row matches
count(?[item.flagged]) = 0              // no flagged rows
```

## Unary tests from Go

Host Go code compiles a unary test once with `bl.UnaryTest[T]` — where `T` is the
type of the implicit input — and evaluates it against many inputs by passing the
value straight to `Evaluate` (there is only the single `?`, so no wrapping):

```go
// host-side (Go)
import bl "github.com/friendly-business-machines/blkit/core"

var atLeast18, _ = bl.UnaryTest[bl.BlNumber](`>= 18`)
var isUrgent,  _ = bl.UnaryTest[bl.BlString](`contains(?, "urgent")`)
var inRange,   _ = bl.UnaryTest[bl.BlNumber](`[18..65]`)

var n21, _ = bl.Number(21)
var ok, _  = atLeast18.Evaluate(n21)   // the bl.BlBoolean true
```

Use `T = bl.BlValue` when the input type isn't fixed. The comparison and interval
forms are rejected when `T` is a type with no ordering. See
[Values from Go](values-from-go.md) for constructing the input values.
