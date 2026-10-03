# Spec Delta

## ADDED Requirements

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
Typed `.bl` signatures SHALL continue to declare their normal output types. Generated Rust calls containing runtime-fallible string expressions MAY return `Result<Output, String>` instead of plain `Output`; callers SHALL receive an error for invalid runtime string operations instead of a silent fallback or process panic. Existing valid `.bl` programs without fallible string expressions SHALL retain their behavior.

#### Scenario: Dynamic invalid regex in a graph
- **WHEN** a process or decision evaluates `matches(input, pattern)` with an invalid runtime `pattern`
- **THEN** its execution SHALL report an error rather than returning `false`, `[]`, or crashing the process
