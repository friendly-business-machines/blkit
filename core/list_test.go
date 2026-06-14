package core

import "testing"

func TestList(t *testing.T) {
	assertEval(t, map[string]string{
		// literals + functions
		`[1, 2, 3, 4]`:                 "[1, 2, 3, 4]",
		`count([1, 2, 3])`:             "3",
		`isEmpty([])`:                  "true",
		`isEmpty([1])`:                 "false",
		`listContains([1, 2, 3], 2)`:   "true",
		`2 in [1, 2, 3]`:               "true",
		`5 in [1, 2, 3]`:               "false",
		`"US" in ["US", "CA"]`:         "true",
		`indexOf([1, 2, 3, 2], 2)`:     "[2, 4]",
		`sublist([1, 2, 3], 2)`:        "[2, 3]",
		`append([1], 2, 3)`:            "[1, 2, 3]",
		`prepend([2, 3], 1)`:           "[1, 2, 3]",
		`concatenate([1, 2], [3])`:     "[1, 2, 3]",
		`union([1, 2], [2, 3])`:        "[1, 2, 3]",
		`intersection([1, 2], [2, 3])`: "[2]",
		`reverse([1, 2, 3])`:           "[3, 2, 1]",
		`flatten([[1, 2], [[3]], 4])`:  "[1, 2, 3, 4]",
		`distinct([1, 2, 2, 1])`:       "[1, 2]",
		`remove([1, 2, 3], 2)`:         "[1, 3]",
		`listReplace([1, 2, 3], 2, 9)`: "[1, 9, 3]",
		`sort([3, 1, 2])`:              "[1, 2, 3]",
		`sort([3, 1, 2], "desc")`:      "[3, 2, 1]",
		// projection over a list of dictionaries
		`[{name: "A"}, {name: "B"}].name`: `["A", "B"]`,
	})
}

func TestListAggregation(t *testing.T) {
	assertEval(t, map[string]string{
		`sum([1, 2, 3])`:               "6",
		`product([2, 3, 4])`:           "24",
		`min([3, 1, 2])`:               "1",
		`max([1, 2, 3])`:               "3",
		`mean([1, 2, 3])`:              "2",
		`median([6, 1, 2, 3])`:         "2.5",
		`mode([6, 1, 6, 1])`:           "[1, 6]",
		`all([true, true, false])`:     "false",
		`any([false, false, true])`:    "true",
		`all([])`:                      "true",
		`any([])`:                      "false",
		`stringJoin(["a", "b"], ", ")`: "a, b",
		`min(["banana", "apple"])`:     "apple",
		`sum([dtDuration("PT1H"), dtDuration("PT2H")])`: "PT3H",
		`stddev([2, 4, 7, 5])`:                          "2.0816659994661327352822977069799315", // sample stddev
	})
}

func TestListAggregationErrors(t *testing.T) {
	// mixing incompatible element types → TypeError (spec: min([1, "a"]),
	// "mixing numbers with durations")
	assertErr(t,
		`min([1, "a"])`,
		`max([1, "a"])`,
		`sum([1, dtDuration("PT1H")])`,
		`product([1, "a"])`,
		`mean([1, "a"])`,
		`median([1, "a"])`,
		`stddev([1, "a"])`,
	)
}

func TestListAggregationEmpty(t *testing.T) {
	// empty input → null (not an error)
	assertEval(t, map[string]string{
		`min([])`:    "null",
		`max([])`:    "null",
		`sum([])`:    "null",
		`mean([])`:   "null",
		`median([])`: "null",
		`stddev([])`: "null",
	})
}

func TestListMutation(t *testing.T) {
	assertEval(t, map[string]string{
		`insertAfter([1, 2, 3], 2, 9)`:        "[1, 2, 9, 3]",
		`insertBefore([1, 2, 3], 2, 9)`:       "[1, 9, 2, 3]",
		`duplicateValues([1, 2, 2, 3, 3, 3])`: "[2, 3]",
	})
}

func TestListInlinePredicates(t *testing.T) {
	assertEval(t, map[string]string{
		`remove([1, 2, 3], function(i) i = 2)`:         "[1, 3]",
		`remove([1, 2, 3], 2)`:                         "[1, 3]", // positional still works
		`listReplace([2, 4, 7], function(i) i < 5, 5)`: "[5, 5, 7]",
		`listReplace([1, 2, 3], 2, 9)`:                 "[1, 9, 3]", // positional still works
	})
}

func TestListSequence(t *testing.T) {
	assertEval(t, map[string]string{
		`seq(5, 10)`:                "[5, 6, 7, 8, 9, 10]",
		`seq(0, 10, 2)`:             "[0, 2, 4, 6, 8, 10]",
		`seq(10, 5)`:                "[10, 9, 8, 7, 6, 5]",
		`5:10`:                      "[5, 6, 7, 8, 9, 10]",
		`10:5`:                      "[10, 9, 8, 7, 6, 5]",
		`1+2:5*2`:                   "[3, 4, 5, 6, 7, 8, 9, 10]",
		`-2:2`:                      "[-2, -1, 0, 1, 2]",
		`{a: 1, b: 2}`:              "{a: 1, b: 2}", // dict sep untouched
		`{a: (3:5)}.a`:              "[3, 4, 5]",    // sequence in dict value (parenthesised)
		`count(5:10)`:               "6",
		`for x in 1:3 return x * 2`: "[2, 4, 6]",
	})
}

func TestListZipStringJoin(t *testing.T) {
	assertEval(t, map[string]string{
		`zipStringJoin([["a", "b", "c"], ["1", "2", "3"]])`: `["a1", "b2", "c3"]`,
		`zipStringJoin([["a", "b"], ["1", "2"]], "-")`:      `["a-1", "b-2"]`,
	})
}
