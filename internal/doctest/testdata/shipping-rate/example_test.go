package main

import (
	"bytes"
	"encoding/json"
	"os"
	"os/exec"
	"testing"
)

func TestLogicShippingRates(t *testing.T) {
	tests := []struct {
		name       string
		input      ShippingInput
		volumetric string
		billable   string
		total      string
	}{
		{"regional express", ShippingInput{"3.2", "40", "30", "20", 2, "express"}, "4.8", "4.8", "38.88"},
		{"domestic standard", ShippingInput{"10", "20", "15", "10", 1, "standard"}, "0.6", "10", "21.6"},
		{"international overnight", ShippingInput{"0.5", "60", "40", "30", 3, "overnight"}, "14.4", "14.4", "223.02"},
		{"domestic overnight", ShippingInput{"2", "25", "15", "10", 1, "overnight"}, "0.75", "2", "21.6"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			got, err := CalculateShipping(test.input)
			if err != nil {
				t.Fatalf("CalculateShipping: %v", err)
			}
			if got.VolumetricWeightKG != test.volumetric || got.BillableWeightKG != test.billable || got.Total != test.total {
				t.Errorf("got volumetric=%s billable=%s total=%s; want %s %s %s", got.VolumetricWeightKG, got.BillableWeightKG, got.Total, test.volumetric, test.billable, test.total)
			}
		})
	}
}

func TestCommandShippingRate(t *testing.T) {
	input := ShippingInput{"3.2", "40", "30", "20", 2, "express"}
	data, _ := json.Marshal(input)
	cmd := exec.Command(os.Getenv("BLKIT_EXAMPLE_BINARY"))
	cmd.Stdin = bytes.NewReader(data)
	output, err := cmd.Output()
	if err != nil {
		t.Fatalf("run command: %v", err)
	}
	var got ShippingQuote
	if err := json.Unmarshal(output, &got); err != nil {
		t.Fatalf("decode output: %v", err)
	}
	if got.Total != "38.88" {
		t.Errorf("total = %s, want 38.88", got.Total)
	}
}
