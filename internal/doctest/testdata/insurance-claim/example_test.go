package main

import (
	"bytes"
	"encoding/json"
	"os"
	"os/exec"
	"testing"
)

func claim(cover string, damage []string, value, excess string) ClaimInput {
	return ClaimInput{true, "2025-06-01", "2025-01-01", "2025-12-31", cover, damage, value, excess}
}
func TestLogicClaims(t *testing.T) {
	tests := []struct {
		name                                string
		in                                  ClaimInput
		raw, capped, severity, net, outcome string
	}{{"CLM-001", claim("comprehensive", []string{"collision", "vandalism"}, "12000", "500"), "50", "50", "Moderate", "3700", "Offer issued"}, {"CLM-002", claim("comprehensive", []string{"fire", "theft"}, "30000", "1000"), "90", "90", "Total loss", "29000", "Senior assessor referral"}, {"CLM-003", claim("third_party", []string{"third_party_vehicle"}, "8000", "250"), "20", "20", "Minor", "950", "Offer issued"}, {"CLM-004", claim("third_party", []string{"collision"}, "15000", "500"), "30", "30", "Moderate", "0", "Rejected: damage not covered"}, {"CLM-005", claim("comprehensive", []string{"weather"}, "3000", "500"), "15", "15", "Minor", "0", "Valid, no payment"}}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, e := AssessClaim(tt.in)
			if e != nil {
				t.Fatal(e)
			}
			if got.RawScore != tt.raw || got.CappedScore != tt.capped || got.Severity != tt.severity || got.Net != tt.net || got.Outcome != tt.outcome {
				t.Errorf("got %+v", got)
			}
		})
	}
}
func TestCommandClaim(t *testing.T) {
	data, _ := json.Marshal(claim("comprehensive", []string{"collision", "vandalism"}, "12000", "500"))
	cmd := exec.Command(os.Getenv("BLKIT_EXAMPLE_BINARY"))
	cmd.Stdin = bytes.NewReader(data)
	out, e := cmd.Output()
	if e != nil {
		t.Fatal(e)
	}
	var got ClaimResult
	if e = json.Unmarshal(out, &got); e != nil {
		t.Fatal(e)
	}
	if got.Net != "3700" {
		t.Errorf("got %+v", got)
	}
}
