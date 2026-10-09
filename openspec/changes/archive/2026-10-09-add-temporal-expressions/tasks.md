# Tasks

## 1. Shared temporal values and serialization

- [x] 1.1 Reconcile `enhance-bl-number` changes in shared expression/type/codegen paths before implementation; verify both changes' existing language tests pass without overwritten edits.
- [x] 1.2 Add shared Date, Time and DateTime parsers/values with naive, fixed-offset and IANA zones, ISO/RFC 9557 text/JSON forms, and `24:00:00` normalization; verify focused unit tests cover round trips, invalid values, DST gaps/folds and midnight.
- [x] 1.3 Update typed input/output serialization and generated Rust type mappings without breaking offset-bearing RFC 3339 input; verify compiled-task and JSON regression tests, and document accepted value formats in README.

## 2. Temporal expression grammar and arithmetic

Task 3.1's duration values are a prerequisite for task 2.1's `.offset` property and task 2.2's arithmetic; implement 3.1 first, then resume these tasks. Keep task numbers for the existing review/verification record.

- [x] 2.1 Add constructor overloads, `datetime()` migration, component and calendar properties (including duration-valued `.offset`), `today()`/`now()` and evaluation clock snapshot; verify parser/type tests for valid calls and rejection of `dateTime(...)`, `=`, mixed zone kinds and wrong arguments, and update README syntax examples.
- [x] 2.2 Add date/time/datetime comparisons, subtraction, month-clamped arithmetic, time wrapping, zoned midnight projection, zone conversion/stripping and existing range/membership integration; reject applying non-integral-nanosecond durations to `Time`/`DateTime`; verify generated execution tests from `date-features.md` for clamps, wrapping, midnight differences, offsets, precision and zone errors.

## 3. Exact durations

- [x] 3.1 Add DTDuration/YMDuration parsing, normalization, finite-decimal totals and signed components using `rust_decimal`, checked arithmetic, equality, scaling/division (half-even rounding of nonterminating results) and canonical serialization; verify unit tests for representable sub-nanosecond fractions, finite-precision `3600/7`, overflow, negative components, invalid input and zero division, and document both duration formats and precision limits.
- [x] 3.2 Add duration rounding modes, `abs`, `isNegative`, `dtDurationBetween` and `ymDurationBetween`, with expression typing and generated calls; verify tie/negative/positive-step and signed-difference cases through compiled expressions.

## 4. Calendar inputs and operations

- [x] 4.1 Implement immutable Calendar/CalendarEntry values and documented typed JSON/host input shape including ranges, names, bounds and zone-kind validation; verify serialization, chronological ordering and rejection tests, and document the calendar input contract in README.
- [x] 4.2 Add calendar queries, entry access, equality, membership, next/prev and overloaded range overlap; verify compiled expression tests for range coverage, bounds, active entries, ordering and invalid `n`.
- [x] 4.3 Add calendarDrop/Keep/Merge with named optional arguments (`rangeMatch`, `dedupeBy`, `tiebreak`), pattern-name matching and immutable results; verify compiled expression tests for exact/regex/value/list/range matching, dedupe/tiebreak, unknown/duplicate names, and rejection of dictionary-style options while `==` remains equality.

## 5. Business and financial date functions

- [x] 5.1 Add classification, month/week boundaries, weekday/business-day navigation and arithmetic with optional calendar and strict bound errors; verify leap-year, holiday/weekend, n=0, preserved datetime time/zone, inclusive counts and strict-range tests, and document strict mode.
- [x] 5.2 Add signed days/months/years differences with all six bases, datetime `includeTime`, and financial-year/quarter basis rules; verify source examples, leap days, US/European 30/360 differences, invalid basis and UK April-6 boundary in compiled expressions, and document day-count conventions.

## 6. Integration

- [x] 6.1 Run full `cargo test` and OpenSpec strict validation; verify existing string/number, range, decision, graph and generated-code behavior remains intact, and reconcile any new overlap with the number change.
- [x] 6.2 Run four independent read-only reviews of the same completed diff using `.pi/skills/review-rust-project/SKILL.md`, `.pi/skills/review-rust-microsoft/SKILL.md`, `.pi/skills/review-rust-google/SKILL.md`, and `.pi/skills/rust-skills/SKILL.md` (leonardomso/rust-skills; reviewers must not see each other's findings); have the parent adjudicate every finding, including disagreements, against code and sources and record acceptance/rejection with reasons; implement accepted changes, use the existing ponytail-review skill on the revised diff, address its actionable findings, and re-verify affected work. Keep this task unchecked until every stage finishes; verify the recorded decisions and re-verification output.
