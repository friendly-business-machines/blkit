package blkit

import (
	"sort"
	"strings"

	"github.com/expr-lang/expr"
)

// BlTable is a DMN relation: an ordered, immutable list of uniformly-keyed
// dictionary rows. Columns are stored in canonical (sorted) order.
type BlTable struct {
	columns []string
	rows    []BlDictionary
}

func (BlTable) Type() Type { return TypeTable }

func (t BlTable) Equal(other BlValue) BlValue {
	o, ok := other.(BlTable)
	if !ok {
		return BlBoolean{false}
	}
	if !sameStringSet(t.columns, o.columns) || len(t.rows) != len(o.rows) {
		return BlBoolean{false}
	}
	for i := range t.rows {
		if !eqTrue(t.rows[i].Equal(o.rows[i])) {
			return BlBoolean{false}
		}
	}
	return BlBoolean{true}
}

func (t BlTable) String() string {
	parts := make([]string, len(t.rows))
	for i, r := range t.rows {
		parts[i] = r.String()
	}
	return "[" + strings.Join(parts, ", ") + "]"
}

func (BlTable) IsNull() bool { return false }

func (BlTable) isBlValue() {}

func sameStringSet(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	m := map[string]bool{}
	for _, x := range a {
		m[x] = true
	}
	for _, x := range b {
		if !m[x] {
			return false
		}
	}
	return true
}

// Col / Cols / Row form the host-side typed header + positional value rows.
type Col struct {
	Name string
	Type Type
}
type Cols []Col
type Row []any

// Table constructs a BlTable from a typed header and positional value rows.
func Table(columns Cols, rows ...Row) (BlTable, error) {
	names := make([]string, len(columns))
	colType := map[string]Type{}
	seen := map[string]bool{}
	for i, c := range columns {
		if c.Name == "" {
			return BlTable{}, &TypeError{Op: "Table", Detail: "empty column name"}
		}
		if seen[c.Name] {
			return BlTable{}, &TypeError{Op: "Table", Detail: "duplicate column " + c.Name}
		}
		if c.Type == TypeNull || !knownType(c.Type) {
			return BlTable{}, &TypeError{Op: "Table", Detail: "invalid column type"}
		}
		seen[c.Name] = true
		names[i] = c.Name
		colType[c.Name] = c.Type
	}
	built := make([]BlDictionary, 0, len(rows))
	for _, r := range rows {
		if len(r) != len(columns) {
			return BlTable{}, &TypeError{Op: "Table", Detail: "row length mismatch"}
		}
		d := newDictionary()
		for i, cell := range r {
			v, err := wrap(cell)
			if err != nil {
				return BlTable{}, err
			}
			if !v.IsNull() && v.Type() != colType[names[i]] && colType[names[i]] != TypeAny {
				return BlTable{}, &TypeError{Op: "Table", Detail: "cell type mismatch in column " + names[i]}
			}
			d.set(names[i], v)
		}
		built = append(built, d)
	}
	return BlTable{columns: sortedCopy(names), rows: built}, nil
}

func sortedCopy(ss []string) []string {
	out := append([]string{}, ss...)
	sort.Strings(out)
	return out
}

// Columns returns the canonical column order.
func (t BlTable) Columns() []string { return append([]string{}, t.columns...) }

// Rows returns a defensive copy of the rows.
func (t BlTable) Rows() []BlDictionary { return append([]BlDictionary{}, t.rows...) }

// NRows / NCols.
func (t BlTable) NRows() int { return len(t.rows) }
func (t BlTable) NCols() int { return len(t.columns) }

// tableFromDictsFn validates a list of uniformly-keyed dictionaries.
func tableFromDictsFn(l BlList) (BlTable, error) {
	if len(l.items) == 0 {
		return BlTable{}, nil
	}
	var cols []string
	for i, e := range l.items {
		d, ok := e.(BlDictionary)
		if !ok {
			return BlTable{}, &TypeError{Op: "tableFromDicts", Detail: "elements must be dictionaries"}
		}
		keys := d.sortedKeys()
		if i == 0 {
			cols = keys
		} else if !sameStringSet(cols, keys) {
			return BlTable{}, &TypeError{Op: "tableFromDicts", Detail: "non-uniform keys"}
		}
	}
	rows := make([]BlDictionary, len(l.items))
	for i, e := range l.items {
		rows[i] = e.(BlDictionary)
	}
	return BlTable{columns: cols, rows: rows}, nil
}

// tableFn is the columnar constructor: table(names, rows…).
func tableFn(args ...any) (any, error) {
	if len(args) == 0 {
		return nil, &TypeError{Op: "table", Detail: "expected column names"}
	}
	header, ok := asBl(args[0]).(BlList)
	if !ok {
		return nil, &TypeError{Op: "table", Detail: "first argument must be a list of column names"}
	}
	names := make([]string, len(header.items))
	seen := map[string]bool{}
	for i, h := range header.items {
		s, ok := h.(BlString)
		if !ok || s.s == "" {
			return nil, &TypeError{Op: "table", Detail: "column names must be non-empty strings"}
		}
		if seen[s.s] {
			return nil, &TypeError{Op: "table", Detail: "duplicate column " + s.s}
		}
		seen[s.s] = true
		names[i] = s.s
	}
	colType := map[string]Type{}
	rows := make([]BlDictionary, 0, len(args)-1)
	for _, ra := range args[1:] {
		rl, ok := asBl(ra).(BlList)
		if !ok || len(rl.items) != len(names) {
			return nil, &TypeError{Op: "table", Detail: "each row must be a list matching the header length"}
		}
		d := newDictionary()
		for i, cell := range rl.items {
			if !cell.IsNull() {
				if ct, seen := colType[names[i]]; seen {
					if cell.Type() != ct {
						return nil, &TypeError{Op: "table", Detail: "non-uniform type in column " + names[i]}
					}
				} else {
					colType[names[i]] = cell.Type()
				}
			}
			d.set(names[i], cell)
		}
		rows = append(rows, d)
	}
	return BlTable{columns: sortedCopy(names), rows: rows}, nil
}

func hasColumnFn(t BlTable, name BlString) BlBoolean {
	for _, c := range t.columns {
		if c == name.s {
			return BlBoolean{true}
		}
	}
	return BlBoolean{false}
}

// tableComponent resolves a bare dot-path on a table: attributes or column
// projection.
func tableComponent(t BlTable, name string) (BlValue, bool) {
	switch name {
	case "nRows":
		return numFromInt(len(t.rows)), true
	case "nCols":
		return numFromInt(len(t.columns)), true
	case "colNames":
		return listOfStrings(t.columns), true
	}
	// column projection over rows
	out := make([]BlValue, len(t.rows))
	for i, r := range t.rows {
		if v, present := r.get(name); present {
			out[i] = v
		} else {
			out[i] = Null()
		}
	}
	return BlList{out}, true
}

// subTable builds a sub-table preserving the column set.
func (t BlTable) subTable(rows []BlDictionary) BlTable {
	return BlTable{columns: t.columns, rows: rows}
}

// tableRowByIndex returns a 1-row sub-table for a 1-based index (negative from
// end); out-of-range → empty sub-table.
func (t BlTable) rowByIndex(i int) BlTable {
	n := len(t.rows)
	if i < 0 {
		i = n + i + 1
	}
	if i < 1 || i > n {
		return t.subTable(nil)
	}
	return t.subTable([]BlDictionary{t.rows[i-1]})
}

// ToList: single-column table → cell values; otherwise rows (dicts).
func (t BlTable) toList() BlList {
	if len(t.columns) == 1 {
		col := t.columns[0]
		out := make([]BlValue, len(t.rows))
		for i, r := range t.rows {
			v, _ := r.get(col)
			out[i] = v
		}
		return BlList{out}
	}
	out := make([]BlValue, len(t.rows))
	for i, r := range t.rows {
		out[i] = r
	}
	return BlList{out}
}

func (t BlTable) toDict() (BlValue, error) {
	if len(t.rows) != 1 {
		return nil, &TypeError{Op: "toDict", Detail: "requires exactly one row"}
	}
	return t.rows[0], nil
}

func (t BlTable) toValue() (BlValue, error) {
	if len(t.rows) != 1 || len(t.columns) != 1 {
		return nil, &TypeError{Op: "toValue", Detail: "requires exactly one row and one column"}
	}
	v, _ := t.rows[0].get(t.columns[0])
	return v, nil
}

// toRowList returns the table's rows as a BlList of dictionaries.
func (t BlTable) toRowList() BlList {
	out := make([]BlValue, len(t.rows))
	for i, r := range t.rows {
		out[i] = r
	}
	return BlList{out}
}

// retableFn re-wraps a filtered []any: a table receiver yields a sub-table; a
// list receiver yields a list.
func retableFn(args ...any) (any, error) {
	orig := asBl(args[0])
	arr, ok := args[1].([]any)
	if !ok {
		return nil, argTypeError(args[1])
	}
	if t, ok := orig.(BlTable); ok {
		rows := make([]BlDictionary, 0, len(arr))
		for _, e := range arr {
			if d, ok := asBl(e).(BlDictionary); ok {
				rows = append(rows, d)
			}
		}
		return t.subTable(rows), nil
	}
	items := make([]BlValue, len(arr))
	for i, e := range arr {
		items[i] = asBl(e)
	}
	return BlList{items}, nil
}

// withColumnTableFn adds or replaces a column from per-row computed values.
func withColumnTableFn(args ...any) (any, error) {
	t, ok := asBl(args[0]).(BlTable)
	if !ok {
		return nil, argTypeError(args[0])
	}
	name, ok := asBl(args[1]).(BlString)
	if !ok {
		return nil, argTypeError(args[1])
	}
	vals, ok := args[2].([]any)
	if !ok {
		return nil, argTypeError(args[2])
	}
	cols := append([]string{}, t.columns...)
	if !t.hasColumn(name.s) {
		cols = append(cols, name.s)
	}
	rows := make([]BlDictionary, len(t.rows))
	for i, r := range t.rows {
		d := newDictionary()
		for _, c := range t.columns {
			v, _ := r.get(c)
			d.set(c, v)
		}
		if i < len(vals) {
			d.set(name.s, asBl(vals[i]))
		} else {
			d.set(name.s, Null())
		}
		rows[i] = d
	}
	return BlTable{columns: cols, rows: rows}, nil
}

// groupsFn partitions a table into group sub-tables in first-appearance key
// order.
func groupsFn(args ...any) (any, error) {
	t, ok := asBl(args[0]).(BlTable)
	if !ok {
		return nil, argTypeError(args[0])
	}
	keyList, ok := asBl(args[1]).(BlList)
	if !ok {
		return nil, argTypeError(args[1])
	}
	keys := make([]string, len(keyList.items))
	for i, k := range keyList.items {
		s, ok := k.(BlString)
		if !ok {
			return nil, &TypeError{Op: "groupBy", Detail: "keys must be strings"}
		}
		if !t.hasColumn(s.s) {
			return nil, &TypeError{Op: "groupBy", Detail: "unknown column " + s.s}
		}
		keys[i] = s.s
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
	order := []string{}
	groups := map[string][]BlDictionary{}
	for _, r := range t.rows {
		k := keyOf(r)
		if _, seen := groups[k]; !seen {
			order = append(order, k)
		}
		groups[k] = append(groups[k], r)
	}
	out := make([]any, len(order))
	for i, k := range order {
		out[i] = t.subTable(groups[k])
	}
	return out, nil
}

// groupKeyFn returns a group's shared key cell (from its first row).
func groupKeyFn(args ...any) (any, error) {
	t, ok := asBl(args[0]).(BlTable)
	if !ok || len(t.rows) == 0 {
		return Null(), nil
	}
	col, ok := asBl(args[1]).(BlString)
	if !ok {
		return nil, argTypeError(args[1])
	}
	v, _ := t.rows[0].get(col.s)
	if v == nil {
		return Null(), nil
	}
	return v, nil
}

// wrapTableFn builds a table from a []any of row dictionaries (agg result).
func wrapTableFn(args ...any) (any, error) {
	arr, ok := args[0].([]any)
	if !ok {
		return nil, argTypeError(args[0])
	}
	items := make([]BlValue, len(arr))
	for i, e := range arr {
		items[i] = asBl(e)
	}
	return tableFromDictsFn(BlList{items})
}

// selectColumns builds a sub-table restricted to the named columns (in the
// given order). Unknown columns are included with null cells per the spec.
func (t BlTable) selectColumns(names []string) BlTable {
	rows := make([]BlDictionary, len(t.rows))
	for i, r := range t.rows {
		d := newDictionary()
		for _, c := range names {
			if v, present := r.get(c); present {
				d.set(c, v)
			} else {
				d.set(c, Null())
			}
		}
		rows[i] = d
	}
	// dedupe column names, first occurrence wins
	var cols []string
	seen := map[string]bool{}
	for _, c := range names {
		if !seen[c] {
			seen[c] = true
			cols = append(cols, c)
		}
	}
	return BlTable{columns: cols, rows: rows}
}

func columnSelector(sel BlValue) ([]string, error) {
	switch c := sel.(type) {
	case BlString:
		return []string{c.s}, nil
	case BlList:
		names := make([]string, len(c.items))
		for i, e := range c.items {
			s, ok := e.(BlString)
			if !ok {
				return nil, &TypeError{Op: "tableIndex", Detail: "column names must be strings"}
			}
			names[i] = s.s
		}
		return names, nil
	default:
		return nil, &TypeError{Op: "tableIndex", Detail: "column selector must be a name or list of names"}
	}
}

// tableColsFn implements the all-rows column form t[, c].
func tableColsFn(args ...any) (any, error) {
	t, ok := asBl(args[0]).(BlTable)
	if !ok {
		return nil, argTypeError(args[0])
	}
	names, err := columnSelector(asBl(args[1]))
	if err != nil {
		return nil, err
	}
	return t.selectColumns(names), nil
}

// tableIndexFn implements the two-slot form t[r, c].
func tableIndexFn(args ...any) (any, error) {
	t, ok := asBl(args[0]).(BlTable)
	if !ok {
		return nil, argTypeError(args[0])
	}
	// row selection
	var rowsSub BlTable
	switch r := asBl(args[1]).(type) {
	case BlNumber:
		rowsSub = t.rowByIndex(int(r.d.IntPart()))
	case BlList:
		sub, err := tableSliceFn(t, r)
		if err != nil {
			return nil, err
		}
		rowsSub = sub
	default:
		return nil, &TypeError{Op: "tableIndex", Detail: "row selector must be an index or list"}
	}
	names, err := columnSelector(asBl(args[2]))
	if err != nil {
		return nil, err
	}
	return rowsSub.selectColumns(names), nil
}

func tableOptions() []expr.Option {
	return []expr.Option{
		expr.Function("table", tableFn, new(func(...BlValue) BlTable)),
		expr.Function("tableFromDicts", typed1err(tableFromDictsFn), new(func(BlValue) BlTable)),
		expr.Function("hasColumn", typed2(hasColumnFn), new(func(BlValue, BlValue) BlBoolean)),
		expr.Function("tableIndex", tableIndexFn, new(func(BlValue, BlValue, BlValue) BlTable)),
		expr.Function("tableCols", tableColsFn, new(func(BlValue, BlValue) BlTable)),

		// row-scoped method backings emitted by the patcher
		expr.Function("__retable", retableFn, new(func(BlValue, []any) BlValue)),
		expr.Function("__withColumn", withColumnTableFn, new(func(BlValue, BlValue, BlValue) BlTable)),
		expr.Function("__groups", groupsFn, new(func(BlValue, BlValue) []any)),
		expr.Function("__groupKey", groupKeyFn, new(func(BlValue, BlValue) BlValue)),
		expr.Function("__wraptable", wrapTableFn, new(func(BlValue) BlTable)),
	}
}
