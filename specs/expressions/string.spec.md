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

## Go implementation (expr extension)

Lives in `expr/string.go`. Shared mechanics in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).

### Value type & host API (exported)

`BlString` is the immutable Go value type that represents a string inside the engine and at the
host-code boundary. Its only field is private (`s`) so callers cannot mutate the underlying
sequence — every operation in the library returns a fresh `BlString`.

The exported surface has three parts:

- **`BlValue` interface methods** — `Type()`, `Equal()`, `String()`, and the unexported
  `isBlValue()` marker — required of every blkit value type so the engine can treat them
  uniformly. `isBlValue()` is the sealing mechanism: because it's unexported, no type outside
  this package can satisfy `BlValue`, preventing host code from inventing fake values that the
  engine would then have to defend against. `String()` doubles as the `fmt.Stringer`
  implementation, so `fmt.Println(s)` produces the underlying text rather than the raw struct.
  `Equal` performs case-sensitive code-point comparison.
- **`String[T StringInput](v T)`** — the generic host constructor. The `StringInput` constraint
  accepts `string` or `[]byte`; a `[]byte` is interpreted as UTF-8 (invalid byte sequences are
  replaced with `U+FFFD`, matching Go's standard conversion). No error return — both inputs are
  infallible. Other Go types are deliberately rejected at compile time: textual representation
  of numbers, booleans, dates, etc. is a formatting decision the host should make explicitly
  (`strconv.Itoa`, `fmt.Sprintf`, `t.Format(...)`), or via the expression-language `string(from)`
  built-in if the conversion belongs inside an expression.
- **`Native()` accessor** — hands the underlying Go `string` back to host code. From there, Go's
  standard library provides all needed operations (comparison via `<`/`==`, length, indexing,
  slicing, etc.), so this is the only accessor required.

```go
// BlString wraps a native Go string. BlString methods count positions and
// length in Unicode code points (not bytes).
type BlString struct{ s string }

// BlValue interface — required by all Bl* value types.
func (BlString) Type() BlType { return BlTypeString }
func (s BlString) Equal(other BlValue) BlValue   // case-sensitive code-point comparison
func (s BlString) String() string
func (BlString) isBlValue() {}

// Host constructor — accepts a Go string or a UTF-8 []byte.
type StringInput interface { string | []byte }
func String[T StringInput](v T) BlString

// Host accessor (consume an evaluated result).
func (s BlString) Native() string                // underlying Go string
```

### Operator implementation functions (unexported)

`expr-lang/expr` has no knowledge of `BlString` and cannot apply Go's native `+`/`<`/etc. to
blkit values. For every operator that should work on two strings, blkit supplies a named Go
function that performs the operation on the underlying Go strings and returns the result wrapped
as a `BlValue`. The connection from operator token to function happens in two steps, neither of
which is unique to `BlString`:

1. The Registrations section below calls `expr.Function("concatStrings", typed2(concatStrings), …)`,
   which makes the engine aware of the function under that exact string name and records its type
   signature.
2. A central `operatorBindings()` in [bl-expr.spec.md](bl-expr.spec.md#operator-bindings) then
   calls `expr.Operator("+", "addNumbers", "concatStrings", …)`, which tells the engine "when you
   see `+` at parse time, try each of these registered functions in turn and dispatch to whichever
   one's signature matches the operand types." This step is centralised in one place because a
   single operator spans many types — `+` covers number addition, string concatenation, and
   several temporal forms — and `expr.Operator` needs the full list of candidates for each
   operator in a single call.

So when the parser encounters `a + b` and both operands type-check to `BlString`, the engine
finds `concatStrings` in the `"+"` binding list, sees its signature matches, and dispatches to it.

The return type is `BlValue` so the impls can propagate `BlNull` if the engine ever calls them
with a null operand (the type-checker normally prevents that, but the wider return type makes the
null path explicit).

Equality (`=` / `!=`) is **not** registered as a per-type operator impl. The engine dispatches
`=` / `!=` through the `Equal()` method on the `BlValue` interface, which `BlString` implements
above (case-sensitive code-point comparison). That single dispatch path handles null propagation
and cross-type comparison uniformly.

```go
func concatStrings(a, b BlString) BlValue  // "+"
func ltStrings(a, b BlString) BlValue      // "<"
func leStrings(a, b BlString) BlValue      // "<="
func gtStrings(a, b BlString) BlValue      // ">"
func geStrings(a, b BlString) BlValue      // ">="
// "=" and "!=" go through BlValue.Equal(); see BlString.Equal() above.
```

These are written in clean typed form (`BlString → BlValue`) for readability and unit testing.
The engine cannot consume them at this shape directly — they're wrapped by the `typed1`/`typed2`
adapters at registration time.

### Backing implementations (unexported, suffix `Fn`)

The library and conversion functions are implemented as these typed Go functions. They are
wrapped by `typed1`/`typed2`/`typed3` when registered with the engine in the next section.
Variadic implementations (`substringFn`, `matchesFn`, `replaceFn`, `extractFn`, `padStartFn`,
`padEndFn`, `stringJoinFn`) instead implement the engine's `func(...any) (any, error)` shape
directly because they accept optional arguments and cannot be expressed via a fixed-arity adapter.

```go
// Typed implementations — wrapped by typed1/typed2/typed3 at registration.
func stringLengthFn(s BlString) BlNumber
func substringBeforeFn(s, match BlString) BlString          // not found → ""
func substringAfterFn(s, match BlString) BlString           // not found → ""
func upperCaseFn(s BlString) BlString
func lowerCaseFn(s BlString) BlString
func trimFn(s BlString) BlString
func containsFn(s, match BlString) BlBoolean                // overloaded; see calendar.spec.md
func startsWithFn(s, match BlString) BlBoolean
func endsWithFn(s, match BlString) BlBoolean
func splitFn(s, delimiter BlString) BlList
func isBlankFn(s BlString) BlBoolean
func strIndexOfFn(s, match BlString) BlValue                // not found → Null; list overload in list.spec.md
func strIsEmptyFn(s BlString) BlBoolean                     // list overload in list.spec.md
func charAtFn(s BlString, pos BlNumber) BlString            // out of range → ""
func strReverseFn(s BlString) BlString                      // list overload in list.spec.md
func repeatFn(s BlString, times BlNumber) BlString

// Variadic implementations — handle optional args themselves in expr's raw shape.
func substringFn(args ...any) (any, error)    // 2- or 3-arg
func matchesFn(args ...any) (any, error)      // 2- or 3-arg (optional flags)
func replaceFn(args ...any) (any, error)      // 3- or 4-arg (optional flags)
func extractFn(args ...any) (any, error)      // 2- or 3-arg (optional flags)
func padStartFn(args ...any) (any, error)     // 2- or 3-arg (optional padChar)
func padEndFn(args ...any) (any, error)       // 2- or 3-arg (optional padChar)
func stringJoinFn(args ...any) (any, error)   // 1-, 2-, or 4-arg
func stringConvFn(args ...any) (any, error)   // string(from) — accepts any BlValue
```

The regex funcs (`matchesFn`/`replaceFn`/`extractFn`) compile patterns via Go's `regexp` package
(RE2 syntax); `matches` anchors with `^(?:…)$` before compiling. A bad pattern → `BlRegexError`.
`indexOf`/`isEmpty`/`reverse`/`contains` are overloaded with their list/calendar forms; each is
registered once with multiple signatures, in whichever spec owns the canonical entry. Native Go
`string` inputs wrap to `BlString`.

### Registrations (`stringOptions`, unexported)

`stringOptions()` returns the slice of `expr.Option` values the engine consumes during
initialisation to learn about every string-related operator impl and library function. Each entry
is built with `expr.Function(name, impl, typeHints...)`, where:

- `name` is the identifier the parser will recognise in expressions (and that `operatorBindings()`
  references for operator dispatch).
- `impl` must have the signature `func(...any) (any, error)` — that is the only shape
  [`expr-lang/expr`](https://github.com/expr-lang/expr) accepts. The `typed1` / `typed2` /
  `typed3` adapters (defined in [bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go))
  wrap a typed implementation such as `func(BlString, BlString) BlValue` into that shape,
  type-asserting each argument and boxing the result. The variadic implementations declared
  above already satisfy the shape and are registered directly.
- `typeHints` is a variadic list of `new(func(...) ...)` values. The engine reflects on them at
  compile time to validate that callers supply the right argument types — they carry no runtime
  cost. Multiple hints register the function as overloaded across arities (`substring(s, start)`
  and `substring(s, start, length)` are both valid).

The registrations are grouped to reflect their role: operator impls (consumed by
`operatorBindings()`), library functions (called directly by name from expressions), and the
`string` conversion built-in.

```go
func stringOptions() []expr.Option {
    return []expr.Option{
        // operator impls — bound to operator tokens by operatorBindings()
        expr.Function("concatStrings", typed2(concatStrings), new(func(BlString, BlString) BlValue)),
        expr.Function("ltStrings",     typed2(ltStrings),     new(func(BlString, BlString) BlValue)),
        expr.Function("leStrings",     typed2(leStrings),     new(func(BlString, BlString) BlValue)),
        expr.Function("gtStrings",     typed2(gtStrings),     new(func(BlString, BlString) BlValue)),
        expr.Function("geStrings",     typed2(geStrings),     new(func(BlString, BlString) BlValue)),
        // = and != dispatch via BlValue.Equal() — no per-type registration

        // library
        expr.Function("stringLength",    typed1(stringLengthFn),    new(func(BlString) BlNumber)),
        expr.Function("substring",       substringFn,               new(func(BlString, BlNumber) BlString),
                                                                    new(func(BlString, BlNumber, BlNumber) BlString)),
        expr.Function("substringBefore", typed2(substringBeforeFn), new(func(BlString, BlString) BlString)),
        expr.Function("substringAfter",  typed2(substringAfterFn),  new(func(BlString, BlString) BlString)),
        expr.Function("upperCase",       typed1(upperCaseFn),       new(func(BlString) BlString)),
        expr.Function("lowerCase",       typed1(lowerCaseFn),       new(func(BlString) BlString)),
        expr.Function("trim",            typed1(trimFn),            new(func(BlString) BlString)),
        expr.Function("contains",        typed2(containsFn),        new(func(BlString, BlString) BlBoolean)),
        expr.Function("startsWith",      typed2(startsWithFn),      new(func(BlString, BlString) BlBoolean)),
        expr.Function("endsWith",        typed2(endsWithFn),        new(func(BlString, BlString) BlBoolean)),
        expr.Function("matches",         matchesFn,                 new(func(BlString, BlString) BlBoolean),
                                                                    new(func(BlString, BlString, BlString) BlBoolean)),
        expr.Function("replace",         replaceFn,                 new(func(BlString, BlString, BlString) BlString),
                                                                    new(func(BlString, BlString, BlString, BlString) BlString)),
        expr.Function("split",           typed2(splitFn),           new(func(BlString, BlString) BlList)),
        expr.Function("extract",         extractFn,                 new(func(BlString, BlString) BlList),
                                                                    new(func(BlString, BlString, BlString) BlList)),
        expr.Function("isBlank",         typed1(isBlankFn),         new(func(BlString) BlBoolean)),
        expr.Function("indexOf",         typed2(strIndexOfFn),      new(func(BlString, BlString) BlValue)),
        expr.Function("isEmpty",         typed1(strIsEmptyFn),      new(func(BlString) BlBoolean)),
        expr.Function("charAt",          typed2(charAtFn),          new(func(BlString, BlNumber) BlString)),
        expr.Function("reverse",         typed1(strReverseFn),      new(func(BlString) BlString)),
        expr.Function("padStart",        padStartFn,                new(func(BlString, BlNumber) BlString),
                                                                    new(func(BlString, BlNumber, BlString) BlString)),
        expr.Function("padEnd",          padEndFn,                  new(func(BlString, BlNumber) BlString),
                                                                    new(func(BlString, BlNumber, BlString) BlString)),
        expr.Function("repeat",          typed2(repeatFn),          new(func(BlString, BlNumber) BlString)),
        expr.Function("stringJoin",      stringJoinFn,              new(func(BlList) BlString),
                                                                    new(func(BlList, BlString) BlString),
                                                                    new(func(BlList, BlString, BlString, BlString) BlString)),

        // conversion
        expr.Function("string", stringConvFn, new(func(BlValue) BlString)), // string(from) — any BlValue → BlString
    }
}
```

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
