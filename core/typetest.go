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

// isDefinedFn reports whether a name is bound in the evaluation environment.
// normalise lowers `isDefined(x)` to __isDefined($env, "rootName"), so an
// unbound name never appears as a variable reference (avoiding a parse error)
// and path access reports on the root binding.
func isDefinedFn(args ...any) (any, error) {
	env, ok := args[0].(map[string]any)
	if !ok {
		return BlBoolean{false}, nil
	}
	name, ok := asBl(args[1]).(BlString)
	if !ok {
		return nil, argTypeError(args[1])
	}
	_, present := env[name.s]
	return BlBoolean{present}, nil
}

func typeTestOptions() []expr.Option {
	return []expr.Option{
		expr.Function("__instanceOf", instanceOfFn, new(func(BlValue, BlValue) BlBoolean)),
		expr.Function("__isDefined", isDefinedFn, new(func(map[string]any, BlValue) BlBoolean)),
	}
}
