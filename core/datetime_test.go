package core

import "testing"

func TestDatetime(t *testing.T) {
	assertEval(t, map[string]string{
		// construction + String
		`datetime("2025-03-28T14:30:00")`: "2025-03-28T14:30:00",
		// component access
		`datetime("2025-03-28T14:30:00").hour`: "14",
		// daysBetween with includeTime yields a fractional day count
		`daysBetween(datetime("2025-01-15T00:00:00"), datetime("2025-01-16T12:00:00"), true)`: "1.5",
		// now() is a zoned datetime
		`now() instance of datetime`: "true",
	})
}

func TestDatetimeArithmetic(t *testing.T) {
	assertEval(t, map[string]string{
		`datetime("2025-03-28T12:00:00") + ymDuration("P1M")`:               "2025-04-28T12:00:00",
		`datetime("2025-03-28T12:00:00") + dtDuration("PT3H")`:              "2025-03-28T15:00:00",
		`datetime("2025-03-28T12:00:00") - ymDuration("P1M")`:               "2025-02-28T12:00:00",
		`datetime("2025-03-28T12:00:00") - dtDuration("PT3H")`:              "2025-03-28T09:00:00",
		`datetime("2025-03-28T12:00:00") - datetime("2025-03-27T06:00:00")`: "P1DT6H",
	})
}

func TestDatetimeRezoning(t *testing.T) {
	assertEval(t, map[string]string{
		// withOffset/withTimezone preserve the instant, shifting wall-clock numbers
		`withOffset(datetime("2025-03-28T14:30:00+01:00"), dtDuration("PT2H"))`: "2025-03-28T15:30:00+02:00",
		`withOffset(time("14:30:00Z"), dtDuration("PT1H"))`:                     "15:30:00+01:00",
		`withTimezone(datetime("2025-03-28T14:30:00Z"), "Europe/Paris")`:        "2025-03-28T15:30:00[Europe/Paris]",
		// neither is defined for a naive value → null
		`withOffset(datetime("2025-03-28T14:30:00"), dtDuration("PT2H"))`: "null",
	})
}

func TestDatetimeZoneStripping(t *testing.T) {
	assertEval(t, map[string]string{
		// preserve wall-clock, drop the zone label
		`withoutOffset(datetime("2025-03-28T14:30:00+01:00"))`:           "2025-03-28T14:30:00",
		`withoutTimezone(datetime("2025-03-28T14:30:00Z"))`:              "2025-03-28T14:30:00",
		`withoutOffsetOrTimezone(datetime("2025-03-28T14:30:00+01:00"))`: "2025-03-28T14:30:00",
	})
}
