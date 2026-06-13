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
	}
	for src, want := range cases {
		t.Run(src, func(t *testing.T) {
			if got := evalCal(t, src, cal); got != want {
				t.Errorf("%s = %q, want %q", src, got, want)
			}
		})
	}
}
