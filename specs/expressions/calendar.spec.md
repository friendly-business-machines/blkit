---
name: bl.BlCalendar
description: The calendar type in the blkit expression language — a named, ordered collection of temporal entries (dates, datetimes, or date/datetime ranges) for holidays, schedules, blackout periods, maintenance windows, and the like. Covers entry construction, calendar construction with optional validity bounds, query/mutation built-ins, and the Go layer (bl.BlCalendar + expr registrations).
targets:
  - ../../core/calendar.go
---

# bl.BlCalendar — the `calendar` type

`calendar` is a blkit-specific type: an immutable, **chronologically-ordered** collection of
entries, each a temporal value (`date`, `datetime`, or a `range` of either) with an optional
name. It models holiday sets, maintenance windows, blackout periods, freeze schedules, and
similar collections. The Go value type backing it is `bl.BlCalendar`.

Chronological ordering — by entry position, ties broken by name — is canonical: `entries(c)`
yields entries in that order, equality is set-equality (same entries regardless of construction
order), and `next` / `prev` walk the same sequence. Host code passing entries to `bl.Calendar(...)`
need not pre-sort them; the constructor establishes the canonical order.

A literal would be the syntactic form for writing a constant value of a type directly inside
an expression (as `[1, 2, 3]` is for a list). **`calendar` has no such form**, and it has no
expression-language constructor either — a calendar is built host-side via the Go API (see
[§ Construction (host-side)](#construction-host-side)) and supplied to expressions as an input
variable, where it is queried — e.g. `isBusinessDay(date("2025-12-25"), ukHolidays)`.

All calendar built-ins are blkit extensions (**ext** — no DMN equivalent). It pairs with
[date.spec.md](date.spec.md)'s business-day arithmetic, which consumes a calendar. See
[range.spec.md](range.spec.md) for the range entries.

---

## Construction (host-side)

Calendars are **constructed host-side in Go**, not inside the expression language. The
expression language has no `calendar(...)` or `calendarEntry(...)` built-in — calendars enter
the engine as **input variables** (or as runtime values returned from `calendarMerge` /
`calendarDrop` / `calendarKeep` / `calendarMerge`, which never need to mint a fresh entry). The rationale: holiday sets,
maintenance windows, and blackout schedules are configuration data sourced from the host
application's database or config files; building them inline inside a business-rule expression
isn't a meaningful use case, and the host-side functional-options pattern is more idiomatic and
type-safe than encoding the same shape through an expression-language options dictionary.

```go
// host-side (Go) — calendars are built host-side and supplied to expressions as input
// variables. CalendarEntry is infallible so it inlines cleanly inside the entries slice;
// WithValidity takes a bl.BlRange covering the period the calendar is authoritative over.
var ukHolidays, _ = bl.Calendar(
    []bl.BlCalendarEntry{
        bl.CalendarEntry(blDate("2025-01-01"), "New Year's Day"),
        bl.CalendarEntry(blDate("2025-04-18"), "Good Friday"),
        bl.CalendarEntry(blDate("2025-04-21"), "Easter Monday"),
        bl.CalendarEntry(blDate("2025-05-05"), "Early May Bank Holiday"),
        bl.CalendarEntry(blDate("2025-05-26"), "Spring Bank Holiday"),
        bl.CalendarEntry(blDate("2025-08-25"), "Summer Bank Holiday"),
        bl.CalendarEntry(blDate("2025-12-25"), "Christmas Day"),
        bl.CalendarEntry(blDate("2025-12-26"), "Boxing Day"),
        bl.CalendarEntry(
            blRange(blDate("2025-12-24"), blDate("2026-01-02")),
            "Holiday closure"),
    },
    WithValidity(blRange(blDate("2025-01-01"), blDate("2025-12-31"))))

// Hand it to the engine as an input variable.
var schema, _ = bl.Schema(
    bl.Field{Name: "applicantDate", Type: bl.TypeString},
    bl.Field{Name: "ukHolidays",    Type: bl.TypeCalendar},
)
var checkHoliday, _ = bl.Expr(`isPublicHoliday(applicantDate, ukHolidays)`, schema)
var applicantDate, _ = bl.String("2025-12-25")
var inputs, _ = bl.Dictionary(map[string]bl.BlValue{
    "applicantDate": applicantDate,
    "ukHolidays":    ukHolidays,
})
var result, _ = checkHoliday.Evaluate(inputs)
```

`CalendarEntry` is infallible — it just wraps a value and optional name. Type validation (the
value must be a temporal point or temporal range, see [§ Entry kinds](#entry-kinds)) and
zone-kind homogeneity (see [§ Zone-kind homogeneity](#zone-kind-homogeneity)) are checked at
the `bl.Calendar(...)` assembly step, so a single error return surfaces all the structural
problems at once.

Full host-API signatures are in [§ Value types & host API](#value-types--host-api-exported).

### Importing from iCalendar (`.ics`)

`bl.ImportICal(r io.Reader, opts ...ICalOption) (bl.BlCalendar, error)` parses an RFC 5545
iCalendar document and returns the equivalent `bl.BlCalendar`. It's the recommended path for
holiday sets, public-holiday feeds, maintenance-window schedules, and any other data already
distributed as `.ics` (Google Calendar, Apple Calendar, Outlook, CalDAV servers, public
holiday providers, etc.). The underlying parser is
[`github.com/arran4/golang-ical`](https://github.com/arran4/golang-ical) with
[`github.com/teambition/rrule-go`](https://github.com/teambition/rrule-go) for recurrence
expansion; both are added as blkit dependencies.

```go
// host-side (Go)
var f, _    = os.Open("uk-bank-holidays-2025.ics")
defer f.Close()
var ukHolidays, _ = bl.ImportICal(f,
    WithICalExpansionWindow(blRange(blDate("2025-01-01"), blDate("2025-12-31"))),
    WithValidity(blRange(blDate("2025-01-01"), blDate("2025-12-31"))))
```

#### Mapping

| iCalendar feature | `bl.BlCalendar` representation |
|---|---|
| `VEVENT` with `DTSTART` only (or `DTSTART == DTEND`) | a point entry (`bl.BlDate` or `bl.BlDateTime` per the source value type) |
| `VEVENT` with distinct `DTSTART` / `DTEND` | a range entry — `bl.BlRange` with `start = DTSTART`, `end = DTEND` (inclusive at both ends; iCal's `DTEND` is exclusive for `DATE` values per RFC 5545, so the importer subtracts one day for whole-day ranges and uses the value as-is for `DATE-TIME` ranges) |
| `SUMMARY` | the entry name (empty / missing → unnamed entry) |
| `VEVENT` `VALUE=DATE` (all-day) | `bl.BlDate` |
| `VEVENT` `VALUE=DATE-TIME` with `Z` suffix or `TZID` | zoned `bl.BlDateTime` (or `bl.BlDate` for date values with a zone marker) |
| `VEVENT` `VALUE=DATE-TIME` with neither | naive `bl.BlDateTime` / `bl.BlDate` |
| `RRULE` / `RDATE` / `EXDATE` / `EXRULE` | expanded to individual entries within the **expansion window** (see below) |
| `VTIMEZONE` | parsed but discarded; entries reference their IANA zone via `TZID`, resolved through Go's `time.LoadLocation` (so the tzdata source still wins on DST rules — see [days_time_duration.spec.md § Construction](days_time_duration.spec.md#construction) for the tzdata caveat) |
| `VTODO`, `VFREEBUSY`, `VJOURNAL`, `VALARM`, anything else | **skipped silently** unless `WithICalStrict(true)` is set (which raises an error on any non-`VEVENT` component) |

#### Recurrence expansion

A `VEVENT` with `RRULE` represents a (potentially infinite) recurring series. The importer
requires an explicit **expansion window** before expanding such events — otherwise a yearly
Christmas Day event would generate entries forever. The window is supplied via
`WithICalExpansionWindow(bl.BlRange)`; if omitted, `WithValidity(bl.BlRange)` is used as a fallback
(since the calendar isn't authoritative outside its validity range, expanding past it
contributes nothing useful). If **neither** is supplied and the document contains any
`RRULE`, `ImportICal` returns an error rather than guess a window.

`EXDATE` / `EXRULE` exclusions are applied during expansion. `RDATE` additions are merged in
alongside the rule-generated instances.

#### Zone-kind enforcement

All resulting entries must share a single zone-kind (per
[§ Zone-kind homogeneity](#zone-kind-homogeneity)). An iCal document mixing zoned events
(events carrying a `TZID` or `Z` suffix) with naive events (no zone marker) → error. To
import a mixed-zone document, the host can re-run `ImportICal` against a filtered subset, or
post-process the resulting entries with `withoutOffset` / `withTimezone`
([datetime.spec.md § Zone stripping](datetime.spec.md#zone-stripping-ext) /
[§ Zone stripping](datetime.spec.md#zone-stripping-ext)) before rebuilding via
`bl.Calendar(...)`.

#### Options

| Option | Default | Effect |
|---|---|---|
| `WithICalExpansionWindow(bl.BlRange)` | — | Bounded window over which `RRULE` / `EXRULE` series are expanded. Required when the document contains any `RRULE` and `WithValidity` is not supplied. |
| `WithValidity(bl.BlRange)` | — | Sets the resulting calendar's validity range. Also serves as the fallback expansion window for `RRULE` events. |
| `WithICalDefaultName(string)` | `""` (unnamed) | Name applied to any imported entry whose source `VEVENT` has an empty / missing `SUMMARY`. |
| `WithICalStrict(bool)` | `false` | When `true`, any non-`VEVENT` component (`VTODO`, `VFREEBUSY`, etc.) → error. When `false`, such components are skipped silently. |

The Go signatures:

```go
type ICalOption func(*iCalConfig)
func WithICalExpansionWindow(BlRange) ICalOption
func WithICalDefaultName(string) ICalOption
func WithICalStrict(bool) ICalOption
// WithValidity is shared with bl.Calendar(...) and accepted here too — see the host API block.

func ImportICal(r io.Reader, opts ...ICalOption) (BlCalendar, error)
```

#### iCal import edge cases

- A `VEVENT` with no `DTSTART` is malformed; `ImportICal` returns a parse error naming the
  offending event (`UID`, if present).
- A `VEVENT` with `DTEND` before `DTSTART` is malformed; same treatment.
- An `RRULE` referenced without an expansion window → error (see above).
- An unknown `TZID` (not in the tzdata available to Go's `time.LoadLocation`) → error naming
  the unrecognised zone.
- A document with no `VEVENT` (only `VTIMEZONE` definitions, or only skipped component types)
  → an empty `bl.BlCalendar` (matching `bl.Calendar([]bl.BlCalendarEntry{})`).
- `ImportICal` does not deduplicate; two identical recurring instances expanded from
  overlapping `RRULE` / `RDATE` produce two entries. Apply `CalendarMerge([imported],
  WithDedupe(DedupeByValueAndName))` (single-element list) host-side if dedup is wanted.

### Entry kinds

A calendar entry pairs a temporal value with an optional name — the name is the lookup key for
queries like `find` and is otherwise display metadata. `bl.CalendarEntry(value, name...)` accepts
any of the four temporal entry kinds:

- **Single date** — the typical holiday entry, e.g. `blDate("2025-12-25")` paired with
  `"Christmas Day"`.
- **Single datetime** — for instant-specific events (deploy windows, scheduled outages), e.g.
  `blDateTime("2025-03-01T03:00:00[Europe/London]")` paired with `"Spring deploy"`.
- **Date range** — for multi-day closures, conference dates, financial periods, e.g. a
  `bl.BlRange` over `blDate("2025-12-24")` and `blDate("2026-01-02")` paired with `"Holiday
  closure"`.
- **Datetime range** — for sub-day maintenance windows or scheduled blackouts, e.g. a `bl.BlRange`
  over two zoned `bl.BlDateTime`s for a Saturday maintenance window.

Both endpoints of a range entry must be the **same** temporal kind (both `bl.BlDate` or both
`bl.BlDateTime`) and follow the same zone-kind rule as direct comparisons (mismatched
endpoints → `bl.TypeError` at range construction; see
[range.spec.md § Edge cases](range.spec.md#edge-cases)). Range entries must also be **bounded
on both ends** — an open-ended range (e.g. `[blDate("2025-01-01")..]`) is rejected by
`bl.Calendar(...)` at assembly time. Calendars represent scheduled, bounded events; an indefinite
"from X onwards" is either a configuration concern (regenerate the calendar when the scope
changes) or belongs in a different abstraction (a status flag, not a calendar entry). A
`CalendarEntry` whose value is anything other than a temporal point or a bounded temporal range
→ error from `bl.Calendar(...)` at assembly time.

### Zone-kind homogeneity

Every entry in a non-empty calendar must share the **same zone-kind** — either all entries are
**zoned** (carry an offset or IANA timezone) or all entries are **naive** (no zone). Mixing the
two inside one calendar → error from `bl.Calendar(...)` (and from `calendarMerge` at evaluation
time when merging calendars of different zone-kinds). The validity range, when supplied via
`WithValidity(bl.BlRange)`, must also match — a zoned calendar requires a zoned range; a naive
calendar requires a naive range.

```go
// host-side (Go)

// OK — both zoned.
bl.Calendar([]bl.BlCalendarEntry{
    bl.CalendarEntry(blDate("2025-12-25[Europe/London]")),
    bl.CalendarEntry(blDate("2025-12-26[Europe/London]")),
})

// OK — both naive.
bl.Calendar([]bl.BlCalendarEntry{
    bl.CalendarEntry(blDate("2025-12-25")),
    bl.CalendarEntry(blDate("2025-12-26")),
})

// Error — mixed zone-kind.
bl.Calendar([]bl.BlCalendarEntry{
    bl.CalendarEntry(blDate("2025-12-25[Europe/London]")),
    bl.CalendarEntry(blDate("2025-12-26")),
})
// → error: mixed zone-kind entries
```

The rule mirrors the existing zone-kind consistency policy in
[date.spec.md § Operators](date.spec.md#operators) and
[datetime.spec.md § Comparison semantics](datetime.spec.md#comparison-semantics): mixing zoned
and naive operands at any level returns `null` from comparisons and arithmetic, so allowing it
inside a calendar would mean queries silently never match for the wrong-kind entries. Failing
fast at construction surfaces the design mistake immediately.

Within a calendar's chosen zone-kind, **temporal kinds may still mix freely** — a single
calendar may hold date points, datetime points, date ranges, and datetime ranges side by side.
A holiday calendar can reasonably carry both a date entry (`blDate("2025-12-25")`) and a
specific-instant entry (e.g. a zoned `blDateTime` for a service). Queries that cross temporal
kinds within the same zone-kind (`contains` of a `date` point against a `datetime`-ranged
entry, etc.) return `null` for the mismatched entries; see
[§ Cross-kind matching](#cross-kind-matching) below.

To bridge across zone-kinds at query time, strip or attach zones at the consumer side before
calling — the strip/attach functions are in
[datetime.spec.md § Zone stripping](datetime.spec.md#zone-stripping-ext) and
[datetime.spec.md § Zone stripping](datetime.spec.md#zone-stripping-ext).

`[@test] ../../core/calendar_test.go`

---

## Query & inspection built-ins

| Function | Example | Result |
|---|---|---|
| `count(c)` | `count(ukHolidays)` | entry count |
| `isEmpty(c)` | `isEmpty(ukHolidays)` | `true` if the calendar has no entries |
| `entries(c)` | `entries(c)` | list of entries in chronological order (see [§ Sort order](#sort-order)) |
| `names(c)` | `names(c)` | distinct names in chronological order (first-occurrence of each name in the sorted entry sequence; unnamed entries excluded) |
| `find(c, name)` | `find(ukHolidays, "Christmas Day")` | list of entries with that name (case-sensitive) |
| `contains(c, point)` | `contains(ukHolidays, date("2025-12-25"))` | `true` if any entry covers `point` |
| `entriesFor(c, point)` | `entriesFor(c, date("2025-05-05"))` | entries covering the point |
| `overlaps(c, range)` | `overlaps(windows, [t1..t2])` | `true` if any entry overlaps the range |
| `entriesIn(c, range)` | `entriesIn(ukHolidays, [date("2025-01-01")..date("2025-03-31")])` | entries overlapping the range |
| `validFrom(c)` / `validTo(c)` | `validFrom(uk2025)` | the bound, or `null` |
| `validRange(c)` | `validRange(uk2025)` | `[validFrom..validTo]`, or `null` if either unset |
| `entryValue(e)` / `entryName(e)` | `entryName(find(c, "X")[1])` | an entry's value / name (`null` if unnamed) |
| `next(c, point[, n])` | `next(ukHolidays, date("2025-12-01"))` | the **n**th-closest entry strictly **after** `point` (so `n=1` is the immediately-next entry; higher `n` walks forward into the future). `n` defaults to `1`; `null` if fewer than `n` matching entries exist |
| `prev(c, point[, n])` | `prev(ukHolidays, date("2025-12-01"), 2)` | the **n**th-closest entry strictly **before** `point` (so `n=1` is the immediately-previous entry; higher `n` walks backward into the past). `n` defaults to `1`; `null` if fewer than `n` matching entries exist |

`contains`, `overlaps`, `count`, and `isEmpty` are overloaded by argument type — the string /
list / range forms live in their own spokes ([string.spec.md](string.spec.md),
[list.spec.md](list.spec.md), [range.spec.md](range.spec.md)). For calendar arguments, the
patcher dispatches on the receiver's type at call time.

### `next` / `prev` traversal

Calendars are sorted at construction time (see [§ Sort order](#sort-order)); `next` and `prev`
just consult that existing order. `next(c, point, n)` walks the sequence **forward** from
`point` and returns the nth entry past it; `prev(c, point, n)` walks **backward** and returns
the nth entry before it. `n` defaults to `1` and must be a positive integer (`n ≤ 0` →
`bl.TypeError`). Both return a `bl.BlCalendarEntry` on success or `bl.BlNull` when fewer than `n`
matching entries exist on the chosen side of `point`.

The strictly-past-`point` filter:

- An entry is **strictly after** `point` when its position (the value for a point entry, or
  `range.start` for a range entry) is `> point`.
- An entry is **strictly before** `point` when its position (the value for a point entry, or
  `range.end` for a range entry) is `< point`.
- A range entry whose span **straddles** `point` (begins before, ends after) is therefore
  neither next nor prev — it is the currently-active entry, not the next or previous.

`point` must share the calendar's zone-kind (per
[§ Zone-kind homogeneity](#zone-kind-homogeneity)); a mismatch → `bl.TypeError`. The temporal
kind of `point` may differ from individual entries — entries of the other kind are silently
skipped (per the same per-entry null-on-temporal-mismatch rule used by `contains` /
`entriesFor` / `overlaps` / `entriesIn`), so a `date` point against a calendar mixing date and
datetime entries walks only the date entries.

```
// expression-language
// Suppose ukHolidays holds the standard 2025 UK bank-holiday set.

next(ukHolidays, date("2025-04-01"))            // → the entry for Good Friday (2025-04-18)
next(ukHolidays, date("2025-04-01"), 2)         // → Easter Monday (2025-04-21)
next(ukHolidays, date("2025-12-31"))            // → null (no entries after the last holiday)

prev(ukHolidays, date("2025-04-21"))            // → Good Friday (the prior holiday)
prev(ukHolidays, date("2025-04-21"), 1)         // → same (n defaults to 1)
prev(ukHolidays, date("2025-01-01"))            // → null (no entries before New Year's Day)

// Range entries: the Holiday closure entry [2025-12-24..2026-01-02] straddles 2025-12-25, so
// it's the currently-active entry — by the "strictly past point" rule it's neither next nor
// prev for that point. The point entry for Christmas Day (2025-12-25) is at the point, so it's
// also neither (not strictly past).
next(ukHolidays, date("2025-12-25"))            // → Boxing Day (2025-12-26)
prev(ukHolidays, date("2025-12-25"))            // → Summer Bank Holiday (2025-08-25)
```

### Cross-kind matching

A `point` or `range` argument must share the calendar's zone-kind (per
[§ Zone-kind homogeneity](#zone-kind-homogeneity)); a mismatched zone-kind argument →
`bl.TypeError` from the query call. **Within** a calendar's zone-kind, temporal kinds may still
differ between the query and individual entries (a date query against a calendar that holds
both date and datetime entries, or vice versa), so each entry is matched independently:

- **Point against point entry** — same temporal kind: equality. Different kind → that entry
  returns `null` (no implicit coercion).
- **Point against range entry** — same kind: standard range containment (respecting the
  endpoint inclusivity). Different kind → `null` for that entry.
- **Range against point entry** — same kind: true iff the point lies in the query range.
  Different kind → `null`.
- **Range against range entry** — same kind: standard range overlap. Different kind → `null`.

A per-entry `null` contributes "no match" to the aggregating function (`contains`, `overlaps`)
without poisoning the result — scanning continues. Use the conversion functions in
[date.spec.md § Conversion](date.spec.md) / [datetime.spec.md § Conversion](datetime.spec.md)
to coerce explicitly when you need cross-temporal-kind matching to succeed.

`[@test] ../../core/calendar_test.go`

---

## Mutation built-ins (return a new calendar)

| Function | Example | Result |
|---|---|---|
| `calendarDrop(c, target[, options])` | `calendarDrop(ukHolidays, "Boxing Day")` | entries matching `target` **removed** (no-op if no match) |
| `calendarKeep(c, target[, options])` | `calendarKeep(ukHolidays, "Boxing Day")` | entries matching `target` **retained**, all others dropped (symmetric inverse of `calendarDrop`) |
| `calendarMerge(calendars[, options])` | `calendarMerge([england, scotland], {dedupeBy: "value"})` | union of all input entries, re-sorted into the result calendar's canonical chronological order (see [§ Sort order](#sort-order)); optional dedupe via `{dedupeBy, tiebreak}` |

All three return a fresh `bl.BlCalendar` — the receiver is never mutated. There is no `calendarAdd`
inside the expression language because adding an entry would require constructing a fresh
`bl.BlCalendarEntry`, which is a host-only operation
(see [§ Construction (host-side)](#construction-host-side)). To append entries, build the
expanded calendar host-side and re-supply it as an input variable.

### `calendarDrop(c, target)` / `calendarKeep(c, target)`

`calendarDrop` and `calendarKeep` are **symmetric inverses** — every match rule applies
identically, but matching entries are **removed** by `calendarDrop` and **retained** by
`calendarKeep`. Both are polymorphic on the `target` argument's type:

| `target` type | Match rule |
|---|---|
| `bl.BlString` | match entries whose `name` equals `target` (case-sensitive exact match); unnamed entries are never matched |
| `bl.BlRegex` (from [`pattern(s)`](string.spec.md#precompiled-patterns-patterns--blblregex)) | match entries whose `name` matches the precompiled regex (RE2 syntax, anchored at both ends via `^(?:…)$`); unnamed entries are never matched |
| `bl.BlDate` / `bl.BlDateTime` | match entries whose `value` equals `target` (via `bl.BlValue.Equal()`) |
| `bl.BlRange` | depends on `rangeMatch` (see below); default is structural equality |
| `bl.BlList` | match entries that satisfy **any** item in the list, applying the rule above per item |

Strings and precompiled patterns are two distinct entry-name match modes — strings are exact
matches, `pattern(...)` values are regex matches. There is no single-argument form that infers
one from the other; the type of the argument selects the mode.

#### Range-target match mode (`rangeMatch`)

When `target` is a `bl.BlRange` (or contains range items in a list), the optional `options`
dictionary key `rangeMatch` selects how it is compared against each entry:

| `rangeMatch` | Match rule for a range target `T` against an entry `e` |
|---|---|
| `"equality"` (default) | matches iff `e.value` is a `bl.BlRange` structurally equal to `T` (same endpoints and inclusivity); a point entry inside `T` is **not** matched |
| `"entryWithin"` | matches iff `e.value` lies **entirely inside** `T` — a point entry is matched when the point is in `T`; a range entry is matched when its full span is contained in `T` (partial overlap doesn't count). Reads as *"drop / keep everything **within** this period"*. |
| `"entryEncloses"` | matches iff `e.value` is a range that **entirely encloses** `T` — a point entry is essentially never matched (it would have to be a degenerate range equal to `T`); a range entry is matched when `T` lies within its span. Reads as *"drop / keep entries that **span** this period"*. |
| `"overlap"` | matches iff `e.value` **overlaps** with `T` — a point entry is matched when the point is in `T`; a range entry is matched when the two ranges share at least one element. Reads as *"drop / keep everything that **touches** this period"*. |

The two containment modes are **directional** and have distinct semantics: `"entryWithin"`
asks "is the entry inside the target?" (target is the wider one); `"entryEncloses"` asks "does
the entry contain the target?" (entry is the wider one). `"overlap"` is symmetric, so no
direction option is needed.

`rangeMatch` only affects how a `bl.BlRange` target dispatches; `bl.BlString` / `bl.BlRegex` /
`bl.BlDate` / `bl.BlDateTime` targets ignore it (those modes are about names or single-value
equality, with no range comparison to broaden). For a list `target` containing mixed kinds,
the mode applies only to the range elements; the other elements continue to use their own
rules.

```
// expression-language
// target: a Christmas-week range; entries with various positions relative to it.

calendarDrop(c, blRange(date("2025-12-22"), date("2025-12-28")))
    // → drops only entries whose value is the identical range [2025-12-22..2025-12-28]

calendarDrop(c, blRange(date("2025-12-22"), date("2025-12-28")), {rangeMatch: "entryWithin"})
    // → drops every entry within that week: Christmas Day, Boxing Day, the [24..27] curated week, etc.

calendarDrop(c, blRange(date("2025-12-22"), date("2025-12-28")), {rangeMatch: "entryEncloses"})
    // → drops only entries that span the entire Christmas week, e.g. a [2025-12-01..2026-01-31] closure

calendarDrop(c, blRange(date("2025-12-22"), date("2025-12-28")), {rangeMatch: "overlap"})
    // → drops everything touching that week, even partially (a [2025-12-28..2026-01-02] range overlaps at the endpoint)
```

```
// expression-language
calendarDrop(ukHolidays, "Christmas Day")                                  // exact name
calendarDrop(ukHolidays, pattern("^Bank Holiday.*"))                       // regex name
calendarDrop(ukHolidays, date("2025-12-25"))                               // by value (date point)
calendarDrop(ukHolidays, blRange(date("2025-12-24"), date("2026-01-02")))  // by value (range)
calendarDrop(ukHolidays, [
    "Boxing Day",
    pattern("^Bank Holiday.*"),
    date("2025-12-25"),
    blRange(date("2025-12-24"), date("2026-01-02"))
])                                                                          // mixed list

calendarKeep(ukHolidays, [date("2025-12-25"), date("2025-12-26")])         // only the Christmas pair
calendarKeep(ukHolidays, pattern("^Bank Holiday.*"))                       // only "Bank Holiday X"
calendarKeep(ukHolidays, pattern(".*"))                                    // every named entry (drops unnamed)
```

Each entry is checked against the target(s) once; an entry that matches by any criterion
participates in the drop / keep set.

A `target` whose type isn't in the table (or a list element that isn't) → `bl.TypeError`. A
`bl.BlRegex` whose source is malformed never reaches `calendarDrop` — it would have failed at the
`pattern(...)` call site. Zone-kind mismatch (a naive value target against a zoned calendar or
vice versa) → `bl.TypeError`, consistent with
[§ Zone-kind homogeneity](#zone-kind-homogeneity).

`calendarKeep` does **not** match unnamed entries via a `bl.BlString` or `bl.BlRegex` target — they
have no name to compare. An unnamed entry whose value matches a `bl.BlDate` / `bl.BlDateTime` /
`bl.BlRange` target is retained as expected. Host code that wants to retain unnamed entries
alongside name-matched ones should pass an explicit list mixing values and names.

The validity range is preserved across both functions; only the entry list changes.

The host-side equivalents are methods on `bl.BlCalendar`. The polymorphic target argument is
typed `any` and accepts: `string` (exact name), `bl.BlRegex` (regex name), `bl.BlDate`, `bl.BlDateTime`,
`bl.BlRange` (value), or a `[]any` whose elements are any combination of those. Range-match mode
for range targets is supplied via functional options:

```go
// host-side (Go)
var withoutChristmas, _ = ukHolidays.Drop("Christmas Day")
var withoutBankHols, _  = ukHolidays.Drop(mustPattern("^Bank Holiday.*"))
var withoutDec25, _     = ukHolidays.Drop(blDate("2025-12-25"))
var withoutClosure, _   = ukHolidays.Drop(blRange(blDate("2025-12-24"), blDate("2026-01-02")))
var stripped, _         = ukHolidays.Drop([]any{
    "Boxing Day",
    mustPattern("^Bank Holiday.*"),
    blDate("2025-12-25"),
})

// Range-match mode for range targets — default is RangeMatchEquality.
var christmasWeek = blRange(blDate("2025-12-22"), blDate("2025-12-28"))
var withinDropped, _    = ukHolidays.Drop(christmasWeek, WithRangeMatch(RangeMatchEntryWithin))
var enclosingDropped, _ = ukHolidays.Drop(christmasWeek, WithRangeMatch(RangeMatchEntryEncloses))
var touchingDropped, _  = ukHolidays.Drop(christmasWeek, WithRangeMatch(RangeMatchOverlap))

// Keep mirrors Drop with the symmetric inverse semantics.
var onlyChristmasPair, _ = ukHolidays.Keep([]any{
    blDate("2025-12-25"),
    blDate("2025-12-26"),
})
var onlyChristmasWeek, _ = ukHolidays.Keep(christmasWeek, WithRangeMatch(RangeMatchEntryWithin))
```

Both methods return `(bl.BlCalendar, error)`. The error covers the same failure modes as the
expression-language form: an unsupported target type, a zone-kind mismatch, an unknown
range-match mode, or a malformed list element. The validity range is preserved on the returned
calendar.

The Go signatures and the matching enum:

```go
type RangeMatchMode int
const (
    RangeMatchEquality      RangeMatchMode = iota   // default — structural equality
    RangeMatchEntryWithin                           // entry's value lies entirely inside the target range
    RangeMatchEntryEncloses                         // entry's value (necessarily a range) encloses the target range
    RangeMatchOverlap                               // entry's value shares at least one element with the target range
)

type DropKeepOption func(*dropKeepConfig)
func WithRangeMatch(RangeMatchMode) DropKeepOption

func (c BlCalendar) Drop(target any, opts ...DropKeepOption) (BlCalendar, error)
func (c BlCalendar) Keep(target any, opts ...DropKeepOption) (BlCalendar, error)
```

### `calendarMerge(calendars[, options])`

`calendarMerge` takes the **union** of entries from the supplied calendars and returns a fresh
calendar containing them in canonical chronological order (see [§ Sort order](#sort-order)).
With no second argument it returns the raw union — duplicates and all (they sit next to each
other in the sort order, since identical entries share a sort key). An optional **options
dictionary** as the second argument enables deduplication during the merge:

| Key | Values | Meaning |
|---|---|---|
| `dedupeBy` | `"value"` \| `"valueAndName"` | Entry-grouping key. `"value"` groups by entry **value** alone (date / datetime / range), so two entries with the same value but different names are treated as the same group. `"valueAndName"` groups by the (value, name) pair, so only entries that match on **both** axes collapse. Omitting `dedupeBy` disables deduplication. |
| `tiebreak` | `"first"` (default) \| `"name"` | Only consulted when `dedupeBy: "value"` (since same-value entries can have different names). `"first"` keeps the entry from the **first input calendar** in which the value appears (matching the order calendars appear in the merge's calendars list); within a single input calendar's entries the [§ Sort order](#sort-order) breaks remaining ties. `"name"` keeps the entry whose name comes first in code-point order; unnamed entries sort before any named entry (empty-string convention), and the sort order applies as the final tiebreak. |

```
// expression-language

// Plain union — no dedupe, duplicates preserved (and adjacent in the sorted output).
calendarMerge([england, scotland])

// Dedupe by value only — keep the entry from the first calendar in which each value appears.
calendarMerge([england, scotland], {dedupeBy: "value"})

// Same, but on a value-clash keep the entry whose name sorts first (code-point ascending).
calendarMerge([england, scotland, wales], {dedupeBy: "value", tiebreak: "name"})

// Dedupe by (value, name) pair — entries that differ on either axis are kept.
calendarMerge([england, scotland], {dedupeBy: "valueAndName"})
```

The recognised keys are `dedupeBy` and `tiebreak`; an unknown key → `bl.TypeError`. A `dedupeBy`
value other than `"value"` or `"valueAndName"`, or a `tiebreak` value other than `"first"` or
`"name"`, is also a `bl.TypeError`. `tiebreak` is silently ignored when `dedupeBy` is omitted
(no groups to break) or when it equals `"valueAndName"` (groups are defined by exact equality
on both axes, so any group member is interchangeable — the first-occurrence one is kept for
determinism).

Validity bounds are **not** unioned across the input calendars — the result has none unless
re-supplied to a follow-up `bl.Calendar(..., WithValidity(...))` host construction. Dedupe
operates on entries only.

**Zone-kind enforcement** (see [§ Zone-kind homogeneity](#zone-kind-homogeneity)):
`calendarMerge` rejects a list of calendars whose zone-kinds differ with `bl.TypeError`. Merging
an empty calendar with a non-empty one is fine; the result inherits the non-empty operand's
zone-kind.

The host-side equivalent is a package-level `CalendarMerge` function that takes the input
calendars as a slice plus functional options for dedupe:

```go
// host-side (Go)
var concat, _   = CalendarMerge([]bl.BlCalendar{england, scotland})
var byValue, _  = CalendarMerge([]bl.BlCalendar{england, scotland},
    WithDedupe(DedupeByValue))                                          // tiebreak defaults to TiebreakFirst
var byValName, _ = CalendarMerge([]bl.BlCalendar{england, scotland},
    WithDedupe(DedupeByValueAndName))
var byValNm, _  = CalendarMerge([]bl.BlCalendar{england, scotland, wales},
    WithDedupe(DedupeByValue), WithTiebreak(TiebreakName))              // value-grouping, name-tiebreak
```

The Go signatures and the matching enums:

```go
type DedupeMode int
const (
    DedupeNone DedupeMode = iota          // default — no deduplication
    DedupeByValue                          // group by entry value alone
    DedupeByValueAndName                   // group by (value, name) pair
)

type TiebreakMode int
const (
    TiebreakFirst TiebreakMode = iota     // default — entry from the first input calendar in which the value appears wins
    TiebreakName                          // value-clash winner is the name that sorts first in code-point order
)

type MergeOption func(*mergeConfig)
func WithDedupe(DedupeMode) MergeOption
func WithTiebreak(TiebreakMode) MergeOption

func CalendarMerge(calendars []BlCalendar, opts ...MergeOption) (BlCalendar, error)
```

Validity bounds are not unioned (matching the expression-language behaviour); the returned
calendar has none unless re-supplied via a follow-up `bl.Calendar(..., WithValidity(...))`.

`[@test] ../../core/calendar_test.go`

---

## Operators

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `=` `!=` | equality (same entries as a set, same validity bounds) | `c1 = c2` | `true` iff the calendars contain the same entries and the same validity range, regardless of construction order |
| `in` | point membership in the calendar | `date("2025-12-25") in ukHolidays` | `true` iff any entry covers the point (sugar for `contains(c, point)`) |

The `in` operator on a calendar is **patcher-lowered** to a call to `contains(c, point)`, so
`date("2025-12-25") in ukHolidays` is exactly equivalent to
`contains(ukHolidays, date("2025-12-25"))` — the per-entry cross-temporal-kind null-on-mismatch
rule from [§ Cross-kind matching](#cross-kind-matching) applies identically, and the operand
must share the calendar's zone-kind (per
[§ Zone-kind homogeneity](#zone-kind-homogeneity)). The left operand must be a `bl.BlDate` or
`bl.BlDateTime`; a range left operand → `bl.TypeError` from the patcher (use the explicit
`overlaps(c, r)` / `entriesIn(c, r)` calls for range-on-calendar queries — those have to pick
between overlap and containment semantics, which `in` doesn't).

Calendars have no arithmetic operators and no ordering operators (`<`/`<=`/`>`/`>=`).

`[@test] ../../core/calendar_test.go`

---

## Use with date arithmetic

Calendars are the holiday source for the business-day built-ins in
[date.spec.md](date.spec.md):

```
// expression-language
addBusinessDays(date("2025-04-17"), 2, ukHolidays)              // skips weekends + calendar holidays
isPublicHoliday(date("2025-04-18"), ukHolidays)                 // → true
businessDaysBetween(date("2025-04-14"), date("2025-04-25"), ukHolidays)  // → 8
```

Iteration outside `[validFrom, validTo]` is silently tolerated by default — the calendar simply
contributes no holiday information beyond its bounds. Callers that need a hard guarantee can
opt in by passing `strictCalendarRange: true` to any iterating business-day function, which
raises `bl.CalendarRangeError` the moment iteration would step past the boundary (see
[datetime.spec.md § Calendar-range strictness](datetime.spec.md#calendar-range-strictness)).

---

## Semantics & behaviour

- A calendar is an immutable, **chronologically-ordered** collection. Every mutation built-in
  returns a fresh calendar; the receiver is unchanged. The construction-time entry slice
  order is not preserved — see [§ Sort order](#sort-order) for the canonical ordering.
- A non-empty calendar has a single **zone-kind** (zoned or naive); all entries and the
  validity bounds must agree on it. See
  [§ Zone-kind homogeneity](#zone-kind-homogeneity) for the construction rule and
  [§ Cross-kind matching](#cross-kind-matching) for the query consequence.

### Sort order

A calendar's entries are kept in a **total order** computed at construction time. `entries(c)`
yields entries in this order, `next` / `prev` walk it, and equality is set-equality (two
calendars built from the same entries in different argument orders are equal).

The sort key is a tuple, compared lexicographically:

1. **Position** (ascending). For a point entry (`bl.BlDate` or `bl.BlDateTime`), the position is the
   value itself. For a range entry, the position is `range.start`. When the entry types differ
   on this axis (a `bl.BlDate` against a `bl.BlDateTime`, both in the calendar's shared zone-kind),
   the `bl.BlDate` is projected to **midnight of that day** in the same zone-kind for the
   comparison only — its in-place storage is unchanged. So
   `date("2025-12-25")` sorts at the same position as `datetime("2025-12-25T00:00:00")`.
2. **Specificity** (point before range, on tie). When two entries share a position but one is
   a point and the other is a range starting at that position, the point sorts first — it is
   the more specific entry.
3. **Range end** (ascending, on tie within range entries). When two ranges share `range.start`,
   the one with the smaller `range.end` sorts first; this puts shorter ranges before longer
   ones.
4. **Range inclusivity** (closed-closed < closed-open < open-closed < open-open, on tie). A
   stable, deterministic order for ranges whose endpoints match but whose inclusivity differs.
5. **Name** (code-point ascending, on tie). Unnamed entries sort as empty string (so before
   any named entry). This is the final tiebreaker among observably-distinct entries.

Two entries that are equal on all five keys (same position, same kind, same range bounds, same
name) are observationally indistinguishable; their relative order is implementation-defined
but stable for a given calendar value, so `entries(c)` is deterministic.

For zoned calendars, positions compare as **UTC instants** (the same rule as zoned `bl.BlDateTime`
comparison in [datetime.spec.md § Comparison semantics](datetime.spec.md#comparison-semantics));
for naive calendars, positions compare as **wall-clock**. A calendar's zone-kind is uniform
([§ Zone-kind homogeneity](#zone-kind-homogeneity)), so the comparison kind never varies within
a single calendar.

- Equality is structural: same entries in the same order, with matching names, values, and
  validity bounds.
- Validity bounds (`validFrom` / `validTo`) are advisory metadata for the business-day
  arithmetic in [date.spec.md](date.spec.md) — they do **not** filter the query built-ins.
  `contains(c, point)` returns `true` for a point covered by an entry even if the point falls
  outside the validity range.
- Range entries follow the inclusivity of their endpoints. A `[date("2025-12-24")..date("2026-01-02")]`
  closure entry contains both `2025-12-24` and `2026-01-02`; switching to `[date("2025-12-24")..date("2026-01-02"))`
  excludes the closing date.
- Entries are matched independently against the query argument — a per-entry `null` result
  contributes no match but doesn't poison the aggregate result. So a `contains(c, date(…))`
  against a calendar containing both date and datetime entries scans every entry; only the
  date entries can match, and the call returns `true` if any of those match.

---

## Go implementation (expr extension)

Lives in `expr/calendar.go`. Shared mechanics in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).

### Value types & host API (exported)

`bl.BlCalendar` is the immutable Go value type that represents a calendar inside the engine and at
the host-code boundary. It wraps an insertion-ordered slice of `bl.BlCalendarEntry` plus the two
optional validity bounds. All fields are private so callers cannot mutate the underlying value;
every operation in the library returns a fresh `bl.BlCalendar`.

`bl.BlCalendarEntry` is the immutable Go value type for a single entry. It pairs an optional name
(a `*string` so unnamed entries are distinct from named-with-empty-string) with a temporal
`value` — one of `bl.BlDate`, `bl.BlDateTime`, or a `bl.BlRange` whose endpoints are one of those.
`bl.BlCalendarEntry` implements `bl.BlValue` so it can be returned from `entries(c)` and `find(c,
name)` (which produce a `bl.BlList` of entries), but its `Type()` returns `bl.TypeAny` because it
is not a first-class language type with a literal form — host code and other built-ins treat it
opaquely, accessing only via `entryName(e)` and `entryValue(e)`.

The exported surface has four parts:

- **`bl.BlValue` interface methods on `bl.BlCalendar`** — `Type()`, `Equal()`, `bl.String()`, and the
  unexported `isBlValue()` marker. `Equal` is structural — same entries in the same order, same
  validity bounds. `bl.String()` doubles as the `fmt.Stringer` implementation, producing a
  multi-line summary suitable for debug output (e.g.
  `"bl.BlCalendar{2025-01-01: New Year's Day, 2025-12-25: Christmas Day}"`).
- **`bl.BlValue` interface methods on `bl.BlCalendarEntry`** — same set, with `Type()` returning
  `bl.TypeAny`. `Equal` compares both name and value; `bl.String()` produces e.g.
  `"2025-12-25: Christmas Day"` (or `"2025-12-25"` for unnamed).
- **`bl.CalendarEntry(value, name...)` / `bl.Calendar(entries, opts...)`** — the host constructors.
  `CalendarEntry` is **infallible**: it just wraps the value and the optional name into a
  `bl.BlCalendarEntry` so it inlines cleanly inside an entries slice literal. All validation —
  the value must be a temporal point or temporal range, the entries must share a single
  zone-kind, the validity range must match the entries' zone-kind, and the range must be
  well-formed — is performed by `bl.Calendar(entries, opts...)`, which returns an `error`
  describing the first structural problem found. Validity is supplied as a single functional
  option `WithValidity(bl.BlRange)` taking the range the calendar is authoritative over; the
  range may be open at either end (`[from..]` for an indefinite upper bound, `[..to]` for an
  indefinite lower bound). Omitting `WithValidity` leaves the calendar unbounded on both ends.
  The same zone-kind homogeneity check runs at evaluation time inside `calendarMerge` when
  combining calendars of different kinds.
- **`Entries()` / `ValidFrom()` / `ValidTo()` / `ZoneKind()` accessors on `bl.BlCalendar`**, plus
  **`Name()` / `Value()` accessors on `bl.BlCalendarEntry`** — hand the internal state back to host
  code in defensive-copy form. `Entries()` returns a fresh `[]bl.BlCalendarEntry`; mutating the
  returned slice does not affect the calendar. `ZoneKind()` returns the calendar's declared
  zone-kind as a small enum (`CalendarZoneNaive` / `CalendarZoneZoned` / `CalendarZoneEmpty`
  for an empty calendar with no determined kind), so host code can check before adding entries.

```go
// bl.BlCalendarEntry is an immutable (optional-name, temporal-value) pair.
type BlCalendarEntry struct {
    name  *string  // nil == unnamed (distinct from named-with-empty-string)
    value BlValue  // BlDate / BlDateTime / BlRange of either
}

// bl.BlValue interface — required by all Bl* value types.
func (BlCalendarEntry) Type() Type { return TypeAny } // not a first-class language type
func (e BlCalendarEntry) Equal(other BlValue) BlValue     // same name and value
func (e BlCalendarEntry) String() string                  // "2025-12-25: Christmas Day" / "2025-12-25"
func (BlCalendarEntry) isBlValue() {}

// Host accessors.
func (e BlCalendarEntry) Name() (string, bool)            // value, ok — ok == false for unnamed
func (e BlCalendarEntry) Value() BlValue

// bl.BlCalendar wraps insertion-ordered entries plus optional validity bounds.
type BlCalendar struct {
    entries             []BlCalendarEntry
    validFrom, validTo  BlValue            // BlNull when unset
}

// bl.BlValue interface — required by all Bl* value types.
func (BlCalendar) Type() Type { return TypeCalendar }
func (c BlCalendar) Equal(other BlValue) BlValue   // set-equality on entries; same validity range
func (c BlCalendar) String() string                // debug summary
func (BlCalendar) isBlValue() {}

// Host constructors.
type CalendarOption func(*bl.BlCalendar)
func WithValidity(r BlRange) CalendarOption                                  // r may be open on either end
func CalendarEntry(value BlValue, name ...string) BlCalendarEntry            // infallible; validation deferred to Calendar(...)
func Calendar(entries []BlCalendarEntry, opts ...CalendarOption) (BlCalendar, error) // non-temporal entry, mixed zone-kind, or validFrom > validTo → error

// Host accessors (consume an evaluated result).
func (c BlCalendar) Entries() []BlCalendarEntry   // defensive copy
func (c BlCalendar) ValidFrom() BlValue           // BlNull when unset
func (c BlCalendar) ValidTo() BlValue             // BlNull when unset
func (c BlCalendar) ZoneKind() CalendarZoneKind   // CalendarZoneNaive / CalendarZoneZoned / CalendarZoneEmpty

// Host-side equivalents of the expression-language mutation built-ins.
// Drop/Keep target accepts: string | bl.BlRegex | bl.BlDate | bl.BlDateTime | bl.BlRange | []any.
// The opts ... slot supplies WithRangeMatch(...) for range-target dispatch (see § calendarDrop).
func (c BlCalendar) Drop(target any, opts ...DropKeepOption) (BlCalendar, error)
func (c BlCalendar) Keep(target any, opts ...DropKeepOption) (BlCalendar, error)
func CalendarMerge(calendars []BlCalendar, opts ...MergeOption) (BlCalendar, error)

// Import from RFC 5545 iCalendar (.ics) — see § Importing from iCalendar.
func ImportICal(r io.Reader, opts ...ICalOption) (BlCalendar, error)

type CalendarZoneKind int
const (
    CalendarZoneEmpty CalendarZoneKind = iota   // no entries — kind not yet determined
    CalendarZoneNaive
    CalendarZoneZoned
)
```

### Backing implementations (unexported, suffix `Fn`)

Calendar has **no per-type operator implementation functions**. Equality (`=` / `!=`)
dispatches through the `bl.BlValue.Equal()` interface method (see [§ Value types & host
API](#value-types--host-api-exported)). Calendar has no arithmetic operators, no ordering
operators, and no `in` operator.

The library functions are implemented as these typed/variadic Go functions, wrapped by
`typed1`/`typed2`/`variadic` at registration time. Per-entry matching helpers
(`entryMatchesPoint`, `entryOverlapsRange`) factor out the cross-kind null-on-mismatch logic
so that `contains` / `entriesFor` / `overlaps` / `entriesIn` share one implementation:

```go
// Typed implementations — wrapped by typed1/typed2 at registration.
func entriesFn(c BlCalendar) BlList                                       // insertion-ordered
func namesFn(c BlCalendar) BlList                                         // distinct names; unnamed excluded
func findFn(c BlCalendar, name BlString) BlList                           // case-sensitive; empty list if absent
func calContainsFn(c BlCalendar, point BlValue) BlBoolean                 // overloads string/range/list contains
func entriesForFn(c BlCalendar, point BlValue) BlList
func calOverlapsFn(c BlCalendar, r BlRange) BlBoolean                     // overloads range overlaps
func entriesInFn(c BlCalendar, r BlRange) BlList
func validFromFn(c BlCalendar) BlValue                                    // BlValue so it can be BlNull
func validToFn(c BlCalendar) BlValue
func validRangeFn(c BlCalendar) BlValue                                   // BlRange or BlNull
func nextFn(args ...any) (any, error)                                     // (c, point) | (c, point, n) — point is BlDate | BlDateTime; returns BlCalendarEntry or BlNull
func prevFn(args ...any) (any, error)                                     // (c, point) | (c, point, n) — symmetric inverse of nextFn
func entryNameFn(e BlCalendarEntry) BlValue                               // BlString or BlNull
func entryValueFn(e BlCalendarEntry) BlValue                              // BlDate / BlDateTime / BlRange
func calendarDropFn(args ...any) (any, error)                             // (c, target) | (c, target, BlDictionary) — target is BlString | BlRegex | BlDate | BlDateTime | BlRange | BlList; options dict accepts `rangeMatch`
func calendarKeepFn(args ...any) (any, error)                             // symmetric inverse of calendarDropFn — same argument shapes
func calendarMergeFn(args ...any) (any, error)                            // (BlList) | (BlList, BlDictionary) — union, re-sorted into canonical order [+ {dedupeBy, tiebreak} options dict]
func calCountFn(c BlCalendar) BlNumber                                    // overload of list count
func calIsEmptyFn(c BlCalendar) BlBoolean                                 // overload of list isEmpty

// Internal cross-kind matcher used by contains/entriesFor/overlaps/entriesIn.
func entryMatchesPoint(e BlCalendarEntry, point BlValue) BlValue          // BlBoolean(true|false) or BlNull on temporal-kind mismatch
func entryOverlapsRange(e BlCalendarEntry, r BlRange) BlValue             // BlBoolean(true|false) or BlNull on temporal-kind mismatch

// Zone-kind helpers used by both construction and query paths.
func temporalZoneKind(v BlValue) CalendarZoneKind                         // for an entry value (point/range) or a query arg
func enforceCalendarZoneKind(c BlCalendar, v BlValue) error               // TypeError when v's kind doesn't match c's
```

The host-construction check (in `bl.Calendar(...)` / `bl.CalendarEntry(...)`), the
calendar-merge check (`calendarMergeFn`), and the query-time check (`calContainsFn`,
`entriesForFn`, `calOverlapsFn`, `entriesInFn`) all share `enforceCalendarZoneKind` — so the
homogeneity rule has a single source of truth, and any new calendar-aware function inherits it
by routing through the helper.

Native Go inputs wrap through the engine's input bridge — a host-built `bl.BlCalendar` passed as an
input variable arrives unchanged. There is no native Go type that maps to `bl.BlCalendar`, so the
construction path is always the host constructors above (or the `calendar` / `calendarEntry`
built-ins inside an expression).

### Registrations (`calendarOptions`, unexported — all ext)

`calendarOptions()` returns the slice of `expr.Option` values the engine consumes during
initialisation to learn about the calendar library. Each entry is built with
`expr.Function(name, impl, typeHints...)`, where:

- `name` is the identifier the parser will recognise in expressions.
- `impl` must have the signature `func(...any) (any, error)` — that is the only shape
  [`expr-lang/expr`](https://github.com/expr-lang/expr) accepts. The `typed1` / `typed2`
  adapters (defined in [bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go))
  wrap a typed implementation such as `func(bl.BlCalendar) bl.BlList` into that shape; the variadic
  impls are registered directly because their multi-shape dispatch can't be expressed as a
  fixed-arity adapter.
- `typeHints` is a variadic list of `new(func(...) ...)` values. The engine reflects on them
  at compile time to validate that callers supply the right argument types — they carry no
  runtime cost. Multiple hints register the function as overloaded across signatures (e.g.
  `contains` / `overlaps` / `count` / `isEmpty` add `bl.BlCalendar` signatures to the same names
  registered in other spokes).

The registrations are grouped by role: query/inspection, mutation, and the overloaded entries
shared with other spokes. Calendar construction is host-only (see
[§ Construction (host-side)](#construction-host-side)) — no `calendar(...)` /
`calendarEntry(...)` / `calendarAdd(...)` built-ins are registered.

```go
func calendarOptions() []expr.Option {
    return []expr.Option{
        // NOTE — no calendar(...) or calendarEntry(...) constructors; calendars and entries are
        // built host-side via the bl.Calendar(...) / bl.CalendarEntry(...) Go constructors (see
        // § Value types & host API). Likewise no calendarAdd, which would require minting a
        // fresh entry from inside an expression.

        // query / inspection
        expr.Function("entries",    typed1(entriesFn),    new(func(bl.BlCalendar) bl.BlList)),
        expr.Function("names",      typed1(namesFn),      new(func(bl.BlCalendar) bl.BlList)),
        expr.Function("find",       typed2(findFn),       new(func(bl.BlCalendar, bl.BlString) bl.BlList)),
        expr.Function("contains",   typed2(calContainsFn), new(func(bl.BlCalendar, bl.BlValue) bl.BlBoolean)),     // overload; string/list/range overloads elsewhere
        expr.Function("entriesFor", typed2(entriesForFn), new(func(bl.BlCalendar, bl.BlValue) bl.BlList)),
        expr.Function("overlaps",   typed2(calOverlapsFn), new(func(bl.BlCalendar, bl.BlRange) bl.BlBoolean)),     // overload; range overload in range.spec.md
        expr.Function("entriesIn",  typed2(entriesInFn),  new(func(bl.BlCalendar, bl.BlRange) bl.BlList)),
        expr.Function("validFrom",  typed1(validFromFn),  new(func(bl.BlCalendar) bl.BlValue)),
        expr.Function("validTo",    typed1(validToFn),    new(func(bl.BlCalendar) bl.BlValue)),
        expr.Function("validRange", typed1(validRangeFn), new(func(bl.BlCalendar) bl.BlValue)),
        expr.Function("entryName",  typed1(entryNameFn),  new(func(bl.BlCalendarEntry) bl.BlValue)),
        expr.Function("entryValue", typed1(entryValueFn), new(func(bl.BlCalendarEntry) bl.BlValue)),
        expr.Function("next",       nextFn,
            new(func(bl.BlCalendar, bl.BlDate) bl.BlValue),
            new(func(bl.BlCalendar, bl.BlDate, bl.BlNumber) bl.BlValue),
            new(func(bl.BlCalendar, bl.BlDateTime) bl.BlValue),
            new(func(bl.BlCalendar, bl.BlDateTime, bl.BlNumber) bl.BlValue)),                                  // returns bl.BlCalendarEntry or bl.BlNull
        expr.Function("prev",       prevFn,
            new(func(bl.BlCalendar, bl.BlDate) bl.BlValue),
            new(func(bl.BlCalendar, bl.BlDate, bl.BlNumber) bl.BlValue),
            new(func(bl.BlCalendar, bl.BlDateTime) bl.BlValue),
            new(func(bl.BlCalendar, bl.BlDateTime, bl.BlNumber) bl.BlValue)),                                  // symmetric inverse of next

        // mutation — each returns a fresh calendar
        expr.Function("calendarDrop", calendarDropFn,
            // (c, target) — target type selects the dispatch
            new(func(bl.BlCalendar, bl.BlString) bl.BlCalendar),
            new(func(bl.BlCalendar, bl.BlRegex) bl.BlCalendar),
            new(func(bl.BlCalendar, bl.BlDate) bl.BlCalendar),
            new(func(bl.BlCalendar, bl.BlDateTime) bl.BlCalendar),
            new(func(bl.BlCalendar, bl.BlRange) bl.BlCalendar),
            new(func(bl.BlCalendar, bl.BlList) bl.BlCalendar),
            // (c, target, options) — options dict; recognised key `rangeMatch`
            new(func(bl.BlCalendar, bl.BlString,    bl.BlDictionary) bl.BlCalendar),
            new(func(bl.BlCalendar, bl.BlRegex,     bl.BlDictionary) bl.BlCalendar),
            new(func(bl.BlCalendar, bl.BlDate,      bl.BlDictionary) bl.BlCalendar),
            new(func(bl.BlCalendar, bl.BlDateTime,  bl.BlDictionary) bl.BlCalendar),
            new(func(bl.BlCalendar, bl.BlRange,     bl.BlDictionary) bl.BlCalendar),
            new(func(bl.BlCalendar, bl.BlList,      bl.BlDictionary) bl.BlCalendar)),                          // polymorphic; see § calendarDrop / calendarKeep
        expr.Function("calendarKeep", calendarKeepFn,
            new(func(bl.BlCalendar, bl.BlString) bl.BlCalendar),
            new(func(bl.BlCalendar, bl.BlRegex) bl.BlCalendar),
            new(func(bl.BlCalendar, bl.BlDate) bl.BlCalendar),
            new(func(bl.BlCalendar, bl.BlDateTime) bl.BlCalendar),
            new(func(bl.BlCalendar, bl.BlRange) bl.BlCalendar),
            new(func(bl.BlCalendar, bl.BlList) bl.BlCalendar),
            new(func(bl.BlCalendar, bl.BlString,    bl.BlDictionary) bl.BlCalendar),
            new(func(bl.BlCalendar, bl.BlRegex,     bl.BlDictionary) bl.BlCalendar),
            new(func(bl.BlCalendar, bl.BlDate,      bl.BlDictionary) bl.BlCalendar),
            new(func(bl.BlCalendar, bl.BlDateTime,  bl.BlDictionary) bl.BlCalendar),
            new(func(bl.BlCalendar, bl.BlRange,     bl.BlDictionary) bl.BlCalendar),
            new(func(bl.BlCalendar, bl.BlList,      bl.BlDictionary) bl.BlCalendar)),                          // symmetric inverse of calendarDrop
        expr.Function("calendarMerge", calendarMergeFn,
            new(func(bl.BlList) bl.BlCalendar),
            new(func(bl.BlList, bl.BlDictionary) bl.BlCalendar)),                                          // see § calendarMerge for the options dict

        // overloads of list-spoke aggregate functions (canonical entries in list.spec.md)
        expr.Function("count",   typed1(calCountFn),   new(func(bl.BlCalendar) bl.BlNumber)),                   // overload
        expr.Function("isEmpty", typed1(calIsEmptyFn), new(func(bl.BlCalendar) bl.BlBoolean)),                  // overload
    }
}
```

`isPublicHoliday` / `isBusinessDay` / `addBusinessDays` / … ([date.spec.md](date.spec.md)) take
a `bl.BlCalendar` (named `phCalendar`). Iterating variants raise `bl.CalendarRangeError` past the
validity bounds only when `strictCalendarRange: true` is supplied.

`[@test] ../../core/calendar_test.go`

---

## Edge cases

- Empty calendar via `bl.Calendar(nil)` or `bl.Calendar([]bl.BlCalendarEntry{})` → no entries, no
  validity bounds. A `WithValidity(r)` whose range is malformed (start > end, or endpoints
  with mismatched zone-kind / temporal kind) → error from `bl.BlRange` construction, surfaced via
  `bl.Calendar(...)`.
- `validRange(c)` → `null` unless both bounds are set; validity does not filter the query
  built-ins.
- `find` and the by-name form (`bl.BlString` target) of `calendarDrop` / `calendarKeep` are
  case-sensitive exact matchers; the by-pattern form (`bl.BlRegex` target) is anchored end-to-end
  RE2 (use `.*` for partial matches). Unnamed entries are never matched by either name form,
  so `calendarKeep(c, pattern(".*"))` drops every unnamed entry.
- `calendarDrop(c, [])` returns the receiver unchanged; `calendarKeep(c, [])` returns an empty
  calendar (kept set is empty by definition). A list element whose type isn't string / regex /
  date / datetime / range → `bl.TypeError` at evaluation from either function.
- A `bl.BlRegex` target whose source was malformed never reaches `calendarDrop` / `calendarKeep` —
  it would have failed at the `pattern(...)` call site with `bl.RegexError`. See
  [string.spec.md § Precompiled patterns](string.spec.md#precompiled-patterns-patterns--blblregex).
- `calendarMerge` does not union validity bounds (result has none unless re-supplied
  host-side). It performs no deduplication by default; pass `{dedupeBy: "value"}` or
  `{dedupeBy: "valueAndName"}` to dedupe. Unknown keys in the options dictionary, or unknown
  values for `dedupeBy` / `tiebreak`, → `bl.TypeError`. `tiebreak` is consulted only when
  `dedupeBy: "value"`; it is silently ignored otherwise. Dedupe equality is structural: two
  ranges with the same endpoints and inclusivity are equal, but a point entry never matches a
  range entry even if the point lies inside the range (use `entriesFor` / `entriesIn` for
  containment-style queries).
- Mixing zoned and naive entries (or zoned/naive entries with the opposite-kind validity
  bounds) → error from `bl.Calendar(...)`; mixing zoned and naive calendars in `calendarMerge`
  → `bl.TypeError` at evaluation. See [§ Zone-kind homogeneity](#zone-kind-homogeneity).
- Querying a calendar with a `point` or `range` whose zone-kind doesn't match the calendar's
  → `bl.TypeError`. To bridge, strip or attach zones at the consumer side via
  `withoutOffset` / `withoutTimezone` / `withOffset` / `withTimezone`.
- `contains` / `entriesFor` / `overlaps` / `entriesIn` with mismatched **temporal** types
  *within* the calendar's zone-kind (e.g. `date` point against a `datetime`-ranged entry) →
  `null` for the mismatched entries; the aggregating function treats per-entry `null` as no
  match and keeps scanning.
- `bl.CalendarEntry(value, ...)` is infallible — non-temporal values are accepted into the entry
  wrapper but rejected by `bl.Calendar(...)` at assembly time with a structural error naming the
  offending entry's index. A range whose endpoints are mismatched (e.g. date start with
  datetime end, or differing zone-kinds) → error from `bl.BlRange` construction (see
  [range.spec.md § Edge cases](range.spec.md#edge-cases)) — caught before the entry reaches
  `CalendarEntry`.
- An unbounded range entry (e.g. an open-ended `bl.BlRange`) is **rejected** by `bl.Calendar(...)` —
  calendars hold scheduled, bounded events. The validity range supplied via `WithValidity(r)`
  may still be open on either end (that's a property of the calendar's scope, not of a single
  entry).
- Equality requires the same entries in the same order **and** the same validity bounds; two
  otherwise-identical calendars with different `validFrom` are not equal.
- `entryName(e)` returns `bl.BlNull` for unnamed entries; `Name()` in host code returns
  `("", false)`.
