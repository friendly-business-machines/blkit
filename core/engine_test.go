package core

import "testing"

type agePointsEnv struct {
	Age    BlNumber `expr:"age"`
	Points BlNumber `expr:"points"`
}

type ageEnv struct {
	Age BlNumber `expr:"age"`
}

type scoreEnv struct {
	Score BlNumber `expr:"score"`
}

func TestEngineEvaluateWithSchema(t *testing.T) {
	eligible, err := Expr[agePointsEnv](`age >= 18 and points > 50000`)
	if err != nil {
		t.Fatalf("compile: %v", err)
	}
	age, _ := Number(21)
	points, _ := Number(60000)
	out, err := eligible.Evaluate(agePointsEnv{Age: age, Points: points})
	if err != nil {
		t.Fatalf("eval: %v", err)
	}
	if b, ok := out.(BlBoolean); !ok || !b.b {
		t.Errorf("got %v, want true", out)
	}
}

func TestEngineSourceRoundTrips(t *testing.T) {
	src := `age >= 18`
	e, err := Expr[ageEnv](src)
	if err != nil {
		t.Fatalf("compile: %v", err)
	}
	if e.Source() != src {
		t.Errorf("Source() = %q, want %q", e.Source(), src)
	}
}

func TestArithmetic(t *testing.T) {
	assertEval(t, map[string]string{
		`2 + 3`:    "5",
		`10 - 4`:   "6",
		`3 * 4`:    "12",
		`10 / 4`:   "2.5",
		`2 ** 8`:   "256",
		`9 ** 0.5`: "3",
		`-(7)`:     "-7",
		`null + 1`: "null",
		`5 / 0`:    "null",
	})
}

func TestComparison(t *testing.T) {
	assertEval(t, map[string]string{
		`5 < 10`:              "true",
		`10 >= 10`:            "true",
		`10 > 10`:             "false",
		`"abc" = "abc"`:       "true",
		`"abc" != "abd"`:      "true",
		`5 between 1 and 10`:  "true",
		`15 between 1 and 10`: "false",
		`5 < "x"`:             "null", // incomparable
	})
}

func TestBooleanLogicEngine(t *testing.T) {
	assertEval(t, map[string]string{
		`true and false`: "false",
		`true or false`:  "true",
		`not(true)`:      "false",
		`false and null`: "false",
		`true or null`:   "true",
		`true and null`:  "null",
		`null or false`:  "null",
	})
}

func TestConditional(t *testing.T) {
	assertEval(t, map[string]string{
		`if 5 < 10 then "low" else "high"`:  "low",
		`if 12 < 10 then "low" else "high"`: "high",
		`if null then "low" else "high"`:    "high",
		`if 5 < 10 then 1 else 2`:           "1",
	})
}

func TestConditionalNested(t *testing.T) {
	cases := []struct {
		score int
		want  string
	}{{800, "gold"}, {700, "silver"}, {600, "bronze"}}
	e, err := Expr[scoreEnv](`if score >= 750 then "gold" else if score >= 650 then "silver" else "bronze"`)
	if err != nil {
		t.Fatalf("compile: %v", err)
	}
	for _, c := range cases {
		n, _ := Number(c.score)
		out, _ := e.Evaluate(scoreEnv{Score: n})
		if out.String() != c.want {
			t.Errorf("score=%d got %q want %q", c.score, out.String(), c.want)
		}
	}
}

func TestPrecedence(t *testing.T) {
	assertEval(t, map[string]string{
		`2 + 3 * 4`:   "14",
		`(2 + 3) * 4`: "20",
		`2 ** 3 * 2`:  "16",
		`-2 + 3`:      "1",
	})
}

func TestParseErrors(t *testing.T) {
	for _, src := range []string{``, `1 +`, `(1 + 2`} {
		if _, err := ExprNoEnv(src); err == nil {
			t.Errorf("expected ParseError for %q", src)
		} else if _, ok := err.(*ParseError); !ok {
			t.Errorf("expected *ParseError for %q, got %T", src, err)
		}
	}
}

func TestUnknownVariableIsParseError(t *testing.T) {
	if _, err := Expr[ageEnv](`unknownName > 1`); err == nil {
		t.Errorf("expected ParseError for unknown variable")
	}
}

func TestEdgeCases(t *testing.T) {
	assertEval(t, map[string]string{
		`1 + 1`: "2", // no input needed
		`null`:  "null",
	})
}

func TestExactDecimal(t *testing.T) {
	assertEval(t, map[string]string{
		`0.1 + 0.2`:  "0.3",
		`1500.50`:    "1500.5",
		`3.0 = 3.00`: "true",
		`1.5e3`:      "1500",
	})
}
