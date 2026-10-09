# Proposal

## Why

The current `.bl` temporal expressions only construct simple dates, offset-free times, and offset-required datetimes; real scheduling needs duration arithmetic, zone-aware values, business calendars, and date functions. `date-features.md` collects the desired expression behavior and examples in one place.

## What Changes

- Extend `Date`, `Time`, and `DateTime` to support naive, fixed-offset, and IANA-zoned values, component/calendar properties, conversions, arithmetic, comparisons, and zone operations. Accept `time("24:00:00")` and normalize it to midnight.
- **BREAKING:** Replace the expression constructor `dateTime(...)` with `datetime(...)` (the user-facing type remains `DateTime`); remove the existing offset requirement for `DateTime` inputs. Preserve `.bl` `==`/`!=` equality syntax, not the source document's `=` spelling.
- Add distinct `DTDuration` and `YMDuration` values with finite-decimal arithmetic using the existing `Number` precision, rounding, totals, and difference functions. Fractional input and duration division remain supported; repeating quotients may round.
- Add immutable input-supplied calendars, calendar membership/query/filter/merge operations, business-day and date-difference functions, fiscal-year calculations, and timezone conversion/stripping.
- Keep the existing `.bl` range boundary syntax and validation behavior; extend temporal ranges and membership to the new values without introducing a separate `=` operator or calendar expression literal.
- Cover the documented examples and invalid cases with expression, generated-code, and serialized-input tests. Host-side Go constructors and iCalendar import are out of scope.

## Capabilities

### New Capabilities

- None. These are extensions of the existing business language.

### Modified Capabilities

- `business-language`: Expand the temporal type, expression, range, and validation contract for `date-features.md`, with the explicit syntax and compatibility decisions above.

## Impact

Parser, semantic type inference/validation, Rust code generation and generated temporal types, JSON input/output validation, range operations, and language/compiled execution tests. Existing `.bl` sources using `dateTime(...)` must migrate to `datetime(...)`; existing RFC 3339 offset-bearing datetime inputs remain valid. Calendar input requires an explicit typed host/serialization route, not an expression-language constructor. Existing `enhance-bl-number` work may touch shared expression/type/codegen paths; coordinate integration rather than overwriting it.
