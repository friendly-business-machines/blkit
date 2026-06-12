package blkit

import (
	"fmt"

	"github.com/shopspring/decimal"
)

// BlValue is the interface implemented by every blkit value type, so they can
// pass through the expr VM uniformly as `any`. The unexported isBlValue marker
// seals the interface: no type outside this package can satisfy it, which
// prevents host code from inventing fake values the engine would have to defend
// against.
type BlValue interface {
	Type() Type                  // the language type tag
	Equal(other BlValue) BlValue // three-valued: BlBoolean or BlNull
	String() string              // canonical literal-form rendering
	IsNull() bool                // true only for BlNull
	isBlValue()                  // sealing method
}

// BlNull is the absence-or-unknown value. The struct has no fields, so every
// BlNull is structurally identical to every other BlNull.
type BlNull struct{}

// Null returns a BlNull, matching the same-name-minus-Bl constructor convention.
func Null() BlNull { return BlNull{} }

func (BlNull) Type() Type { return TypeNull }

// Equal always returns BlBoolean(false), even against another BlNull — the
// SQL-style three-valued-logic rule (null is never equal to anything).
func (BlNull) Equal(BlValue) BlValue { return BlBoolean{false} }

func (BlNull) String() string { return "null" }

func (BlNull) IsNull() bool { return true }

func (BlNull) isBlValue() {}

// propagatesNull reports whether any argument is BlNull. Operator and library
// impls use it as the single source of truth for the "any operand null → null"
// rule. The short-circuit boolean impls intentionally do not call it.
func propagatesNull(args ...BlValue) bool {
	for _, a := range args {
		if a == nil || a.IsNull() {
			return true
		}
	}
	return false
}

// wrap converts a native Go input value to its Bl* representation. It is the
// input side of the engine bridge; unwrap is the inverse.
func wrap(v any) (BlValue, error) {
	switch x := v.(type) {
	case nil:
		return Null(), nil
	case BlValue:
		return x, nil
	case bool:
		return BlBoolean{x}, nil
	case string:
		return BlString{x}, nil
	case int:
		return BlNumber{decimal.NewFromInt(int64(x))}, nil
	case int8:
		return BlNumber{decimal.NewFromInt(int64(x))}, nil
	case int16:
		return BlNumber{decimal.NewFromInt(int64(x))}, nil
	case int32:
		return BlNumber{decimal.NewFromInt(int64(x))}, nil
	case int64:
		return BlNumber{decimal.NewFromInt(x)}, nil
	case uint:
		return BlNumber{decimal.NewFromUint64(uint64(x))}, nil
	case uint8:
		return BlNumber{decimal.NewFromUint64(uint64(x))}, nil
	case uint16:
		return BlNumber{decimal.NewFromUint64(uint64(x))}, nil
	case uint32:
		return BlNumber{decimal.NewFromUint64(uint64(x))}, nil
	case uint64:
		return BlNumber{decimal.NewFromUint64(x)}, nil
	case float32:
		return Number(float64(x))
	case float64:
		return Number(x)
	case decimal.Decimal:
		return BlNumber{x}, nil
	case []any:
		items := make([]BlValue, len(x))
		for i, e := range x {
			w, err := wrap(e)
			if err != nil {
				return nil, err
			}
			items[i] = w
		}
		return BlList{items}, nil
	case map[string]any:
		d := newDictionary()
		for k, e := range x {
			w, err := wrap(e)
			if err != nil {
				return nil, err
			}
			d.set(k, w)
		}
		return d, nil
	default:
		return nil, &TypeError{Op: "wrap", Detail: fmt.Sprintf("unsupported host input type %T", v)}
	}
}

// unwrap converts a Bl* value back to a native Go value for host code.
func unwrap(v BlValue) any {
	switch x := v.(type) {
	case BlNull:
		return nil
	case BlBoolean:
		return x.b
	case BlString:
		return x.s
	case BlNumber:
		return x.d
	case BlList:
		out := make([]any, len(x.items))
		for i, e := range x.items {
			out[i] = unwrap(e)
		}
		return out
	case BlDictionary:
		out := make(map[string]any, len(x.keys))
		for _, k := range x.keys {
			out[k] = unwrap(x.m[k])
		}
		return out
	default:
		return v
	}
}

// --- typed adapters -------------------------------------------------------
//
// expr requires every registered function to have the shape
// func(...any) (any, error). The typedN adapters wrap a statically-typed Go
// func into that shape, type-asserting each argument and boxing the result, so
// most impls are written with real Bl* types and the args ...any boilerplate is
// confined here.

func typed1[A BlValue, R BlValue](f func(A) R) func(...any) (any, error) {
	return func(args ...any) (any, error) {
		a, ok := args[0].(A)
		if !ok {
			return nil, argTypeError(args[0])
		}
		return f(a), nil
	}
}

func typed2[A, B BlValue, R BlValue](f func(A, B) R) func(...any) (any, error) {
	return func(args ...any) (any, error) {
		a, ok := args[0].(A)
		if !ok {
			return nil, argTypeError(args[0])
		}
		b, ok := args[1].(B)
		if !ok {
			return nil, argTypeError(args[1])
		}
		return f(a, b), nil
	}
}

func typed3[A, B, C BlValue, R BlValue](f func(A, B, C) R) func(...any) (any, error) {
	return func(args ...any) (any, error) {
		a, ok := args[0].(A)
		if !ok {
			return nil, argTypeError(args[0])
		}
		b, ok := args[1].(B)
		if !ok {
			return nil, argTypeError(args[1])
		}
		c, ok := args[2].(C)
		if !ok {
			return nil, argTypeError(args[2])
		}
		return f(a, b, c), nil
	}
}

func argTypeError(v any) error {
	return &TypeError{Op: "call", Detail: fmt.Sprintf("unexpected argument type %T", v)}
}

// asBl coerces an arbitrary VM value to a BlValue, wrapping native values the
// VM may have produced (e.g. from literals expr lexed before patching).
func asBl(v any) BlValue {
	if v == nil {
		return Null()
	}
	if bv, ok := v.(BlValue); ok {
		return bv
	}
	w, err := wrap(v)
	if err != nil {
		return Null()
	}
	return w
}
