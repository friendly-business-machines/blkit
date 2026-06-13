package blkit

import (
	"fmt"

	"github.com/expr-lang/expr/ast"
	"github.com/shopspring/decimal"
)

// feelPatcher rewrites the parsed expr AST into the engine's lowered form
// before type-checking and compilation. Because ast.Walk is post-order, every
// operand node has already been patched when its parent operator is visited.
type feelPatcher struct {
	counter int // unique suffix source for short-circuit let bindings
}

func newFeelPatcher() *feelPatcher { return &feelPatcher{} }

// binaryDispatch maps an arithmetic/comparison operator token to its dispatch
// function name.
var binaryDispatch = map[string]string{
	"+":  "__add",
	"-":  "__sub",
	"*":  "__mul",
	"/":  "__div",
	"**": "__pow",
	"^":  "__pow",
	"<":  "__lt",
	"<=": "__le",
	">":  "__gt",
	">=": "__ge",
	"==": "__eq",
	"!=": "__ne",
}

func (p *feelPatcher) Visit(node *ast.Node) {
	switch n := (*node).(type) {
	case *ast.IntegerNode:
		ast.Patch(node, constNode(BlNumber{decimal.NewFromInt(int64(n.Value))}))
	case *ast.FloatNode:
		// Decimal/exponent literals are normally rewritten to number("…") in
		// the source before parsing; this is a lossy fallback for any that slip
		// through.
		ast.Patch(node, constNode(BlNumber{decimal.NewFromFloat(n.Value)}))
	case *ast.StringNode:
		ast.Patch(node, constNode(BlString{n.Value}))
	case *ast.BoolNode:
		ast.Patch(node, constNode(BlBoolean{n.Value}))
	case *ast.NilNode:
		ast.Patch(node, constNode(Null()))
	case *ast.UnaryNode:
		p.patchUnary(node, n)
	case *ast.BinaryNode:
		p.patchBinary(node, n)
	case *ast.ConditionalNode:
		// Source-level `if C then A else B`: wrap the (BlValue) condition so
		// expr's native conditional sees a Go bool. A null / non-boolean
		// condition is falsy and takes the else branch. (Synthetic conditionals
		// from the and/or lowering are built after this Visit and are never
		// re-walked, so they are untouched.)
		n.Cond = call("__truthy", n.Cond)
	case *ast.MemberNode:
		p.patchMember(node, n)
	case *ast.CallNode:
		p.patchCall(node, n)
	case *ast.ArrayNode:
		// A list literal `[a, b, c]` becomes __mklist(a, b, c) so it produces a
		// BlList rather than expr's native []any.
		ast.Patch(node, call("__mklist", n.Nodes...))
	case *ast.MapNode:
		// A dictionary literal `{k: v, …}` becomes __mkdict("k", v, …) so it
		// produces a BlDictionary rather than expr's native map[string]any.
		var args []ast.Node
		for _, pr := range n.Pairs {
			pair, ok := pr.(*ast.PairNode)
			if !ok {
				continue
			}
			name, ok := stringProperty(pair.Key)
			if !ok {
				continue
			}
			args = append(args, constNode(BlString{name}), pair.Value)
		}
		ast.Patch(node, call("__mkdict", args...))
	}
}

// patchMember lowers dot-component / dictionary-key access (a string property)
// to a single runtime-dispatching componentAccess call. Numeric/other index
// properties are left for list indexing (handled later).
func (p *feelPatcher) patchMember(node *ast.Node, n *ast.MemberNode) {
	if n.Method {
		return
	}
	if name, ok := stringProperty(n.Property); ok {
		ast.Patch(node, call("componentAccess", n.Node, constNode(BlString{name})))
		return
	}
	// Non-string bracket access: a boolean predicate (or one referencing the
	// magic `item`) is a filter; anything else is a 1-based index.
	if isFilterPredicate(n.Property) {
		ast.Patch(node, p.lowerFilter(n.Node, n.Property, false))
		return
	}
	ast.Patch(node, call("__index", n.Node, n.Property))
}

// lowerFilter builds the row filter: __retable(recv, filter(__items(recv),
// {let item = #; __truthy(<pred with bare columns resolved>)})). __retable
// preserves table-ness (sub-table for a table, list for a list). When negate is
// set the predicate is inverted (filterOut).
func (p *feelPatcher) lowerFilter(recv, pred ast.Node, negate bool) ast.Node {
	rewritten := rewriteRowColumns(pred)
	var body ast.Node
	if negate {
		body = call("__truthy", call("__not", rewritten))
	} else {
		body = call("__truthy", rewritten)
	}
	closure := &ast.PredicateNode{Node: &ast.VariableDeclaratorNode{
		Name:  "item",
		Value: &ast.PointerNode{},
		Expr:  body,
	}}
	filtered := &ast.BuiltinNode{Name: "filter", Arguments: []ast.Node{call("__items", recv), closure}}
	return call("__retable", recv, filtered)
}

// isFilterPredicate reports whether a bracket property is a row filter (vs a
// 1-based index): it references `item`, or its top node is a comparison /
// logical operation or a conditional.
func isFilterPredicate(node ast.Node) bool {
	if referencesItem(node) {
		return true
	}
	switch n := node.(type) {
	case *ast.ConditionalNode:
		return true
	case *ast.CallNode:
		if id, ok := n.Callee.(*ast.IdentifierNode); ok {
			return booleanOpNames[id.Value]
		}
	}
	return false
}

var booleanOpNames = map[string]bool{
	"__lt": true, "__le": true, "__gt": true, "__ge": true,
	"__eq": true, "__ne": true, "__blAnd": true, "__blOr": true,
	"__not": true, "__in": true, "__truthy": true,
}

// rewriteRowColumns rewrites bare identifiers (other than `item`) in a
// row-scoped expression to column lookups on the row bound to `item`. Function
// callees and member-access property names are left intact.
func rewriteRowColumns(node ast.Node) ast.Node {
	switch n := node.(type) {
	case *ast.IdentifierNode:
		if n.Value == "item" {
			return n
		}
		return call("componentAccess", ident("item"), constNode(BlString{n.Value}))
	case *ast.CallNode:
		args := make([]ast.Node, len(n.Arguments))
		for i, a := range n.Arguments {
			args[i] = rewriteRowColumns(a)
		}
		return &ast.CallNode{Callee: n.Callee, Arguments: args}
	case *ast.BuiltinNode:
		args := make([]ast.Node, len(n.Arguments))
		for i, a := range n.Arguments {
			args[i] = rewriteRowColumns(a)
		}
		return &ast.BuiltinNode{Name: n.Name, Arguments: args}
	case *ast.ConditionalNode:
		return &ast.ConditionalNode{
			Ternary: n.Ternary,
			Cond:    rewriteRowColumns(n.Cond),
			Exp1:    rewriteRowColumns(n.Exp1),
			Exp2:    rewriteRowColumns(n.Exp2),
		}
	case *ast.MemberNode:
		return &ast.MemberNode{Node: rewriteRowColumns(n.Node), Property: n.Property, Method: n.Method}
	case *ast.ArrayNode:
		nodes := make([]ast.Node, len(n.Nodes))
		for i, e := range n.Nodes {
			nodes[i] = rewriteRowColumns(e)
		}
		return &ast.ArrayNode{Nodes: nodes}
	default:
		return node
	}
}

// referencesItem reports whether the subtree references the magic filter
// variable `item`.
func referencesItem(node ast.Node) bool {
	found := false
	ast.Walk(&node, visitFunc(func(n *ast.Node) {
		if id, ok := (*n).(*ast.IdentifierNode); ok && id.Value == "item" {
			found = true
		}
	}))
	return found
}

// visitFunc adapts a func to ast.Visitor.
type visitFunc func(*ast.Node)

func (f visitFunc) Visit(node *ast.Node) { f(node) }

// tableMethodBackings maps a non-row-scoped table method name to its backing
// dispatch function. Row-scoped methods (filter/filterOut/withColumn/groupBy/agg)
// are handled separately in patchCall.
var tableMethodBackings = map[string]string{
	"select":   "__tableSelect",
	"rename":   "__tableRename",
	"distinct": "__tableDistinct",
	"sort":     "__tableSort",
	"slice":    "__tableSlice",
	"toList":   "__tableToList",
	"toDict":   "__tableToDict",
	"toValue":  "__tableToValue",
	"union":    "union",
	"join":     "join",
}

// patchCall lowers method-call syntax `recv.method(args…)` for table methods to
// the matching backing call with the receiver as the first argument.
func (p *feelPatcher) patchCall(node *ast.Node, n *ast.CallNode) {
	mn, ok := n.Callee.(*ast.MemberNode)
	if !ok {
		return
	}
	name, ok := stringProperty(mn.Property)
	if !ok {
		return
	}
	if backing, ok := tableMethodBackings[name]; ok {
		args := append([]ast.Node{mn.Node}, n.Arguments...)
		ast.Patch(node, call(backing, args...))
		return
	}
	switch name {
	case "filter":
		if len(n.Arguments) == 1 {
			ast.Patch(node, p.lowerFilter(mn.Node, n.Arguments[0], false))
		}
	case "filterOut":
		if len(n.Arguments) == 1 {
			ast.Patch(node, p.lowerFilter(mn.Node, n.Arguments[0], true))
		}
	case "withColumn":
		if len(n.Arguments) == 2 {
			ast.Patch(node, p.lowerWithColumn(mn.Node, n.Arguments[0], n.Arguments[1]))
		}
	case "agg":
		if gb, ok := mn.Node.(*ast.CallNode); ok {
			if gbm, ok := gb.Callee.(*ast.MemberNode); ok {
				if gn, ok := stringProperty(gbm.Property); ok && gn == "groupBy" {
					ast.Patch(node, p.lowerGroupAgg(gbm.Node, gb.Arguments, n.Arguments))
				}
			}
		}
	}
}

// lowerWithColumn builds __withColumn(t, name, map(__items(t), {let item = #;
// <expr with columns resolved>})).
func (p *feelPatcher) lowerWithColumn(recv, nameNode, exprNode ast.Node) ast.Node {
	closure := &ast.PredicateNode{Node: &ast.VariableDeclaratorNode{
		Name:  "item",
		Value: &ast.PointerNode{},
		Expr:  rewriteRowColumns(exprNode),
	}}
	mapped := &ast.BuiltinNode{Name: "map", Arguments: []ast.Node{call("__items", recv), closure}}
	return call("__withColumn", recv, nameNode, mapped)
}

// lowerGroupAgg fuses t.groupBy(keys…).agg(name, expr, …) into a single
// __wraptable(map(__groups(t, [keys]), {let item = #; __mkdict(<keys>, <aggs>)})).
func (p *feelPatcher) lowerGroupAgg(recv ast.Node, keys, aggArgs []ast.Node) ast.Node {
	var dictArgs []ast.Node
	for _, k := range keys {
		dictArgs = append(dictArgs, k, call("__groupKey", ident("item"), k))
	}
	for i := 0; i+1 < len(aggArgs); i += 2 {
		dictArgs = append(dictArgs, aggArgs[i], rewriteRowColumns(aggArgs[i+1]))
	}
	mkdict := call("__mkdict", dictArgs...)
	closure := &ast.PredicateNode{Node: &ast.VariableDeclaratorNode{
		Name:  "item",
		Value: &ast.PointerNode{},
		Expr:  mkdict,
	}}
	groups := call("__groups", recv, call("__mklist", keys...))
	mapped := &ast.BuiltinNode{Name: "map", Arguments: []ast.Node{groups, closure}}
	return call("__wraptable", mapped)
}

// stringProperty extracts a constant string property name from a MemberNode's
// property node (post-patch a ConstantNode{BlString}; pre-patch a StringNode or
// a bare IdentifierNode for `a.b`).
func stringProperty(prop ast.Node) (string, bool) {
	switch p := prop.(type) {
	case *ast.StringNode:
		return p.Value, true
	case *ast.ConstantNode:
		if s, ok := p.Value.(BlString); ok {
			return s.s, true
		}
	case *ast.IdentifierNode:
		return p.Value, true
	}
	return "", false
}

func (p *feelPatcher) patchUnary(node *ast.Node, n *ast.UnaryNode) {
	switch n.Operator {
	case "-":
		ast.Patch(node, call("__neg", n.Node))
	case "not", "!":
		ast.Patch(node, call("__not", n.Node))
	case "+":
		ast.Patch(node, n.Node)
	}
}

func (p *feelPatcher) patchBinary(node *ast.Node, n *ast.BinaryNode) {
	switch n.Operator {
	case "and", "&&":
		ast.Patch(node, p.lowerLogical(n.Left, n.Right, "__isFalse", "__blAnd"))
	case "or", "||":
		ast.Patch(node, p.lowerLogical(n.Left, n.Right, "__isTrue", "__blOr"))
	case "in":
		// x in y → __in(x, y): list membership / range containment.
		ast.Patch(node, call("__in", n.Left, n.Right))
	default:
		if fn, ok := binaryDispatch[n.Operator]; ok {
			ast.Patch(node, call(fn, n.Left, n.Right))
		}
	}
}

// lowerLogical builds the short-circuit three-valued lowering for `a and b` /
// `a or b`:
//
//	let v = a; (guard(v) ? __val(v) : combine(v, b))
//
// where guard returns a Go bool driving expr's native conditional, so the
// right operand and the combine call run only in the non-short path.
func (p *feelPatcher) lowerLogical(left, right ast.Node, guardFn, combineFn string) ast.Node {
	p.counter++
	name := fmt.Sprintf("__sc%d", p.counter)
	cond := call(guardFn, ident(name))
	exp1 := call("__val", ident(name))
	exp2 := call(combineFn, ident(name), right)
	conditional := &ast.ConditionalNode{
		Ternary: true,
		Cond:    cond,
		Exp1:    exp1,
		Exp2:    exp2,
	}
	return &ast.VariableDeclaratorNode{
		Name:  name,
		Value: left,
		Expr:  conditional,
	}
}

// --- node constructors ----------------------------------------------------

func ident(name string) ast.Node { return &ast.IdentifierNode{Value: name} }

func constNode(v any) ast.Node { return &ast.ConstantNode{Value: v} }

func call(fn string, args ...ast.Node) ast.Node {
	return &ast.CallNode{Callee: ident(fn), Arguments: args}
}
