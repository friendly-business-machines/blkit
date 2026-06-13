package blkit

import (
	"regexp"
	"strings"
	"time"

	"github.com/shopspring/decimal"
)

// Shared temporal helpers for BlDate / BlTime / BlDateTime.

var zoneSuffixRe = regexp.MustCompile(`\[([^\]]+)\]$`)

// splitZoneSuffix extracts a trailing RFC 9557 [Zone] suffix.
func splitZoneSuffix(s string) (body, zone string, hasZone bool) {
	m := zoneSuffixRe.FindStringSubmatch(s)
	if m == nil {
		return s, "", false
	}
	return s[:len(s)-len(m[0])], m[1], true
}

// renderZoneSuffix renders the zone portion of a non-naive temporal value for
// String(): an IANA "[Zone]" when the location is a named zone, otherwise the
// numeric offset (Z / +05:30) via the supplied formatted offset.
func renderZoneSuffix(t time.Time, naive bool) string {
	if naive {
		return ""
	}
	loc := t.Location().String()
	if strings.Contains(loc, "/") {
		return "[" + loc + "]"
	}
	return t.Format("Z07:00")
}

// zoneName returns the IANA zone name of a non-naive value, or "" if it carries
// only a numeric offset (or is naive).
func zoneName(t time.Time, naive bool) string {
	if naive {
		return ""
	}
	loc := t.Location().String()
	if strings.Contains(loc, "/") {
		return loc
	}
	return ""
}

// decimalSecondsBetween returns (to − from) in exact seconds, avoiding the
// int64 nanosecond overflow of time.Duration over large spans.
func decimalSecondsBetween(from, to time.Time) decimal.Decimal {
	secs := decimal.NewFromInt(to.Unix() - from.Unix())
	frac := decimal.New(int64(to.Nanosecond()-from.Nanosecond()), -9)
	return secs.Add(frac)
}

// offsetDuration returns the value's UTC offset as a days-time duration, or
// Null when naive.
func offsetDuration(t time.Time, naive bool) BlValue {
	if naive {
		return Null()
	}
	_, off := t.Zone()
	return dtDur(decimal.NewFromInt(int64(off)))
}

var weekdayNames = map[time.Weekday]string{
	time.Monday: "Monday", time.Tuesday: "Tuesday", time.Wednesday: "Wednesday",
	time.Thursday: "Thursday", time.Friday: "Friday", time.Saturday: "Saturday",
	time.Sunday: "Sunday",
}

var monthNames = map[time.Month]string{
	time.January: "January", time.February: "February", time.March: "March",
	time.April: "April", time.May: "May", time.June: "June", time.July: "July",
	time.August: "August", time.September: "September", time.October: "October",
	time.November: "November", time.December: "December",
}

// weekdayFromName maps an English full weekday name to time.Weekday.
func weekdayFromName(s string) (time.Weekday, bool) {
	for wd, name := range weekdayNames {
		if name == s {
			return wd, true
		}
	}
	return 0, false
}

// calendarProps computes the calendar-property accessors shared by date and
// datetime from the wrapped time.Time.
func calendarProp(t time.Time, name string) (BlValue, bool) {
	switch name {
	case "year":
		return numFromInt(t.Year()), true
	case "month":
		return numFromInt(int(t.Month())), true
	case "day":
		return numFromInt(t.Day()), true
	case "dayName":
		return str(weekdayNames[t.Weekday()]), true
	case "dayNameShort":
		return str(weekdayNames[t.Weekday()][:3]), true
	case "dayOfYear":
		return numFromInt(t.YearDay()), true
	case "weekOfYear":
		return numFromInt((t.YearDay()-1)/7 + 1), true
	case "isoWeekOfYear":
		_, wk := t.ISOWeek()
		return numFromInt(wk), true
	case "isoYearWeek":
		yr, wk := t.ISOWeek()
		return str(decimal.NewFromInt(int64(yr)).String() + "W" + decimal.NewFromInt(int64(wk)).String()), true
	case "monthName":
		return str(monthNames[t.Month()]), true
	case "monthNameShort":
		return str(monthNames[t.Month()][:3]), true
	case "quarter":
		return numFromInt((int(t.Month())-1)/3 + 1), true
	case "yearQuarter":
		q := (int(t.Month())-1)/3 + 1
		return str(decimal.NewFromInt(int64(t.Year())).String() + "Q" + decimal.NewFromInt(int64(q)).String()), true
	}
	return nil, false
}
