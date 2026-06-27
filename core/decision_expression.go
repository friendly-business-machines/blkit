package core

import (
	"fmt"
	"reflect"
	"strconv"
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

// DecisionExpression defines decision logic as named text-expression entries
// over a typed input struct I and output struct O. Each entry binds one output
// to a bl-expression that may reference any declared input or sibling output by
// name. Entries are compiled and topologically sorted at construction;
// Evaluate(inputs I) walks them in dependency order and returns O.
//
// Inputs and outputs are concrete Go structs, so a caller passing the wrong
// input shape or reading a non-existent output is a Go compile error. The two
// structs are joined at construction into a single combined env type (built with
// reflect.StructOf, since Go forbids embedding type parameters) against which
// every entry is type-checked.
type DecisionExpression[I, O any] struct {
	id          string
	name        string
	description string

	entries Entries // original raw sources, keyed by output name
	funcs   []UDF   // user-defined functions entries may call, for rendering

	combinedType reflect.Type // env type joining I's and O's fields
	inputCopy    []fieldCopy  // I field -> combined field
	outputCopy   []fieldCopy  // combined field -> O field
	inputOrder   []string     // input names in I declaration order
	outputOrder  []string     // output names in O declaration order

	evalPlan []compiledEntry // entries in topological evaluation order
}

// fieldCopy records that combined-env field [combined] mirrors source-struct
// field [src].
type fieldCopy struct{ combined, src int }

// compiledEntry is one entry's compiled program plus the metadata Evaluate and
// the topological sort need.
type compiledEntry struct {
	output        string      // the output name this entry binds
	combinedField int         // index of that output in the combined env type
	program       *vm.Program // compiled source
	deps          []string    // sibling-output names this entry references
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
		node = "DecisionExpression"
	}
	return node + ": " + strings.Join(e.Problems, "; ")
}

// decisionFieldName resolves and validates the env variable name for one field of
// a decision contract struct. Every field participates: the tag is optional and is
// used only to rename a field's variable, so an untagged field is exposed under its
// Go field name and an `expr:"name"` tag renames it to name. The field must be
// exported, must not opt out with `expr:"-"`, and the resolved name must be a valid
// expr identifier. A non-empty problem string describes the first rule it breaks.
func decisionFieldName(f reflect.StructField) (name, problem string) {
	if !f.IsExported() {
		return "", fmt.Sprintf("unexported field %q; every contract field must be an exported BlValue", f.Name)
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
// a variable, exposed under its Go field name or the optional `expr:"name"` rename.
// It validates the contracts (every field is a BlValue under a valid expr-identifier
// name, no duplicate or input/output name collisions, at least one output, the entry
// keys are exactly the output names, no two Funcs share a name),
// compiles every entry against the combined env — with config.Funcs registered so
// entries may call them by name — and topologically sorts by inter-entry
// dependencies. It accumulates every problem and panics once with a
// *DecisionDefinitionError.
func NewDecisionExpression[I, O any](config DecisionExpressionConfig) *DecisionExpression[I, O] {
	var problems []string
	add := func(format string, args ...any) {
		problems = append(problems, fmt.Sprintf(format, args...))
	}
	fail := func() { panic(&DecisionDefinitionError{Node: nodeLabel(config), Problems: problems}) }

	iType := reflect.TypeOf((*I)(nil)).Elem()
	oType := reflect.TypeOf((*O)(nil)).Elem()
	if iType.Kind() != reflect.Struct {
		add("input type %s is not a struct", iType)
	}
	if oType.Kind() != reflect.Struct {
		add("output type %s is not a struct", oType)
	}
	if len(problems) > 0 {
		fail()
	}

	// Inputs and outputs may carry only BlValues, since entries operate on and
	// produce BlValues. Go's generics cannot constrain a struct's field types, so
	// this is enforced here, by reflection, at construction.
	blValueType := reflect.TypeOf((*BlValue)(nil)).Elem()

	// Join I's and O's exported fields into one combined env type. Inputs come
	// first, then outputs; each combined field carries the source field's expr
	// tag so entries reference it by its expr name.
	var combinedFields []reflect.StructField
	var inputCopy, outputCopy []fieldCopy
	var inputOrder, outputOrder []string
	outputByTag := map[string]int{} // output name -> combined field index
	seen := map[string]bool{}

	addField := func(srcType reflect.Type, srcIdx int, tag string) int {
		ci := len(combinedFields)
		combinedFields = append(combinedFields, reflect.StructField{
			Name: "F" + strconv.Itoa(ci),
			Type: srcType,
			Tag:  reflect.StructTag(`expr:"` + tag + `"`),
		})
		return ci
	}

	for i := 0; i < iType.NumField(); i++ {
		f := iType.Field(i)
		name, problem := decisionFieldName(f)
		if problem != "" {
			add("input %s", problem)
			continue
		}
		if !f.Type.Implements(blValueType) {
			add("input field %q (%s) is not a BlValue", name, f.Type)
			continue
		}
		if seen[name] {
			add("duplicate input variable name %q", name)
			continue
		}
		seen[name] = true
		ci := addField(f.Type, i, name)
		inputCopy = append(inputCopy, fieldCopy{combined: ci, src: i})
		inputOrder = append(inputOrder, name)
	}
	for j := 0; j < oType.NumField(); j++ {
		f := oType.Field(j)
		name, problem := decisionFieldName(f)
		if problem != "" {
			add("output %s", problem)
			continue
		}
		if !f.Type.Implements(blValueType) {
			add("output field %q (%s) is not a BlValue", name, f.Type)
			continue
		}
		if seen[name] {
			add("output name %q collides with an input or another output", name)
			continue
		}
		seen[name] = true
		ci := addField(f.Type, j, name)
		outputCopy = append(outputCopy, fieldCopy{combined: ci, src: j})
		outputByTag[name] = ci
		outputOrder = append(outputOrder, name)
	}
	if len(outputOrder) == 0 {
		add("at least one output field is required")
	}

	// The entry keys must be exactly the output names.
	for key := range config.Entries {
		if _, ok := outputByTag[key]; !ok {
			add("entry %q has no matching output field", key)
		}
	}
	for _, name := range outputOrder {
		if _, ok := config.Entries[name]; !ok {
			add("output %q has no entry", name)
		}
	}
	if len(problems) > 0 {
		fail()
	}

	combinedType := reflect.StructOf(combinedFields)
	combinedZero := reflect.New(combinedType).Elem().Interface()

	// Register the user-defined functions so entries may call them by name. A
	// duplicate function name is a construction error; without valid registrations
	// the per-entry compiles below would be meaningless, so fail at this barrier.
	udfOpts, uerr := udfOptions(config.Funcs)
	if uerr != nil {
		add("%v", uerr)
		fail()
	}

	// Compile each entry against the combined env (a strict struct env, so any
	// reference to a name that is neither a declared input nor a sibling output
	// is a compile error) plus the registered UDFs, and discover its sibling-output
	// dependencies.
	var entries []compiledEntry
	for _, name := range outputOrder {
		src := config.Entries[name]
		program, err := compileWithEnv(src, combinedZero, seen, udfOpts...)
		if err != nil {
			add("entry %q: %v", name, err)
			continue
		}
		deps, err := siblingDeps(src, outputByTag)
		if err != nil {
			add("entry %q: %v", name, err)
			continue
		}
		entries = append(entries, compiledEntry{
			output:        name,
			combinedField: outputByTag[name],
			program:       program,
			deps:          deps,
		})
	}
	if len(problems) > 0 {
		fail()
	}

	plan, err := topoSortEntries(entries, outputOrder)
	if err != nil {
		add("%v", err)
		fail()
	}

	return &DecisionExpression[I, O]{
		id:           config.Id,
		name:         config.Name,
		description:  config.Description,
		entries:      cloneEntries(config.Entries),
		funcs:        append([]UDF(nil), config.Funcs...),
		combinedType: combinedType,
		inputCopy:    inputCopy,
		outputCopy:   outputCopy,
		inputOrder:   inputOrder,
		outputOrder:  outputOrder,
		evalPlan:     plan,
	}
}

// Evaluate runs the entries against the input variables in topological order and
// returns the produced outputs. An output value whose runtime type disagrees with
// its declared output field is a bl.TypeError.
func (d *DecisionExpression[I, O]) Evaluate(inputs I) (O, error) {
	var out O
	combined := reflect.New(d.combinedType).Elem()
	inv := reflect.ValueOf(inputs)
	for _, c := range d.inputCopy {
		combined.Field(c.combined).Set(inv.Field(c.src))
	}
	for _, step := range d.evalPlan {
		raw, err := expr.Run(step.program, combined.Interface())
		if err != nil {
			return out, &TypeError{Op: "evaluate", Detail: err.Error()}
		}
		if err := setBlField(combined.Field(step.combinedField), asBl(raw)); err != nil {
			return out, err
		}
	}
	outv := reflect.ValueOf(&out).Elem()
	for _, c := range d.outputCopy {
		outv.Field(c.src).Set(combined.Field(c.combined))
	}
	return out, nil
}

// setBlField writes a produced BlValue into a typed output field, turning a
// reflect type-mismatch panic into a bl.TypeError.
func setBlField(field reflect.Value, v BlValue) (err error) {
	defer func() {
		if r := recover(); r != nil {
			err = &TypeError{Op: "evaluate", Detail: "output type mismatch"}
		}
	}()
	field.Set(reflect.ValueOf(v))
	return nil
}

// GetId returns the node's identifier.
func (d *DecisionExpression[I, O]) GetId() string { return d.id }

// GetName returns the node's display name.
func (d *DecisionExpression[I, O]) GetName() string { return d.name }

// GetDescription returns the node's description.
func (d *DecisionExpression[I, O]) GetDescription() string { return d.description }

// Source returns the original raw source for an output name.
func (d *DecisionExpression[I, O]) Source(output string) (string, bool) {
	s, ok := d.entries[output]
	return s, ok
}

// ToMarkdown renders the node as markdown: the input variables (listed, not
// tabulated), then one table of name/expression rows — first any user-defined
// functions, each shown by its call signature (e.g. addTax(amount)) and body, then
// the entries (output name and source expression) in output declaration order.
func (d *DecisionExpression[I, O]) ToMarkdown() string {
	var b strings.Builder
	if d.name != "" {
		b.WriteString("### " + d.name + "\n\n")
	}
	if len(d.inputOrder) > 0 {
		b.WriteString("**Inputs:** " + strings.Join(d.inputOrder, ", ") + "\n\n")
	}

	rows := make([][2]string, 0, len(d.funcs)+len(d.outputOrder))
	for _, u := range d.funcs {
		signature := u.udfName() + "(" + strings.Join(u.udfParams(), ", ") + ")"
		rows = append(rows, [2]string{signature, u.udfSource()})
	}
	for _, name := range d.outputOrder {
		rows = append(rows, [2]string{name, d.entries[name]})
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
func siblingDeps(source string, outputByTag map[string]int) ([]string, error) {
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
		if _, ok := outputByTag[name]; ok {
			deps = append(deps, name)
		}
	}
	return deps, nil
}

// topoSortEntries orders entries so each comes after the sibling outputs it
// depends on. A self-reference or a dependency cycle is an error. Zero-dependency
// entries keep output-declaration order.
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

func nodeLabel(c DecisionExpressionConfig) string {
	if c.Id != "" {
		return c.Id
	}
	if c.Name != "" {
		return c.Name
	}
	return "DecisionExpression"
}
