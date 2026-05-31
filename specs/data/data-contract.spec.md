---
name: Input/Output Contracts
description: Typed input and output contracts (InputContract, OutputContract) — declare allowed and required data attributes and their types for process start/end events and decision model inputs/outputs, with DictionaryContract, ListContract, and TableContract for nested structured types
targets:
  - ../data/data_contract.go
---

# Input and Output Contracts

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

// FieldType is: BlType | DictionaryContract | ListContract | TableContract
```

`BlType` is any of the `Bl` value type classes (`BlNumber`, `BlString`, `BlBoolean`, `BlDate`, `BlTime`, `BlDateTime`, `BlYearsMonthsDuration`, `BlDaysTimeDuration`, `BlList`, `BlDictionary`, `BlRange`, `BlCalendar`).

---

## Nested Type Contracts

For fields that hold structured or collection values, three contract types provide nested type constraints. These are direction-agnostic — they describe value shape and can appear inside an `InputContract`, an `OutputContract`, or any nested contract.

### DictionaryContract

A `DictionaryContract` declares a structured value — a `BlDictionary` with named, typed fields. At runtime, the value must be a `BlDictionary` whose keys and value types conform to the declared fields.

```go
type DictionaryContract struct {
    Fields []ContractField
}

func NewDictionaryContract(fields ...ContractField) *DictionaryContract
```

```go
addressContract := NewDictionaryContract(
    RequiredField("street", BlString),
    RequiredField("city", BlString),
    RequiredField("postal_code", BlString),
    RequiredField("country", BlString),
)

applicantContract := NewDictionaryContract(
    RequiredField("name", BlString),
    RequiredField("age", BlNumber),
    RequiredField("income", BlNumber),
    OptionalField("address", addressContract),
)
```

### ListContract

A `ListContract` declares a typed list — a `BlList` where every element conforms to a specified type. The element type can be a `BlType`, `DictionaryContract`, `ListContract`, or `TableContract`.

```go
type ListContract struct {
    ElementType FieldType
}

func NewListContract(elementType FieldType) *ListContract
```

```go
// List of numbers
scoresContract := NewListContract(BlNumber)

// List of structured dictionaries
addressesContract := NewListContract(addressContract)
```

### TableContract

A `TableContract` declares a relation — a [`BlTable`](../expressions/table.spec.md) (an ordered list of uniformly-keyed `BlDictionary` rows) where each row conforms to the declared columns.

```go
type TableContract struct {
    Fields []ContractField
}

func NewTableContract(fields ...ContractField) *TableContract
```

```go
lineItemsContract := NewTableContract(
    RequiredField("product", BlString),
    RequiredField("quantity", BlNumber),
    RequiredField("unit_price", BlNumber),
)
```

A `TableContract` constrains values to be `BlTable` instances whose columns match the declared fields. It is conceptually similar to `ListContract.create(DictionaryContract.create(*fields))`, but binds to the typed `BlTable` value (with its uniform-keys invariant) rather than a loose `BlList[BlDictionary]`.

---

## Attaching to an Event Node

Every `StartEvent` carries an `*InputContract` — it is a mandatory positional argument to `Start()`, so a process cannot have a contract-less entrypoint. Every `EndEvent` may carry an optional `*OutputContract` via `EndOpts.Contract`. The public process API emerges from the contracts on its boundary nodes.

```go
start := Start("start", "New Application",
    NewInputContract(
        RequiredField("applicant", applicantContract),
        RequiredField("loan_amount", BlNumber),
        OptionalField("referral_code", BlString),
    ),
)

approved := End("approved", "Loan Approved", EndOpts{
    Contract: NewOutputContract(
        RequiredField("offer_id", BlString),
        RequiredField("approved_amount", BlNumber),
    ),
})

rejected := End("rejected", "Loan Rejected", EndOpts{
    Contract: NewOutputContract(
        RequiredField("rejection_reason", BlString),
    ),
})

loanApplication := NewProcess("loan-application", "1.0", ProcessOpts{
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
newApp := Start("new", "New Application",
    NewInputContract(
        RequiredField("applicant", applicantContract),
        RequiredField("loan_amount", BlNumber),
    ),
)

reassess := Start("reassess", "Re-assessment",
    NewInputContract(
        RequiredField("applicant", applicantContract),
        RequiredField("risk_level", BlString),
    ),
)

approved := End("approved", "Loan Approved", EndOpts{
    Contract: NewOutputContract(
        RequiredField("offer_id", BlString),
    ),
})

rejected := End("rejected", "Loan Rejected", EndOpts{
    Contract: NewOutputContract(
        RequiredField("rejection_reason", BlString),
    ),
})

loanDecision := NewProcess("loan-decision", "1.0", ProcessOpts{
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
    InputContract: NewInputContract(
        RequiredField("applicant", applicantContract),
        RequiredField("loan_amount", BlNumber),
    ),
    OutputContract: NewOutputContract(
        RequiredField("approval", BlString),
        RequiredField("risk_score", BlNumber),
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

When a process is submitted (via `MessageGateway.Submit`), the input variables are checked against the selected `StartEvent`'s `InputContract`. Because every `StartEvent` has a contract by construction, this validation is unconditional:

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
- `ListContract` — the value must be a `BlList`, and every element is checked against the declared `element_type`.
- `TableContract` — the value must be a `BlTable`, and each row is checked against the declared fields.

---

## Edge Cases

- `NewInputContract()` and `NewOutputContract()` with no fields are valid but perform no field-level validation — they accept any input/output.
- `Start()` requires an `*InputContract` argument. Passing `nil` is a programmer error and produces a `ProcessDefinitionError` at construction time.
- An `EndEvent` constructed via `End(id, name)` (no `EndOpts`) or `End(id, name, EndOpts{})` (zero opts) has a `nil` `Contract` and performs no output validation.
- A `DecisionTask` with both `InputContract == nil` and `OutputContract == nil` performs no validation; one-set, one-nil is allowed and the set one is independently validated.
- Duplicate field names within the same `NewInputContract(...)` or `NewOutputContract(...)` call produce a `DataContractValidationError` at definition time.
- `DataContractValidationError` at submission time prevents the process from running — the submission is rejected synchronously (the start-command is never published to the broker).
- `DataContractValidationError` at completion time causes the process to fail — the failure is recorded as `PROCESS_FAILED` in the `ExecutionHistory` with the error attached.
- A field typed as `BlList` (the class, not a `ListContract`) declares that the value must be a list, but does not constrain the element type. Use `ListContract` for element-level type constraints.
- A field typed as `BlDictionary` (the class, not a `DictionaryContract`) declares that the value must be a dictionary, but does not constrain its keys or value types. Use `DictionaryContract` for structural constraints on dictionary values.
- `BlNull` is not a valid field type — a field that may be absent should be declared as `optional`. At evaluation time, a missing input resolves to `BlNull` regardless of contract.
- A single `*InputContract` may be shared by reference across multiple `StartEvent`s with identical input shapes; the same applies to `*OutputContract` across multiple `EndEvent`s. Contracts are read-only after construction, so sharing is safe.
- `NewDictionaryContract()` with no fields is valid — it constrains the value to be a `BlDictionary` but imposes no field requirements.
- `NewListContract(elementType)` with a nested `ListContract` is valid — represents a list of lists.
- `NewTableContract()` with no fields is valid — it constrains the value to be a `BlTable` but imposes no column requirements.
- Duplicate field names in a `DictionaryContract` or `TableContract` produce a `DataContractValidationError` at definition time.
