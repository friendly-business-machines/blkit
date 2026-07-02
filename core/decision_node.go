package core

import (
	"fmt"
	"reflect"
	"strconv"
)

// Handle is one input or output field of a decision node's typed contract. It
// plays two roles: at wiring time it is a typed connection point (it carries its
// owning node and field name, stamped at construction), and at evaluation time it
// carries the value. Connecting two handles with Edge requires their T to match,
// so a mis-typed connection is a Go compile error.
type Handle[T BlValue] struct {
	own   any    // owning node / reference data / task boundary
	node  string // owner id
	field string // variable name (the field's expr name)
	val   T
	set   bool
}

// NewHandle wraps a value in a handle — used to build a typed input struct when
// calling Evaluate standalone. Inside a task the engine populates handles itself.
func NewHandle[T BlValue](value T) Handle[T] { return Handle[T]{val: value, set: true} }

// Get returns the handle's value.
func (h Handle[T]) Get() T { return h.val }

// --- erased handle access (used by the reflection-driven node/task machinery) ---

// roHandle is the read-only erased view of a handle (value receiver).
type roHandle interface {
	owner() any
	elemType() reflect.Type
	blType() Type
	getValue() BlValue
	ref() (node, field string)
}

// anyHandle adds the mutating operations (pointer receiver).
type anyHandle interface {
	roHandle
	stamp(owner any, node, field string)
	setValue(v BlValue) error
}

func (h Handle[T]) owner() any             { return h.own }
func (h Handle[T]) elemType() reflect.Type { return reflect.TypeOf((*T)(nil)).Elem() }
func (h Handle[T]) blType() Type           { var z T; return z.Type() }
func (h Handle[T]) ref() (string, string)  { return h.node, h.field }

func (h Handle[T]) getValue() BlValue {
	if h.set {
		return h.val
	}
	return Null()
}

func (h *Handle[T]) stamp(owner any, node, field string) {
	h.own, h.node, h.field = owner, node, field
}

func (h *Handle[T]) setValue(v BlValue) error {
	tv, ok := v.(T)
	if !ok {
		var z T
		return &TypeError{Op: "evaluate", Detail: fmt.Sprintf("field %q: cannot assign %s to %s", h.field, typeName(asBl(v).Type()), typeName(z.Type()))}
	}
	h.val, h.set = tv, true
	return nil
}

var anyHandleType = reflect.TypeOf((*anyHandle)(nil)).Elem()

// isHandleType reports whether t is some Handle[T] (only *Handle[T] satisfies the
// mutating anyHandle interface, so check the pointer).
func isHandleType(t reflect.Type) bool {
	return reflect.PointerTo(t).Implements(anyHandleType)
}

// DecisionNode is the typed interface every decision node satisfies: identity, the
// reflected input/output contracts, and a typed Evaluate. A DecisionTask is itself
// a DecisionNode, so a whole decision composes into a larger one.
type DecisionNode[I, O any] interface {
	GetId() string
	GetName() string
	GetDescription() string
	Inputs() []Field
	Outputs() []Field
	Evaluate(in I) (O, error)
}

// decisionNode is the non-generic erased view a DecisionTask drives. Every concrete
// node implements it (all blkit is one package, so it is unexported and sealed).
type decisionNode interface {
	GetId() string
	GetName() string
	GetDescription() string
	inVars() []decisionVar
	outVars() []decisionVar
	runErased(in map[string]BlValue) (map[string]BlValue, error)
}

// decisionVar describes one variable of a contract struct: its expr name, its Go
// struct field index, and the BlValue type it carries.
type decisionVar struct {
	name   string       // expr variable name (field name or `expr:"name"` tag)
	field  int          // struct field index
	elem   reflect.Type // T (the BlValue type inside Handle[T])
	blType Type         // T's language Type
}

// reflectContract walks a contract struct type, validating that every exported
// field is a Handle[BlValue] under a valid, unique expr name. It returns the
// variables and any problems (accumulated, not raised).
func reflectContract(t reflect.Type) (vars []decisionVar, problems []string) {
	if t.Kind() != reflect.Struct {
		return nil, []string{fmt.Sprintf("type %s is not a struct", t)}
	}
	seen := map[string]bool{}
	for i := 0; i < t.NumField(); i++ {
		f := t.Field(i)
		name, problem := decisionFieldName(f)
		if problem != "" {
			problems = append(problems, problem)
			continue
		}
		if !isHandleType(f.Type) {
			problems = append(problems, fmt.Sprintf("field %q (%s) is not a bl.Handle[BlValue]", name, f.Type))
			continue
		}
		if seen[name] {
			problems = append(problems, fmt.Sprintf("duplicate variable name %q", name))
			continue
		}
		seen[name] = true
		h := reflect.New(f.Type).Interface().(anyHandle)
		vars = append(vars, decisionVar{name: name, field: i, elem: h.elemType(), blType: h.blType()})
	}
	return vars, problems
}

// envType builds a strict struct env type whose fields are the variables' BlValue
// types, tagged with their expr names — the env an expression compiles against.
func envType(vars []decisionVar) (reflect.Type, map[string]bool) {
	fields := make([]reflect.StructField, len(vars))
	declared := make(map[string]bool, len(vars))
	for i, v := range vars {
		fields[i] = reflect.StructField{
			Name: "F" + strconv.Itoa(i),
			Type: v.elem,
			Tag:  reflect.StructTag(`expr:"` + v.name + `"`),
		}
		declared[v.name] = true
	}
	if len(fields) == 0 {
		return reflect.StructOf(nil), declared
	}
	return reflect.StructOf(fields), declared
}

// makeEnv builds an env value of type envT from named values, in vars order.
func makeEnv(envT reflect.Type, vars []decisionVar, vals map[string]BlValue) reflect.Value {
	env := reflect.New(envT).Elem()
	for i, v := range vars {
		val, ok := vals[v.name]
		if !ok || val == nil {
			continue
		}
		if reflect.TypeOf(val).AssignableTo(v.elem) {
			env.Field(i).Set(reflect.ValueOf(val))
		}
	}
	return env
}

// fieldsOf renders the variables as a []Field contract.
func fieldsOf(vars []decisionVar) []Field {
	fs := make([]Field, len(vars))
	for i, v := range vars {
		fs[i] = Field{Name: v.name, Type: v.blType}
	}
	return fs
}

// stampSurface builds a contract struct value (the In/Out port surface) with each
// handle stamped with the owner, node id, and its variable name.
func stampSurface(owner any, nodeID string, sType reflect.Type, vars []decisionVar) reflect.Value {
	s := reflect.New(sType).Elem()
	for _, v := range vars {
		h := s.Field(v.field).Addr().Interface().(anyHandle)
		h.stamp(owner, nodeID, v.name)
	}
	return s
}

// inputFromMap writes named values into a fresh contract struct value of type
// sType, returning it (used to build the typed I a node's Evaluate receives).
func inputFromMap(sType reflect.Type, vars []decisionVar, vals map[string]BlValue) (any, error) {
	s := reflect.New(sType).Elem()
	for _, v := range vars {
		val, ok := vals[v.name]
		if !ok {
			continue
		}
		h := s.Field(v.field).Addr().Interface().(anyHandle)
		if err := h.setValue(val); err != nil {
			return nil, err
		}
	}
	return s.Interface(), nil
}

// outputToMap reads a contract struct value's handles into a named-value map.
func outputToMap(out any, vars []decisionVar) map[string]BlValue {
	ov := reflect.ValueOf(out)
	res := make(map[string]BlValue, len(vars))
	for _, v := range vars {
		h := ov.Field(v.field).Interface().(roHandle)
		res[v.name] = h.getValue()
	}
	return res
}
