package blkit

import "testing"

func TestInlineFunctions(t *testing.T) {
	assertEval(t, map[string]string{
		`apply(function(x, y) x + y, 2, 3)`:            "5",
		`apply(function(i) i < 5, 3)`:                  "true",
		`apply(function(s) upperCase(s), "abc")`:       "ABC",
		`{add: function(x, y) x + y}.add`:              "function(x, y) x + y", // stored in dict
		`apply({add: function(x, y) x + y}.add, 4, 5)`: "9",                    // retrieve + apply
	})
}
