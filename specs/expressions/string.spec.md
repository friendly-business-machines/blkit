---
name: BlString
description: The string type in the blkit expression language — an immutable Unicode sequence. Covers string literals, the concatenation/comparison operators, the string built-in functions (incl. blkit extensions), the regex dialect, and the Go layer (BlString + expr registrations).
targets:
  - ../../expr/string.go
---

# BlString — the `string` type

`string` is an immutable sequence of Unicode code points. The Go value type backing it is
`BlString`. Positions are **1-indexed**; length is measured in code points (not bytes or UTF-16
units).

See [bl-expr.spec.md](bl-expr.spec.md) for the engine and cross-cutting semantics.

---

## Literals

String literals use double quotes; standard escapes apply.

```
"hello"               // → "hello"
"line1\nline2"        // → two lines
"quote: \""           // → quote: "
"naïve café"          // → "naïve café"   (Unicode preserved)
```

`[@test] ../../expr/string_test.go`

---

## Operators

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `+` | concatenation (string-only) | `"foo" + "bar"` | `"foobar"` |
| `=` `!=` | equality (case-sensitive, code-point order) | `"a" = "A"` | `false` |
| `in` | membership | `"active" in ["active","pending"]` | `true` |

Concatenation is **string-only**; to join a non-string, convert first: `"order-" + string(123) // →
"order-123"`. Concatenating a non-string directly → `null`.

`[@test] ../../expr/string_operators_test.go`

---

## Built-in functions

Standard DMN functions plus blkit extensions (**ext** — no DMN equivalent). Positions are 1-indexed.

| Function | Example | Result |
|---|---|---|
| `stringLength(s)` | `stringLength("foo")` | `3` |
| `substring(s, start[, length])` | `substring("foobar", 3, 2)` | `"ob"` (negative `start` counts from end) |
| `substringBefore(s, match)` | `substringBefore("a-b", "-")` | `"a"` (not found → `""`) |
| `substringAfter(s, match)` | `substringAfter("a-b", "-")` | `"b"` |
| `upperCase(s)` / `lowerCase(s)` | `upperCase("aBc")` | `"ABC"` |
| `trim(s)` | `trim("  hi  ")` | `"hi"` |
| `contains(s, match)` | `contains("foobar", "oo")` | `true` (case-sensitive) |
| `startsWith(s, match)` / `endsWith(s, match)` | `startsWith("foo", "fo")` | `true` |
| `matches(s, pattern[, flags])` | `matches("ABC", "[a-z]+", "i")` | `true` (whole-string match) |
| `replace(s, pattern, repl[, flags])` | `replace("a-b", "-", "/")` | `"a/b"` (`$1` group refs) |
| `split(s, delimiter)` | `split("a,b,c", ",")` | `["a","b","c"]` |
| `extract(s, pattern[, flags])` | `extract("id 12, 34", "[0-9]+")` | `["12","34"]` |
| `isBlank(s)` | `isBlank("  ")` | `true` (empty or all-whitespace) |
| `indexOf(s, match)` **ext** | `indexOf("hello", "l")` | `3` (1st position, or `null`) |
| `isEmpty(s)` **ext** | `isEmpty("")` | `true` (zero length) |
| `charAt(s, position)` **ext** | `charAt("hello", 1)` | `"h"` (out of range → `""`) |
| `reverse(s)` **ext** | `reverse("abc")` | `"cba"` |
| `padStart(s, length[, padChar])` **ext** | `padStart("7", 4, "0")` | `"0007"` |
| `padEnd(s, length[, padChar])` **ext** | `padEnd("hi", 5, ".")` | `"hi..."` |
| `repeat(s, times)` **ext** | `repeat("ab", 3)` | `"ababab"` |
| `stringJoin(list[, delimiter[, prefix, suffix]])` | `stringJoin(["a","b"], ", ")` | `"a, b"` |

Concatenation of many parts uses `+` chains or `stringJoin`. `string(from)` (any → string) is the
conversion built-in (see [§ Go implementation](#go-implementation-expr-extension)).

`[@test] ../../expr/string_functions_test.go`

---

## Regex dialect

`matches`, `replace`, and `extract` use the **XML Schema regex dialect** (not PCRE):

- No lookahead/lookbehind.
- Unicode classes `\p{L}`, `\p{N}`, `\p{Z}`, etc.
- Flags: `"i"` case-insensitive, `"m"` multiline, `"s"` dot-matches-newline, `"x"` ignore pattern
  whitespace.
- A malformed pattern → `BlRegexError` at evaluation time.
- `matches` requires the **whole string** to match; use `.*…*` or `contains`/`extract` for partials.

`[@test] ../../expr/string_regex_test.go`

---

## Semantics & behaviour

- Immutable; all functions return new strings.
- Length and indexing are by **Unicode code point** (`stringLength("🎉") // → 1`).
- Comparison is case-sensitive, code-point order.
- `isBlank` treats any Unicode whitespace as blank; `isEmpty` is strictly zero-length.
- `split` with leading/trailing/consecutive delimiters yields empty-string elements.

`[@test] ../../expr/string_semantics_test.go`

---

## Migration mapping (legacy method-chained → string)

| Legacy method | New form |
|---|---|
| `length` | `stringLength(s)` |
| `isEmpty` / `isBlank` | `isEmpty(s)` **ext** / `isBlank(s)` |
| `charAt` | `charAt(s, pos)` **ext** |
| `substring` | `substring(s, start[, len])` |
| `substringBefore` / `substringAfter` | same as built-ins |
| `contains` / `startsWith` / `endsWith` | same as built-ins |
| `matches` / `indexOf` / `extract` | `matches` / `indexOf(s, match)` **ext** / `extract` |
| `upperCase` / `lowerCase` / `trim` / `reverse` | same; `reverse` **ext** |
| `padStart` / `padEnd` / `repeat` | same built-ins **ext** |
| `replace` / `split` | `replace` / `split` |
| `concatenate` / `concat` | `+` operator (or `stringJoin`) |
| `in` | `in` operator |
| `equals` / `notEqual` | `=` / `!=` |
| `compareTo` / `toNativeString` / `String` | Go host accessors on `BlString` (below) |

---

## Go implementation (expr extension)

Lives in `expr/string.go`. Shared mechanics in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).

### Value type & host API (exported)

```go
// BlString wraps an immutable, code-point-aware Go string.
type BlString struct{ s string }

func (BlString) Type() BlType { return BlTypeString }
func (s BlString) Equal(other BlValue) BlValue
func (s BlString) ToMarkdown() string
func (BlString) isBlValue() {}

func String(s string) BlString             // host constructor
func (s BlString) ToNativeString() string  // host accessors
func (s BlString) CompareTo(other BlString) int // -1 / 0 / 1, code-point order
func (s BlString) String() string
```

### Operator impl funcs (unexported)

```go
func concatStrings(a, b BlString) BlValue  // "+"; non-string operand handled by the engine → BlNull
func ltStrings(a, b BlString) BlValue      // "<" ; le/gt/ge code-point order. eq/ne via shared equality
```

`concatStrings` is bound to `+` in `operatorBindings()` alongside the numeric/temporal `+` impls;
the checker selects it for string operands.

### Registrations (`stringOptions`, unexported)

```go
func stringOptions() []expr.Option {
    return []expr.Option{
        expr.Function("concatStrings", typed2(concatStrings), new(func(BlString, BlString) BlValue)),
        expr.Function("ltStrings",     typed2(ltStrings),     new(func(BlString, BlString) BlValue)),

        expr.Function("stringLength",    typed1(stringLengthFn),  new(func(BlString) BlNumber)),
        expr.Function("substring",       substringFn,             new(func(BlString, BlNumber) BlString),
                                                                  new(func(BlString, BlNumber, BlNumber) BlString)),
        expr.Function("substringBefore", typed2(substringBeforeFn), new(func(BlString, BlString) BlString)),
        expr.Function("substringAfter",  typed2(substringAfterFn),  new(func(BlString, BlString) BlString)),
        expr.Function("upperCase",       typed1(upperCaseFn),     new(func(BlString) BlString)),
        expr.Function("lowerCase",       typed1(lowerCaseFn),     new(func(BlString) BlString)),
        expr.Function("trim",            typed1(trimFn),          new(func(BlString) BlString)),
        expr.Function("contains",        typed2(containsFn),      new(func(BlString, BlString) BlBoolean)), // overloaded for calendar — see calendar.spec.md
        expr.Function("startsWith",      typed2(startsWithFn),    new(func(BlString, BlString) BlBoolean)),
        expr.Function("endsWith",        typed2(endsWithFn),      new(func(BlString, BlString) BlBoolean)),
        expr.Function("matches",         matchesFn,               new(func(BlString, BlString) BlBoolean),
                                                                  new(func(BlString, BlString, BlString) BlBoolean)),
        expr.Function("replace",         replaceFn,               new(func(BlString, BlString, BlString) BlString),
                                                                  new(func(BlString, BlString, BlString, BlString) BlString)),
        expr.Function("split",           typed2(splitFn),         new(func(BlString, BlString) BlList)),
        expr.Function("extract",         extractFn,               new(func(BlString, BlString) BlList),
                                                                  new(func(BlString, BlString, BlString) BlList)),
        expr.Function("isBlank",         typed1(isBlankFn),       new(func(BlString) BlBoolean)),
        // ext:
        expr.Function("indexOf",   typed2(strIndexOfFn),  new(func(BlString, BlString) BlValue)),   // string overload (list overload in list.spec.md)
        expr.Function("isEmpty",   typed1(strIsEmptyFn),  new(func(BlString) BlBoolean)),           // string overload
        expr.Function("charAt",    typed2(charAtFn),      new(func(BlString, BlNumber) BlString)),
        expr.Function("reverse",   typed1(strReverseFn),  new(func(BlString) BlString)),            // string overload
        expr.Function("padStart",  padStartFn,            new(func(BlString, BlNumber) BlString), new(func(BlString, BlNumber, BlString) BlString)),
        expr.Function("padEnd",    padEndFn,              new(func(BlString, BlNumber) BlString), new(func(BlString, BlNumber, BlString) BlString)),
        expr.Function("repeat",    typed2(repeatFn),      new(func(BlString, BlNumber) BlString)),
        expr.Function("stringJoin", stringJoinFn,         new(func(BlList) BlString), new(func(BlList, BlString) BlString),
                                                          new(func(BlList, BlString, BlString, BlString) BlString)),

        expr.Function("string",    stringConvFn,          new(func(BlValue) BlString)), // string(from) conversion
    }
}
```

The regex funcs (`matchesFn`/`replaceFn`/`extractFn`) compile XML-Schema patterns; a bad pattern →
`BlRegexError`. `indexOf`/`isEmpty`/`reverse`/`contains` are **overloaded** with their list/calendar
forms (each registered once, with multiple signatures, in whichever spoke owns the canonical entry).
Native Go `string` inputs wrap to `BlString`.

`[@test] ../../expr/string_test.go`

---

## Edge cases

- `substring` start beyond end → `""`; over-long `length` is clamped.
- `substringBefore`/`substringAfter` with delimiter absent → `""`.
- `indexOf` not found → `null` (not `0`).
- `extract` with no matches → `[]` (not `null`).
- `padStart`/`padEnd` with a multi-code-point `padChar` → `BlTypeError`.
- `repeat` with a negative count → `BlTypeError`.
- `split("", "")` rules and empty-segment behaviour as above.
