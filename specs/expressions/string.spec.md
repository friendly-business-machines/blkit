---
name: BlString
description: blkit's string type — an immutable Unicode character sequence; extends BlExpr so all operations are deferred and chainable
targets:
  - ../../expr/string.go
---

# BlString

`BlString` is blkit's string type: an immutable sequence of Unicode code points. In literal notation strings are enclosed in double quotes; `BlString` is the runtime value type.

`BlString` extends `BlExpr`. Every instance is a **literal leaf node** in a deferred expression tree. All methods return a new `BlExpr` node without computing anything. Call `.evaluate(context?)` once to materialise the result.

String positions throughout this spec are **1-indexed**. Unicode code points are the unit of length — not bytes, not UTF-16 code units.

```go
type BlString struct { BlExpr }

// Construction is via Bl.String(value). See bl.spec.md.

// Length and classification
func (s *BlString) Length() BlNumber { ... }     // evaluates to BlNumber
func (s *BlString) IsEmpty() BlExpr { ... }      // evaluates to BlBoolean
func (s *BlString) IsBlank() BlExpr { ... }      // evaluates to BlBoolean

// Content access — return BlString for type-safe chaining
func (s *BlString) CharAt(position BlExpr) BlString { ... }
func (s *BlString) Substring(startPosition BlExpr, length *BlExpr) BlString { ... }
func (s *BlString) SubstringBefore(delimiter BlExpr) BlString { ... }
func (s *BlString) SubstringAfter(delimiter BlExpr) BlString { ... }

// Search — return BlExpr (boolean results)
func (s *BlString) Contains(match BlExpr) BlExpr { ... }
func (s *BlString) StartsWith(match BlExpr) BlExpr { ... }
func (s *BlString) EndsWith(match BlExpr) BlExpr { ... }
func (s *BlString) Matches(pattern BlExpr, flags *BlExpr) BlExpr { ... }
func (s *BlString) IndexOf(match BlExpr) BlNumber { ... }     // evaluates to BlNumber (1-indexed position)
func (s *BlString) Extract(pattern BlExpr, flags *BlExpr) BlList { ... }  // evaluates to BlList

// Transformation — return BlString for type-safe chaining
func (s *BlString) UpperCase() BlString { ... }
func (s *BlString) LowerCase() BlString { ... }
func (s *BlString) Trim() BlString { ... }
func (s *BlString) Reverse() BlString { ... }
func (s *BlString) PadStart(length BlExpr, padChar *BlExpr) BlString { ... }
func (s *BlString) PadEnd(length BlExpr, padChar *BlExpr) BlString { ... }
func (s *BlString) Replace(pattern BlExpr, replacement BlExpr, flags *BlExpr) BlString { ... }
func (s *BlString) Split(delimiter BlExpr) BlList { ... }      // evaluates to BlList
func (s *BlString) Concatenate(other BlExpr) BlString { ... }
func (s *BlString) Concat(others ...BlExpr) BlString { ... }
func (s *BlString) Repeat(times BlExpr) BlString { ... }

// Membership
func (s *BlString) In(test BlExpr) BlExpr { ... }

// Comparison
func (s *BlString) Equals(other BlExpr) BlExpr { ... }
func (s *BlString) NotEqual(other BlExpr) BlExpr { ... }

// Eager host-language utilities (call only on a concrete value after .Evaluate())
func (s *BlString) CompareTo(other BlString) int { ... }
func (s *BlString) ToNativeString() string { ... }
func (s *BlString) String() string { ... }
```

---

## Construction

`Bl.String(value)` creates a `BlString` literal from a native host-language string. The value is stored as-is; no escaping or unescaping is performed. Use `Bl.String("")` for the empty string.

```go
Bl.String("hello").Evaluate()
// → BlString("hello")

Bl.String("").Evaluate()
// → BlString("")

Bl.String("").IsEmpty().Evaluate()
// → BlBoolean.TRUE

Bl.String("naïve café").Evaluate()
// → BlString("naïve café")   (Unicode preserved)

Bl.String("line1\nline2").Evaluate()
// → BlString with an embedded newline
```

---

## Length and Classification

### `length`

A deferred property that evaluates to the number of **Unicode code points** in the string. Not the byte count, and not the UTF-16 code unit count.

```go
Bl.String("hello").Length().Evaluate()
// → BlNumber("5")

Bl.String("").Length().Evaluate()
// → BlNumber("0")

Bl.String("café").Length().Evaluate()
// → BlNumber("4")   (é is one code point)

Bl.String("🎉").Length().Evaluate()
// → BlNumber("1")   (emoji is one code point; UTF-16 encodes it as two units, but blkit counts one)

// Guard: reject strings that are too long
Bl.StringVar("username").Length().LessThanOrEqual(Bl.Number(20)).Evaluate(
    map[string]BlExpr{"username": Bl.String("alice")},
)
// → BlBoolean.TRUE
```

### `is_empty()`

Evaluates to `BlBoolean.TRUE` if the string has zero characters.

```go
Bl.String("").IsEmpty().Evaluate()
// → BlBoolean.TRUE

Bl.String(" ").IsEmpty().Evaluate()
// → BlBoolean.FALSE   (a space is a character)

Bl.String("hello").IsEmpty().Evaluate()
// → BlBoolean.FALSE

Bl.StringVar("input").IsEmpty().Evaluate(map[string]BlExpr{"input": Bl.String("")})
// → BlBoolean.TRUE
```

### `is_blank()`

Evaluates to `BlBoolean.TRUE` if the string is empty or contains only whitespace characters (spaces, tabs, newlines, carriage returns, and other Unicode whitespace code points). Distinguishes between "nothing was entered" and "only spaces were entered".

```go
Bl.String("").IsBlank().Evaluate()
// → BlBoolean.TRUE

Bl.String("   ").IsBlank().Evaluate()
// → BlBoolean.TRUE

Bl.String("\t\n").IsBlank().Evaluate()
// → BlBoolean.TRUE

Bl.String("  hello  ").IsBlank().Evaluate()
// → BlBoolean.FALSE

Bl.String("0").IsBlank().Evaluate()
// → BlBoolean.FALSE

// Validate that a required field has meaningful content
Bl.StringVar("notes").IsBlank().Evaluate(map[string]BlExpr{"notes": Bl.String("   ")})
// → BlBoolean.TRUE  (field is effectively empty)
```

---

## Content Access

### `char_at(position)`

Returns a single-character string at the given 1-indexed position. Evaluates to `Bl.String("")` for any out-of-range position (positive or negative).

```go
Bl.String("hello").CharAt(Bl.Number(1)).Evaluate()
// → BlString("h")

Bl.String("hello").CharAt(Bl.Number(5)).Evaluate()
// → BlString("o")

Bl.String("hello").CharAt(Bl.Number(-1)).Evaluate()
// → BlString("o")   (-1 is the last character)

Bl.String("hello").CharAt(Bl.Number(6)).Evaluate()
// → BlString("")    (out of range)

Bl.String("café").CharAt(Bl.Number(4)).Evaluate()
// → BlString("é")   (4th code point)
```

### `substring(start_position, length?)`

Returns a substring starting at `start_position` (1-indexed). If `length` is provided, at most that many characters are returned. If `start_position` is negative, it counts back from the end of the string (`-1` is the last character). If `length` is omitted, the substring runs to the end of the string.

```go
Bl.String("hello world").Substring(Bl.Number(7), nil).Evaluate()
// → BlString("world")

Bl.String("hello world").Substring(Bl.Number(1), Bl.Number(5)).Evaluate()
// → BlString("hello")

Bl.String("hello world").Substring(Bl.Number(7), Bl.Number(3)).Evaluate()
// → BlString("wor")

Bl.String("hello").Substring(Bl.Number(-3), nil).Evaluate()
// → BlString("llo")   (-3 = 3rd from end = position 3)

Bl.String("hello").Substring(Bl.Number(3), Bl.Number(100)).Evaluate()
// → BlString("llo")   (length clamped to available characters)

Bl.String("hello").Substring(Bl.Number(10), nil).Evaluate()
// → BlString("")      (start beyond end → empty)
```

### `substring_before(delimiter)`

Returns the portion of the string that precedes the first occurrence of `delimiter`. If `delimiter` is not found, evaluates to `Bl.String("")`. If `delimiter` is an empty string, also evaluates to `Bl.String("")`.

```go
Bl.String("hello world").SubstringBefore(Bl.String(" ")).Evaluate()
// → BlString("hello")

Bl.String("2025-03-28").SubstringBefore(Bl.String("-")).Evaluate()
// → BlString("2025")

Bl.String("user@example.com").SubstringBefore(Bl.String("@")).Evaluate()
// → BlString("user")

Bl.String("no-match-here").SubstringBefore(Bl.String("x")).Evaluate()
// → BlString("")    (delimiter not found)

Bl.String("a::b::c").SubstringBefore(Bl.String("::")).Evaluate()
// → BlString("a")   (stops at first occurrence)

// Extracting a file name without extension
Bl.StringVar("filename").SubstringBefore(Bl.String(".")).Evaluate(
    map[string]BlExpr{"filename": Bl.String("report.pdf")},
)
// → BlString("report")
```

### `substring_after(delimiter)`

Returns the portion of the string that follows the first occurrence of `delimiter`. If `delimiter` is not found, evaluates to `Bl.String("")`. If `delimiter` is an empty string, evaluates to the full string.

```go
Bl.String("hello world").SubstringAfter(Bl.String(" ")).Evaluate()
// → BlString("world")

Bl.String("2025-03-28").SubstringAfter(Bl.String("-")).Evaluate()
// → BlString("03-28")   (everything after the first "-")

Bl.String("user@example.com").SubstringAfter(Bl.String("@")).Evaluate()
// → BlString("example.com")

Bl.String("no-match-here").SubstringAfter(Bl.String("x")).Evaluate()
// → BlString("")    (delimiter not found)

Bl.String("a::b::c").SubstringAfter(Bl.String("::")).Evaluate()
// → BlString("b::c")   (everything after the first occurrence)

// Extracting a file extension
Bl.StringVar("filename").SubstringAfter(Bl.String(".")).Evaluate(
    map[string]BlExpr{"filename": Bl.String("report.pdf")},
)
// → BlString("pdf")
```

---

## Search

### `contains(match)`

Evaluates to `BlBoolean.TRUE` if `match` appears anywhere within `self`. Case-sensitive.

```go
Bl.String("hello world").Contains(Bl.String("world")).Evaluate()
// → BlBoolean.TRUE

Bl.String("hello world").Contains(Bl.String("World")).Evaluate()
// → BlBoolean.FALSE   (case-sensitive)

Bl.String("hello world").Contains(Bl.String("")).Evaluate()
// → BlBoolean.TRUE    (empty string is contained in any string)

Bl.String("abc").Contains(Bl.String("abcd")).Evaluate()
// → BlBoolean.FALSE

Bl.StringVar("description").Contains(Bl.String("urgent")).Evaluate(
    map[string]BlExpr{"description": Bl.String("This is an urgent request")},
)
// → BlBoolean.TRUE
```

### `starts_with(match)`

Evaluates to `BlBoolean.TRUE` if `self` begins with `match`. Case-sensitive.

```go
Bl.String("hello world").StartsWith(Bl.String("hello")).Evaluate()
// → BlBoolean.TRUE

Bl.String("hello world").StartsWith(Bl.String("world")).Evaluate()
// → BlBoolean.FALSE

Bl.String("hello").StartsWith(Bl.String("")).Evaluate()
// → BlBoolean.TRUE    (every string starts with the empty string)

Bl.String("hello").StartsWith(Bl.String("hello world")).Evaluate()
// → BlBoolean.FALSE   (match is longer than self)

Bl.StringVar("code").StartsWith(Bl.String("ERR-")).Evaluate(
    map[string]BlExpr{"code": Bl.String("ERR-404")},
)
// → BlBoolean.TRUE
```

### `ends_with(match)`

Evaluates to `BlBoolean.TRUE` if `self` ends with `match`. Case-sensitive.

```go
Bl.String("hello world").EndsWith(Bl.String("world")).Evaluate()
// → BlBoolean.TRUE

Bl.String("hello world").EndsWith(Bl.String("hello")).Evaluate()
// → BlBoolean.FALSE

Bl.String("report.pdf").EndsWith(Bl.String(".pdf")).Evaluate()
// → BlBoolean.TRUE

Bl.String("hello").EndsWith(Bl.String("")).Evaluate()
// → BlBoolean.TRUE

Bl.StringVar("filename").EndsWith(Bl.String(".csv")).Evaluate(
    map[string]BlExpr{"filename": Bl.String("export.csv")},
)
// → BlBoolean.TRUE
```

### `matches(pattern, flags?)`

Evaluates to `BlBoolean.TRUE` if the **entire string** matches the XML Schema regex `pattern`. Use `".*"` prefix/suffix to test for a partial match anywhere in the string (or use `contains()` for a plain substring check).

`flags` is a string of flag characters: `"i"` (case-insensitive), `"m"` (multiline), `"s"` (dot matches newline), `"x"` (ignore whitespace in pattern). A malformed pattern raises `BlRegexError` at evaluation time.

```go
Bl.String("abc123").Matches(Bl.String("[a-z]+[0-9]+"), nil).Evaluate()
// → BlBoolean.TRUE

Bl.String("ABC123").Matches(Bl.String("[a-z]+[0-9]+"), nil).Evaluate()
// → BlBoolean.FALSE   (case-sensitive)

Bl.String("ABC123").Matches(Bl.String("[a-z]+[0-9]+"), Bl.String("i")).Evaluate()
// → BlBoolean.TRUE    (case-insensitive flag)

Bl.String("2025-03-28").Matches(Bl.String(`\d{4}-\d{2}-\d{2}`), nil).Evaluate()
// → BlBoolean.TRUE

Bl.String("hello world").Matches(Bl.String("hello"), nil).Evaluate()
// → BlBoolean.FALSE   (pattern must match the whole string)

Bl.String("hello world").Matches(Bl.String(".*hello.*"), nil).Evaluate()
// → BlBoolean.TRUE    (use .* to allow prefix/suffix)

// Validate a UK postcode format
Bl.StringVar("postcode").Matches(Bl.String("[A-Z]{1,2}[0-9][0-9A-Z]? [0-9][A-Z]{2}"), Bl.String("i")).Evaluate(
    map[string]BlExpr{"postcode": Bl.String("SW1A 1AA")},
)
// → BlBoolean.TRUE
```

### `index_of(match)`

Returns a deferred expression that evaluates to the 1-indexed position of the **first** occurrence of `match` in `self`, or `BlNull` if not found.

```go
Bl.String("hello world").IndexOf(Bl.String("world")).Evaluate()
// → BlNumber("7")

Bl.String("hello world").IndexOf(Bl.String("o")).Evaluate()
// → BlNumber("5")   (first "o", not second)

Bl.String("hello world").IndexOf(Bl.String("xyz")).Evaluate()
// → BlNull.INSTANCE

Bl.String("hello").IndexOf(Bl.String("")).Evaluate()
// → BlNumber("1")   (empty string is found at position 1)

Bl.String("aababc").IndexOf(Bl.String("ab")).Evaluate()
// → BlNumber("2")   (first occurrence at position 2)
```

### `extract(pattern, flags?)`

Returns a deferred expression that evaluates to a `BlList` of all non-overlapping substrings that match `pattern`. Uses the XML Schema regex dialect. If no matches are found, evaluates to an empty `BlList`. A malformed pattern raises `BlRegexError` at evaluation time.

`flags` follows the same conventions as `matches()`.

Unlike `matches()`, `extract()` does not require the pattern to match the entire string — it finds all matching substrings within `self`.

```go
Bl.String("order-123 and order-456").Extract(Bl.String("order-[0-9]+"), nil).Evaluate()
// → BlList([BlString("order-123"), BlString("order-456")])

Bl.String("The price is $12.50 and $7.99").Extract(Bl.String(`\$[0-9]+\.[0-9]{2}`), nil).Evaluate()
// → BlList([BlString("$12.50"), BlString("$7.99")])

Bl.String("hello world").Extract(Bl.String("[0-9]+"), nil).Evaluate()
// → BlList([])   (no matches)

Bl.String("aababc").Extract(Bl.String("ab"), nil).Evaluate()
// → BlList([BlString("ab"), BlString("ab")])

Bl.String("Hello World").Extract(Bl.String("[a-z]+"), Bl.String("i")).Evaluate()
// → BlList([BlString("Hello"), BlString("World")])

// Extract all hashtags from a comment
Bl.StringVar("comment").Extract(Bl.String("#[A-Za-z0-9_]+"), nil).Evaluate(
    map[string]BlExpr{"comment": Bl.String("Great work #teamA on the #Q4release!")},
)
// → BlList([BlString("#teamA"), BlString("#Q4release")])
```

---

## Transformation

### `upper_case()` / `lower_case()`

Convert the string to all uppercase or all lowercase using Unicode case folding. Locale-independent.

```go
Bl.String("Hello World").UpperCase().Evaluate()
// → BlString("HELLO WORLD")

Bl.String("Hello World").LowerCase().Evaluate()
// → BlString("hello world")

Bl.String("café").UpperCase().Evaluate()
// → BlString("CAFÉ")

Bl.String("STRASSE").LowerCase().Evaluate()
// → BlString("strasse")

// Case-insensitive comparison
Bl.StringVar("status").LowerCase().Equals(Bl.String("active")).Evaluate(
    map[string]BlExpr{"status": Bl.String("ACTIVE")},
)
// → BlBoolean.TRUE
```

### `trim()`

Removes leading and trailing whitespace (spaces, tabs, newlines, and other Unicode whitespace). Does not remove whitespace from the middle of the string.

```go
Bl.String("  hello  ").Trim().Evaluate()
// → BlString("hello")

Bl.String("\t hello \n").Trim().Evaluate()
// → BlString("hello")

Bl.String("hello world").Trim().Evaluate()
// → BlString("hello world")   (internal space unchanged)

Bl.String("   ").Trim().Evaluate()
// → BlString("")

Bl.StringVar("input").Trim().IsEmpty().Evaluate(map[string]BlExpr{"input": Bl.String("   ")})
// → BlBoolean.TRUE
```

### `reverse()`

Returns the string with its Unicode code points in reverse order.

```go
Bl.String("hello").Reverse().Evaluate()
// → BlString("olleh")

Bl.String("abcde").Reverse().Evaluate()
// → BlString("edcba")

Bl.String("a").Reverse().Evaluate()
// → BlString("a")

Bl.String("").Reverse().Evaluate()
// → BlString("")

Bl.String("racecar").Reverse().Equals(Bl.String("racecar")).Evaluate()
// → BlBoolean.TRUE   (palindrome check)

// Check palindrome via chain
Bl.StringVar("word").Reverse().Equals(Bl.StringVar("word")).Evaluate(
    map[string]BlExpr{"word": Bl.String("level")},
)
// → BlBoolean.TRUE
```

### `pad_start(length, pad_char?)`

Pads the **beginning** of the string with `pad_char` until the total string length reaches `length` code points. If the string is already at or longer than `length`, it is returned unchanged. If `pad_char` is omitted, a single space (`" "`) is used. `pad_char` must be exactly one code point; providing a multi-character string raises `BlTypeError` at evaluation time.

```go
Bl.String("42").PadStart(Bl.Number(5), nil).Evaluate()
// → BlString("   42")   (3 spaces prepended)

Bl.String("42").PadStart(Bl.Number(5), Bl.String("0")).Evaluate()
// → BlString("00042")

Bl.String("hello").PadStart(Bl.Number(3), nil).Evaluate()
// → BlString("hello")   (already longer than 3; unchanged)

Bl.String("hello").PadStart(Bl.Number(5), nil).Evaluate()
// → BlString("hello")   (already exactly 5; unchanged)

Bl.String("7").PadStart(Bl.Number(4), Bl.String("0")).Evaluate()
// → BlString("0007")

// Format an invoice number to 8 digits
Bl.StringVar("invoice_id").PadStart(Bl.Number(8), Bl.String("0")).Evaluate(
    map[string]BlExpr{"invoice_id": Bl.String("1234")},
)
// → BlString("00001234")
```

### `pad_end(length, pad_char?)`

Pads the **end** of the string with `pad_char` until the total string length reaches `length` code points. Same rules as `pad_start` for the default and validation of `pad_char`.

```go
Bl.String("hi").PadEnd(Bl.Number(5), nil).Evaluate()
// → BlString("hi   ")   (3 spaces appended)

Bl.String("hi").PadEnd(Bl.Number(5), Bl.String("-")).Evaluate()
// → BlString("hi---")

Bl.String("hello").PadEnd(Bl.Number(3), nil).Evaluate()
// → BlString("hello")   (already longer than 3; unchanged)

Bl.String("name").PadEnd(Bl.Number(10), Bl.String(".")).Evaluate()
// → BlString("name......")

// Align table columns
Bl.StringVar("label").PadEnd(Bl.Number(20), Bl.String(" ")).Evaluate(
    map[string]BlExpr{"label": Bl.String("Total")},
)
// → BlString("Total               ")
```

### `replace(pattern, replacement, flags?)`

Returns the string with all occurrences of `pattern` replaced by `replacement`. `pattern` is an XML Schema regex. `replacement` may reference captured groups using `$1`, `$2`, etc. (XML Schema substitution syntax). `flags` follows the same conventions as `matches()`.

```go
Bl.String("hello world").Replace(Bl.String("world"), Bl.String("there"), nil).Evaluate()
// → BlString("hello there")

Bl.String("aababc").Replace(Bl.String("a"), Bl.String("X"), nil).Evaluate()
// → BlString("XXbXbc")   (all occurrences replaced)

Bl.String("2025-03-28").Replace(Bl.String("-"), Bl.String("/"), nil).Evaluate()
// → BlString("2025/03/28")

Bl.String("Hello World").Replace(Bl.String("[aeiou]"), Bl.String("*"), Bl.String("i")).Evaluate()
// → BlString("H*ll* W*rld")   (case-insensitive vowel replacement)

// Normalise whitespace: replace multiple spaces with one
Bl.StringVar("text").Replace(Bl.String("  +"), Bl.String(" "), nil).Evaluate(
    map[string]BlExpr{"text": Bl.String("too   many   spaces")},
)
// → BlString("too many spaces")
```

### `split(delimiter)`

Returns a deferred expression that evaluates to a `BlList` of `BlString` parts, split on every occurrence of `delimiter`. The delimiter itself is not included in any part. If `delimiter` is an empty string, splits on every character boundary, returning a list of single-character strings.

```go
Bl.String("a,b,c").Split(Bl.String(",")).Evaluate()
// → BlList([BlString("a"), BlString("b"), BlString("c")])

Bl.String("hello world").Split(Bl.String(" ")).Evaluate()
// → BlList([BlString("hello"), BlString("world")])

Bl.String("one::two::three").Split(Bl.String("::")).Evaluate()
// → BlList([BlString("one"), BlString("two"), BlString("three")])

Bl.String("abc").Split(Bl.String("")).Evaluate()
// → BlList([BlString("a"), BlString("b"), BlString("c")])

Bl.String("no-delim").Split(Bl.String(",")).Evaluate()
// → BlList([BlString("no-delim")])   (single-element list)

Bl.String(",a,,b,").Split(Bl.String(",")).Evaluate()
// → BlList([BlString(""), BlString("a"), BlString(""), BlString("b"), BlString("")])
// (leading/trailing/consecutive delimiters produce empty strings)
```

### `concatenate(other)` / `concat(*others)`

`concatenate(other)` appends a single string expression.

`concat(*others)` is a variadic convenience: it appends any number of string expressions in one call, avoiding deeply nested `concatenate()` chains.

Both operands and all arguments must evaluate to `BlString`; providing a non-string type evaluates to `BlNull`.

```go
Bl.String("hello").Concatenate(Bl.String(" world")).Evaluate()
// → BlString("hello world")

Bl.String("Hello").Concat(Bl.String(", "), Bl.StringVar("name"), Bl.String("!")).Evaluate(
    map[string]BlExpr{"name": Bl.String("Alice")},
)
// → BlString("Hello, Alice!")

// Building a full address in one chain
Bl.StringVar("street").Concat(
    Bl.String(", "),
    Bl.StringVar("city"),
    Bl.String(", "),
    Bl.StringVar("country"),
).Evaluate(map[string]BlExpr{
    "street":  Bl.String("10 Downing St"),
    "city":    Bl.String("London"),
    "country": Bl.String("UK"),
})
// → BlString("10 Downing St, London, UK")

// Concatenate() — single operand
Bl.StringVar("first").Concatenate(Bl.String(" ")).Concatenate(Bl.StringVar("last")).Evaluate(
    map[string]BlExpr{"first": Bl.String("Jane"), "last": Bl.String("Doe")},
)
// → BlString("Jane Doe")
```

### `repeat(times)`

Returns the string repeated `times` times. `times` must be a non-negative integer; zero returns `Bl.String("")`. A negative count raises `BlTypeError` at evaluation time.

```go
Bl.String("ab").Repeat(Bl.Number(3)).Evaluate()
// → BlString("ababab")

Bl.String("-").Repeat(Bl.Number(20)).Evaluate()
// → BlString("--------------------")

Bl.String("hello").Repeat(Bl.Number(1)).Evaluate()
// → BlString("hello")

Bl.String("x").Repeat(Bl.Number(0)).Evaluate()
// → BlString("")

// Generate a separator line of variable length
Bl.String("=").Repeat(Bl.StringVar("width")).Evaluate(map[string]BlExpr{"width": Bl.Number(10)})
// → BlString("==========")
```

---

## Membership

### `in_(test)`

Applies a membership check to `self`. Inherited from `BlExpr` and listed here for discoverability. `test` may be a `BlList`, `BlRange`, or `BlCalendar`.

```go
Bl.String("active").In(Bl.List(Bl.String("active"), Bl.String("pending"))).Evaluate()
// → BlBoolean.TRUE

Bl.String("archived").In(Bl.List(Bl.String("active"), Bl.String("pending"))).Evaluate()
// → BlBoolean.FALSE

Bl.String("b").Equals(Bl.String("a")).Not().Evaluate()
// → BlBoolean.TRUE

Bl.StringVar("status").In(Bl.List(
    Bl.String("active"), Bl.String("pending"), Bl.String("review"),
)).Evaluate(map[string]BlExpr{"status": Bl.String("review")})
// → BlBoolean.TRUE
```

---

## Comparison

String comparison is case-sensitive, Unicode code point order. Two `BlString`s are equal if and only if they contain the same sequence of code points.

```go
Bl.String("hello").Equals(Bl.String("hello")).Evaluate()
// → BlBoolean.TRUE

Bl.String("hello").Equals(Bl.String("Hello")).Evaluate()
// → BlBoolean.FALSE   (case-sensitive)

Bl.String("abc").NotEqual(Bl.String("abd")).Evaluate()
// → BlBoolean.TRUE

Bl.String("").Equals(Bl.String("")).Evaluate()
// → BlBoolean.TRUE

Bl.StringVar("country").Equals(Bl.String("GB")).Evaluate(
    map[string]BlExpr{"country": Bl.String("GB")},
)
// → BlBoolean.TRUE
```

---

## Eager Host-Language Utilities

These methods must only be called on a **concrete** `BlString` value — the result of `.Evaluate()`.

### `compare_to(other)`

Returns `-1` if `self` precedes `other` in lexicographic Unicode code point order, `0` if equal, `1` if `self` follows. Useful for implementing native sort comparators.

```go
Bl.String("apple").Evaluate().CompareTo(Bl.String("banana"))   // → -1
Bl.String("banana").Evaluate().CompareTo(Bl.String("banana"))  // → 0
Bl.String("cherry").Evaluate().CompareTo(Bl.String("banana"))  // → 1
```

### `to_native_string()` / `__str__()`

Returns the raw host-language string value. `String()` is the standard string representation and returns the same value.

```go
Bl.String("hello").Evaluate().ToNativeString()   // → "hello"  (Go string)
Bl.String("world").Evaluate().String()           // → "world"
```

---

## Regex Dialect

`matches()`, `replace()`, and `extract()` all use the **XML Schema regex dialect** — not PCRE:

- No lookahead or lookbehind assertions.
- Character class syntax follows XML Schema: `\p{L}` for Unicode letters, `\p{N}` for numeric, `\p{Z}` for separators.
- Supported flags: `"i"` (case-insensitive), `"m"` (multiline `^`/`$`), `"s"` (dot matches newline), `"x"` (ignore unescaped whitespace in pattern).
- A malformed pattern raises `BlRegexError` at evaluation time (not construction time, because the pattern may itself be a deferred expression).

---

## Edge Cases

- `substring()` with `start_position` beyond the string length evaluates to `Bl.String("")`.
- `substring()` with a `length` that extends beyond the end is silently clamped to the available characters.
- `substring_before()` and `substring_after()` with a delimiter not present in `self` evaluate to `Bl.String("")`.
- `split()` with leading, trailing, or consecutive delimiters produces empty-string elements in the result list.
- `pad_start()` and `pad_end()` with a `pad_char` that is not exactly one code point raise `BlTypeError` at evaluation time.
- `repeat()` with a negative count raises `BlTypeError` at evaluation time.
- `extract()` returns an empty `BlList` (not `BlNull`) when no matches are found.
- `index_of()` returns `BlNull` (not `0`) when the match is not found.
- `concat()` with zero additional arguments evaluates to `self` unchanged.
- `is_blank()` considers any Unicode whitespace code point (not just ASCII space) as whitespace.
