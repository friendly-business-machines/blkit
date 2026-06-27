package core

import (
	"errors"
	"fmt"
	"reflect"

	"github.com/expr-lang/expr"
	"github.com/expr-lang/expr/vm"
)

// UDF is a named, host-defined function compiled from an expression string. It is
// a sealed interface — only *BlUDF[P, R] implements it — so a heterogeneous set of
// UDFs (different parameter and return types) can be passed together to Expr or as
// dependencies to another Func.
type UDF interface {
	udfName() string
	udfSource() string
	udfParams() []string
	udfOption() expr.Option
}

// BlUDF is a named user-defined function: an expression body compiled once against
// a parameter struct P, callable from other expressions by name with compile-time-
// checked arguments, and returning a value of type R.
type BlUDF[P any, R BlValue] struct {
	name    string
	body    string
	program *vm.Program  // body compiled once; reused on every call
	pType   reflect.Type // P's reflect type
	bind    []int        // positional parameter index -> P field index
	option  expr.Option  // the expr.Function registration carrying impl + prototype
}

// Func compiles a UDF from an expression body. P is the parameter struct: its
// exported fields, in declaration order, are the positional parameters, and each
// field's `expr:"name"` tag is the name the body uses for that parameter (each
// field's type must implement BlValue). R is the return type. deps are other UDFs
// the body may call by name.
//
// Func calls expr.Compile once to compile the body to a stored *vm.Program, and
// builds an expr.Function registration (with a reflect-built prototype derived
// from P's fields and R) that makes the UDF callable from other expressions. The
// body is never recompiled — every call runs the stored program via expr.Run.
func Func[P any, R BlValue](name, body string, deps ...UDF) (*BlUDF[P, R], error) {
	if name == "" {
		return nil, &ParseError{Source: body, Err: errors.New("empty function name")}
	}
	pType := reflect.TypeOf((*P)(nil)).Elem()
	if pType.Kind() != reflect.Struct {
		return nil, &ParseError{Source: body, Err: fmt.Errorf("parameter type %s is not a struct", pType)}
	}

	// Parameters are P's exported, expr-tagged fields in declaration order.
	blValue := reflect.TypeOf((*BlValue)(nil)).Elem()
	var paramTypes []reflect.Type
	var bind []int
	for i := 0; i < pType.NumField(); i++ {
		f := pType.Field(i)
		if !f.IsExported() {
			continue
		}
		if _, ok := exprTagName(f); !ok {
			continue
		}
		if !f.Type.Implements(blValue) {
			return nil, &ParseError{Source: body, Err: fmt.Errorf("parameter %s: type %s does not implement BlValue", f.Name, f.Type)}
		}
		paramTypes = append(paramTypes, f.Type)
		bind = append(bind, i)
	}

	// Compile the body once against P, with the dependency UDFs registered so the
	// body may call them (expr.Compile is invoked inside compileWithEnv).
	depOpts, err := udfOptions(deps)
	if err != nil {
		return nil, &ParseError{Source: body, Err: err}
	}
	program, err := compileWithEnv(body, *new(P), envFieldNames(pType), depOpts...)
	if err != nil {
		return nil, &ParseError{Source: body, Err: err}
	}

	u := &BlUDF[P, R]{name: name, body: body, program: program, pType: pType, bind: bind}

	// Register impl under name with a reflect-built prototype func(paramTypes...) R,
	// which is what gives call sites their compile-time argument and return typing.
	rType := reflect.TypeOf((*R)(nil)).Elem()
	funcType := reflect.FuncOf(paramTypes, []reflect.Type{rType}, false)
	u.option = expr.Function(name, u.impl, reflect.New(funcType).Interface())
	return u, nil
}

// impl is the expr.Function implementation invoked when the UDF is called from an
// expression: it binds the positional call arguments into a P value and runs the
// stored program.
func (u *BlUDF[P, R]) impl(args ...any) (any, error) {
	if len(args) != len(u.bind) {
		return nil, &TypeError{Op: u.name, Detail: "argument count mismatch"}
	}
	pv := reflect.New(u.pType).Elem()
	for i, fieldIdx := range u.bind {
		if err := setBlField(pv.Field(fieldIdx), asBl(args[i])); err != nil {
			return nil, &TypeError{Op: u.name, Detail: "argument type mismatch"}
		}
	}
	out, err := u.run(pv.Interface())
	if err != nil {
		return nil, err
	}
	return out, nil
}

// Call invokes the UDF host-side with a typed parameter struct, returning the
// typed result. Like a call from inside an expression, it runs the stored program
// via expr.Run.
func (u *BlUDF[P, R]) Call(params P) (R, error) {
	return u.run(params)
}

// run executes the stored program (via expr.Run) against an env value and coerces
// the result to R — a result of the wrong type is a TypeError.
func (u *BlUDF[P, R]) run(env any) (R, error) {
	var out R
	raw, err := expr.Run(u.program, env)
	if err != nil {
		return out, &TypeError{Op: u.name, Detail: err.Error()}
	}
	if err := setBlField(reflect.ValueOf(&out).Elem(), asBl(raw)); err != nil {
		return out, &TypeError{Op: u.name, Detail: "return type mismatch"}
	}
	return out, nil
}

// Name returns the UDF's name.
func (u *BlUDF[P, R]) Name() string { return u.name }

// Source returns the UDF's original body source.
func (u *BlUDF[P, R]) Source() string { return u.body }

func (u *BlUDF[P, R]) udfName() string   { return u.name }
func (u *BlUDF[P, R]) udfSource() string { return u.body }

// udfParams returns the positional parameter names, in call order — each bound
// field's expr name. Used to render the call signature.
func (u *BlUDF[P, R]) udfParams() []string {
	names := make([]string, len(u.bind))
	for i, fieldIdx := range u.bind {
		names[i], _ = exprTagName(u.pType.Field(fieldIdx))
	}
	return names
}

func (u *BlUDF[P, R]) udfOption() expr.Option { return u.option }
