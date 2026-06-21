package core

import "testing"

func evalUnary[T BlValue](t *testing.T, src string, in T) string {
	t.Helper()
	e, err := UnaryTest[T](src)
	if err != nil {
		t.Fatalf("compile unary %q: %v", src, err)
	}
	out, err := e.Evaluate(in)
	if err != nil {
		t.Fatalf("eval unary %q: %v", src, err)
	}
	return out.String()
}

func TestUnaryTestsNumber(t *testing.T) {
	n5, n21, n70, n3 := mustNum(t, 5), mustNum(t, 21), mustNum(t, 70), mustNum(t, 3)
	cases := []struct {
		src  string
		in   BlNumber
		want string
	}{
		{`>= 18`, n21, "true"},
		{`>= 18`, n5, "false"},
		{`< 10`, n5, "true"},
		{`[18..65]`, n21, "true"},
		{`[18..65]`, n70, "false"},
		{`-`, n70, "true"},
		{`5`, n5, "true"},
		{`5`, n21, "false"},
		{`2, 3, 4`, n3, "true"},
		{`2, 3, 4`, n5, "false"},
		{`< 10, > 50`, n70, "true"},
		{`< 10, > 50`, n21, "false"},
		{`not(0)`, n5, "true"},
		{`not(5)`, n5, "false"},
	}
	for _, c := range cases {
		t.Run(c.src, func(t *testing.T) {
			if got := evalUnary[BlNumber](t, c.src, c.in); got != c.want {
				t.Errorf("UnaryTest(%q) on %v = %q, want %q", c.src, c.in, got, c.want)
			}
		})
	}
}

func TestUnaryTestsString(t *testing.T) {
	cases := []struct {
		src  string
		in   BlString
		want string
	}{
		{`contains(?, "urgent")`, mustStr(t, "urgent notice"), "true"},
		{`contains(?, "urgent")`, mustStr(t, "all good"), "false"},
		{`endsWith(?, "@blkit.io")`, mustStr(t, "a@blkit.io"), "true"},
		{`"valid"`, mustStr(t, "valid"), "true"},
		{`"low", "medium"`, mustStr(t, "medium"), "true"},
	}
	for _, c := range cases {
		t.Run(c.src, func(t *testing.T) {
			if got := evalUnary[BlString](t, c.src, c.in); got != c.want {
				t.Errorf("UnaryTest(%q) on %v = %q, want %q", c.src, c.in, got, c.want)
			}
		})
	}
}

func TestUnaryTestsDate(t *testing.T) {
	cases := []struct {
		src  string
		in   BlDate
		want string
	}{
		{`?.year >= 2025`, mustDate(t, "2025-03-28"), "true"},
		{`.year >= 2025`, mustDate(t, "2024-03-28"), "false"},
	}
	for _, c := range cases {
		t.Run(c.src, func(t *testing.T) {
			if got := evalUnary[BlDate](t, c.src, c.in); got != c.want {
				t.Errorf("UnaryTest(%q) on %v = %q, want %q", c.src, c.in, got, c.want)
			}
		})
	}
}

// The `-` wildcard matches anything, including a null input — exercised over the
// BlValue interface so a null can be passed where a typed input cannot.
func TestUnaryTestWildcardNull(t *testing.T) {
	if got := evalUnary[BlValue](t, `-`, BlValue(Null())); got != "true" {
		t.Errorf("wildcard on null = %q, want true", got)
	}
}

func mustNum(t *testing.T, n int) BlNumber    { v, _ := Number(n); return v }
func mustStr(t *testing.T, s string) BlString { v, _ := String(s); return v }
func mustDate(t *testing.T, s string) BlDate  { v, _ := Date(s); return v }
