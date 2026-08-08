package main

import (
	"bytes"
	"encoding/json"
	bl "github.com/friendly-business-machines/blkit/core"
	"os"
	"os/exec"
	"testing"
)

func TestLogicExpenseRoutes(t *testing.T) {
	tests := []struct{ name, amount, category, level, want string }{
		{"automatic", "30", "meals", "junior", "Automatic"},
		{"manager", "450", "travel", "junior", "Manager"},
		{"junior equipment", "450", "equipment", "junior", "Finance Director"},
		{"executive", "1200", "other", "executive", "Manager"},
		{"large", "3500", "equipment", "senior", "Finance Director"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			amount, _ := bl.Number(tt.amount)
			category, _ := bl.String(tt.category)
			level, _ := bl.String(tt.level)
			got, err := expenseRouting.Evaluate(routeInputs{
				Amount:   bl.NewHandle(amount),
				Category: bl.NewHandle(category),
				Level:    bl.NewHandle(level),
			})
			if err != nil {
				t.Fatal(err)
			}
			if got.Route.Get().String() != tt.want {
				t.Errorf("route=%s want %s", got.Route.Get().String(), tt.want)
			}
		})
	}
}
func TestCommandExpenseRoute(t *testing.T) {
	data, _ := json.Marshal(ExpenseInput{"450", "travel", "junior"})
	cmd := exec.Command(os.Getenv("BLKIT_EXAMPLE_BINARY"))
	cmd.Stdin = bytes.NewReader(data)
	out, err := cmd.Output()
	if err != nil {
		t.Fatal(err)
	}
	var got ExpenseRoute
	if err = json.Unmarshal(out, &got); err != nil {
		t.Fatal(err)
	}
	if got.Route != "Manager" {
		t.Errorf("got %s", got.Route)
	}
}
