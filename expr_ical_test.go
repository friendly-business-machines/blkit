package blkit

import (
	"strings"
	"testing"
)

const sampleICS = `BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
UID:1
SUMMARY:Christmas Day
DTSTART;VALUE=DATE:20251225
END:VEVENT
BEGIN:VEVENT
UID:2
SUMMARY:Holiday closure
DTSTART;VALUE=DATE:20251224
DTEND;VALUE=DATE:20260102
END:VEVENT
END:VCALENDAR`

func TestImportICal(t *testing.T) {
	cal, err := ImportICal(strings.NewReader(sampleICS))
	if err != nil {
		t.Fatalf("ImportICal: %v", err)
	}
	if n := len(cal.entries); n != 2 {
		t.Fatalf("got %d entries, want 2", n)
	}
	// point entry: Christmas
	schema := BlSchema{{Name: "cal", Type: TypeCalendar}}
	in, _ := Dictionary(map[string]BlValue{"cal": cal})
	check := func(src, want string) {
		e, _ := Expr(src, schema)
		out, err := e.Evaluate(in)
		if err != nil {
			t.Fatalf("%s: %v", src, err)
		}
		if out.String() != want {
			t.Errorf("%s = %q, want %q", src, out.String(), want)
		}
	}
	check(`contains(cal, date("2025-12-25"))`, "true")
	check(`contains(cal, date("2025-12-28"))`, "true")  // inside the closure range
	check(`contains(cal, date("2026-01-05"))`, "false") // after closure (DTEND exclusive → ends 2026-01-01)
	check(`count(cal)`, "2")
}
