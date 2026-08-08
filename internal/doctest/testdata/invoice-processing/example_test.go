package main

import (
	"bytes"
	"encoding/json"
	"os"
	"os/exec"
	"testing"
)

func invoice(total string, duplicate, active, valid bool) InvoiceInput {
	price := total
	if !valid {
		price = "5999"
	}
	return InvoiceInput{total, duplicate, active, true, []InvoiceLine{{"work", "1", price, "services"}}}
}
func TestLogicInvoices(t *testing.T) {
	tests := []struct {
		name     string
		in       InvoiceInput
		errors   int
		approval bool
	}{{"small", invoice("6000", false, true, true), 0, false}, {"large", invoice("50000", false, true, true), 0, true}, {"duplicate", invoice("6000", true, true, true), 1, false}, {"vendor", invoice("6000", false, false, true), 1, false}, {"lines", invoice("6000", false, true, false), 1, false}}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, e := DecideInvoice(tt.in)
			if e != nil {
				t.Fatal(e)
			}
			if len(got.ValidationErrors) != tt.errors || got.RequiresApproval != tt.approval {
				t.Errorf("got %+v", got)
			}
		})
	}
	combined := invoice("6000", true, false, false)
	got, e := DecideInvoice(combined)
	if e != nil {
		t.Fatal(e)
	}
	if len(got.ValidationErrors) != 3 {
		t.Errorf("errors=%v", got.ValidationErrors)
	}
}
func TestCommandInvoice(t *testing.T) {
	data, _ := json.Marshal(invoice("50000", false, true, true))
	cmd := exec.Command(os.Getenv("BLKIT_EXAMPLE_BINARY"))
	cmd.Stdin = bytes.NewReader(data)
	out, e := cmd.Output()
	if e != nil {
		t.Fatal(e)
	}
	var got InvoiceDecision
	if e = json.Unmarshal(out, &got); e != nil {
		t.Fatal(e)
	}
	if !got.RequiresApproval {
		t.Errorf("got %+v", got)
	}
}
