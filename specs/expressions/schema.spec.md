---
name: bl.BlSchema
description: Unified shape declaration for Bl* values — named fields, optionality, and arbitrary nesting. Drives parse-time typing for bl.Expr and is intended to replace the InputContract / OutputContract / DictionaryContract / ListContract / TableContract family and the reflected Parameters / Outputs structs used by BusinessKnowledgeModel and DecisionNode.
targets:
  - ../../schema.go
---

# bl.BlSchema

`bl.BlSchema` is a declarative shape for any `Bl*` value: a scalar type, a typed list, a
dictionary with named fields, or a table with typed columns. One type drives every consumer
that today carries its own representation — parse-time typing for `bl.Expr`, runtime
contract validation at process boundaries, BKM parameter declarations, decision-node output
declarations, and JSON-schema export.

Validation **policy** (closed vs permissive) lives on the call site, not the schema. The
same `bl.BlSchema` value can serve as an `InputContract` (closed) and an `OutputContract`
(permissive) depending on which validation method the caller uses.

## Type definition

A schema is an ordered set of named, typed fields. Nesting hangs off the `Type` of each
field — a `bl.TypeDictionary` or `bl.TypeTable` field carries a nested `bl.BlSchema` describing
its record shape; a `bl.TypeList` field carries the set of `bl.Type`s its elements are
allowed to take. There is no top-level "kind" tag — the field's `Type` already says what
shape it is.

```go
// host-side (Go)
type BlSchema []Field

// Field is a named, typed entry in a schema. Which nested fields apply depends on Type:
//
//   Type ∈ {bl.TypeNumber, bl.TypeString, bl.TypeBoolean, ...}  → scalar leaf, no nesting
//   Type == bl.TypeDictionary or bl.TypeTable                → Fields describes the record
//   Type == bl.TypeList                                   → Element lists the allowed element types
//
// Inapplicable nesting fields are zero and ignored.
type Field struct {
    Name     string
    Type     Type
    Optional bool      // fields are required by default; set Optional: true to make absence permitted

    // Nested shape — at most one is populated, determined by Type.
    Fields  BlSchema   // for TypeDictionary / TypeTable
    Element []Type   // for TypeList; the element must be one of these Types
}
```

A `bl.BlSchema` is ordered so that markdown / JSON-schema rendering is stable, but name
lookups use the slice as a name-keyed set.

Top-level usage is always record-shaped — a process boundary, a BKM parameter list, a
parse-time environment, etc. Non-record top-level inputs (a process that takes a single
list or a single table at its boundary) are expressed by wrapping in a one-field schema
with the appropriate `Type`.

## Construction (host-side)

`bl.Schema(fields...)` builds and validates a `bl.BlSchema`. `bl.Field` stays as a plain struct
literal — only the top-level constructor needs to validate, and it walks nested `Fields`
and `Element` recursively.

```go
// host-side (Go)
// Schema builds a bl.BlSchema from the supplied fields and validates well-formedness
// (duplicate / empty field names, empty list Element, bl.TypeNull in any position,
// scalars with nesting populated, unknown bl.Type values, and the same checks applied
// recursively to nested Fields). On error the returned bl.BlSchema is nil.
func Schema(fields ...Field) (BlSchema, error)
```

Example shapes:

```go
// host-side (Go)
var addressSchema, _ = bl.Schema(
    bl.Field{Name: "street",     Type: bl.TypeString},
    bl.Field{Name: "city",       Type: bl.TypeString},
    bl.Field{Name: "postalCode", Type: bl.TypeString},
    bl.Field{Name: "region",     Type: bl.TypeString, Optional: true},
)

var applicantSchema, _ = bl.Schema(
    bl.Field{Name: "name",    Type: bl.TypeString},
    bl.Field{Name: "age",     Type: bl.TypeNumber},
    bl.Field{Name: "address", Type: bl.TypeDictionary, Fields: addressSchema},
    bl.Field{Name: "scores",  Type: bl.TypeList,       Element: []bl.Type{bl.TypeNumber}, Optional: true},
    bl.Field{Name: "tokens",  Type: bl.TypeList,       Element: []bl.Type{bl.TypeNumber, bl.TypeString, bl.TypeList}, Optional: true},
)

// Tabular field: a list of named columns the rows must conform to.
var lineItemsColumns, _ = bl.Schema(
    bl.Field{Name: "product",   Type: bl.TypeString},
    bl.Field{Name: "quantity",  Type: bl.TypeNumber},
    bl.Field{Name: "unitPrice", Type: bl.TypeNumber},
)
var lineItemsField = bl.Field{
    Name:   "lineItems",
    Type:   bl.TypeTable,
    Fields: lineItemsColumns,
}
```

Validation rules applied by `bl.Schema(...)`:

- A field declaring `Type: bl.TypeNull` — `bl.BlNull` is the absence marker, not a declarable
  shape. See [null.spec.md](null.spec.md). The same rule applies inside `Element`:
  `bl.TypeNull` is rejected wherever it appears.
- A `bl.TypeList` field with an empty `Element` — a list field must declare what its
  elements are allowed to be.
- A `bl.TypeList` field whose `Element` contains duplicate `bl.Type`s.
- A `bl.TypeDictionary` or `bl.TypeTable` field with empty `Fields` is allowed (degenerate);
  duplicate field names or an empty field name within `Fields` is not.
- A scalar-typed field with non-empty `Fields` or non-empty `Element` (nesting fields are
  meaningless for scalars).
- A `Type` value outside the declared `bl.Type` constants (in `Type` or anywhere inside
  `Element`).

## Validation

```go
// host-side (Go)
// ValidateInput rejects values whose shape doesn't match the schema and rejects any
// undeclared keys / columns ("closed").
func (s BlSchema) ValidateInput(v BlValue) error

// ValidateOutput rejects values whose shape doesn't match the schema's declared fields
// but accepts undeclared keys / columns ("permissive").
func (s BlSchema) ValidateOutput(v BlValue) error
```

A schema validates against a `bl.BlDictionary`: every non-`Optional` field in `s` must be
present with a non-`bl.BlNull` value, and every present field's value must satisfy its
field's `Type` (plus nested `Fields` / `Element` for structural types). `ValidateInput`
additionally rejects undeclared keys; `ValidateOutput` ignores them.

Per-field semantics (dispatched on `Type`):

- **Scalar types** (`bl.TypeNumber`, `bl.TypeString`, `bl.TypeBoolean`, `bl.TypeDate`, …) —
  `v.Type() == f.Type`. `bl.BlNull` fails.
- **`bl.TypeList`** — `v` is a `bl.BlList`; every element's `Type()` is one of the `bl.Type`s
  in `f.Element`. The list itself is checked only at this level; the spec does not let a
  list field constrain its elements' inner structure (use a `bl.TypeTable` field for
  records, or wrap structured rows in a higher-level schema).
- **`bl.TypeDictionary`** — `v` is a `bl.BlDictionary`; `f.Fields` validates it (recursively,
  with the same input / output policy).
- **`bl.TypeTable`** — `v` is a `bl.BlTable`; the declared columns in `f.Fields` are present on
  every row and each row's column values satisfy the column schemas. `ValidateInput`
  rejects rows with undeclared columns; `ValidateOutput` ignores them.

`bl.BlNull` field values:

- A required field whose value is `bl.BlNull` fails validation under both methods.
- An `Optional` field whose value is `bl.BlNull` passes — it is treated as absence.

Failures produce a `bl.SchemaError` carrying a path to the offending node
(`applicant.address.postalCode: expected bl.BlString, got bl.BlNumber`) so a single error message
locates the problem in a deeply nested value.

`[@test] ../../schema_test.go`

## Parse-time use by `bl.Expr`

`bl.Expr` takes a `bl.BlSchema` directly (see
[bl-expr.spec.md § Using the engine](bl-expr.spec.md#using-the-engine)): top-level field
names are the available variables, each variable's `Type` is its parse-time type, and
nested `Fields` are available to the member-access type-checker (so
`applicant.address.postalCode` can be statically verified when the relevant fields are
declared).

```go
// host-side (Go)
func Expr(source string, schema BlSchema) (BlExpr, error)
```

## Migration

`bl.BlSchema` is intended to subsume the types below. The mapping is mechanical; migration
will land incrementally.

| Existing | `bl.BlSchema` equivalent | Notes |
|---|---|---|
| `InputContract` ([data-contract.spec.md:18](../data/data-contract.spec.md#L18)) | `bl.BlSchema` validated via `ValidateInput` | Closed semantics preserved. |
| `OutputContract` ([data-contract.spec.md:25](../data/data-contract.spec.md#L25)) | `bl.BlSchema` validated via `ValidateOutput` | Permissive semantics preserved. |
| `DictionaryContract` ([data-contract.spec.md:57](../data/data-contract.spec.md#L57)) | A field with `Type: bl.TypeDictionary` and nested `Fields` | Direct mapping. |
| `ListContract` ([data-contract.spec.md:85](../data/data-contract.spec.md#L85)) | A field with `Type: bl.TypeList` and an `Element` `[]bl.Type` | Existing `ListContract` allowed structured element types (e.g. a nested `DictionaryContract`); `bl.BlSchema` element constraints are `bl.Type`-only. Structured-row use cases migrate to `bl.TypeTable`. |
| `TableContract` ([data-contract.spec.md:105](../data/data-contract.spec.md#L105)) | A field with `Type: bl.TypeTable` and nested `Fields` | Direct mapping. |
| Generic `Parameters` ([business-knowledge-model.spec.md:21](../decision-tasks/business-knowledge-model.spec.md#L21)) | `bl.BlSchema` | The reflected-struct path is replaced by an explicit `bl.BlSchema` carried on the BKM. |
| Generic `Outputs` ([decision-node.spec.md](../decision-tasks/decision-node.spec.md)) | `bl.BlSchema` | Same — node carries a `bl.BlSchema` describing its outputs. |

The `InputContract` / `OutputContract` Go-type split (which today makes wrong-direction use
a Go type error) collapses into one `bl.BlSchema` type — direction discrimination moves to the
call site (`bl.Start(...)` accepts the schema and validates as input; `bl.End(...)` accepts the
schema and validates as output). Callers that want stronger direction typing can wrap with
named aliases at their boundary.

## Edge cases

- An empty `bl.BlSchema{}` is valid. `ValidateInput` accepts only the empty `bl.BlDictionary`;
  `ValidateOutput` accepts any `bl.BlDictionary`.
- A `bl.TypeDictionary` or `bl.TypeTable` field with empty `Fields` is valid — same
  open/closed treatment as an empty top-level schema.
- `bl.Field{Name: "x", Type: bl.TypeAny}` — a required field whose value can be any
  non-`bl.BlNull` `bl.BlValue`. Useful for permissive pass-through cases.
- Schemas are deep-comparable via `reflect.DeepEqual` (a `bl.BlSchema` is a slice, so equality
  follows slice semantics).
- `bl.BlSchema` is acyclic by construction: every `Fields` is supplied as a fully-built
  value, so no recursive shape can be expressed. Recursive process / decision shapes are
  out of scope for v1.
- `Element` order is irrelevant — it is a set of allowed `bl.Type`s, not a sequence.
  Duplicate entries are a well-formedness error (see § Construction).

`[@test] ../../schema_edge_cases_test.go`
