package core

import (
	"reflect"

	"github.com/expr-lang/expr/ast"
	"github.com/expr-lang/expr/parser"
)

// exprTagName returns the variable name expr exposes for a struct field: the
// `expr:"name"` tag value, the field name when untagged, or ("", false) for
// `expr:"-"`.
func exprTagName(f reflect.StructField) (string, bool) {
	switch tag := f.Tag.Get("expr"); tag {
	case "-":
		return "", false
	case "":
		return f.Name, true
	default:
		return tag, true
	}
}

// envFieldNames returns the set of variable names a struct env exposes: the
// expr-tag name of each exported field.
func envFieldNames(t reflect.Type) map[string]bool {
	out := map[string]bool{}
	if t.Kind() != reflect.Struct {
		return out
	}
	for i := 0; i < t.NumField(); i++ {
		f := t.Field(i)
		if !f.IsExported() {
			continue
		}
		if name, ok := exprTagName(f); ok {
			out[name] = true
		}
	}
	return out
}

// firstUndefined reports the first value-position identifier in a parsed,
// normalised expression that is not a declared variable. blkit's operator and
// function overloads make expr's own strict checker lenient about undefined
// names in operand/argument position, so blkit validates them itself: every free
// identifier must be a declared env field. Function callees, member-access
// property names, and names bound by let/filter/loop/dictionary-key forms are not
// variables and are exempt.
func firstUndefined(normalisedSource string, declared map[string]bool) (string, bool) {
	tree, err := parser.Parse(normalisedSource)
	if err != nil {
		return "", false // the compile pass surfaces the real parse error
	}
	free := map[string]bool{}
	freeIdents(tree.Node, map[string]bool{}, free)
	for name := range free {
		if !declared[name] {
			return name, true
		}
	}
	return "", false
}

// freeIdents collects every free value-position identifier in node into out. It
// skips function callees and member-access property names, and treats names
// bound by let, filter `item`, loop, and dictionary-key forms as bound. It is
// conservative: uncertain forms are left uncollected so a valid expression is
// never falsely rejected.
func freeIdents(node ast.Node, bound, out map[string]bool) {
	if node == nil {
		return
	}
	switch n := node.(type) {
	case *ast.IdentifierNode:
		if !bound[n.Value] {
			out[n.Value] = true
		}
	case *ast.UnaryNode:
		freeIdents(n.Node, bound, out)
	case *ast.BinaryNode:
		freeIdents(n.Left, bound, out)
		freeIdents(n.Right, bound, out)
	case *ast.MemberNode:
		// The receiver is a value reference; a string property is a field name.
		// A non-string bracket property is an index or a row/element-scoped
		// predicate (table columns, filter `item`) that the patcher resolves
		// rather than the env — skip it so a column name is never read as an
		// undefined variable.
		freeIdents(n.Node, bound, out)
	case *ast.ChainNode:
		freeIdents(n.Node, bound, out)
	case *ast.SliceNode:
		freeIdents(n.Node, bound, out)
		freeIdents(n.From, bound, out)
		freeIdents(n.To, bound, out)
	case *ast.CallNode:
		if _, ok := n.Callee.(*ast.IdentifierNode); ok {
			// Plain function call: the callee is a function name; the arguments
			// are ordinary value expressions.
			for _, a := range n.Arguments {
				freeIdents(a, bound, out)
			}
		} else {
			// Method call (receiver.method(args)) or a computed callee: the
			// arguments may be row/element-scoped (table column expressions)
			// resolved by the patcher, so check only the receiver, not the args.
			freeIdents(n.Callee, bound, out)
		}
	case *ast.BuiltinNode:
		for _, a := range n.Arguments {
			freeIdents(a, bound, out)
		}
	case *ast.ConditionalNode:
		freeIdents(n.Cond, bound, out)
		freeIdents(n.Exp1, bound, out)
		freeIdents(n.Exp2, bound, out)
	case *ast.VariableDeclaratorNode:
		freeIdents(n.Value, bound, out)
		freeIdents(n.Expr, withBound(bound, n.Name), out)
	case *ast.ArrayNode:
		for _, e := range n.Nodes {
			freeIdents(e, bound, out)
		}
	case *ast.MapNode:
		// A dictionary literal's keys are in scope for its value expressions
		// (forward-references), so bind them before walking the values.
		inner := withBound(bound)
		for _, p := range n.Pairs {
			if pair, ok := p.(*ast.PairNode); ok {
				if name, ok := stringProperty(pair.Key); ok {
					inner[name] = true
				}
			}
		}
		for _, p := range n.Pairs {
			if pair, ok := p.(*ast.PairNode); ok {
				freeIdents(pair.Value, inner, out)
			}
		}
	}
}

// withBound returns a copy of bound with the given extra names added.
func withBound(bound map[string]bool, names ...string) map[string]bool {
	out := make(map[string]bool, len(bound)+len(names))
	for k := range bound {
		out[k] = true
	}
	for _, n := range names {
		out[n] = true
	}
	return out
}
