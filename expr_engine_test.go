package blkit

import "testing"

func TestEngineEvaluateWithSchema(t *testing.T) {
	schema, err := Schema(
		Field{Name: "age", Type: TypeNumber},
		Field{Name: "income", Type: TypeNumber},
	)
	if err != nil {
		t.Fatalf("schema: %v", err)
	}
	eligible, err := Expr(`age >= 18 and income > 50000`, schema)
	if err != nil {
		t.Fatalf("compile: %v", err)
	}
	age, _ := Number(21)
	income, _ := Number(60000)
	inputs, _ := Dictionary(map[string]BlValue{"age": age, "income": income})
	out, err := eligible.Evaluate(inputs)
	if err != nil {
		t.Fatalf("eval: %v", err)
	}
	if b, ok := out.(BlBoolean); !ok || !b.b {
		t.Errorf("got %v, want true", out)
	}
}

func TestEngineSourceRoundTrips(t *testing.T) {
	src := `age >= 18`
	e, _ := Expr(src, nil)
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
		`if score >= 750 then "prime" else if score >= 650 then "standard" else "subprime"`: "subprime",
	})
}

func TestConditionalNested(t *testing.T) {
	cases := []struct {
		score int
		want  string
	}{{800, "prime"}, {700, "standard"}, {600, "subprime"}}
	e, err := Expr(`if score >= 750 then "prime" else if score >= 650 then "standard" else "subprime"`,
		BlSchema{{Name: "score", Type: TypeNumber}})
	if err != nil {
		t.Fatalf("compile: %v", err)
	}
	for _, c := range cases {
		n, _ := Number(c.score)
		in, _ := Dictionary(map[string]BlValue{"score": n})
		out, _ := e.Evaluate(in)
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
		if _, err := Expr(src, nil); err == nil {
			t.Errorf("expected ParseError for %q", src)
		} else if _, ok := err.(*ParseError); !ok {
			t.Errorf("expected *ParseError for %q, got %T", src, err)
		}
	}
}

func TestUnknownVariableWithSchemaIsParseError(t *testing.T) {
	schema := BlSchema{{Name: "age", Type: TypeNumber}}
	if _, err := Expr(`unknownName > 1`, schema); err == nil {
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

func TestInstanceOf(t *testing.T) {
	assertEval(t, map[string]string{
		`42 instance of number`:               "true",
		`"x" instance of number`:              "false",
		`"x" instance of string`:              "true",
		`true instance of boolean`:            "true",
		`date("2025-01-01") instance of date`: "true",
		`pattern("[0-9]+") instance of regex`: "true",
		`[1, 2] instance of list`:             "true",
		`{a: 1} instance of dictionary`:       "true",
		`[1..10] instance of range`:           "true",
		`null instance of null`:               "true",
		`null instance of number`:             "false",
		`5 instance of null`:                  "false",
	})
}

func TestIsDefined(t *testing.T) {
	schema := BlSchema{{Name: "applicant", Type: TypeDictionary}}
	mid := func(src string, want string) {
		t.Helper()
		e, err := Expr(src, schema)
		if err != nil {
			t.Fatalf("compile %q: %v", src, err)
		}
		name, _ := String("Alice")
		appl, _ := Dictionary(map[string]BlValue{"name": name})
		in, _ := Dictionary(map[string]BlValue{"applicant": appl})
		out, _ := e.Evaluate(in)
		if out.String() != want {
			t.Errorf("%s = %q, want %q", src, out.String(), want)
		}
	}
	mid(`isDefined(applicant)`, "true")
	mid(`isDefined(applicant.middleName)`, "true") // path on a bound dict resolves
	mid(`isDefined(undeclaredName)`, "false")      // unbound name → false (no parse error)
}

func TestZipAndDateDiffs(t *testing.T) {
	assertEval(t, map[string]string{
		`zipStringJoin([["a", "b", "c"], ["1", "2", "3"]])`:                   `["a1", "b2", "c3"]`,
		`zipStringJoin([["a", "b"], ["1", "2"]], "-")`:                        `["a-1", "b-2"]`,
		`daysBetween(date("2025-01-01"), date("2025-03-15"))`:                 "73",
		`monthsBetween(date("2024-01-10"), date("2025-07-25"))`:               "18.483870967741936", // ≈ 18.4839 calendar
		`yearsBetween(date("2024-01-10"), date("2025-07-25"))`:                "1.536986301369863",  // ≈ 1.5370 calendar
		`monthsBetween(date("2025-01-01"), date("2025-01-31"), "actual/360")`: "1",
	})
}
