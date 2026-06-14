package core

import "testing"

func evalUnary(t *testing.T, src string, inputType Type, in BlValue) string {
	t.Helper()
	e, err := UnaryTest(src, inputType)
	if err != nil {
		t.Fatalf("compile unary %q: %v", src, err)
	}
	out, err := e.Evaluate(in)
	if err != nil {
		t.Fatalf("eval unary %q: %v", src, err)
	}
	return out.String()
}

func TestUnaryTests(t *testing.T) {
	n21, _ := Number(21)
	n70, _ := Number(70)
	n5, _ := Number(5)

	cases := []struct {
		src  string
		typ  Type
		in   BlValue
		want string
	}{
		{`>= 18`, TypeNumber, n21, "true"},
		{`>= 18`, TypeNumber, n5, "false"},
		{`< 10`, TypeNumber, n5, "true"},
		{`[18..65]`, TypeNumber, n21, "true"},
		{`[18..65]`, TypeNumber, n70, "false"},
		{`-`, TypeNumber, n70, "true"},
		{`-`, TypeNumber, Null(), "true"},
		{`5`, TypeNumber, n5, "true"},
		{`5`, TypeNumber, n21, "false"},
		{`2, 3, 4`, TypeNumber, mustNum(t, 3), "true"},
		{`2, 3, 4`, TypeNumber, n5, "false"},
		{`< 10, > 50`, TypeNumber, n70, "true"},
		{`< 10, > 50`, TypeNumber, n21, "false"},
		{`not(0)`, TypeNumber, n5, "true"},
		{`not(5)`, TypeNumber, n5, "false"},
		{`contains(?, "urgent")`, TypeString, mustStr(t, "urgent notice"), "true"},
		{`contains(?, "urgent")`, TypeString, mustStr(t, "all good"), "false"},
		{`endsWith(?, "@blkit.io")`, TypeString, mustStr(t, "a@blkit.io"), "true"},
		{`"valid"`, TypeString, mustStr(t, "valid"), "true"},
		{`"low", "medium"`, TypeString, mustStr(t, "medium"), "true"},
		{`?.year >= 2025`, TypeDate, mustDate(t, "2025-03-28"), "true"},
		{`.year >= 2025`, TypeDate, mustDate(t, "2024-03-28"), "false"},
	}
	for _, c := range cases {
		t.Run(c.src, func(t *testing.T) {
			if got := evalUnary(t, c.src, c.typ, c.in); got != c.want {
				t.Errorf("UnaryTest(%q) on %v = %q, want %q", c.src, c.in, got, c.want)
			}
		})
	}
}

func mustNum(t *testing.T, n int) BlNumber    { v, _ := Number(n); return v }
func mustStr(t *testing.T, s string) BlString { v, _ := String(s); return v }
func mustDate(t *testing.T, s string) BlDate  { v, _ := Date(s); return v }
