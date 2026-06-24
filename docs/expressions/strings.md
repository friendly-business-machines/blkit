# Strings

> Working with text — concatenation, substring, search and replace, case
> conversion, and formatting functions.

A blkit `string` is an immutable sequence of Unicode code points. Everything
about strings is measured and indexed by **code point** — not by byte, and not
by UTF-16 unit — so `"🎉"` is one character long even though it occupies four
bytes in UTF-8. Positions are **1-indexed**: the first character is at position
`1`, not `0`.

This page covers how to write string literals, the operators that work on
strings, the full built-in function library, and the regex dialect those
functions share.

## Literals

A string literal is delimited by double quotes. The expression text is
interpreted as UTF-8, so non-ASCII characters can be written directly inside a
literal:

```
// expression-language
"hello"               // → hello
"naïve café"          // → naïve café
"🎉"                  // → 🎉
```

The supported escape sequences are:

| Escape | Result |
|---|---|
| `\\` | backslash `\` |
| `\"` | double quote `"` |
| `\n` | newline (U+000A) |
| `\r` | carriage return (U+000D) |
| `\t` | tab (U+0009) |
| `\uXXXX` | code point `U+XXXX` (exactly 4 hex digits; BMP only) |
| `\U{XXXXXX}` | code point by hex value (1–6 hex digits, for any code point including beyond U+FFFF) |

Any other `\x` sequence is a parse error. Forward slashes (`/`) are not special
and never need escaping.

```
// expression-language
"line1\nline2"        // → line1
                      //   line2
"quote: \""           // → quote: "
"path: C:\\tmp"       // → path: C:\tmp
"é"              // → é
"\U{1F389}"           // → 🎉
```

Because length is measured in code points, every escape contributes exactly
**one** code point regardless of how many source characters it spans, and a
directly-typed non-ASCII character behaves identically to its escaped form —
`"é"` and `"é"` are the same one-character value:

```
// expression-language
stringLength("hello")          // → 5
stringLength("a\nb")           // → 3    (\n is one code point)
stringLength("\\")             // → 1    (one backslash)
stringLength("\"")             // → 1    (one double quote)
stringLength("é")         // → 1    (U+00E9 = é)
stringLength("é")              // → 1    (typed directly — same value)
stringLength("\U{1F389}")      // → 1    (one supplementary code point)
stringLength("🎉a")            // → 2
```

## Operators

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `+` | concatenation (string-only) | `"foo" + "bar"` | `"foobar"` |
| `=` `!=` | equality (case-sensitive, code-point comparison) | `"a" = "A"` | `false` |
| `in` | membership in a list | `"active" in ["active", "pending"]` | `true` |

Equality is case-sensitive and compares by code point.

Concatenation with `+` is **string-only**. To join a non-string value, convert
it first with `string(...)`:

```
// expression-language
"order-" + string(123)        // → "order-123"
```

## Built-in functions

blkit's string functions. All
positions are 1-indexed, and all functions return a new string — strings are
immutable.

| Function | Example | Result |
|---|---|---|
| `stringLength(s)` | `stringLength("foo")` | `3` |
| `substring(s, start[, length])` | `substring("foobar", 3, 2)` | `"ob"` (negative `start` counts from the end) |
| `substringBefore(s, match)` | `substringBefore("a-b", "-")` | `"a"` |
| `substringAfter(s, match)` | `substringAfter("a-b", "-")` | `"b"` |
| `upperCase(s)` | `upperCase("aBc")` | `"ABC"` |
| `lowerCase(s)` | `lowerCase("aBc")` | `"abc"` |
| `trim(s)` | `trim("  hi  ")` | `"hi"` |
| `trimLeading(s)` | `trimLeading("  hi  ")` | `"hi  "` |
| `trimTrailing(s)` | `trimTrailing("  hi  ")` | `"  hi"` |
| `contains(s, match)` | `contains("foobar", "oo")` | `true` (case-sensitive substring test) |
| `startsWith(s, match)` | `startsWith("foo", "fo")` | `true` |
| `endsWith(s, match)` | `endsWith("foo", "oo")` | `true` |
| `matches(s, pattern[, flags])` | `matches("ABC", "[a-z]+", "i")` | `true` (whole-string match) |
| `replace(s, pattern, repl[, flags])` | `replace("a-b", "-", "/")` | `"a/b"` (`$1` group references) |
| `split(s, delimiter)` | `split("a,b,c", ",")` | `["a", "b", "c"]` |
| `split(s, delimiters)` | `split("a,b;c", [",", ";"])` | `["a", "b", "c"]` (list-of-delimiters overload) |
| `extract(s, pattern[, flags])` | `extract("id 12, 34", "[0-9]+")` | `["12", "34"]` |
| `isBlank(s)` | `isBlank("  ")` | `true` (empty or all-whitespace) |
| `isEmpty(s)` | `isEmpty("")` | `true` (strictly zero length) |
| `indexOf(s, match)` | `indexOf("hello", "l")` | `3` (first position) |
| `charAt(s, position)` | `charAt("hello", 1)` | `"h"` |
| `reverse(s)` | `reverse("abc")` | `"cba"` |
| `padLeading(s, length[, padChar])` | `padLeading("7", 4, "0")` | `"0007"` |
| `padTrailing(s, length[, padChar])` | `padTrailing("hi", 5, ".")` | `"hi..."` |
| `repeat(s, times)` | `repeat("ab", 3)` | `"ababab"` |
| `string(from)` | `string(123)` | `"123"` (any value → string conversion) |

To join the elements of a list into one string there is
`stringJoin(list[, delimiter[, prefix, suffix]])` — for example
`stringJoin(["a", "b"], ", ")` → `"a, b"`. It takes a list as its first
argument, so it is documented alongside the other list-reducing functions in
[Lists](lists.md).

### Inspecting and slicing

`stringLength`, `charAt`, `indexOf`, `contains`, `startsWith`, `endsWith`,
`isEmpty`, and `isBlank` inspect a string without changing it. `substring`,
`substringBefore`, `substringAfter`, and `reverse` carve out a piece of it.

```
// expression-language
substring("foobar", 3)         // → "obar"   (no length → to the end)
substring("foobar", 3, 2)      // → "ob"
substring("foobar", -2)        // → "ar"     (negative start counts from the end)
substringBefore("file.txt", ".")   // → "file"
substringAfter("file.txt", ".")    // → "txt"
charAt("hello", 1)             // → "h"
indexOf("hello", "l")          // → 3        (first occurrence)
reverse("abc")                 // → "cba"
```

### Case, trimming, and padding

```
// expression-language
upperCase("aBc")               // → "ABC"
lowerCase("aBc")               // → "abc"
trim("  hi  ")                 // → "hi"
trimLeading("  hi  ")          // → "hi  "
trimTrailing("  hi  ")         // → "  hi"
padLeading("7", 4, "0")        // → "0007"
padTrailing("hi", 5, ".")      // → "hi..."
repeat("ab", 3)                // → "ababab"
```

`padLeading` / `padTrailing` default to a space when `padChar` is omitted. The
pad character must be a single code point.

### Splitting and joining

`split` divides a string on a delimiter and returns a list:

```
// expression-language
split("a,b,c", ",")            // → ["a", "b", "c"]
split("a,b;c", [",", ";"])     // → ["a", "b", "c"]   (list-of-delimiters overload)
```

For the list-of-delimiters form the engine scans left-to-right and the **first
match wins**; where several delimiters match at one position the longest is
chosen:

```
// expression-language
split("a--b", ["--", "-"])     // → ["a", "b"]        (longest match at a position wins)
split("abcde", ["bc", "cd"])   // → ["a", "de"]       ("bc" matches first, shadowing "cd")
```

## Regex dialect

Three functions accept a regular expression in their pattern slot: `matches`,
`replace`, and `extract`. They share the **RE2 syntax** of Go's standard
[`regexp`](https://pkg.go.dev/regexp/syntax) package:

- No lookahead, lookbehind, or backreferences (these are outside RE2 by design).
- Shorthand classes: `\d` (ASCII digit `[0-9]`), `\w` (ASCII word character
  `[A-Za-z0-9_]`), `\s` (whitespace), and the negations `\D`, `\W`, `\S`. For
  Unicode-aware letters and digits use the property classes below.
- Unicode property classes: `\p{L}` (letter), `\p{N}` (number), `\p{Zs}` (space
  separator), and so on.
- POSIX classes inside `[…]`: `[[:alpha:]]`, `[[:digit:]]`, `[[:space:]]`, …
- Flags: `"i"` case-insensitive, `"m"` multiline (`^`/`$` match line
  boundaries), `"s"` dot-matches-newline. These map to RE2's inline flag groups
  `(?i)` / `(?m)` / `(?s)`, which you may also write directly inside the pattern.
- `matches` requires the **whole string** to match — the engine anchors the
  pattern with `^(?:…)$` internally. Use `contains` or `extract` for partial
  matching.
- Replacement strings use `$1`, `$2`, … for numbered groups and `${name}` for
  named groups.
- A malformed pattern produces a `bl.RegexError` at evaluation time.

### Backslashes in the literal

A regex metacharacter beginning with `\` must be escaped at the string-literal
layer: write `"\\d"` to pass the pattern `\d` to the regex engine, `"\\w"` for
`\w`, `"\\."` for a literal dot, and so on. Inline-flag groups like `(?i)` need
no escaping.

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

Remember that `contains` is a literal substring test, not a regex. For
partial-string regex matching use `extract` or widen the pattern:
`matches(s, ".*pat.*")`.

## Precompiled patterns

`pattern(s)` compiles a regex source string into a first-class
`bl.BlRegex` value. The dialect is identical to the inline-string form; a
malformed source raises `bl.RegexError` at the `pattern(...)` call site rather
than at the use site. All three regex built-ins accept either a string source or
a `bl.BlRegex` in the pattern slot:

```
// expression-language — inline pattern(...) builds a bl.BlRegex used right away.
matches("alice@example.com",
    pattern("[\\w.+-]+@[\\w-]+\\.[\\w.-]+"))                      // → true
extract("a@b.com, c@d.org",
    pattern("[\\w.+-]+@[\\w-]+\\.[\\w.-]+"))                      // → ["a@b.com", "c@d.org"]
matches("Hi there", pattern("(?i)^hi.*"))                         // → true   (case-insensitive flag inline)
```

`pattern(s)` takes only the source string — no separate flags argument. Any
flags must be written inline as `(?i)` / `(?m)` / `(?s)` groups inside the
source.

## Strings from Go

Host Go code builds `string` values with the `bl.String` constructor (a Go
`string` or valid-UTF-8 `[]byte`) and reads a result back with `Native()`. See
[Values from Go](values-from-go.md) for the full host-side story.
