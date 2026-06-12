package blkit

import (
	"fmt"
	"strings"

	"github.com/expr-lang/expr"
)

// BlBoolean wraps a native Go bool. An unknown boolean is modelled as BlNull,
// not a third enum value, and propagates through three-valued logic.
type BlBoolean struct{ b bool }

func (BlBoolean) Type() Type { return TypeBoolean }

func (b BlBoolean) Equal(other BlValue) BlValue {
	o, ok := other.(BlBoolean)
	if !ok {
		return BlBoolean{false}
	}
	return BlBoolean{b.b == o.b}
}

func (b BlBoolean) String() string {
	if b.b {
		return "true"
	}
	return "false"
}

func (BlBoolean) IsNull() bool { return false }

func (BlBoolean) isBlValue() {}

// Native hands the underlying Go bool back to host code.
func (b BlBoolean) Native() bool { return b.b }

// BooleanInput is the compile-time gate on host inputs to Boolean.
type BooleanInput interface {
	bool |
		int | int8 | int16 | int32 | int64 |
		uint | uint8 | uint16 | uint32 | uint64 |
		string
}

// Boolean constructs a BlBoolean from a Go bool (direct), any integer (0→false,
// non-zero→true), or a case-insensitive "true"/"false" string. The error fires
// only for an unrecognised string.
func Boolean[T BooleanInput](v T) (BlBoolean, error) {
	switch x := any(v).(type) {
	case bool:
		return BlBoolean{x}, nil
	case int:
		return BlBoolean{x != 0}, nil
	case int8:
		return BlBoolean{x != 0}, nil
	case int16:
		return BlBoolean{x != 0}, nil
	case int32:
		return BlBoolean{x != 0}, nil
	case int64:
		return BlBoolean{x != 0}, nil
	case uint:
		return BlBoolean{x != 0}, nil
	case uint8:
		return BlBoolean{x != 0}, nil
	case uint16:
		return BlBoolean{x != 0}, nil
	case uint32:
		return BlBoolean{x != 0}, nil
	case uint64:
		return BlBoolean{x != 0}, nil
	case string:
		switch strings.ToLower(x) {
		case "true":
			return BlBoolean{true}, nil
		case "false":
			return BlBoolean{false}, nil
		default:
			return BlBoolean{}, &TypeError{Op: "Boolean", Detail: fmt.Sprintf("%q is not a boolean literal", x)}
		}
	default:
		return BlBoolean{}, &TypeError{Op: "Boolean", Detail: "unsupported input"}
	}
}

// --- three-valued logic helpers -------------------------------------------

// blAndFn / blOrFn complete the three-valued table for the non-short-circuit
// cases; the patcher's conditional owns evaluation order. A non-boolean operand
// yields BlNull.
func blAndFn(a, b BlValue) BlValue {
	av, aok := boolOrNull(a)
	bv, bok := boolOrNull(b)
	if aok && !av {
		return BlBoolean{false}
	}
	if bok && !bv {
		return BlBoolean{false}
	}
	if aok && bok {
		return BlBoolean{av && bv}
	}
	return Null()
}

func blOrFn(a, b BlValue) BlValue {
	av, aok := boolOrNull(a)
	bv, bok := boolOrNull(b)
	if aok && av {
		return BlBoolean{true}
	}
	if bok && bv {
		return BlBoolean{true}
	}
	if aok && bok {
		return BlBoolean{av || bv}
	}
	return Null()
}

// boolOrNull reports a BlValue's truth value; ok is false for null or any
// non-boolean operand (which makes the connective propagate null).
func boolOrNull(v BlValue) (val bool, ok bool) {
	if bb, isBool := v.(BlBoolean); isBool {
		return bb.b, true
	}
	return false, false
}

// notFn is logical negation: not(null) → null; a non-boolean operand → null.
func notFn(x BlValue) BlValue {
	bb, ok := x.(BlBoolean)
	if !ok {
		return Null()
	}
	return BlBoolean{!bb.b}
}

func booleanOptions() []expr.Option {
	return []expr.Option{
		expr.Function("__not", typed1(notFn), new(func(BlValue) BlValue)),
	}
}
