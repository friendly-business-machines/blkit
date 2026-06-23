# Lists

> Ordered collections and the comprehensions (for, some, every) and functions
> that operate over them.

A **list** is blkit's ordered, immutable, heterogeneous collection — the value
you get from a literal like `[1, 2, 3]`, from a filter, or from a comprehension.
Lists keep their elements in order, may mix types, and are never mutated in
place: every operation returns a fresh list. This page is about *writing*
expressions over lists — how to build them, index into them, iterate with
`for`/`some`/`every`, and reach for the built-in function library. The Go value
type backing a list is `bl.BlList`.

One thing to fix in your mind up front: **indexing is 1-based**, and negative
indexes count from the end. There is no element zero.

## Literals

A list literal is written with square brackets and comma-separated elements.
Elements may be any expression, and they may mix types freely.

```
// expression-language
[1, 2, 3, 4]                          // a four-element list
[1, "two", true, null]                // heterogeneous — perfectly legal
[]                                    // the empty list
[{name: "A"}, {name: "B"}]            // a list of dictionaries
```

## Indexing

Index into a list with `list[n]`. Positions start at `1`; a negative index
counts back from the end. An out-of-range index is not an error — it evaluates
to `null`.

```
// expression-language
[1, 2, 3, 4][1]                       // → 1     (first element, 1-based)
[1, 2, 3, 4][-1]                      // → 4     (last element)
[1, 2, 3, 4][-2]                      // → 3     (second from end)
[1, 2, 3, 4][10]                      // → null  (out of range)
[1, 2, 3, 4][0]                       // → null  (there is no element 0)
```

## Filtering

When the bracket contains a *predicate* rather than a number, you get a filter:
each element is tested, and the elements for which the predicate is true are
returned as a new list.

Inside the `list[predicate]` form, the variable name **`item`** is *magic* — the
engine implicitly binds it to the element currently being tested, so you don't
have to declare or name it:

```
// expression-language
[1, 2, 3, 4][item > 2]                // → [3, 4]
[1, 2, 3, 4][item mod 2 = 0]          // → [2, 4]
```

`item` is only magic inside this filter shorthand. The iteration forms below
(`for`, `some`, `every`) use *explicit* binding — you choose the variable name,
and `item` carries no special meaning there. If you need a name other than
`item` in a filter — for example to avoid a collision when filtering a list of
lists — use the `for` form instead.

## Projection (`.field`)

Applying field access to a **list of dictionaries** projects that field across
every element, returning a list of the values. This is sugar for
`for x in list return x.field`; the two are exactly equivalent, but projection
reads like column access.

```
// expression-language
[{name: "A"}, {name: "B"}].name       // → ["A", "B"]
[{name: "A"}, {age: 30}].name         // → ["A", null]   (missing field → null)
```

Behaviour depends on the input shape:

| Input | `.field` returns |
|---|---|
| Single dictionary with the field | the field's value (ordinary field access) |
| Single dictionary without the field | `null` |
| List of dictionaries, all have the field | list of values |
| List of dictionaries, some missing the field | list with `null` in the gaps |
| List containing non-dictionary elements | `bl.TypeError` |
| Empty list | `[]` |

## Comprehensions: `for`, `some`, `every`

Three comprehension forms iterate over one or more lists with explicitly named
iteration variables.

### `for … return` — mapping

`for x in list return expr` evaluates `expr` once per element and collects the
results into a new list. With multiple `in` clauses it forms the Cartesian
product, pairing each value of the first with each value of the second:

```
// expression-language
for x in [1, 2, 3] return x * 2                // → [2, 4, 6]
for x in [1, 2], y in [3, 4] return x * y      // → [3, 4, 6, 8]   (every x with every y)
```

### `some … satisfies` — existential

`some x in list satisfies predicate` is `true` when at least one element
satisfies the predicate, and `false` otherwise. Multiple clauses test every
pair:

```
// expression-language
some x in [1, 2, 3] satisfies x > 2                  // → true   (3 satisfies)
some x in [1, 2], y in [3, 4] satisfies x + y > 5    // → true   (2 + 4 = 6)
```

### `every … satisfies` — universal

`every x in list satisfies predicate` is `true` when all elements satisfy the
predicate:

```
// expression-language
every x in [1, 2, 3] satisfies x > 0                 // → true
every x in [1, 2], y in [3, 4] satisfies x + y >= 4  // → true   (every pair)
```

## Operators

Lists support equality and membership only. They have no arithmetic (`+`, `-`)
and no ordering (`<`, `<=`) operators — "which list is smaller" has no
meaningful definition.

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `=` `!=` | element-wise, order-sensitive equality | `[1, 2] = [1, 2]` | `true` |
| `in` | membership of a value in a list | `2 in [1, 2, 3]` | `true` |

Equality is element-wise *and order-sensitive*: `[1, 2] = [2, 1]` is `false`,
and lists of different length are never equal.

## Built-in functions

The list library follows DMN's FEEL functions, plus blkit extensions marked
**ext**. All positions are 1-based.

| Function | Example | Result |
|---|---|---|
| `count(l)` | `count([1,2,3])` | `3` |
| `isEmpty(l)` | `isEmpty([])` | `true` |
| `listContains(l, e)` | `listContains([1,2,3], 2)` | `true` |
| `indexOf(l, match)` | `indexOf([1,2,3,2], 2)` | `[2, 4]` (all positions) |
| `sublist(l, start[, length])` | `sublist([1,2,3], 2)` | `[2, 3]` (negative `start` from end) |
| `append(l, items…)` | `append([1], 2, 3)` | `[1, 2, 3]` |
| `prepend(l, items…)` **ext** | `prepend([2,3], 1)` | `[1, 2, 3]` |
| `concatenate(lists…)` | `concatenate([1,2],[3])` | `[1, 2, 3]` |
| `union(lists…)` | `union([1,2],[2,3])` | `[1, 2, 3]` (dedup) |
| `insertBefore(l, position, item)` | `insertBefore([1,3], 1, 2)` | `[2, 1, 3]` (single item) |
| `insertBefore(l, position, items)` **ext** | `insertBefore([1,5], 2, [2,3,4])` | `[1, 2, 3, 4, 5]` (spread) |
| `insertAfter(l, position, item)` **ext** | `insertAfter([1,3], 1, 2)` | `[1, 2, 3]` (single item) |
| `insertAfter(l, position, items)` **ext** | `insertAfter([1,5], 1, [2,3,4])` | `[1, 2, 3, 4, 5]` (spread) |
| `remove(l, position)` | `remove([1,2,3], 2)` | `[1, 3]` |
| `remove(l, match)` **ext** | `remove([1,2,3], function(i) i=2)` | `[1, 3]` |
| `listReplace(l, position, item)` | `listReplace([1,2,3], 2, 9)` | `[1, 9, 3]` |
| `listReplace(l, match, item)` **ext** | `listReplace([2,4,7], function(i) i<5, 5)` | `[5, 5, 7]` |
| `reverse(l)` | `reverse([1,2,3])` | `[3, 2, 1]` |
| `flatten(l)` | `flatten([[1,2],[[3]],4])` | `[1, 2, 3, 4]` (recursive) |
| `distinct(l)` | `distinct([1,2,2,1])` | `[1, 2]` (first-occurrence order) |
| `duplicateValues(l)` **ext** | `duplicateValues([1,2,2,1])` | `[1, 2]` (values seen more than once) |
| `intersection(lists…)` **ext** | `intersection([1,2],[2,3])` | `[2]` |
| `sort(l[, order])` | `sort([3,1,2], "desc")` | `[3, 2, 1]` |
| `stringJoin(l[, sep[, prefix, suffix]])` | `stringJoin(["a","b"], ", ")` | `"a, b"` |
| `zipStringJoin(lists[, delim[, prefix, suffix]])` **ext** | `zipStringJoin([["a","b","c"],["1","2","3"]], "-")` | `["a-1","b-2","c-3"]` |
| `seq(start, end[, step])` **ext** | `seq(5, 10)` | `[5, 6, 7, 8, 9, 10]` |

### Inserting a single list as one element

`insertBefore` and `insertAfter` dispatch on the type of the third argument: any
non-list value is inserted as a single element, while a list is *spread* into
the result. That means there's no direct way to insert one list as a single
element — `insertBefore([1, 4], 2, [2, 3])` spreads to `[1, 2, 3, 4]`. To insert
a list as one element, wrap it so the engine spreads the *outer* list:

```
// expression-language
insertBefore([1, 4], 2, [[2, 3]])    // → [1, [2, 3], 4]
```

### `zipStringJoin` (**ext**)

`zipStringJoin` takes `N` lists of equal length and produces one list of
strings, each the concatenation of the corresponding-position elements from
every input list.

```
// expression-language
zipStringJoin([["a","b","c"], ["1","2","3"]])           // → ["a1", "b2", "c3"]
zipStringJoin([["a","b","c"], ["1","2","3"]], "-")      // → ["a-1", "b-2", "c-3"]
```

Input constraints:

- The outer argument is a list of lists.
- All inner lists must be the **same length** — a mismatch is a `bl.TypeError`.
- All inner-list elements must be strings — non-strings are a `bl.TypeError`
  (convert upstream with `string(x)` if needed).

The optional `delim` takes one of two forms — a single string applied at every
gap, or a list of `N−1` strings applied one per gap:

```
// expression-language
zipStringJoin([["a","b"], ["1","2"], ["x","y"]], "-")        // → ["a-1-x", "b-2-y"]
zipStringJoin([["a","b"], ["1","2"], ["x","y"]], ["-", ":"]) // → ["a-1:x", "b-2:y"]
```

`prefix` and `suffix` likewise take a single string (wrapping each output
element once) or a list of `N` strings (wrapping each inner list's contribution
individually). The three arguments are independent, so you can mix forms.

```
// expression-language
zipStringJoin([["a","b"], ["1","2"], ["x","y"]], "-", "[", "]")
// → ["[a-1-x]", "[b-2-y]"]

zipStringJoin([["a","b"], ["1","2"], ["x","y"]], "-", ["<", "(", "{"], [">", ")", "}"])
// → ["<a>-(1)-{x}", "<b>-(2)-{y}"]
```

Edge cases: `zipStringJoin([])` → `[]`; `zipStringJoin([[]])` → `[]`;
`zipStringJoin([list])` (a single inner list, nothing to concat) → `list`.

### `seq` and the `:` operator (**ext**)

`seq(start, end[, step])` materialises a numeric sequence as a list. Both
`start` and `end` are inclusive and `step` defaults to `1`. The `start:end`
shorthand is exactly equivalent to `seq(start, end, 1)`.

```
// expression-language
seq(5, 10)                          // → [5, 6, 7, 8, 9, 10]
5:10                                // → [5, 6, 7, 8, 9, 10]   (sugar)
seq(0, 10, 2)                       // → [0, 2, 4, 6, 8, 10]
seq(1, 1)                           // → [1]
seq(10, 5)                          // → [10, 9, 8, 7, 6, 5]   (auto-reversed)
10:5                                // → [10, 9, 8, 7, 6, 5]
seq(1.5, 3.5)                       // → [1.5, 2.5, 3.5]
seq(0, 1, 0.25)                     // → [0, 0.25, 0.5, 0.75, 1]
```

A few behaviours worth knowing:

- **Auto-reverse / auto-direction.** When `start > end` and `step` is positive,
  the sequence steps downward. A `step` with the "wrong" sign for the start→end
  direction is treated as auto-direction — `seq(5, 10, -1)` uses `abs(step)` in
  the appropriate direction, giving `[5, 6, 7, 8, 9, 10]`, identical to
  `seq(5, 10, 1)`. `reverse(10:5)` yields the ascending `[5, 6, 7, 8, 9, 10]`.
- **Fractional steps** are preserved exactly (no float rounding), matching
  blkit's arbitrary-precision numbers. When `(end - start)` is not an exact
  multiple of `step`, the last element is the largest value not exceeding `end`
  (ascending) — so `seq(0, 0.95, 0.1)` stops at `0.9`.
- A `step` of `0` is a `bl.TypeError` (the sequence would never terminate), as
  are non-numeric `start`, `end`, or `step`.

### Aggregation

| Function | Supported types | Example | Result |
|---|---|---|---|
| `sum(l)` | `number` \| `duration` | `sum([dtDuration("PT1H"), dtDuration("PT2H")])` | `dtDuration("PT3H")` |
| `product(l)` | `number` | `product([2,3,4])` | `24` |
| `min(l)` | comparable | `min([3,1,2])` | `1` |
| `max(l)` | comparable | `max([1,2,3])` | `3` |
| `mean(l)` | `number` \| `duration` | `mean([1,2,3])` | `2` (arithmetic mean) |
| `median(l)` | `number` \| `duration` | `median([6,1,2,3])` | `2.5` (average of middle two) |
| `stddev(l)` | `number` | `stddev([2,4,7,5])` | `≈2.08` (sample stddev) |
| `mode(l)` | any | `mode([6,1,6,1])` | `[1, 6]` (multimodal — a list) |
| `all(l)` | `boolean` | `all([true,true,false])` | `false` (three-valued) |
| `any(l)` | `boolean` | `any([false,false,true])` | `true` (three-valued) |

Aggregates **ignore `null` elements**, and return `null` only when the list is
empty or contains no elements of the expected type. `all`/`any` follow
three-valued logic: `all([])` is `true` (vacuously), `any([])` is `false`, and
`all([true, null])` is `null`.

`sum`, `mean`, and `median` accept either a list of numbers or a list of a
**single duration kind** (all days-time, or all years-months) — mixing the two
duration kinds, or numbers with durations, is a `bl.TypeError`. `product` and
`stddev` are number-only, since they multiply or square (undefined for
durations).

**Comparable types.** `min` and `max` accept any list whose elements all share
one orderable type: `number`, `string`, `date`, `time`, `datetime`, days-time
`duration`, or years-months `duration`. `boolean`, `null`, `list`,
`dictionary`, `table`, `range`, and `calendar` are *not* comparable — they
support only `=`/`!=`. Mixing incompatible comparable types, e.g.
`min([1, "a"])`, is a `bl.TypeError`.

### Ordering used by `min` / `max` / `sort`

The ordering applied to each comparable type is the same as the `<`/`<=`/`>`/`>=`
operators on that type:

| Element type | Ordering |
|---|---|
| `number` | numeric; precision-preserving (`3.0 = 3.00`) |
| `string` | **code-point order** — case-sensitive, not locale-aware |
| `date` | chronological |
| `time` | chronological; wall-clock for naive, UTC-instant for zoned |
| `datetime` | chronological; wall-clock for naive, UTC-instant for zoned |
| `duration` (days-time) | total seconds |
| `duration` (years-months) | total months |

String ordering is by Unicode code point, which has some sharp edges worth
seeing:

```
// expression-language
min(["banana", "apple", "cherry"])   // → "apple"
min(["banana", "Apple"])             // → "Apple"   (A = U+0041 < b = U+0062)
min(["app", "apple"])                // → "app"     (a prefix sorts first)
min(["cafz", "café"])                // → "cafz"    (z = U+007A < é = U+00E9)
```

### `sort(l[, order])`

`sort` orders a list by the **element itself**. The optional `order` argument
is one of:

| `order` | Meaning |
|---|---|
| omitted, or `"asc"` | ascending in the element type's natural ordering |
| `"desc"` | descending |
| a list | **explicit value order** — elements ranked by their position in that list; anything not listed trails in ascending order |

The sort is **stable**: equal-ranked elements keep their input order. Elements
that compare as `null` (e.g. a naive-vs-zoned datetime) sort to the **end** under
`"asc"`/explicit order and **lead** under `"desc"`. A list of mutually
non-comparable elements, or an `order` string other than `"asc"`/`"desc"`, is a
`bl.TypeError`.

```
// expression-language
sort([3, 1, 2])                              // → [1, 2, 3]
sort([3, 1, 2], "desc")                      // → [3, 2, 1]
sort(["m", "s", "l"], ["s", "m", "l"])       // → ["s", "m", "l"]   (explicit order)
sort(["m", "s", "xl"], ["s", "m", "l"])      // → ["s", "m", "xl"]  ("xl" unlisted → trails)
```

Note this **diverges from DMN FEEL**, whose `sort(list, precedes)` takes a
comparator function. blkit's `sort` does not accept a comparator.

## Worked example: compile once, evaluate many

Projection, filtering, and the aggregates compose into readable
column-style queries. Suppose the env carries a list of order dictionaries:

```
// expression-language
orders = [
    {id: 1, amount: 100, quantity: 2, region: "NA"},
    {id: 2, amount: 150, quantity: 1, region: "EU"},
    {id: 3, amount: 75,  quantity: 3, region: "NA"}
]
```

Then:

```
// expression-language
sum(orders.amount)                                    // → 325   (total revenue)
distinct(orders.region)                               // → ["NA", "EU"]
count(orders[item.region = "NA"])                     // → 2     (filter, then count)
sum(for o in orders return o.amount * o.quantity)     // → 575   (per-row arithmetic, then sum)
```

On the host side, you compile an expression once against a typed env and
evaluate it repeatedly. Build list inputs with the variadic `bl.List`
constructor — it's infallible, preserves order, and accepts both individual
values and a spread slice:

```go
// host-side (Go)
import bl "github.com/friendly-business-machines/blkit/core"

type cart struct {
    Items bl.BlList `expr:"items"`
}

// Compile once.
var total, _ = bl.Expr[cart](`sum(items)`)

// Build a list value to pass in.
var prices = bl.List(bl.Number(100), bl.Number(150), bl.Number(75))

// Evaluate against any input.
var revenue, _ = total.Evaluate(cart{Items: prices})   // → 325
```

`bl.List()` with no arguments yields the empty list, and `bl.List(slice...)`
spreads an existing `[]bl.BlValue`:

```go
// host-side (Go)
var names  = []bl.BlValue{bl.String("Alice"), bl.String("Bob"), bl.String("Carol")}
var roster = bl.List(names...)
var empty  = bl.List()
```

When a list arrives as an input variable, the engine's bridge can also wrap a
native Go slice into a `bl.BlList` automatically; when the host already holds
`bl.BlValue`s, `bl.List(...)` is preferred since it skips that round-trip.

## Null and edge-case behaviour

- **Out-of-range index → `null`**, never an error — including index `0` and any
  negative index past the start.
- `sublist` with `length = 0` → `[]`; a negative `start` counts from the end.
- `remove` at an out-of-range position → the list unchanged.
- Numeric aggregates over an empty or non-numeric list → `null`; aggregates
  otherwise ignore `null` elements.
- `sort` with an unknown `order` string, or over non-comparable element types →
  `bl.TypeError`.
- `seq` with `step = 0`, or non-numeric arguments → `bl.TypeError`; a
  wrong-signed `step` is treated as auto-direction.

## Further reading

The authoritative definition of every list operator, function, and edge case is
the spec at `specs/expressions/list.spec.md`; the generated
[Reference](../reference/blkit.md) lists the full `bl.BlList` API, and
[Architecture → Expressions](../architecture/expressions.md) explains how the
engine compiles and runs the language underneath.
