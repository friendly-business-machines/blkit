package blkit

import "testing"

func TestComprehensions(t *testing.T) {
	assertEval(t, map[string]string{
		// indexing (1-based; negative from end; out of range → null)
		`[1, 2, 3, 4][1]`:  "1",
		`[1, 2, 3, 4][-1]`: "4",
		`[1, 2, 3, 4][5]`:  "null",
		// filter (magic item)
		`[1, 2, 3, 4][item > 2]`:   "[3, 4]",
		`[1, 2, 3, 4][even(item)]`: "[2, 4]",
		// for … return
		`for x in [1, 2, 3] return x * 2`:           "[2, 4, 6]",
		`for x in [1, 2], y in [3, 4] return x * y`: "[3, 4, 6, 8]",
		// some / every
		`some x in [1, 2, 3] satisfies x > 2`:               "true",
		`some x in [1, 2, 3] satisfies x > 5`:               "false",
		`every x in [1, 2, 3] satisfies x >= 1`:             "true",
		`every x in [1, 2, 3] satisfies x > 2`:              "false",
		`some x in [1, 2], y in [3, 4] satisfies x + y > 5`: "true",
		// composition with aggregates
		`sum(for x in [1, 2, 3] return x * x)`: "14",
		`count([1, 2, 3, 4][item > 2])`:        "2",
	})
}
