package core

import (
	"strings"

	"github.com/expr-lang/expr"
	"github.com/expr-lang/expr/vm"
)

// BlUnaryTest is a compiled decision-table cell predicate over a single typed
// input T (the implicit `?`). Parse once with UnaryTest, then Evaluate each
// candidate input.
type BlUnaryTest[T BlValue] struct {
	source  string
	program *vm.Program
}

// unaryEnv binds the single unary-test input to the internal `?` placeholder so
// the strict struct env type-checks the cell against the input type T.
type unaryEnv[T BlValue] struct {
	Input T `expr:"__input"`
}

// UnaryTest compiles a unary-test source string whose implicit `?` placeholder
// holds a value of type T at evaluation time. The unary-test forms are
// normalised into ordinary `?`-referencing expressions (with `?` bound to the
// input), then run through the same parse/patch/type-check/compile pipeline as
// Expr. Evaluate is passed the input value directly (no dictionary wrapping).
func UnaryTest[T BlValue](source string) (*BlUnaryTest[T], error) {
	if strings.TrimSpace(source) == "" {
		return nil, &ParseError{Source: source, Err: errEmptySource}
	}
	rewritten := normaliseUnaryTest(source)
	program, err := compileWithEnv(rewritten, unaryEnv[T]{}, map[string]bool{inputPlaceholder: true})
	if err != nil {
		return nil, &ParseError{Source: source, Err: err}
	}
	return &BlUnaryTest[T]{source: source, program: program}, nil
}

// Source returns the original unary-test source text.
func (u *BlUnaryTest[T]) Source() string { return u.source }

// Evaluate tests input against the compiled unary-test predicate.
func (u *BlUnaryTest[T]) Evaluate(input T) (BlValue, error) {
	out, err := expr.Run(u.program, unaryEnv[T]{Input: input})
	if err != nil {
		return nil, &TypeError{Op: "evaluate", Detail: err.Error()}
	}
	return asBl(out), nil
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
