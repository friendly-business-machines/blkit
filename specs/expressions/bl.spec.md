---
name: Bl
description: The Bl entry point — a static (no-instance) class providing factory methods for constructing typed values and structural expression builders for composing deferred expression trees
targets:
  - ../expr/feel.go
  - ../expr/types.go
---

# Bl

`Bl` is the primary entry point for the `blkit.expr` namespace. It is a static (no-instance) class — it is never instantiated. All methods are static factory methods or structural expression builders.

`Bl` provides three categories of methods:

- **Factory methods** — create concrete blkit values (`BlNumber`, `BlString`, `BlDate`, etc.).
- **Typed variable references** — create variable references that carry type information, enabling type-safe chaining of type-specific operations.
- **Structural expression builders** — create composite expression nodes: lists, contexts, ranges, conditionals, loops, untyped variable references, and function calls.

Every value produced by `Bl` is a `BlExpr` node. Values can be chained into deferred expression trees using the methods defined on `BlExpr` (see [bl-expr.spec.md](bl-expr.spec.md)). The terminal `.evaluate()` call materialises the result.

```go
// Bl is never instantiated. All methods are package-level functions on the Bl object.

// ------------------------------------------------------------------ //
// Factory methods — create concrete blkit values                      //
// ------------------------------------------------------------------ //

func (Bl) Number(value any) BlNumber { ... }
// Creates an arbitrary-precision decimal.
// Bl.Number(42)   Bl.Number(3.14)   Bl.Number("999999999999999999.99")

func (Bl) String(value string) BlString { ... }
// Creates a string value.
// Bl.String("hello")

func (Bl) Boolean(value bool) BlBoolean { ... }
// Creates a boolean value.
// Bl.Boolean(true)   Bl.Boolean(false)

func (Bl) Date(year, month, day int, opts ...DateOption) BlDate { ... }
// Creates a calendar date. Optional offset or timezone (blkit extension).
// Bl.Date(2025, 3, 28)
// Bl.Date(2025, 3, 28, WithTimezone("Europe/London"))

func (Bl) Today(timezone ...string) BlDate { ... }
// Current date. Local system timezone if not provided.
// Bl.Today()   Bl.Today("America/New_York")

func (Bl) Time(hour, minute, second int, opts ...TimeOption) BlTime { ... }
// Creates a time-of-day value.
// Bl.Time(14, 30, 0)   Bl.Time(14, 30, 0, WithOffset("+05:30"))

func (Bl) DateTime(year, month, day, hour, minute, second int, opts ...DateTimeOption) BlDateTime { ... }
// Creates a combined date-and-time value.
// Bl.DateTime(2025, 3, 28, 14, 30, 0)

func (Bl) Now(timezone ...string) BlDateTime { ... }
// Current date and time. Local system timezone if not provided.

func (Bl) YearsMonths(years, months int) BlYearsMonthsDuration { ... }
// Creates a years-and-months duration.
// Bl.YearsMonths(1, 6)   → P1Y6M

func (Bl) DaysTime(days, hours, minutes, seconds int) BlDaysTimeDuration { ... }
// Creates a days-and-time duration.
// Bl.DaysTime(5, 3, 30, 0)   → P5DT3H30M

func (Bl) Calendar(entries []BlCalendarEntry, opts ...CalendarOption) BlCalendar { ... }
// Creates a calendar (blkit-specific type, no FEEL counterpart). opts may
// include WithValidFrom(date) and WithValidTo(date) to bound the calendar's
// authoritative range.
// Bl.Calendar([]BlCalendarEntry{
//     Bl.CalendarEntry(Bl.Date(2025, 12, 25), "Christmas Day"),
// })

func (Bl) CalendarEntry(value BlExpr, name ...string) BlCalendarEntry { ... }
// Creates a single calendar entry.

func (Bl) Null() BlNull { ... }
// Creates a null value.

// ------------------------------------------------------------------ //
// Typed variable references                                           //
// ------------------------------------------------------------------ //
// Create a variable reference that carries type information. The returned
// object has all the methods of the declared type, enabling compile-time
// checking of type-specific operations. If the variable resolves to a
// different type at .Evaluate() time, a BlTypeError is raised.

func (Bl) NumberVar(name string) BlNumber { ... }
// Bl.NumberVar("price").Add(Bl.Number(1))

func (Bl) StringVar(name string) BlString { ... }
// Bl.StringVar("name").UpperCase()

func (Bl) BooleanVar(name string) BlBoolean { ... }

func (Bl) DateVar(name string) BlDate { ... }
// Bl.DateVar("start").IsWeekend()

func (Bl) TimeVar(name string) BlTime { ... }

func (Bl) DateTimeVar(name string) BlDateTime { ... }

func (Bl) YearsMonthsVar(name string) BlYearsMonthsDuration { ... }

func (Bl) DaysTimeVar(name string) BlDaysTimeDuration { ... }

func (Bl) ListVar(name string) BlList { ... }
// Bl.ListVar("items").Count()

func (Bl) ContextVar(name string) BlContext { ... }

func (Bl) TableVar(name string) BlTable { ... }
// Bl.TableVar("line_items").Column("unit_price").Sum()

func (Bl) RangeVar(name string) BlRange { ... }

func (Bl) CalendarVar(name string) BlCalendar { ... }
// Bl.CalendarVar("holidays").Contains(Bl.Date(2025, 12, 25))

// ------------------------------------------------------------------ //
// Structural expression builders                                      //
// ------------------------------------------------------------------ //

// Variable references are always typed — see "Typed variable references"
// above. Pick the typed factory that matches the variable's expected type.

func (Bl) List(items ...BlExpr) BlExpr { ... }
// Constructs a list literal from expression nodes.
// Bl.List(Bl.Number(1), Bl.Number(2), Bl.NumberVar("x"))

func (Bl) Context(entries map[string]BlExpr) BlExpr { ... }
// Constructs a context (record) literal.
// Bl.Context(map[string]BlExpr{"name": Bl.String("Alice"), "age": Bl.Number(30)})

func (Bl) Table(items ...BlExpr) BlTable { ... }
// Constructs a BlTable — an ordered list of uniformly-keyed BlContext rows.
// Two construction styles are supported:
//   - Pass row contexts directly; columns are inferred from the first row:
//       Bl.Table(Bl.Context(map[string]BlExpr{"name": Bl.String("Alice"), "age": Bl.Number(30)}),
//                Bl.Context(map[string]BlExpr{"name": Bl.String("Bob"),   "age": Bl.Number(34)}))
//   - Declare an explicit column ordering with Bl.Columns(...) followed by Bl.Row(...) entries:
//       Bl.Table(Bl.Columns("name", "age"),
//                Bl.Row(Bl.String("Alice"), Bl.Number(30)),
//                Bl.Row(Bl.String("Bob"),   Bl.Number(34)))
// See [table.spec.md](table.spec.md) for full semantics.

func (Bl) Columns(names ...string) BlExpr { ... }
// Declares a column ordering for use inside Bl.Table(...). Not a value on its own.

func (Bl) Row(values ...BlExpr) BlExpr { ... }
// Declares a positional row for use inside Bl.Table(...) when columns are declared
// via Bl.Columns(...). Values are matched to columns by index.

func (Bl) Range(start, end BlExpr, startIncluded, endIncluded bool) BlExpr { ... }
// Constructs a range.
// Bl.Range(Bl.Number(18), Bl.Number(65), true, true)   → [18..65]

func (Bl) If(condition, then_, else_ BlExpr) BlExpr { ... }
// Conditional expression.
// Bl.If(Bl.NumberVar("age").GreaterThanOrEqual(Bl.Number(18)),
//       Bl.String("adult"), Bl.String("minor"))

func (Bl) For(varName string, collection BlExpr) *BlForBuilder { ... }
// Begins a for-expression. Chain .Return() to complete.
// Bl.For("x", Bl.ListVar("items")).Return(Bl.NumberVar("x").Multiply(Bl.Number(2)))

func (Bl) Some(varName string, collection BlExpr) *BlQuantifierBuilder { ... }
// Begins a quantified expression (existential). Chain .Satisfies() to complete.

func (Bl) Every(varName string, collection BlExpr) *BlQuantifierBuilder { ... }
// Begins a quantified expression (universal). Chain .Satisfies() to complete.

// ------------------------------------------------------------------ //
// Conversion factories — string ↔ number ↔ temporal                  //
// ------------------------------------------------------------------ //

func (Bl) ToString(value BlExpr) BlString { ... }
// Deferred conversion of any value to its FEEL-style string representation.
// Bl.ToString(Bl.Number(42))                          // → "42"
// Bl.ToString(Bl.NumberVar("price"))                  // → resolved at evaluate time

func (Bl) ToNumber(value BlExpr, groupingSep, decimalSep BlExpr) BlNumber { ... }
// Parses a string with explicit grouping and decimal separators.
// Bl.ToNumber(Bl.String("1,234.56"), Bl.String(","), Bl.String("."))
// → BlNumber("1234.56")

func (Bl) ToDuration(value BlExpr) BlExpr { ... }
// Parses an ISO 8601 duration string. Returns BlYearsMonthsDuration if the
// string uses only Y/M designators, BlDaysTimeDuration otherwise. A string
// mixing both raises BlParseError at evaluation time.
// Bl.ToDuration(Bl.String("P1Y6M"))    // → BlYearsMonthsDuration
// Bl.ToDuration(Bl.String("P2DT3H"))   // → BlDaysTimeDuration

// Per-type parse factories: Bl.ToDate, Bl.ToTime, Bl.ToDateTime, Bl.ToDaysTime,
// Bl.ToYearsMonths — see each type's spec for the accepted string formats.
```

---

## Usage Examples

```go
// Typed variable — type-safe operations
expr := Bl.NumberVar("age").GreaterThanOrEqual(Bl.Number(18))
result := expr.Evaluate(map[string]BlExpr{"age": Bl.Number(21)})
// → BlBoolean.TRUE

// Universal operations on a typed variable (Equals, In, InstanceOf, etc.)
expr = Bl.StringVar("status").Equals(Bl.String("active"))
result = expr.Evaluate(map[string]BlExpr{"status": Bl.String("active")})
// → BlBoolean.TRUE

// Type-safe chaining — each method returns the concrete type
expr = Bl.Number(42).Add(Bl.Number(8)).Multiply(Bl.Number(2))
result = expr.Evaluate()
// → BlNumber(100)

// Conditional expression with typed variables
expr = Bl.If(
    Bl.NumberVar("score").GreaterThanOrEqual(Bl.Number(90)),
    Bl.String("pass"),
    Bl.String("fail"),
)
result = expr.Evaluate(map[string]BlExpr{"score": Bl.Number(95)})
// → BlString("pass")

// For-expression (list comprehension)
expr = Bl.For("x", Bl.ListVar("items")).Return(
    Bl.NumberVar("x").Multiply(Bl.Number(2)),
)
result = expr.Evaluate(map[string]BlExpr{"items": Bl.List(Bl.Number(1), Bl.Number(2), Bl.Number(3))})
// → BlList([BlNumber(2), BlNumber(4), BlNumber(6)])

// Business-day calculation with a calendar
holidays := Bl.Calendar(
    Bl.CalendarEntry(Bl.Date(2025, 4, 18), "Good Friday"),
    Bl.CalendarEntry(Bl.Date(2025, 4, 21), "Easter Monday"),
)
next_bday := Bl.Date(2025, 4, 17).NextBusinessDay(holidays).Evaluate()
// → BlDate(2025, 4, 22)   (skips Good Friday, weekend, Easter Monday)

// Compile-time safety — invalid operations caught before runtime
Bl.NumberVar("price").Add(Bl.Number(1))      // ✓ compiles — BlNumber has Add()
Bl.StringVar("name").UpperCase()              // ✓ compiles — BlString has UpperCase()
Bl.StringVar("name").Multiply(Bl.Number(2))  // ✗ compile error — BlString has no Multiply()
```

---

## Type System

blkit's type system is modelled on FEEL's but is not a conformance implementation of it — types, operations, and semantics are chosen for blkit's needs and may diverge from the FEEL spec. Each type has its own Interface Specification:

| FEEL Counterpart | Class | Spec |
|---|---|---|
| `number` | `BlNumber` | [number.spec.md](number.spec.md) |
| `string` | `BlString` | [string.spec.md](string.spec.md) |
| `boolean` | `BlBoolean` | [boolean.spec.md](boolean.spec.md) |
| `date` | `BlDate` | [date.spec.md](date.spec.md) |
| `time` | `BlTime` | [time.spec.md](time.spec.md) |
| `date and time` | `BlDateTime` | [datetime.spec.md](datetime.spec.md) |
| `duration` (years-months) | `BlYearsMonthsDuration` | [years_months_duration.spec.md](years_months_duration.spec.md) |
| `duration` (days-time) | `BlDaysTimeDuration` | [days_time_duration.spec.md](days_time_duration.spec.md) |
| `list` | `BlList` | [list.spec.md](list.spec.md) |
| `context` | `BlContext` | [context.spec.md](context.spec.md) |
| *(list of uniformly-keyed contexts; DMN "relation")* | `BlTable` | [table.spec.md](table.spec.md) |
| `null` | `BlNull` | [null.spec.md](null.spec.md) |
| range | `BlRange` | [range.spec.md](range.spec.md) |
| `Any` | `BlValue` (union/interface) | — |
| *(no FEEL counterpart — blkit-specific)* | `BlCalendar` | [calendar.spec.md](calendar.spec.md) |

`BlValue` is the union type / interface / sealed class that all blkit value types implement, used where any blkit value may appear.

The deferred expression base class shared by all types is documented in [bl-expr.spec.md](bl-expr.spec.md).

---

## Expression Types (FEEL Reference)

The left column illustrates how the same idea is written in standard FEEL (DMN 1.4) — included as a familiarity aid for readers who know FEEL. blkit does not parse FEEL strings; expressions are constructed programmatically using `Bl.*` methods.

### Expressions

```
age + 1                                            → Bl.var("age").add(Bl.number(1))
if age >= 18 then "adult" else "minor"             → Bl.if_(...)
[1, 2, 3]                                         → Bl.list_(...)
{ name: "Alice", age: 30 }                        → Bl.context_(...)
some x in [1,2,3] satisfies x > 2                 → Bl.some(...)
every x in scores satisfies x >= 60               → Bl.every(...)
for x in items return x * 2                        → Bl.for_(...)
```

### Decision Table Input Entries

In FEEL, unary tests are predicates with an implicit input value. In blkit, decision table input entries are standard boolean `BlExpr` expressions that explicitly reference InputClause labels as variables — no special unary test concept is needed.

```
> 100                    → Bl.number_var("Score").greater_than(Bl.number(100))
>= 18                   → Bl.number_var("Age").greater_than_or_equal(Bl.number(18))
< 25                    → Bl.number_var("Age").less_than(Bl.number(25))
[18..65]                 → Bl.number_var("Age").in_(Bl.range_(Bl.number(18), Bl.number(65)))
"low", "medium", "high"  → Bl.string_var("Risk").in_(Bl.list_(Bl.string("low"), Bl.string("medium"), Bl.string("high")))
-                        → None (wildcard — always matches)
```

---

## Built-in Function Coverage

blkit's value types expose chainable methods that cover the DMN 1.4 FEEL built-in function library — there is no separate stringly-typed dispatch (no `Bl.Call("function name", ...)`). Every common FEEL function maps to a method or factory:

| FEEL function | blkit equivalent |
|---|---|
| `string length(s)` | `s.Length()` |
| `substring(s, start, length?)` | `s.Substring(start, length)` |
| `upper case(s)` / `lower case(s)` / `trim(s)` | `s.UpperCase()` / `s.LowerCase()` / `s.Trim()` |
| `contains(s, m)` / `starts with(s, m)` / `ends with(s, m)` | `s.Contains(m)` / `s.StartsWith(m)` / `s.EndsWith(m)` |
| `matches(s, pat, flags?)` / `replace(s, pat, r, flags?)` | `s.Matches(pat, flags)` / `s.Replace(pat, r, flags)` |
| `split(s, delim)` | `s.Split(delim)` |
| `string join(list, sep?)` | `list.Join(sep)` (see [list.spec.md](list.spec.md)) |
| `floor(n, scale?)` / `ceiling(n, scale?)` | `n.Floor(scale)` / `n.Ceiling(scale)` |
| `round up/down/half up/half down(n, scale)` | `n.RoundUp(scale)` / `RoundDown` / `RoundHalfUp` / `RoundHalfDown` |
| `decimal(n, scale)` | `n.Round(scale)` (round-half-even) |
| `abs(n)` / `modulo(a, b)` / `sqrt(n)` / `log(n)` / `exp(n)` | `n.Abs()` / `n.Modulo(b)` / `n.Sqrt()` / `n.Log()` / `n.Exp()` |
| `odd(n)` / `even(n)` | `n.IsOdd()` / `n.IsEven()` |
| `list contains(l, e)` / `count(l)` | `l.Contains(e)` / `l.Count()` |
| `min/max/sum/mean/median/stddev/mode/product(l)` | `l.Min()` / `Max()` / `Sum()` / `Mean()` / `Median()` / `Stddev()` / `Mode()` / `Product()` |
| `all(l)` / `any(l)` | `l.All()` / `l.Any()` |
| `sublist(l, start, length?)` / `append(l, ...)` / `concatenate(l, ...)` | `l.Sublist(start, length)` / `l.Append(...)` / `l.Concatenate(...)` |
| `insert before(l, pos, item)` / `remove(l, pos)` / `reverse(l)` | `l.InsertBefore(pos, item)` / `l.Remove(pos)` / `l.Reverse()` |
| `index of(l, m)` / `union(l, ...)` / `distinct values(l)` / `duplicate values(l)` / `flatten(l)` / `sort(l, p)` | `l.IndexOf(m)` / `l.Union(...)` / `l.DistinctValues()` / `l.DuplicateValues()` / `l.Flatten()` / `l.Sort(p)` |
| `now()` / `today()` | `Bl.Now()` / `Bl.Today()` |
| `day of week(d)` / `day of year(d)` / `month of year(d)` / `week of year(d)` | `d.DayOfWeek()` / `d.DayOfYear()` / `d.MonthOfYear()` / `d.WeekOfYear()` |
| `date(year, month, day)` / `time(hour, minute, second, offset?)` | `Bl.Date(y, m, d)` / `Bl.Time(h, m, s, opts...)` |
| `date(from)` / `time(from)` / `date and time(from)` | `Bl.ToDate(s)` / `Bl.ToTime(s)` / `Bl.ToDateTime(s)` |
| `date and time(date, time)` | `date.AtTime(time)` (see [date.spec.md](date.spec.md)) |
| `duration(from)` | `Bl.ToDuration(s)` |
| `years and months duration(from, to)` | `from.DiffYearsMonths(to)` |
| `get value(ctx, k)` / `get entries(ctx)` / `put(ctx, k, v)` / `put all(ctx, ...)` / `context merge(ctx, ...)` | `ctx.Get(k)` / `ctx.GetEntries()` / `ctx.Put(k, v)` / `ctx.PutAll(...)` / `ctx.Merge(...)` |
| `context(entries)` | `Bl.Context(entries)` |
| `string(from)` | `Bl.ToString(value)` |
| `number(from, groupingSep, decimalSep)` | `Bl.ToNumber(s, groupingSep, decimalSep)` |
| `not(b)` | `b.Not()` (universal on `BlExpr`) |

The Trisotech `date add` / `time add` / `datetime add` / `date diff` extensions are likewise covered by chainable methods (`d.Add(duration)`, `d.DiffDaysTime(other)`, etc.) — no separate dispatch needed.

---

## Error Handling

- A type mismatch during evaluation (e.g. adding a string to a number) produces a `BlTypeError`.
- Accessing a missing context key returns `null` (not an error).
- Division by zero returns `null` (matching FEEL semantics).
- `Bl.list_()` with no arguments evaluates to an empty `BlList`.
- Identifiers and keywords are case-sensitive (matching FEEL); `TRUE` is not the same as `true`.
- The FEEL-style notation produced by `to_markdown()` uses double-quoted strings only.

---

## Edge Cases

- `BlNumber` values are arbitrary-precision decimals (matching FEEL); integer overflow is not possible, but the implementation must preserve precision through the host language's decimal type.
- `BlNull` propagates through most operations (e.g. `null + 1` → `null`), matching FEEL semantics.
- A `None` input entry in a decision table always matches (wildcard).

---

## Out of Scope

### String-Based Expression Evaluation

blkit does **not** include a string-based expression evaluator — there is no facility to accept a raw FEEL (or FEEL-like) expression string (e.g. `"age + 1"`) and evaluate it at runtime. This omission is intentional.

All blkit expressions are constructed programmatically using `Bl.*` factory methods and structural expression builders, then evaluated via `.evaluate()`. This design choice provides compile-time type safety, IDE discoverability, and avoids the cost and ambiguity of a FEEL parser.
