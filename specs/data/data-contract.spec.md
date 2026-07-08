---
name: Input/Output Contracts
description: Typed input and output contracts (InputContract, OutputContract) — declare allowed and required data attributes and their types for process start/end events and decision model inputs/outputs, with DictionaryContract, ListContract, and TableContract for nested structured types
targets:
  - ../../core/data_contract.go
---

# Input and Output Contracts

> **Status:** The boundary-validation subset of this spec is implemented in
> core (`core/data_contract.go`): `InputContract` / `OutputContract` wrap a
> `BlSchema`, `RequiredField` / `OptionalField` take a `bl.Type`, nested
> shapes are declared via `Field.Fields` / `Field.Element`, and contracts are
> CBOR-serializable so they travel through the message-broker registry. The
> richer construction DSL below (`DictionaryContract` / `ListContract` /
> `TableContract` wrappers) lands with the process layer.

`InputContract` and `OutputContract` are typed contracts — sets of named, typed fields that declare what data a component expects or produces.

- An **`InputContract`** describes inbound data. It is attached to a `StartEvent` (mandatory; validated at submission) and to a `DecisionTask` (optional; validated at evaluation entry).
- An **`OutputContract`** describes outbound data. It is attached to an `EndEvent` (optional; validated at completion) and to a `DecisionTask` (optional; validated at evaluation exit).

The two types are structurally identical but distinct so that the wrong direction is a Go type error, not a runtime check. Fields reference `Bl` type classes directly rather than type name strings, giving compile-time safety in typed languages. For structured or collection types, `DictionaryContract`, `ListContract`, and `TableContract` provide nested type constraints.

```go
type InputContract struct {
    Fields []ContractField
}

func NewInputContract(fields ...ContractField) *InputContract


type OutputContract struct {
    Fields []ContractField
}

func NewOutputContract(fields ...ContractField) *OutputContract


type ContractField struct {
    Name       string
    Type       FieldType
    IsRequired bool
}

func RequiredField(name string, fieldType FieldType) ContractField
func OptionalField(name string, fieldType FieldType) ContractField

// FieldType is: bl.Type | DictionaryContract | ListContract | TableContract
```

`bl.Type` is any of the `Bl` value type classes (`bl.BlNumber`, `bl.BlString`, `bl.BlBoolean`, `bl.BlDate`, `bl.BlTime`, `bl.BlDateTime`, `bl.BlYearsMonthsDuration`, `bl.BlDaysTimeDuration`, `bl.BlList`, `bl.BlDictionary`, `bl.BlRange`, `bl.BlCalendar`).

---

## Nested Type Contracts

For fields that hold structured or collection values, three contract types provide nested type constraints. These are direction-agnostic — they describe value shape and can appear inside an `InputContract`, an `OutputContract`, or any nested contract.

### DictionaryContract

A `DictionaryContract` declares a structured value — a `bl.BlDictionary` with named, typed fields. At runtime, the value must be a `bl.BlDictionary` whose keys and value types conform to the declared fields.

```go
type DictionaryContract struct {
    Fields []ContractField
}

func NewDictionaryContract(fields ...ContractField) *DictionaryContract
```

```go
addressContract := bl.NewDictionaryContract(
    bl.RequiredField("street", bl.BlString),
    bl.RequiredField("city", bl.BlString),
    bl.RequiredField("postal_code", bl.BlString),
    bl.RequiredField("country", bl.BlString),
)

applicantContract := bl.NewDictionaryContract(
    bl.RequiredField("name", bl.BlString),
    bl.RequiredField("age", bl.BlNumber),
    bl.RequiredField("income", bl.BlNumber),
    bl.OptionalField("address", addressContract),
)
```

### ListContract

A `ListContract` declares a typed list — a `bl.BlList` where every element conforms to a specified type. The element type can be a `bl.Type`, `DictionaryContract`, `ListContract`, or `TableContract`.

```go
type ListContract struct {
    ElementType FieldType
}

func NewListContract(elementType FieldType) *ListContract
```

```go
// List of numbers
scoresContract := bl.NewListContract(bl.BlNumber)

// List of structured dictionaries
addressesContract := bl.NewListContract(addressContract)
```

### TableContract

A `TableContract` declares a relation — a [`bl.BlTable`](../expressions/table.spec.md) (an ordered list of uniformly-keyed `bl.BlDictionary` rows) where each row conforms to the declared columns.

```go
type TableContract struct {
    Fields []ContractField
}

func NewTableContract(fields ...ContractField) *TableContract
```

```go
lineItemsContract := bl.NewTableContract(
    bl.RequiredField("product", bl.BlString),
    bl.RequiredField("quantity", bl.BlNumber),
    bl.RequiredField("unit_price", bl.BlNumber),
)
```

A `TableContract` constrains values to be `bl.BlTable` instances whose columns match the declared fields. It is conceptually similar to `ListContract.create(DictionaryContract.create(*fields))`, but binds to the typed `bl.BlTable` value (with its uniform-keys invariant) rather than a loose `bl.BlList[bl.BlDictionary]`.

---

## Attaching to an Event Node

Every `StartEvent` carries an `*InputContract` — it is a mandatory positional argument to `bl.Start()`, so a process cannot have a contract-less entrypoint. Every `EndEvent` may carry an optional `*OutputContract` via `EndOpts.Contract`. The public process API emerges from the contracts on its boundary nodes.

```go
start := bl.Start("start", "New Application",
    bl.NewInputContract(
        bl.RequiredField("applicant", applicantContract),
        bl.RequiredField("loan_amount", bl.BlNumber),
        bl.OptionalField("referral_code", bl.BlString),
    ),
)

approved := bl.End("approved", "Loan Approved", EndOpts{
    Contract: bl.NewOutputContract(
        bl.RequiredField("offer_id", bl.BlString),
        bl.RequiredField("approved_amount", bl.BlNumber),
    ),
})

rejected := bl.End("rejected", "Loan Rejected", EndOpts{
    Contract: bl.NewOutputContract(
        bl.RequiredField("rejection_reason", bl.BlString),
    ),
})

loanApplication := bl.NewProcess("loan-application", "1.0", ProcessOpts{
    Name: "Loan Application",
    Graph: []ProcessNode{
        start.To(/* ... */).To(approved),
        /* ... */.To(rejected),
    },
})
```

### Multiple Start Nodes

For processes with multiple entrypoints, each start node carries its own `InputContract` directly — no special multi-start handling is required.

```go
newApp := bl.Start("new", "New Application",
    bl.NewInputContract(
        bl.RequiredField("applicant", applicantContract),
        bl.RequiredField("loan_amount", bl.BlNumber),
    ),
)

reassess := bl.Start("reassess", "Re-assessment",
    bl.NewInputContract(
        bl.RequiredField("applicant", applicantContract),
        bl.RequiredField("risk_level", bl.BlString),
    ),
)

approved := bl.End("approved", "Loan Approved", EndOpts{
    Contract: bl.NewOutputContract(
        bl.RequiredField("offer_id", bl.BlString),
    ),
})

rejected := bl.End("rejected", "Loan Rejected", EndOpts{
    Contract: bl.NewOutputContract(
        bl.RequiredField("rejection_reason", bl.BlString),
    ),
})

loanDecision := bl.NewProcess("loan-decision", "1.0", ProcessOpts{
    Name: "Loan Decision Process",
    Graph: []ProcessNode{
        newApp.To(/* ... */),
        reassess.To(/* ... */),
        /* ... */.To(approved),
        /* ... */.To(rejected),
    },
})
```

See [event-nodes.spec.md](../processes/event-nodes.spec.md) for full `StartEvent` and `EndEvent` definitions and constructor signatures.

---

## Attaching to a DecisionTask

A `DecisionTask` carries two independent contract fields — `InputContract` and `OutputContract` — both optional. The input contract declares expected callers; the output contract declares which computed values are exposed.

```go
loanModel := &DecisionTask{
    Id:   "loan-model",
    Name: "Loan Approval Model",
    InputContract: bl.NewInputContract(
        bl.RequiredField("applicant", applicantContract),
        bl.RequiredField("loan_amount", bl.BlNumber),
    ),
    OutputContract: bl.NewOutputContract(
        bl.RequiredField("approval", bl.BlString),
        bl.RequiredField("risk_score", bl.BlNumber),
    ),
}
```

- **`InputContract`** declares the named, typed variables the model expects from callers. At evaluation time, input values are validated against the contract. A mismatch produces a `DataContractValidationError`.
- **`OutputContract`** declares which computed values are exposed in `DecisionTaskResult.outputs` and their expected types. The field names must match node `output_name` (or `id`) values.

See [decision-task.spec.md](../decision-tasks/decision-task.spec.md) for full integration details.

---

## Validation

### Event-node validation (at submission and completion)

Validation runs only at the process boundaries — internal tasks are not contract-validated.

#### Input validation (at submission)

When a process is submitted (via `MessageBroker.Submit`), the input variables are checked against the selected `StartEvent`'s `InputContract`. Submission-time validation runs against the contract carried in the broker registry's `ProcessRegistration` (see [message-brokers/overview.spec.md](../message-brokers/overview.spec.md#operation-flows)), so the submitting client does not import the process package. Because every `StartEvent` has a contract by construction, this validation is unconditional:

1. Every **required** field must be present in the input. If missing, a `DataContractValidationError` is produced.
2. Every field present in the input must be **declared** in the contract (either required or optional). Undeclared fields produce a `DataContractValidationError`.
3. Every field's value must conform to the declared **type**. A type mismatch produces a `DataContractValidationError`.

A `DataContractValidationError` at submission rejects the submission synchronously — the start-command is never published.

#### Output validation (at completion)

When the runtime reaches an `EndEvent` whose `Contract != nil`, the `OutputContract` is checked against the variables in the `ExecutionContext` before `PROCESS_COMPLETED` is recorded:

1. Every **required** field must be present in the context. If missing, a `DataContractValidationError` is produced.
2. Every field's value must conform to the declared **type**. A type mismatch produces a `DataContractValidationError`.
3. Undeclared fields in the context are **not** rejected at output — only the declared fields are validated.

A `DataContractValidationError` at completion fails the process — the failure is recorded as `PROCESS_FAILED` in the `ExecutionHistory` with the error attached.

An `EndEvent` with a `nil` `Contract` performs no output validation.

### DecisionTask validation

When a `DecisionTask` has an `InputContract` set, `model.Validate()` checks:

- Every field declared in the input contract is referenced by at least one node's `requires`.
- At evaluation time, input values are validated against the contract types. A mismatch produces a `DataContractValidationError`.

When an `OutputContract` is set, `model.Validate()` additionally checks:

- Every field in the output contract matches a node's `output_name` (or `id`).
- Node output types are compatible with the declared contract types (where statically determinable).

### Nested type validation

Nested types are validated recursively:

- `DictionaryContract` — each field is checked for presence (required/optional) and type conformance.
- `ListContract` — the value must be a `bl.BlList`, and every element is checked against the declared `element_type`.
- `TableContract` — the value must be a `bl.BlTable`, and each row is checked against the declared fields.

---

## CBOR Encoding

CBOR (RFC 8949) is the canonical wire and storage encoding for `Bl` values —
used by the [state stores](../state-stores/overview.spec.md) to persist values
and by the [message brokers](../message-brokers/overview.spec.md#wire-format)
for every payload, including the contracts themselves. CBOR's semantic tags
preserve `Bl` types losslessly; JSON, where produced, is a human-readable copy
only and is never used for deserialization.

### Bl values

| Bl type | CBOR representation |
|---|---|
| `bl.BlNumber` | Decimal fraction (tag 4) |
| `bl.BlString` | Text string |
| `bl.BlBoolean` | Boolean |
| `bl.BlNull` | Null |
| `bl.BlDate` | Tagged map (blkit tag) with year, month, day, offset, timezone |
| `bl.BlTime` | Tagged map (blkit tag) with hour, minute, second, offset, timezone |
| `bl.BlDateTime` | Tagged map (blkit tag) with date and time components |
| `bl.BlYearsMonthsDuration` | Tagged map (blkit tag) with years, months |
| `bl.BlDaysTimeDuration` | Tagged map (blkit tag) with days, hours, minutes, seconds |
| `bl.BlList` | Array of `Bl` values |
| `bl.BlDictionary` | Map of string keys to `Bl` values |
| `bl.BlTable` | Tagged array (blkit tag) of uniformly-keyed maps |
| `bl.BlRange` | Tagged map (blkit tag) with start, end, and inclusive flags |
| `bl.BlCalendar` | Tagged map (blkit tag) with the calendar definition |

`Bl` types with blkit-specific attributes (e.g. `bl.BlDate` with offset and
timezone) use blkit-defined semantic tags from CBOR's private-use range,
encoding the full set of attributes as a CBOR map. This ensures lossless
round-tripping of all `Bl` values regardless of custom attributes.

### Contracts

`InputContract`, `OutputContract`, and the nested contract types are
themselves CBOR-serializable: a contract encodes as a map of its fields, each
field carrying its name, required flag, and type — where the type is either a
`Bl` type-class identifier or a nested contract map. This is what lets a
`ProcessRegistration` carry its start events' contracts through the message
broker at worker-registration time, derived from the process definition at
runtime — no build step or generated artifacts — so producers can validate a
`Submit` without importing the process package.

---

## Edge Cases

- `bl.NewInputContract()` and `bl.NewOutputContract()` with no fields are valid but perform no field-level validation — they accept any input/output.
- `bl.Start()` requires an `*InputContract` argument. Passing `nil` is a programmer error and produces a `ProcessDefinitionError` at construction time.
- An `EndEvent` constructed via `bl.End(id, name)` (no `EndOpts`) or `bl.End(id, name, EndOpts{})` (zero opts) has a `nil` `Contract` and performs no output validation.
- A `DecisionTask` with both `InputContract == nil` and `OutputContract == nil` performs no validation; one-set, one-nil is allowed and the set one is independently validated.
- Duplicate field names within the same `bl.NewInputContract(...)` or `bl.NewOutputContract(...)` call produce a `DataContractValidationError` at definition time.
- `DataContractValidationError` at submission time prevents the process from running — the submission is rejected synchronously (the start-command is never published to the broker).
- `DataContractValidationError` at completion time causes the process to fail — the failure is recorded as `PROCESS_FAILED` in the `ExecutionHistory` with the error attached.
- A field typed as `bl.BlList` (the class, not a `ListContract`) declares that the value must be a list, but does not constrain the element type. Use `ListContract` for element-level type constraints.
- A field typed as `bl.BlDictionary` (the class, not a `DictionaryContract`) declares that the value must be a dictionary, but does not constrain its keys or value types. Use `DictionaryContract` for structural constraints on dictionary values.
- `bl.BlNull` is not a valid field type — a field that may be absent should be declared as `optional`. At evaluation time, a missing input resolves to `bl.BlNull` regardless of contract.
- A single `*InputContract` may be shared by reference across multiple `StartEvent`s with identical input shapes; the same applies to `*OutputContract` across multiple `EndEvent`s. Contracts are read-only after construction, so sharing is safe.
- `bl.NewDictionaryContract()` with no fields is valid — it constrains the value to be a `bl.BlDictionary` but imposes no field requirements.
- `bl.NewListContract(elementType)` with a nested `ListContract` is valid — represents a list of lists.
- `bl.NewTableContract()` with no fields is valid — it constrains the value to be a `bl.BlTable` but imposes no column requirements.
- Duplicate field names in a `DictionaryContract` or `TableContract` produce a `DataContractValidationError` at definition time.
