package core

import (
	"strings"

	"github.com/expr-lang/expr"
)

// UnaryTest compiles a unary-test source string. inputType is the type the
// implicit `?` placeholder will hold at evaluation time. The unary-test forms
// are normalised into ordinary `?`-referencing expressions (with `?` bound to
// the input), then run through the same parse/patch/type-check/compile pipeline
// as Expr. The returned BlExpr is evaluated by passing the input value directly
// (no dictionary wrapping).
func UnaryTest(source string, inputType Type) (BlExpr, error) {
	if strings.TrimSpace(source) == "" {
		return nil, &ParseError{Source: source, Err: errEmptySource}
	}
	rewritten := normaliseUnaryTest(source)
	schema := BlSchema{{Name: inputPlaceholder, Type: inputType}}
	src, err := normalise(rewritten)
	if err != nil {
		return nil, &ParseError{Source: source, Err: err}
	}
	if err := checkUndefinedVars(src, schema); err != nil {
		return nil, &ParseError{Source: source, Err: err}
	}
	program, err := expr.Compile(src, buildOptions(schema, rewritten)...)
	if err != nil {
		return nil, &ParseError{Source: source, Err: err}
	}
	return &compiled{program: program, source: source}, nil
}

// normaliseUnaryTest rewrites a unary-test source string into an ordinary
// expression referencing the implicit input (rendered as the internal
// placeholder). A top-level comma list is a disjunction.
func normaliseUnaryTest(source string) string {
	parts := splitTopLevelCommas(source)
	rewritten := make([]string, 0, len(parts))
	for _, p := range parts {
		rewritten = append(rewritten, normaliseUnaryPart(strings.TrimSpace(p)))
	}
	if len(rewritten) == 1 {
		return rewritten[0]
	}
	return "(" + strings.Join(rewritten, " or ") + ")"
}

func normaliseUnaryPart(p string) string {
	if p == "-" {
		return "true" // wildcard matches anything
	}
	if strings.Contains(p, "?") {
		// A `?`-expression already references the input; bind `?` to it.
		return strings.ReplaceAll(p, "?", inputPlaceholder)
	}
	if strings.HasPrefix(p, "not(") && strings.HasSuffix(p, ")") {
		inner := p[len("not(") : len(p)-1]
		return "not(" + normaliseUnaryTest(inner) + ")"
	}
	if startsWithComparison(p) {
		return inputPlaceholder + " " + p
	}
	if strings.HasPrefix(p, ".") {
		return inputPlaceholder + p
	}
	if strings.HasPrefix(p, "[") || strings.HasPrefix(p, "(") {
		// an interval form → membership test
		return inputPlaceholder + " in " + p
	}
	// bare value → equality test
	return inputPlaceholder + " = " + p
}

func startsWithComparison(p string) bool {
	for _, op := range []string{"<=", ">=", "!=", "<", ">", "="} {
		if strings.HasPrefix(p, op) {
			return true
		}
	}
	return false
}
