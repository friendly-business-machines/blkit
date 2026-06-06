---
name: BlSchema
description: Unified shape declaration for Bl* values — named fields, optionality, and arbitrary nesting. Drives parse-time typing for Bl.Expr and is intended to replace the InputContract / OutputContract / DictionaryContract / ListContract / TableContract family and the reflected Parameters / Outputs structs used by BusinessKnowledgeModel and DecisionNode.
targets:
  - ../../expr/schema.go
---

# BlSchema

`BlSchema` is a declarative shape for any `Bl*` value: a scalar type, a typed list, a
dictionary with named fields, or a table with typed columns. One type drives every consumer
that today carries its own representation — parse-time typing for `Bl.Expr`, runtime
contract validation at process boundaries, BKM parameter declarations, decision-node output
declarations, and JSON-schema export.

Validation **policy** (closed vs permissive) lives on the call site, not the schema. The
same `BlSchema` value can serve as an `InputContract` (closed) and an `OutputContract`
(permissive) depending on which validation method the caller uses.

## Type definition

A schema is an ordered set of named, typed fields. Nesting hangs off the `Type` of each
field — a `BlTypeDictionary` or `BlTypeTable` field carries a nested `BlSchema` describing
its record shape; a `BlTypeList` field carries the set of `BlType`s its elements are
allowed to take. There is no top-level "kind" tag — the field's `Type` already says what
shape it is.

```go
// host-side (Go)
type BlSchema []BlField

// BlField is a named, typed entry in a schema. Which nested fields apply depends on Type:
//
//   Type ∈ {BlTypeNumber, BlTypeString, BlTypeBoolean, ...}  → scalar leaf, no nesting
//   Type == BlTypeDictionary or BlTypeTable                   → Fields describes the record
//   Type == BlTypeList                                        → Element lists the allowed element types
//
// Inapplicable nesting fields are zero and ignored.
type BlField struct {
    Name     string
    Type     BlType
    Optional bool      // fields are required by default; set Optional: true to make absence permitted

    // Nested shape — at most one is populated, determined by Type.
    Fields  BlSchema   // for BlTypeDictionary / BlTypeTable
    Element []BlType   // for BlTypeList; the element must be one of these BlTypes
}
```

A `BlSchema` is ordered so that markdown / JSON-schema rendering is stable, but name
lookups use the slice as a name-keyed set.

Top-level usage is always record-shaped — a process boundary, a BKM parameter list, a
parse-time environment, etc. Non-record top-level inputs (a process that takes a single
list or a single table at its boundary) are expressed by wrapping in a one-field schema
with the appropriate `Type`.

## Construction (host-side)

`Schema(fields...)` builds and validates a `BlSchema`. `BlField` stays as a plain struct
literal — only the top-level constructor needs to validate, and it walks nested `Fields`
and `Element` recursively.

```go
// host-side (Go)
// Schema builds a BlSchema from the supplied fields and validates well-formedness
// (duplicate / empty field names, empty list Element, BlTypeNull in any position,
// scalars with nesting populated, unknown BlType values, and the same checks applied
// recursively to nested Fields). On error the returned BlSchema is nil.
func Schema(fields ...BlField) (BlSchema, error)
```

Example shapes:

```go
// host-side (Go)
var addressSchema, _ = Schema(
    BlField{Name: "street",     Type: BlTypeString},
    BlField{Name: "city",       Type: BlTypeString},
    BlField{Name: "postalCode", Type: BlTypeString},
    BlField{Name: "region",     Type: BlTypeString, Optional: true},
)

var applicantSchema, _ = Schema(
    BlField{Name: "name",    Type: BlTypeString},
    BlField{Name: "age",     Type: BlTypeNumber},
    BlField{Name: "address", Type: BlTypeDictionary, Fields: addressSchema},
    BlField{Name: "scores",  Type: BlTypeList,       Element: []BlType{BlTypeNumber}, Optional: true},
    BlField{Name: "tokens",  Type: BlTypeList,       Element: []BlType{BlTypeNumber, BlTypeString, BlTypeList}, Optional: true},
)

// Tabular field: a list of named columns the rows must conform to.
var lineItemsColumns, _ = Schema(
    BlField{Name: "product",   Type: BlTypeString},
    BlField{Name: "quantity",  Type: BlTypeNumber},
    BlField{Name: "unitPrice", Type: BlTypeNumber},
)
var lineItemsField = BlField{
    Name:   "lineItems",
    Type:   BlTypeTable,
    Fields: lineItemsColumns,
}
```

Validation rules applied by `Schema(...)`:

- A field declaring `Type: BlTypeNull` — `BlNull` is the absence marker, not a declarable
  shape. See [null.spec.md](null.spec.md). The same rule applies inside `Element`:
  `BlTypeNull` is rejected wherever it appears.
- A `BlTypeList` field with an empty `Element` — a list field must declare what its
  elements are allowed to be.
- A `BlTypeList` field whose `Element` contains duplicate `BlType`s.
- A `BlTypeDictionary` or `BlTypeTable` field with empty `Fields` is allowed (degenerate);
  duplicate field names or an empty field name within `Fields` is not.
- A scalar-typed field with non-empty `Fields` or non-empty `Element` (nesting fields are
  meaningless for scalars).
- A `Type` value outside the declared `BlType` constants (in `Type` or anywhere inside
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

A schema validates against a `BlDictionary`: every non-`Optional` field in `s` must be
present with a non-`BlNull` value, and every present field's value must satisfy its
field's `Type` (plus nested `Fields` / `Element` for structural types). `ValidateInput`
additionally rejects undeclared keys; `ValidateOutput` ignores them.

Per-field semantics (dispatched on `Type`):

- **Scalar types** (`BlTypeNumber`, `BlTypeString`, `BlTypeBoolean`, `BlTypeDate`, …) —
  `v.Type() == f.Type`. `BlNull` fails.
- **`BlTypeList`** — `v` is a `BlList`; every element's `Type()` is one of the `BlType`s
  in `f.Element`. The list itself is checked only at this level; the spec does not let a
  list field constrain its elements' inner structure (use a `BlTypeTable` field for
  records, or wrap structured rows in a higher-level schema).
- **`BlTypeDictionary`** — `v` is a `BlDictionary`; `f.Fields` validates it (recursively,
  with the same input / output policy).
- **`BlTypeTable`** — `v` is a `BlTable`; the declared columns in `f.Fields` are present on
  every row and each row's column values satisfy the column schemas. `ValidateInput`
  rejects rows with undeclared columns; `ValidateOutput` ignores them.

`BlNull` field values:

- A required field whose value is `BlNull` fails validation under both methods.
- An `Optional` field whose value is `BlNull` passes — it is treated as absence.

Failures produce a `BlSchemaError` carrying a path to the offending node
(`applicant.address.postalCode: expected BlString, got BlNumber`) so a single error message
locates the problem in a deeply nested value.

`[@test] ../../expr/schema_test.go`

## Parse-time use by `Bl.Expr`

`Bl.Expr` takes a `BlSchema` directly (see
[bl-expr.spec.md § Using the engine](bl-expr.spec.md#using-the-engine)): top-level field
names are the available variables, each variable's `Type` is its parse-time type, and
nested `Fields` are available to the member-access type-checker (so
`applicant.address.postalCode` can be statically verified when the relevant fields are
declared).

```go
// host-side (Go)
func (Bl) Expr(source string, schema BlSchema) (BlExpr, error)
```

## Migration

`BlSchema` is intended to subsume the types below. The mapping is mechanical; migration
will land incrementally.

| Existing | `BlSchema` equivalent | Notes |
|---|---|---|
| `InputContract` ([data-contract.spec.md:18](../data/data-contract.spec.md#L18)) | `BlSchema` validated via `ValidateInput` | Closed semantics preserved. |
| `OutputContract` ([data-contract.spec.md:25](../data/data-contract.spec.md#L25)) | `BlSchema` validated via `ValidateOutput` | Permissive semantics preserved. |
| `DictionaryContract` ([data-contract.spec.md:57](../data/data-contract.spec.md#L57)) | A field with `Type: BlTypeDictionary` and nested `Fields` | Direct mapping. |
| `ListContract` ([data-contract.spec.md:85](../data/data-contract.spec.md#L85)) | A field with `Type: BlTypeList` and an `Element` `[]BlType` | Existing `ListContract` allowed structured element types (e.g. a nested `DictionaryContract`); `BlSchema` element constraints are `BlType`-only. Structured-row use cases migrate to `BlTypeTable`. |
| `TableContract` ([data-contract.spec.md:105](../data/data-contract.spec.md#L105)) | A field with `Type: BlTypeTable` and nested `Fields` | Direct mapping. |
| Generic `Parameters` ([business-knowledge-model.spec.md:21](../decision-tasks/business-knowledge-model.spec.md#L21)) | `BlSchema` | The reflected-struct path is replaced by an explicit `BlSchema` carried on the BKM. |
| Generic `Outputs` ([decision-node.spec.md](../decision-tasks/decision-node.spec.md)) | `BlSchema` | Same — node carries a `BlSchema` describing its outputs. |

The `InputContract` / `OutputContract` Go-type split (which today makes wrong-direction use
a Go type error) collapses into one `BlSchema` type — direction discrimination moves to the
call site (`Start(...)` accepts the schema and validates as input; `End(...)` accepts the
schema and validates as output). Callers that want stronger direction typing can wrap with
named aliases at their boundary.

## Edge cases

- An empty `BlSchema{}` is valid. `ValidateInput` accepts only the empty `BlDictionary`;
  `ValidateOutput` accepts any `BlDictionary`.
- A `BlTypeDictionary` or `BlTypeTable` field with empty `Fields` is valid — same
  open/closed treatment as an empty top-level schema.
- `BlField{Name: "x", Type: BlTypeAny}` — a required field whose value can be any
  non-`BlNull` `BlValue`. Useful for permissive pass-through cases.
- Schemas are deep-comparable via `reflect.DeepEqual` (a `BlSchema` is a slice, so equality
  follows slice semantics).
- `BlSchema` is acyclic by construction: every `Fields` is supplied as a fully-built
  value, so no recursive shape can be expressed. Recursive process / decision shapes are
  out of scope for v1.
- `Element` order is irrelevant — it is a set of allowed `BlType`s, not a sequence.
  Duplicate entries are a well-formedness error (see § Construction).

`[@test] ../../expr/schema_edge_cases_test.go`
