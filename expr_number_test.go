package blkit

import (
	"testing"

	"github.com/shopspring/decimal"
)

func TestNumberConstruction(t *testing.T) {
	if n, err := Number(30); err != nil || n.String() != "30" {
		t.Errorf("Number(30) = %v, %v", n, err)
	}
	if n, _ := Number(3.14159); n.String() != "3.14159" {
		t.Errorf("Number(3.14159) = %v", n)
	}
	if n, _ := Number(decimal.RequireFromString("1500.50")); n.String() != "1500.5" {
		t.Errorf("Number(1500.50) = %v", n)
	}
	if n, _ := Number("$1,234.56"); n.String() != "1234.56" {
		t.Errorf(`Number("$1,234.56") = %v`, n)
	}
	if n, _ := Number(true); n.String() != "1" {
		t.Errorf("Number(true) = %v", n)
	}
}

func TestNumberConstructionErrors(t *testing.T) {
	if _, err := Number("not a number"); err == nil {
		t.Errorf("expected error for unparseable string")
	}
}

func TestNumberDecimalAccessor(t *testing.T) {
	n, _ := Number(42)
	if n.Decimal().IntPart() != 42 {
		t.Errorf("Decimal() = %v", n.Decimal())
	}
}

func TestNumberFunctions(t *testing.T) {
	assertEval(t, map[string]string{
		`roundHalfEven(2.5, 0)`: "2",
		`roundHalfEven(3.5, 0)`: "4",
		`floor(-1.56, 1)`:       "-1.6",
		`ceiling(-1.56, 1)`:     "-1.5",
		`round(2.345, 2)`:       "2.35",
		`roundUp(5.1, 0)`:       "6",
		`roundDown(5.9, 0)`:     "5",
		`roundHalfUp(5.5, 0)`:   "6",
		`roundHalfUp(5.1, 0)`:   "5",
		`roundHalfDown(5.5, 0)`: "5",
		`roundHalfDown(5.9, 0)`: "6",
		`abs(-10)`:              "10",
		`modulo(-10, 3)`:        "2",
		`sqrt(16)`:              "4",
		`sqrt(0)`:               "0",
		`sqrt(-1)`:              "null",
		`odd(5)`:                "true",
		`even(2)`:               "true",
		`isPositive(5)`:         "true",
		`isNegative(-3)`:        "true",
		`isZero(0)`:             "true",
		`clamp(150, 0, 100)`:    "100",
		`clamp(50, 0, 100)`:     "50",
		`clamp(5, 100, 0)`:      "null",
		`log(100)`:              "2",
		`log(8, 2)`:             "3",
	})
}

func TestNumberLogExp(t *testing.T) {
	// ln(e) ≈ 1, exp(1) ≈ 2.718...
	if got := evalNil(t, `ln(2.718281828459045)`); got[:1] != "0" && got[:1] != "1" {
		t.Errorf("ln(e) = %q", got)
	}
	if got := evalNil(t, `ln(0)`); got != "null" {
		t.Errorf("ln(0) = %q, want null", got)
	}
	if got := evalNil(t, `ln(-1)`); got != "null" {
		t.Errorf("ln(-1) = %q, want null", got)
	}
}

func TestNumberSemantics(t *testing.T) {
	assertEval(t, map[string]string{
		`(-2) ** 0.5`:   "null", // complex
		`3.0 = 3.00`:    "true",
		`modulo(10, 0)`: "null",
	})
}
