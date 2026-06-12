package blkit

import "testing"

// evalNil compiles src with no schema and evaluates it against no input,
// returning the canonical string rendering of the result.
func evalNil(t *testing.T, src string) string {
	t.Helper()
	e, err := Expr(src, nil)
	if err != nil {
		t.Fatalf("compile %q: %v", src, err)
	}
	out, err := e.Evaluate(nil)
	if err != nil {
		t.Fatalf("eval %q: %v", src, err)
	}
	return out.String()
}

// assertEval runs a table of source → expected-rendering cases.
func assertEval(t *testing.T, cases map[string]string) {
	t.Helper()
	for src, want := range cases {
		src, want := src, want
		t.Run(src, func(t *testing.T) {
			if got := evalNil(t, src); got != want {
				t.Errorf("%s = %q, want %q", src, got, want)
			}
		})
	}
}
