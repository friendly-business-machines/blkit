package blkit

import (
	"fmt"
	"math"
	"strings"

	"github.com/expr-lang/expr"
	"github.com/shopspring/decimal"
)

// BlYearsMonthsDuration wraps a signed arbitrary-precision decimal count of
// total months.
type BlYearsMonthsDuration struct{ months decimal.Decimal }

func (BlYearsMonthsDuration) Type() Type { return TypeYearsMonthsDuration }

func (d BlYearsMonthsDuration) Equal(other BlValue) BlValue {
	o, ok := other.(BlYearsMonthsDuration)
	if !ok {
		return BlBoolean{false}
	}
	return BlBoolean{d.months.Equal(o.months)}
}

func (d BlYearsMonthsDuration) String() string { return formatYMDuration(d.months) }

func (BlYearsMonthsDuration) IsNull() bool { return false }

func (BlYearsMonthsDuration) isBlValue() {}

func ymDur(months decimal.Decimal) BlYearsMonthsDuration { return BlYearsMonthsDuration{months} }

// TotalMonths returns the signed exact decimal total used for all arithmetic.
func (d BlYearsMonthsDuration) TotalMonths() decimal.Decimal { return d.months }

// Years returns the integer years portion, truncated toward zero, sign carried.
func (d BlYearsMonthsDuration) Years() int {
	return int(d.months.Div(decMonthsPerYear).Truncate(0).IntPart())
}

// Months returns the months remainder (|m| < 12), possibly fractional, same sign.
func (d BlYearsMonthsDuration) Months() decimal.Decimal {
	rem := d.months.Abs().Mod(decMonthsPerYear)
	if d.months.Sign() < 0 {
		return rem.Neg()
	}
	return rem
}

// YMNumberInput is the per-argument constraint for YM.
type YMNumberInput interface {
	int | int8 | int16 | int32 | int64 |
		uint | uint8 | uint16 | uint32 | uint64 |
		float32 | float64 |
		decimal.Decimal | string | BlNumber | BlString
}

type ymComponents struct {
	years, months any
}

// YM builds an explicit (years, months) component bundle for YMDuration.
func YM[Y, M YMNumberInput](years Y, months M) ymComponents {
	return ymComponents{years: years, months: months}
}

// YMDurationInput is the compile-time gate on host inputs to YMDuration.
type YMDurationInput interface {
	string | BlString |
		int | int8 | int16 | int32 | int64 |
		uint | uint8 | uint16 | uint32 | uint64 |
		float32 | float64 |
		decimal.Decimal | BlNumber |
		ymComponents
}

// YMDuration constructs a BlYearsMonthsDuration. See the spec for the input matrix.
func YMDuration[T YMDurationInput](v T) (BlYearsMonthsDuration, error) {
	switch x := any(v).(type) {
	case string:
		return ymDurationFromString(x)
	case BlString:
		return ymDurationFromString(x.s)
	case int:
		return ymDur(decimal.NewFromInt(int64(x))), nil
	case int8:
		return ymDur(decimal.NewFromInt(int64(x))), nil
	case int16:
		return ymDur(decimal.NewFromInt(int64(x))), nil
	case int32:
		return ymDur(decimal.NewFromInt(int64(x))), nil
	case int64:
		return ymDur(decimal.NewFromInt(x)), nil
	case uint:
		return ymDur(decimal.NewFromUint64(uint64(x))), nil
	case uint8:
		return ymDur(decimal.NewFromUint64(uint64(x))), nil
	case uint16:
		return ymDur(decimal.NewFromUint64(uint64(x))), nil
	case uint32:
		return ymDur(decimal.NewFromUint64(uint64(x))), nil
	case uint64:
		return ymDur(decimal.NewFromUint64(x)), nil
	case float32:
		return ymDurationFromFloat(float64(x))
	case float64:
		return ymDurationFromFloat(x)
	case decimal.Decimal:
		return ymDur(x), nil
	case BlNumber:
		return ymDur(x.d), nil
	case ymComponents:
		return ymDurationFromComponents(x)
	default:
		return BlYearsMonthsDuration{}, &TypeError{Op: "YMDuration", Detail: "unsupported input"}
	}
}

func ymDurationFromFloat(f float64) (BlYearsMonthsDuration, error) {
	if math.IsNaN(f) || math.IsInf(f, 0) {
		return BlYearsMonthsDuration{}, &TypeError{Op: "YMDuration", Detail: "NaN/Inf"}
	}
	return ymDur(decimal.NewFromFloat(f)), nil
}

func ymDurationFromString(s string) (BlYearsMonthsDuration, error) {
	trimmed := strings.TrimSpace(s)
	if startsWithP(trimmed) {
		return ymDurationFn(BlString{trimmed})
	}
	d, err := decimal.NewFromString(trimmed)
	if err != nil {
		return BlYearsMonthsDuration{}, &TypeError{Op: "YMDuration", Detail: fmt.Sprintf("cannot parse %q", s)}
	}
	return ymDur(d), nil
}

func ymDurationFromComponents(c ymComponents) (BlYearsMonthsDuration, error) {
	y, err := toDecimal(c.years)
	if err != nil {
		return BlYearsMonthsDuration{}, err
	}
	m, err := toDecimal(c.months)
	if err != nil {
		return BlYearsMonthsDuration{}, err
	}
	return ymDur(y.Mul(decMonthsPerYear).Add(m)), nil
}

// ymDurationFn is the engine constructor: Y/M-only ISO 8601 parser.
func ymDurationFn(s BlString) (BlYearsMonthsDuration, error) {
	p, ok := parseISODuration(s.s)
	if !ok {
		return BlYearsMonthsDuration{}, &ParseError{Source: s.s, Err: fmt.Errorf("invalid duration")}
	}
	if p.hasDays || p.hasHours || p.hasMinutesTime || p.hasSeconds {
		return BlYearsMonthsDuration{}, &ParseError{Source: s.s, Err: fmt.Errorf("day/time designators not allowed in ymDuration")}
	}
	total := p.years.Mul(decMonthsPerYear).Add(p.monthsDate)
	if p.negative {
		total = total.Neg()
	}
	return ymDur(total), nil
}

// formatYMDuration renders the canonical ISO 8601 form from total months.
func formatYMDuration(months decimal.Decimal) string {
	if months.IsZero() {
		return "P0Y0M"
	}
	sign := ""
	a := months
	if a.Sign() < 0 {
		sign = "-"
		a = a.Neg()
	}
	years := a.Div(decMonthsPerYear).Floor()
	rem := a.Sub(years.Mul(decMonthsPerYear))
	var b strings.Builder
	b.WriteString(sign)
	b.WriteString("P")
	if !years.IsZero() {
		b.WriteString(years.String() + "Y")
	}
	if !rem.IsZero() {
		b.WriteString(rem.String() + "M")
	}
	return b.String()
}

// --- operator impls -------------------------------------------------------

func addYMDuration(a, b BlYearsMonthsDuration) BlYearsMonthsDuration {
	return ymDur(a.months.Add(b.months))
}
func subYMDuration(a, b BlYearsMonthsDuration) BlYearsMonthsDuration {
	return ymDur(a.months.Sub(b.months))
}
func negYMDuration(d BlYearsMonthsDuration) BlYearsMonthsDuration { return ymDur(d.months.Neg()) }
func scaleYMDuration(d BlYearsMonthsDuration, n BlNumber) BlYearsMonthsDuration {
	return ymDur(d.months.Mul(n.d))
}
func divYMDuration(d BlYearsMonthsDuration, n BlNumber) BlValue {
	if n.d.IsZero() {
		return Null()
	}
	return ymDur(d.months.DivRound(n.d, numericPrecision+2).Truncate(numericPrecision))
}

// --- component accessors --------------------------------------------------

func durationYearsYMFn(d BlYearsMonthsDuration) BlNumber  { return numFromInt(d.Years()) }
func durationMonthsYMFn(d BlYearsMonthsDuration) BlNumber { return num(d.Months()) }
func durationTotalMonthsFn(d BlYearsMonthsDuration) BlNumber {
	return num(d.months)
}
func durationTotalYearsFn(d BlYearsMonthsDuration) BlNumber {
	return num(d.months.DivRound(decMonthsPerYear, numericPrecision))
}

// --- library impls --------------------------------------------------------

func absYMFn(d BlYearsMonthsDuration) BlYearsMonthsDuration { return ymDur(d.months.Abs()) }
func isNegativeYMFn(d BlYearsMonthsDuration) BlBoolean      { return BlBoolean{d.months.Sign() < 0} }

func roundYMApply(d, step BlYearsMonthsDuration, mode func(decimal.Decimal) decimal.Decimal) (BlYearsMonthsDuration, error) {
	if step.months.Sign() <= 0 {
		return BlYearsMonthsDuration{}, &TypeError{Op: "round", Detail: "step must be positive"}
	}
	q := d.months.DivRound(step.months, numericPrecision+4)
	return ymDur(mode(q).Mul(step.months)), nil
}

func roundYMFn(d, step BlYearsMonthsDuration) (BlYearsMonthsDuration, error) {
	return roundYMApply(d, step, roundHalfUpMode)
}
func roundUpYMFn(d, step BlYearsMonthsDuration) (BlYearsMonthsDuration, error) {
	return roundYMApply(d, step, roundUpMode)
}
func roundDownYMFn(d, step BlYearsMonthsDuration) (BlYearsMonthsDuration, error) {
	return roundYMApply(d, step, roundDownMode)
}
func roundHalfUpYMFn(d, step BlYearsMonthsDuration) (BlYearsMonthsDuration, error) {
	return roundYMApply(d, step, roundHalfUpMode)
}
func roundHalfDownYMFn(d, step BlYearsMonthsDuration) (BlYearsMonthsDuration, error) {
	return roundYMApply(d, step, roundHalfDownMode)
}
func roundHalfEvenYMFn(d, step BlYearsMonthsDuration) (BlYearsMonthsDuration, error) {
	return roundYMApply(d, step, roundHalfEvenMode)
}

func yearsMonthsDurationOptions() []expr.Option {
	return []expr.Option{
		expr.Function("addYMDuration", typed2(addYMDuration), new(func(BlValue, BlValue) BlYearsMonthsDuration)),
		expr.Function("subYMDuration", typed2(subYMDuration), new(func(BlValue, BlValue) BlYearsMonthsDuration)),
		expr.Function("negYMDuration", typed1(negYMDuration), new(func(BlValue) BlYearsMonthsDuration)),
		expr.Function("scaleYMDuration", typed2(scaleYMDuration), new(func(BlValue, BlValue) BlYearsMonthsDuration)),
		expr.Function("divYMDuration", typed2(divYMDuration), new(func(BlValue, BlValue) BlValue)),

		// component accessors (.years/.months/.totalMonths/.totalYears) are
		// internal arms of componentAccess (expr_components.go).

		expr.Function("ymDuration", typed1err(ymDurationFn), new(func(BlValue) BlYearsMonthsDuration)),
		// abs / isNegative / round* are unified cross-type dispatchers in
		// expr_overloads.go.
	}
}
