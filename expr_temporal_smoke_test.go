package blkit

import "testing"

func TestTemporalSmoke(t *testing.T) {
	assertEval(t, map[string]string{
		// construction + String
		`date("2025-03-28")`:              "2025-03-28",
		`date(2025, 3, 28)`:               "2025-03-28",
		`time("11:45:30")`:                "11:45:30",
		`datetime("2025-03-28T14:30:00")`: "2025-03-28T14:30:00",
		`dtDuration("P1DT2H30M")`:         "P1DT2H30M",
		`dtDuration("PT90M")`:             "PT1H30M",
		`ymDuration("P1Y6M")`:             "P1Y6M",
		`ymDuration("P13M")`:              "P1Y1M",
		// component access
		`date("2025-03-28").year`:                 "2025",
		`date("2025-03-28").month`:                "3",
		`date("2025-03-28").day`:                  "28",
		`date("2025-03-24").dayName`:              "Monday",
		`date("2019-09-17").dayOfYear`:            "260",
		`date("2025-09-17").quarter`:              "3",
		`time("11:45:30").hour`:                   "11",
		`datetime("2025-03-28T14:30:00").hour`:    "14",
		`dtDuration("P2DT3H45M10S").days`:         "2",
		`dtDuration("P2DT3H45M10S").hours`:        "3",
		`dtDuration("P2DT3H45M10S").totalSeconds`: "186310",
		`ymDuration("P2Y7M").years`:               "2",
		`ymDuration("P2Y7M").totalMonths`:         "31",
		// arithmetic
		`date("2025-01-31") + ymDuration("P1M")`:  "2025-02-28",
		`date("2025-03-28") - date("2025-01-01")`: "P86D",
		`dtDuration("P1D") + dtDuration("PT12H")`: "P1DT12H",
		`dtDuration("PT1H") * 2.5`:                "PT2H30M",
		`dtDuration("PT1H") / 4`:                  "PT15M",
		`ymDuration("P6M") * 3`:                   "P1Y6M",
		`time("23:00:00") + dtDuration("PT2H")`:   "01:00:00",
		`-dtDuration("P2DT3H")`:                   "-P2DT3H",
		// comparison
		`date("2025-01-01") < date("2025-06-01")`:  "true",
		`dtDuration("PT60S") = dtDuration("PT1M")`: "true",
		`dtDuration("PT60S") < dtDuration("PT2M")`: "true",
		`ymDuration("P1Y") = ymDuration("P12M")`:   "true",
		// functions
		`abs(dtDuration("-PT5H"))`:                                  "PT5H",
		`isNegative(dtDuration("-PT1H"))`:                           "true",
		`isWeekday(date("2025-03-24"))`:                             "true",
		`isWeekend(date("2025-03-29"))`:                             "true",
		`lastDayOfMonth(date("2024-02-10"))`:                        "2024-02-29",
		`firstDayOfMonth(date("2025-02-14"))`:                       "2025-02-01",
		`daysBetween(date("2025-01-01"), date("2025-03-15"))`:       "73",
		`ymDurationBetween(date("2011-12-22"), date("2013-08-24"))`: "P1Y8M",
		`dtDurationBetween(date("2025-01-01"), date("2025-03-28"))`: "P86D",
		`round(dtDuration("PT37M"), dtDuration("PT15M"))`:           "PT30M",
		`financialYear(date("2024-08-01"), "AU")`:                   "FY2025",
		`financialYearQuarter(date("2024-08-01"), "AU")`:            "FY2025Q1",
		// cross-kind null
		`date("2025-01-01Z") < date("2025-06-01")`: "null",
	})
}
