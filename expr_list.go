package blkit

import (
	"sort"
	"strings"

	"github.com/expr-lang/expr"
	"github.com/shopspring/decimal"
)

// BlList is an ordered, immutable, heterogeneous sequence of BlValues.
// Indexing in the expression language is 1-based.
type BlList struct{ items []BlValue }

func (BlList) Type() Type { return TypeList }

func (l BlList) Equal(other BlValue) BlValue {
	o, ok := other.(BlList)
	if !ok || len(l.items) != len(o.items) {
		return BlBoolean{false}
	}
	for i := range l.items {
		if eq, ok := l.items[i].Equal(o.items[i]).(BlBoolean); !ok || !eq.b {
			return BlBoolean{false}
		}
	}
	return BlBoolean{true}
}

func (l BlList) String() string {
	parts := make([]string, len(l.items))
	for i, e := range l.items {
		parts[i] = literalString(e)
	}
	return "[" + strings.Join(parts, ", ") + "]"
}

func (BlList) IsNull() bool { return false }

func (BlList) isBlValue() {}

// Native returns a defensive copy of the underlying elements.
func (l BlList) Native() []BlValue {
	out := make([]BlValue, len(l.items))
	copy(out, l.items)
	return out
}

// List constructs a BlList from the supplied values.
func List(items ...BlValue) BlList {
	cp := make([]BlValue, len(items))
	copy(cp, items)
	return BlList{cp}
}

func listOfStrings(ss []string) BlList {
	items := make([]BlValue, len(ss))
	for i, s := range ss {
		items[i] = BlString{s}
	}
	return BlList{items}
}

func sortLongestFirst(xs []string) {
	sort.SliceStable(xs, func(i, j int) bool { return len(xs[i]) > len(xs[j]) })
}

// literalString renders a BlValue in canonical literal form for embedding in a
// list/dictionary rendering: strings are quoted, everything else uses String().
func literalString(v BlValue) string {
	if s, ok := v.(BlString); ok {
		return quoteString(s.s)
	}
	return v.String()
}

func quoteString(s string) string {
	var b strings.Builder
	b.WriteByte('"')
	for _, r := range s {
		switch r {
		case '"':
			b.WriteString(`\"`)
		case '\\':
			b.WriteString(`\\`)
		case '\n':
			b.WriteString(`\n`)
		case '\r':
			b.WriteString(`\r`)
		case '\t':
			b.WriteString(`\t`)
		default:
			b.WriteRune(r)
		}
	}
	b.WriteByte('"')
	return b.String()
}

// --- core list functions --------------------------------------------------

func countFn(l BlList) BlNumber        { return numFromInt(len(l.items)) }
func listIsEmptyFn(l BlList) BlBoolean { return BlBoolean{len(l.items) == 0} }

func listContainsFn(l BlList, e BlValue) BlBoolean {
	for _, x := range l.items {
		if eqTrue(x.Equal(e)) {
			return BlBoolean{true}
		}
	}
	return BlBoolean{false}
}

func eqTrue(v BlValue) bool {
	b, ok := v.(BlBoolean)
	return ok && b.b
}

func listIndexOfFn(l BlList, match BlValue) BlList {
	var positions []BlValue
	for i, x := range l.items {
		if eqTrue(x.Equal(match)) {
			positions = append(positions, numFromInt(i+1))
		}
	}
	return BlList{positions}
}

func listReverseFn(l BlList) BlList {
	out := make([]BlValue, len(l.items))
	for i, e := range l.items {
		out[len(l.items)-1-i] = e
	}
	return BlList{out}
}

func flattenFn(l BlList) BlList {
	var out []BlValue
	var rec func(items []BlValue)
	rec = func(items []BlValue) {
		for _, e := range items {
			if inner, ok := e.(BlList); ok {
				rec(inner.items)
			} else {
				out = append(out, e)
			}
		}
	}
	rec(l.items)
	return BlList{out}
}

func distinctFn(l BlList) BlList {
	var out []BlValue
	for _, e := range l.items {
		if !containsValue(out, e) {
			out = append(out, e)
		}
	}
	return BlList{out}
}

func duplicateValuesFn(l BlList) BlList {
	var out []BlValue
	for i, e := range l.items {
		// appears again later and not already recorded
		if containsValue(l.items[i+1:], e) && !containsValue(out, e) {
			out = append(out, e)
		}
	}
	return BlList{out}
}

func containsValue(items []BlValue, e BlValue) bool {
	for _, x := range items {
		if eqTrue(x.Equal(e)) {
			return true
		}
	}
	return false
}

func sortFn(args ...any) (any, error) {
	l, ok := args[0].(BlList)
	if !ok {
		return nil, argTypeError(args[0])
	}
	items := make([]BlValue, len(l.items))
	copy(items, l.items)
	desc := false
	var explicit []BlValue
	if len(args) > 1 {
		switch o := args[1].(type) {
		case BlString:
			switch o.s {
			case "asc":
			case "desc":
				desc = true
			default:
				return nil, &TypeError{Op: "sort", Detail: "order must be asc/desc"}
			}
		case BlList:
			explicit = o.items
		default:
			return nil, argTypeError(args[1])
		}
	}
	if explicit != nil {
		rank := func(v BlValue) int {
			for i, e := range explicit {
				if eqTrue(e.Equal(v)) {
					return i
				}
			}
			return len(explicit)
		}
		sort.SliceStable(items, func(i, j int) bool {
			ri, rj := rank(items[i]), rank(items[j])
			if ri != rj {
				return ri < rj
			}
			c, ok := compareValues(items[i], items[j])
			return ok && c < 0
		})
		return BlList{items}, nil
	}
	var sortErr error
	sort.SliceStable(items, func(i, j int) bool {
		c, ok := compareValues(items[i], items[j])
		if !ok {
			sortErr = &TypeError{Op: "sort", Detail: "non-comparable elements"}
			return false
		}
		if desc {
			return c > 0
		}
		return c < 0
	})
	if sortErr != nil {
		return nil, sortErr
	}
	return BlList{items}, nil
}

// --- variadic list functions ----------------------------------------------

func sublistFn(args ...any) (any, error) {
	l, ok := args[0].(BlList)
	if !ok {
		return nil, argTypeError(args[0])
	}
	start, ok := args[1].(BlNumber)
	if !ok {
		return nil, argTypeError(args[1])
	}
	n := len(l.items)
	st := int(start.d.IntPart())
	if st < 0 {
		st = n + st + 1
	}
	if st < 1 {
		st = 1
	}
	if st > n {
		return BlList{}, nil
	}
	end := n
	if len(args) > 2 {
		length, ok := args[2].(BlNumber)
		if !ok {
			return nil, argTypeError(args[2])
		}
		end = st - 1 + int(length.d.IntPart())
		if end > n {
			end = n
		}
	}
	if end < st-1 {
		end = st - 1
	}
	return BlList{append([]BlValue{}, l.items[st-1:end]...)}, nil
}

func appendFn(args ...any) (any, error) {
	l, ok := args[0].(BlList)
	if !ok {
		return nil, argTypeError(args[0])
	}
	out := append([]BlValue{}, l.items...)
	for _, a := range args[1:] {
		out = append(out, asBl(a))
	}
	return BlList{out}, nil
}

func prependFn(args ...any) (any, error) {
	l, ok := args[0].(BlList)
	if !ok {
		return nil, argTypeError(args[0])
	}
	var out []BlValue
	for _, a := range args[1:] {
		out = append(out, asBl(a))
	}
	out = append(out, l.items...)
	return BlList{out}, nil
}

func concatenateFn(args ...any) (any, error) {
	var out []BlValue
	for _, a := range args {
		l, ok := a.(BlList)
		if !ok {
			return nil, argTypeError(a)
		}
		out = append(out, l.items...)
	}
	return BlList{out}, nil
}

func unionFn(args ...any) (any, error) {
	var out []BlValue
	for _, a := range args {
		l, ok := a.(BlList)
		if !ok {
			return nil, argTypeError(a)
		}
		for _, e := range l.items {
			if !containsValue(out, e) {
				out = append(out, e)
			}
		}
	}
	return BlList{out}, nil
}

func intersectionFn(args ...any) (any, error) {
	if len(args) == 0 {
		return BlList{}, nil
	}
	first, ok := args[0].(BlList)
	if !ok {
		return nil, argTypeError(args[0])
	}
	var out []BlValue
	for _, e := range first.items {
		inAll := true
		for _, a := range args[1:] {
			l, ok := a.(BlList)
			if !ok {
				return nil, argTypeError(a)
			}
			if !containsValue(l.items, e) {
				inAll = false
				break
			}
		}
		if inAll && !containsValue(out, e) {
			out = append(out, e)
		}
	}
	return BlList{out}, nil
}

func removeFn(args ...any) (any, error) {
	l, ok := args[0].(BlList)
	if !ok {
		return nil, argTypeError(args[0])
	}
	pos, ok := args[1].(BlNumber)
	if !ok {
		return nil, argTypeError(args[1])
	}
	p := int(pos.d.IntPart())
	if p < 1 || p > len(l.items) {
		return l, nil
	}
	out := append([]BlValue{}, l.items[:p-1]...)
	out = append(out, l.items[p:]...)
	return BlList{out}, nil
}

func listReplaceFn(args ...any) (any, error) {
	l, ok := args[0].(BlList)
	if !ok {
		return nil, argTypeError(args[0])
	}
	pos, ok := args[1].(BlNumber)
	if !ok {
		return nil, argTypeError(args[1])
	}
	p := int(pos.d.IntPart())
	if p < 1 || p > len(l.items) {
		return l, nil
	}
	out := append([]BlValue{}, l.items...)
	out[p-1] = asBl(args[2])
	return BlList{out}, nil
}

func insertBeforeFn(args ...any) (any, error) { return insertAt(args, 0) }
func insertAfterFn(args ...any) (any, error)  { return insertAt(args, 1) }

func insertAt(args []any, offset int) (any, error) {
	l, ok := args[0].(BlList)
	if !ok {
		return nil, argTypeError(args[0])
	}
	pos, ok := args[1].(BlNumber)
	if !ok {
		return nil, argTypeError(args[1])
	}
	p := int(pos.d.IntPart()) - 1 + offset
	if p < 0 || p > len(l.items) {
		return nil, &TypeError{Op: "insert", Detail: "position out of range"}
	}
	var ins []BlValue
	if sub, ok := args[2].(BlList); ok {
		ins = sub.items
	} else {
		ins = []BlValue{asBl(args[2])}
	}
	out := append([]BlValue{}, l.items[:p]...)
	out = append(out, ins...)
	out = append(out, l.items[p:]...)
	return BlList{out}, nil
}

func stringJoinFn(args ...any) (any, error) {
	l, ok := args[0].(BlList)
	if !ok {
		return nil, argTypeError(args[0])
	}
	sep, prefix, suffix := "", "", ""
	if len(args) >= 2 {
		s, ok := args[1].(BlString)
		if !ok {
			return nil, argTypeError(args[1])
		}
		sep = s.s
	}
	if len(args) == 4 {
		pre, ok1 := args[2].(BlString)
		suf, ok2 := args[3].(BlString)
		if !ok1 || !ok2 {
			return nil, &TypeError{Op: "stringJoin", Detail: "prefix/suffix must be strings"}
		}
		prefix, suffix = pre.s, suf.s
	}
	parts := make([]string, 0, len(l.items))
	for _, e := range l.items {
		s, ok := e.(BlString)
		if !ok {
			return nil, &TypeError{Op: "stringJoin", Detail: "elements must be strings"}
		}
		parts = append(parts, s.s)
	}
	return str(prefix + strings.Join(parts, sep) + suffix), nil
}

func seqFn(args ...any) (any, error) {
	start, ok1 := args[0].(BlNumber)
	end, ok2 := args[1].(BlNumber)
	if !ok1 || !ok2 {
		return nil, &TypeError{Op: "seq", Detail: "start/end must be numbers"}
	}
	step := decimal.NewFromInt(1)
	if len(args) > 2 {
		s, ok := args[2].(BlNumber)
		if !ok {
			return nil, argTypeError(args[2])
		}
		if s.d.IsZero() {
			return nil, &TypeError{Op: "seq", Detail: "step must be non-zero"}
		}
		step = s.d.Abs()
	}
	asc := start.d.Cmp(end.d) <= 0
	var out []BlValue
	cur := start.d
	for !asc || cur.Cmp(end.d) <= 0 {

		if !asc && cur.Cmp(end.d) < 0 {
			break
		}
		out = append(out, num(cur))
		if asc {
			cur = cur.Add(step)
		} else {
			cur = cur.Sub(step)
		}
	}
	return BlList{out}, nil
}

// --- aggregation ----------------------------------------------------------

func nonNull(items []BlValue) []BlValue {
	var out []BlValue
	for _, e := range items {
		if !e.IsNull() {
			out = append(out, e)
		}
	}
	return out
}

func sumFn(l BlList) (BlValue, error) {
	items := nonNull(l.items)
	if len(items) == 0 {
		return Null(), nil
	}
	switch items[0].(type) {
	case BlNumber:
		acc := decimal.Zero
		for _, e := range items {
			n, ok := e.(BlNumber)
			if !ok {
				return nil, &TypeError{Op: "sum", Detail: "list mixes numbers with other types"}
			}
			acc = acc.Add(n.d)
		}
		return num(acc), nil
	case BlDaysTimeDuration:
		acc := decimal.Zero
		for _, e := range items {
			d, ok := e.(BlDaysTimeDuration)
			if !ok {
				return nil, &TypeError{Op: "sum", Detail: "list mixes duration kinds or other types"}
			}
			acc = acc.Add(d.secs)
		}
		return dtDur(acc), nil
	case BlYearsMonthsDuration:
		acc := decimal.Zero
		for _, e := range items {
			d, ok := e.(BlYearsMonthsDuration)
			if !ok {
				return nil, &TypeError{Op: "sum", Detail: "list mixes duration kinds or other types"}
			}
			acc = acc.Add(d.months)
		}
		return ymDur(acc), nil
	}
	return nil, &TypeError{Op: "sum", Detail: "sum is defined for numbers and durations only"}
}

func productFn(l BlList) (BlValue, error) {
	items := nonNull(l.items)
	if len(items) == 0 {
		return Null(), nil
	}
	acc := decimal.NewFromInt(1)
	for _, e := range items {
		n, ok := e.(BlNumber)
		if !ok {
			return nil, &TypeError{Op: "product", Detail: "product is defined for numbers only"}
		}
		acc = acc.Mul(n.d)
	}
	return num(acc), nil
}

func minFn(l BlList) (BlValue, error) { return extremum(l, true) }
func maxFn(l BlList) (BlValue, error) { return extremum(l, false) }

func extremum(l BlList, wantMin bool) (BlValue, error) {
	items := nonNull(l.items)
	if len(items) == 0 {
		return Null(), nil
	}
	best := items[0]
	for _, e := range items[1:] {
		c, ok := compareValues(e, best)
		if !ok {
			return nil, &TypeError{Op: "min/max", Detail: "list mixes incomparable element types"}
		}
		if (wantMin && c < 0) || (!wantMin && c > 0) {
			best = e
		}
	}
	return best, nil
}

func meanFn(l BlList) (BlValue, error) {
	items := nonNull(l.items)
	if len(items) == 0 {
		return Null(), nil
	}
	s, err := sumFn(l)
	if err != nil {
		return nil, err
	}
	n := decimal.NewFromInt(int64(len(items)))
	switch v := s.(type) {
	case BlNumber:
		return num(v.d.DivRound(n, numericPrecision)), nil
	case BlDaysTimeDuration:
		return dtDur(v.secs.DivRound(n, numericPrecision)), nil
	case BlYearsMonthsDuration:
		return ymDur(v.months.DivRound(n, numericPrecision)), nil
	}
	return Null(), nil
}

func medianFn(l BlList) (BlValue, error) {
	items := nonNull(l.items)
	if len(items) == 0 {
		return Null(), nil
	}
	sorted, err := sortFn(BlList{items})
	if err != nil {
		return nil, err
	}
	si := sorted.(BlList).items
	n := len(si)
	if n%2 == 1 {
		return si[n/2], nil
	}
	a, b := si[n/2-1], si[n/2]
	return meanFn(BlList{[]BlValue{a, b}})
}

func stddevFn(l BlList) (BlValue, error) {
	items := nonNull(l.items)
	if len(items) < 2 {
		return Null(), nil
	}
	m, err := meanFn(l)
	if err != nil {
		return nil, err
	}
	mean, ok := m.(BlNumber)
	if !ok {
		return nil, &TypeError{Op: "stddev", Detail: "stddev is defined for numbers only"}
	}
	acc := decimal.Zero
	for _, e := range items {
		n, ok := e.(BlNumber)
		if !ok {
			return nil, &TypeError{Op: "stddev", Detail: "stddev is defined for numbers only"}
		}
		diff := n.d.Sub(mean.d)
		acc = acc.Add(diff.Mul(diff))
	}
	variance := acc.DivRound(decimal.NewFromInt(int64(len(items)-1)), numericPrecision)
	return num(decimalSqrt(variance)), nil
}

func modeFn(l BlList) BlList {
	items := nonNull(l.items)
	type entry struct {
		v     BlValue
		count int
	}
	var entries []*entry
	for _, e := range items {
		found := false
		for _, en := range entries {
			if eqTrue(en.v.Equal(e)) {
				en.count++
				found = true
				break
			}
		}
		if !found {
			entries = append(entries, &entry{v: e, count: 1})
		}
	}
	maxCount := 0
	for _, en := range entries {
		if en.count > maxCount {
			maxCount = en.count
		}
	}
	if maxCount <= 1 {
		return BlList{}
	}
	var out []BlValue
	for _, en := range entries {
		if en.count == maxCount {
			out = append(out, en.v)
		}
	}
	// Return in ascending order when the values are comparable.
	if sorted, err := sortFn(BlList{out}); err == nil {
		return sorted.(BlList)
	}
	return BlList{out}
}

func allFn(l BlList) BlValue { return quantifyList(l, true) }
func anyFn(l BlList) BlValue { return quantifyList(l, false) }

func quantifyList(l BlList, wantAll bool) BlValue {
	sawNull := false
	for _, e := range l.items {
		b, ok := e.(BlBoolean)
		if !ok {
			if e.IsNull() {
				sawNull = true
				continue
			}
			return Null()
		}
		if wantAll && !b.b {
			return BlBoolean{false}
		}
		if !wantAll && b.b {
			return BlBoolean{true}
		}
	}
	if sawNull {
		return Null()
	}
	return BlBoolean{wantAll}
}

func listOptions() []expr.Option {
	return []expr.Option{
		expr.Function("count", listArg(typed1(countFn)), new(func(BlValue) BlNumber)),
		expr.Function("listContains", typed2(listContainsFn), new(func(BlValue, BlValue) BlBoolean)),
		expr.Function("sublist", sublistFn,
			new(func(BlValue, BlValue) BlList),
			new(func(BlValue, BlValue, BlValue) BlList)),
		expr.Function("append", appendFn, new(func(BlValue, ...BlValue) BlList)),
		expr.Function("prepend", prependFn, new(func(BlValue, ...BlValue) BlList)),
		expr.Function("concatenate", concatenateFn, new(func(...BlValue) BlList)),
		expr.Function("union", unionDispatch, new(func(...BlValue) BlValue)),
		expr.Function("intersection", intersectionFn, new(func(...BlValue) BlList)),
		expr.Function("insertBefore", insertBeforeFn, new(func(BlValue, BlValue, BlValue) BlList)),
		expr.Function("insertAfter", insertAfterFn, new(func(BlValue, BlValue, BlValue) BlList)),
		expr.Function("remove", removeFn, new(func(BlValue, BlValue) BlList)),
		expr.Function("listReplace", listReplaceFn, new(func(BlValue, BlValue, BlValue) BlList)),
		expr.Function("flatten", typed1(flattenFn), new(func(BlValue) BlList)),
		expr.Function("distinct", typed1(distinctFn), new(func(BlValue) BlList)),
		expr.Function("duplicateValues", typed1(duplicateValuesFn), new(func(BlValue) BlList)),
		expr.Function("sort", sortFn,
			new(func(BlValue) BlList),
			new(func(BlValue, BlValue) BlList)),
		expr.Function("stringJoin", stringJoinFn,
			new(func(BlValue) BlString),
			new(func(BlValue, BlValue) BlString),
			new(func(BlValue, BlValue, BlValue, BlValue) BlString)),
		expr.Function("seq", seqFn, new(func(...BlValue) BlList)),
		expr.Function("zipStringJoin", zipStringJoinFn,
			new(func(BlValue) BlList),
			new(func(BlValue, BlValue) BlList)),

		// aggregation (listArg lets each accept a BlTable as its row list)
		expr.Function("sum", listArg(typed1err(sumFn)), new(func(BlValue) BlValue)),
		expr.Function("product", listArg(typed1err(productFn)), new(func(BlValue) BlValue)),
		expr.Function("min", listArg(typed1err(minFn)), new(func(BlValue) BlValue)),
		expr.Function("max", listArg(typed1err(maxFn)), new(func(BlValue) BlValue)),
		expr.Function("mean", listArg(typed1err(meanFn)), new(func(BlValue) BlValue)),
		expr.Function("median", listArg(typed1err(medianFn)), new(func(BlValue) BlValue)),
		expr.Function("stddev", listArg(typed1err(stddevFn)), new(func(BlValue) BlValue)),
		expr.Function("mode", listArg(typed1(modeFn)), new(func(BlValue) BlList)),
		expr.Function("all", listArg(typed1(allFn)), new(func(BlValue) BlValue)),
		expr.Function("any", listArg(typed1(anyFn)), new(func(BlValue) BlValue)),

		// list constructor emitted by the ArrayNode patcher
		expr.Function("__mklist", mkListFn, new(func(...BlValue) BlList)),
	}
}

// zipStringJoinFn joins N equal-length lists of strings position-wise.
func zipStringJoinFn(args ...any) (any, error) {
	outer, ok := asBl(args[0]).(BlList)
	if !ok {
		return nil, argTypeError(args[0])
	}
	if len(outer.items) == 0 {
		return BlList{}, nil
	}
	lists := make([]BlList, len(outer.items))
	n := -1
	for i, e := range outer.items {
		l, ok := e.(BlList)
		if !ok {
			return nil, &TypeError{Op: "zipStringJoin", Detail: "elements must be lists"}
		}
		if n == -1 {
			n = len(l.items)
		} else if len(l.items) != n {
			return nil, &TypeError{Op: "zipStringJoin", Detail: "inner lists must be the same length"}
		}
		lists[i] = l
	}
	delim := ""
	if len(args) > 1 {
		if d, ok := asBl(args[1]).(BlString); ok {
			delim = d.s
		}
	}
	out := make([]BlValue, n)
	for r := 0; r < n; r++ {
		parts := make([]string, len(lists))
		for c, l := range lists {
			s, ok := l.items[r].(BlString)
			if !ok {
				return nil, &TypeError{Op: "zipStringJoin", Detail: "elements must be strings"}
			}
			parts[c] = s.s
		}
		out[r] = str(strings.Join(parts, delim))
	}
	return BlList{out}, nil
}

// unionDispatch routes `union(...)` to table-union when the first operand is a
// BlTable, otherwise to list-union.
func unionDispatch(args ...any) (any, error) {
	if len(args) > 0 {
		if _, ok := asBl(args[0]).(BlTable); ok {
			return unionTablesFn(args...)
		}
	}
	return unionFn(args...)
}

// mkListFn wraps the (already-Bl) elements of an array literal into a BlList.
func mkListFn(args ...any) (any, error) {
	items := make([]BlValue, len(args))
	for i, a := range args {
		items[i] = asBl(a)
	}
	return BlList{items}, nil
}
