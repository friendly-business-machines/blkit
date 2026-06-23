# Booleans & Logic

> Boolean values and blkit's three-valued (true / false / null) logic,
> including and, or, not, and comparison results.

The `boolean` type has exactly two values, `true` and `false`. What sets blkit
apart from a general-purpose language is what happens at the edges: a *missing*
or *unknown* value is `null`, and `null` flows through logical operators in a
defined, SQL-style way rather than throwing or quietly defaulting to `false`.
That is **three-valued logic**, and it is what this page is really about — the
booleans themselves are the easy part.

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
error when you call `bl.Expr` (see [Data Types](data-types.md) for how env
fields are declared).

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
null`) instead. See [Data Types → Null](data-types.md) for the full story on
null and the null-handling helpers.

## Building booleans host-side

Host Go code constructs a `bl.BlBoolean` with the generic `bl.Boolean`
constructor. It accepts a Go `bool` directly, any Go integer (C convention:
`0` → `false`, non-zero → `true`), or a case-insensitive `"true"` / `"false"`
string:

```go
// host-side (Go)
// Direct from a Go bool — infallible.
var approved, _ = bl.Boolean(true)
var rejected, _ = bl.Boolean(false)

// From an integer — 0 → false, any non-zero → true.
var flag,   _ = bl.Boolean(1)          // → bl.BlBoolean(true)
var noFlag, _ = bl.Boolean(0)          // → bl.BlBoolean(false)

// From a string — case-insensitive; an unrecognised string errors.
var fromConf,  _ = bl.Boolean("true")  // → bl.BlBoolean(true)
var fromMixed, _ = bl.Boolean("True")  // → bl.BlBoolean(true)
var bad, err     = bl.Boolean("yes")   // err != nil — not a recognised literal
```

`bl.Boolean(...)` returns `(bl.BlBoolean, error)`. The only failure mode is a
string that isn't a case-variant of `"true"` or `"false"` — `"yes"`, `"1"`, and
`""` all error. Integer-shaped strings like `"1"` and `"0"` are intentionally
rejected so the string path mirrors the language's literal rules; convert to an
integer first if you want `0`/non-zero coercion. Go `bool` and integer inputs
are infallible.

To model an **unknown** boolean from host code, don't reach for a `*bool` — pass
`bl.Null()`, which then propagates through the three-valued logic above:

```go
// host-side (Go)
var hasConsent bl.BlValue
if maybeConsent != nil {
    hasConsent, _ = bl.Boolean(*maybeConsent)
} else {
    hasConsent = bl.Null()              // unknown, not false
}
```

## A worked example

Compile once, evaluate many times — the same pattern as every blkit expression
(see [Architecture → Expressions](../architecture/expressions.md)):

```go
// host-side (Go)
type applicant struct {
    Age     bl.BlNumber  `expr:"age"`
    Consent bl.BlValue   `expr:"consent"`   // may be a BlBoolean or BlNull
}

var eligible, _ = bl.Expr[applicant](`age >= 18 and consent`)

// Known consent:
var yes, _ = bl.Boolean(true)
var age, _ = bl.Number(20)
var r1, _  = eligible.Evaluate(applicant{Age: age, Consent: yes})   // → true

// Unknown consent: the result is null (unknown), not a silent false.
var r2, _ = eligible.Evaluate(applicant{Age: age, Consent: bl.Null()}) // → null
```

That `null` result is the point of three-valued logic: "this applicant is old
enough, but we don't yet know whether they consented" is a genuinely different
answer from "this applicant is not eligible", and blkit keeps the two distinct
instead of collapsing them to `false`.

## Edge cases

- **No truthy/falsy coercion** — a non-boolean operand to `and` / `or` / `not`
  is `null`, never a coerced boolean.
- **Literals are case-insensitive on input** (`true`, `True`, `TRUE`); lowercase
  is canonical on output. An env field colliding with a boolean literal in any
  casing is rejected at compile time.
- **Equality against `null` is `false`**, never `null`; cross-type equality
  (`true = 1`) is `false` too.
- **Short-circuit operators skip their right operand** — `false and X` and
  `true or X` never evaluate `X`, so a failing or expensive right-hand side is
  not run.

---

The behaviour on this page is defined authoritatively by
`specs/expressions/boolean.spec.md` (and `specs/expressions/null.spec.md` for
null propagation). For the host-side API, see the generated
[Reference](../reference/blkit.md); for how three-valued lowering works inside
the engine, see [Architecture → Expressions](../architecture/expressions.md).
