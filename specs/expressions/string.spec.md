---
name: bl.BlString
description: The string type in the blkit expression language — an immutable Unicode sequence. Covers string literals, the concatenation/comparison operators, the string built-in functions (incl. blkit extensions), the regex dialect, and the Go layer (bl.BlString + expr registrations).
targets:
  - ../../string.go
---

# bl.BlString — the `string` type

`string` is an immutable sequence of Unicode code points. The Go value type backing it is
`bl.BlString`. Positions are **1-indexed**; length is measured in code points (not bytes or UTF-16
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

`[@test] ../../string_test.go`

---

## Construction (host-side)

Host Go code constructs a `bl.BlString` via the generic `String[T StringInput](v T) (bl.BlString,
error)` constructor. The `StringInput` constraint accepts `string` or `[]byte`. A `string`
input is infallible (a Go `string` has no encoding contract to violate, so it can't fail). A
`[]byte` is interpreted as UTF-8 and validated; the `error` return fires when the bytes
contain invalid UTF-8 (random binary data, Latin-1 text, a truncated multi-byte sequence,
etc.).

```go
// host-side (Go)
// From a Go string — infallible.
var greeting, _ = bl.String("hello")

// From a valid-UTF-8 []byte.
var bytes      = []byte{0x68, 0x65, 0x6C, 0x6C, 0x6F}     // "hello"
var greet2, _  = bl.String(bytes)

// From invalid bytes — error.
var bad        = []byte{0x68, 0xFF, 0xFE}                 // 0xFF / 0xFE aren't valid UTF-8 leading bytes
var _, err     = bl.String(bad)                              // err != nil — invalid UTF-8

// Other Go types are deliberately rejected at compile time — convert explicitly first.
// e.g. for a number, use strconv.Itoa or fmt.Sprintf before passing to String:
var label, _   = bl.String(fmt.Sprintf("score: %d", 42))     // → "score: 42"
```

Other Go types — numbers, booleans, dates, etc. — are deliberately rejected at compile time:
their textual representation is a formatting decision the host should make explicitly via
`strconv` / `fmt` / `time.Time.Format` / etc., or via the expression-language `string(from)`
built-in if the conversion belongs inside an expression.

---

## Operators

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `+` | concatenation (string-only) | `"foo" + "bar"` | `"foobar"` |
| `=` `!=` | equality (case-sensitive, code-point order) | `"a" = "A"` | `false` |
| `in` | membership | `"active" in ["active","pending"]` | `true` |

Concatenation is **string-only**; to join a non-string, convert first: `"order-" + string(123) // →
"order-123"`. Concatenating a non-string directly → `null`.

`[@test] ../../string_operators_test.go`

---

## Built-in functions

DMN-inspired functions plus blkit extensions (**ext** — no DMN equivalent). Positions are 1-indexed.

| Function | Example | Result |
|---|---|---|
| `stringLength(s)` | `stringLength("foo")` | `3` |
| `substring(s, start[, length])` | `substring("foobar", 3, 2)` | `"ob"` (negative `start` counts from end) |
| `substringBefore(s, match)` | `substringBefore("a-b", "-")` | `"a"` (not found → `""`) |
| `substringAfter(s, match)` | `substringAfter("a-b", "-")` | `"b"` |
| `upperCase(s)` / `lowerCase(s)` | `upperCase("aBc")` | `"ABC"` |
| `trim(s)` | `trim("  hi  ")` | `"hi"` |
| `trimLeading(s)` **ext** | `trimLeading("  hi  ")` | `"hi  "` |
| `trimTrailing(s)` **ext** | `trimTrailing("  hi  ")` | `"  hi"` |
| `contains(s, match)` | `contains("foobar", "oo")` | `true` (case-sensitive) |
| `startsWith(s, match)` / `endsWith(s, match)` | `startsWith("foo", "fo")` | `true` |
| `matches(s, pattern[, flags])` | `matches("ABC", "[a-z]+", "i")` | `true` (whole-string match) |
| `replace(s, pattern, repl[, flags])` | `replace("a-b", "-", "/")` | `"a/b"` (`$1` group refs) |
| `split(s, delimiter)` | `split("a,b,c", ",")` | `["a","b","c"]` |
| `split(s, delimiters)` **ext** | `split("a,b;c", [",", ";"])` | `["a","b","c"]` (list-of-delimiters overload) |
| `extract(s, pattern[, flags])` | `extract("id 12, 34", "[0-9]+")` | `["12","34"]` |
| `isBlank(s)` | `isBlank("  ")` | `true` (empty or all-whitespace) |
| `indexOf(s, match)` **ext** | `indexOf("hello", "l")` | `3` (1st position, or `null`) |
| `isEmpty(s)` **ext** | `isEmpty("")` | `true` (zero length) |
| `charAt(s, position)` **ext** | `charAt("hello", 1)` | `"h"` (out of range → `""`) |
| `reverse(s)` **ext** | `reverse("abc")` | `"cba"` |
| `padLeading(s, length[, padChar])` **ext** | `padLeading("7", 4, "0")` | `"0007"` |
| `padTrailing(s, length[, padChar])` **ext** | `padTrailing("hi", 5, ".")` | `"hi..."` |
| `repeat(s, times)` **ext** | `repeat("ab", 3)` | `"ababab"` |
| `stringJoin(list[, delimiter[, prefix, suffix]])` | `stringJoin(["a","b"], ", ")` | `"a, b"` — full docs in [list.spec.md](list.spec.md#built-in-functions) (lives there alongside the other list-reducing functions like `sum`/`min`/`max`) |

Concatenation of many parts uses `+` chains or `stringJoin`. `string(from)` (any → string) is the
conversion built-in (see [§ Go implementation](#go-implementation-expr-extension)).

`[@test] ../../string_functions_test.go`

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
- A malformed pattern → `bl.RegexError` at evaluation time.
- `matches` requires the **whole string** to match (the engine anchors the pattern with
  `^(?:…)$` internally); use `contains` or `extract` for partial matching.
- Replacement strings use `$1`, `$2`, … for numbered groups and `${name}` for named groups.

> **Backslashes in the literal.** A regex metacharacter starting with `\` must be escaped at the
> blkit string-literal layer (see [§ Literals](#literals)): write `"\\d"` to pass the pattern
> `\d` to the regex engine, `"\\w"` for `\w`, `"\\."` for a literal dot, and so on. Inline-flag
> groups like `(?i)` need no escaping.

### Examples

```
// expression-language
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

`[@test] ../../string_regex_test.go`

---

## Precompiled patterns (`pattern(s)` / `bl.BlRegex`)

`pattern(s)` compiles a regex source string into a **`bl.BlRegex`** — a first-class blkit value
that carries a compiled RE2 program. The dialect is identical to inline strings (see
[§ Regex dialect](#regex-dialect)); a malformed source → `bl.RegexError` at the `pattern(...)`
call site rather than at the use site. The three regex built-ins (`matches`, `replace`,
`extract`) accept either a `bl.BlString` source pattern or a `bl.BlRegex` value in the pattern slot:

```
// expression-language — inline source string is compiled on every call site.
matches("hello", "h.+o")                                          // → true

// expression-language — inline pattern(...) builds a bl.BlRegex value used right away.
// Useful when you need flags-only-via-inline syntax or want to pass the same regex to
// multiple consumers within one expression.
matches("alice@example.com",
    pattern("[\\w.+-]+@[\\w-]+\\.[\\w.-]+"))                      // → true
extract("a@b.com, c@d.org",
    pattern("[\\w.+-]+@[\\w-]+\\.[\\w.-]+"))                      // → ["a@b.com", "c@d.org"]
matches("Hi there", pattern("(?i)^hi.*"))                         // → true   (case-insensitive flag inline)
```

For genuine **cross-call reuse** (compile once, evaluate many times against different inputs),
build the `bl.BlRegex` host-side via the Go constructor and supply it as an input variable:

```go
// host-side (Go) — compile once.
var emailRe, _ = bl.Pattern(`[\w.+-]+@[\w-]+\.[\w.-]+`)

// Then hand emailRe to the engine as an input variable; expressions reference it by name.
var schema, _ = bl.Schema(
    bl.Field{Name: "addr",    Type: bl.TypeString},
    bl.Field{Name: "emailRe", Type: bl.TypePattern},
)
var matchEmail, _ = bl.Expr(`matches(addr, emailRe)`, schema)
var addr, _   = bl.String("alice@example.com")
var inputs, _ = bl.Dictionary(map[string]bl.BlValue{
    "addr":    addr,
    "emailRe": emailRe,
})
var result, _ = matchEmail.Evaluate(inputs)
```

`pattern(s)` accepts only a single argument — the source string. Flags (`"i"`, `"m"`, `"s"`)
that the three-argument forms of `matches` / `replace` / `extract` accept must be written
inline as `(?i)` / `(?m)` / `(?s)` groups inside the source. A `bl.BlRegex` passed to the
three-argument form of `matches(s, p, flags)` — where the precompiled pattern already encodes
its flags — → `bl.TypeError`.

Two `bl.BlRegex` values compare equal iff their source strings are equal (structural equality on
the source, not on the compiled program).

`pattern(s)` is a blkit extension (**ext**); the inline-string forms remain the FEEL-compatible
path. `bl.BlRegex` is also accepted as a target type by
[calendar.spec.md § calendarDrop](calendar.spec.md#calendardropc-target) (and the symmetric
`calendarKeep`), where it dispatches to regex-based name matching against calendar entries.

`[@test] ../../string_pattern_test.go`

---

## Semantics & behaviour

- Immutable; all functions return new strings.
- Length and indexing are by **Unicode code point** (`stringLength("🎉") // → 1`).
- Comparison is case-sensitive, code-point order.
- `isBlank` treats any Unicode whitespace as blank; `isEmpty` is strictly zero-length.
- `split` with leading/trailing/consecutive delimiters yields empty-string elements.
- `split` with a list of delimiters scans left-to-right, **first match wins**: at each position
  the engine checks each delimiter; if any match, it splits there and resumes scanning after the
  match. When multiple delimiters match at the same position the longest one is chosen (so
  `split("a--b", ["--", "-"])` → `["a", "b"]`, not `["a", "", "b"]`). Overlapping matches at
  later positions are shadowed: `split("abcde", ["bc", "cd"])` → `["a", "de"]` because `"bc"`
  matches at position 1 before the scan reaches the would-be `"cd"` match at position 2. An
  empty delimiter inside the list → `bl.TypeError`; an empty delimiter list → `[s]` (single
  element, no splitting).

`[@test] ../../string_semantics_test.go`

---

## Go implementation (expr extension)

Lives in `expr/string.go`. Shared mechanics in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).

### Value type & host API (exported)

`bl.BlString` is the immutable Go value type that represents a string inside the engine and at the
host-code boundary. Its only field is private (`s`) so callers cannot mutate the underlying
sequence — every operation in the library returns a fresh `bl.BlString`.

The exported surface has three parts:

- **`bl.BlValue` interface methods** — `Type()`, `Equal()`, `bl.String()`, and the unexported
  `isBlValue()` marker — required of every blkit value type so the engine can treat them
  uniformly. `isBlValue()` is the sealing mechanism: because it's unexported, no type outside
  this package can satisfy `bl.BlValue`, preventing host code from inventing fake values that the
  engine would then have to defend against. `bl.String()` doubles as the `fmt.Stringer`
  implementation, so `fmt.Println(s)` produces the underlying text rather than the raw struct.
  `Equal` performs case-sensitive code-point comparison.
- **`String[T StringInput](v T) (bl.BlString, error)`** — the generic host constructor. The
  `StringInput` constraint accepts `string` or `[]byte`. A `string` input is infallible (a Go
  `string` carries no encoding contract, so it can't fail). A `[]byte` input is interpreted as
  UTF-8 and validated; the `error` return fires when the bytes contain invalid UTF-8 sequences
  (raw binary, non-UTF-8 encodings like Latin-1, truncated multi-byte sequences, etc.) — the
  host is responsible for the encoding contract on `[]byte`, and the validation happens at the
  construction boundary rather than producing a string with replacement characters. Other Go
  types are deliberately rejected at compile time: textual representation of numbers,
  booleans, dates, etc. is a formatting decision the host should make explicitly
  (`strconv.Itoa`, `fmt.Sprintf`, `t.Format(...)`), or via the expression-language
  `string(from)` built-in if the conversion belongs inside an expression.
- **`Native()` accessor** — hands the underlying Go `string` back to host code. From there, Go's
  standard library provides all needed operations (comparison via `<`/`==`, length, indexing,
  slicing, etc.), so this is the only accessor required.

```go
// bl.BlString wraps a native Go string. bl.BlString methods count positions and
// length in Unicode code points (not bytes).
type BlString struct{ s string }

// bl.BlValue interface — required by all Bl* value types.
func (BlString) Type() Type { return TypeString }
func (s BlString) Equal(other BlValue) BlValue   // case-sensitive code-point comparison
func (s BlString) String() string
func (BlString) isBlValue() {}

// Host constructor — accepts a Go string (infallible) or a UTF-8 []byte (validated).
// string input never errors; []byte returns an error when the bytes contain invalid UTF-8.
type StringInput interface { string | []byte }
func String[T StringInput](v T) (BlString, error)

// Host accessor (consume an evaluated result).
func (s BlString) Native() string                // underlying Go string

// bl.BlRegex is the precompiled-regex value type — produced by pattern(s) and accepted in the
// pattern slot of matches/replace/extract and as a target type by calendarDrop/calendarKeep.
type BlRegex struct {
    source   string
    compiled *regexp.Regexp // built at construction; never nil for a valid BlRegex
}

// bl.BlValue interface — required by all Bl* value types.
func (BlRegex) Type() Type { return TypeRegex }
func (r BlRegex) Equal(other BlValue) BlValue   // equal iff source strings are equal
func (r BlRegex) String() string                // returns the original source string
func (BlRegex) isBlValue() {}

// Host constructor — compiles immediately; a malformed source returns bl.RegexError.
func Pattern(source string) (BlRegex, error)

// Host accessors (consume an evaluated result).
func (r BlRegex) Source() string                // the original source string
func (r BlRegex) Native() *regexp.Regexp        // the compiled program; do not mutate
```

### Operator implementation functions (unexported)

`expr-lang/expr` has no knowledge of `bl.BlString` and cannot apply Go's native `+`/`<`/etc. to
blkit values. For every operator that should work on two strings, blkit supplies a named Go
function that performs the operation on the underlying Go strings and returns the result wrapped
as a `bl.BlValue`. The connection from operator token to function happens in two steps, neither of
which is unique to `bl.BlString`:

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

So when the parser encounters `a + b` and both operands type-check to `bl.BlString`, the engine
finds `concatStrings` in the `"+"` binding list, sees its signature matches, and dispatches to it.

The return type is `bl.BlValue` so the impls can propagate `bl.BlNull` if the engine ever calls them
with a null operand (the type-checker normally prevents that, but the wider return type makes the
null path explicit).

Equality (`=` / `!=`) is **not** registered as a per-type operator impl. The engine dispatches
`=` / `!=` through the `Equal()` method on the `bl.BlValue` interface, which `bl.BlString` implements
above (case-sensitive code-point comparison). That single dispatch path handles null propagation
and cross-type comparison uniformly.

```go
func concatStrings(a, b BlString) BlValue  // "+"
func ltStrings(a, b BlString) BlValue      // "<"
func leStrings(a, b BlString) BlValue      // "<="
func gtStrings(a, b BlString) BlValue      // ">"
func geStrings(a, b BlString) BlValue      // ">="
// "=" and "!=" go through bl.BlValue.Equal(); see bl.BlString.Equal() above.
```

These are written in clean typed form (`bl.BlString → bl.BlValue`) for readability and unit testing.
The engine cannot consume them at this shape directly — they're wrapped by the `typed1`/`typed2`
adapters at registration time.

### Backing implementations (unexported, suffix `Fn`)

The library and conversion functions are implemented as these typed Go functions. They are
wrapped by `typed1`/`typed2`/`typed3` when registered with the engine in the next section.
Variadic implementations (`substringFn`, `matchesFn`, `replaceFn`, `extractFn`, `padLeadingFn`,
`padTrailingFn`) instead implement the engine's `func(...any) (any, error)` shape
directly because they accept optional arguments and cannot be expressed via a fixed-arity adapter.

```go
// Typed implementations — wrapped by typed1/typed2/typed3 at registration.
func stringLengthFn(s BlString) BlNumber
func substringBeforeFn(s, match BlString) BlString          // not found → ""
func substringAfterFn(s, match BlString) BlString           // not found → ""
func upperCaseFn(s BlString) BlString
func lowerCaseFn(s BlString) BlString
func trimFn(s BlString) BlString
func trimLeadingFn(s BlString) BlString
func trimTrailingFn(s BlString) BlString
func containsFn(s, match BlString) BlBoolean                // overloaded; see calendar.spec.md
func startsWithFn(s, match BlString) BlBoolean
func endsWithFn(s, match BlString) BlBoolean
func isBlankFn(s BlString) BlBoolean
func strIndexOfFn(s, match BlString) BlValue                // not found → Null; list overload in list.spec.md
func strIsEmptyFn(s BlString) BlBoolean                     // list overload in list.spec.md
func charAtFn(s BlString, pos BlNumber) BlString            // out of range → ""
func strReverseFn(s BlString) BlString                      // list overload in list.spec.md
func repeatFn(s BlString, times BlNumber) BlString

// Variadic implementations — handle optional args themselves in expr's raw shape.
func substringFn(args ...any) (any, error)    // 2- or 3-arg
func splitFn(args ...any) (any, error)        // delimiter is BlString or BlList of BlString
func matchesFn(args ...any) (any, error)      // 2- or 3-arg; pattern slot accepts BlString or BlRegex (3-arg form rejects BlRegex)
func replaceFn(args ...any) (any, error)      // 3- or 4-arg; pattern slot accepts BlString or BlRegex (4-arg form rejects BlRegex)
func extractFn(args ...any) (any, error)      // 2- or 3-arg; pattern slot accepts BlString or BlRegex (3-arg form rejects BlRegex)
func patternFn(s BlString) (BlRegex, error)   // ext; compiles s into a BlRegex (RegexError on malformed source)
func padLeadingFn(args ...any) (any, error)   // 2- or 3-arg (optional padChar)
func padTrailingFn(args ...any) (any, error)  // 2- or 3-arg (optional padChar)
// stringJoin's backing impl lives in list.spec.md — see § Backing implementations there.
func stringConvFn(args ...any) (any, error)   // string(from) — accepts any BlValue
```

All implementations are built on the Go standard library; no third-party packages are required
beyond what blkit already pulls in:

- **`unicode/utf8`** for code-point operations — `stringLengthFn` uses
  [`utf8.RuneCountInString`](https://pkg.go.dev/unicode/utf8#RuneCountInString); `charAtFn` and
  `substringFn` walk runes with [`utf8.DecodeRuneInString`](https://pkg.go.dev/unicode/utf8#DecodeRuneInString)
  to convert blkit's 1-indexed code-point positions to byte offsets without allocating an
  intermediate `[]rune`.
- **`unicode.IsSpace`** for whitespace predicates — `trimFn` uses
  [`strings.TrimSpace`](https://pkg.go.dev/strings#TrimSpace) (defined in terms of
  `unicode.IsSpace`); `trimLeadingFn` / `trimTrailingFn` use
  [`strings.TrimLeftFunc`](https://pkg.go.dev/strings#TrimLeftFunc) /
  [`strings.TrimRightFunc`](https://pkg.go.dev/strings#TrimRightFunc) with
  [`unicode.IsSpace`](https://pkg.go.dev/unicode#IsSpace); `isBlankFn` checks
  `strings.IndexFunc(s, notSpace) == -1`.
- **`strings`** for prefix/suffix/contains/case/repeat — `containsFn` →
  [`strings.Contains`](https://pkg.go.dev/strings#Contains), `startsWithFn`/`endsWithFn` →
  [`strings.HasPrefix`](https://pkg.go.dev/strings#HasPrefix) / `HasSuffix`, `upperCaseFn`/
  `lowerCaseFn` → [`strings.ToUpper`](https://pkg.go.dev/strings#ToUpper) / `ToLower` (simple
  per-rune mapping; no locale-aware case folding), `repeatFn` →
  [`strings.Repeat`](https://pkg.go.dev/strings#Repeat).
- **`regexp`** for the regex funcs (`matchesFn`/`replaceFn`/`extractFn`) — compiled via Go's
  [`regexp`](https://pkg.go.dev/regexp) package (RE2 syntax); `matches` anchors with `^(?:…)$`
  before compiling. A bad pattern → `bl.RegexError`. The multi-delimiter `split` form also routes
  through `regexp`: delimiters are sorted longest-first and concatenated with `|` into a single
  alternation, giving the left-to-right / longest-match-wins semantics for free (RE2's
  leftmost-first matching picks the first listed alternative at each position).

`indexOf`/`isEmpty`/`reverse`/`contains` are overloaded with their list/calendar forms; each is
registered once with multiple signatures, in whichever spec owns the canonical entry. Native Go
`string` inputs wrap to `bl.BlString`.

### Registrations (`stringOptions`, unexported)

`stringOptions()` returns the slice of `expr.Option` values the engine consumes during
initialisation to learn about every string-related operator impl and library function. Each entry
is built with `expr.Function(name, impl, typeHints...)`, where:

- `name` is the identifier the parser will recognise in expressions (and that `operatorBindings()`
  references for operator dispatch).
- `impl` must have the signature `func(...any) (any, error)` — that is the only shape
  [`expr-lang/expr`](https://github.com/expr-lang/expr) accepts. The `typed1` / `typed2` /
  `typed3` adapters (defined in [bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go))
  wrap a typed implementation such as `func(bl.BlString, bl.BlString) bl.BlValue` into that shape,
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
        expr.Function("concatStrings", typed2(concatStrings), new(func(bl.BlString, bl.BlString) bl.BlValue)),
        expr.Function("ltStrings",     typed2(ltStrings),     new(func(bl.BlString, bl.BlString) bl.BlValue)),
        expr.Function("leStrings",     typed2(leStrings),     new(func(bl.BlString, bl.BlString) bl.BlValue)),
        expr.Function("gtStrings",     typed2(gtStrings),     new(func(bl.BlString, bl.BlString) bl.BlValue)),
        expr.Function("geStrings",     typed2(geStrings),     new(func(bl.BlString, bl.BlString) bl.BlValue)),
        // = and != dispatch via bl.BlValue.Equal() — no per-type registration

        // library
        expr.Function("stringLength",    typed1(stringLengthFn),    new(func(bl.BlString) bl.BlNumber)),
        expr.Function("substring",       substringFn,               new(func(bl.BlString, bl.BlNumber) bl.BlString),
                                                                    new(func(bl.BlString, bl.BlNumber, bl.BlNumber) bl.BlString)),
        expr.Function("substringBefore", typed2(substringBeforeFn), new(func(bl.BlString, bl.BlString) bl.BlString)),
        expr.Function("substringAfter",  typed2(substringAfterFn),  new(func(bl.BlString, bl.BlString) bl.BlString)),
        expr.Function("upperCase",       typed1(upperCaseFn),       new(func(bl.BlString) bl.BlString)),
        expr.Function("lowerCase",       typed1(lowerCaseFn),       new(func(bl.BlString) bl.BlString)),
        expr.Function("trim",            typed1(trimFn),            new(func(bl.BlString) bl.BlString)),
        expr.Function("trimLeading",     typed1(trimLeadingFn),     new(func(bl.BlString) bl.BlString)),
        expr.Function("trimTrailing",    typed1(trimTrailingFn),    new(func(bl.BlString) bl.BlString)),
        expr.Function("contains",        typed2(containsFn),        new(func(bl.BlString, bl.BlString) bl.BlBoolean)),
        expr.Function("startsWith",      typed2(startsWithFn),      new(func(bl.BlString, bl.BlString) bl.BlBoolean)),
        expr.Function("endsWith",        typed2(endsWithFn),        new(func(bl.BlString, bl.BlString) bl.BlBoolean)),
        expr.Function("matches",         matchesFn,                 new(func(bl.BlString, bl.BlString) bl.BlBoolean),
                                                                    new(func(bl.BlString, bl.BlString, bl.BlString) bl.BlBoolean),
                                                                    new(func(bl.BlString, bl.BlRegex)  bl.BlBoolean)),
        expr.Function("replace",         replaceFn,                 new(func(bl.BlString, bl.BlString, bl.BlString) bl.BlString),
                                                                    new(func(bl.BlString, bl.BlString, bl.BlString, bl.BlString) bl.BlString),
                                                                    new(func(bl.BlString, bl.BlRegex,  bl.BlString) bl.BlString)),
        expr.Function("split",           splitFn,                   new(func(bl.BlString, bl.BlString) bl.BlList),
                                                                    new(func(bl.BlString, bl.BlList) bl.BlList)),
        expr.Function("extract",         extractFn,                 new(func(bl.BlString, bl.BlString) bl.BlList),
                                                                    new(func(bl.BlString, bl.BlString, bl.BlString) bl.BlList),
                                                                    new(func(bl.BlString, bl.BlRegex)  bl.BlList)),
        expr.Function("pattern",         typed1(patternFn),         new(func(bl.BlString) bl.BlRegex)),  // ext
        expr.Function("isBlank",         typed1(isBlankFn),         new(func(bl.BlString) bl.BlBoolean)),
        expr.Function("indexOf",         typed2(strIndexOfFn),      new(func(bl.BlString, bl.BlString) bl.BlValue)),
        expr.Function("isEmpty",         typed1(strIsEmptyFn),      new(func(bl.BlString) bl.BlBoolean)),
        expr.Function("charAt",          typed2(charAtFn),          new(func(bl.BlString, bl.BlNumber) bl.BlString)),
        expr.Function("reverse",         typed1(strReverseFn),      new(func(bl.BlString) bl.BlString)),
        expr.Function("padLeading",      padLeadingFn,              new(func(bl.BlString, bl.BlNumber) bl.BlString),
                                                                    new(func(bl.BlString, bl.BlNumber, bl.BlString) bl.BlString)),
        expr.Function("padTrailing",     padTrailingFn,             new(func(bl.BlString, bl.BlNumber) bl.BlString),
                                                                    new(func(bl.BlString, bl.BlNumber, bl.BlString) bl.BlString)),
        expr.Function("repeat",          typed2(repeatFn),          new(func(bl.BlString, bl.BlNumber) bl.BlString)),
        // stringJoin is registered in list.spec.md (its first arg is a bl.BlList, and it lives
        // there alongside the other list-reducing functions like sum/min/max/zipStringJoin).

        // conversion
        expr.Function("string", stringConvFn, new(func(bl.BlValue) bl.BlString)), // string(from) — any bl.BlValue → bl.BlString
    }
}
```

`[@test] ../../string_test.go`

---

## Edge cases

- `substring` start beyond end → `""`; over-long `length` is clamped.
- `substringBefore`/`substringAfter` with delimiter absent → `""`.
- `indexOf` not found → `null` (not `0`).
- `extract` with no matches → `[]` (not `null`).
- `padLeading`/`padTrailing` with a multi-code-point `padChar` → `bl.TypeError`.
- `repeat` with a negative count → `bl.TypeError`.
- `split("", "")` rules and empty-segment behaviour as above.
