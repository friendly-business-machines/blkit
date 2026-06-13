package blkit

import (
	"github.com/expr-lang/expr"
	"github.com/shopspring/decimal"
)

// Cross-type function overloads. abs / isNegative / round* apply to numbers and
// both duration types. With uniform BlValue parameter hints they cannot be
// registered separately per type (identical signatures collide), so each is a
// single dispatcher switching on the first operand's runtime type.

func overloadOptions() []expr.Option {
	return []expr.Option{
		expr.Function("abs", absAny, new(func(BlValue) BlValue)),
		expr.Function("isNegative", isNegativeAny, new(func(BlValue) BlBoolean)),
		expr.Function("round", roundAny, new(func(BlValue, BlValue) BlValue)),
		expr.Function("roundUp", roundUpAny, new(func(BlValue, BlValue) BlValue)),
		expr.Function("roundDown", roundDownAny, new(func(BlValue, BlValue) BlValue)),
		expr.Function("roundHalfUp", roundHalfUpAny, new(func(BlValue, BlValue) BlValue)),
		expr.Function("roundHalfDown", roundHalfDownAny, new(func(BlValue, BlValue) BlValue)),
		expr.Function("roundHalfEven", roundHalfEvenAny, new(func(BlValue, BlValue) BlValue)),
		expr.Function("componentAccess", componentAccessFn, new(func(BlValue, BlValue) BlValue)),
	}
}

func absAny(args ...any) (any, error) {
	switch v := asBl(args[0]).(type) {
	case BlNumber:
		return absFn(v), nil
	case BlDaysTimeDuration:
		return absDTFn(v), nil
	case BlYearsMonthsDuration:
		return absYMFn(v), nil
	default:
		return nil, &TypeError{Op: "abs", Detail: "unsupported type"}
	}
}

func isNegativeAny(args ...any) (any, error) {
	switch v := asBl(args[0]).(type) {
	case BlNumber:
		return isNegativeFn(v), nil
	case BlDaysTimeDuration:
		return isNegativeDTFn(v), nil
	case BlYearsMonthsDuration:
		return isNegativeYMFn(v), nil
	default:
		return nil, &TypeError{Op: "isNegative", Detail: "unsupported type"}
	}
}

// roundDispatch routes a rounding call by operand type: numeric (n, scale) or
// duration (d, step).
func roundDispatch(args []any, numeric func(n, scale BlNumber) BlNumber,
	dt func(d, step BlDaysTimeDuration) (BlDaysTimeDuration, error),
	ym func(d, step BlYearsMonthsDuration) (BlYearsMonthsDuration, error)) (any, error) {
	switch a := asBl(args[0]).(type) {
	case BlNumber:
		s, ok := asBl(args[1]).(BlNumber)
		if !ok {
			return nil, argTypeError(args[1])
		}
		return numeric(a, s), nil
	case BlDaysTimeDuration:
		s, ok := asBl(args[1]).(BlDaysTimeDuration)
		if !ok {
			return nil, argTypeError(args[1])
		}
		return dt(a, s)
	case BlYearsMonthsDuration:
		s, ok := asBl(args[1]).(BlYearsMonthsDuration)
		if !ok {
			return nil, argTypeError(args[1])
		}
		return ym(a, s)
	default:
		return nil, &TypeError{Op: "round", Detail: "unsupported type"}
	}
}

func roundAny(args ...any) (any, error) {
	return roundDispatch(args, roundFn, roundDTFn, roundYMFn)
}
func roundUpAny(args ...any) (any, error) {
	return roundDispatch(args, roundUpFn, roundUpDTFn, roundUpYMFn)
}
func roundDownAny(args ...any) (any, error) {
	return roundDispatch(args, roundDownFn, roundDownDTFn, roundDownYMFn)
}
func roundHalfUpAny(args ...any) (any, error) {
	return roundDispatch(args, roundHalfUpFn, roundHalfUpDTFn, roundHalfUpYMFn)
}
func roundHalfDownAny(args ...any) (any, error) {
	return roundDispatch(args, roundHalfDownFn, roundHalfDownDTFn, roundHalfDownYMFn)
}
func roundHalfEvenAny(args ...any) (any, error) {
	return roundDispatch(args, roundHalfEvenFn, roundHalfEvenDTFn, roundHalfEvenYMFn)
}

// --- decimal rounding-mode helpers (round to integer) ---------------------

func roundHalfUpMode(q decimal.Decimal) decimal.Decimal   { return q.Round(0) }
func roundUpMode(q decimal.Decimal) decimal.Decimal       { return q.RoundUp(0) }
func roundDownMode(q decimal.Decimal) decimal.Decimal     { return q.RoundDown(0) }
func roundHalfEvenMode(q decimal.Decimal) decimal.Decimal { return q.RoundBank(0) }

func roundHalfDownMode(q decimal.Decimal) decimal.Decimal {
	truncated := q.Truncate(0)
	remainder := q.Sub(truncated).Abs()
	if remainder.Cmp(decimal.New(5, -1)) > 0 {
		return q.RoundUp(0)
	}
	return truncated
}
