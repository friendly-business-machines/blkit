package core

import "github.com/expr-lang/expr"

// nameToType maps a language type name (as written in `instance of T`) to its
// Type tag.
var nameToType = map[string]Type{
	"null": TypeNull, "number": TypeNumber, "string": TypeString,
	"boolean": TypeBoolean, "date": TypeDate, "time": TypeTime,
	"datetime": TypeDateTime, "dtDuration": TypeDaysTimeDuration,
	"ymDuration": TypeYearsMonthsDuration, "list": TypeList,
	"dictionary": TypeDictionary, "range": TypeRange, "table": TypeTable,
	"calendar": TypeCalendar, "regex": TypeRegex, "any": TypeAny,
}

// instanceOfFn implements `x instance of T` (normalise lowers it to
// __instanceOf(x, "T")). For T == "any" everything non-null matches.
func instanceOfFn(args ...any) (any, error) {
	v := asBl(args[0])
	name, ok := asBl(args[1]).(BlString)
	if !ok {
		return nil, argTypeError(args[1])
	}
	want, known := nameToType[name.s]
	if !known {
		return nil, &TypeError{Op: "instance of", Detail: "unknown type " + name.s}
	}
	if want == TypeAny {
		return BlBoolean{!v.IsNull()}, nil
	}
	return BlBoolean{v.Type() == want}, nil
}

// isBoundFn backs `isDefined(name)` for a bare name. Variable names are now
// concrete env-struct fields known at compile time, so a declared name always
// resolves; normalise lowers `isDefined(name)` to __isBound(name) — passing the
// value forces the strict checker to reject an undeclared name — and this impl
// ignores the value and reports true.
func isBoundFn(args ...any) (any, error) {
	return BlBoolean{true}, nil
}

// hasKeyFn backs `isDefined(d.key)`: normalise lowers a dotted path to
// __hasKey(d, "key"), and this reports whether the dictionary d actually
// contains key — a missing key is "not defined", distinct from a present null.
func hasKeyFn(args ...any) (any, error) {
	d, ok := asBl(args[0]).(BlDictionary)
	if !ok {
		return BlBoolean{false}, nil
	}
	name, ok := asBl(args[1]).(BlString)
	if !ok {
		return nil, argTypeError(args[1])
	}
	_, present := d.m[name.s]
	return BlBoolean{present}, nil
}

func typeTestOptions() []expr.Option {
	return []expr.Option{
		expr.Function("__instanceOf", instanceOfFn, new(func(BlValue, BlValue) BlBoolean)),
		expr.Function("__isBound", isBoundFn, new(func(BlValue) BlBoolean)),
		expr.Function("__hasKey", hasKeyFn, new(func(BlValue, BlValue) BlBoolean)),
	}
}
