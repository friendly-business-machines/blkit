package main

import (
	"bytes"
	"encoding/json"
	"os"
	"os/exec"
	"testing"
)

func TestLogicAdmission(t *testing.T) {
	tests := []struct {
		name  string
		input AdmissionInput
		want  AdmissionResult
	}{
		{"honors", AdmissionInput{780, "3.9", "0.25", "full-time"}, AdmissionResult{"Admitted", 21, "Honors"}},
		{"standard", AdmissionInput{710, "3.2", "0.38", "part-time"}, AdmissionResult{"Admitted", 18, "Standard"}},
		{"foundation", AdmissionInput{660, "2.8", "0.39", "full-time"}, AdmissionResult{"Waitlisted", 15, "Foundation"}},
		{"support", AdmissionInput{610, "2.5", "0.48", "full-time"}, AdmissionResult{"Waitlisted", 12, "Support"}},
		{"low score", AdmissionInput{580, "3.0", "0.30", "full-time"}, AdmissionResult{"Declined", 0, ""}},
		{"high absence", AdmissionInput{700, "3.4", "0.55", "full-time"}, AdmissionResult{"Declined", 0, ""}},
		{"withdrawn", AdmissionInput{730, "3.7", "0.28", "withdrawn"}, AdmissionResult{"Declined", 0, ""}},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			got, err := DecideAdmission(test.input)
			if err != nil {
				t.Fatalf("DecideAdmission: %v", err)
			}
			if got != test.want {
				t.Errorf("got %+v, want %+v", got, test.want)
			}
		})
	}
}

func TestCommandAdmission(t *testing.T) {
	data, _ := json.Marshal(AdmissionInput{780, "3.9", "0.25", "full-time"})
	cmd := exec.Command(os.Getenv("BLKIT_EXAMPLE_BINARY"))
	cmd.Stdin = bytes.NewReader(data)
	output, err := cmd.Output()
	if err != nil {
		t.Fatalf("run: %v", err)
	}
	var got AdmissionResult
	if err := json.Unmarshal(output, &got); err != nil {
		t.Fatal(err)
	}
	if got.Decision != "Admitted" || got.Track != "Honors" {
		t.Errorf("got %+v", got)
	}
}
