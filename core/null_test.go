package core

import "testing"

func TestNullConstruction(t *testing.T) {
	if !Null().IsNull() {
		t.Errorf("Null().IsNull() = false")
	}
	n, _ := Number(0)
	if n.IsNull() {
		t.Errorf("Number(0).IsNull() = true")
	}
}

func TestNullLiteralCaseInsensitive(t *testing.T) {
	assertEval(t, map[string]string{
		`null`: "null",
		`Null`: "null",
		`NULL`: "null",
	})
}

func TestNullEquality(t *testing.T) {
	// SQL-style: null is never equal to anything, including itself.
	assertEval(t, map[string]string{
		`null = null`:  "false",
		`null != null`: "false",
		`null = 5`:     "false",
		`null != 5`:    "false",
	})
}

func TestNullPropagation(t *testing.T) {
	assertEval(t, map[string]string{
		`null + 1`:  "null",
		`null * 2`:  "null",
		`null - 1`:  "null",
		`null < 5`:  "null",
		`null <= 5`: "null",
	})
}

func TestNullFunctions(t *testing.T) {
	assertEval(t, map[string]string{
		`isNull(null)`:       "true",
		`isNull(0)`:          "false",
		`isNull("")`:         "false",
		`getOrElse(null, 1)`: "1",
		`getOrElse(42, 1)`:   "42",
		`getOrElse("", "x")`: "", // empty string is defined
	})
}

func TestNullDivisionAndUndefined(t *testing.T) {
	assertEval(t, map[string]string{
		`1 / 0`:    "null",
		`sqrt(-1)`: "null",
	})
}
