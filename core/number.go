package core

import (
	"fmt"
	"math"
	"math/big"
	"regexp"
	"strings"

	"github.com/expr-lang/expr"
	"github.com/shopspring/decimal"
)

// numericPrecision is the significant-digit limit for blkit numbers
// (IEEE 754 decimal128). Non-terminating division is truncated here.
const numericPrecision = 34

func init() {
	// blkit owns exact decimal arithmetic; widen division precision so
	// non-terminating quotients carry full decimal128 significance.
	decimal.DivisionPrecision = numericPrecision
}

// BlNumber wraps an arbitrary-precision decimal.
type BlNumber struct{ d decimal.Decimal }

func (BlNumber) Type() Type { return TypeNumber }

func (n BlNumber) Equal(other BlValue) BlValue {
	o, ok := other.(BlNumber)
	if !ok {
		return BlBoolean{false}
	}
	return BlBoolean{n.d.Equal(o.d)}
}

func (n BlNumber) String() string {
	return n.d.String()
}

func (BlNumber) IsNull() bool { return false }

func (BlNumber) isBlValue() {}

// Decimal hands the underlying shopspring/decimal value back to host code.
func (n BlNumber) Decimal() decimal.Decimal { return n.d }

// NumberInput is the compile-time gate on host inputs to Number.
type NumberInput interface {
	int | int8 | int16 | int32 | int64 |
		uint | uint8 | uint16 | uint32 | uint64 |
		float32 | float64 |
		bool | string | decimal.Decimal
}

var numericStrip = regexp.MustCompile(`[^0-9eE+\-.]`)

// Number constructs a BlNumber from any Go numeric type, bool (true→1,
// false→0), a decimal string (accepting thousands separators, currency
// symbols, and surrounding whitespace), or a decimal.Decimal. The error fires
// only for a NaN/Inf float or an unparseable string.
func Number[T NumberInput](v T) (BlNumber, error) {
	switch x := any(v).(type) {
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
		return numberFromFloat(float64(x))
	case float64:
		return numberFromFloat(x)
	case bool:
		if x {
			return BlNumber{decimal.NewFromInt(1)}, nil
		}
		return BlNumber{decimal.NewFromInt(0)}, nil
	case decimal.Decimal:
		return BlNumber{x}, nil
	case string:
		return numberFromString(x)
	default:
		return BlNumber{}, &TypeError{Op: "Number", Detail: fmt.Sprintf("unsupported input %T", v)}
	}
}

func numberFromFloat(f float64) (BlNumber, error) {
	if math.IsNaN(f) || math.IsInf(f, 0) {
		return BlNumber{}, &TypeError{Op: "Number", Detail: "NaN/Inf not representable"}
	}
	return BlNumber{decimal.NewFromFloat(f)}, nil
}

func numberFromString(s string) (BlNumber, error) {
	cleaned := numericStrip.ReplaceAllString(strings.TrimSpace(s), "")
	if cleaned == "" {
		return BlNumber{}, &TypeError{Op: "Number", Detail: fmt.Sprintf("cannot parse %q", s)}
	}
	d, err := decimal.NewFromString(cleaned)
	if err != nil {
		return BlNumber{}, &TypeError{Op: "Number", Detail: fmt.Sprintf("cannot parse %q", s)}
	}
	return BlNumber{d}, nil
}

// num is an internal infallible constructor for engine-produced values.
func num(d decimal.Decimal) BlNumber { return BlNumber{d} }

// numFromInt is an internal constructor for integer engine results.
func numFromInt(i int) BlNumber { return BlNumber{decimal.NewFromInt(int64(i))} }

// --- operator impls -------------------------------------------------------

func addNumbers(a, b BlNumber) BlValue { return num(a.d.Add(b.d)) }
func subNumbers(a, b BlNumber) BlValue { return num(a.d.Sub(b.d)) }
func mulNumbers(a, b BlNumber) BlValue { return num(a.d.Mul(b.d)) }

func divNumbers(a, b BlNumber) BlValue {
	if b.d.IsZero() {
		return Null()
	}
	return num(a.d.DivRound(b.d, numericPrecision+2).Truncate(numericPrecision))
}

func powNumber(a, b BlNumber) BlValue {
	base, exp := a.d, b.d
	if base.Sign() < 0 && !exp.Equal(exp.Truncate(0)) {
		return Null() // complex result
	}
	if base.IsZero() && exp.Sign() < 0 {
		return Null() // division by zero
	}
	res, err := base.PowWithPrecision(exp, numericPrecision+6)
	if err != nil {
		return Null()
	}
	// Non-integer exponents go through an approximate ln/exp series, leaving a
	// sub-ulp error (e.g. 9**0.5 = 2.999…9). Round to a couple of guard digits
	// below full precision so exact roots snap back (the trailing-9 cascade
	// rounds up); String() then trims trailing zeros.
	if exp.Equal(exp.Truncate(0)) {
		return num(res.Truncate(numericPrecision))
	}
	return num(res.Round(numericPrecision - 2))
}

func negNumber(n BlNumber) BlNumber { return num(n.d.Neg()) }

// --- library impls --------------------------------------------------------

func roundFn(n, scale BlNumber) BlNumber         { return roundHalfUpFn(n, scale) }
func roundUpFn(n, scale BlNumber) BlNumber       { return num(n.d.RoundUp(int32(scale.d.IntPart()))) }
func roundDownFn(n, scale BlNumber) BlNumber     { return num(n.d.RoundDown(int32(scale.d.IntPart()))) }
func roundHalfUpFn(n, scale BlNumber) BlNumber   { return num(n.d.Round(int32(scale.d.IntPart()))) }
func roundHalfEvenFn(n, scale BlNumber) BlNumber { return num(n.d.RoundBank(int32(scale.d.IntPart()))) }

func roundHalfDownFn(n, scale BlNumber) BlNumber {
	s := int32(scale.d.IntPart())
	truncated := n.d.Truncate(s)
	remainder := n.d.Sub(truncated).Abs()
	half := decimal.New(5, -s-1) // 0.5 * 10^-s
	if remainder.Cmp(half) > 0 {
		return num(n.d.RoundUp(s))
	}
	return num(truncated)
}

func absFn(n BlNumber) BlNumber { return num(n.d.Abs()) }

func moduloFn(a, b BlNumber) BlValue {
	if b.d.IsZero() {
		return Null()
	}
	// floor modulo: sign follows the divisor.
	m := a.d.Mod(b.d)
	if !m.IsZero() && (m.Sign() != b.d.Sign()) {
		m = m.Add(b.d)
	}
	return num(m)
}

func sqrtFn(n BlNumber) BlValue {
	if n.d.Sign() < 0 {
		return Null()
	}
	if n.d.IsZero() {
		return num(decimal.Zero)
	}
	return num(decimalSqrt(n.d))
}

func expFn(n BlNumber) BlNumber {
	f, _ := n.d.Float64()
	return num(decimal.NewFromFloat(math.Exp(f)))
}

func lnFn(n BlNumber) BlValue {
	if n.d.Sign() <= 0 {
		return Null()
	}
	f, _ := n.d.Float64()
	return num(decimal.NewFromFloat(math.Log(f)))
}

func oddFn(n BlNumber) BlBoolean {
	if !n.d.Equal(n.d.Truncate(0)) {
		return BlBoolean{false}
	}
	return BlBoolean{n.d.Mod(decimal.NewFromInt(2)).Abs().Equal(decimal.NewFromInt(1))}
}

func evenFn(n BlNumber) BlBoolean {
	if !n.d.Equal(n.d.Truncate(0)) {
		return BlBoolean{false}
	}
	return BlBoolean{n.d.Mod(decimal.NewFromInt(2)).IsZero()}
}

func isPositiveFn(n BlNumber) BlBoolean { return BlBoolean{n.d.Sign() > 0} }
func isNegativeFn(n BlNumber) BlBoolean { return BlBoolean{n.d.Sign() < 0} }
func isZeroFn(n BlNumber) BlBoolean     { return BlBoolean{n.d.IsZero()} }

func clampFn(n, lo, hi BlNumber) BlValue {
	if lo.d.GreaterThan(hi.d) {
		return Null()
	}
	if n.d.LessThan(lo.d) {
		return num(lo.d)
	}
	if n.d.GreaterThan(hi.d) {
		return num(hi.d)
	}
	return num(n.d)
}

// variadic impls (optional args).

func floorFn(args ...any) (any, error) {
	n, scale, err := optScale(args)
	if err != nil {
		return nil, err
	}
	return num(n.d.RoundFloor(scale)), nil
}

func ceilingFn(args ...any) (any, error) {
	n, scale, err := optScale(args)
	if err != nil {
		return nil, err
	}
	return num(n.d.RoundCeil(scale)), nil
}

func optScale(args []any) (BlNumber, int32, error) {
	n, ok := args[0].(BlNumber)
	if !ok {
		return BlNumber{}, 0, argTypeError(args[0])
	}
	var scale int32
	if len(args) > 1 {
		s, ok := args[1].(BlNumber)
		if !ok {
			return BlNumber{}, 0, argTypeError(args[1])
		}
		scale = int32(s.d.IntPart())
	}
	return n, scale, nil
}

func logFn(args ...any) (any, error) {
	n, ok := args[0].(BlNumber)
	if !ok {
		return nil, argTypeError(args[0])
	}
	if n.d.Sign() <= 0 {
		return Null(), nil
	}
	base := 10.0
	if len(args) > 1 {
		b, ok := args[1].(BlNumber)
		if !ok {
			return nil, argTypeError(args[1])
		}
		bf, _ := b.d.Float64()
		if bf <= 0 || bf == 1 {
			return Null(), nil
		}
		base = bf
	}
	f, _ := n.d.Float64()
	return num(decimal.NewFromFloat(math.Log(f) / math.Log(base))), nil
}

func numberFn(args ...any) (any, error) {
	s, ok := args[0].(BlString)
	if !ok {
		return nil, argTypeError(args[0])
	}
	if len(args) == 1 {
		n, err := numberFromString(s.s)
		if err != nil {
			return Null(), nil
		}
		return n, nil
	}
	// 3-arg: number(from, groupingSep, decimalSep)
	groupSep, ok := args[1].(BlString)
	if !ok {
		return nil, argTypeError(args[1])
	}
	decSep, ok := args[2].(BlString)
	if !ok {
		return nil, argTypeError(args[2])
	}
	cleaned := strings.ReplaceAll(s.s, groupSep.s, "")
	if decSep.s != "." {
		cleaned = strings.ReplaceAll(cleaned, decSep.s, ".")
	}
	d, err := decimal.NewFromString(strings.TrimSpace(cleaned))
	if err != nil {
		return Null(), nil
	}
	return num(d), nil
}

// decimalSqrt computes a square root to numericPrecision significant digits via
// big.Float, then parses the shortest exact decimal back.
func decimalSqrt(d decimal.Decimal) decimal.Decimal {
	bf, _, err := big.ParseFloat(d.String(), 10, 256, big.ToNearestEven)
	if err != nil {
		f, _ := d.Float64()
		return decimal.NewFromFloat(math.Sqrt(f))
	}
	root := new(big.Float).SetPrec(256).Sqrt(bf)
	text := trimTrailingZeros(root.Text('f', numericPrecision))
	out, err := decimal.NewFromString(text)
	if err != nil {
		f, _ := d.Float64()
		return decimal.NewFromFloat(math.Sqrt(f))
	}
	return out
}

// trimTrailingZeros removes trailing fractional zeros (and a trailing dot) from
// a plain decimal string so perfect roots render as "4", not "4.000…".
func trimTrailingZeros(s string) string {
	if !strings.Contains(s, ".") {
		return s
	}
	s = strings.TrimRight(s, "0")
	return strings.TrimRight(s, ".")
}

// numberOptions registers the number operator impls and library functions.
// Parameter hints are BlValue because operator results flow through the VM as
// the BlValue interface; the impls assert their concrete operand types at
// runtime (returning a TypeError on a genuine mismatch).
func numberOptions() []expr.Option {
	return []expr.Option{
		expr.Function("floor", floorFn, new(func(BlValue) BlNumber), new(func(BlValue, BlValue) BlNumber)),
		expr.Function("ceiling", ceilingFn, new(func(BlValue) BlNumber), new(func(BlValue, BlValue) BlNumber)),
		expr.Function("modulo", typed2(moduloFn), new(func(BlValue, BlValue) BlValue)),
		expr.Function("sqrt", typed1(sqrtFn), new(func(BlValue) BlValue)),
		expr.Function("exp", typed1(expFn), new(func(BlValue) BlNumber)),
		expr.Function("ln", typed1(lnFn), new(func(BlValue) BlValue)),
		expr.Function("log", logFn, new(func(BlValue) BlValue), new(func(BlValue, BlValue) BlValue)),
		expr.Function("odd", typed1(oddFn), new(func(BlValue) BlBoolean)),
		expr.Function("even", typed1(evenFn), new(func(BlValue) BlBoolean)),
		expr.Function("isPositive", typed1(isPositiveFn), new(func(BlValue) BlBoolean)),
		expr.Function("isZero", typed1(isZeroFn), new(func(BlValue) BlBoolean)),
		// abs / isNegative / round* are unified cross-type dispatchers in
		// overloads.go (they overload number + both durations).
		expr.Function("clamp", typed3(clampFn), new(func(BlValue, BlValue, BlValue) BlValue)),
		expr.Function("number", numberFn,
			new(func(BlValue) BlNumber),
			new(func(BlValue, BlValue, BlValue) BlNumber)),
	}
}
