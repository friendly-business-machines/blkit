---
name: BlDaysTimeDuration
description: blkit's days-and-time duration type (modelled on FEEL) — extends BlExpr so all operations are deferred and chainable; covers the PnDTnHnMnS subset of ISO 8601 durations
targets:
  - ../../expr/days_time_duration.go
---

# BlDaysTimeDuration

`BlDaysTimeDuration` represents a duration value covering days, hours, minutes, and seconds — no years or months. It is modelled on FEEL's `days and time duration` type and maps to the ISO 8601 `PnDTnHnMnS` format. It extends `BlExpr`, so every instance is a literal leaf node and all operations return deferred `BlExpr` nodes.

`BlDaysTimeDuration` and `BlYearsMonthsDuration` are distinct, incompatible types:

- `BlDaysTimeDuration` and `BlYearsMonthsDuration` **cannot** be added to each other. Attempting to do so evaluates to a `BlTypeError`.
- `BlDaysTimeDuration` can be added to or subtracted from `BlDate`, `BlTime`, and `BlDateTime`.

```go
type BlDaysTimeDuration struct { BlExpr }

// ------------------------------------------------------------------ //
// Construction — see bl.spec.md                                       //
// ------------------------------------------------------------------ //

// Bl.DaysTime(days, hours, minutes, seconds int) BlDaysTimeDuration
//   Normalises: seconds 0–59, minutes 0–59, hours 0–23; overflow carries
//   into days. Bl.DaysTime(0, 25, 0, 0) → P1DT1H.
// Bl.DaysTimeFromSeconds(totalSeconds float64) BlDaysTimeDuration
//   Constructs from a total-seconds count (may be fractional for sub-second
//   precision). Bl.DaysTimeFromSeconds(90) → PT1M30S.

// Parsing — accepts ISO 8601 PnDTnHnMnS form. Year or month designators
// are invalid. Raises BlParseError on non-conforming input.
func (Bl) ToDaysTime(value string) BlDaysTimeDuration { ... }

// ------------------------------------------------------------------ //
// Components — deferred; evaluate to BlNumber                        //
// ------------------------------------------------------------------ //

func (d *BlDaysTimeDuration) Days() BlNumber { ... }        // whole days component (>= 0 for positive durations)

func (d *BlDaysTimeDuration) Hours() BlNumber { ... }       // hours component, 0–23 (after normalisation)

func (d *BlDaysTimeDuration) Minutes() BlNumber { ... }     // minutes component, 0–59

func (d *BlDaysTimeDuration) Seconds() BlNumber { ... }     // seconds component, 0–59.999… (includes fractional)

func (d *BlDaysTimeDuration) TotalSeconds() BlNumber { ... } // total duration in seconds (signed)

// ------------------------------------------------------------------ //
// Sign — deferred; evaluate to BlBoolean                             //
// ------------------------------------------------------------------ //

func (d *BlDaysTimeDuration) IsNegative() BlExpr { ... }

// ------------------------------------------------------------------ //
// Arithmetic — deferred; evaluate to BlDaysTimeDuration              //
// ------------------------------------------------------------------ //

func (d *BlDaysTimeDuration) Negate() BlDaysTimeDuration { ... }
func (d *BlDaysTimeDuration) Abs() BlDaysTimeDuration { ... }
func (d *BlDaysTimeDuration) Add(other BlExpr) BlDaysTimeDuration { ... }
func (d *BlDaysTimeDuration) Subtract(other BlExpr) BlDaysTimeDuration { ... }
func (d *BlDaysTimeDuration) Multiply(factor BlExpr) BlDaysTimeDuration { ... }
// factor must evaluate to BlNumber. Result may include fractional seconds.

func (d *BlDaysTimeDuration) Divide(factor BlExpr) BlDaysTimeDuration { ... }
// factor must evaluate to BlNumber. Evaluates to BlNull on zero divisor.

// ------------------------------------------------------------------ //
// Comparison — deferred; evaluate to BlBoolean                       //
// ------------------------------------------------------------------ //

func (d *BlDaysTimeDuration) Equals(other BlExpr) BlExpr { ... }
func (d *BlDaysTimeDuration) NotEqual(other BlExpr) BlExpr { ... }
func (d *BlDaysTimeDuration) LessThan(other BlExpr) BlExpr { ... }
func (d *BlDaysTimeDuration) GreaterThan(other BlExpr) BlExpr { ... }
func (d *BlDaysTimeDuration) LessThanOrEqual(other BlExpr) BlExpr { ... }
func (d *BlDaysTimeDuration) GreaterThanOrEqual(other BlExpr) BlExpr { ... }

// ------------------------------------------------------------------ //
// Eager host-language utilities (call only after .Evaluate())         //
// ------------------------------------------------------------------ //

func (d *BlDaysTimeDuration) CompareTo(other BlDaysTimeDuration) int { ... }   // -1, 0, or 1
func (d *BlDaysTimeDuration) String() string { ... }   // ISO 8601: "P1DT2H30M" or "-PT90S"
```

---

## Construction

### `Bl.DaysTime(days, hours, minutes, seconds)`

Constructs a duration from explicit components. All components are normalised: seconds overflow into minutes, minutes into hours, hours into days.

```go
Bl.DaysTime(1, 2, 30, 0)
// → P1DT2H30M

Bl.DaysTime(0, 25, 0, 0)
// → P1DT1H   (25 hours normalised to 1 day 1 hour)

Bl.DaysTime(0, 0, 90, 0)
// → PT1H30M   (90 minutes normalised)

Bl.DaysTime(0, 0, 0, 0)
// → PT0S   (zero duration)

Bl.DaysTime(-1, 0, 0, 0)
// → -P1D   (negative one day)
```

### `BlDaysTimeDuration.of_seconds(total_seconds)`

```go
Bl.DaysTimeFromSeconds(90)
// → PT1M30S

Bl.DaysTimeFromSeconds(86400)
// → P1D   (exactly one day)

Bl.DaysTimeFromSeconds(86461)
// → P1DT1M1S

Bl.DaysTimeFromSeconds(-3600)
// → -PT1H

Bl.DaysTimeFromSeconds(1.5)
// → PT1.5S   (fractional seconds supported)
```

### `Bl.ToDaysTime(value)`

```go
Bl.ToDaysTime("P1D")
// → days=1, hours=0, minutes=0, seconds=0

Bl.ToDaysTime("PT2H")
// → days=0, hours=2, minutes=0, seconds=0

Bl.ToDaysTime("PT30M")
// → PT30M

Bl.ToDaysTime("PT90S")
// → PT1M30S   (normalised)

Bl.ToDaysTime("P1DT2H30M15.500S")
// → days=1, hours=2, minutes=30, seconds=15.5

Bl.ToDaysTime("-PT1H")
// → negative 1-hour duration

Bl.ToDaysTime("P1Y")
// → raises BlParseError (year designator not allowed)
```

---

## Components

```go
d := Bl.DaysTime(2, 3, 45, 10)

d.Days().Evaluate()          // → BlNumber("2")
d.Hours().Evaluate()         // → BlNumber("3")
d.Minutes().Evaluate()       // → BlNumber("45")
d.Seconds().Evaluate()       // → BlNumber("10")
d.TotalSeconds().Evaluate()  // → BlNumber("100510")   (2*86400 + 3*3600 + 45*60 + 10)
d.IsNegative().Evaluate()    // → BlBoolean.FALSE

neg := Bl.DaysTimeFromSeconds(-7200)
neg.IsNegative().Evaluate()  // → BlBoolean.TRUE
neg.Days().Evaluate()        // → BlNumber("0")
neg.Hours().Evaluate()       // → BlNumber("-2")
```

---

## Arithmetic

```go
Bl.DaysTime(1, 0, 0, 0).Add(
    Bl.DaysTime(0, 12, 0, 0),
).Evaluate()
// → P1DT12H

Bl.DaysTime(2, 0, 0, 0).Subtract(
    Bl.DaysTime(0, 6, 0, 0),
).Evaluate()
// → P1DT18H

Bl.DaysTime(0, 1, 0, 0).Multiply(Bl.Number(2.5)).Evaluate()
// → PT2H30M   (2.5 hours)

Bl.DaysTime(1, 0, 0, 0).Divide(Bl.Number(8)).Evaluate()
// → PT3H   (24 hours ÷ 8 = 3 hours)

Bl.DaysTime(1, 0, 0, 0).Divide(Bl.Number(0)).Evaluate()
// → BlNull   (division by zero)

Bl.DaysTime(2, 3, 0, 0).Negate().Evaluate()
// → -P2DT3H

Bl.ToDaysTime("-P1DT6H").Abs().Evaluate()
// → P1DT6H
```

---

## Comparison

Durations are compared by `total_seconds`. Representations that differ structurally but have the same total are equal.

```go
Bl.ToDaysTime("PT60S").Equals(
    Bl.ToDaysTime("PT1M"),
).Evaluate()
// → BlBoolean.TRUE   (both total_seconds = 60)

Bl.DaysTime(0, 1, 0, 0).LessThan(
    Bl.DaysTime(0, 2, 0, 0),
).Evaluate()
// → BlBoolean.TRUE

Bl.DaysTime(0, 0, 0, 0).IsNegative().Evaluate()
// → BlBoolean.FALSE   (zero is not negative)
```

---

## Cross-type Constraints

`BlDaysTimeDuration` and `BlYearsMonthsDuration` are incompatible:

- Adding one to the other evaluates to `BlTypeError`.
- Both can be added to or subtracted from `BlDate`, `BlDateTime`, and `BlTime`.
- Only `BlDaysTimeDuration` can be added to `BlTime` (time has no year/month concept).

---

## Edge Cases

- `of(0, 0, 0, 0)` and `of_seconds(0)` produce a valid zero duration. `is_negative()` evaluates to `BlBoolean.FALSE` for zero.
- `divide()` with a zero factor evaluates to `BlNull`, consistent with `BlNumber`.
- `multiply()` with a fractional factor may produce fractional seconds; implementations must preserve this precision.
- `parse()` accepts fractional seconds (`PT1.5S`) but not fractional minutes or hours.
- `seconds` evaluates to a `BlNumber` that may include a decimal component (e.g. `BlNumber("15.500")`).
