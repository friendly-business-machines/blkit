package core

import "testing"

func TestDate(t *testing.T) {
	assertEval(t, map[string]string{
		// construction + String
		`date("2025-03-28")`: "2025-03-28",
		`date(2025, 3, 28)`:  "2025-03-28",
		// component access
		`date("2025-03-28").year`:      "2025",
		`date("2025-03-28").month`:     "3",
		`date("2025-03-28").day`:       "28",
		`date("2025-03-24").dayName`:   "Monday",
		`date("2019-09-17").dayOfYear`: "260",
		`date("2025-09-17").quarter`:   "3",
		// arithmetic
		`date("2025-01-31") + ymDuration("P1M")`:   "2025-02-28",
		`date("2025-03-28") - date("2025-01-01")`:  "P86D",
		`date("2025-03-28") + dtDuration("PT24H")`: "2025-03-29",
		`date("2025-03-28") - dtDuration("PT24H")`: "2025-03-27",
		`date("2025-03-28") - ymDuration("P1M")`:   "2025-02-28",
		// comparison
		`date("2025-01-01") < date("2025-06-01")`: "true",
		// cross-kind null (naive vs zoned)
		`date("2025-01-01Z") < date("2025-06-01")`: "null",
		// functions
		`isWeekday(date("2025-03-24"))`:                  "true",
		`isWeekend(date("2025-03-29"))`:                  "true",
		`lastDayOfMonth(date("2024-02-10"))`:             "2024-02-29",
		`firstDayOfMonth(date("2025-02-14"))`:            "2025-02-01",
		`financialYear(date("2024-08-01"), "AU")`:        "FY2025",
		`financialYearQuarter(date("2024-08-01"), "AU")`: "FY2025Q1",
	})
}

func TestDateDifference(t *testing.T) {
	assertEval(t, map[string]string{
		`daysBetween(date("2025-01-01"), date("2025-03-15"))`:                 "73",
		`ymDurationBetween(date("2011-12-22"), date("2013-08-24"))`:           "P1Y8M",
		`dtDurationBetween(date("2025-01-01"), date("2025-03-28"))`:           "P86D",
		`monthsBetween(date("2024-01-10"), date("2025-07-25"))`:               "18.483870967741936", // ≈ 18.4839 calendar
		`yearsBetween(date("2024-01-10"), date("2025-07-25"))`:                "1.536986301369863",  // ≈ 1.5370 calendar
		`monthsBetween(date("2025-01-01"), date("2025-01-31"), "actual/360")`: "1",
	})
}

func TestDateErrors(t *testing.T) {
	assertErr(t,
		`date("not-a-date")`,                                   // unparseable date literal
		`date("2025-01-01") + date("2025-01-02")`,              // cannot add two dates
		`firstDayOfWeekInMonth(date("2025-03-15"), "NotADay")`, // bad weekday name
	)
}

func TestDateNavigation(t *testing.T) {
	assertEval(t, map[string]string{
		`nextWeekday(date("2025-03-28"))`:                         "2025-03-31", // Fri → Mon
		`prevWeekday(date("2025-03-31"))`:                         "2025-03-28", // Mon → Fri
		`nextDayOfWeek(date("2025-03-28"), "Monday")`:             "2025-03-31",
		`prevDayOfWeek(date("2025-03-28"), "Monday")`:             "2025-03-24",
		`firstDayOfNextMonth(date("2025-03-15"))`:                 "2025-04-01",
		`lastDayOfPrevMonth(date("2025-03-15"))`:                  "2025-02-28",
		`weekdaysBetween(date("2025-03-24"), date("2025-03-28"))`: "5", // Mon–Fri inclusive
		`today() instance of date`:                                "true",
	})
}

func TestDateWeekInMonth(t *testing.T) {
	assertEval(t, map[string]string{
		`firstDayOfWeekInMonth(date("2025-03-15"), "Monday")`:   "2025-03-03",
		`lastDayOfWeekInMonth(date("2025-03-15"), "Friday")`:    "2025-03-28",
		`nthDayOfWeekInMonth(date("2025-03-15"), 2, "Monday")`:  "2025-03-10",
		`nthDayOfWeekInMonth(date("2025-03-15"), -1, "Monday")`: "2025-03-31",
		`nthDayOfWeekInMonth(date("2025-03-15"), 6, "Monday")`:  "null",
	})
}
