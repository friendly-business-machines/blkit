package blkit

import "testing"

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
