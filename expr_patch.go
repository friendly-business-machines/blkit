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
	}
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
