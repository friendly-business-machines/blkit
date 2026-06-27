package core

import (
	"fmt"
	"reflect"
	"strings"

	"github.com/expr-lang/expr"
	"github.com/expr-lang/expr/parser"
	"github.com/expr-lang/expr/vm"
)

// Entries maps each output name to the raw-string source expression that
// produces it. The key is the output's variable name (its Go field name, or its
// `expr:"name"` rename) — the name entries reference — matching a field of O.
type Entries map[string]string

// DecisionExpressionConfig configures a DecisionExpression. The input and output
// contracts are the type parameters I and O of NewDecisionExpression; the config
// carries identity and the entry sources.
type DecisionExpressionConfig struct {
	Id          string
	Name        string
	Description string

	// Entries maps each output name to its source expression. The key set must
	// be exactly the set of output names (O's expr-tag field names).
	Entries Entries

	// Funcs are user-defined functions (see Func) that every entry may call by
	// name, with compile-time-checked arguments. Two Funcs sharing a name is a
	// construction error.
	Funcs []UDF
}

// DecisionExpression defines decision logic as named text-expression entries over
// a typed input struct I and output struct O whose fields are Handle values. Each
// entry binds one output to a bl-expression that may reference any declared input
// or sibling output by name. Entries are compiled and topologically sorted at
// construction; Evaluate(inputs I) walks them in dependency order and returns O.
type DecisionExpression[I, O any] struct {
	In  I // input port surface (stamped handles for wiring)
	Out O // output port surface

	id          string
	name        string
	description string

	entries Entries // original raw sources, keyed by output name
	funcs   []UDF   // user-defined functions entries may call, for rendering

	ivars, ovars []decisionVar
	envT         reflect.Type   // combined env joining I's and O's variables (BlValue fields)
	envIndex     map[string]int // variable name -> combined env field index
	evalPlan     []compiledEntry
}

// compiledEntry is one entry's compiled program plus the metadata Evaluate and
// the topological sort need.
type compiledEntry struct {
	output   string      // the output name this entry binds
	envField int         // index of that output in the combined env type
	program  *vm.Program // compiled source
	deps     []string    // sibling-output names this entry references
}

// DecisionDefinitionError reports one or more problems found while constructing a
// decision node. Following the decision-family convention, the constructor
// accumulates every problem and panics once with this error, so a malformed
// package-scope node fails fast at program (or test) startup.
type DecisionDefinitionError struct {
	Node     string
	Problems []string
}

func (e *DecisionDefinitionError) Error() string {
	node := e.Node
	if node == "" {
		node = "Decision"
	}
	return node + ": " + strings.Join(e.Problems, "; ")
}

// decisionFieldName resolves and validates the env variable name for one field of
// a decision contract struct. The field must be exported, must not opt out with
// `expr:"-"`, and the resolved name (its Go field name, or its `expr:"name"`
// rename) must be a valid expr identifier. A non-empty problem string describes
// the first rule it breaks.
func decisionFieldName(f reflect.StructField) (name, problem string) {
	if !f.IsExported() {
		return "", fmt.Sprintf("unexported field %q; every contract field must be an exported Handle", f.Name)
	}
	name = f.Name
	switch tag := f.Tag.Get("expr"); tag {
	case "":
		// untagged: keep the Go field name
	case "-":
		return "", fmt.Sprintf(`field %q uses expr:"-"; fields cannot be excluded — every contract field is a variable`, f.Name)
	default:
		name = tag
	}
	if !isIdentifier(name) {
		return "", fmt.Sprintf("variable name %q (field %q) is not a valid expr identifier", name, f.Name)
	}
	return name, ""
}

// NewDecisionExpression builds a DecisionExpression from the typed input struct I,
// output struct O, and the configured entries. Every exported field of I and O is
// a Handle variable, exposed under its Go field name or the optional `expr:"name"`
// rename. It validates the contracts, compiles every entry against the combined
// env (with config.Funcs registered), and topologically sorts by inter-entry
// dependencies. It accumulates every problem and panics once with a
// *DecisionDefinitionError.
func NewDecisionExpression[I, O any](config DecisionExpressionConfig) *DecisionExpression[I, O] {
	var problems []string
	add := func(format string, args ...any) {
		problems = append(problems, fmt.Sprintf(format, args...))
	}
	fail := func() {
		panic(&DecisionDefinitionError{Node: nodeLabel(config.Id, config.Name, "DecisionExpression"), Problems: problems})
	}

	iType := reflect.TypeOf((*I)(nil)).Elem()
	oType := reflect.TypeOf((*O)(nil)).Elem()

	ivars, ip := reflectContract(iType)
	for _, p := range ip {
		add("input %s", p)
	}
	ovars, op := reflectContract(oType)
	for _, p := range op {
		add("output %s", p)
	}
	if len(ovars) == 0 {
		add("at least one output field is required")
	}

	// Inputs and sibling outputs share one combined env, so a name may not appear
	// in both.
	inNames := map[string]bool{}
	for _, v := range ivars {
		inNames[v.name] = true
	}
	for _, v := range ovars {
		if inNames[v.name] {
			add("output name %q collides with an input", v.name)
		}
	}
	if len(problems) > 0 {
		fail()
	}

	combined := append(append([]decisionVar{}, ivars...), ovars...)
	envT, declared := envType(combined)
	envIndex := make(map[string]int, len(combined))
	for i, v := range combined {
		envIndex[v.name] = i
	}
	outputByName := map[string]bool{}
	for _, v := range ovars {
		outputByName[v.name] = true
	}

	// The entry keys must be exactly the output names.
	for key := range config.Entries {
		if !outputByName[key] {
			add("entry %q has no matching output field", key)
		}
	}
	for _, v := range ovars {
		if _, ok := config.Entries[v.name]; !ok {
			add("output %q has no entry", v.name)
		}
	}
	if len(problems) > 0 {
		fail()
	}

	udfOpts, uerr := udfOptions(config.Funcs)
	if uerr != nil {
		add("%v", uerr)
		fail()
	}

	envZero := reflect.New(envT).Elem().Interface()
	var entries []compiledEntry
	for _, v := range ovars {
		src := config.Entries[v.name]
		program, err := compileWithEnv(src, envZero, declared, udfOpts...)
		if err != nil {
			add("entry %q: %v", v.name, err)
			continue
		}
		deps, err := siblingDeps(src, outputByName)
		if err != nil {
			add("entry %q: %v", v.name, err)
			continue
		}
		entries = append(entries, compiledEntry{output: v.name, envField: envIndex[v.name], program: program, deps: deps})
	}
	if len(problems) > 0 {
		fail()
	}

	var outputOrder []string
	for _, v := range ovars {
		outputOrder = append(outputOrder, v.name)
	}
	plan, err := topoSortEntries(entries, outputOrder)
	if err != nil {
		add("%v", err)
		fail()
	}

	d := &DecisionExpression[I, O]{
		id:          config.Id,
		name:        config.Name,
		description: config.Description,
		entries:     cloneEntries(config.Entries),
		funcs:       append([]UDF(nil), config.Funcs...),
		ivars:       ivars,
		ovars:       ovars,
		envT:        envT,
		envIndex:    envIndex,
		evalPlan:    plan,
	}
	d.In = stampSurface(d, d.id, iType, ivars).Interface().(I)
	d.Out = stampSurface(d, d.id, oType, ovars).Interface().(O)
	return d
}

// Evaluate runs the entries against the input variables in topological order and
// returns the produced outputs. An output value whose runtime type disagrees with
// its declared output field is a bl.TypeError.
func (d *DecisionExpression[I, O]) Evaluate(inputs I) (O, error) {
	var out O
	env := reflect.New(d.envT).Elem()
	iv := reflect.ValueOf(inputs)
	for _, v := range d.ivars {
		val := iv.Field(v.field).Interface().(roHandle).getValue()
		if val != nil && reflect.TypeOf(val).AssignableTo(v.elem) {
			env.Field(d.envIndex[v.name]).Set(reflect.ValueOf(val))
		}
	}
	for _, step := range d.evalPlan {
		raw, err := expr.Run(step.program, env.Interface())
		if err != nil {
			return out, &TypeError{Op: "evaluate", Detail: err.Error()}
		}
		if err := setBlField(env.Field(step.envField), asBl(raw)); err != nil {
			return out, err
		}
	}
	ov := reflect.ValueOf(&out).Elem()
	for _, v := range d.ovars {
		val := env.Field(d.envIndex[v.name]).Interface().(BlValue)
		if err := ov.Field(v.field).Addr().Interface().(anyHandle).setValue(val); err != nil {
			return out, err
		}
	}
	return out, nil
}

// setBlField writes a produced BlValue into a typed env field, turning a reflect
// type-mismatch panic into a bl.TypeError.
func setBlField(field reflect.Value, v BlValue) (err error) {
	defer func() {
		if r := recover(); r != nil {
			err = &TypeError{Op: "evaluate", Detail: "output type mismatch"}
		}
	}()
	field.Set(reflect.ValueOf(v))
	return nil
}

// DecisionNode[I, O] interface satisfaction.
func (d *DecisionExpression[I, O]) GetId() string          { return d.id }
func (d *DecisionExpression[I, O]) GetName() string        { return d.name }
func (d *DecisionExpression[I, O]) GetDescription() string { return d.description }
func (d *DecisionExpression[I, O]) Inputs() []Field        { return fieldsOf(d.ivars) }
func (d *DecisionExpression[I, O]) Outputs() []Field       { return fieldsOf(d.ovars) }

// erased decisionNode satisfaction.
func (d *DecisionExpression[I, O]) inVars() []decisionVar  { return d.ivars }
func (d *DecisionExpression[I, O]) outVars() []decisionVar { return d.ovars }
func (d *DecisionExpression[I, O]) concurrent() bool       { return false }

func (d *DecisionExpression[I, O]) runErased(in map[string]BlValue) (map[string]BlValue, error) {
	input, err := inputFromMap(reflect.TypeOf((*I)(nil)).Elem(), d.ivars, in)
	if err != nil {
		return nil, err
	}
	out, err := d.Evaluate(input.(I))
	if err != nil {
		return nil, err
	}
	return outputToMap(out, d.ovars), nil
}

// Source returns the original raw source for an output name.
func (d *DecisionExpression[I, O]) Source(output string) (string, bool) {
	s, ok := d.entries[output]
	return s, ok
}

// ToMarkdown renders the node as markdown: the input variables (listed, not
// tabulated), then one table of name/expression rows — first any user-defined
// functions, then the entries in output declaration order.
func (d *DecisionExpression[I, O]) ToMarkdown() string {
	var b strings.Builder
	if d.name != "" {
		b.WriteString("### " + d.name + "\n\n")
	}
	if len(d.ivars) > 0 {
		names := make([]string, len(d.ivars))
		for i, v := range d.ivars {
			names[i] = v.name
		}
		b.WriteString("**Inputs:** " + strings.Join(names, ", ") + "\n\n")
	}
	rows := make([][2]string, 0, len(d.funcs)+len(d.ovars))
	for _, u := range d.funcs {
		signature := u.udfName() + "(" + strings.Join(u.udfParams(), ", ") + ")"
		rows = append(rows, [2]string{signature, u.udfSource()})
	}
	for _, v := range d.ovars {
		rows = append(rows, [2]string{v.name, d.entries[v.name]})
	}
	mdTable(&b, "Name", "Expression", rows)
	return b.String()
}

// mdTable writes a two-column markdown table with the given headers and rows,
// padding each column to the width of its widest cell.
func mdTable(b *strings.Builder, h1, h2 string, rows [][2]string) {
	w1, w2 := len(h1), len(h2)
	for _, r := range rows {
		if len(r[0]) > w1 {
			w1 = len(r[0])
		}
		if len(r[1]) > w2 {
			w2 = len(r[1])
		}
	}
	line := func(a, c string) {
		b.WriteString("| " + pad(a, w1) + " | " + pad(c, w2) + " |\n")
	}
	line(h1, h2)
	b.WriteString("|" + strings.Repeat("-", w1+2) + "|" + strings.Repeat("-", w2+2) + "|\n")
	for _, r := range rows {
		line(r[0], r[1])
	}
}

func pad(s string, w int) string {
	if len(s) >= w {
		return s
	}
	return s + strings.Repeat(" ", w-len(s))
}

// siblingDeps returns the sibling-output names an entry source references, used
// to order the evaluation plan.
func siblingDeps(source string, outputByName map[string]bool) ([]string, error) {
	src, err := normalise(source)
	if err != nil {
		return nil, err
	}
	tree, err := parser.Parse(src)
	if err != nil {
		return nil, err
	}
	free := map[string]bool{}
	freeIdents(tree.Node, map[string]bool{}, free)
	var deps []string
	for name := range free {
		if outputByName[name] {
			deps = append(deps, name)
		}
	}
	return deps, nil
}

// topoSortEntries orders entries so each comes after the sibling outputs it
// depends on. A self-reference or a dependency cycle is an error.
func topoSortEntries(entries []compiledEntry, order []string) ([]compiledEntry, error) {
	byName := make(map[string]compiledEntry, len(entries))
	indeg := make(map[string]int, len(entries))
	for _, e := range entries {
		byName[e.output] = e
		indeg[e.output] = 0
	}
	dependents := map[string][]string{}
	for _, e := range entries {
		seen := map[string]bool{}
		for _, d := range e.deps {
			if d == e.output {
				return nil, fmt.Errorf("entry %q references itself", e.output)
			}
			if seen[d] {
				continue
			}
			seen[d] = true
			indeg[e.output]++
			dependents[d] = append(dependents[d], e.output)
		}
	}
	var queue []string
	for _, name := range order {
		if indeg[name] == 0 {
			queue = append(queue, name)
		}
	}
	var out []compiledEntry
	for len(queue) > 0 {
		n := queue[0]
		queue = queue[1:]
		out = append(out, byName[n])
		for _, m := range dependents[n] {
			indeg[m]--
			if indeg[m] == 0 {
				queue = append(queue, m)
			}
		}
	}
	if len(out) != len(entries) {
		return nil, fmt.Errorf("dependency cycle among entries")
	}
	return out, nil
}

func cloneEntries(e Entries) Entries {
	out := make(Entries, len(e))
	for k, v := range e {
		out[k] = v
	}
	return out
}

// nodeLabel picks a human label for a decision node's errors.
func nodeLabel(id, name, fallback string) string {
	if id != "" {
		return id
	}
	if name != "" {
		return name
	}
	return fallback
}
