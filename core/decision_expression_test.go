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
	if !strings.Contains(md, "| principal | loan_amount / term") {
		t.Errorf("missing principal row:\n%s", md)
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
}
