package blkit

import (
	"fmt"

	"github.com/expr-lang/expr/ast"
	"github.com/expr-lang/expr/parser"
)

// checkUndefinedVars enforces the spec rule that, when a schema is supplied, a
// reference to a name that is neither declared nor a function/keyword is a
// parse error. expr's map-typed env permits arbitrary keys, so this is done
// here by walking the parsed (normalised) AST and validating value-position
// identifiers against the schema's top-level field names.
//
// It is conservative: a function callee, a member-access property, and any
// let-bound name are skipped, and only clearly-undeclared value identifiers are
// flagged — uncertain cases are allowed so valid expressions never falsely
// fail.
func checkUndefinedVars(normalisedSource string, schema BlSchema) error {
	tree, err := parser.Parse(normalisedSource)
	if err != nil {
		return nil // the real compile pass will surface the parse error
	}
	declared := map[string]bool{}
	for _, f := range schema {
		declared[f.Name] = true
	}
	bound := map[string]bool{inputPlaceholder: true, "$env": true}
	var bad string
	collectVarIdents(tree.Node, declared, bound, &bad)
	if bad != "" {
		return fmt.Errorf("unknown name %s", bad)
	}
	return nil
}

// collectVarIdents walks the AST recording the first value-position identifier
// not present in declared or bound. Function callees and member properties are
// not treated as variable references.
func collectVarIdents(node ast.Node, declared, bound map[string]bool, bad *string) {
	if node == nil || *bad != "" {
		return
	}
	switch n := node.(type) {
	case *ast.IdentifierNode:
		if !declared[n.Value] && !bound[n.Value] {
			*bad = n.Value
		}
	case *ast.UnaryNode:
		collectVarIdents(n.Node, declared, bound, bad)
	case *ast.BinaryNode:
		collectVarIdents(n.Left, declared, bound, bad)
		collectVarIdents(n.Right, declared, bound, bad)
	case *ast.MemberNode:
		// Receiver is a value reference; the property name is not.
		collectVarIdents(n.Node, declared, bound, bad)
		// A non-string bracket property is an index or a filter predicate; in
		// the filter form the magic `item` variable is bound.
		if _, isStr := stringProperty(n.Property); !isStr {
			inner := map[string]bool{"item": true}
			for k := range bound {
				inner[k] = true
			}
			collectVarIdents(n.Property, declared, inner, bad)
		}
	case *ast.ChainNode:
		collectVarIdents(n.Node, declared, bound, bad)
	case *ast.SliceNode:
		collectVarIdents(n.Node, declared, bound, bad)
		collectVarIdents(n.From, declared, bound, bad)
		collectVarIdents(n.To, declared, bound, bad)
	case *ast.CallNode:
		// The callee is a function name, not a variable — skip it unless it is
		// a complex (non-identifier) expression.
		if _, ok := n.Callee.(*ast.IdentifierNode); !ok {
			collectVarIdents(n.Callee, declared, bound, bad)
		}
		for _, a := range n.Arguments {
			collectVarIdents(a, declared, bound, bad)
		}
	case *ast.BuiltinNode:
		for _, a := range n.Arguments {
			collectVarIdents(a, declared, bound, bad)
		}
	case *ast.ConditionalNode:
		collectVarIdents(n.Cond, declared, bound, bad)
		collectVarIdents(n.Exp1, declared, bound, bad)
		collectVarIdents(n.Exp2, declared, bound, bad)
	case *ast.VariableDeclaratorNode:
		collectVarIdents(n.Value, declared, bound, bad)
		inner := map[string]bool{n.Name: true}
		for k := range bound {
			inner[k] = true
		}
		collectVarIdents(n.Expr, declared, inner, bad)
	case *ast.ArrayNode:
		for _, e := range n.Nodes {
			collectVarIdents(e, declared, bound, bad)
		}
	case *ast.MapNode:
		// A dictionary literal's keys are in scope for its value expressions
		// (forward-references), so bind them before walking the values.
		inner := map[string]bool{}
		for k := range bound {
			inner[k] = true
		}
		for _, p := range n.Pairs {
			if pair, ok := p.(*ast.PairNode); ok {
				if name, ok := stringProperty(pair.Key); ok {
					inner[name] = true
				}
			}
		}
		for _, p := range n.Pairs {
			if pair, ok := p.(*ast.PairNode); ok {
				collectVarIdents(pair.Value, declared, inner, bad)
			}
		}
	}
}
