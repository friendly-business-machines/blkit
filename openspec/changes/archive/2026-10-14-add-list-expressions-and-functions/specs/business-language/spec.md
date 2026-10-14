# Spec Delta

## ADDED Requirements

### Requirement: Typed lists support positional access, filtering, and projection
Lists SHALL remain ordered, immutable `List<T>` values; literal elements SHALL share one type, and `null` SHALL remain invalid as a list element. `list[index]` SHALL use one-based integral positions, with `-1` denoting the last element; zero, nonintegral, and out-of-bounds positions SHALL report errors. `list[predicate]` SHALL return elements for which a `Bool` predicate holds, in original order; within its predicate only, leading `.item` SHALL refer to the current element. Leading `.item` outside a bracket filter predicate SHALL fail validation. Plain `item` SHALL remain an ordinary name (including decision nodes, context entries, input names and iteration bindings), and numeric positions supplied by it or any other valid name SHALL retain their normal dependencies. Ordinary field access such as `order.item` SHALL retain its meaning. `listOfDictionaries.field` SHALL return a list of that field's values in input order, while ordinary dictionary `.field` access SHALL retain its scalar behavior. Missing fields and ill-typed predicates SHALL fail validation when statically known, otherwise evaluation SHALL report an error; errors inside predicates SHALL propagate. Numeric `mod` SHALL be an infix operator with multiplication-level precedence and the same checked floor-remainder semantics as `modulo`, including zero-divisor errors. An empty list SHALL be typed from context where necessary.

#### Scenario: Index, filter, project
- **WHEN** `.bl` evaluates `[1,2,3,4][1]`, `[1,2,3,4][-2]`, `[1,2,3,4][.item > 2]`, and `[{name: "A"}, {name: "B"}].name`
- **THEN** it SHALL produce `1`, `3`, `[3,4]`, and `["A","B"]` respectively

#### Scenario: Project and summarize orders
- **WHEN** `orders` is `[{id: 1, amount: 100, quantity: 2, region: "NA"}, {id: 2, amount: 150, quantity: 1, region: "EU"}, {id: 3, amount: 75, quantity: 3, region: "NA"}]` of a declared single dictionary type
- **THEN** `sum(orders.amount)` SHALL be `325`, `distinct(orders.region)` SHALL be `["NA","EU"]`, `count(orders[.item.region == "NA"])` SHALL be `2`, and `sum(for o in orders return o.amount * o.quantity)` SHALL be `575`

#### Scenario: Filter using numeric remainder
- **WHEN** `.bl` evaluates `[1,2,3,4][.item mod 2 == 0]`
- **THEN** it SHALL return `[2,4]`; `mod` SHALL use the same checked numeric remainder semantics as `modulo` and reject a zero divisor

#### Scenario: Explicit filter element and ordinary names
- **WHEN** `.bl` evaluates `[1,2][odd(.item)]`, uses a Number-typed input or decision/context entry named `item` as a list index, accesses `order.item`, or evaluates `.item` outside a bracket filter
- **THEN** the filter SHALL return `[1]`, the ordinary names and field access SHALL retain their existing meanings, and the out-of-filter `.item` SHALL fail validation

#### Scenario: Single-type constraint and invalid positions
- **WHEN** a program uses `[1,"two"]`, `[1,null]`, `[1,2][0]`, or `[1,2][3]`
- **THEN** incompatible/null elements SHALL fail validation and invalid positions SHALL report errors rather than yield null or wrap

### Requirement: Lists support only structural equality and scalar membership
Lists SHALL support order-sensitive, element-wise `==` and `!=` (different lengths are unequal); a scalar `in List<T>` SHALL test element equality. List-valued `+`, `-`, `<`, `<=`, `>`, and `>=` SHALL be rejected. Existing range and calendar membership SHALL remain unchanged.

#### Scenario: Equality and membership
- **WHEN** `.bl` evaluates `[1,2] == [1,2]`, `[1,2] == [2,1]`, and `2 in [1,2,3]`
- **THEN** the results SHALL be `true`, `false`, and `true`

### Requirement: List inspection and transformations are immutable and one-based
The following list overloads SHALL return results without changing input lists: `count(l)` returns a `Number`; `isEmpty(l)` a `Bool`; `listContains(l,e)` membership; `indexOf(l,match)` all matching one-based positions as `List<Number>` (or `[]` when absent); `sublist(l,start[,length])` a slice starting at a one-based or negative-from-end position, defaulting to the remainder and truncating at the end; `append(l,items…)` and `prepend(l,items…)` add individual elements; `concatenate(lists…)` joins lists in argument order; `reverse(l)` reverses elements; `flatten(l)` recursively removes nested list levels from a uniformly nested typed list. `union(lists…)` SHALL concatenate in argument order while retaining each equal value only once. Inputs and outputs SHALL have consistent element types; numeric positions and lengths SHALL be integral, a length SHALL be nonnegative, and invalid positions SHALL report errors. Existing string, dictionary, and calendar overloads SHALL remain available.

#### Scenario: Inspect and slice
- **WHEN** `.bl` evaluates `count([1,2,3])`, `isEmpty([])` in a typed list context, `indexOf([1,2,3,2],2)`, and `sublist([1,2,3],-2)`
- **THEN** the results SHALL be `3`, `true`, `[2,4]`, and `[2,3]`

#### Scenario: Compose and flatten
- **WHEN** `.bl` evaluates `append([1],2,3)`, `prepend([2,3],1)`, `concatenate([1,2],[3])`, `union([1,2],[2,3])`, `reverse([1,2,3])`, and `flatten([[[1,2]],[[3,4]]])`
- **THEN** the results SHALL be `[1,2,3]`, `[1,2,3]`, `[1,2,3]`, `[1,2,3]`, `[3,2,1]`, and `[1,2,3,4]`

### Requirement: List editing supports positions and predicate matches
`insertBefore(l,position,item-or-items)` and `insertAfter(l,position,item-or-items)` SHALL insert a non-list argument as one element or spread a list argument at the requested position; wrapping a list in an outer list SHALL insert it as one element. `remove(l,position)` SHALL remove the element at that position and `listReplace(l,position,item)` SHALL replace it. For all four positional editing operations, positive positions SHALL be one-based and negative positions SHALL count backward from the end of the list (`-1` denotes the last element); zero, nonintegral, and out-of-bounds positions SHALL fail. `remove(l,function(i) predicate)` SHALL remove all matching elements and `listReplace(l,function(i) predicate,item)` SHALL replace all matching elements; predicates SHALL evaluate per element with a scoped `i` and return `Bool`. `function` SHALL be reserved as the inline-predicate introducer, not a valid knowledge-model name; in these predicate positions, a complete `function(i)` header SHALL start a predicate body (including bodies beginning with unary `-` or a list literal), and a missing body SHALL fail parsing. All operations SHALL preserve the relative order of unaffected elements and leave the original list unchanged; mismatched element types or invalid predicates SHALL fail rather than mutate or silently skip.

#### Scenario: Insert scalar or spread list
- **WHEN** `.bl` evaluates `insertBefore([1,3],1,2)`, `insertAfter([1,5],1,[2,3,4])`, and `insertBefore([[1],[4]],2,[[2,3]])`
- **THEN** it SHALL return `[2,1,3]`, `[1,2,3,4,5]`, and `[[1],[2,3],[4]]` respectively

#### Scenario: Remove and replace by position
- **WHEN** `.bl` evaluates `remove([1,2,3],2)` and `listReplace([1,2,3],2,9)`
- **THEN** the results SHALL be `[1,3]` and `[1,9,3]`

#### Scenario: Edit from the end
- **WHEN** `.bl` evaluates `insertBefore([10,20,30],-1,5)`, `insertAfter([10,20,30],-1,5)`, `remove([10,20,30],-1)`, and `listReplace([10,20,30],-1,5)`
- **THEN** the results SHALL be `[10,20,5,30]`, `[10,20,30,5]`, `[10,20]`, and `[10,20,5]` respectively

#### Scenario: Remove and replace by match
- **WHEN** `.bl` evaluates `remove([1,2,3],function(i) i==2)` and `listReplace([2,4,7],function(i) i<5,5)`
- **THEN** it SHALL return `[1,3]` and `[5,5,7]`

### Requirement: Set-like list functions retain predictable ordering
`distinct(l)` SHALL retain first occurrences; `duplicateValues(l)` SHALL return values occurring more than once, once each in first-occurrence order; `intersection(lists…)` SHALL return shared values once, in first-list order. Equality SHALL follow the element type's `==`, including structural equality of nested lists and dictionaries. Inputs SHALL have compatible element types.

#### Scenario: Stable deduplication and intersection
- **WHEN** `.bl` evaluates `distinct([1,2,2,1])`, `duplicateValues([1,2,2,1])`, and `intersection([1,2],[2,3])`
- **THEN** the results SHALL be `[1,2]`, `[1,2]`, and `[2]`

### Requirement: List sorting uses type ordering and is stable
`sort(l[,order])` SHALL sort a list by its elements, stably, using ascending order by default or for `"asc"`, descending for `"desc"`, or an explicit list of priority values of the same element type. Ranked values SHALL follow their first position in the priority list; unlisted values SHALL follow, in ascending natural order. Equal-ranked or equal-valued elements SHALL retain original relative order. Orderable types SHALL be `Number`, `String`, `Date`, `Time`, `DateTime`, `DTDuration`, and `YMDuration` of one compatible kind; ordering SHALL follow their existing comparison operators, with strings compared case-sensitively by Unicode code point. Unsupported element types, mixed incompatible kinds, and invalid order arguments SHALL fail validation or evaluation as appropriate; no comparator callback is added.

#### Scenario: Natural and explicit order
- **WHEN** `.bl` evaluates `sort([3,1,2])`, `sort([3,1,2],"desc")`, and `sort(["m","s","xl"],["s","m","l"])`
- **THEN** the results SHALL be `[1,2,3]`, `[3,2,1]`, and `["s","m","xl"]`

#### Scenario: Code-point comparison
- **WHEN** `.bl` evaluates `min(["banana","Apple"])`, `min(["app","apple"])`, and `min(["cafz","café"])`
- **THEN** the results SHALL be `"Apple"`, `"app"`, and `"cafz"`

### Requirement: List joining validates strings and shapes
`zipStringJoin(lists[,delim[,prefix,suffix]])` SHALL join corresponding `String` elements in each inner list into one `String` per position, returning `List<String>`. Its outer argument SHALL contain equally long lists of strings; wrong shapes or non-string elements SHALL report a type/evaluation error, not truncate. Omitted delimiters and wrappers SHALL be empty strings. `delim` SHALL be one string used between all contributions or a list of N−1 strings for N input lists; `prefix` and `suffix` SHALL independently be either a single string wrapping each finished result or a list of N strings wrapping each list's contribution. Argument lengths other than those specified SHALL report errors. With zero input lists, the result SHALL be `[]`; list-valued delimiters, prefixes, and suffixes SHALL each be empty (zero gaps and zero contributions), while scalar formatting remains allowed.

#### Scenario: Empty columns
- **WHEN** `.bl` evaluates `zipStringJoin([], [], [], [])`
- **THEN** it SHALL return `[]`

#### Scenario: Per-gap and per-contribution formatting
- **WHEN** `.bl` evaluates `zipStringJoin([["a","b"],["1","2"],["x","y"]],["-", ":"], ["<","(","{"], [">",")","}"])`
- **THEN** the result SHALL be `["<a>-(1):{x}","<b>-(2):{y}"]`

#### Scenario: Scalar delimiter and wrappers
- **WHEN** `.bl` evaluates `zipStringJoin([["a","b"],["1","2"]],"-","[","]")`
- **THEN** the result SHALL be `["[a-1]","[b-2]"]`

#### Scenario: Invalid columns
- **WHEN** input lists have unequal lengths or any element is not a `String`
- **THEN** evaluation/validation SHALL fail rather than truncate or coerce the elements

### Requirement: Inclusive decimal sequences are available as lists
`seq(start,end[,step])` and `start:end` SHALL produce `List<Number>`, including endpoints when reached exactly, with default step magnitude 1. Sequence direction SHALL follow the endpoints even when the supplied step has the opposite sign. Fractional steps SHALL use `Number` decimal precision, stop before an endpoint not reached exactly, and fail for zero steps or unrepresentable/resource-exhausting output rather than loop indefinitely. `start:end` SHALL NOT change existing `[a..b]` range syntax or dictionary key/value syntax.

#### Scenario: Ascending and descending sequence
- **WHEN** `.bl` evaluates `seq(5,10)`, `10:5`, `seq(0,1,0.25)`, and `seq(5,10,-1)`
- **THEN** the results SHALL be `[5,6,7,8,9,10]`, `[10,9,8,7,6,5]`, `[0,0.25,0.5,0.75,1]`, and `[5,6,7,8,9,10]`

#### Scenario: Endpoint not reached
- **WHEN** `.bl` evaluates `seq(0,0.95,0.1)`
- **THEN** the last result SHALL be `0.9` rather than an overshoot or a rounded binary-float artifact

### Requirement: Boolean list aggregates require booleans
`all(List<Bool>)` SHALL return `true` exactly when every element is true; `any(List<Bool>)` SHALL return `true` when at least one is true. Empty `all` SHALL be `true` and empty `any` SHALL be `false`; non-boolean list elements SHALL be rejected.

#### Scenario: Boolean aggregations
- **WHEN** `.bl` evaluates `all([true,true,false])` and `any([false,false,true])`
- **THEN** the results SHALL be `false` and `true`

## MODIFIED Requirements

### Requirement: String conversion, joining, and literal transformations are typed
The language SHALL provide `string(from)` for `String`, `Number`, `Bool`, `Date`, `Time`, and `DateTime`, producing text in each type's existing human-readable serialized form (Boolean `true`/`false`, decimal number text, and the documented temporal formats); it SHALL reject lists and dictionaries. `stringJoin(List<String>[,sep[,prefix,suffix]]) -> String` SHALL join in list order, defaulting omitted separator and wrappers to `""`; `sep` SHALL separate elements and prefix/suffix SHALL wrap the completed string once. An empty list SHALL yield the prefix followed by the suffix. The following SHALL accept a `String` and return a `String`: `upperCase`, `lowerCase`, `trim`, `trimLeading`, `trimTrailing`, and `reverse`. Case changes SHALL use Unicode casing, trims SHALL remove Unicode whitespace only at the indicated edge(s), and reverse SHALL preserve each visible character's internal code-point order. List `reverse` SHALL be a separate overload; all existing string behavior SHALL remain available.

#### Scenario: Conversion and join
- **WHEN** `.bl` evaluates `"order-" + string(123)`, `stringJoin(["a", "b"], ", ")`, `stringJoin([], ",")`, and `stringJoin(["a","b"],"", "[", "]")`
- **THEN** it SHALL produce `"order-123"`, `"a, b"`, `""`, and `"[ab]"`

#### Scenario: Transformations use Unicode text
- **WHEN** `trim(" é ")`, `upperCase("é")`, and `reverse("éx")` are evaluated
- **THEN** they SHALL produce `"é"`, `"É"`, and `"xé"`, respectively, preserving the combining accent with `e`

### Requirement: Number aggregates consume lists of numbers
`product` and `stddev` SHALL take exactly one `List<Number>` and return a `Number`; `stddev` SHALL calculate the sample standard deviation using denominator `N - 1` and require at least two values. `sum`, `mean`, and `median` SHALL accept a homogeneous `List<Number>`, `List<DTDuration>`, or `List<YMDuration>` and return the same element type; `median` SHALL average the middle pair for an even-sized list. `min` and `max` SHALL return an element of a homogeneous list of one comparable type (`Number`, `String`, `Date`, `Time`, `DateTime`, `DTDuration`, or `YMDuration`), using the same ordering as the type's comparison operators. `mode(List<T>)` SHALL return a `List<T>` containing every value tied for highest frequency; numeric modes SHALL be in ascending order, and for types without natural ordering, tied values SHALL be returned in first-occurrence order. For empty lists, `sum` SHALL return numeric zero and `product` numeric one when the list's type is Number; `min`, `max`, `mean`, `median`, and `mode` SHALL report evaluation errors, and `stddev` SHALL require two values. Mixed types and mixed duration kinds SHALL be rejected; decimal results SHALL follow `Number` precision and overflow/error rules. Duration aggregations SHALL preserve the duration kind and its precision/error rules; empty duration `sum` SHALL return the zero duration of its inferred kind.

#### Scenario: Numeric aggregation
- **WHEN** `.bl` evaluates `min([3, 1, 2])`, `max([3, 1, 2])`, `sum([1, 2, 3])`, `mean([1, 2, 3])`, `median([1, 2, 3, 4])`, `product([2, 3, 4])`, `stddev([1, 2, 3])`, and `mode([3, 2, 3, 2])`
- **THEN** the results SHALL be `1`, `3`, `6`, `2`, `2.5`, `24`, `1`, and `[2,3]`, respectively

#### Scenario: Empty and undersized lists
- **WHEN** `.bl` evaluates `sum([])` as a Number list, `product([])` as a Number list, `mean([])`, or `stddev([5])`
- **THEN** the first two results SHALL be `0` and `1`, and the latter two SHALL report evaluation errors

#### Scenario: Duration and multiple modes
- **WHEN** `.bl` evaluates `sum([dtDuration("PT1H"),dtDuration("PT2H")])`, `median([6,1,2,3])`, and `mode([6,1,6,1])`
- **THEN** the results SHALL be `dtDuration("PT3H")`, `2.5`, and `[1,6]`

### Requirement: List iteration expressions can consume dictionary results
`every <name> in <list> satisfies <Bool-expression>` SHALL evaluate to `Bool` and `for <name> in <list> return <expression>` SHALL produce a `List<T>` in source-list order. They SHALL also allow comma-separated additional `name in list` bindings with Cartesian-product semantics; `some` SHALL use the same binding and predicate rules but return `true` on any match. Each bound name SHALL be visible only in its iteration body and in subsequent binding sources, and SHALL shadow an outer name only within that scope. An empty list SHALL make `every` return `true`, `some` return `false`, and `for` return `[]` with its element type inferred from the body or expected output type. Non-list sources and statically ill-typed predicates SHALL fail validation; errors from evaluated bodies SHALL propagate. Typed dictionary values/entries SHALL retain their value types so existing numeric list aggregates and entry projections work.

#### Scenario: Iterate values and entries
- **WHEN** `scores` is a `Dictionary<Number>` with keys `alice: 90` and `bob: 75`, and `every v in values(scores) satisfies v >= 50` and `for e in getEntries(scores) return e.value > 80` are evaluated
- **THEN** the expressions return `true` and `[true, false]` in key order, and `sum(values(scores))` returns `165`

#### Scenario: Cartesian map and quantifiers
- **WHEN** `.bl` evaluates `for x in [1,2], y in [3,4] return x * y`, `some x in [1,2], y in [3,4] satisfies x + y > 5`, and `every x in [1,2], y in [3,4] satisfies x + y >= 4`
- **THEN** the results SHALL be `[3,4,6,8]`, `true`, and `true`

#### Scenario: Empty list and validation
- **WHEN** an `every`, `some`, or `for` expression consumes an empty typed list in any binding, or an iteration source is a scalar or a predicate body is not `Bool`
- **THEN** `every` returns `true`, `some` returns `false`, `for` returns a typed empty list, and invalid expressions fail validation
