package main

import (
	"bytes"
	"encoding/json"
	"os"
	"os/exec"
	"testing"
)

func TestLogicPricing(t *testing.T) {
	tests := []struct {
		name                     string
		in                       PricingInput
		base, effective, monthly string
	}{
		{"starter", PricingInput{"Starter", "Monthly", 3, "Standard", ""}, "10", "10", "30"},
		{"pro annual", PricingInput{"Pro", "Annual", 30, "Gold", "SAVE10"}, "24", "18.63", "558.9"},
		{"enterprise", PricingInput{"Enterprise", "Annual", 150, "Platinum", ""}, "83", "56.44", "8466"},
		{"launch", PricingInput{"Pro", "Monthly", 5, "Silver", "LAUNCH20"}, "29", "24.37", "121.85"}}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := PriceSubscription(tt.in)
			if err != nil {
				t.Fatal(err)
			}
			if got.BasePerSeat != tt.base || got.EffectivePerSeat != tt.effective || got.TotalMonthly != tt.monthly {
				t.Errorf("got %+v", got)
			}
		})
	}
}
func TestCommandPricing(t *testing.T) {
	data, _ := json.Marshal(PricingInput{"Pro", "Annual", 30, "Gold", "SAVE10"})
	cmd := exec.Command(os.Getenv("BLKIT_EXAMPLE_BINARY"))
	cmd.Stdin = bytes.NewReader(data)
	out, err := cmd.Output()
	if err != nil {
		t.Fatal(err)
	}
	var got PricingResult
	if err = json.Unmarshal(out, &got); err != nil {
		t.Fatal(err)
	}
	if got.TotalMonthly != "558.9" {
		t.Errorf("got %+v", got)
	}
}
