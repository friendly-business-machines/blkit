package main

import (
	"bytes"
	"encoding/json"
	"os"
	"os/exec"
	"testing"
)

func tax(income, residency string, age int, cover bool, debt, payg string) TaxInput {
	return TaxInput{income, residency, age, cover, "single", debt, payg, "0"}
}
func TestLogicTaxRows(t *testing.T) {
	tests := []struct {
		name                    string
		in                      TaxInput
		total, balance, outcome string
	}{{"low", tax("15000", "resident", 30, false, "0", "1200"), "0", "-1200", "refund"}, {"middle", tax("50000", "resident", 30, true, "0", "7000"), "6538", "-462", "refund"}, {"high", tax("200000", "resident", 45, false, "0", "65000"), "63138", "-1862", "refund"}, {"senior", tax("35000", "resident", 67, true, "0", "0"), "98", "98", "payable"}, {"foreign", tax("80000", "foreign", 35, false, "0", "24000"), "24000", "0", "settled"}, {"help", tax("75000", "resident", 28, true, "30000", "14500"), "17413", "2913", "payable"}}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, e := CalculateTax(tt.in)
			if e != nil {
				t.Fatal(e)
			}
			if got.TotalTax != tt.total || got.Balance != tt.balance || got.Outcome != tt.outcome {
				t.Errorf("got %+v", got)
			}
		})
	}
}
func TestLogicBuildTaxableIncome(t *testing.T) {
	capitalGain, taxable, err := BuildTaxableIncome(IncomeInput{GrossCapitalGains: "18000", PriorCapitalLosses: "4000", CGTDiscountEligible: true})
	if err != nil {
		t.Fatal(err)
	}
	if capitalGain != "7000" || taxable != "7000" {
		t.Errorf("gain=%s taxable=%s", capitalGain, taxable)
	}
	capitalGain, taxable, err = BuildTaxableIncome(IncomeInput{Salary: "110000", Interest: "800", UnfrankedDividends: "300", FrankedCash: "2800", FrankingCredits: "1200", NetRental: "-2500", ForeignIncome: "1500", GrossCapitalGains: "18000", PriorCapitalLosses: "4000", Deductions: "7350", CGTDiscountEligible: true})
	if err != nil {
		t.Fatal(err)
	}
	if capitalGain != "7000" || taxable != "113750" {
		t.Errorf("gain=%s taxable=%s", capitalGain, taxable)
	}
}

func TestCommandTax(t *testing.T) {
	data, _ := json.Marshal(tax("200000", "resident", 45, false, "0", "65000"))
	cmd := exec.Command(os.Getenv("BLKIT_EXAMPLE_BINARY"))
	cmd.Stdin = bytes.NewReader(data)
	out, e := cmd.Output()
	if e != nil {
		t.Fatal(e)
	}
	var got TaxResult
	if e = json.Unmarshal(out, &got); e != nil {
		t.Fatal(e)
	}
	if got.TotalTax != "63138" || got.Balance != "-1862" {
		t.Errorf("got %+v", got)
	}
}
