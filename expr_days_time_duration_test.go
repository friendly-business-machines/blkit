package blkit

import "testing"

func TestDaysTimeDuration(t *testing.T) {
	assertEval(t, map[string]string{
		// construction + String (normalising)
		`dtDuration("P1DT2H30M")`: "P1DT2H30M",
		`dtDuration("PT90M")`:     "PT1H30M",
		// component access
		`dtDuration("P2DT3H45M10S").days`:         "2",
		`dtDuration("P2DT3H45M10S").hours`:        "3",
		`dtDuration("P2DT3H45M10S").totalSeconds`: "186310",
		// arithmetic
		`dtDuration("P1D") + dtDuration("PT12H")`:     "P1DT12H",
		`dtDuration("P1DT12H") - dtDuration("PT12H")`: "P1D",
		`dtDuration("PT1H") * 2.5`:                    "PT2H30M",
		`dtDuration("PT1H") / 4`:                      "PT15M",
		`-dtDuration("P2DT3H")`:                       "-P2DT3H",
		// comparison
		`dtDuration("PT60S") = dtDuration("PT1M")`: "true",
		`dtDuration("PT60S") < dtDuration("PT2M")`: "true",
		// functions
		`abs(dtDuration("-PT5H"))`:                        "PT5H",
		`isNegative(dtDuration("-PT1H"))`:                 "true",
		`round(dtDuration("PT37M"), dtDuration("PT15M"))`: "PT30M",
	})
}

func TestDaysTimeDurationErrors(t *testing.T) {
	assertErr(t,
		`dtDuration("PT1H") + ymDuration("P1M")`,  // cannot mix duration kinds
		`dtDuration("PT1H") * dtDuration("PT1H")`, // cannot multiply two durations
		`dtDuration("garbage")`,                   // unparseable duration literal
	)
}
