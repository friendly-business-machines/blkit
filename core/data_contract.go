package core

import "fmt"

// Input and output contracts (specs/data/data-contract.spec.md) — typed sets
// of named fields declaring what data a process boundary expects or produces.
// A contract wraps a BlSchema, the runtime shape validator: the schema's
// fields are plain data (names, Type enums, nesting), so contracts serialize
// through the message broker's CBOR wire format and a producer can validate a
// Submit against a registry-carried contract without importing the process
// package (specs/message-brokers/overview.spec.md § Wire format).
//
// This is the boundary-validation subset of the data-contract spec. The
// richer construction DSL (DictionaryContract / ListContract / TableContract
// wrappers) lands with the process layer; nested shapes are declared today
// via Field.Fields / Field.Element directly.

// InputContract describes inbound data: attached to a StartEvent (validated
// at submission) and to a DecisionTask (validated at evaluation entry).
// Validation is closed — undeclared fields are rejected.
type InputContract struct {
	Fields BlSchema `cbor:"fields"`
}

// OutputContract describes outbound data: attached to an EndEvent (validated
// at completion) and to a DecisionTask (validated at evaluation exit).
// Validation is permissive — undeclared fields are accepted.
type OutputContract struct {
	Fields BlSchema `cbor:"fields"`
}

// NewInputContract builds an InputContract from the supplied fields,
// validating well-formedness (non-empty unique names, known types, coherent
// nesting).
func NewInputContract(fields ...Field) (*InputContract, error) {
	s, err := Schema(fields...)
	if err != nil {
		return nil, err
	}
	return &InputContract{Fields: s}, nil
}

// NewOutputContract builds an OutputContract from the supplied fields,
// validating well-formedness.
func NewOutputContract(fields ...Field) (*OutputContract, error) {
	s, err := Schema(fields...)
	if err != nil {
		return nil, err
	}
	return &OutputContract{Fields: s}, nil
}

// RequiredField declares a mandatory contract field of the given type.
func RequiredField(name string, t Type) Field {
	return Field{Name: name, Type: t}
}

// OptionalField declares an optional contract field of the given type.
func OptionalField(name string, t Type) Field {
	return Field{Name: name, Type: t, Optional: true}
}

// Validate checks input variables against the contract: every required field
// present, no undeclared fields, every value conforming to its declared type.
// Returns a *DataContractValidationError on mismatch.
func (c *InputContract) Validate(input map[string]any) error {
	return validateContract(c.Fields, input, true)
}

// Validate checks output variables against the contract: every required field
// present and type-conforming; undeclared fields are not rejected.
func (c *OutputContract) Validate(output map[string]any) error {
	return validateContract(c.Fields, output, false)
}

func validateContract(s BlSchema, vars map[string]any, closed bool) error {
	if vars == nil {
		vars = map[string]any{}
	}
	v, err := wrap(vars)
	if err != nil {
		return &DataContractValidationError{Err: err}
	}
	if closed {
		err = s.ValidateInput(v)
	} else {
		err = s.ValidateOutput(v)
	}
	if err != nil {
		if se, ok := err.(*SchemaError); ok {
			return &DataContractValidationError{Path: se.Path, Err: se.Err}
		}
		return &DataContractValidationError{Err: err}
	}
	return nil
}

// DataContractValidationError reports a value that does not conform to a
// contract — a missing required field, an undeclared field, or a type
// mismatch (specs/data/data-contract.spec.md § Validation).
type DataContractValidationError struct {
	Path string // dotted path to the offending field; empty for value-level errors
	Err  error
}

func (e *DataContractValidationError) Error() string {
	if e.Path == "" {
		return fmt.Sprintf("data contract violation: %v", e.Err)
	}
	return fmt.Sprintf("data contract violation at %s: %v", e.Path, e.Err)
}

func (e *DataContractValidationError) Unwrap() error { return e.Err }
