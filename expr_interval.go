package blkit

import "github.com/expr-lang/expr"

// Interval algebra over ranges and points. A point p is treated as the closed
// interval [p, p]. When any range argument is empty, the predicates return
// BlNull (see range.spec.md § Empty-range semantics).

type interval struct {
	lo, hi       BlValue // BlNull for unbounded
	loInc, hiInc bool
	empty        bool
}

func toInterval(v BlValue) (interval, bool) {
	if r, ok := v.(BlRange); ok {
		return interval{lo: r.start, hi: r.end, loInc: r.startIncluded, hiInc: r.endIncluded, empty: r.isEmptyRange()}, true
	}
	if v.IsNull() {
		return interval{}, false
	}
	return interval{lo: v, hi: v, loInc: true, hiInc: true}, true
}

// cmpBound compares two non-null bound values; ok=false if incomparable.
func cmpBound(a, b BlValue) (int, bool) {
	return compareValues(a, b)
}

// loBeforeLo reports whether interval a's lower bound is strictly left of b's
// lower bound, accounting for unbounded ends and inclusion.
func loStrictlyLeft(a, b interval) bool {
	if a.lo.IsNull() {
		return !b.lo.IsNull()
	}
	if b.lo.IsNull() {
		return false
	}
	c, ok := cmpBound(a.lo, b.lo)
	if !ok {
		return false
	}
	if c != 0 {
		return c < 0
	}
	return a.loInc && !b.loInc
}

func loEqual(a, b interval) bool {
	if a.lo.IsNull() || b.lo.IsNull() {
		return a.lo.IsNull() && b.lo.IsNull()
	}
	c, ok := cmpBound(a.lo, b.lo)
	return ok && c == 0 && a.loInc == b.loInc
}

func hiEqual(a, b interval) bool {
	if a.hi.IsNull() || b.hi.IsNull() {
		return a.hi.IsNull() && b.hi.IsNull()
	}
	c, ok := cmpBound(a.hi, b.hi)
	return ok && c == 0 && a.hiInc == b.hiInc
}

// hiBeforeLo reports whether a is entirely left of b (a.hi before b.lo), i.e.
// the `before` relation.
func hiBeforeLo(a, b interval) bool {
	if a.hi.IsNull() || b.lo.IsNull() {
		return false // unbounded toward each other → touch/overlap
	}
	c, ok := cmpBound(a.hi, b.lo)
	if !ok {
		return false
	}
	if c < 0 {
		return true
	}
	if c == 0 {
		return !(a.hiInc && b.loInc)
	}
	return false
}

// hiMeetsLo reports whether a.hi == b.lo and both endpoints are included.
func hiMeetsLo(a, b interval) bool {
	if a.hi.IsNull() || b.lo.IsNull() {
		return false
	}
	c, ok := cmpBound(a.hi, b.lo)
	return ok && c == 0 && a.hiInc && b.loInc
}

func hiWithinHi(a, b interval) bool {
	// a.hi <= b.hi
	if b.hi.IsNull() {
		return true
	}
	if a.hi.IsNull() {
		return false
	}
	c, ok := cmpBound(a.hi, b.hi)
	if !ok {
		return false
	}
	if c != 0 {
		return c < 0
	}
	return !a.hiInc || b.hiInc
}

// intervalsOverlap reports a non-empty intersection.
func intervalsOverlap(a, b interval) bool {
	return !hiBeforeLo(a, b) && !hiBeforeLo(b, a)
}

// binPred wraps a two-argument interval predicate with the shared null/empty
// handling: null operand → null; empty range operand → null.
func binPred(args []any, f func(a, b interval) bool) (any, error) {
	av, bv := asBl(args[0]), asBl(args[1])
	if av.IsNull() || bv.IsNull() {
		return Null(), nil
	}
	a, ok1 := toInterval(av)
	b, ok2 := toInterval(bv)
	if !ok1 || !ok2 {
		return Null(), nil
	}
	if a.empty || b.empty {
		return Null(), nil
	}
	return BlBoolean{f(a, b)}, nil
}

func beforeFn(args ...any) (any, error) { return binPred(args, hiBeforeLo) }
func afterFn(args ...any) (any, error) {
	return binPred(args, func(a, b interval) bool { return hiBeforeLo(b, a) })
}
func meetsFn(args ...any) (any, error) { return binPred(args, hiMeetsLo) }
func metByFn(args ...any) (any, error) {
	return binPred(args, func(a, b interval) bool { return hiMeetsLo(b, a) })
}
func overlapsFn(args ...any) (any, error) {
	// calendar overlaps(c, range) overload
	if cal, ok := asBl(args[0]).(BlCalendar); ok {
		r, ok := asBl(args[1]).(BlRange)
		if !ok {
			return nil, argTypeError(args[1])
		}
		return calOverlapsFn(cal, r), nil
	}
	return binPred(args, intervalsOverlap)
}
func overlapsBeforeFn(args ...any) (any, error) {
	return binPred(args, func(a, b interval) bool { return intervalsOverlap(a, b) && loStrictlyLeft(a, b) })
}
func overlapsAfterFn(args ...any) (any, error) {
	return binPred(args, func(a, b interval) bool { return intervalsOverlap(a, b) && loStrictlyLeft(b, a) })
}
func startsFn(args ...any) (any, error) {
	return binPred(args, func(a, b interval) bool { return loEqual(a, b) && hiWithinHi(a, b) })
}
func startedByFn(args ...any) (any, error) {
	return binPred(args, func(a, b interval) bool { return loEqual(a, b) && hiWithinHi(b, a) })
}
func finishesFn(args ...any) (any, error) {
	// a finishes b: same end, a's start at or after b's start.
	return binPred(args, func(a, b interval) bool { return hiEqual(a, b) && !loStrictlyLeft(a, b) })
}
func finishedByFn(args ...any) (any, error) {
	return binPred(args, func(a, b interval) bool { return hiEqual(a, b) && !loStrictlyLeft(b, a) })
}
func coincidesFn(args ...any) (any, error) {
	return binPred(args, func(a, b interval) bool { return loEqual(a, b) && hiEqual(a, b) })
}

func intervalOptions() []expr.Option {
	return []expr.Option{
		expr.Function("before", beforeFn, new(func(BlValue, BlValue) BlValue)),
		expr.Function("after", afterFn, new(func(BlValue, BlValue) BlValue)),
		expr.Function("meets", meetsFn, new(func(BlValue, BlValue) BlValue)),
		expr.Function("metBy", metByFn, new(func(BlValue, BlValue) BlValue)),
		expr.Function("overlaps", overlapsFn, new(func(BlValue, BlValue) BlValue)),
		expr.Function("overlapsBefore", overlapsBeforeFn, new(func(BlValue, BlValue) BlValue)),
		expr.Function("overlapsAfter", overlapsAfterFn, new(func(BlValue, BlValue) BlValue)),
		expr.Function("starts", startsFn, new(func(BlValue, BlValue) BlValue)),
		expr.Function("startedBy", startedByFn, new(func(BlValue, BlValue) BlValue)),
		expr.Function("finishes", finishesFn, new(func(BlValue, BlValue) BlValue)),
		expr.Function("finishedBy", finishedByFn, new(func(BlValue, BlValue) BlValue)),
		expr.Function("coincides", coincidesFn, new(func(BlValue, BlValue) BlValue)),
	}
}
