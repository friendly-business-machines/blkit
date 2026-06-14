package core

import (
	"fmt"
	"time"

	"github.com/expr-lang/expr"
)

// BlDate is a calendar date with an optional UTC offset or IANA timezone.
type BlDate struct {
	t     time.Time
	naive bool
}

func (BlDate) Type() Type { return TypeDate }

func (d BlDate) Equal(other BlValue) BlValue {
	o, ok := other.(BlDate)
	if !ok {
		return BlBoolean{false}
	}
	if d.naive != o.naive {
		return Null()
	}
	if d.naive {
		return BlBoolean{d.t.Year() == o.t.Year() && d.t.YearDay() == o.t.YearDay()}
	}
	return BlBoolean{d.t.Equal(o.t)}
}

func (d BlDate) String() string {
	return d.t.Format("2006-01-02") + renderZoneSuffix(d.t, d.naive)
}

func (BlDate) IsNull() bool { return false }

func (BlDate) isBlValue() {}

// Time returns the wrapped time.Time (date portion meaningful).
func (d BlDate) Time() time.Time { return d.t }

// IsNaive reports whether the value is timezone-naive.
func (d BlDate) IsNaive() bool { return d.naive }

// DateComponents is the explicit component bundle for Date.
type DateComponents struct {
	Year, Month, Day int
	Offset           *time.Duration
	Zone             string
}

// DateInput is the compile-time gate on host inputs to Date.
type DateInput interface {
	string | time.Time | DateComponents
}

// Date constructs a BlDate from an ISO 8601 / RFC 9557 string, a time.Time, or
// a DateComponents bundle.
func Date[T DateInput](v T) (BlDate, error) {
	switch x := any(v).(type) {
	case string:
		return parseDate(x)
	case time.Time:
		return BlDate{t: midnight(x), naive: false}, nil
	case DateComponents:
		return dateFromComponents(x)
	default:
		return BlDate{}, &TypeError{Op: "Date", Detail: "unsupported input"}
	}
}

// ToDateComponentsAsNaive decomposes a time.Time into naive DateComponents.
func ToDateComponentsAsNaive(t time.Time) DateComponents {
	return DateComponents{Year: t.Year(), Month: int(t.Month()), Day: t.Day()}
}

// Today returns the current date as a naive BlDate in the local zone.
func Today() BlDate {
	n := time.Now().Local()
	return BlDate{t: time.Date(n.Year(), n.Month(), n.Day(), 0, 0, 0, 0, time.UTC), naive: true}
}

func midnight(t time.Time) time.Time {
	return time.Date(t.Year(), t.Month(), t.Day(), 0, 0, 0, 0, t.Location())
}

func parseDate(s string) (BlDate, error) {
	body, zone, hasZone := splitZoneSuffix(s)
	if hasZone {
		loc, err := time.LoadLocation(zone)
		if err != nil {
			return BlDate{}, &TypeError{Op: "date", Detail: "unknown zone " + zone}
		}
		t, err := time.ParseInLocation("2006-01-02", body, loc)
		if err != nil {
			return BlDate{}, &ParseError{Source: s, Err: err}
		}
		return BlDate{t: t, naive: false}, nil
	}
	if t, err := time.Parse("2006-01-02Z07:00", body); err == nil {
		return BlDate{t: midnight(t), naive: false}, nil
	}
	t, err := time.Parse("2006-01-02", body)
	if err != nil {
		return BlDate{}, &ParseError{Source: s, Err: err}
	}
	return BlDate{t: time.Date(t.Year(), t.Month(), t.Day(), 0, 0, 0, 0, time.UTC), naive: true}, nil
}

func dateFromComponents(c DateComponents) (BlDate, error) {
	if c.Offset != nil && c.Zone != "" {
		return BlDate{}, &TypeError{Op: "Date", Detail: "offset and zone are mutually exclusive"}
	}
	if c.Month < 1 || c.Month > 12 || c.Day < 1 || c.Day > 31 {
		return BlDate{}, &TypeError{Op: "Date", Detail: "invalid month/day"}
	}
	loc := time.UTC
	naive := true
	switch {
	case c.Zone != "":
		l, err := time.LoadLocation(c.Zone)
		if err != nil {
			return BlDate{}, &TypeError{Op: "Date", Detail: "unknown zone"}
		}
		loc, naive = l, false
	case c.Offset != nil:
		loc, naive = time.FixedZone("", int(c.Offset.Seconds())), false
	}
	t := time.Date(c.Year, time.Month(c.Month), c.Day, 0, 0, 0, 0, loc)
	// reject rollover (e.g. day 31 in a 30-day month)
	if int(t.Month()) != c.Month || t.Day() != c.Day {
		return BlDate{}, &TypeError{Op: "Date", Detail: "invalid day for month"}
	}
	return BlDate{t: t, naive: naive}, nil
}

// dateFn is the engine constructor: date("…") | date(y,m,d) | date(dt).
func dateFn(args ...any) (any, error) {
	switch len(args) {
	case 1:
		switch a := args[0].(type) {
		case BlString:
			return parseDate(a.s)
		case BlDateTime:
			return BlDate{t: midnight(a.t), naive: a.naive}, nil
		default:
			return nil, argTypeError(args[0])
		}
	case 3:
		y, ok1 := args[0].(BlNumber)
		m, ok2 := args[1].(BlNumber)
		d, ok3 := args[2].(BlNumber)
		if !ok1 || !ok2 || !ok3 {
			return nil, &TypeError{Op: "date", Detail: "expected numeric components"}
		}
		return dateFromComponents(DateComponents{
			Year: int(y.d.IntPart()), Month: int(m.d.IntPart()), Day: int(d.d.IntPart()),
		})
	default:
		return nil, &TypeError{Op: "date", Detail: fmt.Sprintf("wrong arity %d", len(args))}
	}
}

// --- operator impls -------------------------------------------------------

func addDateYM(d BlDate, dur BlYearsMonthsDuration) BlDate {
	return BlDate{t: addMonthsClamped(d.t, dur), naive: d.naive}
}
func addDateDT(d BlDate, dur BlDaysTimeDuration) BlDate {
	days := dur.secs.Div(decSecondsPerDay).Truncate(0).IntPart()
	return BlDate{t: d.t.AddDate(0, 0, int(days)), naive: d.naive}
}
func subDateYM(d BlDate, dur BlYearsMonthsDuration) BlDate { return addDateYM(d, negYMDuration(dur)) }
func subDateDT(d BlDate, dur BlDaysTimeDuration) BlDate    { return addDateDT(d, negDTDuration(dur)) }

func subDates(a, b BlDate) BlValue {
	if a.naive != b.naive {
		return Null()
	}
	return dtDur(decimalSecondsBetween(b.t, a.t))
}

func ltDates(a, b BlDate) BlValue { return cmpDates(a, b, func(c int) bool { return c < 0 }) }
func leDates(a, b BlDate) BlValue { return cmpDates(a, b, func(c int) bool { return c <= 0 }) }
func gtDates(a, b BlDate) BlValue { return cmpDates(a, b, func(c int) bool { return c > 0 }) }
func geDates(a, b BlDate) BlValue { return cmpDates(a, b, func(c int) bool { return c >= 0 }) }

func cmpDates(a, b BlDate, pick func(int) bool) BlValue {
	if a.naive != b.naive {
		return Null()
	}
	c := a.t.Compare(b.t)
	return BlBoolean{pick(c)}
}

// addMonthsClamped adds a years-months duration, clamping the day to the
// target month's last day (2025-01-31 + P1M → 2025-02-28).
func addMonthsClamped(t time.Time, dur BlYearsMonthsDuration) time.Time {
	totalMonths := int(dur.months.Truncate(0).IntPart())
	y, m, d := t.Date()
	base := int(m) - 1 + totalMonths
	ny := y + base/12
	nm := base%12 + 1
	if nm <= 0 {
		nm += 12
		ny--
	}
	last := lastDayOf(ny, time.Month(nm), t.Location())
	if d > last {
		d = last
	}
	return time.Date(ny, time.Month(nm), d, t.Hour(), t.Minute(), t.Second(), t.Nanosecond(), t.Location())
}

func lastDayOf(year int, month time.Month, loc *time.Location) int {
	return time.Date(year, month+1, 0, 0, 0, 0, 0, loc).Day()
}

// dateComponent resolves a dot-component on a BlDate.
func dateComponent(d BlDate, name string) (BlValue, bool) {
	switch name {
	case "offset":
		return offsetDuration(d.t, d.naive), true
	case "timezone":
		z := zoneName(d.t, d.naive)
		if z == "" {
			return Null(), true
		}
		return str(z), true
	}
	return calendarProp(d.t, name)
}

func dateOptions() []expr.Option {
	return []expr.Option{
		expr.Function("addDateYM", typed2(addDateYM), new(func(BlValue, BlValue) BlDate)),
		expr.Function("addDateDT", typed2(addDateDT), new(func(BlValue, BlValue) BlDate)),
		expr.Function("subDateYM", typed2(subDateYM), new(func(BlValue, BlValue) BlDate)),
		expr.Function("subDateDT", typed2(subDateDT), new(func(BlValue, BlValue) BlDate)),
		expr.Function("subDates", typed2(subDates), new(func(BlValue, BlValue) BlValue)),
		expr.Function("date", dateFn,
			new(func(BlValue) BlDate),
			new(func(BlValue, BlValue, BlValue) BlDate)),
		expr.Function("today", todayFn, new(func() BlDate)),
	}
}

func todayFn(args ...any) (any, error) { return Today(), nil }
