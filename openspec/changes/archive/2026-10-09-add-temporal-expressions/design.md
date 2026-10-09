# Design

## Context

See proposal.md and specs/business-language/spec.md. Current `.bl` validates constant `date`/`time`/`dateTime` strings in `src/semantic/types.rs`, parses expressions in `src/expr.rs`, and emits chrono-based values and custom `Date`/`Time` JSON wrappers from `src/codegen/mod.rs`. `DateTime` is currently a `chrono::DateTime<FixedOffset>` alias. Ranges, `between`, JSON/compiled tasks and decision expressions share these paths; updating parsing alone would leave generated behavior inconsistent. An independent `enhance-bl-number` change may also edit expression/type/codegen code.

## Goals / Non-Goals

**Goals:** One temporal interpretation used by compile-time checks, generated execution, range operations and JSON; predictable failure at invalid-input boundaries; preserve existing `.bl` `==` syntax and non-temporal behavior.

**Non-Goals:** Go constructors or iCalendar import; a general-purpose dictionary or regex-value language, a standalone `Range<T>` declaration, changing process retry/deadline duration syntax, or converting previously persisted process definitions in place.

## Decisions

1. **Shared temporal runtime rather than more generated ad-hoc wrappers.** Put Date/Time/DateTime (civil value + explicit naive/fixed-offset/IANA zone kind), durations, calendar/entry values, parsers, serialization, and operations in a library module; generated code calls it. Semantic constant checks use the same parsers; semantic type checking still runs before emission. Avoid two independent parsers in validator and generated Rust. Keep `DateTime` as the `.bl` type, rename only its expression constructor to `datetime`; reject `dateTime` and migrate examples/tests. `==`/`!=` stay unchanged. Alternative: extend the emitted chrono snippets and aliases; they cannot represent naive and IANA-zoned values uniformly and duplicate validation.

2. **Use the existing finite-decimal `Number` precision for durations.** Reuse chrono for calendar calculations and `rust_decimal` for signed duration totals, fractional input, scaling and division. Accept exactly representable finite decimals within its limits (including sub-nanosecond input); use checked arithmetic, with half-even rounding at the available precision for nonterminating division and errors for overflow or zero division. Canonical ISO strings normalize the stored, possibly rounded decimal with a fraction only on the smallest emitted unit; `.total*` returns a `Number` at the same precision. Do not silently discard sub-nanosecond precision when applying a days-time duration to `Time` or `DateTime`: if the value is not an integral number of nanoseconds, report an evaluation error. Apply months only when integral; fractional `YMDuration` remains valid for arithmetic but fails when applied to a calendar date. Alternative: arbitrary-precision rational durations would preserve `3600/7` exactly but cannot serialize that result as a finite ISO decimal or existing `Number` without another policy. Implement duration values before temporal `.offset` properties, which return `DTDuration`.

3. **Zone resolution is explicit.** Preserve the zone label on values; for instant comparisons project fixed-offset/IANA dates to midnight in their zone and zoned time-only values to a shared snapshot of the evaluation's current date in each zone. Snapshot `today()`/`now()` once per expression evaluation and use that same clock/zone for time-only projection. Reject mixed naive/zoned operations and nonexistent/ambiguous IANA local times instead of silently choosing an offset. For time-only `24:00:00`, parse as naive/zoned `00:00:00` and discard the day advance as requested; datetime input still requires `T`. Alternative: silently use the current system offset without the IANA zone identity, which breaks DST and determinism within an evaluation.

4. **Calendar input is data, not expression construction.** Expose a typed `Calendar` and read-only `CalendarEntry` in generated signatures and JSON. Serialize calendar input as an object with `validFrom`, `validTo` and `entries`, each entry holding `value` (ISO point or range with `start`, `end` and endpoint inclusion) and optional `name`; keep the same shapes on output. Parse entries through shared temporal parsers, validate zone-kind consistency, bounds and chronological sorting at the boundary; transformations create new calendars. `entries*`, `find`, `next`/`prev` return typed entry/list results usable by `entryValue`/`entryName`; calendar equality includes validity. Parse named optional arguments only for documented calendar functions, e.g. `calendarDrop(c, range, rangeMatch="overlap")` and `calendarMerge([a, b], dedupeBy="value", tiebreak="first")`; `=` binds a call argument, while `==` remains equality. Reject duplicate/unknown names and positional arguments after named arguments. Do not introduce `{key: value}` expression literals or general dictionary values. Keep `pattern(...)` limited to calendar name targets. Resolve `overlaps(c, range)` by argument type, leaving existing range-vs-range semantics intact. Alternative: invent a calendar literal or general dictionary language and replicate date parsing in expression grammar; more scope for no benefit.

5. **Fallible expression values propagate errors.** Component constructors and dynamic validation, zone resolution, date arithmetic, invalid regex/options, and strict calendar bounds emit recoverable evaluation errors through the same generated execution-error path used by fallible string expressions; constant-invalid values fail compilation. The `bl.CalendarRangeError` category is preserved in diagnostics rather than turned into `false` or a panic. Add specific type inference branches for each overload, including `CalendarEntry` and list results; do not loosen other builtin type checks.

## Risks / Trade-offs

- [Different host timezones/DST data yield different results for `now`, `today` and zoned time-only values] → Snapshot per evaluation, expose host-local behavior in documentation, use fixed clocks/zones in tests.
- [Existing `dateTime(...)` sources and generated code cease to compile] → Migrate tests/examples/README together and give an actionable unknown-function diagnostic; leave offset-bearing RFC 3339 inputs valid.
- [Temporal type serialization changes affect persisted JSON/process versions] → Version `.bl` processes when migrating and verify old offset-bearing data round-trips; avoid rewriting stored records.
- [Calendar operations plus finite-decimal durations increase scope] → Implement in bounded tested slices against the spec; no general dictionary/regex framework or Go/iCal import.
- [Concurrent number change touches shared files] → Reconcile its approved artifacts/branch before editing the same parser or codegen sites; preserve both behaviors in regression tests.

## Migration Plan

1. Implement shared values/parsing and compile-time inference before switching generated representations; preserve original offset-bearing input acceptance.
2. Switch generator and JSON boundaries; migrate `dateTime(...)` usages to `datetime(...)` in examples and tests, retaining `==`. No alias is planned.
3. Add duration/calendar/date functions and evaluate source examples against compiled tasks and decision expressions; re-run existing range, string, process and serialization tests.
4. Document the calendar JSON shape, time-only DST rules and migration in README. Existing persisted process versions are not rewritten; users publish a new process version for changed definitions.
