package core

import (
	"errors"
	"fmt"
	"reflect"

	"github.com/expr-lang/expr"
	"github.com/expr-lang/expr/vm"
)

// Type identifies a language type for parse-time checking and `instance of`.
type Type int

const (
	TypeNull Type = iota
	TypeNumber
	TypeString
	TypeBoolean
	TypeDate
	TypeTime
	TypeDateTime
	TypeDaysTimeDuration
	TypeYearsMonthsDuration
	TypeList
	TypeDictionary
	TypeRange
	TypeTable
	TypeCalendar
	TypeRegex
	TypeGroupedTable // transient t.groupBy(...) handle
	TypeSortKey      // transient asc/desc/inOrder sort key
	TypeAny
)

// typeName renders a Type as its lowercase language type name (used by
// `instance of` and error messages).
func typeName(t Type) string {
	switch t {
	case TypeNull:
		return "null"
	case TypeNumber:
		return "number"
	case TypeString:
		return "string"
	case TypeBoolean:
		return "boolean"
	case TypeDate:
		return "date"
	case TypeTime:
		return "time"
	case TypeDateTime:
		return "datetime"
	case TypeDaysTimeDuration:
		return "dtDuration"
	case TypeYearsMonthsDuration:
		return "ymDuration"
	case TypeList:
		return "list"
	case TypeDictionary:
		return "dictionary"
	case TypeRange:
		return "range"
	case TypeTable:
		return "table"
	case TypeCalendar:
		return "calendar"
	case TypeRegex:
		return "regex"
	case TypeAny:
		return "any"
	default:
		return "unknown"
	}
}

// BlExpr is a compiled, type-checked expression over a concrete env struct E.
// Parse once with Expr, then Evaluate repeatedly. E's exported fields — renamed
// by `expr:"name"` struct tags — are the variables the source may reference; the
// Go compiler rejects passing any other type to Evaluate, and an undeclared name
// is a compile error.
type BlExpr[E any] struct {
	source  string
	program *vm.Program
}

// Source returns the original source text (before normalisation).
func (e *BlExpr[E]) Source() string { return e.source }

// Evaluate runs the compiled program against env and returns the result.
func (e *BlExpr[E]) Evaluate(env E) (BlValue, error) {
	out, err := expr.Run(e.program, env)
	if err != nil {
		return nil, &TypeError{Op: "evaluate", Detail: err.Error()}
	}
	return asBl(out), nil
}

// NoEnv is the env type for variable-free expressions (`1 + 1`,
// `date("2025-01-01")`); pair it with Expr[NoEnv] or the ExprNoEnv shorthand and
// evaluate with Evaluate(NoEnv{}).
type NoEnv = struct{}

// inputPlaceholder is the internal variable name the `?` unary-test placeholder
// is rewritten to (`?` is not a legal expr identifier).
const inputPlaceholder = "__input"

// typeRegistrations lists every spoke's option assembler. baseOptions calls
// each to learn that spoke's value type, functions, and operator impls. It is a
// function (not a package var) so the chain funcOptions → apply → BlFunc.Apply →
// compileDynamic → baseOptions doesn't form a static initialization cycle.
func typeRegistrations() []func() []expr.Option {
	return []func() []expr.Option{
		numberOptions,
		stringOptions,
		booleanOptions,
		nullOptions,
		dateOptions,
		timeOptions,
		datetimeOptions,
		daysTimeDurationOptions,
		yearsMonthsDurationOptions,
		listOptions,
		dictionaryOptions,
		rangeOptions,
		intervalOptions,
		comprehensionOptions,
		tableOptions,
		tableMethodOptions,
		calendarOptions,
		typeTestOptions,
		funcOptions,
		overloadOptions,
	}
}

// Expr compiles a source string once against the concrete env struct E. E's
// exported fields (renamed by `expr:"name"` tags) declare the variables the
// source may reference; an undeclared name is a compile-time error. The returned
// BlExpr can be evaluated repeatedly.
// Expr calls expr.Compile once, registering each supplied UDF (a named, host-
// defined function — see Func) so the source may call it by name with compile-
// time-checked arguments.
func Expr[E any](source string, udfs ...UDF) (*BlExpr[E], error) {
	if source == "" {
		return nil, &ParseError{Source: source, Err: errEmptySource}
	}
	opts, err := udfOptions(udfs)
	if err != nil {
		return nil, &ParseError{Source: source, Err: err}
	}
	program, err := compileWithEnv(source, *new(E), envFieldNames(reflect.TypeOf((*E)(nil)).Elem()), opts...)
	if err != nil {
		return nil, &ParseError{Source: source, Err: err}
	}
	return &BlExpr[E]{source: source, program: program}, nil
}

// ExprNoEnv compiles a variable-free expression. It is shorthand for Expr[NoEnv];
// evaluate the result with Evaluate(NoEnv{}).
func ExprNoEnv(source string, udfs ...UDF) (*BlExpr[NoEnv], error) {
	return Expr[NoEnv](source, udfs...)
}

// udfOptions collects each UDF's expr.Function registration, rejecting a
// duplicate function name within one call.
func udfOptions(udfs []UDF) ([]expr.Option, error) {
	if len(udfs) == 0 {
		return nil, nil
	}
	seen := make(map[string]bool, len(udfs))
	opts := make([]expr.Option, 0, len(udfs))
	for _, u := range udfs {
		name := u.udfName()
		if seen[name] {
			return nil, fmt.Errorf("duplicate function %q", name)
		}
		seen[name] = true
		opts = append(opts, u.udfOption())
	}
	return opts, nil
}

// buildOptions assembles the strict typed environment for E plus the
// env-independent registrations.
func buildOptions[E any]() []expr.Option {
	return buildOptionsEnv(*new(E))
}

// buildOptionsEnv assembles the strict typed environment for an explicit env
// value plus the env-independent registrations.
func buildOptionsEnv(env any) []expr.Option {
	return append([]expr.Option{expr.Env(env)}, baseOptions()...)
}

// compileWithEnv normalises a source string, rejects any reference to a name not
// in declared (blkit's own undefined-name discipline — see firstUndefined), and
// calls expr.Compile against the explicit struct env value plus any extra options
// (e.g. UDF registrations). It backs Expr[E], UnaryTest, DecisionExpression, and
// Func, whose env types are built at runtime (reflect.StructOf / reflected param
// structs) and so have no Go type parameter. The raw error is returned; callers
// wrap it in a ParseError with their original source.
func compileWithEnv(source string, env any, declared map[string]bool, extra ...expr.Option) (*vm.Program, error) {
	src, err := normalise(source)
	if err != nil {
		return nil, err
	}
	if name, bad := firstUndefined(src, declared); bad {
		return nil, fmt.Errorf("unknown name %s", name)
	}
	opts := buildOptionsEnv(env)
	opts = append(opts, extra...)
	return expr.Compile(src, opts...)
}

// baseOptions assembles the env-independent options: every spoke's
// registrations, the operator dispatch functions, and the FEEL patcher.
func baseOptions() []expr.Option {
	var opts []expr.Option
	for _, reg := range typeRegistrations() {
		opts = append(opts, reg()...)
	}
	opts = append(opts, operatorRegistrations()...)
	opts = append(opts, expr.Patch(newFeelPatcher()))
	return opts
}

// compileDynamic compiles a source string against a permissive, untyped map
// environment. It is for internal use only — user-defined function bodies, whose
// parameter names come from the parsed source rather than a Go struct — and is
// never exposed to callers (the public surface is always struct-typed).
func compileDynamic(source string) (*vm.Program, error) {
	src, err := normalise(source)
	if err != nil {
		return nil, &ParseError{Source: source, Err: err}
	}
	opts := append([]expr.Option{expr.AllowUndefinedVariables(), expr.Env(map[string]any{})}, baseOptions()...)
	program, err := expr.Compile(src, opts...)
	if err != nil {
		return nil, &ParseError{Source: source, Err: err}
	}
	return program, nil
}

// runDynamic evaluates a program compiled by compileDynamic against named
// variable bindings.
func runDynamic(program *vm.Program, vars map[string]BlValue) (BlValue, error) {
	env := make(map[string]any, len(vars))
	for k, v := range vars {
		env[k] = v
	}
	out, err := expr.Run(program, env)
	if err != nil {
		return nil, &TypeError{Op: "evaluate", Detail: err.Error()}
	}
	return asBl(out), nil
}

var errEmptySource = errors.New("empty source string")
