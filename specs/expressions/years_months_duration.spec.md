---
name: BlYearsMonthsDuration
description: blkit's years-and-months duration type (modelled on FEEL) — extends BlExpr so all operations are deferred and chainable; covers the PnYnM subset of ISO 8601 durations
targets:
  - ../../expr/years_months_duration.go
---

# BlYearsMonthsDuration

`BlYearsMonthsDuration` represents a duration value covering only years and months — no days, hours, minutes, or seconds. It is modelled on FEEL's `years and months duration` type and maps to the ISO 8601 `PnYnM` format. It extends `BlExpr`, so every instance is a literal leaf node and all operations return deferred `BlExpr` nodes.

`BlYearsMonthsDuration` and `BlDaysTimeDuration` are distinct, incompatible types:

- `BlYearsMonthsDuration` and `BlDaysTimeDuration` **cannot** be added to each other. Attempting to do so evaluates to a `BlTypeError`.
- `BlYearsMonthsDuration` can be added to or subtracted from `BlDate` and `BlDateTime`.
- `BlYearsMonthsDuration` **cannot** be added to or subtracted from `BlTime` — time has no year/month concept.

```go
type BlYearsMonthsDuration struct { BlExpr }

// ------------------------------------------------------------------ //
// Construction — see bl.spec.md                                       //
// ------------------------------------------------------------------ //

// Bl.YearsMonths(years, months int) BlYearsMonthsDuration
//   Normalises months so the months component is in 0–11.
//   Bl.YearsMonths(0, 15) → P1Y3M; Bl.YearsMonths(-1, -6) → -P1Y6M.
// Bl.YearsMonthsFromMonths(totalMonths int) BlYearsMonthsDuration
//   Constructs from a total month count. Normalises to years + months.
//   Bl.YearsMonthsFromMonths(14) → P1Y2M; Bl.YearsMonthsFromMonths(-3) → -P3M.

// Parsing — accepts ISO 8601 PnYnM form. P designators for days/hours/
// minutes/seconds are invalid. Raises BlParseError on non-conforming input.
func (Bl) ToYearsMonths(value string) BlYearsMonthsDuration { ... }

// ------------------------------------------------------------------ //
// Components — deferred; evaluate to BlNumber                        //
// ------------------------------------------------------------------ //

func (d *BlYearsMonthsDuration) Years() BlNumber { ... }        // whole years component (>= 0)

func (d *BlYearsMonthsDuration) Months() BlNumber { ... }       // months component, 0–11 (after normalisation)

func (d *BlYearsMonthsDuration) TotalMonths() BlNumber { ... }  // years * 12 + months (signed)

// ------------------------------------------------------------------ //
// Sign — deferred; evaluate to BlBoolean                             //
// ------------------------------------------------------------------ //

func (d *BlYearsMonthsDuration) IsNegative() BlExpr { ... }

// ------------------------------------------------------------------ //
// Arithmetic — deferred; evaluate to BlYearsMonthsDuration           //
// ------------------------------------------------------------------ //

func (d *BlYearsMonthsDuration) Negate() BlYearsMonthsDuration { ... }
func (d *BlYearsMonthsDuration) Abs() BlYearsMonthsDuration { ... }
func (d *BlYearsMonthsDuration) Add(other BlExpr) BlYearsMonthsDuration { ... }
func (d *BlYearsMonthsDuration) Subtract(other BlExpr) BlYearsMonthsDuration { ... }
func (d *BlYearsMonthsDuration) Multiply(factor BlExpr) BlYearsMonthsDuration { ... }
// factor must evaluate to BlNumber. Fractional results are rounded to the nearest whole month.

func (d *BlYearsMonthsDuration) Divide(factor BlExpr) BlYearsMonthsDuration { ... }
// factor must evaluate to BlNumber. Evaluates to BlNull on zero divisor.
// Result is rounded to the nearest whole month.

// ------------------------------------------------------------------ //
// Comparison — deferred; evaluate to BlBoolean                       //
// ------------------------------------------------------------------ //

func (d *BlYearsMonthsDuration) Equals(other BlExpr) BlExpr { ... }
func (d *BlYearsMonthsDuration) NotEqual(other BlExpr) BlExpr { ... }
func (d *BlYearsMonthsDuration) LessThan(other BlExpr) BlExpr { ... }
func (d *BlYearsMonthsDuration) GreaterThan(other BlExpr) BlExpr { ... }
func (d *BlYearsMonthsDuration) LessThanOrEqual(other BlExpr) BlExpr { ... }
func (d *BlYearsMonthsDuration) GreaterThanOrEqual(other BlExpr) BlExpr { ... }

// ------------------------------------------------------------------ //
// Eager host-language utilities (call only after .Evaluate())         //
// ------------------------------------------------------------------ //

func (d *BlYearsMonthsDuration) CompareTo(other BlYearsMonthsDuration) int { ... }   // -1, 0, or 1
func (d *BlYearsMonthsDuration) String() string { ... }   // ISO 8601: "P1Y2M" or "-P6M"
```

---

## Construction

### `Bl.YearsMonths(years, months)`

Constructs a duration from explicit year and month components. The `months` component is normalised to `0–11`; any overflow (or underflow for negative values) is carried into `years`.

```go
Bl.YearsMonths(1, 6)
// → P1Y6M   (years=1, months=6)

Bl.YearsMonths(0, 15)
// → P1Y3M   (normalised: 15 months = 1 year 3 months)

Bl.YearsMonths(0, -3)
// → -P3M    (negative three months)

Bl.YearsMonths(-2, 0)
// → -P2Y

Bl.YearsMonths(0, 0)
// → P0M     (zero duration)
```

### `BlYearsMonthsDuration.of_months(total_months)`

Constructs from a signed total-months count.

```go
Bl.YearsMonthsFromMonths(14)
// → P1Y2M   (14 months = 1 year 2 months)

Bl.YearsMonthsFromMonths(-18)
// → -P1Y6M

Bl.YearsMonthsFromMonths(0)
// → P0M
```

### `Bl.ToYearsMonths(value)`

Parses an ISO 8601 years-months duration string.

```go
Bl.ToYearsMonths("P1Y")
// → years=1, months=0

Bl.ToYearsMonths("P6M")
// → years=0, months=6

Bl.ToYearsMonths("P1Y6M")
// → years=1, months=6

Bl.ToYearsMonths("-P1Y6M")
// → negative duration, years=1, months=6

Bl.ToYearsMonths("P1DT2H")
// → raises BlParseError (day/time designators not allowed)
```

---

## Components

```go
d := Bl.YearsMonths(2, 7)

d.Years().Evaluate()        // → BlNumber("2")
d.Months().Evaluate()       // → BlNumber("7")
d.TotalMonths().Evaluate()  // → BlNumber("31")   (2 * 12 + 7)

neg := Bl.YearsMonths(-1, -3)
neg.Years().Evaluate()        // → BlNumber("-1")
neg.Months().Evaluate()       // → BlNumber("-3")   (sign is carried through)
neg.TotalMonths().Evaluate()  // → BlNumber("-15")
neg.IsNegative().Evaluate()   // → BlBoolean.TRUE
```

---

## Arithmetic

```go
Bl.YearsMonths(1, 0).Add(
    Bl.YearsMonths(0, 6),
).Evaluate()
// → P1Y6M

Bl.YearsMonths(2, 3).Subtract(
    Bl.YearsMonths(0, 6),
).Evaluate()
// → P1Y9M   (2Y3M - 6M = 1Y9M)

Bl.YearsMonths(0, 6).Multiply(Bl.Number(3)).Evaluate()
// → P1Y6M   (6 months × 3 = 18 months = 1Y6M)

Bl.YearsMonths(1, 0).Divide(Bl.Number(4)).Evaluate()
// → P3M   (12 months ÷ 4 = 3 months)

Bl.YearsMonths(0, 7).Divide(Bl.Number(2)).Evaluate()
// → P3M   (3.5 months → rounded to 4M or 3M depending on rounding rule — rounds to nearest: 4M)
// Note: result is P4M (3.5 rounds to 4 under round-half-up)

Bl.YearsMonths(1, 0).Divide(Bl.Number(0)).Evaluate()
// → BlNull   (division by zero)

Bl.YearsMonths(1, 6).Negate().Evaluate()
// → -P1Y6M

Bl.ToYearsMonths("-P2Y3M").Abs().Evaluate()
// → P2Y3M
```

---

## Comparison

Durations are compared by `total_months`. Representations that differ structurally but have the same total are equal.

```go
Bl.YearsMonths(1, 0).Equals(
    Bl.YearsMonthsFromMonths(12),
).Evaluate()
// → BlBoolean.TRUE   (P1Y == P12M)

Bl.YearsMonths(0, 6).LessThan(
    Bl.YearsMonths(1, 0),
).Evaluate()
// → BlBoolean.TRUE   (6 months < 12 months)

Bl.YearsMonths(-1, 0).LessThan(
    Bl.YearsMonths(0, 0),
).Evaluate()
// → BlBoolean.TRUE   (negative < zero)
```

---

## Edge Cases

- `of(0, 0)` and `of_months(0)` produce a valid zero duration. `is_negative()` evaluates to `BlBoolean.FALSE` for zero.
- Adding a `BlYearsMonthsDuration` to a `BlDaysTimeDuration` evaluates to `BlTypeError` — the types are incompatible.
- `multiply()` with a fractional `BlNumber` rounds the resulting total-months to the nearest whole month (half-up tie-breaking).
- `divide()` with a zero divisor evaluates to `BlNull`, consistent with `BlNumber.divide()`.
- `parse()` rejects the compact ISO 8601 basic form (`P1Y6M` is accepted; `P0001-06` is not).
