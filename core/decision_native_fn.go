package core

import (
	"fmt"
	"reflect"
	"runtime"
	"strings"
	"time"
)

// DecisionNativeFunction is a DecisionNode whose logic is a plain Go function
// func(I) (O, error) over concrete input/output structs of Handle fields. It is
// the escape hatch for logic that is neither a table nor an expression.
type DecisionNativeFunction[I, O any] struct {
	In  I // input port surface for wiring
	Out O // output port surface

	id          string
	name        string
	description string
	fn          func(I) (O, error)
	conc        bool
	retry       *RetryConfig

	ivars, ovars []decisionVar
}

// DecisionNativeFunctionConfig configures a DecisionNativeFunction. The function
// itself is passed as a second constructor argument so I and O are inferred.
type DecisionNativeFunctionConfig struct {
	Id          string
	Name        string
	Description string

	// Concurrent lets a containing DecisionTask overlap this node with others.
	Concurrent bool

	// Retry, when non-nil, re-runs Fn on a non-nil error per the RetryConfig.
	Retry *RetryConfig
}

// NewDecisionNativeFunction builds a node from a config and a typed function. It
// validates the contracts (every I/O field a Handle, non-empty O), requires a
// non-nil fn, and rejects a Retry with no limit. Problems are accumulated and
// raised once as a *DecisionDefinitionError.
func NewDecisionNativeFunction[I, O any](config DecisionNativeFunctionConfig, fn func(I) (O, error)) *DecisionNativeFunction[I, O] {
	var problems []string
	iType := reflect.TypeOf((*I)(nil)).Elem()
	oType := reflect.TypeOf((*O)(nil)).Elem()
	ivars, ip := reflectContract(iType)
	for _, p := range ip {
		problems = append(problems, "input "+p)
	}
	ovars, op := reflectContract(oType)
	for _, p := range op {
		problems = append(problems, "output "+p)
	}
	if len(ovars) == 0 {
		problems = append(problems, "at least one output field is required")
	}
	if fn == nil {
		problems = append(problems, "a non-nil Fn is required")
	}
	if config.Retry != nil && !config.Retry.bounded() {
		problems = append(problems, "Retry must set MaxRetries or RetryFor")
	}
	if len(problems) > 0 {
		panic(&DecisionDefinitionError{Node: nodeLabel(config.Id, config.Name, "DecisionNativeFunction"), Problems: problems})
	}

	d := &DecisionNativeFunction[I, O]{
		id:          config.Id,
		name:        config.Name,
		description: config.Description,
		fn:          fn,
		conc:        config.Concurrent,
		retry:       config.Retry,
		ivars:       ivars,
		ovars:       ovars,
	}
	d.In = stampSurface(d, d.id, iType, ivars).Interface().(I)
	d.Out = stampSurface(d, d.id, oType, ovars).Interface().(O)
	return d
}

// attempt runs Fn once, recovering a panic into an error.
func (d *DecisionNativeFunction[I, O]) attempt(in I) (out O, err error) {
	defer func() {
		if r := recover(); r != nil {
			err = fmt.Errorf("panic in native function: %v", r)
		}
	}()
	return d.fn(in)
}

// Evaluate runs Fn against the typed input and returns the typed output. A panic
// is recovered into an error; an error is retried per Retry; the final error (if
// any) is tagged with the node Id.
func (d *DecisionNativeFunction[I, O]) Evaluate(in I) (O, error) {
	out, err := d.attempt(in)
	if err != nil && d.retry != nil {
		start := time.Now()
		delay := d.retry.RetryDelay
		for attempts := 0; err != nil; attempts++ {
			if d.retry.MaxRetries > 0 && attempts >= d.retry.MaxRetries {
				break
			}
			if d.retry.RetryFor > 0 && time.Since(start) >= d.retry.RetryFor {
				break
			}
			if delay > 0 {
				time.Sleep(delay)
				if d.retry.ExponentialBackoff {
					delay *= 2
				}
			}
			out, err = d.attempt(in)
		}
	}
	if err != nil {
		return out, fmt.Errorf("%s: %w", nodeLabel(d.id, d.name, "DecisionNativeFunction"), err)
	}
	return out, nil
}

// Concurrent reports whether a containing DecisionTask may overlap this node.
func (d *DecisionNativeFunction[I, O]) Concurrent() bool { return d.conc }

// DecisionNode[I, O] interface satisfaction.
func (d *DecisionNativeFunction[I, O]) GetId() string          { return d.id }
func (d *DecisionNativeFunction[I, O]) GetName() string        { return d.name }
func (d *DecisionNativeFunction[I, O]) GetDescription() string { return d.description }
func (d *DecisionNativeFunction[I, O]) Inputs() []Field        { return fieldsOf(d.ivars) }
func (d *DecisionNativeFunction[I, O]) Outputs() []Field       { return fieldsOf(d.ovars) }

// erased decisionNode satisfaction.
func (d *DecisionNativeFunction[I, O]) inVars() []decisionVar  { return d.ivars }
func (d *DecisionNativeFunction[I, O]) outVars() []decisionVar { return d.ovars }
func (d *DecisionNativeFunction[I, O]) concurrent() bool       { return d.conc }

func (d *DecisionNativeFunction[I, O]) runErased(in map[string]BlValue) (map[string]BlValue, error) {
	input, err := inputFromMap(reflect.TypeOf((*I)(nil)).Elem(), d.ivars, in)
	if err != nil {
		return nil, err
	}
	out, err := d.Evaluate(input.(I))
	if err != nil {
		return nil, err
	}
	return outputToMap(out, d.ovars), nil
}

// ToMarkdown renders the node: name heading, optional description, a Logic line
// naming the bound function, and the input/output contracts as typed tables.
func (d *DecisionNativeFunction[I, O]) ToMarkdown() string {
	var b strings.Builder
	label := d.name
	if label == "" {
		label = d.id
	}
	b.WriteString("### " + label + "\n\n")
	if d.description != "" {
		b.WriteString("_" + d.description + "_\n\n")
	}
	logic := "**Logic:** native Go function `" + funcName(d.fn) + "`"
	if d.conc {
		logic += " · runs concurrently"
	}
	b.WriteString(logic + "\n\n")
	b.WriteString("#### Inputs\n\n")
	contractTable(&b, d.ivars)
	b.WriteString("\n#### Outputs\n\n")
	contractTable(&b, d.ovars)
	return b.String()
}

// contractTable writes a Name/Type markdown table for a set of variables.
func contractTable(b *strings.Builder, vars []decisionVar) {
	rows := make([][2]string, len(vars))
	for i, v := range vars {
		rows[i] = [2]string{v.name, typeLabel(v.blType)}
	}
	mdTable(b, "Name", "Type", rows)
}

// funcName recovers a function's Go name by reflection.
func funcName(fn any) string {
	rv := reflect.ValueOf(fn)
	if rv.Kind() != reflect.Func {
		return "func"
	}
	f := runtime.FuncForPC(rv.Pointer())
	if f == nil {
		return "func"
	}
	name := f.Name()
	if i := strings.LastIndex(name, "."); i >= 0 {
		name = name[i+1:]
	}
	return name
}

// typeLabel renders a Type as a capitalised label (e.g. "Number").
func typeLabel(t Type) string {
	n := typeName(t)
	if n == "" {
		return n
	}
	return strings.ToUpper(n[:1]) + n[1:]
}
