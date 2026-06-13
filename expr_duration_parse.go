package blkit

import (
	"regexp"
	"strings"

	"github.com/shopspring/decimal"
)

// isoDurationRe matches an ISO 8601 duration with a relaxed grammar: a leading
// sign, P, optional Y/M/D date designators, optional T with H/M/S time
// designators, each accepting a decimal fraction. Designators are matched
// case-insensitively (the input is upper-cased first).
var isoDurationRe = regexp.MustCompile(
	`^([+-]?)P(?:(\d+(?:\.\d+)?)Y)?(?:(\d+(?:\.\d+)?)M)?(?:(\d+(?:\.\d+)?)D)?(?:T(?:(\d+(?:\.\d+)?)H)?(?:(\d+(?:\.\d+)?)M)?(?:(\d+(?:\.\d+)?)S)?)?$`)

// isoDurationParts holds the parsed designator values (all non-negative; the
// sign is separate).
type isoDurationParts struct {
	negative                             bool
	years, monthsDate, days              decimal.Decimal
	hours, minutesTime, seconds          decimal.Decimal
	hasYears, hasMonthsDate, hasDays     bool
	hasHours, hasMinutesTime, hasSeconds bool
}

// parseISODuration parses an ISO 8601 duration string (case-insensitive
// designators). It does not enforce which designators are allowed — the
// dt/ym constructors apply that restriction.
func parseISODuration(s string) (isoDurationParts, bool) {
	up := strings.ToUpper(strings.TrimSpace(s))
	m := isoDurationRe.FindStringSubmatch(up)
	if m == nil || up == "P" || up == "PT" {
		return isoDurationParts{}, false
	}
	// Require at least one component.
	if m[2] == "" && m[3] == "" && m[4] == "" && m[5] == "" && m[6] == "" && m[7] == "" {
		return isoDurationParts{}, false
	}
	p := isoDurationParts{negative: m[1] == "-"}
	set := func(raw string, dst *decimal.Decimal, has *bool) bool {
		if raw == "" {
			return true
		}
		d, err := decimal.NewFromString(raw)
		if err != nil {
			return false
		}
		*dst = d
		*has = true
		return true
	}
	if !set(m[2], &p.years, &p.hasYears) ||
		!set(m[3], &p.monthsDate, &p.hasMonthsDate) ||
		!set(m[4], &p.days, &p.hasDays) ||
		!set(m[5], &p.hours, &p.hasHours) ||
		!set(m[6], &p.minutesTime, &p.hasMinutesTime) ||
		!set(m[7], &p.seconds, &p.hasSeconds) {
		return isoDurationParts{}, false
	}
	return p, true
}

var (
	decSecondsPerDay    = decimal.NewFromInt(86400)
	decSecondsPerHour   = decimal.NewFromInt(3600)
	decSecondsPerMinute = decimal.NewFromInt(60)
	decMonthsPerYear    = decimal.NewFromInt(12)
)
