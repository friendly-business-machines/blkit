package core

import (
	"strings"

	"github.com/expr-lang/expr"
)

// BlFunc is an inline user-defined function value: `function(params) body`. It
// captures its parameter names and body source; it is applied by binding the
// arguments to the parameters and evaluating the body. The body sees only its
// parameters (no outer-scope closure), matching the spec's pure, bounded model.
type BlFunc struct {
	params []string
	body   string
}

func (BlFunc) Type() Type { return TypeAny }

func (f BlFunc) Equal(other BlValue) BlValue {
	o, ok := other.(BlFunc)
	if !ok {
		return BlBoolean{false}
	}
	return BlBoolean{f.body == o.body && strings.Join(f.params, ",") == strings.Join(o.params, ",")}
}

func (f BlFunc) String() string {
	return "function(" + strings.Join(f.params, ", ") + ") " + f.body
}

func (BlFunc) IsNull() bool { return false }
func (BlFunc) isBlValue()   {}

// Params returns the function's parameter names.
func (f BlFunc) Params() []string { return append([]string{}, f.params...) }

// Apply binds args to the parameters and evaluates the body.
func (f BlFunc) Apply(args []BlValue) (BlValue, error) {
	if len(args) != len(f.params) {
		return nil, &TypeError{Op: "apply", Detail: "argument count mismatch"}
	}
	prog, err := Expr(f.body, nil)
	if err != nil {
		return nil, err
	}
	entries := map[string]BlValue{}
	for i, p := range f.params {
		entries[p] = args[i]
	}
	input, _ := Dictionary(entries)
	return prog.Evaluate(input)
}

// mkFuncFn is the constructor the source rewrite emits: __mkfunc("p1,p2",
// "body").
func mkFuncFn(paramsStr, body BlString) BlFunc {
	var params []string
	for _, p := range strings.Split(paramsStr.s, ",") {
		if t := strings.TrimSpace(p); t != "" {
			params = append(params, t)
		}
	}
	return BlFunc{params: params, body: body.s}
}

// applyFn implements apply(f, args…).
func applyFn(args ...any) (any, error) {
	f, ok := asBl(args[0]).(BlFunc)
	if !ok {
		return nil, &TypeError{Op: "apply", Detail: "first argument must be a function"}
	}
	vals := make([]BlValue, len(args)-1)
	for i, a := range args[1:] {
		vals[i] = asBl(a)
	}
	return f.Apply(vals)
}

func funcOptions() []expr.Option {
	return []expr.Option{
		expr.Function("__mkfunc", typed2(mkFuncFn), new(func(BlValue, BlValue) BlFunc)),
		expr.Function("apply", applyFn, new(func(...BlValue) BlValue)),
	}
}
