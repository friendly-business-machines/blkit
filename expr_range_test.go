package blkit

import "testing"

func TestRange(t *testing.T) {
	assertEval(t, map[string]string{
		// literals + membership
		`5 in [1..10]`:       "true",
		`3 in [1..10]`:       "true",
		`10 in [1..10)`:      "false",
		`10 in [1..10]`:      "true",
		`1 in (1..10]`:       "false",
		`25 in [18..65]`:     "true",
		`5 between 1 and 10`: "true",
		// component access
		`[1..10].start`:         "1",
		`[1..10].end`:           "10",
		`[1..10].startIncluded`: "true",
		`[1..10).endIncluded`:   "false",
		// unbounded
		`5 in [18..null)`:  "false",
		`20 in [18..null)`: "true",
		// empty-range
		`isEmpty((3..3))`: "true",
		`isEmpty([3..3])`: "false",
		`7 in [5..3]`:     "null",
		// interval algebra
		`before([1..5], [6..10])`:         "true",
		`after([6..10], [1..5])`:          "true",
		`meets([1..5], [5..10])`:          "true",
		`metBy([5..10], [1..5])`:          "true",
		`overlaps([5..10], [1..6])`:       "true",
		`overlapsBefore([1..5], [4..10])`: "true",
		`overlapsAfter([4..10], [1..5])`:  "true",
		`includes([1..10], 5)`:            "true",
		`during(5, [1..10])`:              "true",
		`starts(1, [1..5])`:               "true",
		`startedBy([1..5], 1)`:            "true",
		`finishes(5, [1..5])`:             "true",
		`finishedBy([1..5], 5)`:           "true",
		`coincides([1..5], [1..5])`:       "true",
		`before(3, 5)`:                    "true",
		`during(date("2025-05-15"), [date("2025-04-01")..date("2025-06-30")])`: "true",
	})
}

func TestRangeErrors(t *testing.T) {
	// endpoints of mismatched types cannot form a range
	assertErr(t, `[date("2025-01-01")..5]`, `[1.."z"]`)
}
