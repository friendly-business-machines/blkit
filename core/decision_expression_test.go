package core

import (
	"strings"
	"testing"
)

type shipmentInputs struct {
	Weight  Handle[BlNumber] `expr:"weight"`
	Rate    Handle[BlNumber] `expr:"rate"`
	Parcels Handle[BlNumber] `expr:"parcels"`
}

type breakdownOutputs struct {
	PerParcel Handle[BlNumber] `expr:"per_parcel"`
	Freight   Handle[BlNumber] `expr:"freight"`
	Total     Handle[BlNumber] `expr:"total"`
}

type quoteInputs struct {
	Weight Handle[BlNumber] `expr:"weight"`
	Rate   Handle[BlNumber] `expr:"rate"`
}

type amountOutput struct {
	Amount Handle[BlNumber] `expr:"amount"`
}

func numStr(t *testing.T, s string) BlNumber { t.Helper(); v, _ := Number(s); return v }

func hNum(t *testing.T, n int) Handle[BlNumber]       { t.Helper(); return NewHandle(mustNum(t, n)) }
func hNumStr(t *testing.T, s string) Handle[BlNumber] { t.Helper(); return NewHandle(numStr(t, s)) }

func TestDecisionExpressionSingleOutput(t *testing.T) {
	d := NewDecisionExpression[quoteInputs, amountOutput](DecisionExpressionConfig{
		Id:      "freight_quote",
		Name:    "Freight Quote",
		Entries: Entries{"amount": `weight * rate / 12`},
	})
	out, err := d.Evaluate(quoteInputs{Weight: hNum(t, 24000), Rate: hNumStr(t, "0.06")})
	if err != nil {
		t.Fatalf("evaluate: %v", err)
	}
	if out.Amount.Get().String() != "120" { // 24000 * 0.06 / 12
		t.Errorf("amount = %s, want 120", out.Amount.Get().String())
	}
}

func TestDecisionExpressionCrossEntry(t *testing.T) {
	d := NewDecisionExpression[shipmentInputs, breakdownOutputs](DecisionExpressionConfig{
		Id:   "freight_breakdown",
		Name: "Freight Breakdown",
		Entries: Entries{
			"per_parcel": `weight / parcels`,
			"freight":    `weight * rate / 12`,
			"total":      `per_parcel + freight`, // references sibling outputs
		},
	})
	out, err := d.Evaluate(shipmentInputs{
		Weight:  hNum(t, 120000),
		Rate:    hNumStr(t, "0.06"),
		Parcels: hNum(t, 12),
	})
	if err != nil {
		t.Fatalf("evaluate: %v", err)
	}
	if out.PerParcel.Get().String() != "10000" || out.Freight.Get().String() != "600" || out.Total.Get().String() != "10600" {
		t.Errorf("got per_parcel=%s freight=%s total=%s", out.PerParcel.Get().String(), out.Freight.Get().String(), out.Total.Get().String())
	}
}

func TestDecisionExpressionConditional(t *testing.T) {
	type scoreIn struct {
		Score Handle[BlNumber] `expr:"score"`
	}
	type statusOut struct {
		Status Handle[BlString] `expr:"status"`
	}
	d := NewDecisionExpression[scoreIn, statusOut](DecisionExpressionConfig{
		Entries: Entries{"status": `if score >= 700 then "approved" else "review"`},
	})
	for _, c := range []struct {
		score int
		want  string
	}{{750, "approved"}, {600, "review"}} {
		out, err := d.Evaluate(scoreIn{Score: hNum(t, c.score)})
		if err != nil {
			t.Fatalf("evaluate: %v", err)
		}
		if out.Status.Get().String() != c.want {
			t.Errorf("score=%d status=%s want %s", c.score, out.Status.Get().String(), c.want)
		}
	}
}

func TestDecisionExpressionUntaggedFieldUsesGoName(t *testing.T) {
	type scoreIn struct {
		Score Handle[BlNumber] // no tag: variable name is the Go field name, "Score"
	}
	type gradeOut struct {
		Grade Handle[BlString] // no tag: variable name "Grade"
	}
	d := NewDecisionExpression[scoreIn, gradeOut](DecisionExpressionConfig{
		Entries: Entries{"Grade": `if Score >= 50 then "pass" else "fail"`},
	})
	out, err := d.Evaluate(scoreIn{Score: hNum(t, 70)})
	if err != nil {
		t.Fatalf("evaluate: %v", err)
	}
	if out.Grade.Get().String() != "pass" {
		t.Errorf("grade = %s, want pass", out.Grade.Get().String())
	}
}

func TestDecisionExpressionCallsUDF(t *testing.T) {
	addTax, err := Func[taxParams, BlNumber]("addTax", `amount * 1.2`)
	if err != nil {
		t.Fatalf("Func: %v", err)
	}
	type priceIn struct {
		Base Handle[BlNumber] `expr:"base"`
	}
	type priceOut struct {
		Gross        Handle[BlNumber] `expr:"gross"`
		WithShipping Handle[BlNumber] `expr:"with_shipping"`
	}
	d := NewDecisionExpression[priceIn, priceOut](DecisionExpressionConfig{
		Name:    "Gross Price",
		Entries: Entries{"gross": `addTax(base)`, "with_shipping": `gross + 5`},
		Funcs:   []UDF{addTax},
	})
	out, err := d.Evaluate(priceIn{Base: hNum(t, 100)})
	if err != nil {
		t.Fatalf("evaluate: %v", err)
	}
	if out.Gross.Get().String() != "120" || out.WithShipping.Get().String() != "125" {
		t.Errorf("got gross=%s with_shipping=%s, want 120 and 125", out.Gross.Get().String(), out.WithShipping.Get().String())
	}
	md := d.ToMarkdown()
	for _, want := range []string{"**Inputs:** base", "addTax(base)", "| addTax(amount) | amount * 1.2 |"} {
		if !strings.Contains(md, want) {
			t.Errorf("markdown missing %q:\n%s", want, md)
		}
	}
}

func TestDecisionExpressionToMarkdown(t *testing.T) {
	d := NewDecisionExpression[shipmentInputs, breakdownOutputs](DecisionExpressionConfig{
		Name: "Freight Breakdown",
		Entries: Entries{
			"per_parcel": `weight / parcels`,
			"freight":    `weight * rate / 12`,
			"total":      `per_parcel + freight`,
		},
	})
	md := d.ToMarkdown()
	if !strings.Contains(md, "### Freight Breakdown") {
		t.Errorf("missing header:\n%s", md)
	}
	if !strings.Contains(md, "**Inputs:** weight, rate, parcels") {
		t.Errorf("missing inputs line:\n%s", md)
	}
	if !strings.Contains(md, "| per_parcel | weight / parcels") {
		t.Errorf("missing per_parcel row:\n%s", md)
	}
}

func TestDecisionExpressionOutputTypeMismatch(t *testing.T) {
	d := NewDecisionExpression[quoteInputs, amountOutput](DecisionExpressionConfig{
		Entries: Entries{"amount": `"not a number"`},
	})
	if _, err := d.Evaluate(quoteInputs{Weight: hNum(t, 1), Rate: hNum(t, 1)}); err == nil {
		t.Errorf("expected a TypeError for a string produced for a number output")
	}
}

func TestDecisionExpressionConstructionErrors(t *testing.T) {
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
	mustPanic("missing entry", func() {
		NewDecisionExpression[quoteInputs, breakdownOutputs](DecisionExpressionConfig{
			Entries: Entries{"per_parcel": `1`},
		})
	})
	mustPanic("extra entry", func() {
		NewDecisionExpression[quoteInputs, amountOutput](DecisionExpressionConfig{
			Entries: Entries{"amount": `1`, "bogus": `2`},
		})
	})
	mustPanic("undefined name", func() {
		NewDecisionExpression[quoteInputs, amountOutput](DecisionExpressionConfig{
			Entries: Entries{"amount": `weight + missing`},
		})
	})
	mustPanic("cycle", func() {
		NewDecisionExpression[quoteInputs, breakdownOutputs](DecisionExpressionConfig{
			Entries: Entries{"per_parcel": `total`, "freight": `1`, "total": `per_parcel`},
		})
	})
	mustPanic("empty outputs", func() {
		NewDecisionExpression[quoteInputs, struct{}](DecisionExpressionConfig{Entries: Entries{}})
	})
	mustPanic("duplicate output tag", func() {
		type dupOut struct {
			A Handle[BlNumber] `expr:"x"`
			B Handle[BlNumber] `expr:"x"`
		}
		NewDecisionExpression[quoteInputs, dupOut](DecisionExpressionConfig{Entries: Entries{"x": `1`}})
	})
	mustPanic("input/output collision", func() {
		type collideOut struct {
			Weight Handle[BlNumber] `expr:"weight"`
		}
		NewDecisionExpression[quoteInputs, collideOut](DecisionExpressionConfig{Entries: Entries{"weight": `1`}})
	})
	mustPanic("non-handle input field", func() {
		type badIn struct {
			Amount int `expr:"amount"`
		}
		NewDecisionExpression[badIn, amountOutput](DecisionExpressionConfig{Entries: Entries{"amount": `1`}})
	})
	mustPanic("bare-value output field", func() {
		type badOut struct {
			Amount BlNumber `expr:"amount"`
		}
		NewDecisionExpression[quoteInputs, badOut](DecisionExpressionConfig{Entries: Entries{"amount": `1`}})
	})
	mustPanic("invalid identifier tag", func() {
		type badName struct {
			Amount Handle[BlNumber] `expr:"not a name"`
		}
		NewDecisionExpression[quoteInputs, badName](DecisionExpressionConfig{Entries: Entries{"not a name": `1`}})
	})
	mustPanic("excluded field via expr:-", func() {
		type excludedOut struct {
			Amount Handle[BlNumber] `expr:"amount"`
			Hidden Handle[BlNumber] `expr:"-"`
		}
		NewDecisionExpression[quoteInputs, excludedOut](DecisionExpressionConfig{Entries: Entries{"amount": `1`}})
	})
	mustPanic("unexported field", func() {
		type unexportedIn struct {
			Weight Handle[BlNumber] `expr:"weight"`
			secret Handle[BlNumber] `expr:"secret"`
		}
		_ = unexportedIn{}.secret
		NewDecisionExpression[unexportedIn, amountOutput](DecisionExpressionConfig{Entries: Entries{"amount": `weight`}})
	})
	mustPanic("duplicate function name", func() {
		f1, _ := Func[taxParams, BlNumber]("addTax", `amount * 1.2`)
		f2, _ := Func[taxParams, BlNumber]("addTax", `amount * 1.3`)
		NewDecisionExpression[quoteInputs, amountOutput](DecisionExpressionConfig{
			Entries: Entries{"amount": `weight`},
			Funcs:   []UDF{f1, f2},
		})
	})
}
