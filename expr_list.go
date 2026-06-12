package blkit

import (
	"sort"
	"strings"
)

// BlList is an ordered, heterogeneous sequence of BlValues. Indexing in the
// expression language is 1-based.
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

// ToArray returns a copy of the underlying elements for host code.
func (l BlList) ToArray() []BlValue {
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

// listOfStrings builds a BlList of BlStrings.
func listOfStrings(ss []string) BlList {
	items := make([]BlValue, len(ss))
	for i, s := range ss {
		items[i] = BlString{s}
	}
	return BlList{items}
}

// sortLongestFirst orders alternation branches so the longest delimiter wins.
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
