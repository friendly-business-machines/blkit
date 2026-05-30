---
name: BlCalendar
description: The calendar type in the blkit expression language — a named, ordered collection of temporal entries (dates, datetimes, or temporal ranges) for holidays/schedules/blackout periods. Covers construction via built-ins, query/mutation built-ins, and the Go layer (BlCalendar + expr registrations).
targets:
  - ../../expr/calendar.go
---

# BlCalendar — the `calendar` type

`calendar` is a blkit-specific type: an ordered, immutable collection of entries, each a temporal
value (`date`, `datetime`, or a `range` of either) with an optional name. It models holiday sets,
maintenance windows, blackout periods, and similar schedules. The Go value type backing it is
`BlCalendar`.

A literal would be the syntactic form for writing a constant value of a type directly inside an
expression (as `[1, 2, 3]` is for a list). **`calendar` has no such form.** A calendar is produced
by the `calendar(...)` built-in (or supplied host-side as an input variable) and then queried —
for example, the `calendar([…])` in `isBusinessDay(date("2025-12-25"), calendar([…]))`. All
calendar built-ins are blkit extensions (**ext** — no DMN equivalent). It pairs with
[date.spec.md](date.spec.md)'s business-day arithmetic, which consumes a calendar. See
[range.spec.md](range.spec.md) for entry ranges.

---

## Construction

```
// An entry: calendarEntry(value[, name]). value is a date, datetime, or range of either.
calendarEntry(date("2025-12-25"), "Christmas Day")
calendarEntry(date("2025-04-18"))                                  // unnamed
calendarEntry([date("2025-12-24")..date("2026-01-02")], "Closure") // range entry

// A calendar from a list of entries; optional validFrom/validTo bounds:
calendar([
  calendarEntry(date("2025-01-01"), "New Year's Day"),
  calendarEntry(date("2025-12-25"), "Christmas Day")
])
calendar(entries, date("2025-01-01"), date("2025-12-31"))   // with validity bounds
calendar([])                                                // empty calendar
```

`validFrom`/`validTo` bound the period over which the calendar is authoritative (used by
business-day arithmetic); `validFrom > validTo` → `BlTypeError`.

`[@test] ../../expr/calendar_test.go`

---

## Query & inspection built-ins

| Function | Example | Result |
|---|---|---|
| `count(c)` | `count(ukHolidays)` | entry count |
| `isEmpty(c)` | `isEmpty(calendar([]))` | `true` |
| `entries(c)` | `entries(c)` | list of entries (insertion order) |
| `names(c)` | `names(c)` | distinct names (first-appearance order; unnamed excluded) |
| `find(c, name)` | `find(ukHolidays, "Christmas Day")` | list of entries with that name (case-sensitive) |
| `contains(c, point)` | `contains(ukHolidays, date("2025-12-25"))` | `true` (point covered by any entry) |
| `entriesFor(c, point)` | `entriesFor(c, date("2025-05-05"))` | entries covering the point |
| `overlaps(c, range)` | `overlaps(windows, [t1..t2])` | `true` if any entry overlaps the range |
| `entriesIn(c, range)` | `entriesIn(ukHolidays, [date("2025-01-01")..date("2025-03-31")])` | entries overlapping the range |
| `validFrom(c)` / `validTo(c)` | `validFrom(uk2025)` | the bound, or `null` |
| `validRange(c)` | `validRange(uk2025)` | `[validFrom..validTo]`, or `null` if either unset |
| `entryValue(e)` / `entryName(e)` | `entryName(find(c,"X")[1])` | an entry's value / name |

`contains` and `overlaps` are overloaded by argument type (the string/range forms live in
[string.spec.md](string.spec.md) / [range.spec.md](range.spec.md)). A `date` point against a
`datetime`-ranged entry (or vice versa) → `null` for that entry (no implicit coercion).

`[@test] ../../expr/calendar_query_test.go`

---

## Mutation built-ins (return a new calendar)

| Function | Example | Result |
|---|---|---|
| `calendarAdd(c, entries…)` | `calendarAdd(base, calendarEntry(date("2025-12-26"), "Boxing Day"))` | calendar with entries appended |
| `calendarRemove(c, name)` | `calendarRemove(c, "Boxing Day")` | entries with that name removed (no-op if absent) |
| `calendarMerge(calendars)` | `calendarMerge([england, scotland])` | all entries concatenated (no dedup) |

`[@test] ../../expr/calendar_mutation_test.go`

---

## Operators

| Operator | Meaning | Example |
|---|---|---|
| `=` `!=` | equality (same entries, same order) | `c1 = c2` |

`[@test] ../../expr/calendar_operators_test.go`

---

## Use with date arithmetic

Calendars are the holiday source for the business-day built-ins in [date.spec.md](date.spec.md):

```
addBusinessDays(date("2025-04-17"), 2, ukHolidays)   // skips weekends + calendar holidays
isPublicHoliday(date("2025-04-18"), ukHolidays)      // → true
businessDaysBetween(date("2025-04-14"), date("2025-04-25"), ukHolidays)  // → 8
```

Iteration outside `[validFrom, validTo]` is silently tolerated by default — the calendar simply
contributes no holiday information beyond its bounds. Callers that need a hard guarantee can
opt in by passing `strictCalendarRange: true` to any iterating business-day function, which
raises `BlCalendarRangeError` the moment iteration would step past the boundary (see
[date.spec.md § Calendar-range strictness](date.spec.md#calendar-range-strictness)).

---

## Migration mapping (legacy method-chained → string)

| Legacy | New form |
|---|---|
| `Bl.Calendar(entries, opts)` | `calendar(entries[, validFrom, validTo])` **ext** |
| `Bl.CalendarEntry(value, name?)` | `calendarEntry(value[, name])` **ext** |
| `validFrom` / `validTo` / `validRange` | `validFrom(c)` / `validTo(c)` / `validRange(c)` **ext** |
| `size` / `isEmpty` | `count(c)` / `isEmpty(c)` |
| `entries` / `names` / `find` | `entries(c)` / `names(c)` / `find(c, name)` |
| `contains` / `entriesFor` | `contains(c, point)` / `entriesFor(c, point)` |
| `overlaps` / `entriesIn` | `overlaps(c, range)` / `entriesIn(c, range)` |
| `add` / `remove` / `merge` | `calendarAdd(c, …)` / `calendarRemove(c, name)` / `calendarMerge([…])` |
| `equals` | `=` / `!=` |
| entry `Name` / `Value` fields | `entryName(e)` / `entryValue(e)` |
| `toList` / `String` | Go host accessors (below) |

---

## Go implementation (expr extension)

Lives in `expr/calendar.go`. Shared mechanics in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).

### Value types & host API (exported)

```go
// A temporal value (BlDate/BlDateTime/BlRange) with an optional name.
type BlCalendarEntry struct{ name *string; value BlValue }
func (BlCalendarEntry) Type() BlType { return BlTypeAny } // not a first-class language type
func (e BlCalendarEntry) ToMarkdown() string
func (BlCalendarEntry) isBlValue() {}

type BlCalendar struct{ entries []BlCalendarEntry; validFrom, validTo BlValue }
func (BlCalendar) Type() BlType { return BlTypeCalendar }
func (c BlCalendar) Equal(other BlValue) BlValue // same entries, same order
func (c BlCalendar) ToMarkdown() string
func (BlCalendar) isBlValue() {}

// Host constructors / accessors (calendars are usually built host-side and passed in).
func CalendarEntry(value BlValue, name ...string) (BlCalendarEntry, error)
func Calendar(entries []BlCalendarEntry, opts ...CalendarOption) (BlCalendar, error) // WithValidFrom/To
func (c BlCalendar) ToList() []BlCalendarEntry
func (c BlCalendar) String() string
```

### Registrations (`calendarOptions`, unexported — all ext)

```go
func calendarOptions() []expr.Option {
    return []expr.Option{
        expr.Function("calendar", calendarBuildFn,
            new(func(BlList) BlCalendar), new(func(BlList, BlValue, BlValue) BlCalendar)),
        expr.Function("calendarEntry", calendarEntryFn,
            new(func(BlValue) BlCalendarEntry), new(func(BlValue, BlString) BlCalendarEntry)),

        expr.Function("entries",    typed1(entriesFn),    new(func(BlCalendar) BlList)),
        expr.Function("names",      typed1(namesFn),      new(func(BlCalendar) BlList)),
        expr.Function("find",       typed2(findFn),       new(func(BlCalendar, BlString) BlList)),
        expr.Function("contains",   typed2(calContainsFn),new(func(BlCalendar, BlValue) BlValue)),  // overloads string contains
        expr.Function("entriesFor", typed2(entriesForFn), new(func(BlCalendar, BlValue) BlList)),
        expr.Function("overlaps",   typed2(calOverlapsFn),new(func(BlCalendar, BlRange) BlBoolean)), // overloads range overlaps
        expr.Function("entriesIn",  typed2(entriesInFn),  new(func(BlCalendar, BlRange) BlList)),
        expr.Function("validFrom",  typed1(validFromFn),  new(func(BlCalendar) BlValue)),
        expr.Function("validTo",    typed1(validToFn),    new(func(BlCalendar) BlValue)),
        expr.Function("validRange", typed1(validRangeFn), new(func(BlCalendar) BlValue)),
        expr.Function("entryName",  typed1(entryNameFn),  new(func(BlCalendarEntry) BlValue)),
        expr.Function("entryValue", typed1(entryValueFn), new(func(BlCalendarEntry) BlValue)),
        expr.Function("calendarAdd",    variadic(calendarAddFn), new(func(BlCalendar, ...BlCalendarEntry) BlCalendar)),
        expr.Function("calendarRemove", typed2(calendarRemoveFn),new(func(BlCalendar, BlString) BlCalendar)),
        expr.Function("calendarMerge",  typed1(calendarMergeFn), new(func(BlList) BlCalendar)),

        // count/isEmpty get a BlCalendar overload (canonical entries in list.go):
        expr.Function("count",   typed1(calCountFn),   new(func(BlCalendar) BlNumber)),
        expr.Function("isEmpty", typed1(calIsEmptyFn), new(func(BlCalendar) BlBoolean)),
    }
}
```

`isPublicHoliday`/`isBusinessDay`/`addBusinessDays`/… ([date.spec.md](date.spec.md)) take a
`BlCalendar` (named `phCalendar`). Iterating variants raise `BlCalendarRangeError` past the
validity bounds only when `strictCalendarRange: true` is supplied.
**Operators.** `=`/`!=`. The engine bridge accepts a host-built `BlCalendar` directly as an input
variable.

`[@test] ../../expr/calendar_test.go`

---

## Edge cases

- `calendar([])` → empty, no validity bounds; `validFrom > validTo` → `BlTypeError`.
- `validRange(c)` → `null` unless both bounds are set; validity does not filter query built-ins.
- `find` is case-sensitive; `calendarRemove` ignores unnamed entries and is a no-op for absent names.
- `calendarMerge` does not union validity bounds (result has none unless re-supplied) and performs no
  dedup.
- `contains`/`entriesFor`/`overlaps`/`entriesIn` with mismatched temporal types → `null` for the
  mismatched entries.
- A `calendarEntry` whose range has non-temporal endpoints → `BlTypeError`.
- equality requires the same entries in the same order.
