package core

import (
	"strings"
	"testing"
)

type loanInputs struct {
	LoanAmount BlNumber `expr:"loan_amount"`
	Rate       BlNumber `expr:"rate"`
	Term       BlNumber `expr:"term"`
}

type breakdownOutputs struct {
	Principal BlNumber `expr:"principal"`
	Interest  BlNumber `expr:"interest"`
	Total     BlNumber `expr:"total"`
}

type paymentInputs struct {
	LoanAmount BlNumber `expr:"loan_amount"`
	Rate       BlNumber `expr:"rate"`
}

type amountOutput struct {
	Amount BlNumber `expr:"amount"`
}

func numStr(t *testing.T, s string) BlNumber { t.Helper(); v, _ := Number(s); return v }

func TestDecisionExpressionSingleOutput(t *testing.T) {
	d := NewDecisionExpression[paymentInputs, amountOutput](DecisionExpressionConfig{
		Id:      "monthly_payment",
		Name:    "Monthly Payment",
		Entries: Entries{"amount": `loan_amount * rate / 12`},
	})
	out, err := d.Evaluate(paymentInputs{LoanAmount: mustNum(t, 24000), Rate: numStr(t, "0.06")})
	if err != nil {
		t.Fatalf("evaluate: %v", err)
	}
	if out.Amount.String() != "120" { // 24000 * 0.06 / 12
		t.Errorf("amount = %s, want 120", out.Amount.String())
	}
}

func TestDecisionExpressionCrossEntry(t *testing.T) {
	d := NewDecisionExpression[loanInputs, breakdownOutputs](DecisionExpressionConfig{
		Id:   "monthly_breakdown",
		Name: "Monthly Breakdown",
		Entries: Entries{
			"principal": `loan_amount / term`,
			"interest":  `loan_amount * rate / 12`,
			"total":     `principal + interest`, // references sibling outputs
		},
	})
	out, err := d.Evaluate(loanInputs{
		LoanAmount: mustNum(t, 120000),
		Rate:       numStr(t, "0.06"),
		Term:       mustNum(t, 12),
	})
	if err != nil {
		t.Fatalf("evaluate: %v", err)
	}
	if out.Principal.String() != "10000" || out.Interest.String() != "600" || out.Total.String() != "10600" {
		t.Errorf("got principal=%s interest=%s total=%s", out.Principal.String(), out.Interest.String(), out.Total.String())
	}
}

func TestDecisionExpressionConditional(t *testing.T) {
	type scoreIn struct {
		Score BlNumber `expr:"score"`
	}
	type statusOut struct {
		Status BlString `expr:"status"`
	}
	d := NewDecisionExpression[scoreIn, statusOut](DecisionExpressionConfig{
		Entries: Entries{"status": `if score >= 700 then "approved" else "review"`},
	})
	for _, c := range []struct {
		score int
		want  string
	}{{750, "approved"}, {600, "review"}} {
		out, err := d.Evaluate(scoreIn{Score: mustNum(t, c.score)})
		if err != nil {
			t.Fatalf("evaluate: %v", err)
		}
		if out.Status.String() != c.want {
			t.Errorf("score=%d status=%s want %s", c.score, out.Status.String(), c.want)
		}
	}
}

func TestDecisionExpressionUntaggedFieldUsesGoName(t *testing.T) {
	type scoreIn struct {
		Score BlNumber // no tag: variable name is the Go field name, "Score"
	}
	type gradeOut struct {
		Grade BlString // no tag: variable name "Grade"
	}
	d := NewDecisionExpression[scoreIn, gradeOut](DecisionExpressionConfig{
		Entries: Entries{"Grade": `if Score >= 50 then "pass" else "fail"`},
	})
	out, err := d.Evaluate(scoreIn{Score: mustNum(t, 70)})
	if err != nil {
		t.Fatalf("evaluate: %v", err)
	}
	if out.Grade.String() != "pass" {
		t.Errorf("grade = %s, want pass", out.Grade.String())
	}
}

func TestDecisionExpressionCallsUDF(t *testing.T) {
	addTax, err := Func[taxParams, BlNumber]("addTax", `amount * 1.2`)
	if err != nil {
		t.Fatalf("Func: %v", err)
	}
	type priceIn struct {
		Base BlNumber `expr:"base"`
	}
	type priceOut struct {
		Gross        BlNumber `expr:"gross"`
		WithShipping BlNumber `expr:"with_shipping"`
	}
	d := NewDecisionExpression[priceIn, priceOut](DecisionExpressionConfig{
		Name:    "Gross Price",
		Entries: Entries{"gross": `addTax(base)`, "with_shipping": `gross + 5`},
		Funcs:   []UDF{addTax},
	})
	out, err := d.Evaluate(priceIn{Base: mustNum(t, 100)})
	if err != nil {
		t.Fatalf("evaluate: %v", err)
	}
	if out.Gross.String() != "120" || out.WithShipping.String() != "125" {
		t.Errorf("got gross=%s with_shipping=%s, want 120 and 125", out.Gross.String(), out.WithShipping.String())
	}
	// The markdown lists the inputs, renders the UDF call verbatim in the entries
	// table, and includes the function (by call signature) in that same table.
	md := d.ToMarkdown()
	for _, want := range []string{"**Inputs:** base", "addTax(base)", "| addTax(amount) | amount * 1.2 |"} {
		if !strings.Contains(md, want) {
			t.Errorf("markdown missing %q:\n%s", want, md)
		}
	}
	if strings.Contains(md, "**Functions**") {
		t.Errorf("functions should share the entries table, not a separate section:\n%s", md)
	}
}

func TestDecisionExpressionToMarkdown(t *testing.T) {
	d := NewDecisionExpression[loanInputs, breakdownOutputs](DecisionExpressionConfig{
		Name: "Monthly Breakdown",
		Entries: Entries{
			"principal": `loan_amount / term`,
			"interest":  `loan_amount * rate / 12`,
			"total":     `principal + interest`,
		},
	})
	md := d.ToMarkdown()
	if !strings.Contains(md, "### Monthly Breakdown") {
		t.Errorf("missing header:\n%s", md)
	}
	if !strings.Contains(md, "**Inputs:** loan_amount, rate, term") {
		t.Errorf("missing inputs line:\n%s", md)
	}
	if !strings.Contains(md, "| principal | loan_amount / term") {
		t.Errorf("missing principal row:\n%s", md)
	}
	// A node with no UDFs has no Functions section.
	if strings.Contains(md, "**Functions**") {
		t.Errorf("unexpected Functions section:\n%s", md)
	}
}

func TestDecisionExpressionOutputTypeMismatch(t *testing.T) {
	d := NewDecisionExpression[paymentInputs, amountOutput](DecisionExpressionConfig{
		Entries: Entries{"amount": `"not a number"`},
	})
	if _, err := d.Evaluate(paymentInputs{LoanAmount: mustNum(t, 1), Rate: mustNum(t, 1)}); err == nil {
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
		NewDecisionExpression[paymentInputs, breakdownOutputs](DecisionExpressionConfig{
			Entries: Entries{"principal": `1`},
		})
	})
	mustPanic("extra entry", func() {
		NewDecisionExpression[paymentInputs, amountOutput](DecisionExpressionConfig{
			Entries: Entries{"amount": `1`, "bogus": `2`},
		})
	})
	mustPanic("undefined name", func() {
		NewDecisionExpression[paymentInputs, amountOutput](DecisionExpressionConfig{
			Entries: Entries{"amount": `loan_amount + missing`},
		})
	})
	mustPanic("cycle", func() {
		NewDecisionExpression[paymentInputs, breakdownOutputs](DecisionExpressionConfig{
			Entries: Entries{"principal": `total`, "interest": `1`, "total": `principal`},
		})
	})
	mustPanic("empty outputs", func() {
		NewDecisionExpression[paymentInputs, struct{}](DecisionExpressionConfig{Entries: Entries{}})
	})
	mustPanic("duplicate output tag", func() {
		type dupOut struct {
			A BlNumber `expr:"x"`
			B BlNumber `expr:"x"`
		}
		NewDecisionExpression[paymentInputs, dupOut](DecisionExpressionConfig{Entries: Entries{"x": `1`}})
	})
	mustPanic("input/output collision", func() {
		type collideOut struct {
			LoanAmount BlNumber `expr:"loan_amount"`
		}
		NewDecisionExpression[paymentInputs, collideOut](DecisionExpressionConfig{Entries: Entries{"loan_amount": `1`}})
	})
	mustPanic("non-BlValue input field", func() {
		type badIn struct {
			Amount int `expr:"amount"`
		}
		NewDecisionExpression[badIn, amountOutput](DecisionExpressionConfig{Entries: Entries{"amount": `1`}})
	})
	mustPanic("non-BlValue output field", func() {
		type badOut struct {
			Amount string `expr:"amount"`
		}
		NewDecisionExpression[paymentInputs, badOut](DecisionExpressionConfig{Entries: Entries{"amount": `1`}})
	})
	mustPanic("invalid identifier tag", func() {
		type badName struct {
			Amount BlNumber `expr:"not a name"`
		}
		NewDecisionExpression[paymentInputs, badName](DecisionExpressionConfig{Entries: Entries{"not a name": `1`}})
	})
	mustPanic("excluded field via expr:-", func() {
		type excludedOut struct {
			Amount BlNumber `expr:"amount"`
			Hidden BlNumber `expr:"-"`
		}
		NewDecisionExpression[paymentInputs, excludedOut](DecisionExpressionConfig{Entries: Entries{"amount": `1`}})
	})
	mustPanic("unexported field", func() {
		type unexportedIn struct {
			LoanAmount BlNumber `expr:"loan_amount"`
			secret     BlNumber `expr:"secret"`
		}
		_ = unexportedIn{}.secret
		NewDecisionExpression[unexportedIn, amountOutput](DecisionExpressionConfig{Entries: Entries{"amount": `loan_amount`}})
	})
	mustPanic("duplicate function name", func() {
		f1, _ := Func[taxParams, BlNumber]("addTax", `amount * 1.2`)
		f2, _ := Func[taxParams, BlNumber]("addTax", `amount * 1.3`)
		NewDecisionExpression[paymentInputs, amountOutput](DecisionExpressionConfig{
			Entries: Entries{"amount": `loan_amount`},
			Funcs:   []UDF{f1, f2},
		})
	})
}
