# Dictionaries

> Key-value structures — construction, member access, and the dictionary
> functions.

A **dictionary** is an unordered map from string keys to values — blkit's way of
modelling structured, nested data inside an expression. An applicant record, an
order line, a parsed JSON object: anything with named fields is naturally a
dictionary. This page covers writing dictionary literals, reading members,
transforming dictionaries with the built-in functions, and passing one in from
host Go code.

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
{}                                 // empty dictionary
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
bracket access. Both navigate into nested dictionaries.

```
// expression-language
{a: {b: 3}}.a.b                    // → 3      (path access through nesting)
applicant["my key"]                // bracket access for special-character keys
applicant.address.postcode         // navigate nested input data
```

Use the dot form for ordinary identifier keys, and bracket form when the key is
not a valid identifier (`d["my key"]`) or when it lives in a variable. The two
are interchangeable: `d.name` and `d["name"]` mean exactly the same thing.

## Operators

Dictionaries support only member access and equality. They have no arithmetic
(`+`, `-`, …), no ordering (`<`, `<=`, `>`, `>=`), and no `in` operator —
membership is tested with `has` (see below).

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `.` / `[ ]` | member access (path / bracket lookup) | `d.name`, `d["my key"]` | the value |
| `=` | structural equality (order-insensitive) | `{a:1, b:2} = {b:2, a:1}` | `true` |
| `!=` | structural inequality | `{a:1} != {a:2}` | `true` |

Equality is **structural and order-insensitive**: two dictionaries are equal
when they hold the same key/value pairs, regardless of the order they were
written in.

## Built-in functions

The built-in dictionary functions read, transform, and inspect dictionaries.
Functions that surface keys (`keys`, `values`, `getEntries`) return them in
code-point-sorted order.

### Reading

| Function | Example | Result |
|---|---|---|
| `getValue(d, key)` | `getValue({foo: 123}, "foo")` | `123` |
| `getValue(d, keys)` | `getValue({x:1, y:{z:0}}, ["y","z"])` | `0` (follows a nested path) |
| `getEntries(d)` | `getEntries({foo: 123})` | `[{key: "foo", value: 123}]` |
| `keys(d)` | `keys({a:1, b:2})` | `["a", "b"]` (code-point-sorted) |
| `values(d)` | `values({a:1, b:2})` | `[1, 2]` (by sorted key) |
| `has(d, key)` | `has({a:1}, "a")` | `true` |
| `size(d)` | `size({a:1, b:2})` | `2` |
| `isEmpty(d)` | `isEmpty({})` | `true` |

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
| `dictionaryRemove(d, key)` | `dictionaryRemove({a:1, b:2}, "a")` | `{b: 2}` |

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

## Dictionaries from Go

Host Go code builds `dictionary` values by handing a `map[string]bl.BlValue` to
the `bl.Dictionary` constructor; a top-level `bl.BlDictionary` env field is the
idiomatic way to hand a whole nested record in. See
[Values from Go](values-from-go.md) for the full host-side story.
