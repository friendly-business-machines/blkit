package main

import (
	"bytes"
	"encoding/json"
	"os"
	"os/exec"
	"testing"
)

func TestLogicOnboarding(t *testing.T) {
	tests := []struct {
		name        string
		in          OnboardingInput
		result      string
		outstanding int
	}{{"Alice", OnboardingInput{"2024-03-04", 2, 3, 1}, "welcome", 0}, {"Bob", OnboardingInput{"2024-03-04", 4, 6, 3}, "escalate", 1}, {"Carol", OnboardingInput{"2024-03-11", 5, 5, 5}, "welcome", 0}, {"David", OnboardingInput{"2024-03-11", 6, 7, 6}, "escalate", 3}}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, e := DecideOnboarding(tt.in)
			if e != nil {
				t.Fatal(e)
			}
			if got.Result != tt.result || len(got.Outstanding) != tt.outstanding {
				t.Errorf("got %+v", got)
			}
		})
	}
	got, e := DecideOnboarding(OnboardingInput{"2024-03-08", 1, 1, 1})
	if e != nil {
		t.Fatal(e)
	}
	if got.Deadline != "2024-03-15" {
		t.Errorf("deadline=%s", got.Deadline)
	}
}
func TestCommandOnboarding(t *testing.T) {
	data, _ := json.Marshal(OnboardingInput{"2024-03-04", 4, 6, 3})
	cmd := exec.Command(os.Getenv("BLKIT_EXAMPLE_BINARY"))
	cmd.Stdin = bytes.NewReader(data)
	out, e := cmd.Output()
	if e != nil {
		t.Fatal(e)
	}
	var got OnboardingDecision
	if e = json.Unmarshal(out, &got); e != nil {
		t.Fatal(e)
	}
	if got.Result != "escalate" || len(got.Outstanding) != 1 {
		t.Errorf("got %+v", got)
	}
}
