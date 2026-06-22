package core

import "testing"

type taxParams struct {
	Amount BlNumber `expr:"amount"`
}

type addParams struct {
	A BlNumber `expr:"a"`
	B BlNumber `expr:"b"`
}

type priceEnv struct {
	Base BlNumber `expr:"base"`
}

func TestUDFCallFromExpression(t *testing.T) {
	addTax, err := Func[taxParams, BlNumber]("addTax", `amount * 1.2`)
	if err != nil {
		t.Fatalf("Func: %v", err)
	}
	// Variable argument + composition with an operator (return type is BlNumber).
	e, err := Expr[priceEnv](`addTax(base) + 5`, addTax)
	if err != nil {
		t.Fatalf("Expr: %v", err)
	}
	out, err := e.Evaluate(priceEnv{Base: mustNum(t, 100)})
	if err != nil {
		t.Fatalf("Evaluate: %v", err)
	}
	if out.String() != "125" { // 100*1.2 + 5
		t.Errorf("addTax(base)+5 = %s, want 125", out.String())
	}
	// Literal argument (the patcher wraps 100 into a BlNumber before type-check).
	e2, err := ExprNoEnv(`addTax(100)`, addTax)
	if err != nil {
		t.Fatalf("ExprNoEnv: %v", err)
	}
	out2, _ := e2.Evaluate(NoEnv{})
	if out2.String() != "120" {
		t.Errorf("addTax(100) = %s, want 120", out2.String())
	}
}

func TestUDFCallHostSide(t *testing.T) {
	addTax, _ := Func[taxParams, BlNumber]("addTax", `amount * 1.2`)
	out, err := addTax.Call(taxParams{Amount: mustNum(t, 100)})
	if err != nil {
		t.Fatalf("Call: %v", err)
	}
	if out.String() != "120" {
		t.Errorf("Call = %s, want 120", out.String())
	}
}

func TestUDFMultiParam(t *testing.T) {
	add, err := Func[addParams, BlNumber]("add", `a + b`)
	if err != nil {
		t.Fatalf("Func: %v", err)
	}
	e, _ := ExprNoEnv(`add(3, 4)`, add)
	out, _ := e.Evaluate(NoEnv{})
	if out.String() != "7" {
		t.Errorf("add(3, 4) = %s, want 7", out.String())
	}
	h, _ := add.Call(addParams{A: mustNum(t, 10), B: mustNum(t, 20)})
	if h.String() != "30" {
		t.Errorf("Call add = %s, want 30", h.String())
	}
}

func TestUDFArgTypeMismatchIsCompileError(t *testing.T) {
	addTax, _ := Func[taxParams, BlNumber]("addTax", `amount * 1.2`)
	type nameEnv struct {
		Name BlString `expr:"name"`
	}
	if _, err := Expr[nameEnv](`addTax(name)`, addTax); err == nil {
		t.Errorf("expected a compile error passing a BlString to a BlNumber parameter")
	}
}

func TestUDFUndefinedNameInBody(t *testing.T) {
	if _, err := Func[taxParams, BlNumber]("bad", `amount + missing`); err == nil {
		t.Errorf("expected an error for an undefined name in the UDF body")
	}
}

func TestUDFReturnTypeMismatch(t *testing.T) {
	// Declared to return BlNumber, but the body produces a BlString. Compiles
	// fine; the mismatch surfaces as a runtime TypeError.
	bad, err := Func[taxParams, BlNumber]("bad", `"not a number"`)
	if err != nil {
		t.Fatalf("Func: %v", err)
	}
	if _, err := bad.Call(taxParams{Amount: mustNum(t, 1)}); err == nil {
		t.Errorf("expected a TypeError for a string returned from a number UDF")
	}
}

func TestUDFComposition(t *testing.T) {
	addTax, _ := Func[taxParams, BlNumber]("addTax", `amount * 1.2`)
	withFee, err := Func[taxParams, BlNumber]("withFee", `addTax(amount) + 2`, addTax)
	if err != nil {
		t.Fatalf("Func withFee: %v", err)
	}
	out, err := withFee.Call(taxParams{Amount: mustNum(t, 100)})
	if err != nil {
		t.Fatalf("Call: %v", err)
	}
	if out.String() != "122" { // 100*1.2 + 2
		t.Errorf("withFee = %s, want 122", out.String())
	}
	// Calling an unregistered sibling fails at construction.
	if _, err := Func[taxParams, BlNumber]("bad", `addTax(amount)`); err == nil {
		t.Errorf("expected an error calling an unregistered UDF")
	}
}

func TestUDFDuplicateName(t *testing.T) {
	a, _ := Func[taxParams, BlNumber]("dup", `amount`)
	b, _ := Func[taxParams, BlNumber]("dup", `amount * 2`)
	if _, err := ExprNoEnv(`dup(1)`, a, b); err == nil {
		t.Errorf("expected an error passing two UDFs with the same name")
	}
}

func TestUDFBackwardCompatNoUDFs(t *testing.T) {
	e, err := Expr[priceEnv](`base + 1`)
	if err != nil {
		t.Fatalf("Expr: %v", err)
	}
	out, _ := e.Evaluate(priceEnv{Base: mustNum(t, 41)})
	if out.String() != "42" {
		t.Errorf("base+1 = %s, want 42", out.String())
	}
}
