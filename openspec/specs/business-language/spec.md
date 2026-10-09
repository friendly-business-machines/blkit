# business-language Specification

## Purpose

Defines the initial `.bl` language capability for writing closed-world, type-safe business processes that can be validated and transpiled to Rust.

## Requirements

### Requirement: Source files declare namespace and version
A `.bl` source file SHALL declare a namespace and version before process declarations so generated processes can be identified for change management.

#### Scenario: Valid namespace and version
- **WHEN** a source file declares `namespace orders` and `version "1.0"`
- **THEN** the language front end accepts those declarations as the process identity context

#### Scenario: Missing namespace or version
- **WHEN** a source file omits either the namespace or the version declaration
- **THEN** validation fails with a diagnostic identifying the missing declaration

### Requirement: Domain types are declared in source
The language SHALL support business-domain record declarations, enum declarations, and typed list values using only `.bl` source-defined domain types and built-in types. In a project build, a declaration MAY reference domain types defined in another file with the same namespace and process version. A standalone single-file compilation SHALL continue to resolve declarations from that file alone.

#### Scenario: Valid domain declarations
- **WHEN** a source declares a record type, an enum type, and a `List<T>` field
- **THEN** validation accepts the declarations when all referenced types exist in its compilation scope

#### Scenario: Shared domain type
- **WHEN** one `.bl` file defines `Order` and another file in the same project namespace and version uses `Order` in a process signature
- **THEN** project validation accepts the type regardless of source-file order

#### Scenario: Unknown type reference
- **WHEN** a declaration references a type that is neither built in nor declared in its compilation scope
- **THEN** validation fails with a diagnostic identifying the unknown type

### Requirement: Source-defined tasks and decisions are shared within a project scope
A project build SHALL resolve peer `start_event`, `end_event`, `decision_task`, process, and domain-type declarations across `.bl` files with matching namespace and process version before validating process graphs, independently of source-file order. Declarations from another namespace or version SHALL NOT become visible implicitly; duplicate peer names within a scope SHALL be rejected. Standalone single-file compilation SHALL resolve peers from that file alone. Generic `task` and `decision` declarations SHALL NOT be accepted.

#### Scenario: Cross-file task call
- **WHEN** one file defines a typed `decision_task` and another file in the same scope has a process whose `flow` and `bind` reference it
- **THEN** validation accepts the references irrespective of source-file order

#### Scenario: Cross-file decision call
- **WHEN** a process references a decision task in another file with matching namespace and version
- **THEN** validation accepts and emits its decision evaluation

#### Scenario: Duplicate or out-of-scope declaration
- **WHEN** two files in one scope define the same peer name, or a process references a decision task only defined in another namespace or version
- **THEN** project validation rejects the duplicate or unknown reference before emitting Rust

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

### Requirement: Processes have typed input and output
A process SHALL be declared as `process <name> { ... }` without parenthesized parameters or an arrow output type. It SHALL have exactly one reachable `start_event` in this change. Its incoming JSON value SHALL be a JSON object with keys and values matching that start event's outputs; every declared start output SHALL be present, with no unknown keys. A normal result SHALL come from a referenced `end_event` whose declared input ports are bound to compatible outputs. All reachable normal end events of the same process SHALL declare the same result shape and types. One normal end input SHALL produce that input's value directly; multiple inputs SHALL produce a JSON object keyed by port names. Exceptional terminal routes SHALL NOT require normal end inputs.

#### Scenario: Valid process signature
- **WHEN** `start_event start { output amount: Number; }`, `end_event done { input result: Number; }`, and a process connect the ports through a decision task
- **THEN** start input `{"amount": 3}` validates and a normal `done` produces its bound numeric value

#### Scenario: Invalid process signature type
- **WHEN** start input is a scalar, lacks a declared port, contains an unknown key, or has an incompatible value type
- **THEN** validation rejects the start request before an instance begins

#### Scenario: Exceptional route
- **WHEN** a route reaches a named error, cancel, or terminate event rather than a normal end event
- **THEN** that outcome does not require an end-event result value

#### Scenario: Ambiguous process shape
- **WHEN** a process has two reachable start events or two normal end events with incompatible input-port shapes
- **THEN** validation fails before generation

### Requirement: Rust generation follows successful validation
The compiler SHALL generate Rust output only after parsing, name validation, graph validation, and type validation have succeeded.

#### Scenario: Valid source generates Rust
- **WHEN** an input `.bl` file passes validation
- **THEN** the compiler produces Rust code representing the declared types, lists, tasks, process graphs, and retry policies

#### Scenario: Invalid source does not generate Rust
- **WHEN** an input `.bl` file fails parsing or validation
- **THEN** the compiler produces diagnostics and does not emit Rust code for that file

#### Scenario: Generated behavior is callable
- **WHEN** valid `.bl` declares a graph that branches on a numeric field and reaches a typed normal end node
- **THEN** the generated Rust compiles and the registered process can be invoked to return the expected result for each branch

### Requirement: Processes declare their task graph in `.bl`
The language SHALL support process graphs assembled from named peer start, decision-task, gateway, join, wait, subprocess, and terminal nodes connected by explicit `flow` and typed `bind` statements. Graph nodes SHALL use their specific kind as the declaration keyword and enclose their properties in `{ ... }`; only `decision_task` SHALL be an authored task kind in this change. The compiler SHALL reject unknown or duplicate nodes, unreachable nodes or terminals, dead-end routes, invalid gateway split/join pairing, and unavailable/mistyped bindings. Cycles SHALL require a positive process deadline; backward flow SHALL NOT enter a start event or cause unmatched joins. An old `node <name> = <kind>` declaration, `link`, generic `task`, business-rule call, process signature, implicit sequencing, or process-level `return` SHALL NOT be accepted.

#### Scenario: Valid source-defined process map
- **WHEN** a process connects a start event, a decision task, and an end event with typed flow and bindings
- **THEN** validation accepts the graph without redeclaring any of those nodes inside the process

#### Scenario: Invalid graph link
- **WHEN** flow references a missing peer, strands a route, or bind references an unavailable output
- **THEN** validation fails before emitting Rust

#### Scenario: Bounded cyclic graph
- **WHEN** a graph contains a reachable backward flow, a valid exit, and a positive process deadline
- **THEN** repeated node visits are accepted

#### Scenario: Cycle without deadline
- **WHEN** a graph has a directed cycle without a positive process deadline
- **THEN** validation rejects it before Rust generation

#### Scenario: Existing process stays valid
- **WHEN** an existing process is migrated to braced peers, typed `flow` and `bind`, and supported decision-task nodes
- **THEN** its business behavior can still validate and execute, while its unmigrated legacy syntax is rejected

### Requirement: Gateway conditions and joins are typed
The language SHALL support named, explicitly connected AND, OR, and XOR split and join gateways in a `.bl` process map. Each gateway SHALL use a kind-specific braced declaration; `flow` statements SHALL carry typed branch conditions, ordered fallbacks, and AND branch labels as applicable; typed `bind` statements SHALL carry required values separately. AND splits SHALL activate all outgoing branches; XOR splits SHALL activate the first matching branch in outgoing-flow declaration order; OR splits SHALL activate every matching branch. Conditions SHALL be `Bool` expressions that can reference start-event outputs and upstream task outputs available on every route to that gateway. XOR and OR splits SHALL provide a fallback when no condition matches. Joins SHALL account for the branches activated for that instance: AND waits for all incoming branches and combines their results into a declared record, XOR accepts the selected branch's result of a common type, and OR waits for every selected branch and produces a `List<T>` of compatible branch results in split-flow declaration order. Every route to a normal end event SHALL bind compatible values to its required input ports.

#### Scenario: Conditions use task output and input
- **WHEN** an XOR gateway's `flow` condition uses a completed decision-task output and a start-event output
- **THEN** validation accepts its `Bool` condition and selects exactly one branch

#### Scenario: Inclusive parallel routing
- **WHEN** two OR conditions match
- **THEN** both branches activate and the OR join waits for both, but not for inactive branches

#### Scenario: Invalid reference or route
- **WHEN** a condition uses an unavailable output or a normal end-event input cannot be bound on a reachable route
- **THEN** validation fails before generation

#### Scenario: Multiple XOR conditions match
- **WHEN** multiple XOR conditions match
- **THEN** the first matching outgoing flow in declaration order is selected

#### Scenario: Inclusive join output
- **WHEN** an OR gateway selects two branches of the same declared output type
- **THEN** its join produces a typed list in split-flow declaration order

### Requirement: Compiler emits the validated process graph
After successful validation, the compiler SHALL emit Rust representing `.bl`-defined tasks, named nodes, explicit typed links, gateway routes, terminal events, retry policy, and process identity for runtime registration. The runtime SHALL NOT need to reconstruct or infer a process map from individual compiled functions. The generated business logic SHALL be linked into a binary at build time rather than compiled by a running worker.

#### Scenario: Compiled graph is executable
- **WHEN** a valid `.bl` graph with a gateway, at least two task nodes, and a named terminal is compiled into a worker binary
- **THEN** the runtime can register that generated graph and execute the intended route

#### Scenario: Invalid graph produces no Rust
- **WHEN** graph validation fails
- **THEN** the compiler emits diagnostics rather than Rust output for that file

#### Scenario: Worker advertises compiled identities
- **WHEN** a worker starts with generated process definitions linked into its binary
- **THEN** it can advertise those definitions' namespace, version, and process names without compiling `.bl` at runtime

### Requirement: Processes declare bounded retry policy in source
A `.bl` process SHALL optionally declare `max retries` (additional attempts after the initial execution), `retry for` (a duration measured from the first execution failure), `retry delay` (minimum wait before the first retry), and exponential backoff. Absence of a retry declaration SHALL mean no retries. Both the attempt limit and time window SHALL bound retries; later delays SHALL grow exponentially from the declared minimum. The compiler SHALL reject invalid or unbounded declarations.

#### Scenario: Bounded policy is compiled
- **WHEN** a process declares three additional retries, a ten-minute retry window, a one-second minimum delay, and exponential backoff
- **THEN** validation accepts the policy and the compiled process definition carries all four parameters

#### Scenario: No retry declaration
- **WHEN** a process declares no retry policy
- **THEN** its compiled definition permits no additional attempts

#### Scenario: Invalid policy
- **WHEN** a retry declaration supplies a negative limit, nonpositive duration, or missing required parameter
- **THEN** validation fails before Rust is emitted

### Requirement: Process graphs may call a typed subprocess
A peer subprocess node SHALL invoke a process in the caller's namespace and version. It SHALL declare typed input and normal-output ports and a braced body; its incoming values SHALL be supplied by `bind`, not by positional call arguments. A normal child result SHALL be available only on its success route, and its port bindings SHALL be type checked. Standalone compilation SHALL resolve its child from the file; project builds SHALL resolve processes across the same scope. Unknown, out-of-scope, incompatible, or recursive process calls SHALL fail before Rust generation.

#### Scenario: Typed successful call
- **WHEN** a parent flows through a subprocess peer, binds its input from a compatible output, and binds its normal result to a successor
- **THEN** validation succeeds and the generated graph executes the child

#### Scenario: Invalid call
- **WHEN** the child is missing, a bound input is incompatible, or a success-only output is used on an exceptional route
- **THEN** validation fails before emitting Rust

### Requirement: Subprocess outcomes have explicit per-kind routes
A subprocess node SHALL have exactly one ordinary success `flow` and MAY have at most one exceptional outgoing `flow` each with `on error`, `on cancel`, or `on terminate`. Exceptional flows SHALL carry no child output; ordinary flows SHALL run only after a normal child end event. Duplicate or inappropriate outcome flows SHALL fail validation. Unhandled child outcomes SHALL propagate to the parent; caught child outcomes SHALL permit parent continuation. A child error SHALL retain its terminal name when propagated. External parent cancellation and parent deadline expiry SHALL NOT be catchable by child outcome flows.

#### Scenario: Catch only modeled error
- **WHEN** a child reaches a named error event and its parent has `flow called -> handler on error;`
- **THEN** only that handler route activates without a normal child output

#### Scenario: Unhandled child terminal
- **WHEN** a child reaches terminate and its parent has no `on terminate` flow
- **THEN** the parent terminates rather than taking its normal route

#### Scenario: Invalid exceptional routing
- **WHEN** a non-subprocess node declares `on error` or a subprocess declares two `on cancel` flows
- **THEN** validation rejects the graph

### Requirement: Subprocess call graphs cannot recurse
The compiler SHALL reject direct and indirect cycles between process calls in the same namespace/version while permitting acyclic nested calls, irrespective of process graph deadlines.

#### Scenario: Nested nonrecursive call
- **WHEN** A calls B and B calls C without a call cycle
- **THEN** validation accepts their call graph when their types and routes are valid

#### Scenario: Recursive call cycle
- **WHEN** A calls B and B calls A, or A calls itself
- **THEN** validation rejects the cycle before Rust generation

### Requirement: Process graphs end in named terminal nodes
A process graph SHALL connect through `flow` to named, braced terminal-event peers of kind `end_event`, `error_event`, `cancel_event`, or `terminate_event`. A normal end event SHALL require bound, typed input ports; a named error event SHALL identify the business error and require no normal output; cancel and terminate events SHALL require no normal output. Top-level terminals SHALL apply to the instance; child terminals SHALL end only the child scope before handling or propagation. No compensation is implied.

#### Scenario: Normal typed end
- **WHEN** a reachable `end_event done { input result: Number; }` receives a compatible bound result
- **THEN** validation accepts the output

#### Scenario: Modeled business error
- **WHEN** a route flows to a named `error_event` peer
- **THEN** validation accepts it without a normal result on that route

#### Scenario: Incompatible normal end
- **WHEN** a `Bool` output is bound to a `Number` end-event input
- **THEN** validation rejects the binding

#### Scenario: Child terminal is scoped
- **WHEN** a subprocess reaches `cancel_event` and its caller handles `on cancel`
- **THEN** the child scope ends and the parent follows its handler

### Requirement: Intermediate waits and process deadlines are source-configurable
A process SHALL support pause-for with a positive duration literal and pause-until with a `DateTime` value supplied by a typed binding. Waits SHALL use kind-specific peer declarations with braced bodies, and the process deadline policy SHALL be a semicolon-terminated statement in its braced process body selecting a positive duration since queued or first claimed. The compiler SHALL reject invalid duration/type/origin; `timeout` remains reserved as a terminal name. Waits SHALL retain their existing durable runtime behavior.

#### Scenario: Dynamic pause until
- **WHEN** a pause-until peer receives a `DateTime` input through `bind`
- **THEN** validation accepts the wait

#### Scenario: Wrong wait type
- **WHEN** a pause-until peer receives a `Number` binding
- **THEN** validation rejects it before generation

### Requirement: Task iteration is explicit and bounded
A `decision_task` used in a process SHALL support a conditional loop checked before or after each invocation and SHALL declare at least one positive maximum iteration count or elapsed duration. It SHALL support sequential or parallel multi-instance execution over a typed `List<T>`, passing each item through the named input port and yielding ordered typed results. Conditions SHALL be `Bool` expressions with unambiguous availability; typed `bind` statements SHALL supply inputs and initial results where needed. Invalid bounds, element types, or repeated-output references SHALL fail validation. Old generic `node ... = task ... repeat_*` syntax SHALL be rejected.

#### Scenario: Post-check loop
- **WHEN** a bounded post-check decision task's condition is false after its first invocation
- **THEN** it runs once and exposes its output downstream

#### Scenario: Pre-check loop
- **WHEN** a pre-check condition is initially false with a type-correct initial result
- **THEN** the task is not invoked and the initial result is available downstream

#### Scenario: Sequential and parallel instances
- **WHEN** a three-element list enters a sequential or parallel decision task
- **THEN** it runs once per item and returns outputs in input order

### Requirement: Same-type range expressions have explicit boundaries
`.bl` SHALL accept interval expressions `[a..b]`, `(a..b)`, `[a..b)`, and `(a..b]`, where finite endpoints have the same type from `Number`, `Date`, `DateTime`, or `Time`. At least one finite bound SHALL establish the range's type; if both are `null`, a type SHALL be supplied by context (for example, the value tested for membership). `[`/`]` SHALL include the corresponding endpoint, and `(`/`)` SHALL exclude it. The literal `null` SHALL denote an unbounded endpoint only inside a range expression; it SHALL NOT introduce a nullable value type. An omitted bound SHALL have no inclusion semantics. Both bounds MAY be unbounded. Statically reversed finite bounds SHALL fail compilation. Reversed finite bounds discovered only at evaluation SHALL describe an empty range rather than silently swapping bounds. Equal finite bounds SHALL describe a singleton only when both ends are inclusive; otherwise they SHALL describe an empty range.

#### Scenario: Boundary combinations
- **WHEN** `.bl` evaluates `1 in [1..5]`, `1 in (1..5)`, `5 in [1..5)`, and `5 in (1..5]`
- **THEN** the results SHALL be `true`, `false`, `false`, and `true`, respectively

#### Scenario: Unbounded ends
- **WHEN** `.bl` evaluates `18 in [18..null)`, `100 in [18..null)`, a negative `Number` input in `(null..0)`, and `0 in (null..0)`
- **THEN** the results SHALL be `true`, `true`, `true`, and `false`, respectively

#### Scenario: Equal and inverted endpoints
- **WHEN** `.bl` evaluates `1 in [1..1]` and `1 in (1..1]`, or compiles `[5..1]`
- **THEN** the membership results SHALL be `true` and `false`, and the inverted literal SHALL be rejected with a diagnostic

#### Scenario: Dynamically reversed range
- **WHEN** a range formed from two runtime `Number` inputs has a lower bound greater than its upper bound
- **THEN** it SHALL be empty: membership and overlap SHALL return `false`

#### Scenario: Invalid bound type or null outside a range
- **WHEN** a range has a `String` endpoint, mixes `Date` and `DateTime`, both bounds and type context are absent, or `null` is used as a general expression
- **THEN** validation SHALL fail before Rust is emitted

### Requirement: Range membership, equality, and interval relations
The `in` operator SHALL test whether a scalar belongs to a range of the same type. `x between a and b` SHALL be equivalent to `x in [a..b]` with finite same-type bounds. Ranges SHALL support `==` and `!=` based on their effective endpoints and inclusion, not textual formatting; no `=` equality operator is added. The functions `before`, `after`, `meets`, `metBy`, `overlaps`, `overlapsBefore`, `overlapsAfter`, and `coincides` SHALL accept two ranges of the same type and return `Bool`; the numeric point overloads for `before`, `after`, `meets`, and `metBy` are defined below. `includes(range, value)` and `during(value, range)` SHALL test membership. `starts(value, range)`/`startedBy(range, value)` SHALL test the included finite start point; `finishes(value, range)`/`finishedBy(range, value)` SHALL test the included finite end point. Incompatible arguments SHALL fail type validation before generation.

`before(A,B)` SHALL mean A's upper bound is strictly below B's lower bound; `after` SHALL be its converse. `meets(A,B)` SHALL mean A's finite upper bound equals B's finite lower bound, irrespective of endpoint inclusion; `metBy` SHALL be its converse. `overlaps(A,B)` SHALL mean the ranges have at least one value in common. `overlapsBefore(A,B)` SHALL mean A starts before B, B starts strictly before A ends, and A ends strictly before B ends; `overlapsAfter` SHALL be the converse. For these ordering relations, unbounded ends compare below/above all finite bounds as appropriate. `coincides` SHALL mean range equality. Empty ranges SHALL NOT overlap any range.

In addition, `before(Number, Range<Number>)` SHALL test whether the point is strictly below a finite lower endpoint; `before(Range<Number>, Number)` SHALL test whether a finite upper endpoint is strictly below the point; `after` SHALL be the converse in each argument order. `meets(Number, Range<Number>)` SHALL test equality to the finite lower endpoint and `meets(Range<Number>, Number)` SHALL test equality to the finite upper endpoint, irrespective of inclusion; `metBy` SHALL be the converse. Existing `during`, `includes`, `starts`, `startedBy`, `finishes`, and `finishedBy` SHALL continue to accept Number points and Number ranges, respecting included endpoints. No point overload is added to the two-range `overlaps`, `overlapsBefore`, `overlapsAfter`, or `coincides` relations. A missing required finite endpoint SHALL make these point `before`/`after`/`meets`/`metBy` checks false.

#### Scenario: Scalar membership and between
- **WHEN** `.bl` evaluates `25 in [18..65]`, `3 in [1..10]`, `10 in [1..10)`, and `5 between 1 and 10`
- **THEN** the results SHALL be `true`, `true`, `false`, and `true`, respectively

#### Scenario: Date, DateTime, and Time membership
- **WHEN** `.bl` evaluates `date("2026-10-02") in [date("2026-10-01")..date("2026-10-03")]`, `dateTime("2026-10-02T08:30:00Z") in [dateTime("2026-10-02T09:00:00+01:00")..null)`, and `time("12:00:00") in (time("09:00:00")..time("17:00:00"))`
- **THEN** all three results SHALL be `true`; the `DateTime` comparison SHALL use instants

#### Scenario: Range equality respects endpoint inclusion
- **WHEN** `.bl` evaluates `[1..5] == [1..5]` and `[1..5] == [1..5)`
- **THEN** the results SHALL be `true` and `false`, respectively

#### Scenario: Ordered relations and shared endpoint
- **WHEN** `.bl` evaluates `before([1..5], [6..10])`, `after([6..10], [1..5])`, `meets([1..5], [5..10])`, and `metBy([5..10], [1..5])`
- **THEN** all four results SHALL be `true`

#### Scenario: Overlap relations
- **WHEN** `.bl` evaluates `overlaps([5..10], [1..6])`, `overlapsBefore([1..5], [4..10])`, and `overlapsAfter([4..10], [1..5])`
- **THEN** all three results SHALL be `true`

#### Scenario: Point relations and coincidence
- **WHEN** `.bl` evaluates `includes([1..10], 5)`, `during(5, [1..10])`, `starts(1, [1..5])`, `startedBy([1..5], 1)`, `finishes(5, [1..5])`, `finishedBy([1..5], 5)`, and `coincides([1..5], [1..5])`
- **THEN** all seven results SHALL be `true`

#### Scenario: Open and unbounded point boundaries
- **WHEN** `.bl` evaluates `starts(1, (1..5])`, `finishes(5, [1..5))`, or `starts(1, (null..5])`
- **THEN** each result SHALL be `false`

#### Scenario: Numeric points before, after, and meeting intervals
- **WHEN** `.bl` evaluates `before(0, [1..5])`, `after(6, [1..5])`, `before([1..5], 6)`, `meets(1, (1..5])`, `metBy([1..5], 1)`, and `before(0, (null..5])`
- **THEN** the first five results SHALL be `true` and the last SHALL be `false`

### Requirement: String operators are typed and preserve existing equality and range behavior
`.bl` expressions SHALL support `String + String -> String` concatenation and `String in List<String> -> Bool` literal, case-sensitive membership in tasks, process expressions, and decision models. Existing `String` `==` and `!=` SHALL remain case-sensitive, `=` SHALL NOT become an equality operator, and `x in range` SHALL retain its previous meaning. Incompatible types SHALL be rejected before code generation.

#### Scenario: Compose and compare strings
- **WHEN** expressions evaluate `"foo" + "bar"`, `"a" == "A"`, `"a" != "A"`, and `"active" in ["active", "pending"]`
- **THEN** their values SHALL be `"foobar"`, `false`, `true`, and `true`

#### Scenario: Type errors and unsupported equality alias
- **WHEN** an expression uses `"a" + 1`, `"a" in [1, 2]`, or `"a" = "A"`
- **THEN** validation SHALL reject it before generating Rust

### Requirement: String conversion, joining, and literal transformations are typed
The language SHALL provide `string(from)` for `String`, `Number`, `Bool`, `Date`, `Time`, and `DateTime`, producing text in each type's existing human-readable serialized form (Boolean `true`/`false`, decimal number text, and the documented temporal formats); it SHALL reject lists and records. `stringJoin(List<String>, String) -> String` SHALL place the separator between elements, returning `""` for an empty list. The following SHALL accept a `String` and return a `String`: `upperCase`, `lowerCase`, `trim`, `trimLeading`, `trimTrailing`, and `reverse`. Case changes SHALL use Unicode casing, trims SHALL remove Unicode whitespace only at the indicated edge(s), and reverse SHALL preserve each visible character's internal code-point order.

#### Scenario: Conversion and join
- **WHEN** `.bl` evaluates `"order-" + string(123)`, `stringJoin(["a", "b"], ", ")`, and `stringJoin([], ",")`
- **THEN** it SHALL produce `"order-123"`, `"a, b"`, and `""`

#### Scenario: Transformations use Unicode text
- **WHEN** `trim(" é ")`, `upperCase("é")`, and `reverse("éx")` are evaluated
- **THEN** they SHALL produce `"é"`, `"É"`, and `"xé"`, respectively, preserving the combining accent with `e`

### Requirement: String positions count visible Unicode characters
`stringLength(String) -> Number`, `substring(String, Number[, Number]) -> String`, `charAt(String, Number) -> String`, and `indexOf(String, String) -> Number` SHALL count Unicode extended grapheme clusters rather than UTF-8 bytes or Unicode scalar values. Position arguments SHALL be integral and one-based; negative positions SHALL count from the right (`-1` is the last character), and zero or positions outside the string SHALL produce evaluation errors. `substring`'s optional length SHALL be a nonnegative integer number of visible characters, default to the remainder of the string, and truncate at the end. `indexOf` SHALL return the first one-based matching position at a grapheme boundary, or `0` when no match exists; searching for `""` SHALL return `1`.

#### Scenario: One-based and right-relative positions
- **WHEN** a task evaluates `stringLength("éx")`, `charAt("éx", 1)`, `charAt("éx", -1)`, `substring("éx", -2, 1)`, and `indexOf("éx", "x")`
- **THEN** it SHALL return `2`, `"é"`, `"x"`, `"é"`, and `2`, respectively

#### Scenario: Missing match and invalid position
- **WHEN** `indexOf("abc", "z")` is evaluated, or `charAt("abc", 0)`, `substring("abc", 4)`, or `substring("abc", 1, -1)` is evaluated
- **THEN** the first SHALL return `0`, and each remaining expression SHALL report an evaluation error

### Requirement: Literal searching and splitting are distinct from regex operations
`contains(String, String)`, `startsWith(String, String)`, `endsWith(String, String)`, `isBlank(String)`, and `isEmpty(String)` SHALL return `Bool`. `substringBefore(String, String)` and `substringAfter(String, String)` SHALL return the text before/after the first literal match, or `""` when no match exists. `isEmpty` SHALL test exact emptiness and `isBlank` SHALL test whether the text contains only Unicode whitespace. `split(String, String)` and `split(String, List<String>)` SHALL return `List<String>`, splitting on literal, case-sensitive delimiter text, preserving leading, adjacent, and trailing empty fields; for multiple delimiters the earliest match SHALL win and input list order SHALL break ties. Empty delimiter strings or an empty delimiter list SHALL cause an evaluation error.

#### Scenario: Literal search and blanks
- **WHEN** `.bl` evaluates `contains("abc", "b")`, `substringBefore("a:b", ":")`, `substringAfter("a:b", "x")`, `isBlank(" ")`, and `isEmpty(" ")`
- **THEN** it SHALL return `true`, `"a"`, `""`, `true`, and `false`

#### Scenario: Split using one or many delimiters
- **WHEN** `.bl` evaluates `split("a,b,", ",")` and `split("a,b;c", [",", ";"])`
- **THEN** it SHALL return `["a", "b", ""]` and `["a", "b", "c"]`

### Requirement: Regex functions support flags, errors, and structured extraction
`matches(String, String[, String]) -> Bool` SHALL search for a regex match anywhere in the input. `replace(String, String, String[, String]) -> String` SHALL replace all regex matches, accepting `$1`, `$2`, and so on to interpolate captured groups. `extract(String, String[, String]) -> List<List<String>>` SHALL return matches in text order, each inner list containing its participating captured groups in group order; if the pattern has no capture groups, each inner list SHALL contain the full match. Optional groups that did not participate SHALL be omitted; no match SHALL return `[]`. The optional flags string SHALL allow only `i` (case insensitive), `m` (multiline), and `s` (dot matches newline); absent flags SHALL use none. Pattern text inside `.bl` quotes SHALL follow existing literal rules: backslashes are passed through unchanged rather than interpreted as string escapes. Invalid constant regex patterns/flags SHALL fail validation; invalid runtime-provided patterns/flags SHALL cause an evaluation error rather than a default value or a panic. Existing decision-table `column matches (test, ...)` syntax SHALL remain a distinct unary-test construct.

#### Scenario: Regex search, replacement, and extraction
- **WHEN** `.bl` evaluates `matches("abc", "b")`, `replace("order-123, order-456", "order-(\d+)", "item-$1")`, and `extract("ab a", "(a)(b)?")`
- **THEN** it SHALL return `true`, `"item-123, item-456"`, and `[["a", "b"], ["a"]]`

#### Scenario: Flags and invalid patterns
- **WHEN** `matches("ABC", "abc", "i")` is evaluated, or a malformed pattern/unknown flag is supplied from an input `String`
- **THEN** the first SHALL return `true` and malformed runtime inputs SHALL report an evaluation error; malformed literal patterns/flags SHALL be rejected at compilation

### Requirement: String padding and repetition have bounded integer arguments
`padLeading(String, Number[, String])` and `padTrailing(String, Number[, String])` SHALL pad to the requested total visible-character length, without truncating text already at or above that length; omitted pad text SHALL mean one space. Pad text SHALL be exactly one visible character. `repeat(String, Number) -> String` SHALL repeat the input the requested number of times, returning `""` for zero. Length and repeat arguments SHALL be nonnegative integral `Number` values; non-integral or negative inputs, empty/multi-character pad text, and resource-size overflow SHALL cause evaluation errors rather than panics.

#### Scenario: Padding and repetition
- **WHEN** `.bl` evaluates `padLeading("a", 3, "é")`, `padTrailing("abc", 2)`, and `repeat("ab", 2)`
- **THEN** it SHALL return `"ééa"`, `"abc"`, and `"abab"`

#### Scenario: Invalid padding or repeat
- **WHEN** `.bl` evaluates `padLeading("a", 3, "xy")` or `repeat("x", -1)`
- **THEN** evaluation SHALL report an error

### Requirement: Fallible string expressions report execution errors through generated calls
Named, typed `.bl` output port declarations SHALL continue to describe their normal types. Generated Rust calls containing runtime-fallible string expressions MAY return `Result<Output, String>` instead of plain `Output`; callers SHALL receive an execution error for invalid runtime string operations instead of a silent fallback or process panic. Valid supported `.bl` programs without fallible expressions SHALL retain their evaluation behavior.

#### Scenario: Dynamic invalid regex in a graph
- **WHEN** a decision task evaluates `matches(input, pattern)` with an invalid runtime `pattern`
- **THEN** its execution reports an error rather than returning `false`, `[]`, or crashing the process

### Requirement: Number literals and arithmetic are decimal expressions
`.bl` SHALL accept signed integers, fractional decimal literals, and decimal scientific notation (for example `42`, `-5`, `3.14`, `1500.50`, `1.5e3`, `1.5e-3`) as `Number`. For representable results, `+`, binary `-`, unary `-`, `*`, `/`, and `**` SHALL operate on `Number` without binary floating-point artifacts. Exponentiation SHALL bind tighter than unary negation, which SHALL bind tighter than multiplication/division, which SHALL bind tighter than addition/subtraction; `**` SHALL be right-associative. Numeric `==`, `!=`, `<`, `<=`, `>`, and `>=` SHALL compare values rather than source spelling; `=` SHALL remain invalid. Decimal output through `string(Number)` SHALL omit redundant fractional trailing zeroes and an unnecessary decimal point. Results requiring more precision than `Number` supports SHALL be rounded to representable decimal precision; unrepresentable inputs/results, division by zero, and invalid exponent domains SHALL report validation errors for invalid constants or evaluation errors for runtime inputs, never a panic or silent wrap.

#### Scenario: Exact decimal and normalized output
- **WHEN** `.bl` evaluates `0.1 + 0.2`, `10 / 4`, `3.0 == 3.00`, `2 ** 8`, `9 ** 0.5`, `1.5e3`, and `string(1500.50)`
- **THEN** the results SHALL be `0.3`, `2.5`, `true`, `256`, `3`, `1500`, and `"1500.5"`, respectively

#### Scenario: Arithmetic precedence and signed values
- **WHEN** `.bl` evaluates `10 - 4 * 2`, `-(7)`, `-2 ** 2`, and `2 ** 3 ** 2`
- **THEN** the results SHALL be `2`, `-7`, `-4`, and `512`, respectively

#### Scenario: Invalid calculations and equality alias
- **WHEN** `.bl` evaluates `1 / 0` or an unrepresentable result, or source contains `3.0 = 3.00`
- **THEN** arithmetic SHALL report a calculation error and the `=` expression SHALL fail validation before generation

### Requirement: Number rounding supports decimal scales and explicit tie modes
`round(n, scale)` SHALL alias `roundHalfUp(n, scale)`. `roundUp` SHALL round away from zero; `roundDown` SHALL round toward zero; `roundHalfUp` and `roundHalfDown` SHALL round nearest with exact halfway ties away from and toward zero, respectively; `roundHalfEven` SHALL resolve halfway ties to the even neighbor. `floor(n[, scale])` SHALL round toward negative infinity and `ceiling(n[, scale])` toward positive infinity; omitted scale SHALL mean zero. Scale SHALL be an integral `Number` specifying decimal places, including negative scales for tens/hundreds. Invalid or unrepresentable scales SHALL produce evaluation errors.

#### Scenario: Rounding and ties
- **WHEN** `.bl` evaluates `round(2.345, 2)`, `roundUp(5.1, 0)`, `roundDown(5.9, 0)`, `roundHalfUp(-5.5, 0)`, `roundHalfDown(5.5, 0)`, and `roundHalfEven(2.5, 0)`
- **THEN** the results SHALL be `2.35`, `6`, `5`, `-6`, `5`, and `2`, respectively

#### Scenario: Floor, ceiling, and negative scale
- **WHEN** `.bl` evaluates `floor(-1.56, 1)`, `ceiling(-1.56, 1)`, `floor(1.9)`, and `round(1250, -2)`
- **THEN** the results SHALL be `-1.6`, `-1.5`, `1`, and `1300`, respectively

### Requirement: Number math and predicates are typed
`abs(n)`, `sqrt(n)`, `exp(n)`, `ln(n)`, `log(n[, base])`, and `clamp(n, min, max)` SHALL accept `Number` and return `Number`; omitted `log` base SHALL be 10. `modulo(dividend, divisor)` SHALL use floor-division remainder with the sign of a nonzero divisor. `odd(n)` and `even(n)` SHALL require integral `Number` and return `Bool`; `isPositive(n)`, `isNegative(n)`, and `isZero(n)` SHALL return `Bool`, with zero neither positive nor negative. `clamp` SHALL require `min <= max` and return the nearest inclusive bound or `n` when in range. `sqrt` SHALL reject negative inputs, `ln`/`log` SHALL require positive arguments and a positive base other than 1, and zero divisors, invalid domains, or values outside representable precision/range SHALL produce evaluation errors. Inexact transcendental results SHALL be decimal approximations; exact representable cases SHALL return the exact result.

#### Scenario: Math and clamp
- **WHEN** `.bl` evaluates `abs(-10)`, `modulo(-10, 3)`, `sqrt(16)`, `log(100)`, `log(8, 2)`, `clamp(150, 0, 100)`, and `exp(1)`
- **THEN** the results SHALL be `10`, `2`, `4`, `2`, `3`, `100`, and a decimal approximation of Euler's number, respectively

#### Scenario: Predicates and invalid inputs
- **WHEN** `.bl` evaluates `odd(5)`, `even(2)`, `isPositive(5)`, `isNegative(-3)`, and `isZero(0)`, or evaluates `odd(1.5)`, `sqrt(-1)`, `log(8, 1)`, or `clamp(1, 5, 0)`
- **THEN** the first five results SHALL be `true`; each remaining call SHALL report an evaluation error

### Requirement: Number aggregates consume lists of numbers
`min`, `max`, `sum`, `mean`, `median`, `product`, `stddev`, and `mode` SHALL each take exactly one `List<Number>` and return a `Number`. `median` SHALL average the middle pair for an even-sized list. `stddev` SHALL calculate the sample standard deviation using denominator `N - 1`; it SHALL require at least two values. `mode` SHALL return the most frequent value; if frequencies tie, it SHALL return the numerically smallest tied value. For an empty list `sum` SHALL return zero and `product` SHALL return one; `min`, `max`, `mean`, `median`, and `mode` SHALL report evaluation errors. Decimal results SHALL follow `Number` precision and overflow/error rules.

#### Scenario: Numeric aggregation
- **WHEN** `.bl` evaluates `min([3, 1, 2])`, `max([3, 1, 2])`, `sum([1, 2, 3])`, `mean([1, 2, 3])`, `median([1, 2, 3, 4])`, `product([2, 3, 4])`, `stddev([1, 2, 3])`, and `mode([3, 2, 3, 2])`
- **THEN** the results SHALL be `1`, `3`, `6`, `2`, `2.5`, `24`, `1`, and `2`, respectively

#### Scenario: Empty and undersized lists
- **WHEN** `.bl` evaluates `sum([])`, `product([])`, `mean([])`, or `stddev([5])`
- **THEN** the first two results SHALL be `0` and `1`, and the latter two SHALL report evaluation errors

### Requirement: Number text conversion accepts explicit separators
`number(text)` SHALL parse a `String` containing a signed decimal number with `.` as its decimal separator and no grouping separator, returning `Number`. `number(text, groupingSeparator, decimalSeparator)` SHALL parse a `String` with the supplied distinct, single-character grouping and decimal separators (for example `number("1.500,50", ".", ",")`). Grouping, if present, SHALL divide the integer portion into groups of three digits except for a leading group of one to three digits. Parsing SHALL reject misplaced separators, empty/non-numeric text, unsupported separators, and unrepresentable values; a constant invalid string SHALL fail validation and a runtime string SHALL report an evaluation error.

#### Scenario: Parse default and localized decimals
- **WHEN** `.bl` evaluates `number("1500.50")` and `number("1.500,50", ".", ",")`
- **THEN** both SHALL equal `1500.5`

#### Scenario: Invalid grouping and nonnumeric text
- **WHEN** `.bl` evaluates `number("12.34,50", ".", ",")` or receives `"nope"` as runtime input to `number`
- **THEN** each SHALL fail with a diagnostic or evaluation error as appropriate, rather than returning zero

### Requirement: Runtime-fallible numeric expressions propagate errors
A `.bl` expression involving a numeric operation that can fail for runtime values SHALL report an execution error to its caller, including from decision tasks, decision tables, process conditions, and generated standalone or project builds; it SHALL NOT panic or substitute a plausible result. Named output ports SHALL retain their declared `Number` or `Bool` types. Existing unaffected expressions SHALL preserve their behavior.

#### Scenario: Dynamic division error in a task
- **WHEN** a compiled decision task divides a constant by a runtime `Number` input of zero
- **THEN** the invocation SHALL return an execution error rather than a process panic or a numeric result

### Requirement: Declarations and statements have explicit delimiters
A `.bl` source SHALL end every statement, including namespace/version, field, variant, port, expression, rule, `flow`, and `bind` statements, with `;`. A declaration or control block SHALL close with `}` rather than indentation or a terminating semicolon. Processes and peer graph nodes SHALL have braced bodies, including empty bodies. Decision-task bodies and their kind-specific decision nodes SHALL be braced. Existing record and enum declaration layouts MAY retain their current colon/indentation structure, but their member statements SHALL end in `;`. Whitespace and newlines alone SHALL NOT terminate statements. A semicolon inside a quoted literal SHALL NOT terminate a statement.

#### Scenario: Braces and semicolons
- **WHEN** a source uses `namespace demo;`, `version "1.0";`, and `process p { flow start -> done; }` with braced peer nodes
- **THEN** the source passes delimiter validation

#### Scenario: Missing delimiter
- **WHEN** a `flow` lacks `;`, a braced declaration lacks `}`, or a declaration uses the old colon/indentation form where braces are required
- **THEN** compilation fails with a syntax diagnostic and emits no Rust

### Requirement: Process flow and data bindings are separate
A process body SHALL reference named peer nodes using `flow <source> -> <target>;` for execution order and `bind <source>.<output> -> <target>.<input>;` for data transfer. A `flow` SHALL NOT implicitly pass data; a `bind` SHALL NOT imply execution order. Each bound input used on an activated route SHALL have a compatible source output available before its target executes. Unknown nodes/ports, incompatible types, unavailable outputs, missing required inputs, and ambiguous bindings SHALL fail validation. Nodes SHALL NOT be redefined in a process body. A source node with exactly one output MAY be named without `.output` in a value reference; a multi-output source SHALL use a qualified output name.

#### Scenario: Distinct edges
- **WHEN** a process contains `flow start -> calculate;`, `flow calculate -> done;`, `bind start.my_input_number -> calculate.amount;`, and `bind calculate.result -> done.result;` with valid peers
- **THEN** the task runs after `start`, and the bound values reach the matching typed inputs

#### Scenario: Data does not schedule tasks
- **WHEN** a process binds an output to a task input but provides no executable control-flow path to that task
- **THEN** validation rejects the graph rather than treating the binding as a flow edge

#### Scenario: Missing or mistyped input
- **WHEN** a decision-task input has no binding on an activated route or a `Number` output is bound to a `String` input
- **THEN** validation fails before generating Rust
