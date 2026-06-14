package core

import (
	"io"
	"strings"
	"time"

	ics "github.com/arran4/golang-ical"
	"github.com/teambition/rrule-go"
)

// icalConfig holds ImportICal options.
type icalConfig struct {
	window   *BlRange // RRULE expansion window
	validity *BlRange // resulting calendar's validity bounds
	strict   bool     // error on non-VEVENT components
}

// ICalOption configures ImportICal.
type ICalOption func(*icalConfig)

// WithICalExpansionWindow bounds RRULE recurrence expansion to the given range.
func WithICalExpansionWindow(r BlRange) ICalOption {
	return func(c *icalConfig) { c.window = &r }
}

// WithICalValidity sets the imported calendar's validity bounds.
func WithICalValidity(r BlRange) ICalOption {
	return func(c *icalConfig) { c.validity = &r }
}

// WithICalStrict makes ImportICal error on any non-VEVENT component.
func WithICalStrict(strict bool) ICalOption {
	return func(c *icalConfig) { c.strict = strict }
}

// ImportICal parses an RFC 5545 iCalendar document into a BlCalendar. VEVENTs
// become point or range entries; SUMMARY becomes the entry name; RRULE/RDATE
// recurrences are expanded within the expansion window.
func ImportICal(r io.Reader, opts ...ICalOption) (BlCalendar, error) {
	var cfg icalConfig
	for _, o := range opts {
		o(&cfg)
	}
	cal, err := ics.ParseCalendar(r)
	if err != nil {
		return BlCalendar{}, &TypeError{Op: "ImportICal", Detail: err.Error()}
	}
	if cfg.strict {
		for _, comp := range cal.Components {
			if _, ok := comp.(*ics.VEvent); !ok {
				return BlCalendar{}, &TypeError{Op: "ImportICal", Detail: "non-VEVENT component in strict mode"}
			}
		}
	}
	var entries []BlCalendarEntry
	for _, ev := range cal.Events() {
		evEntries, err := icalEventEntries(ev, cfg.window)
		if err != nil {
			return BlCalendar{}, err
		}
		entries = append(entries, evEntries...)
	}
	c, err := Calendar(entries)
	if err != nil {
		return BlCalendar{}, err
	}
	if cfg.validity != nil {
		c.validFrom = cfg.validity.start
		c.validTo = cfg.validity.end
	}
	return c, nil
}

func icalProp(ev *ics.VEvent, p ics.ComponentProperty) (*ics.IANAProperty, bool) {
	prop := ev.GetProperty(p)
	if prop == nil {
		return nil, false
	}
	return prop, true
}

// icalEventEntries converts one VEVENT (possibly recurring) to calendar entries.
func icalEventEntries(ev *ics.VEvent, window *BlRange) ([]BlCalendarEntry, error) {
	startProp, ok := icalProp(ev, ics.ComponentPropertyDtStart)
	if !ok {
		return nil, nil
	}
	var name *string
	if sum, ok := icalProp(ev, ics.ComponentPropertySummary); ok && sum.Value != "" {
		n := sum.Value
		name = &n
	}
	startVal, isDate, startTime, err := parseICalValue(startProp)
	if err != nil {
		return nil, err
	}

	// Recurrence: expand within the window.
	if rprop, ok := icalProp(ev, ics.ComponentPropertyRrule); ok && window != nil {
		return expandRecurrence(rprop.Value, startTime, isDate, name, *window)
	}

	// Single event: point, or range when a distinct DTEND is present.
	if endProp, ok := icalProp(ev, ics.ComponentPropertyDtEnd); ok {
		endVal, endIsDate, _, err := parseICalValue(endProp)
		if err != nil {
			return nil, err
		}
		if !eqTrue(endVal.Equal(startVal)) {
			end := endVal
			if endIsDate {
				// RFC 5545 DATE DTEND is exclusive: make the range inclusive.
				if d, ok := endVal.(BlDate); ok {
					end = BlDate{t: d.t.AddDate(0, 0, -1), naive: d.naive}
				}
			}
			rng, err := Range(startVal, end, true, true)
			if err != nil {
				return nil, err
			}
			return []BlCalendarEntry{{name: name, value: rng}}, nil
		}
	}
	return []BlCalendarEntry{{name: name, value: startVal}}, nil
}

// parseICalValue parses an iCal DATE / DATE-TIME property value into a BlDate or
// BlDateTime, returning whether it was a DATE and the underlying time.Time.
func parseICalValue(prop *ics.IANAProperty) (BlValue, bool, time.Time, error) {
	v := prop.Value
	isDate := false
	if vals, ok := prop.ICalParameters["VALUE"]; ok {
		for _, x := range vals {
			if x == "DATE" {
				isDate = true
			}
		}
	}
	if len(v) == 8 { // YYYYMMDD
		isDate = true
	}
	if isDate {
		t, err := time.Parse("20060102", v)
		if err != nil {
			return nil, false, time.Time{}, &TypeError{Op: "ImportICal", Detail: "bad DATE " + v}
		}
		return BlDate{t: time.Date(t.Year(), t.Month(), t.Day(), 0, 0, 0, 0, time.UTC), naive: true}, true, t, nil
	}
	// DATE-TIME
	if strings.HasSuffix(v, "Z") {
		t, err := time.Parse("20060102T150405Z", v)
		if err != nil {
			return nil, false, time.Time{}, &TypeError{Op: "ImportICal", Detail: "bad DATE-TIME " + v}
		}
		return BlDateTime{t: t, naive: false}, false, t, nil
	}
	loc := time.UTC
	naive := true
	if tzids, ok := prop.ICalParameters["TZID"]; ok && len(tzids) > 0 {
		if l, err := time.LoadLocation(tzids[0]); err == nil {
			loc, naive = l, false
		}
	}
	t, err := time.ParseInLocation("20060102T150405", v, loc)
	if err != nil {
		return nil, false, time.Time{}, &TypeError{Op: "ImportICal", Detail: "bad DATE-TIME " + v}
	}
	return BlDateTime{t: t, naive: naive}, false, t, nil
}

// expandRecurrence expands an RRULE within the window into point entries.
func expandRecurrence(rruleStr string, start time.Time, isDate bool, name *string, window BlRange) ([]BlCalendarEntry, error) {
	opt, err := rrule.StrToROption(rruleStr)
	if err != nil {
		return nil, &TypeError{Op: "ImportICal", Detail: "bad RRULE: " + err.Error()}
	}
	opt.Dtstart = start
	rule, err := rrule.NewRRule(*opt)
	if err != nil {
		return nil, &TypeError{Op: "ImportICal", Detail: err.Error()}
	}
	winStart := temporalToTime(window.start, start)
	winEnd := temporalToTime(window.end, start.AddDate(10, 0, 0))
	occurrences := rule.Between(winStart, winEnd, true)
	entries := make([]BlCalendarEntry, 0, len(occurrences))
	for _, occ := range occurrences {
		var val BlValue
		if isDate {
			val = BlDate{t: time.Date(occ.Year(), occ.Month(), occ.Day(), 0, 0, 0, 0, time.UTC), naive: true}
		} else {
			val = BlDateTime{t: occ, naive: false}
		}
		entries = append(entries, BlCalendarEntry{name: name, value: val})
	}
	return entries, nil
}

func temporalToTime(v BlValue, fallback time.Time) time.Time {
	switch x := v.(type) {
	case BlDate:
		return x.t
	case BlDateTime:
		return x.t
	}
	return fallback
}
