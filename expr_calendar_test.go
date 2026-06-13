package blkit

import "testing"

func ukHolidays(t *testing.T) BlCalendar {
	t.Helper()
	d := func(s string) BlValue { v, _ := Date(s); return v }
	cal, err := Calendar([]BlCalendarEntry{
		CalendarEntry(d("2025-01-01"), "New Year's Day"),
		CalendarEntry(d("2025-04-18"), "Good Friday"),
		CalendarEntry(d("2025-04-21"), "Easter Monday"),
		CalendarEntry(d("2025-12-25"), "Christmas Day"),
		CalendarEntry(d("2025-12-26"), "Boxing Day"),
	})
	if err != nil {
		t.Fatalf("Calendar: %v", err)
	}
	return cal
}

func evalCal(t *testing.T, src string, cal BlCalendar) string {
	t.Helper()
	schema := BlSchema{{Name: "cal", Type: TypeCalendar}}
	e, err := Expr(src, schema)
	if err != nil {
		t.Fatalf("compile %q: %v", src, err)
	}
	in, _ := Dictionary(map[string]BlValue{"cal": cal})
	out, err := e.Evaluate(in)
	if err != nil {
		t.Fatalf("eval %q: %v", src, err)
	}
	return out.String()
}

func TestCalendar(t *testing.T) {
	cal := ukHolidays(t)
	cases := map[string]string{
		`count(cal)`:                               "5",
		`isEmpty(cal)`:                             "false",
		`contains(cal, date("2025-12-25"))`:        "true",
		`contains(cal, date("2025-12-27"))`:        "false",
		`date("2025-12-25") in cal`:                "true",
		`isPublicHoliday(date("2025-12-25"), cal)`: "true",
		`isPublicHoliday(date("2025-07-01"), cal)`: "false",
		`isBusinessDay(date("2025-12-25"), cal)`:   "false", // Christmas (Thursday) but holiday
		`isBusinessDay(date("2025-12-24"), cal)`:   "true",  // Wednesday, not a holiday
		`isBusinessDay(date("2025-12-27"), cal)`:   "false", // Saturday
		`isWeekend(date("2025-12-27"))`:            "true",
		// Good Friday 2025-04-18 + Easter Mon 04-21 are holidays; 04-17 is Thu.
		`addBusinessDays(date("2025-04-17"), 2, cal)`:                      "2025-04-23",
		`nextBusinessDay(date("2025-12-24"), cal)`:                         "2025-12-29", // skip Christmas, Boxing, weekend
		`businessDaysBetween(date("2025-12-22"), date("2025-12-28"), cal)`: "3",          // Mon,Tue,Wed (24 ok? 25,26 holidays) → 22,23,24
		`validFrom(cal)`:             "null",
		`find(cal, "Christmas Day")`: `[2025-12-25: Christmas Day]`,
		// accessors
		`count(entries(cal))`:                                             "5",
		`entriesFor(cal, date("2025-12-25"))`:                             `[2025-12-25: Christmas Day]`,
		`count(entriesIn(cal, [date("2025-12-01")..date("2025-12-31")]))`: "2",
		// business-day navigation backwards
		`prevBusinessDay(date("2025-12-29"), cal)`:         "2025-12-24", // skip weekend + Boxing + Christmas
		`subtractBusinessDays(date("2025-12-29"), 2, cal)`: "2025-12-23",
		// entry navigation (strictly after / before a point; n defaults to 1)
		`next(cal, date("2025-12-25"))`:    "2025-12-26: Boxing Day",
		`prev(cal, date("2025-12-25"))`:    "2025-04-21: Easter Monday",
		`next(cal, date("2025-01-01"), 2)`: "2025-04-21: Easter Monday",
	}
	for src, want := range cases {
		t.Run(src, func(t *testing.T) {
			if got := evalCal(t, src, cal); got != want {
				t.Errorf("%s = %q, want %q", src, got, want)
			}
		})
	}
}

func boundedCal(t *testing.T) BlCalendar {
	t.Helper()
	d := func(s string) BlValue { v, _ := Date(s); return v }
	c, err := Calendar(
		[]BlCalendarEntry{CalendarEntry(d("2025-12-25"), "Christmas")},
		WithValidity(mustRange(t, "2025-01-01", "2025-12-31")),
	)
	if err != nil {
		t.Fatal(err)
	}
	return c
}

func mustRange(t *testing.T, a, b string) BlRange {
	da, _ := Date(a)
	db, _ := Date(b)
	r, _ := Range(da, db, true, true)
	return r
}

func TestStrictCalendarRange(t *testing.T) {
	cal := boundedCal(t)
	schema := BlSchema{{Name: "cal", Type: TypeCalendar}}
	in, _ := Dictionary(map[string]BlValue{"cal": cal})
	// in-bounds: no error
	e, _ := Expr(`addBusinessDays(date("2025-06-02"), 3, cal, true)`, schema)
	if _, err := e.Evaluate(in); err != nil {
		t.Errorf("in-bounds strict errored: %v", err)
	}
	// stepping past validTo with strict → CalendarRangeError
	e2, _ := Expr(`addBusinessDays(date("2025-12-30"), 5, cal, true)`, schema)
	_, err := e2.Evaluate(in)
	if err == nil {
		t.Errorf("expected CalendarRangeError past validity bound")
	}
	// without strict → no error
	e3, _ := Expr(`addBusinessDays(date("2025-12-30"), 5, cal)`, schema)
	if _, err := e3.Evaluate(in); err != nil {
		t.Errorf("non-strict errored: %v", err)
	}
}

func TestCalendarValidity(t *testing.T) {
	cal := boundedCal(t) // validity 2025-01-01..2025-12-31
	assertEval := func(src, want string) {
		t.Helper()
		if got := evalCal(t, src, cal); got != want {
			t.Errorf("%s = %q, want %q", src, got, want)
		}
	}
	assertEval(`validRange(cal)`, "[2025-01-01..2025-12-31]")
	assertEval(`validFrom(cal)`, "2025-01-01")
	assertEval(`validTo(cal)`, "2025-12-31")
}

func TestCalendarMerge(t *testing.T) {
	c1 := ukHolidays(t)
	d, _ := Date("2026-01-01")
	c2, _ := Calendar([]BlCalendarEntry{CalendarEntry(d, "New Year 2026")})
	schema := BlSchema{{Name: "a", Type: TypeCalendar}, {Name: "b", Type: TypeCalendar}}
	e, err := Expr(`count(calendarMerge([a, b]))`, schema)
	if err != nil {
		t.Fatalf("compile: %v", err)
	}
	in, _ := Dictionary(map[string]BlValue{"a": c1, "b": c2})
	out, err := e.Evaluate(in)
	if err != nil {
		t.Fatalf("eval: %v", err)
	}
	if out.String() != "6" { // 5 UK holidays + 1
		t.Errorf("calendarMerge count = %q, want 6", out.String())
	}
}

func TestCalendarDropKeep(t *testing.T) {
	cal := ukHolidays(t) // from expr_calendar_test.go
	schema := BlSchema{{Name: "cal", Type: TypeCalendar}}
	in, _ := Dictionary(map[string]BlValue{"cal": cal})
	check := func(src, want string) {
		t.Helper()
		e, err := Expr(src, schema)
		if err != nil {
			t.Fatalf("compile %q: %v", src, err)
			return
		}
		out, err := e.Evaluate(in)
		if err != nil {
			t.Fatalf("eval %q: %v", src, err)
			return
		}
		if out.String() != want {
			t.Errorf("%s = %q, want %q", src, out.String(), want)
		}
	}
	check(`count(calendarDrop(cal, "Boxing Day"))`, "4")                             // drop by name
	check(`count(calendarKeep(cal, "Christmas Day"))`, "1")                          // keep by name
	check(`count(calendarDrop(cal, date("2025-12-25")))`, "4")                       // drop by date value
	check(`count(calendarDrop(cal, pattern(".*Day")))`, "2")                         // drop names ending "Day": New Year's, Good Friday(no), ... regex on names
	check(`count(calendarKeep(cal, [date("2025-01-01"), date("2025-12-25")]))`, "2") // keep by list of dates
}
