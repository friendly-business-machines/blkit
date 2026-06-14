package core

import "testing"

func TestBooleanConstruction(t *testing.T) {
	if b, _ := Boolean(true); !b.Native() {
		t.Errorf("Boolean(true) = %v", b)
	}
	if b, _ := Boolean(1); !b.Native() {
		t.Errorf("Boolean(1) = %v", b)
	}
	if b, _ := Boolean(0); b.Native() {
		t.Errorf("Boolean(0) = %v", b)
	}
	if b, _ := Boolean("True"); !b.Native() {
		t.Errorf(`Boolean("True") = %v`, b)
	}
	if _, err := Boolean("yes"); err == nil {
		t.Errorf(`expected error for Boolean("yes")`)
	}
	if _, err := Boolean("1"); err == nil {
		t.Errorf(`expected error for Boolean("1")`)
	}
}

func TestBooleanLiteralsCaseInsensitive(t *testing.T) {
	assertEval(t, map[string]string{
		`True`:  "true",
		`TRUE`:  "true",
		`False`: "false",
		`FALSE`: "false",
	})
}

func TestThreeValuedLogic(t *testing.T) {
	cases := map[string]string{
		`true and true`:   "true",
		`true and false`:  "false",
		`true and null`:   "null",
		`false and true`:  "false",
		`false and false`: "false",
		`false and null`:  "false",
		`null and true`:   "null",
		`null and false`:  "false",
		`null and null`:   "null",
		`true or true`:    "true",
		`true or false`:   "true",
		`true or null`:    "true",
		`false or false`:  "false",
		`false or null`:   "null",
		`null or true`:    "true",
		`null or false`:   "null",
		`null or null`:    "null",
		`not(true)`:       "false",
		`not(false)`:      "true",
		`not(null)`:       "null",
	}
	assertEval(t, cases)
}

func TestBooleanNoCoercion(t *testing.T) {
	// A non-boolean operand to and/or/not → null.
	assertEval(t, map[string]string{
		`not(5)`:     "null",
		`5 and true`: "null",
	})
}

func TestBooleanEquality(t *testing.T) {
	assertEval(t, map[string]string{
		`true = true`:  "true",
		`true = false`: "false",
		`true = 1`:     "false", // cross-type
	})
}
