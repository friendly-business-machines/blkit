package main

import (
	"bytes"
	"encoding/json"
	"os"
	"os/exec"
	"testing"
)

func validOrder() FulfilmentInput {
	return FulfilmentInput{"O1", "C1", []Item{{"SKU", 1}}, "token", Address{"1 Road", "City", "123", "GB"}, true, true}
}
func TestLogicFulfilmentRoutes(t *testing.T) {
	tests := []struct {
		name           string
		stock, payment bool
		want           string
	}{{"success", true, true, "end-success"}, {"backorder", false, false, "end-backorder"}, {"payment", true, false, "end-payment-error"}}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			in := validOrder()
			in.StockAvailable = tt.stock
			in.PaymentOK = tt.payment
			got, e := DecideFulfilment(in)
			if e != nil {
				t.Fatal(e)
			}
			if got.Route != tt.want {
				t.Errorf("route=%s", got.Route)
			}
		})
	}
	bad := validOrder()
	bad.Items = nil
	got, e := DecideFulfilment(bad)
	if e != nil {
		t.Fatal(e)
	}
	if got.Route != "invalid-order" || len(got.ValidationErrors) == 0 {
		t.Errorf("got %+v", got)
	}
}
func TestCommandFulfilment(t *testing.T) {
	in := validOrder()
	data, _ := json.Marshal(in)
	cmd := exec.Command(os.Getenv("BLKIT_EXAMPLE_BINARY"))
	cmd.Stdin = bytes.NewReader(data)
	out, e := cmd.Output()
	if e != nil {
		t.Fatal(e)
	}
	var got FulfilmentDecision
	if e = json.Unmarshal(out, &got); e != nil {
		t.Fatal(e)
	}
	if got.Route != "end-success" {
		t.Errorf("got %+v", got)
	}
}
