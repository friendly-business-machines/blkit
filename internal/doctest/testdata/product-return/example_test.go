package main

import (
	"bytes"
	"encoding/json"
	"os"
	"os/exec"
	"testing"
)

func baseReturn(reason, grade, preferred, price string, qty int, stock bool) ReturnInput {
	return ReturnInput{"2025-01-01", "2025-01-13", "2025-01-13", "2025-01-21", true, qty, qty, reason, grade, preferred, stock, price}
}
func TestLogicReturns(t *testing.T) {
	tests := []struct {
		name                                          string
		in                                            ReturnInput
		percent, amount, resolution, resolutionAmount string
	}{
		{"defective", baseReturn("DEFECTIVE", "D", "replacement", "80", 1, true), "100", "80", "Replacement dispatched", "80"},
		{"used", baseReturn("NO_LONGER_NEEDED", "C", "refund", "12", 3, false), "50", "18", "Partial refund", "18"},
		{"credit", baseReturn("NOT_AS_DESCRIBED", "B", "store_credit", "350", 1, false), "100", "350", "Store credit", "385"},
		{"duplicate", baseReturn("DUPLICATE_ORDER", "A", "refund", "8", 2, false), "100", "16", "Full refund", "16"},
		{"damaged", baseReturn("NO_LONGER_NEEDED", "D", "refund", "450", 1, false), "0", "0", "Declined", "0"}}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, e := CalculateReturn(tt.in)
			if e != nil {
				t.Fatal(e)
			}
			if got.RefundPercent != tt.percent || got.RefundAmount != tt.amount || got.Resolution != tt.resolution || got.ResolutionAmount != tt.resolutionAmount {
				t.Errorf("got %+v", got)
			}
		})
	}
}
func TestCommandReturn(t *testing.T) {
	data, _ := json.Marshal(baseReturn("NOT_AS_DESCRIBED", "B", "store_credit", "350", 1, false))
	cmd := exec.Command(os.Getenv("BLKIT_EXAMPLE_BINARY"))
	cmd.Stdin = bytes.NewReader(data)
	out, e := cmd.Output()
	if e != nil {
		t.Fatal(e)
	}
	var got ReturnResult
	if e = json.Unmarshal(out, &got); e != nil {
		t.Fatal(e)
	}
	if got.ResolutionAmount != "385" {
		t.Errorf("got %+v", got)
	}
}
