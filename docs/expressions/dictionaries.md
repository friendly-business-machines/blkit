# Dictionaries

> Key-value structures — construction, member access, and the dictionary
> functions.

A **dictionary** is an unordered map from string keys to values — blkit's way of
modelling structured, nested data inside an expression. An applicant record, an
order line, a parsed JSON object: anything with named fields is naturally a
dictionary. This page covers writing dictionary literals, reading members,
testing for keys, transforming dictionaries with the built-in functions, and
passing one in from host Go code.

A dictionary is a pure *value*: every operation produces a fresh dictionary
rather than mutating the original, and keys are case-sensitive, non-empty
strings. Because dictionaries are unordered, two dictionaries with the same
key/value pairs are equal regardless of how they were written, and any operation
that surfaces keys returns them in **code-point-sorted order** so the result is
deterministic.

## Literals

A dictionary literal is delimited by braces, with comma-separated `key: value`
entries:

```
// expression-language
{}                                 // → empty dictionary
{name: "Alice", age: 30}           // unquoted keys
{"my key": 1}                      // quoted keys, for special characters
{a: 1, b: {c: 2}}                  // nested dictionaries
```

Keys may be written bare (`name`) when they are valid identifiers, or quoted
(`"my key"`) when they contain spaces or other special characters.

A literal evaluates **left to right in a single eager pass**: each entry's value
expression is evaluated against the entries already defined to its left. So an
entry can refer back to an earlier sibling key:

```
// expression-language
{a: 2, b: a * 2}                   // → {a: 2, b: 4}
```

Here `a` resolves to `2` (the entry just defined), then `b` is computed as
`a * 2 → 4`. Once the literal has been evaluated it holds only resolved values —
there are no lingering dependencies between entries, and the source order is not
preserved afterwards.

## Member access

Two equivalent forms read a value out of a dictionary: the dot operator and
bracket access. Both navigate into nested dictionaries, and **a missing key
yields `null`** rather than an error.

```
// expression-language
{a: {b: 3}}.a.b                    // → 3      (path access through nesting)
{a: 1}.missing                     // → null   (key not present)
applicant["my key"]                // bracket access for special-character keys
applicant.address.postcode         // navigate nested input data
```

Use the dot form for ordinary identifier keys, and bracket form when the key is
not a valid identifier (`d["my key"]`) or when it lives in a variable. The two
are interchangeable: `d.name` and `d["name"]` mean exactly the same thing.

Because a missing key quietly becomes `null`, member access never fails on an
absent field — the `null` then flows through the rest of the expression under
blkit's [null-aware semantics](booleans-and-logic.md).

## Operators

Dictionaries support only member access and equality. They have no arithmetic
(`+`, `-`, …), no ordering (`<`, `<=`, `>`, `>=`), and no `in` operator —
membership is tested with `has` or `isDefined` (see below).

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `.` / `[ ]` | member access (path / bracket lookup) | `d.name`, `d["my key"]` | the value, or `null` if missing |
| `=` | structural equality (order-insensitive) | `{a:1, b:2} = {b:2, a:1}` | `true` |
| `!=` | structural inequality | `{a:1} != {a:2}` | `true` |

Equality is **structural and order-insensitive**: two dictionaries are equal
when they hold the same key/value pairs, regardless of the order they were
written in.

## Key presence vs. null

Member access can't, on its own, distinguish a key that is *absent* from a key
that is *present but holds `null`* — both read back as `null`. When that
difference matters, use `isDefined`, which probes the dictionary at the
**resolution layer**, before a missing key has collapsed to `null`:

```
// expression-language, with `applicant` a declared dictionary field
isDefined(applicant)             // → true   (a declared field is always defined)
isDefined(applicant.name)        // → true   when the dictionary has key "name"
isDefined(applicant.middleName)  // → false  when there is no "middleName" key
isDefined(undeclaredName)        // compile error — not a declared field
```

`isDefined(d.key)` answers *"does this key exist?"*. To instead ask whether a
*present* value is `null`, use `isNull(x)`; to supply a fallback for a `null`,
use `getOrElse(x, default)` (both in [Booleans and logic](booleans-and-logic.md)
and the null reference). Note that naming a top-level variable the env doesn't
declare is a **compile-time error**, so `isDefined` on a bare name is only ever
statically `true` — its real use is for dictionary paths.

The function `has(d, key)` is the value-layer equivalent for a key you have as a
string: `has(applicant, "name")` is the counterpart to `isDefined(applicant.name)`.

## Built-in functions

These are the DMN-inspired dictionary functions plus blkit's extensions (marked
**ext**). Functions that surface keys (`keys`, `values`, `getEntries`) return
them in code-point-sorted order.

### Reading

| Function | Example | Result |
|---|---|---|
| `getValue(d, key)` | `getValue({foo: 123}, "foo")` | `123` |
| `getValue(d, keys)` | `getValue({x:1, y:{z:0}}, ["y","z"])` | `0` (follows a nested path) |
| `getEntries(d)` | `getEntries({foo: 123})` | `[{key: "foo", value: 123}]` |
| `keys(d)` **ext** | `keys({a:1, b:2})` | `["a", "b"]` (code-point-sorted) |
| `values(d)` **ext** | `values({a:1, b:2})` | `[1, 2]` (by sorted key) |
| `has(d, key)` **ext** | `has({a:1}, "a")` | `true` |
| `size(d)` **ext** | `size({a:1, b:2})` | `2` |
| `isEmpty(d)` **ext** | `isEmpty({})` | `true` |

`getValue` is the function form of member access. Given a single string key it
behaves like `d.key`; given a *list* of keys it walks a nested path, equivalent
to chaining dots (`getValue(d, ["y","z"])` is `d.y.z`). `getEntries` turns a
dictionary into a list of `{key, value}` dictionaries — handy for iterating (see
below).

### Transforming

Every transform returns a **new** dictionary; the input is never mutated.

| Function | Example | Result |
|---|---|---|
| `dictionaryPut(d, key, value)` | `dictionaryPut({x:1}, "y", 2)` | `{x:1, y:2}` |
| `dictionaryPut(d, keys, value)` | `dictionaryPut({x:1, y:{z:0}}, ["y","z"], 2)` | `{x:1, y:{z:2}}` |
| `dictionaryMerge(dicts)` | `dictionaryMerge([{x:1}, {y:2}])` | `{x:1, y:2}` (later wins) |
| `dictionaryRemove(d, key)` **ext** | `dictionaryRemove({a:1, b:2}, "a")` | `{b: 2}` |

- **`dictionaryPut`** adds or replaces an entry. With a single string key it sets
  a top-level entry; with a list of keys it sets a value at a nested path,
  rebuilding the intermediate dictionaries.
- **`dictionaryMerge`** takes a *list* of dictionaries and folds them into one;
  on a key collision the later dictionary in the list wins.
- **`dictionaryRemove`** drops a key. Removing a key that isn't present leaves
  the dictionary unchanged.

## Iterating keys and values

Dictionaries themselves are unordered and aren't directly iterable, but the
reading functions give you ordered lists you can drive a comprehension over.
`keys`, `values`, and `getEntries` all bridge a dictionary into a
[list](lists.md):

```
// expression-language, with `scores` a dictionary like {alice: 90, bob: 75}
keys(scores)                                      // → ["alice", "bob"]
values(scores)                                    // → [90, 75]

// sum every value
sum(values(scores))                               // → 165

// every score is a pass?
every v in values(scores) satisfies v >= 50       // → true

// rebuild entries that pass a threshold
for e in getEntries(scores) return e.value > 80   // → [true, false]
```

Iterating `getEntries(d)` gives you `{key, value}` dictionaries, so a
comprehension can see both the key and the value of each entry.

## Passing a dictionary in from Go

Host Go code builds a dictionary by handing a `map[string]bl.BlValue` to the
`bl.Dictionary` constructor. Because the language treats dictionaries as
unordered, the map's Go iteration order is irrelevant — keys are sorted into
canonical order at construction. The constructor returns
`(bl.BlDictionary, error)`; the only validation is that an empty-string key is
rejected (duplicate keys can't occur in a Go map).

A top-level `bl.BlDictionary` field on the env struct is the idiomatic way to
hand a whole nested record to an expression:

```go
// host-side (Go)
import bl "github.com/friendly-business-machines/blkit/core"

var applicant, _ = bl.Dictionary(map[string]bl.BlValue{
    "name":   bl.String("Alice"),
    "age":    bl.Number(30),
    "income": bl.Number(75000),
})

type ApplicantEnv struct {
    Applicant bl.BlDictionary `expr:"applicant"`
}

var eligible, _ = bl.Expr[ApplicantEnv](
    `applicant.age >= 18 and applicant.income > 50000`,
)
var result, _ = eligible.Evaluate(ApplicantEnv{Applicant: applicant})
// result is bl.BlBoolean(true)
```

Compile the expression once with `bl.Expr`, then call `Evaluate` as many times
as you like with different `ApplicantEnv` values — the compiled program is
reusable and holds no per-run state.

To read a dictionary back out of an evaluated result, `bl.BlDictionary` has a
`Native()` accessor that returns a defensive copy of the underlying
`map[string]bl.BlValue`; you can mutate that copy freely without affecting the
original. Its iteration order is Go's standard map randomisation, so sort it
yourself, or use the sorted output of `keys(d)` from inside an expression, when
you need a stable order.

## Null and edge-case behaviour

- **Missing key** — reading an absent key via `.`, `[]`, or `getValue` yields
  `null`, never an error.
- **Absent vs. null** — `isDefined(d.key)` distinguishes a missing key (`false`)
  from a key present with a `null` value (`true`); plain member access cannot.
- **`dictionaryRemove` of an absent key** — returns the dictionary unchanged.
- **Merging or putting into an empty dictionary** — a no-op shape: the result is
  just the other contents.
- **Special-character keys** — must be written with quotes in a literal
  (`{"my key": 1}`) and read with bracket access (`d["my key"]`).
- **Empty-string keys** — rejected: `dictionaryPut` with an empty-string key
  raises a type error, and the host `bl.Dictionary` constructor returns an error.

## Further reading

The dictionary type is defined authoritatively in
`specs/expressions/dictionary.spec.md`, with literals and path access also
covered in `specs/expressions/bl-expr.spec.md`. For the exhaustive API surface
see the generated [Reference](../reference/blkit.md), and for how dictionaries
fit into the engine's value system see
[Architecture → Expressions](../architecture/expressions.md).
