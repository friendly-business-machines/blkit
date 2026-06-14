package core

import (
	"sort"

	"github.com/expr-lang/expr"
)

// BlSortKey is the transient sort key produced by asc/desc/inOrder for t.sort.
type BlSortKey struct {
	column     string
	descending bool
	order      *BlList // explicit value order (inOrder); nil for asc/desc
}

func (BlSortKey) Type() Type            { return TypeSortKey }
func (BlSortKey) Equal(BlValue) BlValue { return BlBoolean{false} }
func (k BlSortKey) String() string      { return "sortKey(" + k.column + ")" }
func (BlSortKey) IsNull() bool          { return false }
func (BlSortKey) isBlValue()            {}

func ascFn(c BlString) BlSortKey  { return BlSortKey{column: c.s} }
func descFn(c BlString) BlSortKey { return BlSortKey{column: c.s, descending: true} }
func inOrderFn(c BlString, order BlList) BlSortKey {
	o := order
	return BlSortKey{column: c.s, order: &o}
}

func (t BlTable) hasColumn(name string) bool {
	for _, c := range t.columns {
		if c == name {
			return true
		}
	}
	return false
}

// selectFn keeps only the named columns, in the listed order.
func selectFn(t BlTable, names ...BlString) (BlTable, error) {
	cols := make([]string, len(names))
	for i, n := range names {
		if !t.hasColumn(n.s) {
			return BlTable{}, &TypeError{Op: "select", Detail: "unknown column " + n.s}
		}
		cols[i] = n.s
	}
	rows := make([]BlDictionary, len(t.rows))
	for i, r := range t.rows {
		d := newDictionary()
		for _, c := range cols {
			v, _ := r.get(c)
			d.set(c, v)
		}
		rows[i] = d
	}
	return BlTable{columns: cols, rows: rows}, nil
}

func renameFn(t BlTable, from, to BlString) (BlTable, error) {
	if !t.hasColumn(from.s) {
		return BlTable{}, &TypeError{Op: "rename", Detail: "unknown column " + from.s}
	}
	if t.hasColumn(to.s) {
		return BlTable{}, &TypeError{Op: "rename", Detail: "column " + to.s + " already exists"}
	}
	cols := make([]string, len(t.columns))
	for i, c := range t.columns {
		if c == from.s {
			cols[i] = to.s
		} else {
			cols[i] = c
		}
	}
	rows := make([]BlDictionary, len(t.rows))
	for i, r := range t.rows {
		d := newDictionary()
		for _, c := range t.columns {
			v, _ := r.get(c)
			if c == from.s {
				d.set(to.s, v)
			} else {
				d.set(c, v)
			}
		}
		rows[i] = d
	}
	return BlTable{columns: cols, rows: rows}, nil
}

func tableDistinctFn(t BlTable) BlTable {
	var out []BlDictionary
	for _, r := range t.rows {
		dup := false
		for _, e := range out {
			if eqTrue(r.Equal(e)) {
				dup = true
				break
			}
		}
		if !dup {
			out = append(out, r)
		}
	}
	return t.subTable(out)
}

func tableSortFn(t BlTable, keys ...any) (BlTable, error) {
	if len(keys) == 0 {
		return BlTable{}, &TypeError{Op: "sort", Detail: "at least one sort key required"}
	}
	specs := make([]BlSortKey, len(keys))
	for i, k := range keys {
		switch kv := asBl(k).(type) {
		case BlString:
			specs[i] = BlSortKey{column: kv.s}
		case BlSortKey:
			specs[i] = kv
		default:
			return BlTable{}, &TypeError{Op: "sort", Detail: "invalid sort key"}
		}
		if !t.hasColumn(specs[i].column) {
			return BlTable{}, &TypeError{Op: "sort", Detail: "unknown column " + specs[i].column}
		}
	}
	rows := append([]BlDictionary{}, t.rows...)
	var sortErr error
	sort.SliceStable(rows, func(i, j int) bool {
		for _, k := range specs {
			ci, _ := rows[i].get(k.column)
			cj, _ := rows[j].get(k.column)
			c, ok := compareCells(ci, cj, k, &sortErr)
			if c != 0 {
				return ok && c < 0
			}
		}
		return false
	})
	if sortErr != nil {
		return BlTable{}, sortErr
	}
	return t.subTable(rows), nil
}

// compareCells returns the ordering of two cells under a sort key (already
// adjusted for direction / explicit order). Null cells sort to the end (asc) /
// lead (desc).
func compareCells(a, b BlValue, k BlSortKey, sortErr *error) (int, bool) {
	if k.order != nil {
		ra, rb := rankInOrder(a, *k.order), rankInOrder(b, *k.order)
		if ra != rb {
			return cmpInt(ra, rb), true
		}
		// both unlisted (or both same rank): fall back to ascending compare
		return ascCompare(a, b, sortErr), true
	}
	c := ascCompare(a, b, sortErr)
	if k.descending {
		return -c, true
	}
	return c, true
}

func cmpInt(a, b int) int {
	switch {
	case a < b:
		return -1
	case a > b:
		return 1
	default:
		return 0
	}
}

func rankInOrder(v BlValue, order BlList) int {
	for i, e := range order.items {
		if eqTrue(e.Equal(v)) {
			return i
		}
	}
	return len(order.items) // unlisted → trails
}

// ascCompare compares two cells ascending; nulls sort last.
func ascCompare(a, b BlValue, sortErr *error) int {
	an, bn := a.IsNull(), b.IsNull()
	if an || bn {
		switch {
		case an && bn:
			return 0
		case an:
			return 1 // null after
		default:
			return -1
		}
	}
	c, ok := compareValues(a, b)
	if !ok {
		if *sortErr == nil {
			*sortErr = &TypeError{Op: "sort", Detail: "non-comparable cells"}
		}
		return 0
	}
	return c
}

func tableSliceFn(t BlTable, sel BlValue) (BlTable, error) {
	switch s := sel.(type) {
	case BlNumber:
		return t.rowByIndex(int(s.d.IntPart())), nil
	case BlList:
		flat := flattenFn(s)
		seen := map[int]bool{}
		var out []BlDictionary
		for _, e := range flat.items {
			idx, ok := e.(BlNumber)
			if !ok {
				return BlTable{}, &TypeError{Op: "slice", Detail: "index must be a number"}
			}
			i := int(idx.d.IntPart())
			n := len(t.rows)
			if i < 0 {
				i = n + i + 1
			}
			if i < 1 || i > n || seen[i] {
				continue
			}
			seen[i] = true
			out = append(out, t.rows[i-1])
		}
		return t.subTable(out), nil
	default:
		return BlTable{}, &TypeError{Op: "slice", Detail: "invalid row selector"}
	}
}

// unionTablesFn stacks rows of every operand (UNION ALL). An optional trailing
// BlString is the reconciliation mode.
func unionTablesFn(args ...any) (any, error) {
	how := "error"
	var tables []BlTable
	for i, a := range args {
		v := asBl(a)
		if s, ok := v.(BlString); ok && i == len(args)-1 {
			how = s.s
			break
		}
		t, ok := v.(BlTable)
		if !ok {
			return nil, &TypeError{Op: "union", Detail: "operands must be tables"}
		}
		tables = append(tables, t)
	}
	if len(tables) == 0 {
		return BlTable{}, nil
	}
	cols, err := reconcileColumns(tables, how)
	if err != nil {
		return nil, err
	}
	var rows []BlDictionary
	for _, t := range tables {
		for _, r := range t.rows {
			d := newDictionary()
			for _, c := range cols {
				if v, present := r.get(c); present {
					d.set(c, v)
				} else {
					d.set(c, Null())
				}
			}
			rows = append(rows, d)
		}
	}
	return BlTable{columns: cols, rows: rows}, nil
}

func reconcileColumns(tables []BlTable, how string) ([]string, error) {
	first := tables[0].columns
	switch how {
	case "error":
		for _, t := range tables[1:] {
			if !sameStringSet(first, t.columns) {
				return nil, &TypeError{Op: "union", Detail: "column sets differ"}
			}
		}
		return first, nil
	case "all":
		cols := append([]string{}, first...)
		seen := map[string]bool{}
		for _, c := range first {
			seen[c] = true
		}
		for _, t := range tables[1:] {
			for _, c := range t.columns {
				if !seen[c] {
					seen[c] = true
					cols = append(cols, c)
				}
			}
		}
		return cols, nil
	case "common":
		var cols []string
		for _, c := range first {
			inAll := true
			for _, t := range tables[1:] {
				if !t.hasColumn(c) {
					inAll = false
					break
				}
			}
			if inAll {
				cols = append(cols, c)
			}
		}
		return cols, nil
	default:
		return nil, &TypeError{Op: "union", Detail: "unknown reconciliation mode " + how}
	}
}

// joinFn implements equi-join and cross join.
func joinFn(args ...any) (any, error) {
	if len(args) < 3 {
		return nil, &TypeError{Op: "join", Detail: "expected (t, other, on[, how])"}
	}
	t, ok1 := asBl(args[0]).(BlTable)
	other, ok2 := asBl(args[1]).(BlTable)
	if !ok1 || !ok2 {
		return nil, &TypeError{Op: "join", Detail: "operands must be tables"}
	}
	how := "inner"
	if len(args) >= 4 {
		h, ok := asBl(args[3]).(BlString)
		if !ok {
			return nil, &TypeError{Op: "join", Detail: "how must be a string"}
		}
		how = h.s
	}
	var keys []string
	switch on := asBl(args[2]).(type) {
	case BlString:
		keys = []string{on.s}
	case BlList:
		for _, e := range on.items {
			s, ok := e.(BlString)
			if !ok {
				return nil, &TypeError{Op: "join", Detail: "keys must be strings"}
			}
			keys = append(keys, s.s)
		}
	default:
		return nil, &TypeError{Op: "join", Detail: "on must be a column name or list"}
	}
	if how == "cross" {
		if len(keys) != 0 {
			return nil, &TypeError{Op: "join", Detail: "cross join takes no keys"}
		}
		return crossJoin(t, other)
	}
	if len(keys) == 0 {
		return nil, &TypeError{Op: "join", Detail: "empty key requires how=cross"}
	}
	return equiJoin(t, other, keys, how)
}

func crossJoin(t, other BlTable) (any, error) {
	cols, err := joinColumns(t, other, nil)
	if err != nil {
		return nil, err
	}
	var rows []BlDictionary
	for _, lr := range t.rows {
		for _, rr := range other.rows {
			rows = append(rows, mergeRow(cols, lr, rr, nil))
		}
	}
	return BlTable{columns: cols, rows: rows}, nil
}

func equiJoin(t, other BlTable, keys []string, how string) (any, error) {
	for _, k := range keys {
		if !t.hasColumn(k) || !other.hasColumn(k) {
			return nil, &TypeError{Op: "join", Detail: "key column " + k + " missing in one side"}
		}
	}
	cols, err := joinColumns(t, other, keys)
	if err != nil {
		return nil, err
	}
	keyOf := func(r BlDictionary) string {
		var b []byte
		for _, k := range keys {
			v, _ := r.get(k)
			b = append(b, []byte(v.String())...)
			b = append(b, 0)
		}
		return string(b)
	}
	rightByKey := map[string][]BlDictionary{}
	for _, rr := range other.rows {
		k := keyOf(rr)
		rightByKey[k] = append(rightByKey[k], rr)
	}
	var rows []BlDictionary
	matchedRight := map[int]bool{}
	for _, lr := range t.rows {
		matches := rightByKey[keyOf(lr)]
		if len(matches) == 0 {
			if how == "left" || how == "outer" {
				rows = append(rows, mergeRow(cols, lr, BlDictionary{}, keys))
			}
			continue
		}
		for _, rr := range matches {
			rows = append(rows, mergeRow(cols, lr, rr, keys))
		}
	}
	if how == "right" || how == "outer" {
		for ri, rr := range other.rows {
			if _, ok := rightByKeyHasLeft(t, rr, keys); !ok {
				_ = ri
				_ = matchedRight
				rows = append(rows, mergeRow(cols, BlDictionary{}, rr, keys))
			}
		}
	}
	if how == "right" {
		// right join: every right row; recompute properly
		return rightJoin(t, other, keys, cols)
	}
	return BlTable{columns: cols, rows: rows}, nil
}

func rightByKeyHasLeft(t BlTable, rr BlDictionary, keys []string) (int, bool) {
	for i, lr := range t.rows {
		if rowsKeyEqual(lr, rr, keys) {
			return i, true
		}
	}
	return 0, false
}

func rightJoin(t, other BlTable, keys, cols []string) (any, error) {
	var rows []BlDictionary
	for _, rr := range other.rows {
		var matches []BlDictionary
		for _, lr := range t.rows {
			if rowsKeyEqual(lr, rr, keys) {
				matches = append(matches, lr)
			}
		}
		if len(matches) == 0 {
			rows = append(rows, mergeRow(cols, BlDictionary{}, rr, keys))
		} else {
			for _, lr := range matches {
				rows = append(rows, mergeRow(cols, lr, rr, keys))
			}
		}
	}
	return BlTable{columns: cols, rows: rows}, nil
}

func rowsKeyEqual(a, b BlDictionary, keys []string) bool {
	for _, k := range keys {
		av, _ := a.get(k)
		bv, _ := b.get(k)
		if av == nil {
			av = Null()
		}
		if bv == nil {
			bv = Null()
		}
		if !eqTrue(av.Equal(bv)) {
			return false
		}
	}
	return true
}

// joinColumns computes the result column order: keys once, then t's non-key
// columns, then other's non-key columns. A shared non-key column is ambiguous.
func joinColumns(t, other BlTable, keys []string) ([]string, error) {
	isKey := map[string]bool{}
	for _, k := range keys {
		isKey[k] = true
	}
	cols := append([]string{}, keys...)
	for _, c := range t.columns {
		if !isKey[c] {
			cols = append(cols, c)
		}
	}
	otherCols := map[string]bool{}
	for _, c := range t.columns {
		if !isKey[c] {
			otherCols[c] = true
		}
	}
	for _, c := range other.columns {
		if isKey[c] {
			continue
		}
		if otherCols[c] {
			return nil, &TypeError{Op: "join", Detail: "ambiguous column " + c}
		}
		cols = append(cols, c)
	}
	return cols, nil
}

func mergeRow(cols []string, left, right BlDictionary, keys []string) BlDictionary {
	d := newDictionary()
	for _, c := range cols {
		if v, present := left.get(c); present {
			d.set(c, v)
		} else if v, present := right.get(c); present {
			d.set(c, v)
		} else {
			d.set(c, Null())
		}
	}
	return d
}

func tableMethodOptions() []expr.Option {
	return []expr.Option{
		expr.Function("asc", typed1(ascFn), new(func(BlValue) BlSortKey)),
		expr.Function("desc", typed1(descFn), new(func(BlValue) BlSortKey)),
		expr.Function("inOrder", typed2(inOrderFn), new(func(BlValue, BlValue) BlSortKey)),
		// `union` is registered once in listOptions via unionDispatch (it routes
		// to table-union when the first operand is a table).
		expr.Function("join", joinFn, new(func(...BlValue) BlTable)),

		// method backings (emitted by the method-call patcher)
		expr.Function("__tableSelect", variadicTableSelect, new(func(...BlValue) BlTable)),
		expr.Function("__tableRename", variadicTableRename, new(func(...BlValue) BlTable)),
		expr.Function("__tableDistinct", typed1(tableDistinctFn), new(func(BlValue) BlTable)),
		expr.Function("__tableSort", variadicTableSort, new(func(...BlValue) BlTable)),
		expr.Function("__tableSlice", typed2err(tableSliceFn), new(func(BlValue, BlValue) BlTable)),
		expr.Function("__tableToList", typed1(func(t BlTable) BlList { return t.toList() }), new(func(BlValue) BlList)),
		expr.Function("__tableToDict", typed1err(func(t BlTable) (BlValue, error) { return t.toDict() }), new(func(BlValue) BlValue)),
		expr.Function("__tableToValue", typed1err(func(t BlTable) (BlValue, error) { return t.toValue() }), new(func(BlValue) BlValue)),
	}
}

func variadicTableSelect(args ...any) (any, error) {
	t, ok := asBl(args[0]).(BlTable)
	if !ok {
		return nil, argTypeError(args[0])
	}
	names := make([]BlString, 0, len(args)-1)
	for _, a := range args[1:] {
		s, ok := asBl(a).(BlString)
		if !ok {
			return nil, &TypeError{Op: "select", Detail: "column names must be strings"}
		}
		names = append(names, s)
	}
	return selectFn(t, names...)
}

func variadicTableSort(args ...any) (any, error) {
	t, ok := asBl(args[0]).(BlTable)
	if !ok {
		return nil, argTypeError(args[0])
	}
	return tableSortFn(t, args[1:]...)
}

func variadicTableRename(args ...any) (any, error) {
	t, ok := asBl(args[0]).(BlTable)
	if !ok {
		return nil, argTypeError(args[0])
	}
	if len(args) != 3 {
		return nil, &TypeError{Op: "rename", Detail: "expected (from, to)"}
	}
	from, ok1 := asBl(args[1]).(BlString)
	to, ok2 := asBl(args[2]).(BlString)
	if !ok1 || !ok2 {
		return nil, &TypeError{Op: "rename", Detail: "from/to must be strings"}
	}
	return renameFn(t, from, to)
}
