---
name: BlExpr
description: The deferred expression tree — the base class all blkit value types extend; provides only universal operations (equality, logical, navigation, type tests); type-specific operations live on each concrete type
targets:
  - ../../expr/expr.go
---

# BlExpr

`BlExpr` is the abstract base class for every node in a blkit expression tree. All blkit value types (`BlNumber`, `BlString`, `BlBoolean`, etc.) extend `BlExpr`, making every constructed value a valid starting point for a deferred expression chain.

Expression building is always lazy. Methods return a new expression node without evaluating anything. The expression is only evaluated when `.evaluate()` is called explicitly.

`BlExpr` provides **only universal operations** — those that are valid on any blkit value type. Type-specific operations (arithmetic, ordering, string manipulation, date navigation, etc.) are defined on each concrete type. This ensures that invalid operations (e.g. multiplying a string) are caught at compile time rather than at evaluation time.

```go
type BlValue interface{} // BlNumber | BlString | BlBoolean |
    // BlDate | BlTime | BlDateTime |
    // BlYearsMonthsDuration | BlDaysTimeDuration |
    // BlList | BlContext | BlRange | BlNull |
    // BlCalendar  // blkit-specific type


type BlExpr interface {

    // ------------------------------------------------------------------ //
    // Terminal evaluation — the only eager operation                       //
    // ------------------------------------------------------------------ //

    Evaluate(context map[string]any) BlValue
    // Evaluates the entire expression tree and returns a concrete BlValue.
    // `context` is the variable map used to resolve variable references.
    // Raises BlTypeError on type mismatches, BlEvaluationError on other failures.

    // ------------------------------------------------------------------ //
    // Equality — universal; works on any blkit value type                 //
    // ------------------------------------------------------------------ //

    Equals(other BlExpr) BlExpr       // evaluates to BlBoolean
    NotEqual(other BlExpr) BlExpr     // evaluates to BlBoolean

    // ------------------------------------------------------------------ //
    // Logical — universal; three-valued logic                              //
    // ------------------------------------------------------------------ //

    And(other BlExpr) BlExpr   // evaluates to BlBoolean or BlNull
    Or(other BlExpr) BlExpr    // evaluates to BlBoolean or BlNull
    Not() BlExpr               // evaluates to BlBoolean or BlNull

    // ------------------------------------------------------------------ //
    // Navigation and filtering — universal                                //
    // ------------------------------------------------------------------ //

    Index(i BlExpr) BlExpr
    // 1-indexed list access. Negative indices count from the end.
    // e.g. Bl.ListVar("items").Index(Bl.Number(1))
    // Evaluates to BlNull if out of range.

    Filter(predicate BlExpr) BlExpr
    // List filter. Evaluates predicate for each element (with element bound as context);
    // returns a BlList of elements for which the predicate is truthy.
    // e.g. Bl.ListVar("orders").Filter(Bl.NumberVar("amount").GreaterThan(Bl.Number(1000)))

    // ------------------------------------------------------------------ //
    // Type tests — universal                                              //
    // ------------------------------------------------------------------ //

    InstanceOf(typeName string) BlExpr
    // Evaluates to BlBoolean. typeName is a blkit type name:
    // "number", "string", "boolean", "date", "time", "date and time",
    // "days and time duration", "years and months duration", "list", "context", "Any".

    In(test BlExpr) BlExpr
    // Membership test — checks if self is contained in test.
    // test may be a BlList, BlRange, or BlCalendar.
    // Evaluates to BlBoolean.
    // e.g. Bl.StringVar("status").In(Bl.List(Bl.String("active"), Bl.String("pending")))
    // e.g. Bl.NumberVar("age").In(Bl.Range(Bl.Number(18), Bl.Number(65), true, true))

    // ------------------------------------------------------------------ //
    // Text rendering                                                      //
    // ------------------------------------------------------------------ //

    ToMarkdown() string
    // Returns a human-readable syntax string representing this expression
    // tree. The output is intended for inspection and rendering; it is not
    // guaranteed to round-trip through any parser.
}
```

---

## Text Rendering

`to_markdown()` returns a human-readable syntax string representing the expression tree. This is useful for debugging, logging, serialisation, and rendering expressions in human-readable form.

The output uses a compact literal syntax:

```go
Bl.Number(42).ToMarkdown()                              // "42"
Bl.String("hello").ToMarkdown()                         // '"hello"'
Bl.Boolean(true).ToMarkdown()                           // "true"
Bl.Null().ToMarkdown()                                  // "null"
Bl.Date(2025, 3, 28).ToMarkdown()                       // 'date("2025-03-28")'
Bl.Time(14, 30, 0).ToMarkdown()                         // 'time("14:30:00")'
Bl.DateTime(2025, 3, 28, 14, 30, 0).ToMarkdown()        // 'date and time("2025-03-28T14:30:00")'
Bl.YearsMonths(1, 6).ToMarkdown()                       // 'duration("P1Y6M")'
Bl.DaysTime(2, 12, 0, 0).ToMarkdown()                   // 'duration("P2DT12H")'

Bl.Number(10).Add(Bl.Number(5)).ToMarkdown()            // "10 + 5"
Bl.NumberVar("x").Multiply(Bl.Number(2)).ToMarkdown()   // "x * 2"
Bl.StringVar("name").UpperCase().ToMarkdown()           // "upper case(name)"

Bl.NumberVar("age").GreaterThan(Bl.Number(18)).
    And(Bl.NumberVar("age").LessThan(Bl.Number(65))).
    ToMarkdown()                                         // "age > 18 and age < 65"

Bl.If(
    Bl.NumberVar("score").GreaterThanOrEqual(Bl.Number(90)),
    Bl.String("pass"),
    Bl.String("fail"),
).ToMarkdown()                                           // 'if score >= 90 then "pass" else "fail"'

Bl.List(Bl.Number(1), Bl.Number(2)).ToMarkdown()        // "[1, 2]"
Bl.Context(map[string]BlExpr{"a": Bl.Number(1)}).ToMarkdown()  // "{a: 1}"

Bl.For("x", Bl.ListVar("items")).
    Return(Bl.NumberVar("x").Multiply(Bl.Number(2))).
    ToMarkdown()                                         // "for x in items return x * 2"

Bl.Some("x", Bl.ListVar("items")).
    Satisfies(Bl.NumberVar("x").GreaterThan(Bl.Number(0))).
    ToMarkdown()                                         // "some x in items satisfies x > 0"
```

`to_markdown()` does not evaluate the expression — it renders the tree structure as text. Variable references render as their variable name, not their resolved value.

---

## What Is NOT on BlExpr

The following operations are **not** on `BlExpr`. They live on the concrete types that support them:

| Operation | Defined on |
|---|---|
| Arithmetic (`add`, `subtract`, `multiply`, `divide`, `negate`, etc.) | `BlNumber`, `BlDate`, `BlTime`, `BlDateTime`, `BlYearsMonthsDuration`, `BlDaysTimeDuration` |
| Ordering (`less_than`, `greater_than`, `before`, `after`, etc.) | `BlNumber`, `BlString`, `BlDate`, `BlTime`, `BlDateTime`, `BlYearsMonthsDuration`, `BlDaysTimeDuration` |
| String operations (`upper_case`, `contains`, `split`, etc.) | `BlString` |
| Date navigation (`next_business_day`, `is_weekend`, etc.) | `BlDate` |
| List operations (`count`, `append`, `sort`, etc.) | `BlList` |
| Context operations (`get`, `put`, `keys`, etc.) | `BlContext` |
| Range operations (`includes`, `is_empty`, etc.) | `BlRange` |
| Calendar operations (`contains`, `entries_for`, etc.) | `BlCalendar` |

To use type-specific operations on a variable reference, the caller declares the expected type via the matching typed factory:

```go
Bl.NumberVar("price").Add(Bl.Number(1))    // ✓ BlNumber has Add()
Bl.StringVar("name").UpperCase()           // ✓ BlString has UpperCase()
Bl.DateVar("start").IsWeekend()            // ✓ BlDate has IsWeekend()

Bl.NumberVar("price").UpperCase()          // ✗ compile error — BlNumber has no UpperCase()
```

Universal ops (`Equals`, `In`, `InstanceOf`, `Index`, `Filter`, logical) are inherited from `BlExpr`, so they remain available on every typed variable.

---

## Return Type Principle

Type-specific methods return **the most specific type** their result evaluates to. This enables type-safe chaining:

```go
// Each method returns BlNumber, so the full chain is type-safe
Bl.Number(42).Add(Bl.Number(8)).Multiply(Bl.Number(2)).Round(Bl.Number(0))

// BlDate methods return BlDate, so date operations chain
Bl.Date(2025, 3, 28).Add(Bl.DaysTime(7, 0, 0, 0)).NextWeekday()

// Cross-type results return the appropriate type
Bl.Date(2025, 1, 1).DiffDaysTime(Bl.Date(2025, 3, 28))  // → BlDaysTimeDuration
Bl.Date(2025, 3, 28).AtTime(Bl.Time(14, 30, 0))      // → BlDateTime
```

Methods that produce a boolean result (equality, comparisons, tests) return `BlExpr`, since the logical operators (`and_`, `or_`, `not_`) are already on `BlExpr` and further boolean chaining is universal:

```go
Bl.NumberVar("age").GreaterThan(Bl.Number(18)).
    And(Bl.NumberVar("age").LessThan(Bl.Number(65)))
// GreaterThan returns BlExpr; And() is on BlExpr — chain works
```

---

## BlForBuilder

`BlForBuilder` is returned by `Bl.for_()`. It supports multi-variable iteration (nested `for`) before the mandatory `.return_()` terminal.

```go
type BlForBuilder struct { ... }

func (b *BlForBuilder) For(varName string, collection BlExpr) *BlForBuilder { ... }
// Adds a second (or further) iteration variable.
// e.g. Bl.For("i", rows).For("j", cols).Return(...)

func (b *BlForBuilder) Return(body BlExpr) BlExpr { ... }
// Completes the for-expression. Evaluates to a BlList.
```

---

## BlQuantifierBuilder

`BlQuantifierBuilder` is returned by `Bl.some()` and `Bl.every()`.

```go
type BlQuantifierBuilder struct { ... }

func (b *BlQuantifierBuilder) Satisfies(condition BlExpr) BlExpr { ... }
// Completes the quantified expression. Evaluates to BlBoolean or BlNull
// (BlNull when the collection is empty).
```

---

## How Value Types Fit In

Every blkit value type (`BlNumber`, `BlString`, etc.) extends `BlExpr`. A constructed value instance is a **literal leaf node** in the expression tree. Its `evaluate()` returns itself.

```go
// BlNumber is a BlExpr — a literal node
n := Bl.Number(42)
// n.Evaluate() == Bl.Number(42)   // identity

// Chain type-specific operations on a concrete value
expr := Bl.Number(42).Add(Bl.Number(8)).Multiply(Bl.Number(2))
result := expr.Evaluate(nil)
// result == Bl.Number(100)

// Typed variable reference — use type-specific operations on a variable
expr = Bl.NumberVar("price").Add(Bl.Number(1))
result = expr.Evaluate(map[string]any{"price": Bl.Number(99)})
// result == Bl.Number(100)

// Universal operations on a typed variable — Equals, In, InstanceOf, etc.
expr = Bl.NumberVar("x").Equals(Bl.Number(42))
result = expr.Evaluate(map[string]any{"x": Bl.Number(42)})
// result == Bl.Boolean(true)

// Conditional expression
expr = Bl.If(
    Bl.NumberVar("score").GreaterThanOrEqual(Bl.Number(90)),
    Bl.String("pass"),
    Bl.String("fail"),
)
result = expr.Evaluate(map[string]any{"score": Bl.Number(95)})
// result == Bl.String("pass")
```

### Typed Variable References at Evaluation Time

A typed variable reference (e.g. `Bl.number_var("x")`) declares the caller's expectation about the variable's type. If the variable resolves to a different type at `.evaluate()` time, a `BlTypeError` is raised:

```go
expr := Bl.NumberVar("x").Add(Bl.Number(1))
expr.Evaluate(map[string]any{"x": Bl.Number(5)})     // → BlNumber(6) — works
expr.Evaluate(map[string]any{"x": Bl.String("hi")})  // → raises BlTypeError
```

---

## Relationship to Bl Entry Point

`Bl` (see [bl.spec.md](bl.spec.md)) is the primary entry point for constructing values and expressions. It provides factory methods (`Bl.number()`, `Bl.string()`, `Bl.date()`, etc.), typed variable references (`Bl.number_var()`, `Bl.string_var()`, etc.), and structural expression builders (`Bl.var()`, `Bl.list_()`, `Bl.if_()`, etc.).

`Bl` is the canonical entry point for external callers.

---

## Null Propagation

All methods propagate `BlNull` through the expression tree at evaluation time. When a sub-expression evaluates to `BlNull`, the result of the parent expression is `BlNull` (except for the three-valued logic carve-outs, e.g. `false and null` → `false`).

This propagation happens at `.evaluate()` time, not at tree-construction time.

---

## Type Coercion

`BlExpr` does not coerce native host-language values. Method arguments must be `BlExpr` instances. To wrap a native value, use the `Bl` factory:

```go
Bl.Number(42)          // not: .Add(42)
Bl.String("hello")     // not: .Equals("hello")
Bl.Boolean(true)       // not: .And(true)
```

---

## Edge Cases

- A typed variable reference whose name is not in the context evaluates to `BlNull`.
- A typed variable reference whose context value is the wrong type raises `BlTypeError` at `.evaluate()` time.
- `Bl.list_()` with no arguments evaluates to an empty `BlList`.
- `Bl.context_({})` evaluates to an empty `BlContext`.
- `Bl.for_("x", Bl.list_()).return_(...)` evaluates to an empty `BlList` (empty collection).
- `Bl.some("x", Bl.list_()).satisfies(...)` evaluates to `BlNull` (empty collection).
- `Bl.every("x", Bl.list_()).satisfies(...)` evaluates to `BlBoolean.TRUE` (vacuous truth).
