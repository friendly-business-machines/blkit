package blkit

import (
	"github.com/expr-lang/expr"
)

// BlRange is a contiguous interval of comparable values with configurable
// endpoint inclusion. Either endpoint may be BlNull (unbounded).
type BlRange struct {
	start, end                 BlValue
	startIncluded, endIncluded bool
}

func (BlRange) Type() Type { return TypeRange }

func (r BlRange) Equal(other BlValue) BlValue {
	o, ok := other.(BlRange)
	if !ok {
		return BlBoolean{false}
	}
	se := r.start.Equal(o.start)
	ee := r.end.Equal(o.end)
	startEq := r.start.IsNull() && o.start.IsNull() || eqTrue(se)
	endEq := r.end.IsNull() && o.end.IsNull() || eqTrue(ee)
	return BlBoolean{startEq && endEq && r.startIncluded == o.startIncluded && r.endIncluded == o.endIncluded}
}

func (r BlRange) String() string {
	open, close := "[", "]"
	if !r.startIncluded {
		open = "("
	}
	if !r.endIncluded {
		close = ")"
	}
	return open + r.start.String() + ".." + r.end.String() + close
}

func (BlRange) IsNull() bool { return false }

func (BlRange) isBlValue() {}

// Start / End / StartIncluded / EndIncluded host accessors.
func (r BlRange) Start() BlValue      { return r.start }
func (r BlRange) End() BlValue        { return r.end }
func (r BlRange) StartIncluded() bool { return r.startIncluded }
func (r BlRange) EndIncluded() bool   { return r.endIncluded }

// Range constructs a BlRange. Closed boundary with a null endpoint, or
// cross-type endpoints, returns an error.
func Range(start, end BlValue, startIncluded, endIncluded bool) (BlRange, error) {
	if start.IsNull() && startIncluded {
		return BlRange{}, &TypeError{Op: "Range", Detail: "closed boundary on unbounded start"}
	}
	if end.IsNull() && endIncluded {
		return BlRange{}, &TypeError{Op: "Range", Detail: "closed boundary on unbounded end"}
	}
	if !start.IsNull() && !end.IsNull() {
		if _, ok := compareValues(start, end); !ok {
			return BlRange{}, &TypeError{Op: "Range", Detail: "cross-type or non-comparable endpoints"}
		}
	}
	return BlRange{start: start, end: end, startIncluded: startIncluded, endIncluded: endIncluded}, nil
}

// isEmptyRange reports whether the range contains no values.
func (r BlRange) isEmptyRange() bool {
	if r.start.IsNull() || r.end.IsNull() {
		return false
	}
	c, ok := compareValues(r.start, r.end)
	if !ok {
		return false
	}
	if c > 0 {
		return true // reversed
	}
	if c == 0 && !(r.startIncluded && r.endIncluded) {
		return true // degenerate exclusive
	}
	return false
}

// contains reports point membership; ok=false on incomparable/empty.
func (r BlRange) contains(point BlValue) (bool, bool) {
	if r.isEmptyRange() {
		return false, false
	}
	if !r.start.IsNull() {
		c, ok := compareValues(point, r.start)
		if !ok {
			return false, false
		}
		if c < 0 || (c == 0 && !r.startIncluded) {
			return false, true
		}
	}
	if !r.end.IsNull() {
		c, ok := compareValues(point, r.end)
		if !ok {
			return false, false
		}
		if c > 0 || (c == 0 && !r.endIncluded) {
			return false, true
		}
	}
	return true, true
}

// newRangeFn is the patcher/host target for a range literal.
func newRangeFn(args ...any) (any, error) {
	if len(args) != 4 {
		return nil, &TypeError{Op: "newRange", Detail: "expected 4 args"}
	}
	si, _ := args[2].(BlBoolean)
	ei, _ := args[3].(BlBoolean)
	r, err := Range(asBl(args[0]), asBl(args[1]), si.b, ei.b)
	if err != nil {
		return nil, err
	}
	return r, nil
}

func rangeComponent(r BlRange, name string) (BlValue, bool) {
	switch name {
	case "start":
		return r.start, true
	case "end":
		return r.end, true
	case "startIncluded":
		return BlBoolean{r.startIncluded}, true
	case "endIncluded":
		return BlBoolean{r.endIncluded}, true
	}
	return nil, false
}

// --- interval algebra -----------------------------------------------------

func includesFn(args ...any) (any, error) {
	r, ok := args[0].(BlRange)
	if !ok {
		return nil, argTypeError(args[0])
	}
	in, ok := r.contains(asBl(args[1]))
	if !ok {
		return Null(), nil
	}
	return BlBoolean{in}, nil
}

func duringFn(args ...any) (any, error) {
	r, ok := args[1].(BlRange)
	if !ok {
		return nil, argTypeError(args[1])
	}
	in, ok := r.contains(asBl(args[0]))
	if !ok {
		return Null(), nil
	}
	return BlBoolean{in}, nil
}

func rangeIsEmptyFn(r BlRange) BlBoolean { return BlBoolean{r.isEmptyRange()} }

func rangeOptions() []expr.Option {
	return []expr.Option{
		expr.Function("newRange", newRangeFn, new(func(BlValue, BlValue, BlValue, BlValue) BlRange)),
		expr.Function("includes", includesFn, new(func(BlValue, BlValue) BlValue)),
		expr.Function("during", duringFn, new(func(BlValue, BlValue) BlValue)),
		// isEmpty is a unified cross-type dispatcher in expr_overloads.go.
	}
}
