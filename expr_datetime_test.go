package blkit

import "testing"

func TestDatetime(t *testing.T) {
	assertEval(t, map[string]string{
		// construction + String
		`datetime("2025-03-28T14:30:00")`: "2025-03-28T14:30:00",
		// component access
		`datetime("2025-03-28T14:30:00").hour`: "14",
		// daysBetween with includeTime yields a fractional day count
		`daysBetween(datetime("2025-01-15T00:00:00"), datetime("2025-01-16T12:00:00"), true)`: "1.5",
	})
}
