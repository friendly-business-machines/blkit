package core

import (
	"fmt"
	"reflect"
	"strings"
)

// edge is one connection in a decision graph, from a source handle (a task input,
// a node output, or reference data) to a destination handle (a node input or the
// task output). It captures both endpoints' identity and the source's value.
type edge struct {
	srcOwner any
	srcNode  string
	srcField string
	srcVal   BlValue

	dstOwner any
	dstNode  string
	dstField string
}

// Edge connects a source handle to a destination handle. T must match across the
// two handles, so a mis-typed or non-existent-field connection is a Go compile
// error. It is a free function because Go methods cannot be generic.
func Edge[T BlValue](src, dst Handle[T]) edge {
	sn, sf := src.ref()
	dn, df := dst.ref()
	return edge{
		srcOwner: src.owner(), srcNode: sn, srcField: sf, srcVal: src.getValue(),
		dstOwner: dst.owner(), dstNode: dn, dstField: df,
	}
}

// DecisionTaskConfig configures a DecisionTask's identity. (Process-incorporation
// fields — input/output mappings, exit ports, loop, multi-instance — arrive with
// the process layer.)
type DecisionTaskConfig struct {
	Id          string
	Name        string
	Description string
}

type handleKey struct{ node, field string }

// DecisionTask is a graph of DecisionNodes wired into a compile-checked netlist
// over a typed TaskIn/TaskOut. It is built from a config, then wired with Graph.
// A DecisionTask is itself a DecisionNode, so a whole decision composes into a
// larger one.
type DecisionTask[TaskIn, TaskOut any] struct {
	In  TaskIn  // input boundary handle surface
	Out TaskOut // output boundary handle surface

	id          string
	name        string
	description string
	boundaryID  string // node id the boundary handles are stamped with (shared by clones)

	invars, outvars []decisionVar

	wired   bool
	order   []decisionNode
	dstSrc  map[handleKey]handleKey // destination handle -> source handle
	refVals map[handleKey]BlValue   // reference-data source values
}

// NewDecisionTask builds the task from its config and stamps the In/Out boundary
// handle surfaces. The returned task is not yet wired — call task.Graph(...).
func NewDecisionTask[TaskIn, TaskOut any](config DecisionTaskConfig) *DecisionTask[TaskIn, TaskOut] {
	var problems []string
	iType := reflect.TypeOf((*TaskIn)(nil)).Elem()
	oType := reflect.TypeOf((*TaskOut)(nil)).Elem()
	invars, ip := reflectContract(iType)
	for _, p := range ip {
		problems = append(problems, "input "+p)
	}
	outvars, op := reflectContract(oType)
	for _, p := range op {
		problems = append(problems, "output "+p)
	}
	if len(outvars) == 0 {
		problems = append(problems, "at least one output field is required")
	}
	if len(problems) > 0 {
		panic(&DecisionDefinitionError{Node: nodeLabel(config.Id, config.Name, "DecisionTask"), Problems: problems})
	}

	d := &DecisionTask[TaskIn, TaskOut]{
		id:          config.Id,
		name:        config.Name,
		description: config.Description,
		boundaryID:  config.Id,
		invars:      invars,
		outvars:     outvars,
	}
	d.In = stampSurface(d, d.boundaryID, iType, invars).Interface().(TaskIn)
	d.Out = stampSurface(d, d.boundaryID, oType, outvars).Interface().(TaskOut)
	return d
}

// Graph supplies the wiring and finalises the task: it derives the node set and
// reference data from the edges, topologically sorts them, rejects cycles, checks
// every node input is wired exactly once and every TaskOut field is produced, and
// panics once with a *DecisionDefinitionError on any problem. It returns the task.
func (d *DecisionTask[TaskIn, TaskOut]) Graph(edges ...edge) *DecisionTask[TaskIn, TaskOut] {
	var problems []string
	add := func(format string, args ...any) { problems = append(problems, fmt.Sprintf(format, args...)) }
	fail := func() {
		panic(&DecisionDefinitionError{Node: nodeLabel(d.id, d.name, "DecisionTask"), Problems: problems})
	}
	if d.wired {
		add("Graph has already been called on this task")
		fail()
	}

	nodes := map[string]decisionNode{}
	dstSrc := map[handleKey]handleKey{}
	refVals := map[handleKey]BlValue{}
	self := any(d)

	for _, e := range edges {
		if n, ok := e.srcOwner.(decisionNode); ok && e.srcOwner != self {
			nodes[n.GetId()] = n
		}
		if n, ok := e.dstOwner.(decisionNode); ok && e.dstOwner != self {
			nodes[n.GetId()] = n
		}
		if rv, ok := e.srcOwner.(ReferenceValue); ok && e.srcOwner != self {
			if _, isNode := e.srcOwner.(decisionNode); !isNode {
				refVals[handleKey{e.srcNode, e.srcField}] = rv.GetValue()
			}
		}
		dk := handleKey{e.dstNode, e.dstField}
		if _, exists := dstSrc[dk]; exists {
			add("input %s.%s is wired more than once", e.dstNode, e.dstField)
		}
		dstSrc[dk] = handleKey{e.srcNode, e.srcField}
	}

	if len(nodes) == 0 {
		add("the graph wires no nodes")
	}
	for id, n := range nodes {
		if id == "" {
			add("a node has an empty Id")
		}
		for _, v := range n.inVars() {
			if _, ok := dstSrc[handleKey{id, v.name}]; !ok {
				add("node %q input %q is not wired", id, v.name)
			}
		}
	}
	for _, v := range d.outvars {
		if _, ok := dstSrc[handleKey{d.boundaryID, v.name}]; !ok {
			add("task output %q is not wired", v.name)
		}
	}

	order, cyclic := topoSortNodes(nodes, edges, self)
	if cyclic {
		add("the graph contains a cycle")
	}
	if len(problems) > 0 {
		fail()
	}

	d.order = order
	d.dstSrc = dstSrc
	d.refVals = refVals
	d.wired = true
	return d
}

// topoSortNodes orders the nodes so producers precede consumers. It returns the
// order and whether a cycle was found.
func topoSortNodes(nodes map[string]decisionNode, edges []edge, self any) ([]decisionNode, bool) {
	indeg := map[string]int{}
	for id := range nodes {
		indeg[id] = 0
	}
	adj := map[string][]string{}
	seen := map[[2]string]bool{}
	for _, e := range edges {
		_, srcNode := e.srcOwner.(decisionNode)
		_, dstNode := e.dstOwner.(decisionNode)
		if !srcNode || !dstNode || e.srcOwner == self || e.dstOwner == self {
			continue
		}
		key := [2]string{e.srcNode, e.dstNode}
		if seen[key] {
			continue
		}
		seen[key] = true
		adj[e.srcNode] = append(adj[e.srcNode], e.dstNode)
		indeg[e.dstNode]++
	}
	var queue []string
	for id := range nodes {
		if indeg[id] == 0 {
			queue = append(queue, id)
		}
	}
	var order []decisionNode
	for len(queue) > 0 {
		id := queue[0]
		queue = queue[1:]
		order = append(order, nodes[id])
		for _, m := range adj[id] {
			indeg[m]--
			if indeg[m] == 0 {
				queue = append(queue, m)
			}
		}
	}
	return order, len(order) != len(nodes)
}

// Evaluate runs the decision logic against the typed input and returns TaskOut.
func (d *DecisionTask[TaskIn, TaskOut]) Evaluate(in TaskIn) (TaskOut, error) {
	var out TaskOut
	if !d.wired {
		return out, fmt.Errorf("%s: task has no graph (call Graph)", nodeLabel(d.id, d.name, "DecisionTask"))
	}
	env := map[handleKey]BlValue{}
	for k, v := range d.refVals {
		env[k] = v
	}
	iv := reflect.ValueOf(in)
	for _, v := range d.invars {
		env[handleKey{d.boundaryID, v.name}] = iv.Field(v.field).Interface().(roHandle).getValue()
	}

	for _, node := range d.order {
		inMap := map[string]BlValue{}
		for _, v := range node.inVars() {
			src := d.dstSrc[handleKey{node.GetId(), v.name}]
			if val, ok := env[src]; ok {
				inMap[v.name] = val
			}
		}
		outMap, err := node.runErased(inMap)
		if err != nil {
			return out, err
		}
		for _, v := range node.outVars() {
			env[handleKey{node.GetId(), v.name}] = outMap[v.name]
		}
	}

	ov := reflect.ValueOf(&out).Elem()
	for _, v := range d.outvars {
		src := d.dstSrc[handleKey{d.boundaryID, v.name}]
		val, ok := env[src]
		if !ok || val == nil {
			val = Null()
		}
		if err := ov.Field(v.field).Addr().Interface().(anyHandle).setValue(val); err != nil {
			return out, err
		}
	}
	return out, nil
}

// Clone returns a new task sharing the receiver's graph (and its derived nodes,
// reference data, and In/Out boundary) by reference, with identity from the config.
func (d *DecisionTask[TaskIn, TaskOut]) Clone(config DecisionTaskConfig) *DecisionTask[TaskIn, TaskOut] {
	return &DecisionTask[TaskIn, TaskOut]{
		In:          d.In,
		Out:         d.Out,
		id:          config.Id,
		name:        config.Name,
		description: config.Description,
		boundaryID:  d.boundaryID,
		invars:      d.invars,
		outvars:     d.outvars,
		wired:       d.wired,
		order:       d.order,
		dstSrc:      d.dstSrc,
		refVals:     d.refVals,
	}
}

// DecisionNode[TaskIn, TaskOut] interface satisfaction — a DecisionTask is a node.
func (d *DecisionTask[TaskIn, TaskOut]) GetId() string          { return d.id }
func (d *DecisionTask[TaskIn, TaskOut]) GetName() string        { return d.name }
func (d *DecisionTask[TaskIn, TaskOut]) GetDescription() string { return d.description }
func (d *DecisionTask[TaskIn, TaskOut]) Inputs() []Field        { return fieldsOf(d.invars) }
func (d *DecisionTask[TaskIn, TaskOut]) Outputs() []Field       { return fieldsOf(d.outvars) }

// erased decisionNode satisfaction (so a DecisionTask can be wired as a child).
func (d *DecisionTask[TaskIn, TaskOut]) inVars() []decisionVar  { return d.invars }
func (d *DecisionTask[TaskIn, TaskOut]) outVars() []decisionVar { return d.outvars }

func (d *DecisionTask[TaskIn, TaskOut]) runErased(in map[string]BlValue) (map[string]BlValue, error) {
	input, err := inputFromMap(reflect.TypeOf((*TaskIn)(nil)).Elem(), d.invars, in)
	if err != nil {
		return nil, err
	}
	out, err := d.Evaluate(input.(TaskIn))
	if err != nil {
		return nil, err
	}
	return outputToMap(out, d.outvars), nil
}

// ToMarkdown renders the task: title, contracts, and each node.
func (d *DecisionTask[TaskIn, TaskOut]) ToMarkdown() string {
	var b strings.Builder
	label := d.name
	if label == "" {
		label = d.id
	}
	b.WriteString("# " + label + "\n\n")
	if d.description != "" {
		b.WriteString("_" + d.description + "_\n\n")
	}
	b.WriteString("## Inputs\n\n")
	contractTable(&b, d.invars)
	b.WriteString("\n## Outputs\n\n")
	contractTable(&b, d.outvars)
	return b.String()
}
