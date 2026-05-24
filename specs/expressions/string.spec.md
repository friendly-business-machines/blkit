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

A **string literal** is the syntactic form used inside a blkit expression to write a constant
string value — for example, the `"café"` in `upperCase("café")`. Literals are delimited by
double quotes.

Non-ASCII characters may be written directly inside a literal (e.g. `"café"`, `"🎉"`); the
expression text is interpreted as UTF-8. The supported escape sequences are:

| Escape | Result |
|---|---|
| `\\` | backslash `\` |
| `\"` | double quote `"` |
| `\n` | newline (U+000A) |
| `\r` | carriage return (U+000D) |
| `\t` | tab (U+0009) |
| `\uXXXX` | code point `U+XXXX` (exactly 4 hex digits; BMP only) |
| `\U{XXXXXX}` | code point by hex value (1–6 hex digits, for any code point including > U+FFFF) |

Any other `\x` sequence is a parse error. Forward slashes (`/`) are not special and do not need
escaping.

```
"hello"               // → hello
"line1\nline2"        // → line1
                      //   line2
"quote: \""           // → quote: "
"path: C:\\tmp"       // → path: C:\tmp
"naïve café"          // → naïve café
"\u00E9"              // → é
"\U{1F389}"           // → 🎉
```

### Length of literal forms

Length is measured in code points, so every escape — no matter how many source characters it
spans — contributes exactly **one** code point to the resulting string. Directly-typed
non-ASCII characters behave the same way: `"\u00E9"` and `"é"` denote the same one-code-point
value.

```
stringLength("hello")          // → 5
stringLength("a\nb")           // → 3    (\n is one code point)
stringLength("\\")             // → 1    (one backslash)
stringLength("\"")             // → 1    (one double quote)
stringLength("\u00E9")         // → 1    (one code point, U+00E9 = é)
stringLength("é")              // → 1    (typed directly — same value as above)
stringLength("\U{1F389}")      // → 1    (one supplementary code point)
stringLength("🎉")             // → 1
stringLength("🎉a")            // → 2
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

`matches`, `replace`, and `extract` use the **RE2 syntax** of Go's standard
[`regexp`](https://pkg.go.dev/regexp/syntax) package:

- No lookahead/lookbehind and no backreferences (RE2 design).
- Shorthand character classes: `\d` (ASCII digit `[0-9]`), `\w` (ASCII word character
  `[A-Za-z0-9_]`), `\s` (whitespace ` `, `\t`, `\n`, `\r`, `\f`), and their negations `\D`, `\W`,
  `\S`. For Unicode-aware letters/digits use the property classes below (`\p{L}`, `\p{N}`).
- Unicode property classes: `\p{L}` (letter), `\p{N}` (number), `\p{Zs}` (space separator), etc.
- POSIX classes inside `[…]`: `[[:alpha:]]`, `[[:digit:]]`, `[[:space:]]`, …
- Flags: `"i"` case-insensitive, `"m"` multiline (`^`/`$` match line boundaries), `"s"`
  dot-matches-newline. These map to RE2's inline flag groups `(?i)`, `(?m)`, `(?s)`, which may
  also be written directly in the pattern.
- A malformed pattern → `BlRegexError` at evaluation time.
- `matches` requires the **whole string** to match (the engine anchors the pattern with
  `^(?:…)$` internally); use `contains` or `extract` for partial matching.
- Replacement strings use `$1`, `$2`, … for numbered groups and `${name}` for named groups.

> **Backslashes in the literal.** A regex metacharacter starting with `\` must be escaped at the
> blkit string-literal layer (see [§ Literals](#literals)): write `"\\d"` to pass the pattern
> `\d` to the regex engine, `"\\w"` for `\w`, `"\\."` for a literal dot, and so on. Inline-flag
> groups like `(?i)` need no escaping.

### Examples

```
matches("hello", "h.+o")                                        // → true
matches("ABC", "[a-z]+", "i")                                   // → true   (flag)
matches("Hello", "(?i)hello")                                   // → true   (inline flag)
matches("foo\nbar", ".+", "s")                                  // → true   (dot matches newline)
matches("café", "\\p{L}+")                                      // → true   (Unicode letter class)
matches("page 42", "\\d+")                                      // → false  (whole-string, not partial)
matches("42", "\\d+")                                           // → true

replace("a-b-c", "-", "/")                                      // → "a/b/c"
replace("2025-03-28", "(\\d{4})-(\\d{2})-(\\d{2})", "$3/$2/$1") // → "28/03/2025"
replace("HELLO world", "[A-Z]+", "x")                           // → "x world"

extract("id 12, 34", "\\d+")                                    // → ["12", "34"]
extract("a1 b22 c333", "[a-z]\\d+")                             // → ["a1", "b22", "c333"]
```

`contains` is a literal substring test, not a regex — for partial-string regex matching, use
`extract` (which returns `[]` when nothing matches) or wrap the pattern: `matches(s, ".*pat.*")`.

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
// BlString wraps a native Go string. BlString methods count positions and
// length in Unicode code points (not bytes).
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

The regex funcs (`matchesFn`/`replaceFn`/`extractFn`) compile patterns via Go's `regexp` package
(RE2 syntax); `matches` anchors with `^(?:…)$` before compiling. A bad pattern →
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
