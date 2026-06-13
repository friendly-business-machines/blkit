package blkit

import "testing"

func TestTime(t *testing.T) {
	assertEval(t, map[string]string{
		// construction + String
		`time("11:45:30")`: "11:45:30",
		// component access
		`time("11:45:30").hour`: "11",
		// arithmetic (wraps within the day)
		`time("23:00:00") + dtDuration("PT2H")`: "01:00:00",
	})
}
