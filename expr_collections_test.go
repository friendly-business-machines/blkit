package blkit

import "testing"

func TestCollections(t *testing.T) {
	assertEval(t, map[string]string{
		// list literals + functions
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
		`seq(5, 10)`:                   "[5, 6, 7, 8, 9, 10]",
		`seq(0, 10, 2)`:                "[0, 2, 4, 6, 8, 10]",
		`seq(10, 5)`:                   "[10, 9, 8, 7, 6, 5]",
		// aggregation
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
		// dictionary literals + path + functions
		`{a: 1, b: 2}`:                            "{a: 1, b: 2}",
		`{name: "Alice", age: 30}.name`:           "Alice",
		`{a: {b: 3}}.a.b`:                         "3",
		`{a: 1}.missing`:                          "null",
		`getValue({foo: 123}, "foo")`:             "123",
		`getValue({x: 1, y: {z: 0}}, ["y", "z"])`: "0",
		`has({a: 1}, "a")`:                        "true",
		`has({a: 1}, "z")`:                        "false",
		`size({a: 1, b: 2})`:                      "2",
		`keys({b: 1, a: 2})`:                      `["a", "b"]`,
		`dictionaryRemove({a: 1, b: 2}, "a")`:     "{b: 2}",
		`{a: 1, b: 2} = {b: 2, a: 1}`:             "true",
		// projection
		`[{name: "A"}, {name: "B"}].name`: `["A", "B"]`,
	})
}

func TestInlinePredicates(t *testing.T) {
	assertEval(t, map[string]string{
		`remove([1, 2, 3], function(i) i = 2)`:         "[1, 3]",
		`remove([1, 2, 3], 2)`:                         "[1, 3]", // positional still works
		`listReplace([2, 4, 7], function(i) i < 5, 5)`: "[5, 5, 7]",
		`listReplace([1, 2, 3], 2, 9)`:                 "[1, 9, 3]", // positional still works
	})
}
