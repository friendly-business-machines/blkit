---
name: BlCalendar
description: A named, ordered collection of temporal entries — individual dates, datetimes, or date/datetime ranges — used to model schedules, holiday sets, blackout periods, and similar domain calendars
targets:
  - ../../expr/calendar.go
---

# BlCalendar

`BlCalendar` is a blkit-specific type with no FEEL counterpart. It is an ordered, immutable collection of `BlCalendarEntry` items, each holding a temporal value — a `BlDate`, a `BlDateTime`, or a `BlRange` whose endpoints are dates or datetimes.

Its primary purpose is to model named sets of significant moments or periods: public holidays for a region, maintenance windows, financial settlement dates, blackout periods, office closure schedules, and so on. Other functions receive a `BlCalendar` as an ordinary input and query it to make decisions.

`BlCalendar` extends `BlExpr`. Every instance is a literal leaf node. All methods return deferred `BlExpr` nodes; call `.evaluate()` to materialise the result.

```go
type BlCalendarEntry struct {
    Name  *string               // Optional human-readable label, e.g. "Christmas Day", "Q1 Maintenance Window".
                                // Multiple entries may share the same name (e.g. a recurring event).
    Value interface{}           // BlDate | BlDateTime | BlRange
                                // The temporal value. When Value is a BlRange, its endpoints must be
                                // BlDate or BlDateTime; a range of numbers or strings is invalid.
}

type BlCalendar struct { BlExpr }

// Construction — see bl.spec.md.
//   Bl.CalendarEntry(value BlExpr, name ...string) BlCalendarEntry
//     value must evaluate to BlDate, BlDateTime, or a BlRange of either.
//   Bl.Calendar(entries []BlCalendarEntry, opts ...CalendarOption) BlCalendar
//     Empty `entries` slice yields an empty calendar.
//     opts may include WithValidFrom(d) and WithValidTo(d) to bound the
//     range over which the calendar is authoritative. Raises BlTypeError
//     if validFrom is after validTo.

// Validity range — eager structural metadata (valid on a concrete BlCalendar)
// ValidFrom BlDate | BlDateTime | nil
// ValidTo   BlDate | BlDateTime | nil

func (c *BlCalendar) ValidRange() BlExpr { ... }
// Evaluates to BlRange([validFrom..validTo]) if both validFrom and validTo are set,
// or BlNull if either is absent.

// Size — deferred
func (c *BlCalendar) Size() BlNumber { ... }          // total number of entries

func (c *BlCalendar) IsEmpty() BlExpr { ... }      // evaluates to BlBoolean

// Entry access — deferred
func (c *BlCalendar) Entries() BlList { ... }
// Evaluates to a BlList of all BlCalendarEntry values, in insertion order.

func (c *BlCalendar) Names() BlList { ... }
// Evaluates to a BlList of distinct BlString names across all named entries,
// in the order of their first appearance. Unnamed entries are not included.

func (c *BlCalendar) Find(name string) BlList { ... }
// Evaluates to a BlList of all BlCalendarEntry values whose name equals `name`
// (case-sensitive). Evaluates to an empty BlList if no match.

// Point containment — deferred; evaluate to BlBoolean
func (c *BlCalendar) Contains(point BlExpr) BlExpr { ... }
// True if any calendar entry covers `point`:
//   - For a date/datetime entry: point equals the entry's value (exact match).
//   - For a range entry: point falls within the range (respecting boundary inclusions).
// `point` must evaluate to BlDate or BlDateTime.

func (c *BlCalendar) EntriesFor(point BlExpr) BlList { ... }
// Evaluates to a BlList of all BlCalendarEntry values that cover `point`.
// Returns an empty list if no entries match.

// Range overlap — deferred; evaluate to BlBoolean / BlList
func (c *BlCalendar) Overlaps(range_ BlExpr) BlExpr { ... }
// True if any calendar entry overlaps with `range_`.
// A point entry overlaps if it falls within range_; a range entry overlaps if the
// two intervals share at least one point (respecting boundary inclusions).

func (c *BlCalendar) EntriesIn(range_ BlExpr) BlList { ... }
// Evaluates to a BlList of all entries that overlap with `range_`.

// Immutable mutation — deferred; evaluate to BlCalendar
func (c *BlCalendar) Add(entries ...BlCalendarEntry) BlCalendar { ... }
// Returns a new BlCalendar with the given entries appended at the end.

func (c *BlCalendar) Remove(name string) BlCalendar { ... }
// Returns a new BlCalendar with all entries whose name equals `name` removed.
// If no entries have that name, returns an equivalent calendar unchanged.

func (c *BlCalendar) Merge(others ...BlExpr) BlCalendar { ... }
// Returns a new BlCalendar containing all entries from self followed by all entries
// from each of the given calendars, in argument order.

// Equality — deferred; evaluates to BlBoolean
func (c *BlCalendar) Equals(other BlExpr) BlExpr { ... }
// Two calendars are equal if they contain the same entries in the same order.

// Eager host-language utilities (call only on a concrete value after .Evaluate())
func (c *BlCalendar) ToList() []BlCalendarEntry { ... }
func (c *BlCalendar) String() string { ... }
```

---

## BlCalendarEntry

Each entry pairs an optional name with a temporal value.

### `Bl.CalendarEntry(value, name?)`

Creates a single calendar entry.

- `value` must be a `BlDate`, `BlDateTime`, or a `BlRange` whose start and end evaluate to `BlDate` or `BlDateTime`. Providing a range with non-temporal endpoints raises `BlTypeError` at evaluation time.
- `name` is an optional string label. Multiple entries may share the same name.

```go
// A named date entry (e.g. a public holiday)
Bl.CalendarEntry(Bl.Date(2025, 12, 25), strPtr("Christmas Day"))

// An unnamed date entry
Bl.CalendarEntry(Bl.Date(2025, 4, 18), nil)

// A named datetime entry (e.g. a release timestamp)
Bl.CalendarEntry(Bl.DateTime(2025, 6, 1, 9, 0, 0), strPtr("Q2 Go-Live"))

// A named date range (e.g. a holiday period)
Bl.CalendarEntry(
    Bl.Range(Bl.Date(2025, 12, 24), Bl.Date(2025, 12, 31), true, true),
    strPtr("Christmas Period"),
)

// A named datetime range (e.g. a maintenance window)
Bl.CalendarEntry(
    Bl.Range(
        Bl.DateTime(2025, 3, 28, 2, 0, 0),
        Bl.DateTime(2025, 3, 28, 4, 0, 0),
        true, true,
    ),
    strPtr("Q1 Maintenance Window"),
)
```

---

## Construction

### `Bl.Calendar(*entries)`

Creates a calendar from any number of `BlCalendarEntry` values. Entries are stored in the order provided. Duplicate names are permitted.

```go
ukHolidays2025 := Bl.Calendar([]BlCalendarEntry{
    Bl.CalendarEntry(Bl.Date(2025, 1, 1),   strPtr("New Year's Day")),
    Bl.CalendarEntry(Bl.Date(2025, 4, 18),  strPtr("Good Friday")),
    Bl.CalendarEntry(Bl.Date(2025, 4, 21),  strPtr("Easter Monday")),
    Bl.CalendarEntry(Bl.Date(2025, 5, 5),   strPtr("Early May Bank Holiday")),
    Bl.CalendarEntry(Bl.Date(2025, 5, 26),  strPtr("Spring Bank Holiday")),
    Bl.CalendarEntry(Bl.Date(2025, 8, 25),  strPtr("Summer Bank Holiday")),
    Bl.CalendarEntry(Bl.Date(2025, 12, 25), strPtr("Christmas Day")),
    Bl.CalendarEntry(Bl.Date(2025, 12, 26), strPtr("Boxing Day")),
})

// A calendar with no entries
Bl.Calendar(nil).Evaluate()
// → BlCalendar([])
```

### Empty calendar

`Bl.Calendar(nil)` returns an empty calendar — useful as a starting point for building one with `Add()`.

---

## Validity Range

`valid_from` and `valid_to` define the temporal scope over which the calendar is authoritative — typically the period for which its holiday or closure data has been curated.

```go
// A UK holiday calendar covering the year 2025 only
uk2025 := Bl.Calendar([]BlCalendarEntry{
    Bl.CalendarEntry(Bl.Date(2025, 1, 1),   strPtr("New Year's Day")),
    Bl.CalendarEntry(Bl.Date(2025, 12, 25), strPtr("Christmas Day")),
    Bl.CalendarEntry(Bl.Date(2025, 12, 26), strPtr("Boxing Day")),
}, WithValidFrom(Bl.Date(2025, 1, 1)), WithValidTo(Bl.Date(2025, 12, 31)))

uk2025.ValidFrom
// → Bl.Date(2025, 1, 1)   (eager — no .Evaluate() needed)

uk2025.ValidTo
// → Bl.Date(2025, 12, 31)

uk2025.ValidRange().Evaluate()
// → BlRange([Bl.Date(2025, 1, 1)..Bl.Date(2025, 12, 31)])

// Unbounded on one side
multiYear := Bl.Calendar([]BlCalendarEntry{
    Bl.CalendarEntry(Bl.Date(2025, 1, 1), strPtr("New Year's Day")),
}, WithValidFrom(Bl.Date(2025, 1, 1)))  // valid from 2025 onwards, no upper bound
multiYear.ValidTo
// → nil

multiYear.ValidRange().Evaluate()
// → BlNull   (only evaluates to BlRange when both bounds are set)
```

### Interaction with Business-Day Arithmetic

`BlDate.add_business_days()` and `BlDate.subtract_business_days()` iterate day-by-day through the calendar. If the iteration reaches a date outside the calendar's `[valid_from, valid_to]` range, a `BlCalendarRangeError` is raised (or suppressed when `ignore_calendar_range_errors=True`).

When `ignore_calendar_range_errors=True`, dates falling outside the validity range are treated as ordinary days (i.e. not holidays). The iteration still completes; it simply stops consulting the calendar for those dates.

```go
// A calendar limited to 2025 — safe usage stays within bounds
invoiceDue := Bl.Date(2025, 12, 29).AddBusinessDays(
    Bl.Number(3),
    uk2025,
    false,  // raises if arithmetic goes past 2025-12-31
)

// Suppress the error to allow arithmetic that spills into the next year
invoiceDue = Bl.Date(2025, 12, 29).AddBusinessDays(
    Bl.Number(3),
    uk2025,
    true,  // continues past 2025-12-31 as if no calendar
)
```

```go
Bl.Calendar(nil).IsEmpty().Evaluate()
// → BlBoolean.TRUE

Bl.Calendar(nil).Add(
    Bl.CalendarEntry(Bl.Date(2025, 1, 1), strPtr("New Year's Day")),
).Size().Evaluate()
// → BlNumber("1")
```

---

## Size and Classification

### `size`

Evaluates to the total number of entries in the calendar.

```go
ukHolidays2025.Size().Evaluate()
// → BlNumber("8")

Bl.Calendar(nil).Size().Evaluate()
// → BlNumber("0")

// Guard: only proceed if the calendar has been populated
Bl.CalendarVar("schedule").IsEmpty().Evaluate(map[string]BlExpr{"schedule": Bl.Calendar(nil)})
// → BlBoolean.TRUE
```

---

## Entry Access

### `entries`

Evaluates to a `BlList` of all `BlCalendarEntry` values in insertion order.

```go
smallCal := Bl.Calendar([]BlCalendarEntry{
    Bl.CalendarEntry(Bl.Date(2025, 1, 1), strPtr("New Year's Day")),
    Bl.CalendarEntry(Bl.Date(2025, 12, 25), strPtr("Christmas Day")),
})

smallCal.Entries().Evaluate()
// → BlList([
//     Bl.CalendarEntry("New Year's Day", Bl.Date(2025, 1, 1)),
//     Bl.CalendarEntry("Christmas Day", Bl.Date(2025, 12, 25)),
// ])

// Count entries whose value is a range (using BlList operations)
smallCal.Entries().Count().Evaluate()
// → BlNumber("2")
```

### `names()`

Evaluates to a `BlList` of distinct `BlString` names, in the order of their first appearance. Unnamed entries (where `name` is `None`) are not included.

```go
cal := Bl.Calendar([]BlCalendarEntry{
    Bl.CalendarEntry(Bl.Date(2025, 1, 1),   strPtr("New Year's Day")),
    Bl.CalendarEntry(Bl.Date(2025, 4, 18),  strPtr("Good Friday")),
    Bl.CalendarEntry(Bl.Date(2025, 12, 25), strPtr("Christmas Day")),
    Bl.CalendarEntry(Bl.Date(2025, 12, 26), strPtr("Boxing Day")),
    Bl.CalendarEntry(Bl.Date(2026, 1, 1),   nil),  // unnamed
})

cal.Names().Evaluate()
// → BlList([
//     BlString("New Year's Day"),
//     BlString("Good Friday"),
//     BlString("Christmas Day"),
//     BlString("Boxing Day"),
// ])   (unnamed entry excluded)

// Check whether a named event is in the calendar at all
cal.Names().Contains(Bl.String("Good Friday")).Evaluate()
// → BlBoolean.TRUE
```

### `find(name)`

Evaluates to a `BlList` of all entries whose `name` equals `name` (case-sensitive). Returns an empty list when no entries match. Because multiple entries may share a name (e.g. a recurring event), this always returns a list even when only one match is expected.

```go
ukHolidays2025.Find("Christmas Day").Evaluate()
// → BlList([Bl.CalendarEntry("Christmas Day", Bl.Date(2025, 12, 25))])

ukHolidays2025.Find("Nonexistent").Evaluate()
// → BlList([])

// A calendar with a recurring named event (e.g. weekly team meeting)
meetings := Bl.Calendar([]BlCalendarEntry{
    Bl.CalendarEntry(Bl.Date(2025, 3, 3),  strPtr("Team Meeting")),
    Bl.CalendarEntry(Bl.Date(2025, 3, 10), strPtr("Team Meeting")),
    Bl.CalendarEntry(Bl.Date(2025, 3, 17), strPtr("Team Meeting")),
})

meetings.Find("Team Meeting").Evaluate()
// → BlList([
//     Bl.CalendarEntry("Team Meeting", Bl.Date(2025, 3, 3)),
//     Bl.CalendarEntry("Team Meeting", Bl.Date(2025, 3, 10)),
//     Bl.CalendarEntry("Team Meeting", Bl.Date(2025, 3, 17)),
// ])
```

---

## Point Containment

### `contains(point)`

Evaluates to `BlBoolean.TRUE` if any entry in the calendar covers `point`:

- For a **date or datetime entry**: `point` equals the entry's value exactly.
- For a **range entry**: `point` falls within the range, respecting boundary inclusions.

`point` must evaluate to `BlDate` or `BlDateTime`. Querying a `BlDate` against a `BlRange` whose endpoints are `BlDateTime` (or vice versa) evaluates to `BlNull` — mixed temporal types are not implicitly coerced.

```go
ukHolidays2025.Contains(Bl.Date(2025, 12, 25)).Evaluate()
// → BlBoolean.TRUE   (Christmas Day)

ukHolidays2025.Contains(Bl.Date(2025, 12, 24)).Evaluate()
// → BlBoolean.FALSE  (Christmas Eve is not in the list)

ukHolidays2025.Contains(Bl.Date(2025, 6, 15)).Evaluate()
// → BlBoolean.FALSE

// Range-based calendar: office closed from 24 Dec to 2 Jan
closure := Bl.Calendar([]BlCalendarEntry{
    Bl.CalendarEntry(
        Bl.Range(Bl.Date(2025, 12, 24), Bl.Date(2026, 1, 2), true, true),
        strPtr("Christmas Closure"),
    ),
})

closure.Contains(Bl.Date(2025, 12, 27)).Evaluate()
// → BlBoolean.TRUE   (within the range)

closure.Contains(Bl.Date(2025, 12, 24)).Evaluate()
// → BlBoolean.TRUE   (closed range — boundary included)

closure.Contains(Bl.Date(2025, 12, 23)).Evaluate()
// → BlBoolean.FALSE  (day before closure starts)

// Check whether a proposed meeting date is a holiday
Bl.CalendarVar("holidays").Contains(Bl.DateVar("proposed_date")).Evaluate(map[string]BlExpr{
    "holidays":      ukHolidays2025,
    "proposed_date": Bl.Date(2025, 4, 18),
})
// → BlBoolean.TRUE   (Good Friday)
```

### `entries_for(point)`

Evaluates to a `BlList` of all entries that cover `point`. Useful when you need the name or type of the matching entry, not just whether a match exists. Returns an empty list if no entry covers the point.

```go
ukHolidays2025.EntriesFor(Bl.Date(2025, 12, 25)).Evaluate()
// → BlList([Bl.CalendarEntry("Christmas Day", Bl.Date(2025, 12, 25))])

ukHolidays2025.EntriesFor(Bl.Date(2025, 6, 15)).Evaluate()
// → BlList([])

// When two entries cover the same date
overlapping := Bl.Calendar([]BlCalendarEntry{
    Bl.CalendarEntry(Bl.Date(2025, 5, 5), strPtr("Early May Bank Holiday")),
    Bl.CalendarEntry(
        Bl.Range(Bl.Date(2025, 5, 3), Bl.Date(2025, 5, 11), true, true),
        strPtr("Spring Half-Term"),
    ),
})

overlapping.EntriesFor(Bl.Date(2025, 5, 5)).Evaluate()
// → BlList([
//     Bl.CalendarEntry("Early May Bank Holiday", Bl.Date(2025, 5, 5)),
//     Bl.CalendarEntry("Spring Half-Term", Bl.Range(...)),
// ])
```

---

## Range Overlap

### `overlaps(range_)`

Evaluates to `BlBoolean.TRUE` if any entry in the calendar overlaps with `range_`. Overlap means the entry and the range share at least one point:

- A **point entry** (date or datetime) overlaps if it falls within `range_`.
- A **range entry** overlaps with `range_` if the two intervals share any point (they are not entirely before or after each other).

```go
maintenanceWindows := Bl.Calendar([]BlCalendarEntry{
    Bl.CalendarEntry(
        Bl.Range(
            Bl.DateTime(2025, 3, 28, 2, 0, 0),
            Bl.DateTime(2025, 3, 28, 4, 0, 0),
            true, true,
        ),
        strPtr("Q1 Maintenance"),
    ),
    Bl.CalendarEntry(
        Bl.Range(
            Bl.DateTime(2025, 6, 27, 2, 0, 0),
            Bl.DateTime(2025, 6, 27, 4, 0, 0),
            true, true,
        ),
        strPtr("Q2 Maintenance"),
    ),
})

// Does a proposed deployment window overlap any maintenance window?
proposed := Bl.Range(
    Bl.DateTime(2025, 3, 28, 3, 0, 0),
    Bl.DateTime(2025, 3, 28, 5, 0, 0),
    true, true,
)

maintenanceWindows.Overlaps(proposed).Evaluate()
// → BlBoolean.TRUE   (proposed window overlaps Q1 Maintenance)

safeWindow := Bl.Range(
    Bl.DateTime(2025, 4, 1, 10, 0, 0),
    Bl.DateTime(2025, 4, 1, 12, 0, 0),
    true, true,
)

maintenanceWindows.Overlaps(safeWindow).Evaluate()
// → BlBoolean.FALSE
```

### `entries_in(range_)`

Evaluates to a `BlList` of all entries that overlap with `range_`. Returns an empty list if no entries overlap.

```go
maintenanceWindows.EntriesIn(proposed).Evaluate()
// → BlList([Bl.CalendarEntry("Q1 Maintenance", Bl.Range(...))])

// Find all holidays within a specific quarter
q1 := Bl.Range(Bl.Date(2025, 1, 1), Bl.Date(2025, 3, 31), true, true)
ukHolidays2025.EntriesIn(q1).Evaluate()
// → BlList([
//     Bl.CalendarEntry("New Year's Day", Bl.Date(2025, 1, 1)),
// ])
```

---

## Immutable Mutation

### `add(*entries)`

Returns a new `BlCalendar` with the given entries appended at the end. The original calendar is unchanged.

```go
base := Bl.Calendar([]BlCalendarEntry{
    Bl.CalendarEntry(Bl.Date(2025, 1, 1), strPtr("New Year's Day")),
})

extended := base.Add(
    Bl.CalendarEntry(Bl.Date(2025, 12, 25), strPtr("Christmas Day")),
    Bl.CalendarEntry(Bl.Date(2025, 12, 26), strPtr("Boxing Day")),
).Evaluate()

extended.Size().Evaluate()   // → BlNumber("3")
base.Size().Evaluate()       // → BlNumber("1")   (original unchanged)

// Build a calendar incrementally
Bl.Calendar(nil).
    Add(Bl.CalendarEntry(Bl.Date(2025, 1, 1), strPtr("New Year's Day"))).
    Add(Bl.CalendarEntry(Bl.Date(2025, 12, 25), strPtr("Christmas Day"))).
    Size().Evaluate()
// → BlNumber("2")
```

### `remove(name)`

Returns a new `BlCalendar` with all entries whose `name` equals `name` removed. If no entries have that name, returns an equivalent calendar unchanged. Unnamed entries are never removed by this method.

```go
cal := Bl.Calendar([]BlCalendarEntry{
    Bl.CalendarEntry(Bl.Date(2025, 1, 1),   strPtr("New Year's Day")),
    Bl.CalendarEntry(Bl.Date(2025, 12, 25), strPtr("Christmas Day")),
    Bl.CalendarEntry(Bl.Date(2025, 12, 26), strPtr("Boxing Day")),
})

cal.Remove("Boxing Day").Size().Evaluate()
// → BlNumber("2")

cal.Remove("Nonexistent").Size().Evaluate()
// → BlNumber("3")   (unchanged)

// Remove and replace an entry
cal.Remove("Christmas Day").Add(
    Bl.CalendarEntry(Bl.Date(2025, 12, 25), strPtr("Christmas Day")),
    Bl.CalendarEntry(Bl.Date(2025, 12, 26), strPtr("Christmas Day")),  // two days named the same
).Size().Evaluate()
// → BlNumber("4")
```

### `merge(*others)`

Returns a new `BlCalendar` containing all entries from `self` followed by all entries from each of the provided calendars, in argument order. No deduplication is performed.

```go
englandHolidays := Bl.Calendar([]BlCalendarEntry{
    Bl.CalendarEntry(Bl.Date(2025, 1, 1),   strPtr("New Year's Day")),
    Bl.CalendarEntry(Bl.Date(2025, 12, 25), strPtr("Christmas Day")),
    Bl.CalendarEntry(Bl.Date(2025, 12, 26), strPtr("Boxing Day")),
})

scotlandExtras := Bl.Calendar([]BlCalendarEntry{
    Bl.CalendarEntry(Bl.Date(2025, 1, 2),   strPtr("New Year's Holiday")),
    Bl.CalendarEntry(Bl.Date(2025, 11, 30), strPtr("St Andrew's Day")),
})

ukCombined := englandHolidays.Merge(scotlandExtras).Evaluate()
ukCombined.Size().Evaluate()
// → BlNumber("5")

// Merge a company-specific calendar on top of a regional one
Bl.CalendarVar("regional_holidays").Merge(Bl.CalendarVar("company_closures")).Evaluate(map[string]BlExpr{
    "regional_holidays": englandHolidays,
    "company_closures": Bl.Calendar([]BlCalendarEntry{
        Bl.CalendarEntry(Bl.Date(2025, 8, 15), strPtr("Company Away Day")),
    }),
})
// → BlCalendar with 4 entries
```

---

## Typical Usage

A `BlCalendar` is typically constructed once (at application startup or from a data source) and then passed into `NativeFunctionTask` tasks or `DecisionTask` decisions as a context variable.

```go
// In a NativeFunctionTask: calculate next business day
func nextBusinessDay(variables Variables) Variables {
    currentDate := variables["current_date"].(BlDate)
    holidays := variables["holidays"].(BlCalendar)

    candidate := currentDate.Add(Bl.DaysTime(1, 0, 0, 0)).Evaluate()
    for {
        isWeekend := candidate.DayOfWeek().In(
            Bl.List(Bl.String("Saturday"), Bl.String("Sunday")),
        ).Evaluate()
        isHoliday := holidays.Contains(candidate).Evaluate()
        if !isWeekend.ToNativeBoolean() && !isHoliday.ToNativeBoolean() {
            return Variables{"next_business_day": candidate}
        }
        candidate = candidate.Add(Bl.DaysTime(1, 0, 0, 0)).Evaluate()
    }
}


// As a process variable, passed to the process at start:
result := process.Evaluate(map[string]BlExpr{
    "holidays": Bl.Calendar([]BlCalendarEntry{
        Bl.CalendarEntry(Bl.Date(2025, 4, 18), strPtr("Good Friday")),
        Bl.CalendarEntry(Bl.Date(2025, 4, 21), strPtr("Easter Monday")),
    }),
    "current_date": Bl.Date(2025, 4, 17),
})
```

---

## Edge Cases

- `Bl.Calendar(nil)` produces an empty calendar with no validity bounds.
- Providing `valid_from` > `valid_to` raises `BlTypeError` at construction time.
- `valid_range` evaluates to `BlNull` when either `valid_from` or `valid_to` is `None`.
- `valid_from`/`valid_to` do not filter entry-access methods (`entries`, `find()`, `contains()`, etc.) — they are metadata for business-day arithmetic only.
- `merge()` does not automatically union validity ranges; the resulting calendar's `valid_from`/`valid_to` are both `None` unless explicitly re-supplied.
- `find()` is case-sensitive: `find("christmas day")` does not match an entry named `"Christmas Day"`.
- `contains()` and `entries_for()` with a `BlDate` point against a range with `BlDateTime` endpoints (or vice versa) evaluate to `BlNull` — mixed temporal types are not implicitly coerced.
- `remove(name)` on an empty calendar is a no-op that evaluates to an empty calendar.
- `merge()` with no arguments evaluates to an equivalent copy of `self`.
- Two calendars are equal (via `equals()`) only if they have the same entries in the same order — order matters because insertion order is preserved.
- A `BlCalendarEntry` whose `value` is a `BlRange` with non-temporal endpoints (e.g. a range of numbers) raises `BlTypeError` at evaluation time.
- `entries_in()` and `overlaps()` with a `range_` whose endpoint types differ from the calendar entries' types evaluate to `BlNull` for those mismatched entries (they are not included in results and do not contribute to the overlap test).
