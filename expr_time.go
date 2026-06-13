package blkit

import (
	"fmt"
	"time"

	"github.com/expr-lang/expr"
)

// BlTime is a time of day with an optional UTC offset or IANA timezone.
type BlTime struct {
	t     time.Time
	naive bool
}

const timeEpochDate = "0000-01-01"

func (BlTime) Type() Type { return TypeTime }

func (t BlTime) Equal(other BlValue) BlValue {
	o, ok := other.(BlTime)
	if !ok {
		return BlBoolean{false}
	}
	if t.naive != o.naive {
		return Null()
	}
	if t.naive {
		return BlBoolean{t.wallSeconds() == o.wallSeconds()}
	}
	return BlBoolean{t.t.Equal(o.t)}
}

func (t BlTime) wallSeconds() int {
	return t.t.Hour()*3600 + t.t.Minute()*60 + t.t.Second()
}

func (t BlTime) String() string {
	layout := "15:04:05"
	if t.t.Nanosecond() != 0 {
		layout = "15:04:05.999999999"
	}
	return t.t.Format(layout) + renderZoneSuffix(t.t, t.naive)
}

func (BlTime) IsNull() bool { return false }

func (BlTime) isBlValue() {}

// Native hands back the wrapped time.Time (time-of-day portion meaningful).
func (t BlTime) Native() time.Time { return t.t }

// IsNaive reports whether the value is timezone-naive.
func (t BlTime) IsNaive() bool { return t.naive }

// TimeComponents is the explicit component bundle for Time.
type TimeComponents struct {
	Hour, Minute, Second int
	Nanosecond           int
	Offset               *time.Duration
	Zone                 string
}

// TimeInput is the compile-time gate on host inputs to Time.
type TimeInput interface {
	string | time.Time | TimeComponents
}

// Time constructs a BlTime from an ISO 8601 / RFC 9557 string, a time.Time, or
// a TimeComponents bundle.
func Time[T TimeInput](v T) (BlTime, error) {
	switch x := any(v).(type) {
	case string:
		return parseTime(x)
	case time.Time:
		return BlTime{t: timeOnly(x), naive: false}, nil
	case TimeComponents:
		return timeFromComponents(x)
	default:
		return BlTime{}, &TypeError{Op: "Time", Detail: "unsupported input"}
	}
}

// ToTimeComponentsAsNaive decomposes a time.Time into naive TimeComponents.
func ToTimeComponentsAsNaive(t time.Time) TimeComponents {
	return TimeComponents{Hour: t.Hour(), Minute: t.Minute(), Second: t.Second(), Nanosecond: t.Nanosecond()}
}

func timeOnly(t time.Time) time.Time {
	return time.Date(0, 1, 1, t.Hour(), t.Minute(), t.Second(), t.Nanosecond(), t.Location())
}

func parseTime(s string) (BlTime, error) {
	body, zone, hasZone := splitZoneSuffix(s)
	if hasZone {
		loc, err := time.LoadLocation(zone)
		if err != nil {
			return BlTime{}, &TypeError{Op: "time", Detail: "unknown zone " + zone}
		}
		t, err := time.ParseInLocation("15:04:05.999999999", body, loc)
		if err != nil {
			return BlTime{}, &ParseError{Source: s, Err: err}
		}
		return BlTime{t: normaliseTimeDate(t), naive: false}, nil
	}
	if t, err := time.Parse("15:04:05.999999999Z07:00", body); err == nil {
		return BlTime{t: normaliseTimeDate(t), naive: false}, nil
	}
	t, err := time.Parse("15:04:05.999999999", body)
	if err != nil {
		return BlTime{}, &ParseError{Source: s, Err: err}
	}
	return BlTime{t: time.Date(0, 1, 1, t.Hour(), t.Minute(), t.Second(), t.Nanosecond(), time.UTC), naive: true}, nil
}

func normaliseTimeDate(t time.Time) time.Time {
	return time.Date(0, 1, 1, t.Hour(), t.Minute(), t.Second(), t.Nanosecond(), t.Location())
}

func timeFromComponents(c TimeComponents) (BlTime, error) {
	if c.Offset != nil && c.Zone != "" {
		return BlTime{}, &TypeError{Op: "Time", Detail: "offset and zone are mutually exclusive"}
	}
	if c.Hour < 0 || c.Hour > 23 || c.Minute < 0 || c.Minute > 59 || c.Second < 0 || c.Second > 59 {
		return BlTime{}, &TypeError{Op: "Time", Detail: "invalid hour/minute/second"}
	}
	loc := time.UTC
	naive := true
	switch {
	case c.Zone != "":
		l, err := time.LoadLocation(c.Zone)
		if err != nil {
			return BlTime{}, &TypeError{Op: "Time", Detail: "unknown zone"}
		}
		loc, naive = l, false
	case c.Offset != nil:
		loc, naive = time.FixedZone("", int(c.Offset.Seconds())), false
	}
	return BlTime{t: time.Date(0, 1, 1, c.Hour, c.Minute, c.Second, c.Nanosecond, loc), naive: naive}, nil
}

// timeFn is the engine constructor.
func timeFn(args ...any) (any, error) {
	switch len(args) {
	case 1:
		switch a := args[0].(type) {
		case BlString:
			return parseTime(a.s)
		case BlDateTime:
			return BlTime{t: timeOnly(a.t), naive: a.naive}, nil
		default:
			return nil, argTypeError(args[0])
		}
	case 3, 4:
		h, ok1 := args[0].(BlNumber)
		m, ok2 := args[1].(BlNumber)
		s, ok3 := args[2].(BlNumber)
		if !ok1 || !ok2 || !ok3 {
			return nil, &TypeError{Op: "time", Detail: "expected numeric components"}
		}
		tc := TimeComponents{Hour: int(h.d.IntPart()), Minute: int(m.d.IntPart()), Second: int(s.d.IntPart())}
		if len(args) == 4 {
			off, ok := args[3].(BlDaysTimeDuration)
			if !ok {
				return nil, argTypeError(args[3])
			}
			d := time.Duration(off.secs.IntPart()) * time.Second
			tc.Offset = &d
		}
		return timeFromComponents(tc)
	default:
		return nil, &TypeError{Op: "time", Detail: fmt.Sprintf("wrong arity %d", len(args))}
	}
}

// --- operator impls -------------------------------------------------------

func addTimeDur(t BlTime, dur BlDaysTimeDuration) BlTime {
	secs := dur.secs.IntPart() % 86400
	nt := t.t.Add(time.Duration(secs) * time.Second)
	return BlTime{t: normaliseTimeDate(nt), naive: t.naive}
}
func subTimeDur(t BlTime, dur BlDaysTimeDuration) BlTime { return addTimeDur(t, negDTDuration(dur)) }

func ltTimes(a, b BlTime) BlValue { return cmpTimes(a, b, func(c int) bool { return c < 0 }) }
func leTimes(a, b BlTime) BlValue { return cmpTimes(a, b, func(c int) bool { return c <= 0 }) }
func gtTimes(a, b BlTime) BlValue { return cmpTimes(a, b, func(c int) bool { return c > 0 }) }
func geTimes(a, b BlTime) BlValue { return cmpTimes(a, b, func(c int) bool { return c >= 0 }) }

func cmpTimes(a, b BlTime, pick func(int) bool) BlValue {
	if a.naive != b.naive {
		return Null()
	}
	return BlBoolean{pick(a.t.Compare(b.t))}
}

// timeComponent resolves a dot-component on a BlTime.
func timeComponent(t BlTime, name string) (BlValue, bool) {
	switch name {
	case "hour":
		return numFromInt(t.t.Hour()), true
	case "minute":
		return numFromInt(t.t.Minute()), true
	case "second":
		return numFromInt(t.t.Second()), true
	case "offset":
		return offsetDuration(t.t, t.naive), true
	case "timezone":
		z := zoneName(t.t, t.naive)
		if z == "" {
			return Null(), true
		}
		return str(z), true
	}
	return nil, false
}

func timeOptions() []expr.Option {
	return []expr.Option{
		expr.Function("addTimeDur", typed2(addTimeDur), new(func(BlValue, BlValue) BlTime)),
		expr.Function("subTimeDur", typed2(subTimeDur), new(func(BlValue, BlValue) BlTime)),
		expr.Function("time", timeFn,
			new(func(BlValue) BlTime),
			new(func(BlValue, BlValue, BlValue) BlTime),
			new(func(BlValue, BlValue, BlValue, BlValue) BlTime)),
	}
}
