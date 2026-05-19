---
name: BlRange
description: blkit's range type — an interval with configurable boundary inclusion; extends BlExpr so membership tests are deferred and chainable
targets:
  - ../../expr/range.go
---

# BlRange

`BlRange` is blkit's range (interval) type. A range defines a contiguous interval of comparable values with configurable inclusion or exclusion at each endpoint. Ranges are used in `in_()` membership tests, decision table input entries, and explicit range literals. It extends `BlExpr`, so every instance is a literal leaf node and all operations return deferred `BlExpr` nodes.

```go
type BlRange struct { BlExpr }

// Construction is via Bl.Range(start, end, startIncluded, endIncluded).
// See bl.spec.md.

// Boundary metadata — eager (structural properties of the range literal)
// StartIncluded bool   // true → "[", false → "("
// EndIncluded   bool   // true → "]", false → ")"

// Boundary values — deferred; evaluate to the endpoint BlValue
// Start BlExpr
// End   BlExpr

// Membership — deferred; evaluates to BlBoolean
func (r *BlRange) Includes(value BlExpr) BlExpr { ... }

// Range inspection — deferred; evaluates to BlBoolean
func (r *BlRange) IsEmpty() BlExpr { ... }
// evaluates to true if range contains no values (e.g. (3..3) or (5..2))

// Equality — deferred; evaluates to BlBoolean
func (r *BlRange) Equals(other BlExpr) BlExpr { ... }
func (r *BlRange) NotEqual(other BlExpr) BlExpr { ... }

// Eager host-language utilities — only valid on a concrete BlRange after .Evaluate()
func (r *BlRange) String() string { ... }   // Literal notation: "[1..10]", "(0..1)", "[2025-01-01..2025-12-31)"
```

## Deferred semantics

```go
adult_range := Bl.Range(Bl.Number(18), Bl.Number(65), true, true)
expr := adult_range.Includes(Bl.NumberVar("age"))
result := expr.Evaluate(map[string]BlExpr{"age": Bl.Number(25)})
// result == BlBoolean.TRUE
```

## Notation

| Syntax | `start_included` | `end_included` | Meaning |
|---|---|---|---|
| `[a..b]` | `True` | `True` | closed: `a ≤ x ≤ b` |
| `(a..b)` | `False` | `False` | open: `a < x < b` |
| `[a..b)` | `True` | `False` | half-open: `a ≤ x < b` |
| `(a..b]` | `False` | `True` | half-open: `a < x ≤ b` |

## Membership

`includes(value)` evaluates to `BlBoolean.TRUE` if the value falls within the range. The value and range endpoints must be of comparable types:

- `BlNumber` ranges: numeric comparison
- `BlString` ranges: lexicographic Unicode code point comparison
- `BlDate` / `BlTime` / `BlDateTime` ranges: chronological comparison
- `BlDaysTimeDuration` / `BlYearsMonthsDuration` ranges: duration comparison by `total_seconds` / `total_months`

Calling `includes()` with an incompatible type produces a `BlTypeError` at evaluation time.

## Unbounded Ranges

Half-unbounded ranges are supported. Pass `BlNull.INSTANCE` as the unbounded endpoint. `includes()` treats a `null` start as negative infinity and a `null` end as positive infinity.

```go
// [18..null) — "18 or older"
adult_range := Bl.Range(Bl.Number(18), BlNull.INSTANCE, true, false)
adult_range.Includes(Bl.Number(25)).Evaluate()  // → BlBoolean.TRUE
adult_range.Includes(Bl.Number(17)).Evaluate()  // → BlBoolean.FALSE
```

## Use in Unary Tests

In a decision table input entry, range membership is expressed as a standard `BlExpr`: `Bl.number_var("Age").in_(Bl.range_(Bl.number(18), Bl.number(65)))`. This evaluates to `BlBoolean`.

## Edge Cases

- A range where `start > end` is empty; `is_empty()` evaluates to `BlBoolean.TRUE` and `includes()` always evaluates to `BlBoolean.FALSE`.
- `(3..3)` is empty; `[3..3]` contains exactly one value.
- `start_included` and `end_included` are eager `bool` fields on the concrete `BlRange` value (not deferred), since they describe the structural shape of the range literal.
- A closed boundary with a `BlNull` endpoint produces a `BlTypeError` at evaluation time.
- Comparing ranges of mixed temporal types (e.g. `BlDate` vs `BlDateTime` endpoints) produces a `BlTypeError` at evaluation time.
