package core

import (
	"strings"
	"testing"
)

func hStr(t *testing.T, s string) Handle[BlString] {
	t.Helper()
	v, err := String(s)
	if err != nil {
		t.Fatalf("String(%q): %v", s, err)
	}
	return NewHandle(v)
}

// --- DecisionTable -----------------------------------------------------------

type eligIn struct {
	Age    Handle[BlNumber] `expr:"age"`
	Points Handle[BlNumber] `expr:"points"`
}
type eligOut struct {
	Eligibility Handle[BlString] `expr:"eligibility"`
}

func TestDecisionTableUnique(t *testing.T) {
	tbl := NewDecisionTable[eligIn, eligOut](DecisionTableConfig{
		Id:        "eligibility",
		Name:      "Eligibility Check",
		HitPolicy: HitPolicyUnique,
		Columns: []Column{
			{Label: "Age", Expr: `age`, Type: TypeNumber},
			{Label: "Points", Expr: `points`, Type: TypeNumber},
		},
		Rules: Rules{
			{`>= 18`, `>= 30000`, `"eligible"`},
			{`< 18`, `-`, `"ineligible"`},
			{`-`, `< 30000`, `"ineligible"`},
		},
	})
	for _, c := range []struct {
		age, points int
		want        string
	}{{30, 50000, "eligible"}, {16, 50000, "ineligible"}, {40, 20000, "ineligible"}} {
		out, err := tbl.Evaluate(eligIn{Age: hNum(t, c.age), Points: hNum(t, c.points)})
		if err != nil {
			t.Fatalf("evaluate: %v", err)
		}
		if out.Eligibility.Get().String() != c.want {
			t.Errorf("age=%d points=%d: got %s want %s", c.age, c.points, out.Eligibility.Get().String(), c.want)
		}
	}
	md := tbl.ToMarkdown(false, false, false)
	if !strings.Contains(md, "| U |") || !strings.Contains(md, "eligibility") {
		t.Errorf("markdown unexpected:\n%s", md)
	}
}

func TestDecisionTableFirst(t *testing.T) {
	type dIn struct {
		Type  Handle[BlString] `expr:"customer_type"`
		Total Handle[BlNumber] `expr:"order_total"`
	}
	type dOut struct {
		Discount Handle[BlNumber] `expr:"discount"`
	}
	tbl := NewDecisionTable[dIn, dOut](DecisionTableConfig{
		Id:        "discount",
		HitPolicy: HitPolicyFirst,
		Columns: []Column{
			{Label: "Type", Expr: `customer_type`, Type: TypeString},
			{Label: "Total", Expr: `order_total`, Type: TypeNumber},
		},
		Rules: Rules{
			{`vip`, `"VIP"`, `-`, `0.20`},
			{`large-order`, `-`, `> 500`, `0.10`},
			{`default`, `-`, `-`, `0.0`},
		},
	})
	out, err := tbl.Evaluate(dIn{Type: hStr(t, "VIP"), Total: hNum(t, 1000)})
	if err != nil {
		t.Fatalf("evaluate: %v", err)
	}
	if out.Discount.Get().String() != "0.2" {
		t.Errorf("discount = %s, want 0.2", out.Discount.Get().String())
	}
}

func TestDecisionTableCollectSum(t *testing.T) {
	type pIn struct {
		Speed Handle[BlNumber] `expr:"speed"`
		Zone  Handle[BlString] `expr:"zone_type"`
	}
	type pOut struct {
		Fine Handle[BlNumber] `expr:"fine"`
	}
	sum := AggregationSum
	tbl := NewDecisionTable[pIn, pOut](DecisionTableConfig{
		Id:          "penalties",
		HitPolicy:   HitPolicyCollect,
		Aggregation: &sum,
		Columns: []Column{
			{Label: "Speed", Expr: `speed`, Type: TypeNumber},
			{Label: "Zone", Expr: `zone_type`, Type: TypeString},
		},
		Rules: Rules{
			{`> 30`, `"school"`, `200`},
			{`> 50`, `-`, `150`},
			{`-`, `"school"`, `50`},
		},
	})
	out, err := tbl.Evaluate(pIn{Speed: hNum(t, 55), Zone: hStr(t, "school")})
	if err != nil {
		t.Fatalf("evaluate: %v", err)
	}
	if out.Fine.Get().String() != "400" { // 200 + 150 + 50
		t.Errorf("fine = %s, want 400", out.Fine.Get().String())
	}
}

func TestDecisionTableConstructionErrors(t *testing.T) {
	mustPanic := func(name string, fn func()) {
		t.Run(name, func(t *testing.T) {
			defer func() {
				if recover() == nil {
					t.Errorf("expected a DecisionDefinitionError panic")
				}
			}()
			fn()
		})
	}
	mustPanic("wrong width", func() {
		NewDecisionTable[eligIn, eligOut](DecisionTableConfig{
			Columns: []Column{{Label: "Age", Expr: `age`, Type: TypeNumber}, {Label: "Points", Expr: `points`, Type: TypeNumber}},
			Rules:   Rules{{`>= 18`, `"eligible"`}}, // missing a cell
		})
	})
	mustPanic("empty input cell", func() {
		NewDecisionTable[eligIn, eligOut](DecisionTableConfig{
			Columns: []Column{{Label: "Age", Expr: `age`, Type: TypeNumber}, {Label: "Points", Expr: `points`, Type: TypeNumber}},
			Rules:   Rules{{`>= 18`, ``, `"eligible"`}},
		})
	})
	mustPanic("ordering on non-comparable", func() {
		type li struct {
			Tags Handle[BlList] `expr:"tags"`
		}
		NewDecisionTable[li, eligOut](DecisionTableConfig{
			Columns: []Column{{Label: "Tags", Expr: `tags`, Type: TypeList}},
			Rules:   Rules{{`>= 18`, `"eligible"`}},
		})
	})
}

// --- DecisionNativeFunction --------------------------------------------------

type scoreIn struct {
	Age   Handle[BlNumber] `expr:"age"`
	Steps Handle[BlNumber] `expr:"steps"`
}
type scoreOut struct {
	Score Handle[BlNumber] `expr:"score"`
}

func TestDecisionNativeFunction(t *testing.T) {
	fn := func(in scoreIn) (scoreOut, error) {
		sum, _ := Number(in.Age.Get().Decimal().Add(in.Steps.Get().Decimal()))
		return scoreOut{Score: NewHandle(sum)}, nil
	}
	node := NewDecisionNativeFunction(DecisionNativeFunctionConfig{Id: "fitness_score", Name: "Fitness Score"}, fn)
	out, err := node.Evaluate(scoreIn{Age: hNum(t, 40), Steps: hNum(t, 60)})
	if err != nil {
		t.Fatalf("evaluate: %v", err)
	}
	if out.Score.Get().String() != "100" {
		t.Errorf("score = %s, want 100", out.Score.Get().String())
	}
	if md := node.ToMarkdown(); !strings.Contains(md, "native Go function") || !strings.Contains(md, "age") || !strings.Contains(md, "Number") {
		t.Errorf("markdown unexpected:\n%s", md)
	}
}

func TestDecisionNativeFunctionPanicRecovered(t *testing.T) {
	fn := func(in scoreIn) (scoreOut, error) { panic("boom") }
	node := NewDecisionNativeFunction(DecisionNativeFunctionConfig{Id: "x"}, fn)
	if _, err := node.Evaluate(scoreIn{Age: hNum(t, 1), Steps: hNum(t, 1)}); err == nil {
		t.Errorf("expected a recovered-panic error, got nil")
	}
}

// --- ReferenceData -----------------------------------------------------------

func TestReferenceData(t *testing.T) {
	r := NewReferenceData(ReferenceDataConfig[BlNumber]{Id: "min_points", Name: "Minimum Points", Value: mustNum(t, 30000)})
	if r.GetId() != "min_points" || r.GetValue().String() != "30000" {
		t.Errorf("got id=%s value=%s", r.GetId(), r.GetValue().String())
	}
	var rv ReferenceValue = r
	if rv.GetValue().String() != "30000" {
		t.Errorf("ReferenceValue view wrong: %s", rv.GetValue().String())
	}
}

func TestReferenceDataEmptyIdPanics(t *testing.T) {
	defer func() {
		if recover() == nil {
			t.Errorf("expected panic for empty Id")
		}
	}()
	NewReferenceData(ReferenceDataConfig[BlNumber]{Value: mustNum(t, 1)})
}

// --- DecisionTask (full wiring) ----------------------------------------------

type taskIn struct {
	Age       Handle[BlNumber] `expr:"age"`
	Points    Handle[BlNumber] `expr:"points"`
	Referrals Handle[BlNumber] `expr:"referrals"`
}
type taskOut struct {
	Status Handle[BlString] `expr:"status"`
}

type eligIn3 struct {
	Age       Handle[BlNumber] `expr:"age"`
	Points    Handle[BlNumber] `expr:"points"`
	MinPoints Handle[BlNumber] `expr:"min_points"`
}
type apprIn struct {
	Eligibility Handle[BlString] `expr:"eligibility"`
}
type apprOut struct {
	Status Handle[BlString] `expr:"status"`
}

func buildMembershipTask(t *testing.T) *DecisionTask[taskIn, taskOut] {
	t.Helper()
	minPoints := NewReferenceData(ReferenceDataConfig[BlNumber]{Id: "min_points", Value: mustNum(t, 30000)})
	elig := NewDecisionTable[eligIn3, eligOut](DecisionTableConfig{
		Id:        "eligibility",
		HitPolicy: HitPolicyUnique,
		Columns: []Column{
			{Label: "Age", Expr: `age`, Type: TypeNumber},
			{Label: "Points", Expr: `points`, Type: TypeNumber},
		},
		Rules: Rules{
			{`>= 18`, `>= min_points`, `"eligible"`},
			{`< 18`, `-`, `"ineligible"`},
			{`-`, `< min_points`, `"ineligible"`},
		},
	})
	appr := NewDecisionExpression[apprIn, apprOut](DecisionExpressionConfig{
		Id:      "approval",
		Entries: Entries{"status": `if eligibility = "eligible" then "approved" else "denied"`},
	})
	task := NewDecisionTask[taskIn, taskOut](DecisionTaskConfig{Id: "membership", Name: "Membership Approval"})
	task.Graph(
		Edge(task.In.Age, elig.In.Age),
		Edge(task.In.Points, elig.In.Points),
		Edge(minPoints.Value, elig.In.MinPoints),
		Edge(elig.Out.Eligibility, appr.In.Eligibility),
		Edge(appr.Out.Status, task.Out.Status),
	)
	return task
}

func TestDecisionTaskFullWiring(t *testing.T) {
	task := buildMembershipTask(t)
	for _, c := range []struct {
		age, points int
		want        string
	}{{30, 50000, "approved"}, {16, 50000, "denied"}, {40, 20000, "denied"}} {
		out, err := task.Evaluate(taskIn{Age: hNum(t, c.age), Points: hNum(t, c.points), Referrals: hNum(t, 200000)})
		if err != nil {
			t.Fatalf("evaluate: %v", err)
		}
		if out.Status.Get().String() != c.want {
			t.Errorf("age=%d points=%d: got %s want %s", c.age, c.points, out.Status.Get().String(), c.want)
		}
	}
}

func TestDecisionTaskCloneEvaluates(t *testing.T) {
	clone := buildMembershipTask(t).Clone(DecisionTaskConfig{Id: "vip-check", Name: "VIP Check"})
	if clone.GetId() != "vip-check" {
		t.Errorf("clone id = %s", clone.GetId())
	}
	out, err := clone.Evaluate(taskIn{Age: hNum(t, 30), Points: hNum(t, 50000), Referrals: hNum(t, 1)})
	if err != nil {
		t.Fatalf("evaluate: %v", err)
	}
	if out.Status.Get().String() != "approved" {
		t.Errorf("clone status = %s, want approved", out.Status.Get().String())
	}
}

func TestDecisionTaskWiringErrors(t *testing.T) {
	mustPanic := func(name string, fn func()) {
		t.Run(name, func(t *testing.T) {
			defer func() {
				if recover() == nil {
					t.Errorf("expected a DecisionDefinitionError panic")
				}
			}()
			fn()
		})
	}
	mustPanic("output not wired", func() {
		appr := NewDecisionExpression[apprIn, apprOut](DecisionExpressionConfig{
			Id:      "approval",
			Entries: Entries{"status": `"x"`},
		})
		task := NewDecisionTask[apprIn, taskOut](DecisionTaskConfig{Id: "t"})
		// approval's input is wired, but task output Status is never produced.
		task.Graph(Edge(task.In.Eligibility, appr.In.Eligibility))
	})
	mustPanic("node input not wired", func() {
		appr := NewDecisionExpression[apprIn, apprOut](DecisionExpressionConfig{
			Id:      "approval",
			Entries: Entries{"status": `"x"`},
		})
		task := NewDecisionTask[apprIn, taskOut](DecisionTaskConfig{Id: "t"})
		// approval.In.Eligibility is left unwired.
		task.Graph(Edge(appr.Out.Status, task.Out.Status))
	})
}
