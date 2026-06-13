package blkit

import (
	"errors"

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

// BlExpr is a compiled source-text expression. Parse once with Expr, evaluate
// repeatedly.
type BlExpr interface {
	Evaluate(input BlValue) (BlValue, error)
	Source() string
}

type compiled struct {
	program *vm.Program
	source  string
}

func (c *compiled) Source() string { return c.source }

func (c *compiled) Evaluate(input BlValue) (BlValue, error) {
	env := evalEnv(input)
	out, err := expr.Run(c.program, env)
	if err != nil {
		return nil, &TypeError{Op: "evaluate", Detail: err.Error()}
	}
	return asBl(out), nil
}

// evalEnv builds the variable bindings the VM evaluates against from a single
// BlValue input. A BlDictionary spreads its entries as named variables; any
// other value is bound to the implicit unary-test placeholder.
func evalEnv(input BlValue) map[string]any {
	switch v := input.(type) {
	case nil:
		return map[string]any{}
	case BlDictionary:
		env := make(map[string]any, len(v.keys))
		for _, k := range v.keys {
			env[k] = v.m[k]
		}
		return env
	default:
		return map[string]any{inputPlaceholder: input}
	}
}

// inputPlaceholder is the internal variable name the `?` unary-test placeholder
// is rewritten to (`?` is not a legal expr identifier).
const inputPlaceholder = "__input"

// typeRegistrations lists every spoke's option assembler. buildOptions calls
// each to learn that spoke's value type, functions, and operator impls.
var typeRegistrations = []func() []expr.Option{
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
	overloadOptions,
}

// Expr compiles a source string once, optionally type-checking it against a
// declared schema. Pass nil to skip static variable checking. The returned
// BlExpr can be evaluated repeatedly.
func Expr(source string, schema BlSchema) (BlExpr, error) {
	if source == "" {
		return nil, &ParseError{Source: source, Err: errEmptySource}
	}
	src, err := normalise(source)
	if err != nil {
		return nil, &ParseError{Source: source, Err: err}
	}
	if schema != nil {
		if err := checkUndefinedVars(src, schema); err != nil {
			return nil, &ParseError{Source: source, Err: err}
		}
	}
	program, err := expr.Compile(src, buildOptions(schema, source)...)
	if err != nil {
		return nil, &ParseError{Source: source, Err: err}
	}
	return &compiled{program: program, source: source}, nil
}

// buildOptions assembles every spoke's registrations, the operator dispatch
// functions, the patcher, and the typed environment.
func buildOptions(schema BlSchema, source string) []expr.Option {
	opts := []expr.Option{}
	if schema == nil {
		opts = append(opts, expr.AllowUndefinedVariables(), expr.Env(map[string]any{}))
	} else {
		opts = append(opts, expr.Env(schema.env()))
	}
	for _, reg := range typeRegistrations {
		opts = append(opts, reg()...)
	}
	opts = append(opts, operatorRegistrations()...)
	opts = append(opts, expr.Patch(newFeelPatcher()))
	return opts
}

var errEmptySource = errors.New("empty source string")
