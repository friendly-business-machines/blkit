package core

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

type idEnv struct {
	Applicant BlDictionary `expr:"applicant"`
}

func TestIsDefined(t *testing.T) {
	name, _ := String("Alice")
	appl, _ := Dictionary(map[string]BlValue{"name": name})
	mid := func(src string, want string) {
		t.Helper()
		e, err := Expr[idEnv](src)
		if err != nil {
			t.Fatalf("compile %q: %v", src, err)
		}
		out, _ := e.Evaluate(idEnv{Applicant: appl})
		if out.String() != want {
			t.Errorf("%s = %q, want %q", src, out.String(), want)
		}
	}
	mid(`isDefined(applicant)`, "true")             // a declared field is always defined
	mid(`isDefined(applicant.name)`, "true")        // present key probes the dictionary
	mid(`isDefined(applicant.middleName)`, "false") // absent key

	// An undeclared name is now a compile-time error, not a runtime false.
	if _, err := Expr[idEnv](`isDefined(undeclaredName)`); err == nil {
		t.Errorf("expected compile error for isDefined(undeclaredName)")
	}
}
