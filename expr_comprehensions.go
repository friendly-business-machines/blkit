package blkit

import (
	"strings"

	"github.com/expr-lang/expr"
)

// Comprehensions, filter, and indexing.
//
// expr's native map/filter/all/any builtins iterate a []any with a `let v = #`
// closure. blkit lists are BlList structs, so the engine converts to a []any
// view (__items), iterates with expr's builtins, then re-wraps the result
// (__wraplist / __anyTrue / __allTrue). FEEL `for`/`some`/`every` are
// source-rewritten to these forms before parsing; filter `l[pred]` and index
// `l[i]` are produced by the member patcher.

func comprehensionOptions() []expr.Option {
	return []expr.Option{
		expr.Function("__items", itemsFn, new(func(BlValue) []any)),
		expr.Function("__wraplist", wrapListFn, new(func([]any) BlList)),
		expr.Function("__anyTrue", anyTrueFn, new(func([]any) BlBoolean)),
		expr.Function("__allTrue", allTrueFn, new(func([]any) BlBoolean)),
		expr.Function("__index", indexFn, new(func(BlValue, BlValue) BlValue)),
	}
}

func itemsFn(args ...any) (any, error) {
	l, ok := coerceRowList(asBl(args[0]))
	if !ok {
		return nil, &TypeError{Op: "iterate", Detail: "not a list"}
	}
	out := make([]any, len(l.items))
	for i, e := range l.items {
		out[i] = e
	}
	return out, nil
}

// coerceRowList accepts a BlList directly, or a BlTable as its list of rows.
func coerceRowList(v BlValue) (BlList, bool) {
	switch x := v.(type) {
	case BlList:
		return x, true
	case BlTable:
		return x.toRowList(), true
	case BlCalendar:
		return entriesFn(x), true
	}
	return BlList{}, false
}

// listArg wraps a list-function impl so its first argument may be a BlTable
// (coerced to its list of row dictionaries).
func listArg(f func(...any) (any, error)) func(...any) (any, error) {
	return func(args ...any) (any, error) {
		switch v := asBl(args[0]).(type) {
		case BlTable:
			args[0] = v.toRowList()
		case BlCalendar:
			args[0] = entriesFn(v)
		}
		return f(args...)
	}
}

func wrapListFn(args ...any) (any, error) {
	arr, ok := args[0].([]any)
	if !ok {
		return nil, argTypeError(args[0])
	}
	items := make([]BlValue, len(arr))
	for i, e := range arr {
		items[i] = asBl(e)
	}
	return BlList{items}, nil
}

func isGoTrue(v any) bool {
	switch x := v.(type) {
	case bool:
		return x
	case BlBoolean:
		return x.b
	}
	return false
}

func anyTrueFn(args ...any) (any, error) {
	arr, ok := args[0].([]any)
	if !ok {
		return nil, argTypeError(args[0])
	}
	for _, e := range arr {
		if isGoTrue(e) {
			return BlBoolean{true}, nil
		}
	}
	return BlBoolean{false}, nil
}

func allTrueFn(args ...any) (any, error) {
	arr, ok := args[0].([]any)
	if !ok {
		return nil, argTypeError(args[0])
	}
	for _, e := range arr {
		if !isGoTrue(e) {
			return BlBoolean{false}, nil
		}
	}
	return BlBoolean{true}, nil
}

// indexFn implements 1-based list indexing; negatives count from the end;
// out-of-range yields null.
func indexFn(args ...any) (any, error) {
	if t, ok := asBl(args[0]).(BlTable); ok {
		idx, ok := asBl(args[1]).(BlNumber)
		if !ok {
			return Null(), nil
		}
		return t.rowByIndex(int(idx.d.IntPart())), nil
	}
	l, ok := asBl(args[0]).(BlList)
	if !ok {
		return Null(), nil
	}
	idx, ok := asBl(args[1]).(BlNumber)
	if !ok {
		return Null(), nil
	}
	i := int(idx.d.IntPart())
	n := len(l.items)
	if i < 0 {
		i = n + i + 1
	}
	if i < 1 || i > n {
		return Null(), nil
	}
	return l.items[i-1], nil
}

// --- source rewriting -----------------------------------------------------

// lowerComprehensions rewrites FEEL for/some/every into expr's closure-iterating
// builtins over a __items(list) view, recursing into branches and groups.
func lowerComprehensions(s string) string {
	s = mapTopLevelGroups(s, lowerComprehensions)
	if idx := indexTopLevelKeyword(s, "for", 0); idx >= 0 {
		if r, ok := rewriteFor(s, idx); ok {
			return r
		}
	}
	if idx := indexTopLevelKeyword(s, "some", 0); idx >= 0 {
		if r, ok := rewriteQuantifier(s, idx, "some", "__anyTrue"); ok {
			return r
		}
	}
	if idx := indexTopLevelKeyword(s, "every", 0); idx >= 0 {
		if r, ok := rewriteQuantifier(s, idx, "every", "__allTrue"); ok {
			return r
		}
	}
	return s
}

type iterator struct {
	name string
	list string
}

// parseIterators splits the iterator clause `x in xs, y in ys` at top-level
// commas, each into a name and list expression.
func parseIterators(clause string) ([]iterator, bool) {
	var iters []iterator
	for _, part := range splitTopLevelCommas(clause) {
		inIdx := indexTopLevelKeyword(part, "in", 0)
		if inIdx < 0 {
			return nil, false
		}
		name := strings.TrimSpace(part[:inIdx])
		list := strings.TrimSpace(part[inIdx+len("in"):])
		if name == "" || list == "" {
			return nil, false
		}
		iters = append(iters, iterator{name: name, list: list})
	}
	return iters, len(iters) > 0
}

func splitTopLevelCommas(s string) []string {
	var parts []string
	depth := 0
	start := 0
	for i := 0; i < len(s); {
		c := s[i]
		switch {
		case c == '"':
			i = skipString(s, i)
			continue
		case c == '(' || c == '[' || c == '{':
			depth++
		case c == ')' || c == ']' || c == '}':
			depth--
		case c == ',' && depth == 0:
			parts = append(parts, s[start:i])
			start = i + 1
		}
		i++
	}
	parts = append(parts, s[start:])
	return parts
}

// rewriteFor lowers `for <iters> return <body>` starting at s[idx].
func rewriteFor(s string, idx int) (string, bool) {
	retIdx := indexTopLevelKeyword(s, "return", idx+len("for"))
	if retIdx < 0 {
		return "", false
	}
	clause := s[idx+len("for") : retIdx]
	iters, ok := parseIterators(clause)
	if !ok {
		return "", false
	}
	bodyEnd := rightOperandEnd(s, retIdx+len("return"))
	body := strings.TrimSpace(s[retIdx+len("return") : bodyEnd])
	pre := s[:idx]
	post := s[bodyEnd:]
	built := buildForExpr(iters, body)
	return pre + built + lowerComprehensions(post), true
}

func buildForExpr(iters []iterator, body string) string {
	body = lowerComprehensions(body)
	if len(iters) == 1 {
		it := iters[0]
		return "__wraplist(map(__items(" + lowerComprehensions(it.list) + "), let " + it.name + " = #; " + body + "))"
	}
	// Multi-iterator: nest and flatten one level per extra generator.
	inner := buildForExpr(iters[1:], body)
	it := iters[0]
	return "flatten(__wraplist(map(__items(" + lowerComprehensions(it.list) + "), let " + it.name + " = #; " + inner + ")))"
}

// rewriteQuantifier lowers `some/every <iters> satisfies <cond>`.
func rewriteQuantifier(s string, idx int, kw, combiner string) (string, bool) {
	satIdx := indexTopLevelKeyword(s, "satisfies", idx+len(kw))
	if satIdx < 0 {
		return "", false
	}
	clause := s[idx+len(kw) : satIdx]
	iters, ok := parseIterators(clause)
	if !ok {
		return "", false
	}
	condEnd := rightOperandEnd(s, satIdx+len("satisfies"))
	cond := strings.TrimSpace(s[satIdx+len("satisfies") : condEnd])
	pre := s[:idx]
	post := s[condEnd:]
	built := buildQuantifier(iters, cond, combiner)
	return pre + built + lowerComprehensions(post), true
}

func buildQuantifier(iters []iterator, cond, combiner string) string {
	cond = lowerComprehensions(cond)
	it := iters[0]
	var body string
	if len(iters) == 1 {
		body = "__truthy(" + cond + ")"
	} else {
		// Nested quantifier yields a BlBoolean; convert to a Go bool for the
		// outer map closure.
		body = "__truthy(" + buildQuantifier(iters[1:], cond, combiner) + ")"
	}
	return combiner + "(map(__items(" + lowerComprehensions(it.list) + "), let " + it.name + " = #; " + body + "))"
}
