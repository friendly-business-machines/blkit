package core

import (
	"sort"
	"strings"
	"time"

	"github.com/expr-lang/expr"
)

// BlCalendarEntry is an immutable (optional-name, temporal-value) pair.
type BlCalendarEntry struct {
	name  *string
	value BlValue // BlDate / BlDateTime / BlRange of either
}

func (BlCalendarEntry) Type() Type { return TypeAny }

func (e BlCalendarEntry) Equal(other BlValue) BlValue {
	o, ok := other.(BlCalendarEntry)
	if !ok {
		return BlBoolean{false}
	}
	nameEq := (e.name == nil) == (o.name == nil) && (e.name == nil || *e.name == *o.name)
	return BlBoolean{nameEq && eqTrue(e.value.Equal(o.value))}
}

func (e BlCalendarEntry) String() string {
	if e.name != nil {
		return e.value.String() + ": " + *e.name
	}
	return e.value.String()
}

func (BlCalendarEntry) IsNull() bool { return false }
func (BlCalendarEntry) isBlValue()   {}

// Name returns the entry's name and whether it is named.
func (e BlCalendarEntry) Name() (string, bool) {
	if e.name == nil {
		return "", false
	}
	return *e.name, true
}

func (e BlCalendarEntry) Value() BlValue { return e.value }

// CalendarEntry builds an entry; validation is deferred to Calendar.
func CalendarEntry(value BlValue, name ...string) BlCalendarEntry {
	if len(name) > 0 {
		n := name[0]
		return BlCalendarEntry{name: &n, value: value}
	}
	return BlCalendarEntry{value: value}
}

// BlCalendar is a chronologically-ordered collection of temporal entries with
// optional validity bounds.
type BlCalendar struct {
	entries            []BlCalendarEntry
	validFrom, validTo BlValue
}

func (BlCalendar) Type() Type { return TypeCalendar }

func (c BlCalendar) Equal(other BlValue) BlValue {
	o, ok := other.(BlCalendar)
	if !ok || len(c.entries) != len(o.entries) {
		return BlBoolean{false}
	}
	for i := range c.entries {
		if !eqTrue(c.entries[i].Equal(o.entries[i])) {
			return BlBoolean{false}
		}
	}
	return BlBoolean{true}
}

func (c BlCalendar) String() string {
	parts := make([]string, len(c.entries))
	for i, e := range c.entries {
		parts[i] = e.String()
	}
	return "calendar[" + strings.Join(parts, ", ") + "]"
}

func (BlCalendar) IsNull() bool { return false }
func (BlCalendar) isBlValue()   {}

// CalendarOption configures a calendar at construction.
type CalendarOption func(*BlCalendar)

// WithValidity sets the calendar's authoritative validity range.
func WithValidity(r BlRange) CalendarOption {
	return func(c *BlCalendar) {
		c.validFrom = r.start
		c.validTo = r.end
	}
}

// Calendar builds a calendar from entries (sorted into canonical chronological
// order) plus options. A non-temporal entry value → error.
func Calendar(entries []BlCalendarEntry, opts ...CalendarOption) (BlCalendar, error) {
	for _, e := range entries {
		if !isTemporalEntryValue(e.value) {
			return BlCalendar{}, &TypeError{Op: "Calendar", Detail: "entry must be a date, datetime, or range"}
		}
	}
	c := BlCalendar{validFrom: Null(), validTo: Null()}
	c.entries = append([]BlCalendarEntry{}, entries...)
	for _, o := range opts {
		o(&c)
	}
	sortEntries(c.entries)
	return c, nil
}

func isTemporalEntryValue(v BlValue) bool {
	switch x := v.(type) {
	case BlDate, BlDateTime:
		return true
	case BlRange:
		return isTemporalEntryValue(x.start) || isTemporalEntryValue(x.end) || (x.start.IsNull() && x.end.IsNull())
	}
	return false
}

// entryStart returns the comparable start point of an entry for ordering.
func entryStart(e BlCalendarEntry) BlValue {
	if r, ok := e.value.(BlRange); ok {
		return r.start
	}
	return e.value
}

func sortEntries(entries []BlCalendarEntry) {
	sort.SliceStable(entries, func(i, j int) bool {
		si, sj := entryStart(entries[i]), entryStart(entries[j])
		if c, ok := compareValues(si, sj); ok && c != 0 {
			return c < 0
		}
		ni, _ := entries[i].Name()
		nj, _ := entries[j].Name()
		return ni < nj
	})
}

// Entries returns a defensive copy of the entries.
func (c BlCalendar) Entries() []BlCalendarEntry { return append([]BlCalendarEntry{}, c.entries...) }
func (c BlCalendar) ValidFrom() BlValue         { return c.validFrom }
func (c BlCalendar) ValidTo() BlValue           { return c.validTo }

// calendarCovers reports whether an entry covers a temporal point.
func calendarCovers(e BlCalendarEntry, point BlValue) bool {
	switch v := e.value.(type) {
	case BlRange:
		in, ok := v.contains(point)
		return ok && in
	default:
		c, ok := compareValues(e.value, point)
		return ok && c == 0
	}
}

// --- query functions ------------------------------------------------------

func entriesFn(c BlCalendar) BlList {
	out := make([]BlValue, len(c.entries))
	for i, e := range c.entries {
		out[i] = e
	}
	return BlList{out}
}

func findFn(c BlCalendar, name BlString) BlList {
	var out []BlValue
	for _, e := range c.entries {
		if n, ok := e.Name(); ok && n == name.s {
			out = append(out, e)
		}
	}
	return BlList{out}
}

func calContainsFn(c BlCalendar, point BlValue) BlBoolean {
	for _, e := range c.entries {
		if calendarCovers(e, point) {
			return BlBoolean{true}
		}
	}
	return BlBoolean{false}
}

func entriesForFn(c BlCalendar, point BlValue) BlList {
	var out []BlValue
	for _, e := range c.entries {
		if calendarCovers(e, point) {
			out = append(out, e)
		}
	}
	return BlList{out}
}

func entryInterval(e BlCalendarEntry) (interval, bool) {
	return toInterval(e.value)
}

func calOverlapsFn(c BlCalendar, r BlRange) BlBoolean {
	ri, ok := toInterval(r)
	if !ok {
		return BlBoolean{false}
	}
	for _, e := range c.entries {
		ei, ok := entryInterval(e)
		if ok && intervalsOverlap(ei, ri) {
			return BlBoolean{true}
		}
	}
	return BlBoolean{false}
}

func entriesInFn(c BlCalendar, r BlRange) BlList {
	ri, ok := toInterval(r)
	var out []BlValue
	if !ok {
		return BlList{}
	}
	for _, e := range c.entries {
		ei, ok := entryInterval(e)
		if ok && intervalsOverlap(ei, ri) {
			out = append(out, e)
		}
	}
	return BlList{out}
}

func validFromFn(c BlCalendar) BlValue { return c.validFrom }
func validToFn(c BlCalendar) BlValue   { return c.validTo }
func validRangeFn(c BlCalendar) BlValue {
	if c.validFrom.IsNull() || c.validTo.IsNull() {
		return Null()
	}
	r, err := Range(c.validFrom, c.validTo, true, true)
	if err != nil {
		return Null()
	}
	return r
}

func calCountFn(c BlCalendar) BlNumber    { return numFromInt(len(c.entries)) }
func calIsEmptyFn(c BlCalendar) BlBoolean { return BlBoolean{len(c.entries) == 0} }

// next / prev walk entries strictly after / before a point.
func calNextFn(args ...any) (any, error) { return calStep(args, true) }
func calPrevFn(args ...any) (any, error) { return calStep(args, false) }

func calStep(args []any, forward bool) (any, error) {
	c, ok := asBl(args[0]).(BlCalendar)
	if !ok {
		return nil, argTypeError(args[0])
	}
	point := asBl(args[1])
	n := 1
	if len(args) > 2 {
		if num, ok := asBl(args[2]).(BlNumber); ok {
			n = int(num.d.IntPart())
		}
	}
	var matches []BlCalendarEntry
	for _, e := range c.entries {
		cmp, ok := compareValues(entryStart(e), point)
		if !ok {
			continue
		}
		if forward && cmp > 0 {
			matches = append(matches, e)
		}
		if !forward && cmp < 0 {
			matches = append(matches, e)
		}
	}
	if !forward {
		// nearest-before first
		for i, j := 0, len(matches)-1; i < j; i, j = i+1, j-1 {
			matches[i], matches[j] = matches[j], matches[i]
		}
	}
	if n < 1 || n > len(matches) {
		return Null(), nil
	}
	return matches[n-1], nil
}

// --- calendar mutation (name-based drop/keep + merge) ---------------------

func calendarDropFn(args ...any) (any, error) { return calDropKeep(args, true) }
func calendarKeepFn(args ...any) (any, error) { return calDropKeep(args, false) }

func calDropKeep(args []any, drop bool) (any, error) {
	c, ok := asBl(args[0]).(BlCalendar)
	if !ok {
		return nil, argTypeError(args[0])
	}
	target := asBl(args[1])
	var kept []BlCalendarEntry
	for _, e := range c.entries {
		if entryMatchesTarget(e, target) != drop {
			kept = append(kept, e)
		}
	}
	return BlCalendar{entries: kept, validFrom: c.validFrom, validTo: c.validTo}, nil
}

// entryMatchesTarget reports whether a calendar entry matches a drop/keep
// target: a name string, a regex on the name, a temporal point the entry
// covers, a range the entry overlaps, or any element of a list of targets.
func entryMatchesTarget(e BlCalendarEntry, target BlValue) bool {
	switch t := target.(type) {
	case BlString:
		n, named := e.Name()
		return named && n == t.s
	case BlRegex:
		n, named := e.Name()
		return named && t.compiled.MatchString(n)
	case BlDate, BlDateTime:
		return calendarCovers(e, target)
	case BlRange:
		ei, ok1 := toInterval(e.value)
		ti, ok2 := toInterval(t)
		return ok1 && ok2 && intervalsOverlap(ei, ti)
	case BlList:
		for _, el := range t.items {
			if entryMatchesTarget(e, el) {
				return true
			}
		}
		return false
	}
	return false
}

func calendarMergeFn(l BlList) (BlCalendar, error) {
	var entries []BlCalendarEntry
	for _, e := range l.items {
		c, ok := e.(BlCalendar)
		if !ok {
			return BlCalendar{}, &TypeError{Op: "calendarMerge", Detail: "elements must be calendars"}
		}
		entries = append(entries, c.entries...)
	}
	merged := BlCalendar{entries: entries, validFrom: Null(), validTo: Null()}
	sortEntries(merged.entries)
	return merged, nil
}

// --- business-day functions (consume an optional calendar) ----------------

func isWeekendDay(t time.Time) bool {
	return t.Weekday() == time.Saturday || t.Weekday() == time.Sunday
}

// isHoliday reports whether the point is covered by the calendar.
func isHoliday(cal BlCalendar, point BlValue) bool {
	return calContainsFn(cal, point).b
}

func isPublicHolidayFn(args ...any) (any, error) {
	point := asBl(args[0])
	cal, ok := asBl(args[1]).(BlCalendar)
	if !ok {
		return nil, argTypeError(args[1])
	}
	return BlBoolean{isHoliday(cal, point)}, nil
}

func isBusinessDayFn(args ...any) (any, error) {
	t, _, _, ok := temporalParts(asBl(args[0]))
	if !ok {
		return nil, argTypeError(args[0])
	}
	if isWeekendDay(t) {
		return BlBoolean{false}, nil
	}
	if len(args) > 1 {
		if cal, ok := asBl(args[1]).(BlCalendar); ok && isHoliday(cal, asBl(args[0])) {
			return BlBoolean{false}, nil
		}
	}
	return BlBoolean{true}, nil
}

// outOfCalendarRange reports whether a point lies outside the calendar's
// [validFrom, validTo] window (when those bounds are set).
func outOfCalendarRange(cal *BlCalendar, point BlValue) bool {
	if cal == nil {
		return false
	}
	if !cal.validFrom.IsNull() {
		if c, ok := compareValues(point, cal.validFrom); ok && c < 0 {
			return true
		}
	}
	if !cal.validTo.IsNull() {
		if c, ok := compareValues(point, cal.validTo); ok && c > 0 {
			return true
		}
	}
	return false
}

func businessDayStep(v BlValue, cal *BlCalendar, forward, strict bool) (BlValue, error) {
	t, naive, dtk, ok := temporalParts(v)
	if !ok {
		return Null(), nil
	}
	step := 1
	if !forward {
		step = -1
	}
	for {
		t = t.AddDate(0, 0, step)
		cur := rebuildTemporal(t, naive, dtk)
		if strict && outOfCalendarRange(cal, cur) {
			return nil, &CalendarRangeError{Detail: "business-day iteration past calendar validity bounds"}
		}
		if isWeekendDay(t) {
			continue
		}
		if cal != nil && isHoliday(*cal, cur) {
			continue
		}
		return cur, nil
	}
}

// parseCalStrict reads an optional trailing calendar and strictCalendarRange
// flag from args[from:].
func parseCalStrict(args []any, from int) (*BlCalendar, bool) {
	var cal *BlCalendar
	strict := false
	if len(args) > from {
		if c, ok := asBl(args[from]).(BlCalendar); ok {
			cal = &c
		}
	}
	if len(args) > from+1 {
		if b, ok := asBl(args[from+1]).(BlBoolean); ok {
			strict = b.b
		}
	}
	return cal, strict
}

func nextBusinessDayFn(args ...any) (any, error) { return businessDayNav(args, true) }
func prevBusinessDayFn(args ...any) (any, error) { return businessDayNav(args, false) }

func businessDayNav(args []any, forward bool) (any, error) {
	v := asBl(args[0])
	cal, strict := parseCalStrict(args, 1)
	return businessDayStep(v, cal, forward, strict)
}

func addBusinessDaysFn(args ...any) (any, error)      { return businessDayAdd(args, false) }
func subtractBusinessDaysFn(args ...any) (any, error) { return businessDayAdd(args, true) }

func businessDayAdd(args []any, subtract bool) (any, error) {
	v := asBl(args[0])
	n, ok := asBl(args[1]).(BlNumber)
	if !ok {
		return nil, argTypeError(args[1])
	}
	count := int(n.d.IntPart())
	forward := !subtract
	if count < 0 {
		forward = !forward
		count = -count
	}
	cal, strict := parseCalStrict(args, 2)
	cur := v
	for i := 0; i < count; i++ {
		next, err := businessDayStep(cur, cal, forward, strict)
		if err != nil {
			return nil, err
		}
		if next.IsNull() {
			return Null(), nil
		}
		cur = next
	}
	return cur, nil
}

func businessDaysBetweenFn(args ...any) (any, error) {
	t1, _, _, ok1 := temporalParts(asBl(args[0]))
	t2, _, _, ok2 := temporalParts(asBl(args[1]))
	if !ok1 || !ok2 {
		return nil, &TypeError{Op: "businessDaysBetween", Detail: "expected temporals"}
	}
	cal, strict := parseCalStrict(args, 2)
	if strict && cal != nil {
		if outOfCalendarRange(cal, asBl(args[0])) || outOfCalendarRange(cal, asBl(args[1])) {
			return nil, &CalendarRangeError{Detail: "businessDaysBetween past calendar validity bounds"}
		}
	}
	a, b := midnight(t1), midnight(t2)
	if a.After(b) {
		a, b = b, a
	}
	count := 0
	for !a.After(b) {
		cur := BlDate{t: a, naive: true}
		if !isWeekendDay(a) && (cal == nil || !isHoliday(*cal, cur)) {
			count++
		}
		a = a.AddDate(0, 0, 1)
	}
	return numFromInt(count), nil
}

func calendarOptions() []expr.Option {
	return []expr.Option{
		expr.Function("entries", typed1(entriesFn), new(func(BlValue) BlList)),
		expr.Function("find", typed2(findFn), new(func(BlValue, BlValue) BlList)),
		// `contains` / `overlaps` are unified cross-type dispatchers (string vs
		// calendar; interval vs calendar) — see __contains and overlaps.
		expr.Function("entriesFor", typed2(entriesForFn), new(func(BlValue, BlValue) BlList)),
		expr.Function("entriesIn", typed2(entriesInFn), new(func(BlValue, BlValue) BlList)),
		expr.Function("validFrom", typed1(validFromFn), new(func(BlValue) BlValue)),
		expr.Function("validTo", typed1(validToFn), new(func(BlValue) BlValue)),
		expr.Function("validRange", typed1(validRangeFn), new(func(BlValue) BlValue)),
		expr.Function("next", calNextFn, new(func(BlValue, BlValue) BlValue), new(func(BlValue, BlValue, BlValue) BlValue)),
		expr.Function("prev", calPrevFn, new(func(BlValue, BlValue) BlValue), new(func(BlValue, BlValue, BlValue) BlValue)),
		expr.Function("calendarDrop", calendarDropFn, new(func(BlValue, BlValue) BlCalendar)),
		expr.Function("calendarKeep", calendarKeepFn, new(func(BlValue, BlValue) BlCalendar)),
		expr.Function("calendarMerge", typed1err(calendarMergeFn), new(func(BlValue) BlCalendar)),

		// business-day functions (consume an optional calendar)
		// business-day functions take an optional trailing calendar and
		// strictCalendarRange flag; registered variadically (a single signature
		// avoids expr's multi-arity-overload panic when nested in another call).
		expr.Function("isPublicHoliday", isPublicHolidayFn, new(func(...BlValue) BlBoolean)),
		expr.Function("isBusinessDay", isBusinessDayFn, new(func(...BlValue) BlBoolean)),
		expr.Function("nextBusinessDay", nextBusinessDayFn, new(func(...BlValue) BlValue)),
		expr.Function("prevBusinessDay", prevBusinessDayFn, new(func(...BlValue) BlValue)),
		expr.Function("addBusinessDays", addBusinessDaysFn, new(func(...BlValue) BlValue)),
		expr.Function("subtractBusinessDays", subtractBusinessDaysFn, new(func(...BlValue) BlValue)),
		expr.Function("businessDaysBetween", businessDaysBetweenFn, new(func(...BlValue) BlNumber)),
	}
}
