package blkit

import "testing"

func TestYearsMonthsDuration(t *testing.T) {
	assertEval(t, map[string]string{
		// construction + String (normalising)
		`ymDuration("P1Y6M")`: "P1Y6M",
		`ymDuration("P13M")`:  "P1Y1M",
		// component access
		`ymDuration("P2Y7M").years`:       "2",
		`ymDuration("P2Y7M").totalMonths`: "31",
		// arithmetic
		`ymDuration("P6M") * 3`: "P1Y6M",
		// comparison
		`ymDuration("P1Y") = ymDuration("P12M")`: "true",
	})
}
