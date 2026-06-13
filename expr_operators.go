package blkit

import "github.com/expr-lang/expr"

// Operator dispatch.
//
// expr's expr.Operator overload resolution is static and cannot match an
// interface-typed operand (e.g. the result of a chained `a + b + c`, which is
// statically BlValue) against a concrete-typed overload such as
// addNumbers(BlNumber, BlNumber). blkit therefore lowers every binary operator
// in the patcher to a call of a single per-operator dispatch function typed
// (BlValue, BlValue) BlValue, switching on the operands' runtime types. This
// preserves the spec's operator semantics while satisfying expr's type checker.
// Each spoke still contributes its own typed impls (addNumbers, ltStrings, …);
// the dispatchers below route to them.

func operatorRegistrations() []expr.Option {
	return []expr.Option{
		expr.Function("__add", opAdd, new(func(BlValue, BlValue) BlValue)),
		expr.Function("__sub", opSub, new(func(BlValue, BlValue) BlValue)),
		expr.Function("__mul", opMul, new(func(BlValue, BlValue) BlValue)),
		expr.Function("__div", opDiv, new(func(BlValue, BlValue) BlValue)),
		expr.Function("__pow", opPow, new(func(BlValue, BlValue) BlValue)),
		expr.Function("__neg", opNeg, new(func(BlValue) BlValue)),
		expr.Function("__lt", opLt, new(func(BlValue, BlValue) BlValue)),
		expr.Function("__le", opLe, new(func(BlValue, BlValue) BlValue)),
		expr.Function("__gt", opGt, new(func(BlValue, BlValue) BlValue)),
		expr.Function("__ge", opGe, new(func(BlValue, BlValue) BlValue)),
		expr.Function("__eq", opEq, new(func(BlValue, BlValue) BlValue)),
		expr.Function("__ne", opNe, new(func(BlValue, BlValue) BlValue)),

		// and/or lowering helpers (emitted by the patcher's conditional form).
		expr.Function("__isFalse", opIsFalse, new(func(BlValue) bool)),
		expr.Function("__isTrue", opIsTrue, new(func(BlValue) bool)),
		expr.Function("__blAnd", opBlAnd, new(func(BlValue, BlValue) BlValue)),
		expr.Function("__blOr", opBlOr, new(func(BlValue, BlValue) BlValue)),

		// identity — used to keep both branches of the and/or conditional
		// lowering uniformly typed as BlValue.
		expr.Function("__val", func(args ...any) (any, error) { return asBl(args[0]), nil },
			new(func(BlValue) BlValue)),

		// __truthy drives expr's native conditional from a BlValue: true only
		// for BlBoolean(true); null / non-boolean is falsy (takes else branch).
		expr.Function("__truthy", opTruthy, new(func(BlValue) bool)),
	}
}

func opTruthy(args ...any) (any, error) {
	if b, ok := asBl(args[0]).(BlBoolean); ok {
		return b.b, nil
	}
	return false, nil
}

func opAdd(args ...any) (any, error) {
	a, b := asBl(args[0]), asBl(args[1])
	if propagatesNull(a, b) {
		return Null(), nil
	}
	switch x := a.(type) {
	case BlNumber:
		if y, ok := b.(BlNumber); ok {
			return addNumbers(x, y), nil
		}
	case BlString:
		if y, ok := b.(BlString); ok {
			return concatStrings(x, y), nil
		}
	case BlDate:
		switch y := b.(type) {
		case BlYearsMonthsDuration:
			return addDateYM(x, y), nil
		case BlDaysTimeDuration:
			return addDateDT(x, y), nil
		}
	case BlDateTime:
		switch y := b.(type) {
		case BlYearsMonthsDuration:
			return addDateTimeYM(x, y), nil
		case BlDaysTimeDuration:
			return addDateTimeDT(x, y), nil
		}
	case BlTime:
		if y, ok := b.(BlDaysTimeDuration); ok {
			return addTimeDur(x, y), nil
		}
	case BlYearsMonthsDuration:
		switch y := b.(type) {
		case BlYearsMonthsDuration:
			return addYMDuration(x, y), nil
		case BlDate:
			return addDateYM(y, x), nil
		case BlDateTime:
			return addDateTimeYM(y, x), nil
		}
	case BlDaysTimeDuration:
		switch y := b.(type) {
		case BlDaysTimeDuration:
			return addDTDuration(x, y), nil
		case BlDate:
			return addDateDT(y, x), nil
		case BlDateTime:
			return addDateTimeDT(y, x), nil
		case BlTime:
			return addTimeDur(y, x), nil
		}
	}
	return nil, &TypeError{Op: "+", Detail: typeName(a.Type()) + " + " + typeName(b.Type())}
}

func opSub(args ...any) (any, error) {
	a, b := asBl(args[0]), asBl(args[1])
	if propagatesNull(a, b) {
		return Null(), nil
	}
	switch x := a.(type) {
	case BlNumber:
		if y, ok := b.(BlNumber); ok {
			return subNumbers(x, y), nil
		}
	case BlDate:
		switch y := b.(type) {
		case BlDate:
			return subDates(x, y), nil
		case BlYearsMonthsDuration:
			return subDateYM(x, y), nil
		case BlDaysTimeDuration:
			return subDateDT(x, y), nil
		}
	case BlDateTime:
		switch y := b.(type) {
		case BlDateTime:
			return subDateTimes(x, y), nil
		case BlYearsMonthsDuration:
			return subDateTimeYM(x, y), nil
		case BlDaysTimeDuration:
			return subDateTimeDT(x, y), nil
		}
	case BlTime:
		if y, ok := b.(BlDaysTimeDuration); ok {
			return subTimeDur(x, y), nil
		}
	case BlYearsMonthsDuration:
		if y, ok := b.(BlYearsMonthsDuration); ok {
			return subYMDuration(x, y), nil
		}
	case BlDaysTimeDuration:
		if y, ok := b.(BlDaysTimeDuration); ok {
			return subDTDuration(x, y), nil
		}
	}
	return nil, &TypeError{Op: "-", Detail: typeName(a.Type()) + " - " + typeName(b.Type())}
}

func opMul(args ...any) (any, error) {
	a, b := asBl(args[0]), asBl(args[1])
	if propagatesNull(a, b) {
		return Null(), nil
	}
	switch x := a.(type) {
	case BlNumber:
		switch y := b.(type) {
		case BlNumber:
			return mulNumbers(x, y), nil
		case BlDaysTimeDuration:
			return scaleDTDuration(y, x), nil
		case BlYearsMonthsDuration:
			return scaleYMDuration(y, x), nil
		}
	case BlDaysTimeDuration:
		if y, ok := b.(BlNumber); ok {
			return scaleDTDuration(x, y), nil
		}
	case BlYearsMonthsDuration:
		if y, ok := b.(BlNumber); ok {
			return scaleYMDuration(x, y), nil
		}
	}
	return nil, &TypeError{Op: "*", Detail: typeName(a.Type()) + " * " + typeName(b.Type())}
}

func opDiv(args ...any) (any, error) {
	a, b := asBl(args[0]), asBl(args[1])
	if propagatesNull(a, b) {
		return Null(), nil
	}
	switch x := a.(type) {
	case BlNumber:
		if y, ok := b.(BlNumber); ok {
			return divNumbers(x, y), nil
		}
	case BlDaysTimeDuration:
		if y, ok := b.(BlNumber); ok {
			return divDTDuration(x, y), nil
		}
	case BlYearsMonthsDuration:
		if y, ok := b.(BlNumber); ok {
			return divYMDuration(x, y), nil
		}
	}
	return nil, &TypeError{Op: "/", Detail: typeName(a.Type()) + " / " + typeName(b.Type())}
}

func opPow(args ...any) (any, error) {
	a, b := asBl(args[0]), asBl(args[1])
	if propagatesNull(a, b) {
		return Null(), nil
	}
	if x, ok := a.(BlNumber); ok {
		if y, ok := b.(BlNumber); ok {
			return powNumber(x, y), nil
		}
	}
	return nil, &TypeError{Op: "**", Detail: typeName(a.Type()) + " ** " + typeName(b.Type())}
}

func opNeg(args ...any) (any, error) {
	a := asBl(args[0])
	if a.IsNull() {
		return Null(), nil
	}
	switch x := a.(type) {
	case BlNumber:
		return negNumber(x), nil
	case BlDaysTimeDuration:
		return negDTDuration(x), nil
	case BlYearsMonthsDuration:
		return negYMDuration(x), nil
	}
	return nil, &TypeError{Op: "unary -", Detail: typeName(a.Type())}
}

// ordering dispatch — null propagates; incomparable operands yield null.
func opOrder(op string, a, b BlValue, pick func(int) bool) (any, error) {
	if propagatesNull(a, b) {
		return Null(), nil
	}
	c, ok := compareValues(a, b)
	if !ok {
		return Null(), nil
	}
	return BlBoolean{pick(c)}, nil
}

func opLt(args ...any) (any, error) {
	return opOrder("<", asBl(args[0]), asBl(args[1]), func(c int) bool { return c < 0 })
}
func opLe(args ...any) (any, error) {
	return opOrder("<=", asBl(args[0]), asBl(args[1]), func(c int) bool { return c <= 0 })
}
func opGt(args ...any) (any, error) {
	return opOrder(">", asBl(args[0]), asBl(args[1]), func(c int) bool { return c > 0 })
}
func opGe(args ...any) (any, error) {
	return opOrder(">=", asBl(args[0]), asBl(args[1]), func(c int) bool { return c >= 0 })
}

// compareValues returns (-1,0,1) and ok=true for comparable same-domain
// operands; ok=false when the operands are incomparable.
func compareValues(a, b BlValue) (int, bool) {
	switch x := a.(type) {
	case BlNumber:
		if y, ok := b.(BlNumber); ok {
			return x.d.Cmp(y.d), true
		}
	case BlString:
		if y, ok := b.(BlString); ok {
			switch {
			case x.s < y.s:
				return -1, true
			case x.s > y.s:
				return 1, true
			default:
				return 0, true
			}
		}
	case BlDate:
		if y, ok := b.(BlDate); ok && x.naive == y.naive {
			return x.t.Compare(y.t), true
		}
	case BlDateTime:
		if y, ok := b.(BlDateTime); ok && x.naive == y.naive {
			return x.t.Compare(y.t), true
		}
	case BlTime:
		if y, ok := b.(BlTime); ok && x.naive == y.naive {
			return x.t.Compare(y.t), true
		}
	case BlDaysTimeDuration:
		if y, ok := b.(BlDaysTimeDuration); ok {
			return x.secs.Cmp(y.secs), true
		}
	case BlYearsMonthsDuration:
		if y, ok := b.(BlYearsMonthsDuration); ok {
			return x.months.Cmp(y.months), true
		}
	}
	return 0, false
}

// equality — = / != never yield null; a null operand makes both false.
func opEq(args ...any) (any, error) {
	a, b := asBl(args[0]), asBl(args[1])
	if a.IsNull() || b.IsNull() {
		return BlBoolean{false}, nil
	}
	return a.Equal(b), nil
}

func opNe(args ...any) (any, error) {
	a, b := asBl(args[0]), asBl(args[1])
	if a.IsNull() || b.IsNull() {
		return BlBoolean{false}, nil
	}
	eq := a.Equal(b)
	if bb, ok := eq.(BlBoolean); ok {
		return BlBoolean{!bb.b}, nil
	}
	return BlBoolean{false}, nil
}

// and/or helpers. opIsFalse / opIsTrue return Go bools so they can drive expr's
// native ConditionalNode condition; the patcher uses them as the short-circuit
// guard. opBlAnd / opBlOr complete the three-valued table for the non-shorted
// cases.
func opIsFalse(args ...any) (any, error) {
	if b, ok := asBl(args[0]).(BlBoolean); ok {
		return !b.b, nil
	}
	return false, nil
}

func opIsTrue(args ...any) (any, error) {
	if b, ok := asBl(args[0]).(BlBoolean); ok {
		return b.b, nil
	}
	return false, nil
}

func opBlAnd(args ...any) (any, error) { return blAndFn(asBl(args[0]), asBl(args[1])), nil }
func opBlOr(args ...any) (any, error)  { return blOrFn(asBl(args[0]), asBl(args[1])), nil }
