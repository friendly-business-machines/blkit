package main

import (
	"bytes"
	"encoding/json"
	"os"
	"os/exec"
	"testing"
)

func TestLogicDiscounts(t *testing.T) {
	tests := []struct {
		name            string
		in              DiscountInput
		discount, total string
	}{
		{"none", DiscountInput{"Bronze", 8, "120", 5, "Stationery", "", "March"}, "0", "120"},
		{"gold bulk", DiscountInput{"Gold", 18, "350", 15, "Electronics", "", "June"}, "10", "315"},
		{"silver furniture", DiscountInput{"Silver", 24, "600", 30, "Furniture", "", "October"}, "12", "528"},
		{"welcome", DiscountInput{"Bronze", 1, "80", 3, "Stationery", "WELCOME20", "May"}, "20", "64"},
		{"season", DiscountInput{"Platinum", 36, "900", 28, "Electronics", "BULK15", "January"}, "20", "720"}}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := CalculateDiscount(tt.in)
			if err != nil {
				t.Fatal(err)
			}
			if got.DiscountPercent != tt.discount || got.Total != tt.total {
				t.Errorf("got %+v want discount=%s total=%s", got, tt.discount, tt.total)
			}
		})
	}
}
func TestCommandDiscount(t *testing.T) {
	data, _ := json.Marshal(DiscountInput{"Gold", 18, "350", 15, "Electronics", "", "June"})
	cmd := exec.Command(os.Getenv("BLKIT_EXAMPLE_BINARY"))
	cmd.Stdin = bytes.NewReader(data)
	out, err := cmd.Output()
	if err != nil {
		t.Fatal(err)
	}
	var got DiscountResult
	if err = json.Unmarshal(out, &got); err != nil {
		t.Fatal(err)
	}
	if got.Total != "315" {
		t.Errorf("got %+v", got)
	}
}
