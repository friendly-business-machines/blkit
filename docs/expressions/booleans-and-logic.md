# Booleans & Logic

> Boolean values and blkit's three-valued (true / false / null) logic,
> including and, or, not, and comparison results.

The `boolean` type has exactly two values, `true` and `false`. What sets blkit
apart from a general-purpose language is what happens at the edges: a *missing*
or *unknown* value is `null`, and `null` flows through logical operators in a
defined, SQL-style way rather than throwing or quietly defaulting to `false`.
That is **three-valued logic**, and it is what this page is really about — the
booleans themselves are the easy part.

The `boolean` type has just those two values. The wrinkle is that an absent or
unknown value is `null`, and `null` flows through the logical operators rather
than being treated as `false`. The operators below — `and`, `or`, `not`, and
equality — implement this three-valued logic.

## Literals

A boolean literal is how you write a constant `true` or `false` inside an
expression. Lowercase is canonical, but the parser accepts any casing:

```
// expression-language
true, True, TRUE                   // → true
false, False, FALSE                // → false
```

Lowercase is the recommended style for hand-written expressions — a `boolean`
always renders back as `"true"` / `"false"`. The mixed-case forms exist to ease
porting from SQL-style dialects and to spare anyone who types `True` out of
habit; they don't introduce extra literal values.

Because every casing of `true` / `false` is reserved as a literal, a variable
named `True`, `FALSE`, and so on is **not addressable**. An env field whose
source-level name collides with a boolean literal in any casing is a compile
error when you call `bl.Expr` (see [Values from Go](values-from-go.md) for how
env fields are declared).

## Operators

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `and` | logical and (three-valued) | `true and false` | `false` |
| `or` | logical or (three-valued) | `true or false` | `true` |
| `not(x)` | logical negation | `not(true)` | `false` |
| `=` `!=` | equality | `true = true` | `true` |

`not` is written as a function call, `not(x)`, not a prefix operator.

Booleans have **no truthy/falsy coercion**. A non-boolean operand to `and`,
`or`, or `not` produces `null` — it is *not* converted to a boolean the way a
non-empty string or non-zero number might be in other languages:

```
// expression-language
not(2 = 4)                         // → true
"yes" and true                     // → null  (no coercion; "yes" is not a boolean)
not(5)                             // → null
```

Booleans have no arithmetic operators, no ordering operators (`<`, `<=`, `>`,
`>=`), and no `in` operator.

## Three-valued logic

`null` stands for an *unknown* boolean — not a third truth value, but "we don't
know yet". Logical operators combine known and unknown operands by the SQL
truth table:

| `a` | `b` | `a and b` | `a or b` |
|---|---|---|---|
| `true`  | `true`  | `true`  | `true`  |
| `true`  | `false` | `false` | `true`  |
| `true`  | `null`  | `null`  | `true`  |
| `false` | `true`  | `false` | `true`  |
| `false` | `false` | `false` | `false` |
| `false` | `null`  | `false` | `null`  |
| `null`  | `true`  | `null`  | `true`  |
| `null`  | `false` | `false` | `null`  |
| `null`  | `null`  | `null`  | `null`  |

The intuition: a result is only `null` when the unknown operand could still
change the answer. `false and X` is `false` no matter what `X` turns out to be,
so it is a definite `false` even when `X` is `null`; but `true and X` depends
entirely on `X`, so an unknown `X` makes the whole thing unknown.

Negation follows the same idea — negating an unknown stays unknown:

```
// expression-language
not(null)                          // → null
```

### Short-circuiting

Two of the rows above are also **short-circuits**: the operator can decide the
result from the left operand alone and never evaluates the right one.

- `false and X` → `false` — `X` is never evaluated.
- `true or X` → `true` — `X` is never evaluated.

This matters when the right-hand operand could fail or is expensive: guarding it
behind a cheap test on the left means it only runs when it actually matters.

```
// expression-language
// If the list is empty, the average is never computed.
count(items) > 0 and mean(items) > 100
```

### Equality is never null

Equality is the one place `null` does *not* propagate into `null`. Comparing
anything to `null` with `=` or `!=` yields `false`, never `null` — SQL's rule
that null is never equal to anything, including itself:

```
// expression-language
null = null                        // → false
null != null                       // → false
true = null                        // → false
```

The practical consequence: **never test for absence with `x = null`** — it is
always `false`, so the branch never fires. Use `isNull(x)` (or `x instance of
null`) instead, covered in [Null](#null) below.

## Null

`null` is its own value type, with a single value written `null`. It means a
value is **absent or unknown** — there is no data, or the data isn't known yet.
Beyond literals you type yourself, the engine produces `null` for any operation
whose normal result is undefined:

- a dictionary key that isn't present,
- a list index out of range,
- division by zero,
- any arithmetic or path expression whose operand was already `null`.

```
// expression-language
applicant.middleName                // → null if the key is absent
items[99]                           // → null if the list is shorter
10 / 0                              // → null
null + 1                            // → null  (null propagates)
```

That last line is the key behaviour: `null` **propagates**. An operation on
another type — numeric addition, string concatenation, path access — that
receives a `null` operand yields `null` rather than throwing. Combined with the
three-valued logic above, this lets partial data flow through an expression and
surface as a `null` result instead of an error.

### Testing for null

Because `x = null` is always `false` (see [Equality is never null](#equality-is-never-null)),
there are two dedicated ways to test for null inside an expression:

```
// expression-language
isNull(applicant.middleName)        // → true if the key is missing
isNull(0)                           // → false  (zero is a defined value)
isNull("")                          // → false  (empty string is defined)
null instance of null               // → true
```

`isNull(x)` and `x instance of null` are equivalent. Prefer `isNull(x)` for
brevity; `x instance of null` reads naturally alongside other `instance of`
type tests. Both fire **only** on `null` — a defined-but-empty value (`0`, `""`,
the empty list `[]`) is not null.

### Supplying a fallback

`getOrElse(value, default)` returns `default` when `value` is `null`, and
`value` unchanged otherwise. It is the canonical null fallback — shorter than
`if isNull(x) then d else x`, and it doesn't evaluate `x` twice:

```
// expression-language
getOrElse(applicant.middleName, "")  // → "" when the name is absent
getOrElse(null, 1)                   // → 1
getOrElse(42, 1)                     // → 42
```

Like `isNull`, it only fires on `null`: a defined-but-empty value is returned
as-is, not replaced.

## Booleans from Go

Host Go code builds `boolean` values with the `bl.Boolean` constructor (a Go
`bool`, an integer, or a `"true"`/`"false"` string), and models an **unknown**
boolean with `bl.Null()` — which then flows through the three-valued logic above.
See [Values from Go](values-from-go.md) for the full host-side story.
