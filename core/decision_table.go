package core

import (
	"fmt"
	"reflect"
	"strings"

	"github.com/expr-lang/expr"
	"github.com/expr-lang/expr/vm"
	"github.com/shopspring/decimal"
)

// HitPolicy determines how the engine combines multiple matching rules.
type HitPolicy int

const (
	HitPolicyUnique      HitPolicy = iota // exactly one rule may match; multiple → error
	HitPolicyFirst                        // rules in order; first match wins
	HitPolicyPriority                     // among matches, the earliest (highest priority) wins
	HitPolicyAny                          // multiple may match but must produce identical outputs
	HitPolicyCollect                      // all matches collected; combined via Aggregation
	HitPolicyRuleOrder                    // all matches returned in rule declaration order
	HitPolicyOutputOrder                  // all matches returned in rule order (output priority)
)

// Aggregation combines the matched values of a Collect table.
type Aggregation int

const (
	AggregationSum   Aggregation = iota // numeric sum of all matching output values
	AggregationMin                      // minimum numeric value
	AggregationMax                      // maximum numeric value
	AggregationCount                    // number of matching rules
)

// Column is a labelled, typed input column. Expr is a source expression over the
// variables of I, inlined as the `?` subject of each input cell. Type bounds the
// valid unary-test forms for the column.
type Column struct {
	Label string
	Expr  string
	Type  Type
}

// Rule is one row: optional leading id, then input cells (unary tests), then
// output cells (expressions), in column / output order.
type Rule []string

// Rules is a table's rows.
type Rules []Rule

// DecisionTableConfig configures a DecisionTable. The input/output contracts are
// the type parameters I and O.
type DecisionTableConfig struct {
	Id          string
	Name        string
	Description string

	HitPolicy   HitPolicy
	Aggregation *Aggregation

	Columns      []Column
	Rules        Rules
	Descriptions map[string]string
}

// DecisionTable is a DecisionNode that defines decision logic as input columns,
// output columns, and rules whose cells are text expressions over a typed input
// struct I, producing a typed output struct O.
type DecisionTable[I, O any] struct {
	In  I
	Out O

	id          string
	name        string
	description string

	hitPolicy    HitPolicy
	agg          *Aggregation
	columns      []Column
	rules        Rules
	descriptions map[string]string

	ivars, ovars []decisionVar
	envT         reflect.Type

	compiled []compiledRule
}

// compiledRule holds a rule's compiled input predicates and output expressions.
type compiledRule struct {
	id      string
	inputs  []*vm.Program // one per column; nil = wildcard (constant true)
	outputs []*vm.Program // one per output; nil = empty cell (yields null)
}

// NewDecisionTable validates the contracts and compiles every cell into a program
// over the declared inputs. Problems are accumulated and raised once as a
// *DecisionDefinitionError.
func NewDecisionTable[I, O any](config DecisionTableConfig) *DecisionTable[I, O] {
	var problems []string
	add := func(format string, args ...any) { problems = append(problems, fmt.Sprintf(format, args...)) }
	fail := func() {
		panic(&DecisionDefinitionError{Node: nodeLabel(config.Id, config.Name, "DecisionTable"), Problems: problems})
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
	if len(problems) > 0 {
		fail()
	}

	envT, declared := envType(ivars)
	envZero := reflect.New(envT).Elem().Interface()

	// Compile every column expression over the inputs.
	for i, col := range config.Columns {
		if _, err := compileWithEnv(col.Expr, envZero, declared); err != nil {
			add("column %d (%s): %v", i+1, col.Label, err)
		}
	}

	// List-returning policies require every output to be a list.
	listPolicy := config.HitPolicy == HitPolicyRuleOrder || config.HitPolicy == HitPolicyOutputOrder ||
		(config.HitPolicy == HitPolicyCollect && config.Aggregation == nil)
	if listPolicy {
		for _, v := range ovars {
			if v.blType != TypeList {
				add("output %q must be a Handle[BlList] under a list-returning hit policy", v.name)
			}
		}
	}
	// Collect Sum/Min/Max requires numeric outputs.
	if config.HitPolicy == HitPolicyCollect && config.Aggregation != nil {
		switch *config.Aggregation {
		case AggregationSum, AggregationMin, AggregationMax:
			for _, v := range ovars {
				if v.blType != TypeNumber {
					add("output %q must be numeric for a Sum/Min/Max aggregation", v.name)
				}
			}
		}
	}

	expected := len(config.Columns) + len(ovars)
	hasID, widthOK := ruleLayout(config.Rules, expected)
	if !widthOK {
		add("rules have inconsistent width; expected %d cells (or %d with an id column)", expected, expected+1)
		fail()
	}

	seenID := map[string]bool{}
	outputSet := make([]bool, len(ovars))
	var compiled []compiledRule
	for ri, rule := range config.Rules {
		offset := 0
		var id string
		if hasID {
			id = rule[0]
			offset = 1
			if id != "" {
				if seenID[id] {
					add("duplicate rule id %q", id)
				}
				seenID[id] = true
			}
		}
		inputs := make([]*vm.Program, len(config.Columns))
		for ci, col := range config.Columns {
			cell := rule[offset+ci]
			prog, err := compileInputCell(cell, col, envZero, declared)
			if err != nil {
				add("rule %d, column %q: %v", ri+1, col.Label, err)
			}
			inputs[ci] = prog
		}
		outputs := make([]*vm.Program, len(ovars))
		for oi := range ovars {
			cell := rule[offset+len(config.Columns)+oi]
			if strings.TrimSpace(cell) == "" {
				continue // empty → null
			}
			outputSet[oi] = true
			prog, err := compileWithEnv(cell, envZero, declared)
			if err != nil {
				add("rule %d, output %q: %v", ri+1, ovars[oi].name, err)
			}
			outputs[oi] = prog
		}
		compiled = append(compiled, compiledRule{id: id, inputs: inputs, outputs: outputs})
	}

	for oi, set := range outputSet {
		if !set {
			add("output %q is set by no rule", ovars[oi].name)
		}
	}
	for key := range config.Descriptions {
		if !seenID[key] {
			add("description key %q matches no rule id", key)
		}
	}
	if len(problems) > 0 {
		fail()
	}

	d := &DecisionTable[I, O]{
		id:           config.Id,
		name:         config.Name,
		description:  config.Description,
		hitPolicy:    config.HitPolicy,
		agg:          config.Aggregation,
		columns:      append([]Column(nil), config.Columns...),
		rules:        config.Rules,
		descriptions: config.Descriptions,
		ivars:        ivars,
		ovars:        ovars,
		envT:         envT,
		compiled:     compiled,
	}
	d.In = stampSurface(d, d.id, iType, ivars).Interface().(I)
	d.Out = stampSurface(d, d.id, oType, ovars).Interface().(O)
	return d
}

// ruleLayout reports whether the rows carry an id column, and whether every row
// is a consistent width (expected, or expected+1 for an id column).
func ruleLayout(rules Rules, expected int) (hasID, ok bool) {
	if len(rules) == 0 {
		return false, true
	}
	noID, withID := true, true
	for _, r := range rules {
		if len(r) != expected {
			noID = false
		}
		if len(r) != expected+1 {
			withID = false
		}
	}
	switch {
	case noID:
		return false, true
	case withID:
		return true, true
	default:
		return false, false
	}
}

// compileInputCell inlines the column's Expr as the unary test's `?` subject and
// compiles the predicate over the inputs. A `-` cell is the wildcard (nil = always
// true); an empty cell is an error.
func compileInputCell(cell string, col Column, envZero any, declared map[string]bool) (*vm.Program, error) {
	if cell == "-" {
		return nil, nil
	}
	if strings.TrimSpace(cell) == "" {
		return nil, fmt.Errorf("empty input cell; write - for a wildcard")
	}
	if cellNeedsComparable(cell) && !isComparableType(col.Type) {
		return nil, fmt.Errorf("an ordering/interval test requires a comparable column type, not %s", typeName(col.Type))
	}
	rewritten := normaliseUnaryTest(cell)
	inlined := strings.ReplaceAll(rewritten, inputPlaceholder, "("+col.Expr+")")
	return compileWithEnv(inlined, envZero, declared)
}

// cellNeedsComparable reports whether a unary-test cell tests the column directly
// with an ordering or interval form (which require a comparable column type).
// `?`-expressions and wildcards are exempt.
func cellNeedsComparable(cell string) bool {
	for _, p := range splitTopLevelCommas(cell) {
		p = strings.TrimSpace(p)
		if p == "" || p == "-" || strings.Contains(p, "?") || strings.HasPrefix(p, "not(") {
			continue
		}
		if strings.HasPrefix(p, "<") || strings.HasPrefix(p, ">") || strings.HasPrefix(p, "[") || strings.HasPrefix(p, "(") {
			return true
		}
	}
	return false
}

func isComparableType(t Type) bool {
	switch t {
	case TypeNumber, TypeString, TypeDate, TypeTime, TypeDateTime, TypeDaysTimeDuration, TypeYearsMonthsDuration:
		return true
	}
	return false
}

// Evaluate matches the rules against the inputs and combines them per the hit
// policy, returning a typed O.
func (d *DecisionTable[I, O]) Evaluate(in I) (O, error) {
	var out O
	vals := map[string]BlValue{}
	iv := reflect.ValueOf(in)
	for _, v := range d.ivars {
		vals[v.name] = iv.Field(v.field).Interface().(roHandle).getValue()
	}
	env := makeEnv(d.envT, d.ivars, vals).Interface()

	var matches []int
	for ri, cr := range d.compiled {
		ok, err := d.ruleMatches(cr, env)
		if err != nil {
			return out, err
		}
		if ok {
			matches = append(matches, ri)
		}
	}

	if d.hitPolicy == HitPolicyUnique && len(matches) > 1 {
		return out, fmt.Errorf("%s: %d rules matched under a unique hit policy", nodeLabel(d.id, d.name, "DecisionTable"), len(matches))
	}

	ov := reflect.ValueOf(&out).Elem()
	for oi, v := range d.ovars {
		value, err := d.combineOutput(oi, matches, env)
		if err != nil {
			return out, err
		}
		if err := ov.Field(v.field).Addr().Interface().(anyHandle).setValue(value); err != nil {
			return out, err
		}
	}
	return out, nil
}

// ruleMatches reports whether every non-wildcard input predicate of a rule is true.
func (d *DecisionTable[I, O]) ruleMatches(cr compiledRule, env any) (bool, error) {
	for _, prog := range cr.inputs {
		if prog == nil {
			continue // wildcard
		}
		raw, err := expr.Run(prog, env)
		if err != nil {
			return false, &TypeError{Op: "evaluate", Detail: err.Error()}
		}
		switch b := asBl(raw).(type) {
		case BlBoolean:
			if !b.Native() {
				return false, nil
			}
		case BlNull:
			return false, nil
		default:
			return false, &TypeError{Op: "evaluate", Detail: "input predicate did not yield a boolean"}
		}
	}
	return true, nil
}

// combineOutput computes one output field's value from the matching rules per the
// hit policy.
func (d *DecisionTable[I, O]) combineOutput(oi int, matches []int, env any) (BlValue, error) {
	eval := func(ri int) (BlValue, error) {
		prog := d.compiled[ri].outputs[oi]
		if prog == nil {
			return Null(), nil
		}
		raw, err := expr.Run(prog, env)
		if err != nil {
			return nil, &TypeError{Op: "evaluate", Detail: err.Error()}
		}
		return asBl(raw), nil
	}

	switch d.hitPolicy {
	case HitPolicyUnique, HitPolicyFirst, HitPolicyPriority:
		if len(matches) == 0 {
			return Null(), nil
		}
		return eval(matches[0])
	case HitPolicyAny:
		if len(matches) == 0 {
			return Null(), nil
		}
		first, err := eval(matches[0])
		if err != nil {
			return nil, err
		}
		for _, ri := range matches[1:] {
			v, err := eval(ri)
			if err != nil {
				return nil, err
			}
			if eq, ok := first.Equal(v).(BlBoolean); !ok || !eq.Native() {
				return nil, fmt.Errorf("%s: any hit policy but matching rules produced different outputs", nodeLabel(d.id, d.name, "DecisionTable"))
			}
		}
		return first, nil
	case HitPolicyCollect:
		values, err := evalAll(eval, matches)
		if err != nil {
			return nil, err
		}
		if d.agg == nil {
			return List(values...), nil
		}
		return aggregate(values, *d.agg)
	case HitPolicyRuleOrder, HitPolicyOutputOrder:
		values, err := evalAll(eval, matches)
		if err != nil {
			return nil, err
		}
		return List(values...), nil
	}
	return Null(), nil
}

func evalAll(eval func(int) (BlValue, error), matches []int) ([]BlValue, error) {
	values := make([]BlValue, 0, len(matches))
	for _, ri := range matches {
		v, err := eval(ri)
		if err != nil {
			return nil, err
		}
		values = append(values, v)
	}
	return values, nil
}

// aggregate reduces a Collect table's matched values.
func aggregate(values []BlValue, agg Aggregation) (BlValue, error) {
	if agg == AggregationCount {
		n, _ := Number(len(values))
		return n, nil
	}
	if len(values) == 0 {
		n, _ := Number(0)
		return n, nil
	}
	acc := decimal.Zero
	var best decimal.Decimal
	for i, v := range values {
		num, ok := v.(BlNumber)
		if !ok {
			return nil, &TypeError{Op: "aggregate", Detail: "non-numeric value in Sum/Min/Max aggregation"}
		}
		d := num.Decimal()
		switch agg {
		case AggregationSum:
			acc = acc.Add(d)
		case AggregationMin:
			if i == 0 || d.LessThan(best) {
				best = d
			}
		case AggregationMax:
			if i == 0 || d.GreaterThan(best) {
				best = d
			}
		}
	}
	if agg == AggregationSum {
		return Number(acc)
	}
	return Number(best)
}

// DecisionNode[I, O] interface satisfaction.
func (d *DecisionTable[I, O]) GetId() string          { return d.id }
func (d *DecisionTable[I, O]) GetName() string        { return d.name }
func (d *DecisionTable[I, O]) GetDescription() string { return d.description }
func (d *DecisionTable[I, O]) Inputs() []Field        { return fieldsOf(d.ivars) }
func (d *DecisionTable[I, O]) Outputs() []Field       { return fieldsOf(d.ovars) }

// erased decisionNode satisfaction.
func (d *DecisionTable[I, O]) inVars() []decisionVar  { return d.ivars }
func (d *DecisionTable[I, O]) outVars() []decisionVar { return d.ovars }

func (d *DecisionTable[I, O]) runErased(in map[string]BlValue) (map[string]BlValue, error) {
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

// ToMarkdown renders the table as a GitHub-flavoured markdown table.
func (d *DecisionTable[I, O]) ToMarkdown(showRuleIDs, showRuleDescriptions, showInputMappings bool) string {
	var b strings.Builder

	if showInputMappings && len(d.columns) > 0 {
		rows := make([][2]string, len(d.columns))
		for i, c := range d.columns {
			rows[i] = [2]string{c.Label, c.Expr}
		}
		mdTable(&b, "Column", "Expression", rows)
		b.WriteString("\n")
	}

	header := []string{hitPolicyTag(d.hitPolicy, d.agg)}
	if showRuleIDs {
		header = append(header, "rule-id")
	}
	for _, c := range d.columns {
		header = append(header, c.Label)
	}
	header = append(header, "")
	for _, v := range d.ovars {
		header = append(header, v.name)
	}

	grid := [][]string{header}
	for ri, rule := range d.rules {
		row := []string{fmt.Sprintf("%d", ri+1)}
		offset := 0
		if d.compiled[ri].id != "" || ruleHasID(d.rules, len(d.columns)+len(d.ovars)) {
			offset = 1
		}
		if showRuleIDs {
			row = append(row, d.compiled[ri].id)
		}
		for ci := range d.columns {
			row = append(row, rule[offset+ci])
		}
		row = append(row, "█")
		for oi := range d.ovars {
			row = append(row, rule[offset+len(d.columns)+oi])
		}
		grid = append(grid, row)
	}
	writeGrid(&b, grid)

	if showRuleDescriptions && len(d.descriptions) > 0 {
		b.WriteString("\n")
		n := 1
		for _, rule := range d.compiled {
			if desc, ok := d.descriptions[rule.id]; ok {
				fmt.Fprintf(&b, "%d. %s\n", n, desc)
				n++
			}
		}
	}
	return b.String()
}

func ruleHasID(rules Rules, expected int) bool {
	for _, r := range rules {
		if len(r) == expected+1 {
			return true
		}
		if len(r) == expected {
			return false
		}
	}
	return false
}

// hitPolicyTag renders the single-letter hit-policy indicator.
func hitPolicyTag(p HitPolicy, agg *Aggregation) string {
	switch p {
	case HitPolicyUnique:
		return "U"
	case HitPolicyFirst:
		return "F"
	case HitPolicyPriority:
		return "P"
	case HitPolicyAny:
		return "A"
	case HitPolicyRuleOrder:
		return "R"
	case HitPolicyOutputOrder:
		return "O"
	case HitPolicyCollect:
		tag := "C"
		if agg != nil {
			switch *agg {
			case AggregationSum:
				tag += "+"
			case AggregationMin:
				tag += "<"
			case AggregationMax:
				tag += ">"
			case AggregationCount:
				tag += "#"
			}
		}
		return tag
	}
	return "?"
}

// writeGrid renders a 2D string grid as a padded markdown table.
func writeGrid(b *strings.Builder, grid [][]string) {
	if len(grid) == 0 {
		return
	}
	cols := len(grid[0])
	widths := make([]int, cols)
	for _, row := range grid {
		for i := 0; i < cols && i < len(row); i++ {
			if w := displayWidth(row[i]); w > widths[i] {
				widths[i] = w
			}
		}
	}
	writeRow := func(row []string) {
		b.WriteString("|")
		for i := 0; i < cols; i++ {
			cell := ""
			if i < len(row) {
				cell = row[i]
			}
			b.WriteString(" " + cell + strings.Repeat(" ", widths[i]-displayWidth(cell)) + " |")
		}
		b.WriteString("\n")
	}
	writeRow(grid[0])
	b.WriteString("|")
	for i := 0; i < cols; i++ {
		b.WriteString(strings.Repeat("-", widths[i]+2) + "|")
	}
	b.WriteString("\n")
	for _, row := range grid[1:] {
		writeRow(row)
	}
}

// displayWidth counts runes (so the █ separator and multibyte cells align).
func displayWidth(s string) int { return len([]rune(s)) }
