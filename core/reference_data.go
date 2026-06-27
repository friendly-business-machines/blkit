package core

import "strings"

// ReferenceData is a static value source within a decision model: a single
// constant BlValue paired with identity. It is not a DecisionNode and never runs;
// it exposes a single typed Value handle so it can be wired into a DecisionTask
// graph with Edge, exactly like a node output.
type ReferenceData[T BlValue] struct {
	Id          string
	Name        string
	Description string

	// Value is the constant exposed as a wireable handle, stamped with this
	// source's Id. Read the underlying BlValue with GetValue (or Value.Get()).
	Value Handle[T]
}

// ReferenceDataConfig configures a ReferenceData. Value is the constant; Id is
// mandatory. T is given by the config's type argument.
type ReferenceDataConfig[T BlValue] struct {
	Id          string
	Name        string
	Description string
	Value       T
}

// ReferenceValue is the non-generic view of a ReferenceData, so a DecisionTask can
// hold the heterogeneous set of value sources it discovers from its graph edges.
type ReferenceValue interface {
	GetId() string
	GetName() string
	GetDescription() string
	GetValue() BlValue
}

// referenceField is the sentinel field name the Value handle is stamped with.
const referenceField = "$value"

// NewReferenceData builds a ReferenceData from its config, wrapping the constant
// into the .Value handle stamped with the Id. An empty Id, or a nil Value, panics
// with a *DecisionDefinitionError.
func NewReferenceData[T BlValue](config ReferenceDataConfig[T]) *ReferenceData[T] {
	var problems []string
	if config.Id == "" {
		problems = append(problems, "reference data requires a non-empty Id")
	}
	if bv := BlValue(config.Value); bv == nil {
		problems = append(problems, "reference data requires a non-nil Value")
	}
	if len(problems) > 0 {
		panic(&DecisionDefinitionError{Node: nodeLabel(config.Id, config.Name, "ReferenceData"), Problems: problems})
	}
	r := &ReferenceData[T]{Id: config.Id, Name: config.Name, Description: config.Description}
	r.Value = NewHandle(config.Value)
	(&r.Value).stamp(r, config.Id, referenceField)
	return r
}

// GetId / GetName / GetDescription / GetValue satisfy ReferenceValue.
func (r *ReferenceData[T]) GetId() string          { return r.Id }
func (r *ReferenceData[T]) GetName() string        { return r.Name }
func (r *ReferenceData[T]) GetDescription() string { return r.Description }
func (r *ReferenceData[T]) GetValue() BlValue      { return r.Value.getValue() }

// ToMarkdown renders the value source as markdown.
func (r *ReferenceData[T]) ToMarkdown() string {
	var b strings.Builder
	label := r.Name
	if label == "" {
		label = r.Id
	}
	b.WriteString("### " + label + "\n\n")
	if r.Description != "" {
		b.WriteString("_" + r.Description + "_\n\n")
	}
	b.WriteString("**Value:** `" + r.GetValue().String() + "`\n")
	return b.String()
}
