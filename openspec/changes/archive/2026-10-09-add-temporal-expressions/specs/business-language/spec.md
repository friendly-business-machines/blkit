# Spec Delta

## MODIFIED Requirements

### Requirement: MVP built-in types are fixed
The language SHALL provide the built-in user-facing types `Bool`, `String`, `Number`, `Date`, `DateTime`, `Time`, `DTDuration`, `YMDuration`, `Calendar`, `CalendarEntry`, and `List<T>`. `Date` SHALL represent a Gregorian calendar date, optionally naive, offset-bearing, or IANA-zoned; `Time` SHALL represent a wall-clock time with optional fractional seconds and the same zone choices; `DateTime` SHALL combine both. Serialized temporal inputs SHALL accept their ISO 8601 date/time representations and RFC 9557 `[Zone]` suffixes; invalid date/time, offset, zone, or conflicting zone forms SHALL fail validation. For `Time`, `24:00:00` SHALL be accepted and normalized to `00:00:00`, while values after `24:00:00` and leap seconds SHALL be rejected. `.bl` SHALL provide `date(...)`, `time(...)`, and `datetime(...)` constructors, with valid text or documented component/conversion arguments; constant invalid text SHALL fail at compilation and invalid runtime inputs SHALL report an evaluation error. `dateTime(...)` SHALL no longer be a constructor. Plain quoted text SHALL remain `String`. Comparison of two naive points SHALL use wall-clock order; comparison of two zoned datetimes SHALL use instants; zoned dates SHALL use the documented midnight projection, and zoned times SHALL compare at the evaluation's date in their respective zones. Mixed naive/zoned point comparisons SHALL fail rather than silently infer a zone. The language SHALL continue to use `==` and `!=` for equality; `=` SHALL only bind a supported named function argument, never compare values.

#### Scenario: Number literal typing
- **WHEN** source uses numeric literals such as `1`, `1000`, or `12.50`
- **THEN** validation treats those literals as `Number` values without requiring an integer type

#### Scenario: Typed list literal
- **WHEN** a value of type `List<Number>` is supplied as `[1, 2.5]`
- **THEN** validation accepts both elements as `Number` values

#### Scenario: List element type mismatch
- **WHEN** a value of type `List<Number>` is supplied as `[1, "two"]`
- **THEN** validation fails with a diagnostic identifying the incompatible element

#### Scenario: DateTime input
- **WHEN** a typed `DateTime` field receives `"2026-10-02T09:30:00+02:00"`, `"2026-10-02T09:30:00"`, or `"2026-10-02T09:30:00[Europe/Paris]"`
- **THEN** validation accepts each form; two zoned datetimes compare by UTC instant and two naive datetimes by wall clock

#### Scenario: Date and Time input and typed literals
- **WHEN** `Date` receives `"2026-10-02"` or `"2026-10-02+05:30"`, `Time` receives `"09:30:00.250"` or `"09:30:00+02:00"`, or an expression constructs equivalent values with `date(...)` and `time(...)`
- **THEN** the values validate as their declared temporal types, retaining the specified zone kind

#### Scenario: End of day is normalized
- **WHEN** a typed `Time` input or `time(...)` expression receives `"24:00:00"`
- **THEN** it yields `time("00:00:00")` without recording a day advance

#### Scenario: Invalid temporal values
- **WHEN** `Date` receives `"2026-02-30"`, `Time` receives `"24:00:01"` or a leap second, or a temporal constructor has invalid constant text
- **THEN** input validation fails or compilation fails, respectively, with a diagnostic

#### Scenario: Constructor and equality migration
- **WHEN** an expression uses `datetime("2026-10-02T08:30:00Z") == datetime("2026-10-02T09:30:00+01:00")`, `dateTime(...)`, or `a = b`
- **THEN** the first evaluates to `true` and the latter two fail validation

#### Scenario: Unsupported built-in type
- **WHEN** source uses an unsupported built-in type such as `Table<T>`, `Range`, `Any`, or `Optional<T>`
- **THEN** validation fails with a diagnostic identifying the unsupported type

## ADDED Requirements

### Requirement: Temporal construction and properties
`date(text | year, month, day | datetime)`, `time(text | hour, minute, second[, offset] | datetime)`, and `datetime(text | date, time)` SHALL construct or extract corresponding typed values. `today()` SHALL produce a naive current local date and `now()` a zoned current local datetime; `time(now())` SHALL return its time. Properties SHALL be dot-only: `Date` and `DateTime` expose `.year`, `.month`, `.day`, `.offset`, `.timezone`, `.dayName`, `.dayNameShort`, `.dayOfYear`, `.weekOfYear`, `.isoWeekOfYear`, `.isoYearWeek`, `.monthName`, `.monthNameShort`, `.quarter`, `.yearQuarter`; `Time` and `DateTime` expose `.hour`, `.minute`, `.second`, `.offset`, `.timezone`. Offset values SHALL be days-time durations; timezone values SHALL be IANA names. `.weekOfYear` SHALL number Jan 1–7 as week 1 within the calendar year; `.isoYearWeek` SHALL include the ISO week-year; day/month names SHALL be English.

#### Scenario: Calendar-year and ISO week-year differ
- **WHEN** `.isoWeekOfYear` and `.isoYearWeek` are read from `date("2025-12-29")`
- **THEN** they return `1` and `"2026W1"`, respectively

#### Scenario: Zoned properties and conversions
- **WHEN** `.offset` is read from `date("2025-03-28+05:30")`, `.timezone` from `time("11:45:30[Europe/Paris]")`, and `date(datetime("2025-03-28T14:30:00"))` is evaluated
- **THEN** they produce `dtDuration("PT5H30M")`, `"Europe/Paris"`, and `date("2025-03-28")`

### Requirement: Temporal point arithmetic and membership
`Date` and `DateTime` SHALL support addition/subtraction of `DTDuration` or `YMDuration`; year/month changes SHALL clamp invalid days, leave datetime time unchanged, and preserve zones; days-time changes SHALL carry across datetime day boundaries while a bare date uses only whole days. `Time` SHALL add/subtract only `DTDuration` modulo one day. Applying a `DTDuration` to `Time` or `DateTime` SHALL fail if it is not an integral number of nanoseconds; it SHALL NOT silently discard a sub-nanosecond remainder. Subtracting two dates or datetimes SHALL yield a signed `DTDuration`; two naive dates use calendar whole days, two zoned dates use the gap between midnight in each operand's own zone. Temporal points SHALL support same-kind `<`, `<=`, `>`, `>=`, `==`, `!=`, inclusive `between`, and existing list/range `in`; date/datetime points SHALL also support calendar `in`. Range endpoint inclusivity, same-kind rules, and empty/reversed behavior SHALL remain unchanged.

#### Scenario: Clamp, wrap, and zoned gap
- **WHEN** `date("2025-01-31") + ymDuration("P1M")`, `time("23:00:00") + dtDuration("PT2H")`, and `date("2025-03-28+05:30") - date("2025-03-28-05:00")` are evaluated
- **THEN** they return `date("2025-02-28")`, `time("01:00:00")`, and `dtDuration("-PT10H30M")`

#### Scenario: Invalid arithmetic and mixed zone kinds
- **WHEN** a time is added to a years-months duration, a naive datetime is compared with a zoned datetime, or a date is compared with a datetime without conversion
- **THEN** validation or evaluation reports an error rather than inventing a result

### Requirement: Days-time durations use finite-decimal elapsed seconds
`dtDuration(text)` SHALL parse a signed ISO-style days/time duration with D/T/H/M/S units, accepting case-insensitive unit letters and fractions on any unit, normalizing overflow, and rendering uppercase canonical text with a fraction only on the smallest emitted unit. Values SHALL use the existing `Number` (`rust_decimal`) finite-decimal precision, not binary floating point or unbounded rational precision. Representable fractional input, including fractions smaller than a nanosecond, SHALL retain its digits; values beyond the supported range/precision SHALL fail validation. Checked arithmetic SHALL fail on overflow or zero division; nonterminating division SHALL round half-even at the available `Number` precision. Canonical serialization SHALL reflect the resulting stored decimal. `.days`, `.hours`, `.minutes`, `.seconds` SHALL be normalized signed components; `.totalSeconds`, `.totalMinutes`, `.totalHours`, `.totalDays` SHALL return signed `Number` totals at the same precision (so unit conversion may round). Same-kind `+`, `-`, unary `-`, numeric `*`, `/`, `<`, `<=`, `>`, `>=`, `==`, and `!=` SHALL use stored total seconds; invalid duration syntax SHALL report errors.

#### Scenario: Parsing and totals
- **WHEN** `dtDuration("p1.5d")`, `dtDuration("PT90M")`, and `dtDuration("P2DT3H45M10S").totalSeconds` are evaluated
- **THEN** they yield `dtDuration("P1DT12H")`, `dtDuration("PT1H30M")`, and `186310`

#### Scenario: Signed precision and equivalence
- **WHEN** `dtDuration("PT0.1234567891S")`, `dtDuration("-P2DT3H45M10S").days`, and `dtDuration("PT60S") == dtDuration("PT1M")` are evaluated
- **THEN** the representable fractional seconds retain their written digits, `.days` is `-2`, and equality is `true`

### Requirement: Years-months durations use finite-decimal elapsed months
`ymDuration(text)` SHALL parse signed ISO-style Y/M durations with fractions on either unit and case-insensitive unit letters, normalize overflow into years while preserving representable fractional months, and render uppercase canonical text from the stored finite-decimal value. It SHALL use the same `Number` precision, checked arithmetic, half-even rounding for nonterminating division, and overflow/error policy as `DTDuration`. `.years` and `.months` SHALL be normalized signed components with `0 ≤ |months| < 12`; `.totalMonths` and `.totalYears` SHALL return signed `Number` totals at the same precision. Same-kind arithmetic, numeric scaling/division, and comparison/equality SHALL use stored total months. The two duration types SHALL NOT be added to one another; fractional months SHALL be representable even when applying them to a point requires an evaluation error because a fractional calendar month is ambiguous.

#### Scenario: Normalize fractional months
- **WHEN** `ymDuration("P13M")`, `ymDuration("P1.5Y")`, and `ymDuration("P1Y0.25M").months` are evaluated
- **THEN** they yield `ymDuration("P1Y1M")`, `ymDuration("P1Y6M")`, and `0.25`

#### Scenario: Same-kind operations
- **WHEN** `ymDuration("P1Y") == ymDuration("P12M")` and `ymDuration("P6M") * 3` are evaluated
- **THEN** they yield `true` and `ymDuration("P1Y6M")`

#### Scenario: Finite-precision division and conversion
- **WHEN** `dtDuration("PT1H") / 7` is evaluated, serialized, or read via `.totalSeconds`, or `ymDuration("P1Y") / 7` is read via `.totalYears`
- **THEN** the results use representable finite decimals rounded half-even at the available `Number` precision; duration text serializes the stored rounded result without suggesting infinitely many digits

#### Scenario: Sub-nanosecond input versus point arithmetic
- **WHEN** `dtDuration("PT0.1234567891S")` is parsed and then added to a `Time` or `DateTime`
- **THEN** parsing and serialization retain the finite-decimal fraction, but applying it to the point reports an evaluation error instead of truncating precision

### Requirement: Durations support rounding and difference functions
Both duration types SHALL support `abs`, `isNegative` (false at zero), and `round`, `roundUp`, `roundDown`, `roundHalfUp`, `roundHalfDown`, `roundHalfEven` with a positive same-kind duration step. Rounding SHALL return an integral multiple of the step; `round` means half-up, up means away from zero, down means toward zero, and half-even means ties to an even multiple. `dtDurationBetween(from,to)` SHALL equal `to - from`; `ymDurationBetween(from,to)` SHALL produce signed elapsed whole months; both SHALL require operands of the same kind (`Date` or `DateTime`).

#### Scenario: Rounding modes at a tie
- **WHEN** `roundHalfUp`, `roundHalfDown`, and `roundHalfEven` round `dtDuration("PT22M30S")` to `dtDuration("PT15M")`
- **THEN** they return `dtDuration("PT30M")`, `dtDuration("PT15M")`, and `dtDuration("PT30M")`, respectively

#### Scenario: Duration differences
- **WHEN** `dtDurationBetween(date("2025-01-01"), date("2025-03-28"))` and `ymDurationBetween(date("2011-12-22"), date("2013-08-24"))` are evaluated
- **THEN** they return `dtDuration("P86D")` and `ymDuration("P1Y8M")`

### Requirement: Immutable calendars can be supplied and inspected
A typed `Calendar` SHALL be accepted as an input-supplied value (with a documented serialization/host representation), but no calendar expression literal or constructor SHALL be introduced. `CalendarEntry` SHALL be the read-only type returned by entry queries, containing its value and optional name; `entryName` on an unnamed entry SHALL report an evaluation error rather than inventing a name. Entries SHALL be chronologically ordered, optionally named Date/DateTime points or ranges; one non-empty calendar SHALL have a single naive/zoned kind even when point/range and Date/DateTime kinds mix. `count`, `isEmpty`, `entries`, `names` (distinct and chronological, omitting unnamed entries), `find` (case-sensitive), `contains`, `entriesFor`, `overlaps`, `entriesIn`, `validFrom`, `validTo`, `validRange`, `entryValue`, and `entryName` SHALL provide the described queries. `point in calendar` SHALL mean `contains(calendar,point)` for Date/DateTime points. `next`/`prev` SHALL return the nth entry strictly after/before a point (default n=1, n positive): ranges use their start for next and end for prev; an active range is neither. `find` and `entriesFor` SHALL return empty lists on no match; `next`/`prev` SHALL report an evaluation error if the requested entry does not exist. Calendar equality SHALL use entry sets and validity bounds with `==`/`!=`; ordering and arithmetic SHALL be rejected.

#### Scenario: Membership and traversal
- **WHEN** an input holiday calendar includes named Good Friday (2025-04-18) and Easter Monday (2025-04-21), and `next(c, date("2025-04-01"), 2)` is evaluated
- **THEN** the Easter Monday entry is returned and `date("2025-04-18") in c` is `true`

#### Scenario: Range query and invalid traversal
- **WHEN** a calendar contains a range straddling a point, or `next(c, point, 0)` is evaluated
- **THEN** the active range is excluded from next/prev and a nonpositive `n` reports an error

### Requirement: Calendar transformations return new values
`calendarDrop(c,target[,rangeMatch="equality"])` and `calendarKeep(c,target[,rangeMatch="equality"])` SHALL respectively remove/retain matching entries without modifying `c`. A string target SHALL match a name exactly, `pattern(...)` SHALL match names by regex, a Date/DateTime/range SHALL match values, and a list SHALL match any target. For a range target, the optional named argument `rangeMatch` SHALL accept `"equality"` (default), `"entryWithin"`, `"entryEncloses"`, or `"overlap"`. `calendarMerge(calendars[,dedupeBy=...][,tiebreak=...])` SHALL return a chronologically sorted union; optional named arguments `dedupeBy="value" | "valueAndName"` and `tiebreak="first" | "name"` SHALL control deduplication and survivor selection on a value clash. These are named arguments within calls, not dictionary values or equality comparisons; unknown, duplicate, or misplaced named arguments, invalid values/regex, incompatible calendar zone kinds, and invalid target types SHALL report errors.

#### Scenario: Filter and merge with named arguments
- **WHEN** `calendarDrop(c, [date("2025-12-25")..date("2025-12-28")], rangeMatch="overlap")`, `calendarKeep(ukHolidays, [date("2025-12-25"), date("2025-12-26")])`, and `calendarMerge([england, scotland], dedupeBy="value", tiebreak="first")` are evaluated
- **THEN** the first drops entries overlapping the target range, the second contains only matching entries, and the third is a sorted deduplicated union without mutating its inputs

#### Scenario: Invalid named option
- **WHEN** `calendarDrop(c, range, rangeMatch="unknown")`, `calendarMerge([c], dedupeBy="value", dedupeBy="value")`, or `calendarDrop(c, range, unknown="overlap")` is evaluated
- **THEN** validation or evaluation reports an error rather than ignoring the option

### Requirement: Calendar-aware date operations honor holiday bounds
Date/date-time classification SHALL provide `isWeekday`, `isWeekend`, `isPublicHoliday(v,c)`, and `isBusinessDay(v[,c])`. Boundary/navigation functions SHALL provide `lastDayOfMonth`, `firstDayOfMonth`, `lastDayOfPrevMonth`, `firstDayOfNextMonth`, `firstDayOfWeekInMonth(v,dow)`, `lastDayOfWeekInMonth(v,dow)`, `nthDayOfWeekInMonth(v,n,dow)` (negative n counts backward), `nextDayOfWeek`, `prevDayOfWeek`, `nextWeekday`, `prevWeekday`, `nextBusinessDay`, and `prevBusinessDay`. Counting/arithmetic SHALL provide `addBusinessDays(v,n[,c[,strictCalendarRange]])`, `subtractBusinessDays`, inclusive order-independent `weekdaysBetween(a,b)` and `businessDaysBetween(a,b[,c[,strictCalendarRange]])`. Without `c`, business-day operations SHALL exclude weekends only. Navigation SHALL be strict; adding/subtracting zero business days SHALL return the original value. Value-returning operations on DateTime SHALL preserve its time/zone; classification/counts SHALL ignore the time. Iterating business-day functions SHALL ignore holiday information outside calendar validity by default; the trailing `strictCalendarRange` argument set to `true` with a calendar SHALL raise `bl.CalendarRangeError` when iteration crosses its validity bounds. `isBusinessDay` SHALL not accept that flag.

#### Scenario: Weekend and holidays
- **WHEN** `nextBusinessDay(date("2025-04-17"), ukHolidays)` and `addBusinessDays(date("2025-04-17"), 2, ukHolidays)` are evaluated using the documented 2025 UK holidays
- **THEN** they return `date("2025-04-22")` and `date("2025-04-23")`

#### Scenario: Leap year and strict range
- **WHEN** `lastDayOfMonth(date("2024-02-10"))` is evaluated, or `nextBusinessDay` steps beyond a supplied holiday calendar's bound with strict range enabled
- **THEN** the first yields `date("2024-02-29")` and the second raises `bl.CalendarRangeError`

### Requirement: Numeric differences and fiscal periods are available
`daysBetween(v1,v2)` SHALL return signed actual days; `monthsBetween(v1,v2[,basis])` and `yearsBetween(v1,v2[,basis])` SHALL return signed numeric differences using `"calendar"` by default or `"actual/365"`, `"actual/360"`, `"actual/actual"` (ISDA), `"30/360"` (US/NASD), `"30E/360"` (European). A DateTime overload SHALL accept optional trailing `includeTime` (default false) to include fractional sub-day time; a Date overload SHALL reject it. `financialYear(v,basis)` and `financialYearQuarter(v,basis)` SHALL label the financial year by the year it ends and number quarters from its starting boundary. The required basis SHALL be month 1–12 or `"AU"` (July 1), `"UK"` (April 6), `"US"` (October 1), or `"IN"`, `"JP"`, `"CA"`, `"NZ"` (April 1).

#### Scenario: Numeric elapsed time
- **WHEN** `daysBetween(datetime("2025-01-15T00:00:00"), datetime("2025-01-16T12:00:00"), true)` is evaluated
- **THEN** it returns `1.5`

#### Scenario: Australian financial year
- **WHEN** `financialYear(date("2024-07-01"), "AU")` and `financialYearQuarter(date("2025-01-15"), "AU")` are evaluated
- **THEN** they return `"FY2025"` and `"FY2025Q3"`

### Requirement: Zone conversion distinguishes instant from wall clock
`withOffset(time|datetime,dtDuration)` and `withTimezone(datetime,ianaName)` SHALL re-zone while preserving the instant; `withoutOffset`, `withoutTimezone`, and `withoutOffsetOrTimezone` SHALL accept Date/DateTime and strip the matching zone while preserving wall-clock components, returning the same type. Stripping an already-naive value SHALL be a no-op. Invalid IANA names, offset values, and unresolvable or ambiguous local times SHALL report an error rather than silently selecting a different instant.

#### Scenario: Rezone versus strip
- **WHEN** `withOffset(datetime("2025-03-28T14:30:00+01:00"), dtDuration("PT2H"))` and `withoutOffset(datetime("2025-03-28T14:30:00+01:00"))` are evaluated
- **THEN** they return `datetime("2025-03-28T15:30:00+02:00")` and `datetime("2025-03-28T14:30:00")`, respectively
