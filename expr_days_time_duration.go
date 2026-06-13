package blkit

import (
	"fmt"
	"math"
	"strings"
	"time"

	"github.com/expr-lang/expr"
	"github.com/shopspring/decimal"
)

// BlDaysTimeDuration wraps a signed arbitrary-precision decimal count of total
// seconds.
type BlDaysTimeDuration struct{ secs decimal.Decimal }

func (BlDaysTimeDuration) Type() Type { return TypeDaysTimeDuration }

func (d BlDaysTimeDuration) Equal(other BlValue) BlValue {
	o, ok := other.(BlDaysTimeDuration)
	if !ok {
		return BlBoolean{false}
	}
	return BlBoolean{d.secs.Equal(o.secs)}
}

func (d BlDaysTimeDuration) String() string { return formatDTDuration(d.secs) }

func (BlDaysTimeDuration) IsNull() bool { return false }

func (BlDaysTimeDuration) isBlValue() {}

func dtDur(secs decimal.Decimal) BlDaysTimeDuration { return BlDaysTimeDuration{secs} }

// TotalSeconds returns the signed exact decimal total used for all arithmetic.
func (d BlDaysTimeDuration) TotalSeconds() decimal.Decimal { return d.secs }

// Days returns the integer days portion, truncated toward zero, sign carried.
func (d BlDaysTimeDuration) Days() int {
	return int(d.secs.Div(decSecondsPerDay).Truncate(0).IntPart())
}

// Hours returns the hours remainder (|h| < 24), same sign as the value.
func (d BlDaysTimeDuration) Hours() int {
	rem := d.secs.Abs().Mod(decSecondsPerDay)
	h := rem.Div(decSecondsPerHour).Truncate(0).IntPart()
	return int(h) * d.secs.Sign()
}

// Minutes returns the minutes remainder (|m| < 60), same sign as the value.
func (d BlDaysTimeDuration) Minutes() int {
	rem := d.secs.Abs().Mod(decSecondsPerHour)
	m := rem.Div(decSecondsPerMinute).Truncate(0).IntPart()
	return int(m) * d.secs.Sign()
}

// Seconds returns the seconds remainder (|s| < 60), possibly fractional, same sign.
func (d BlDaysTimeDuration) Seconds() decimal.Decimal {
	rem := d.secs.Abs().Mod(decSecondsPerMinute)
	if d.secs.Sign() < 0 {
		return rem.Neg()
	}
	return rem
}

// NativeDuration hands back a time.Duration, saturating at its ±290y bounds.
func (d BlDaysTimeDuration) NativeDuration() time.Duration {
	f, _ := d.secs.Float64()
	if f > math.MaxInt64/1e9 {
		return time.Duration(math.MaxInt64)
	}
	if f < math.MinInt64/1e9 {
		return time.Duration(math.MinInt64)
	}
	return time.Duration(f * float64(time.Second))
}

// DHMSNumberInput is the per-argument constraint for DHMS.
type DHMSNumberInput interface {
	int | int8 | int16 | int32 | int64 |
		uint | uint8 | uint16 | uint32 | uint64 |
		float32 | float64 |
		decimal.Decimal | string | BlNumber | BlString
}

type dhmsComponents struct {
	days, hours, minutes, seconds any
}

// DHMS builds an explicit (days, hours, minutes, seconds) component bundle for
// DTDuration. Each argument is independently typed.
func DHMS[D, H, M, S DHMSNumberInput](days D, hours H, minutes M, seconds S) dhmsComponents {
	return dhmsComponents{days: days, hours: hours, minutes: minutes, seconds: seconds}
}

// DTDurationInput is the compile-time gate on host inputs to DTDuration.
type DTDurationInput interface {
	string | BlString |
		int | int8 | int16 | int32 | int64 |
		uint | uint8 | uint16 | uint32 | uint64 |
		float32 | float64 |
		decimal.Decimal | BlNumber |
		time.Duration |
		dhmsComponents
}

// DTDuration constructs a BlDaysTimeDuration. See the spec for the full input matrix.
func DTDuration[T DTDurationInput](v T) (BlDaysTimeDuration, error) {
	switch x := any(v).(type) {
	case string:
		return dtDurationFromString(x)
	case BlString:
		return dtDurationFromString(x.s)
	case int:
		return dtDur(decimal.NewFromInt(int64(x))), nil
	case int8:
		return dtDur(decimal.NewFromInt(int64(x))), nil
	case int16:
		return dtDur(decimal.NewFromInt(int64(x))), nil
	case int32:
		return dtDur(decimal.NewFromInt(int64(x))), nil
	case int64:
		return dtDur(decimal.NewFromInt(x)), nil
	case uint:
		return dtDur(decimal.NewFromUint64(uint64(x))), nil
	case uint8:
		return dtDur(decimal.NewFromUint64(uint64(x))), nil
	case uint16:
		return dtDur(decimal.NewFromUint64(uint64(x))), nil
	case uint32:
		return dtDur(decimal.NewFromUint64(uint64(x))), nil
	case uint64:
		return dtDur(decimal.NewFromUint64(x)), nil
	case float32:
		return dtDurationFromFloat(float64(x))
	case float64:
		return dtDurationFromFloat(x)
	case decimal.Decimal:
		return dtDur(x), nil
	case BlNumber:
		return dtDur(x.d), nil
	case time.Duration:
		return dtDur(decimal.NewFromFloat(x.Seconds())), nil
	case dhmsComponents:
		return dtDurationFromComponents(x)
	default:
		return BlDaysTimeDuration{}, &TypeError{Op: "DTDuration", Detail: "unsupported input"}
	}
}

func dtDurationFromFloat(f float64) (BlDaysTimeDuration, error) {
	if math.IsNaN(f) || math.IsInf(f, 0) {
		return BlDaysTimeDuration{}, &TypeError{Op: "DTDuration", Detail: "NaN/Inf"}
	}
	return dtDur(decimal.NewFromFloat(f)), nil
}

// dtDurationFromString dispatches on the first non-sign character: P → ISO 8601
// (D/T only); otherwise a decimal number of total seconds.
func dtDurationFromString(s string) (BlDaysTimeDuration, error) {
	trimmed := strings.TrimSpace(s)
	if startsWithP(trimmed) {
		return dtDurationFn(BlString{trimmed})
	}
	d, err := decimal.NewFromString(trimmed)
	if err != nil {
		return BlDaysTimeDuration{}, &TypeError{Op: "DTDuration", Detail: fmt.Sprintf("cannot parse %q", s)}
	}
	return dtDur(d), nil
}

func startsWithP(s string) bool {
	for _, c := range s {
		if c == '+' || c == '-' {
			continue
		}
		return c == 'P' || c == 'p'
	}
	return false
}

func toDecimal(v any) (decimal.Decimal, error) {
	switch x := v.(type) {
	case int:
		return decimal.NewFromInt(int64(x)), nil
	case int8:
		return decimal.NewFromInt(int64(x)), nil
	case int16:
		return decimal.NewFromInt(int64(x)), nil
	case int32:
		return decimal.NewFromInt(int64(x)), nil
	case int64:
		return decimal.NewFromInt(x), nil
	case uint:
		return decimal.NewFromUint64(uint64(x)), nil
	case uint8:
		return decimal.NewFromUint64(uint64(x)), nil
	case uint16:
		return decimal.NewFromUint64(uint64(x)), nil
	case uint32:
		return decimal.NewFromUint64(uint64(x)), nil
	case uint64:
		return decimal.NewFromUint64(x), nil
	case float32:
		f := float64(x)
		if math.IsNaN(f) || math.IsInf(f, 0) {
			return decimal.Decimal{}, &TypeError{Op: "component", Detail: "NaN/Inf"}
		}
		return decimal.NewFromFloat(f), nil
	case float64:
		if math.IsNaN(x) || math.IsInf(x, 0) {
			return decimal.Decimal{}, &TypeError{Op: "component", Detail: "NaN/Inf"}
		}
		return decimal.NewFromFloat(x), nil
	case decimal.Decimal:
		return x, nil
	case string:
		return decimal.NewFromString(x)
	case BlNumber:
		return x.d, nil
	case BlString:
		return decimal.NewFromString(x.s)
	default:
		return decimal.Decimal{}, &TypeError{Op: "component", Detail: "unsupported"}
	}
}

func dtDurationFromComponents(c dhmsComponents) (BlDaysTimeDuration, error) {
	d, err := toDecimal(c.days)
	if err != nil {
		return BlDaysTimeDuration{}, err
	}
	h, err := toDecimal(c.hours)
	if err != nil {
		return BlDaysTimeDuration{}, err
	}
	m, err := toDecimal(c.minutes)
	if err != nil {
		return BlDaysTimeDuration{}, err
	}
	s, err := toDecimal(c.seconds)
	if err != nil {
		return BlDaysTimeDuration{}, err
	}
	total := d.Mul(decSecondsPerDay).
		Add(h.Mul(decSecondsPerHour)).
		Add(m.Mul(decSecondsPerMinute)).
		Add(s)
	return dtDur(total), nil
}

// dtDurationFn is the engine constructor: D/T-only ISO 8601 parser.
func dtDurationFn(s BlString) (BlDaysTimeDuration, error) {
	p, ok := parseISODuration(s.s)
	if !ok {
		return BlDaysTimeDuration{}, &ParseError{Source: s.s, Err: fmt.Errorf("invalid duration")}
	}
	if p.hasYears || p.hasMonthsDate {
		return BlDaysTimeDuration{}, &ParseError{Source: s.s, Err: fmt.Errorf("year/month designators not allowed in dtDuration")}
	}
	total := p.days.Mul(decSecondsPerDay).
		Add(p.hours.Mul(decSecondsPerHour)).
		Add(p.minutesTime.Mul(decSecondsPerMinute)).
		Add(p.seconds)
	if p.negative {
		total = total.Neg()
	}
	return dtDur(total), nil
}

// formatDTDuration renders the canonical ISO 8601 form from total seconds.
func formatDTDuration(secs decimal.Decimal) string {
	if secs.IsZero() {
		return "PT0S"
	}
	sign := ""
	a := secs
	if a.Sign() < 0 {
		sign = "-"
		a = a.Neg()
	}
	days := a.Div(decSecondsPerDay).Floor()
	rem := a.Sub(days.Mul(decSecondsPerDay))
	hours := rem.Div(decSecondsPerHour).Floor()
	rem = rem.Sub(hours.Mul(decSecondsPerHour))
	minutes := rem.Div(decSecondsPerMinute).Floor()
	seconds := rem.Sub(minutes.Mul(decSecondsPerMinute))

	var b strings.Builder
	b.WriteString(sign)
	b.WriteString("P")
	if !days.IsZero() {
		b.WriteString(days.String() + "D")
	}
	if !hours.IsZero() || !minutes.IsZero() || !seconds.IsZero() {
		b.WriteString("T")
		if !hours.IsZero() {
			b.WriteString(hours.String() + "H")
		}
		if !minutes.IsZero() {
			b.WriteString(minutes.String() + "M")
		}
		if !seconds.IsZero() {
			b.WriteString(seconds.String() + "S")
		}
	}
	return b.String()
}

// --- operator impls -------------------------------------------------------

func addDTDuration(a, b BlDaysTimeDuration) BlDaysTimeDuration { return dtDur(a.secs.Add(b.secs)) }
func subDTDuration(a, b BlDaysTimeDuration) BlDaysTimeDuration { return dtDur(a.secs.Sub(b.secs)) }
func negDTDuration(d BlDaysTimeDuration) BlDaysTimeDuration    { return dtDur(d.secs.Neg()) }
func scaleDTDuration(d BlDaysTimeDuration, n BlNumber) BlDaysTimeDuration {
	return dtDur(d.secs.Mul(n.d))
}
func divDTDuration(d BlDaysTimeDuration, n BlNumber) BlValue {
	if n.d.IsZero() {
		return Null()
	}
	return dtDur(d.secs.DivRound(n.d, numericPrecision+2).Truncate(numericPrecision))
}

// --- component accessors --------------------------------------------------

func durationDaysDTFn(d BlDaysTimeDuration) BlNumber    { return numFromInt(d.Days()) }
func durationHoursDTFn(d BlDaysTimeDuration) BlNumber   { return numFromInt(d.Hours()) }
func durationMinutesDTFn(d BlDaysTimeDuration) BlNumber { return numFromInt(d.Minutes()) }
func durationSecondsDTFn(d BlDaysTimeDuration) BlNumber { return num(d.Seconds()) }
func durationTotalSecondsFn(d BlDaysTimeDuration) BlNumber {
	return num(d.secs)
}
func durationTotalMinutesFn(d BlDaysTimeDuration) BlNumber {
	return num(d.secs.DivRound(decSecondsPerMinute, numericPrecision))
}
func durationTotalHoursFn(d BlDaysTimeDuration) BlNumber {
	return num(d.secs.DivRound(decSecondsPerHour, numericPrecision))
}
func durationTotalDaysFn(d BlDaysTimeDuration) BlNumber {
	return num(d.secs.DivRound(decSecondsPerDay, numericPrecision))
}

// --- library impls --------------------------------------------------------

func absDTFn(d BlDaysTimeDuration) BlDaysTimeDuration { return dtDur(d.secs.Abs()) }
func isNegativeDTFn(d BlDaysTimeDuration) BlBoolean   { return BlBoolean{d.secs.Sign() < 0} }

// rounding family — round totalSeconds(d)/totalSeconds(step) per mode, ×step.
func roundDTFn(d, step BlDaysTimeDuration) (BlDaysTimeDuration, error) {
	return roundDTApply(d, step, roundHalfUpMode)
}
func roundUpDTFn(d, step BlDaysTimeDuration) (BlDaysTimeDuration, error) {
	return roundDTApply(d, step, roundUpMode)
}
func roundDownDTFn(d, step BlDaysTimeDuration) (BlDaysTimeDuration, error) {
	return roundDTApply(d, step, roundDownMode)
}
func roundHalfUpDTFn(d, step BlDaysTimeDuration) (BlDaysTimeDuration, error) {
	return roundDTApply(d, step, roundHalfUpMode)
}
func roundHalfDownDTFn(d, step BlDaysTimeDuration) (BlDaysTimeDuration, error) {
	return roundDTApply(d, step, roundHalfDownMode)
}
func roundHalfEvenDTFn(d, step BlDaysTimeDuration) (BlDaysTimeDuration, error) {
	return roundDTApply(d, step, roundHalfEvenMode)
}

func roundDTApply(d, step BlDaysTimeDuration, mode func(decimal.Decimal) decimal.Decimal) (BlDaysTimeDuration, error) {
	if step.secs.Sign() <= 0 {
		return BlDaysTimeDuration{}, &TypeError{Op: "round", Detail: "step must be positive"}
	}
	q := d.secs.DivRound(step.secs, numericPrecision+4)
	return dtDur(mode(q).Mul(step.secs)), nil
}

func daysTimeDurationOptions() []expr.Option {
	return []expr.Option{
		expr.Function("addDTDuration", typed2(addDTDuration), new(func(BlValue, BlValue) BlDaysTimeDuration)),
		expr.Function("subDTDuration", typed2(subDTDuration), new(func(BlValue, BlValue) BlDaysTimeDuration)),
		expr.Function("negDTDuration", typed1(negDTDuration), new(func(BlValue) BlDaysTimeDuration)),
		expr.Function("scaleDTDuration", typed2(scaleDTDuration), new(func(BlValue, BlValue) BlDaysTimeDuration)),
		expr.Function("divDTDuration", typed2(divDTDuration), new(func(BlValue, BlValue) BlValue)),

		// component accessors (.days/.hours/.minutes/.seconds/.total*) are
		// internal arms of componentAccess (expr_components.go), not registered
		// as user-callable functions.

		expr.Function("dtDuration", typed1err(dtDurationFn), new(func(BlValue) BlDaysTimeDuration)),
		// abs / isNegative / round* are registered once as unified cross-type
		// dispatchers in expr_overloads.go (they overload number + both durations).
	}
}
