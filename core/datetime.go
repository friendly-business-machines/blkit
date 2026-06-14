package core

import (
	"fmt"
	"time"

	"github.com/expr-lang/expr"
	"github.com/shopspring/decimal"
)

// BlDateTime is a combined date and time with an optional UTC offset or IANA
// timezone.
type BlDateTime struct {
	t     time.Time
	naive bool
}

func (BlDateTime) Type() Type { return TypeDateTime }

func (dt BlDateTime) Equal(other BlValue) BlValue {
	o, ok := other.(BlDateTime)
	if !ok {
		return BlBoolean{false}
	}
	if dt.naive != o.naive {
		return Null()
	}
	if dt.naive {
		return BlBoolean{dt.wall() == o.wall()}
	}
	return BlBoolean{dt.t.Equal(o.t)}
}

func (dt BlDateTime) wall() string {
	return dt.t.Format("2006-01-02T15:04:05.999999999")
}

func (dt BlDateTime) String() string {
	layout := "2006-01-02T15:04:05"
	if dt.t.Nanosecond() != 0 {
		layout = "2006-01-02T15:04:05.999999999"
	}
	return dt.t.Format(layout) + renderZoneSuffix(dt.t, dt.naive)
}

func (BlDateTime) IsNull() bool { return false }

func (BlDateTime) isBlValue() {}

// Time returns the wrapped time.Time.
func (dt BlDateTime) Time() time.Time { return dt.t }

// IsNaive reports whether the value is timezone-naive.
func (dt BlDateTime) IsNaive() bool { return dt.naive }

// DateTimeComponents is the explicit component bundle for DateTime.
type DateTimeComponents struct {
	Year, Month, Day, Hour, Minute, Second int
	Offset                                 *time.Duration
	Zone                                   string
}

// DateTimeInput is the compile-time gate on host inputs to DateTime.
type DateTimeInput interface {
	string | time.Time | DateTimeComponents
}

// DateTime constructs a BlDateTime.
func DateTime[T DateTimeInput](v T) (BlDateTime, error) {
	switch x := any(v).(type) {
	case string:
		return parseDateTime(x)
	case time.Time:
		return BlDateTime{t: x, naive: false}, nil
	case DateTimeComponents:
		return dateTimeFromComponents(x)
	default:
		return BlDateTime{}, &TypeError{Op: "DateTime", Detail: "unsupported input"}
	}
}

// ToDateTimeComponentsAsNaive decomposes a time.Time into naive components.
func ToDateTimeComponentsAsNaive(t time.Time) DateTimeComponents {
	return DateTimeComponents{
		Year: t.Year(), Month: int(t.Month()), Day: t.Day(),
		Hour: t.Hour(), Minute: t.Minute(), Second: t.Second(),
	}
}

// Now returns the current moment as a non-naive BlDateTime.
func Now() BlDateTime { return BlDateTime{t: time.Now(), naive: false} }

func parseDateTime(s string) (BlDateTime, error) {
	body, zone, hasZone := splitZoneSuffix(s)
	if hasZone {
		loc, err := time.LoadLocation(zone)
		if err != nil {
			return BlDateTime{}, &TypeError{Op: "datetime", Detail: "unknown zone " + zone}
		}
		t, err := time.ParseInLocation("2006-01-02T15:04:05.999999999", body, loc)
		if err != nil {
			return BlDateTime{}, &ParseError{Source: s, Err: err}
		}
		return BlDateTime{t: t, naive: false}, nil
	}
	if t, err := time.Parse("2006-01-02T15:04:05.999999999Z07:00", body); err == nil {
		return BlDateTime{t: t, naive: false}, nil
	}
	t, err := time.Parse("2006-01-02T15:04:05.999999999", body)
	if err != nil {
		return BlDateTime{}, &ParseError{Source: s, Err: err}
	}
	return BlDateTime{t: t.UTC(), naive: true}, nil
}

func dateTimeFromComponents(c DateTimeComponents) (BlDateTime, error) {
	if c.Offset != nil && c.Zone != "" {
		return BlDateTime{}, &TypeError{Op: "DateTime", Detail: "offset and zone are mutually exclusive"}
	}
	if c.Month < 1 || c.Month > 12 || c.Day < 1 || c.Day > 31 || c.Hour < 0 || c.Hour > 23 {
		return BlDateTime{}, &TypeError{Op: "DateTime", Detail: "invalid components"}
	}
	loc := time.UTC
	naive := true
	switch {
	case c.Zone != "":
		l, err := time.LoadLocation(c.Zone)
		if err != nil {
			return BlDateTime{}, &TypeError{Op: "DateTime", Detail: "unknown zone"}
		}
		loc, naive = l, false
	case c.Offset != nil:
		loc, naive = time.FixedZone("", int(c.Offset.Seconds())), false
	}
	t := time.Date(c.Year, time.Month(c.Month), c.Day, c.Hour, c.Minute, c.Second, 0, loc)
	if int(t.Month()) != c.Month || t.Day() != c.Day {
		return BlDateTime{}, &TypeError{Op: "DateTime", Detail: "invalid day for month"}
	}
	return BlDateTime{t: t, naive: naive}, nil
}

func datetimeFn(args ...any) (any, error) {
	switch len(args) {
	case 1:
		s, ok := args[0].(BlString)
		if !ok {
			return nil, argTypeError(args[0])
		}
		return parseDateTime(s.s)
	case 2:
		d, ok1 := args[0].(BlDate)
		t, ok2 := args[1].(BlTime)
		if !ok1 || !ok2 {
			return nil, &TypeError{Op: "datetime", Detail: "expected (date, time)"}
		}
		naive := d.naive && t.naive
		loc := t.t.Location()
		if d.naive && t.naive {
			loc = time.UTC
		}
		return BlDateTime{t: time.Date(d.t.Year(), d.t.Month(), d.t.Day(), t.t.Hour(), t.t.Minute(), t.t.Second(), t.t.Nanosecond(), loc), naive: naive}, nil
	default:
		return nil, &TypeError{Op: "datetime", Detail: fmt.Sprintf("wrong arity %d", len(args))}
	}
}

func nowFn(args ...any) (any, error) { return Now(), nil }

// --- operator impls -------------------------------------------------------

func addDateTimeYM(dt BlDateTime, dur BlYearsMonthsDuration) BlDateTime {
	return BlDateTime{t: addMonthsClamped(dt.t, dur), naive: dt.naive}
}
func addDateTimeDT(dt BlDateTime, dur BlDaysTimeDuration) BlDateTime {
	nanos := dur.secs.Mul(decimal.New(1, 9)).Truncate(0).IntPart()
	return BlDateTime{t: dt.t.Add(time.Duration(nanos)), naive: dt.naive}
}
func subDateTimeYM(dt BlDateTime, dur BlYearsMonthsDuration) BlDateTime {
	return addDateTimeYM(dt, negYMDuration(dur))
}
func subDateTimeDT(dt BlDateTime, dur BlDaysTimeDuration) BlDateTime {
	return addDateTimeDT(dt, negDTDuration(dur))
}
func subDateTimes(a, b BlDateTime) BlValue {
	if a.naive != b.naive {
		return Null()
	}
	return dtDur(decimalSecondsBetween(b.t, a.t))
}

func ltDateTimes(a, b BlDateTime) BlValue {
	return cmpDateTimes(a, b, func(c int) bool { return c < 0 })
}
func leDateTimes(a, b BlDateTime) BlValue {
	return cmpDateTimes(a, b, func(c int) bool { return c <= 0 })
}
func gtDateTimes(a, b BlDateTime) BlValue {
	return cmpDateTimes(a, b, func(c int) bool { return c > 0 })
}
func geDateTimes(a, b BlDateTime) BlValue {
	return cmpDateTimes(a, b, func(c int) bool { return c >= 0 })
}

func cmpDateTimes(a, b BlDateTime, pick func(int) bool) BlValue {
	if a.naive != b.naive {
		return Null()
	}
	return BlBoolean{pick(a.t.Compare(b.t))}
}

func datetimeComponent(dt BlDateTime, name string) (BlValue, bool) {
	switch name {
	case "hour":
		return numFromInt(dt.t.Hour()), true
	case "minute":
		return numFromInt(dt.t.Minute()), true
	case "second":
		return numFromInt(dt.t.Second()), true
	case "offset":
		return offsetDuration(dt.t, dt.naive), true
	case "timezone":
		z := zoneName(dt.t, dt.naive)
		if z == "" {
			return Null(), true
		}
		return str(z), true
	}
	return calendarProp(dt.t, name)
}

// --- shared date/datetime helpers -----------------------------------------

// temporalParts extracts the wrapped time and kind from a BlDate or BlDateTime.
func temporalParts(v any) (t time.Time, naive bool, isDateTime bool, ok bool) {
	switch x := v.(type) {
	case BlDate:
		return x.t, x.naive, false, true
	case BlDateTime:
		return x.t, x.naive, true, true
	}
	return time.Time{}, false, false, false
}

// rebuildTemporal reconstructs a BlDate or BlDateTime from a time and kind.
func rebuildTemporal(t time.Time, naive, isDateTime bool) BlValue {
	if isDateTime {
		return BlDateTime{t: t, naive: naive}
	}
	return BlDate{t: midnight(t), naive: naive}
}

func dateUnary(impl func(time.Time) time.Time) func(...any) (any, error) {
	return func(args ...any) (any, error) {
		t, naive, dtk, ok := temporalParts(args[0])
		if !ok {
			return nil, argTypeError(args[0])
		}
		return rebuildTemporal(impl(t), naive, dtk), nil
	}
}

func datePredicate(impl func(time.Time) bool) func(...any) (any, error) {
	return func(args ...any) (any, error) {
		t, _, _, ok := temporalParts(args[0])
		if !ok {
			return nil, argTypeError(args[0])
		}
		return BlBoolean{impl(t)}, nil
	}
}

func isWeekendTime(t time.Time) bool {
	return t.Weekday() == time.Saturday || t.Weekday() == time.Sunday
}

func firstDayOfMonthTime(t time.Time) time.Time {
	return time.Date(t.Year(), t.Month(), 1, t.Hour(), t.Minute(), t.Second(), t.Nanosecond(), t.Location())
}
func lastDayOfMonthTime(t time.Time) time.Time {
	return time.Date(t.Year(), t.Month()+1, 0, t.Hour(), t.Minute(), t.Second(), t.Nanosecond(), t.Location())
}

func nextWeekdayTime(t time.Time) time.Time {
	for {
		t = t.AddDate(0, 0, 1)
		if !isWeekendTime(t) {
			return t
		}
	}
}
func prevWeekdayTime(t time.Time) time.Time {
	for {
		t = t.AddDate(0, 0, -1)
		if !isWeekendTime(t) {
			return t
		}
	}
}

func dowNav(forward bool) func(...any) (any, error) {
	return func(args ...any) (any, error) {
		t, naive, dtk, ok := temporalParts(args[0])
		if !ok {
			return nil, argTypeError(args[0])
		}
		dowStr, ok := args[1].(BlString)
		if !ok {
			return nil, argTypeError(args[1])
		}
		wd, ok := weekdayFromName(dowStr.s)
		if !ok {
			return nil, &TypeError{Op: "dayOfWeek", Detail: "unknown day name " + dowStr.s}
		}
		step := 1
		if !forward {
			step = -1
		}
		for {
			t = t.AddDate(0, 0, step)
			if t.Weekday() == wd {
				return rebuildTemporal(t, naive, dtk), nil
			}
		}
	}
}

func weekdaysBetweenFn(args ...any) (any, error) {
	t1, _, _, ok1 := temporalParts(args[0])
	t2, _, _, ok2 := temporalParts(args[1])
	if !ok1 || !ok2 {
		return nil, &TypeError{Op: "weekdaysBetween", Detail: "expected temporals"}
	}
	a, b := midnight(t1), midnight(t2)
	if a.After(b) {
		a, b = b, a
	}
	count := 0
	for !a.After(b) {
		if !isWeekendTime(a) {
			count++
		}
		a = a.AddDate(0, 0, 1)
	}
	return numFromInt(count), nil
}

func daysBetweenFn(args ...any) (any, error) {
	t1, _, dt1, ok1 := temporalParts(args[0])
	t2, _, _, ok2 := temporalParts(args[1])
	if !ok1 || !ok2 {
		return nil, &TypeError{Op: "daysBetween", Detail: "expected temporals"}
	}
	includeTime := false
	if len(args) == 3 && dt1 {
		if b, ok := args[2].(BlBoolean); ok {
			includeTime = b.b
		}
	}
	if !includeTime {
		t1, t2 = midnight(t1), midnight(t2)
	}
	secs := decimalSecondsBetween(t1, t2)
	days := secs.DivRound(decSecondsPerDay, numericPrecision)
	if !includeTime {
		days = days.Round(0)
	}
	return num(days), nil
}

func ymDurationBetweenAny(args ...any) (any, error) {
	t1, _, _, ok1 := temporalParts(args[0])
	t2, _, _, ok2 := temporalParts(args[1])
	if !ok1 || !ok2 {
		return nil, &TypeError{Op: "ymDurationBetween", Detail: "expected temporals"}
	}
	return ymDur(decimal.NewFromInt(int64(wholeMonthsBetween(t1, t2)))), nil
}

func dtDurationBetweenAny(args ...any) (any, error) {
	t1, _, _, ok1 := temporalParts(args[0])
	t2, _, _, ok2 := temporalParts(args[1])
	if !ok1 || !ok2 {
		return nil, &TypeError{Op: "dtDurationBetween", Detail: "expected temporals"}
	}
	return dtDur(decimalSecondsBetween(t1, t2)), nil
}

// weekdaysInMonth returns every day in t's month falling on weekday wd,
// preserving t's time-of-day and location.
func weekdaysInMonth(t time.Time, wd time.Weekday) []time.Time {
	first := time.Date(t.Year(), t.Month(), 1, t.Hour(), t.Minute(), t.Second(), t.Nanosecond(), t.Location())
	var out []time.Time
	for d := first; d.Month() == first.Month(); d = d.AddDate(0, 0, 1) {
		if d.Weekday() == wd {
			out = append(out, d)
		}
	}
	return out
}

func weekInMonthArgs(args []any) (time.Time, bool, bool, time.Weekday, bool) {
	t, naive, dtk, ok := temporalParts(asBl(args[0]))
	if !ok {
		return time.Time{}, false, false, 0, false
	}
	dow, ok := asBl(args[len(args)-1]).(BlString)
	if !ok {
		return time.Time{}, false, false, 0, false
	}
	wd, ok := weekdayFromName(dow.s)
	if !ok {
		return time.Time{}, false, false, 0, false
	}
	return t, naive, dtk, wd, true
}

func firstDOWInMonthFn(args ...any) (any, error) {
	t, naive, dtk, wd, ok := weekInMonthArgs(args)
	if !ok {
		return nil, &TypeError{Op: "firstDayOfWeekInMonth", Detail: "expected (date, dayName)"}
	}
	days := weekdaysInMonth(t, wd)
	return rebuildTemporal(days[0], naive, dtk), nil
}

func lastDOWInMonthFn(args ...any) (any, error) {
	t, naive, dtk, wd, ok := weekInMonthArgs(args)
	if !ok {
		return nil, &TypeError{Op: "lastDayOfWeekInMonth", Detail: "expected (date, dayName)"}
	}
	days := weekdaysInMonth(t, wd)
	return rebuildTemporal(days[len(days)-1], naive, dtk), nil
}

func nthDOWInMonthFn(args ...any) (any, error) {
	if len(args) != 3 {
		return nil, &TypeError{Op: "nthDayOfWeekInMonth", Detail: "expected (date, n, dayName)"}
	}
	t, naive, dtk, ok := temporalParts(asBl(args[0]))
	n, okn := asBl(args[1]).(BlNumber)
	dow, okd := asBl(args[2]).(BlString)
	if !ok || !okn || !okd {
		return nil, &TypeError{Op: "nthDayOfWeekInMonth", Detail: "expected (date, n, dayName)"}
	}
	wd, ok := weekdayFromName(dow.s)
	if !ok {
		return nil, &TypeError{Op: "nthDayOfWeekInMonth", Detail: "unknown day name " + dow.s}
	}
	nn := int(n.d.IntPart())
	if nn == 0 {
		return nil, &TypeError{Op: "nthDayOfWeekInMonth", Detail: "n must be non-zero"}
	}
	days := weekdaysInMonth(t, wd)
	var idx int
	if nn > 0 {
		idx = nn - 1
	} else {
		idx = len(days) + nn
	}
	if idx < 0 || idx >= len(days) {
		return Null(), nil
	}
	return rebuildTemporal(days[idx], naive, dtk), nil
}

func monthsBetweenFn(args ...any) (any, error) { return diffUnits(args, false) }
func yearsBetweenFn(args ...any) (any, error)  { return diffUnits(args, true) }

// diffUnits computes signed monthsBetween / yearsBetween under a basis.
func diffUnits(args []any, years bool) (any, error) {
	t1, _, _, ok1 := temporalParts(asBl(args[0]))
	t2, _, _, ok2 := temporalParts(asBl(args[1]))
	if !ok1 || !ok2 {
		return nil, &TypeError{Op: "between", Detail: "expected temporals"}
	}
	basis := "calendar"
	if len(args) > 2 {
		if b, ok := asBl(args[2]).(BlString); ok {
			basis = b.s
		}
	}
	t1, t2 = midnight(t1), midnight(t2)
	sign := 1.0
	if t1.After(t2) {
		t1, t2 = t2, t1
		sign = -1.0
	}
	val, err := unitsBetween(t1, t2, basis, years)
	if err != nil {
		return nil, err
	}
	return num(decimal.NewFromFloat(sign * val)), nil
}

func unitsBetween(t1, t2 time.Time, basis string, years bool) (float64, error) {
	actualDays := float64(int(decimalSecondsBetween(t1, t2).Div(decSecondsPerDay).IntPart()))
	switch basis {
	case "calendar":
		return calendarUnits(t1, t2, years), nil
	case "actual/365", "actual/actual":
		if years {
			return actualDays / 365.0, nil
		}
		return actualDays * 12.0 / 365.0, nil
	case "actual/360":
		if years {
			return actualDays / 360.0, nil
		}
		return actualDays / 30.0, nil
	case "30/360", "30E/360":
		d1, d2 := t1.Day(), t2.Day()
		if d1 > 30 {
			d1 = 30
		}
		if d2 > 30 {
			d2 = 30
		}
		days360 := float64((t2.Year()-t1.Year())*360 + (int(t2.Month())-int(t1.Month()))*30 + (d2 - d1))
		if years {
			return days360 / 360.0, nil
		}
		return days360 / 30.0, nil
	default:
		return 0, &TypeError{Op: "between", Detail: "unknown basis " + basis}
	}
}

// calendarUnits: whole units plus a fractional remainder anchored on the
// day-of-month (months) or day-of-year (years).
func calendarUnits(t1, t2 time.Time, years bool) float64 {
	step := 1
	full := wholeMonthsBetween(t1, t2)
	if years {
		full = full / 12
		step = 12
	}
	anchor := addMonthsClamped(t1, ymDur(decimal.NewFromInt(int64(full*step))))
	next := addMonthsClamped(t1, ymDur(decimal.NewFromInt(int64((full+1)*step))))
	residual := secsFloat(anchor, t2)
	window := secsFloat(anchor, next)
	if window == 0 {
		return float64(full)
	}
	return float64(full) + residual/window
}

func secsFloat(a, b time.Time) float64 {
	f, _ := decimalSecondsBetween(a, b).Float64()
	return f
}

// wholeMonthsBetween returns the signed count of full calendar months from a to b.
func wholeMonthsBetween(a, b time.Time) int {
	months := (b.Year()-a.Year())*12 + (int(b.Month()) - int(a.Month()))
	if months > 0 && b.Day() < a.Day() {
		months--
	} else if months < 0 && b.Day() > a.Day() {
		months++
	}
	return months
}

// zone stripping
func stripToNaive(impl func(time.Time) time.Time) func(...any) (any, error) {
	return func(args ...any) (any, error) {
		t, _, dtk, ok := temporalParts(args[0])
		if !ok {
			return nil, argTypeError(args[0])
		}
		w := impl(t)
		// preserve wall-clock; set naive
		naiveT := time.Date(w.Year(), w.Month(), w.Day(), w.Hour(), w.Minute(), w.Second(), w.Nanosecond(), time.UTC)
		return rebuildTemporal(naiveT, true, dtk), nil
	}
}

func withOffsetFn(args ...any) (any, error) {
	off, ok := args[1].(BlDaysTimeDuration)
	if !ok {
		return nil, argTypeError(args[1])
	}
	loc := time.FixedZone("", int(off.secs.IntPart()))
	switch v := args[0].(type) {
	case BlTime:
		if v.naive {
			return Null(), nil
		}
		return BlTime{t: v.t.In(loc), naive: false}, nil
	case BlDateTime:
		if v.naive {
			return Null(), nil
		}
		return BlDateTime{t: v.t.In(loc), naive: false}, nil
	default:
		return nil, argTypeError(args[0])
	}
}

func withTimezoneFn(dt BlDateTime, zone BlString) BlValue {
	if dt.naive {
		return Null()
	}
	loc, err := time.LoadLocation(zone.s)
	if err != nil {
		return Null()
	}
	return BlDateTime{t: dt.t.In(loc), naive: false}
}

// financial year
var fyStartMonths = map[string]int{"AU": 7, "UK": 4, "US": 10, "IN": 4, "JP": 4, "CA": 4, "NZ": 4}

func fyBasisMonth(basis any) (int, bool) {
	switch b := basis.(type) {
	case BlNumber:
		m := int(b.d.IntPart())
		if m < 1 || m > 12 {
			return 0, false
		}
		return m, true
	case BlString:
		m, ok := fyStartMonths[b.s]
		return m, ok
	}
	return 0, false
}

func financialYearFn(args ...any) (any, error) {
	t, _, _, ok := temporalParts(args[0])
	if !ok {
		return nil, argTypeError(args[0])
	}
	startMonth, ok := fyBasisMonth(args[1])
	if !ok {
		return nil, &TypeError{Op: "financialYear", Detail: "invalid basis"}
	}
	fy := financialYearOf(t, startMonth)
	return str(fmt.Sprintf("FY%d", fy)), nil
}

func financialYearQuarterFn(args ...any) (any, error) {
	t, _, _, ok := temporalParts(args[0])
	if !ok {
		return nil, argTypeError(args[0])
	}
	startMonth, ok := fyBasisMonth(args[1])
	if !ok {
		return nil, &TypeError{Op: "financialYearQuarter", Detail: "invalid basis"}
	}
	fy := financialYearOf(t, startMonth)
	monthsIn := (int(t.Month()) - startMonth + 12) % 12
	q := monthsIn/3 + 1
	return str(fmt.Sprintf("FY%dQ%d", fy, q)), nil
}

// financialYearOf returns the FY label year (the calendar year the FY ends in).
func financialYearOf(t time.Time, startMonth int) int {
	if int(t.Month()) >= startMonth {
		if startMonth == 1 {
			return t.Year()
		}
		return t.Year() + 1
	}
	return t.Year()
}

func datetimeOptions() []expr.Option {
	return []expr.Option{
		expr.Function("addDateTimeYM", typed2(addDateTimeYM), new(func(BlValue, BlValue) BlDateTime)),
		expr.Function("addDateTimeDT", typed2(addDateTimeDT), new(func(BlValue, BlValue) BlDateTime)),
		expr.Function("subDateTimeYM", typed2(subDateTimeYM), new(func(BlValue, BlValue) BlDateTime)),
		expr.Function("subDateTimeDT", typed2(subDateTimeDT), new(func(BlValue, BlValue) BlDateTime)),
		expr.Function("subDateTimes", typed2(subDateTimes), new(func(BlValue, BlValue) BlValue)),

		expr.Function("datetime", datetimeFn,
			new(func(BlValue) BlDateTime),
			new(func(BlValue, BlValue) BlDateTime)),
		expr.Function("now", nowFn, new(func() BlDateTime)),

		expr.Function("withOffset", withOffsetFn,
			new(func(BlValue, BlValue) BlValue)),
		expr.Function("withTimezone", typed2(withTimezoneFn), new(func(BlValue, BlValue) BlValue)),

		expr.Function("isWeekday", datePredicate(func(t time.Time) bool { return !isWeekendTime(t) }), new(func(BlValue) BlBoolean)),
		expr.Function("isWeekend", datePredicate(isWeekendTime), new(func(BlValue) BlBoolean)),
		expr.Function("firstDayOfMonth", dateUnary(firstDayOfMonthTime), new(func(BlValue) BlValue)),
		expr.Function("lastDayOfMonth", dateUnary(lastDayOfMonthTime), new(func(BlValue) BlValue)),
		expr.Function("lastDayOfPrevMonth", dateUnary(func(t time.Time) time.Time { return lastDayOfMonthTime(firstDayOfMonthTime(t).AddDate(0, 0, -1)) }), new(func(BlValue) BlValue)),
		expr.Function("firstDayOfNextMonth", dateUnary(func(t time.Time) time.Time { return lastDayOfMonthTime(t).AddDate(0, 0, 1) }), new(func(BlValue) BlValue)),
		expr.Function("nextWeekday", dateUnary(nextWeekdayTime), new(func(BlValue) BlValue)),
		expr.Function("prevWeekday", dateUnary(prevWeekdayTime), new(func(BlValue) BlValue)),
		expr.Function("nextDayOfWeek", dowNav(true), new(func(BlValue, BlValue) BlValue)),
		expr.Function("prevDayOfWeek", dowNav(false), new(func(BlValue, BlValue) BlValue)),
		expr.Function("firstDayOfWeekInMonth", firstDOWInMonthFn, new(func(BlValue, BlValue) BlValue)),
		expr.Function("lastDayOfWeekInMonth", lastDOWInMonthFn, new(func(BlValue, BlValue) BlValue)),
		expr.Function("nthDayOfWeekInMonth", nthDOWInMonthFn, new(func(BlValue, BlValue, BlValue) BlValue)),
		expr.Function("weekdaysBetween", weekdaysBetweenFn, new(func(BlValue, BlValue) BlNumber)),
		expr.Function("daysBetween", daysBetweenFn,
			new(func(BlValue, BlValue) BlNumber),
			new(func(BlValue, BlValue, BlValue) BlNumber)),
		expr.Function("monthsBetween", monthsBetweenFn,
			new(func(BlValue, BlValue) BlNumber),
			new(func(BlValue, BlValue, BlValue) BlNumber)),
		expr.Function("yearsBetween", yearsBetweenFn,
			new(func(BlValue, BlValue) BlNumber),
			new(func(BlValue, BlValue, BlValue) BlNumber)),
		expr.Function("ymDurationBetween", ymDurationBetweenAny, new(func(BlValue, BlValue) BlYearsMonthsDuration)),
		expr.Function("dtDurationBetween", dtDurationBetweenAny, new(func(BlValue, BlValue) BlDaysTimeDuration)),

		expr.Function("withoutOffset", stripToNaive(func(t time.Time) time.Time { return t }), new(func(BlValue) BlValue)),
		expr.Function("withoutTimezone", stripToNaive(func(t time.Time) time.Time { return t }), new(func(BlValue) BlValue)),
		expr.Function("withoutOffsetOrTimezone", stripToNaive(func(t time.Time) time.Time { return t }), new(func(BlValue) BlValue)),

		expr.Function("financialYear", financialYearFn,
			new(func(BlValue, BlValue) BlString)),
		expr.Function("financialYearQuarter", financialYearQuarterFn,
			new(func(BlValue, BlValue) BlString)),
	}
}
