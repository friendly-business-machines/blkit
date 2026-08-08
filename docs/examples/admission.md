# Course Admission

> A university admissions office decides whether an undergraduate application is
> admitted, waitlisted, or declined — and on what terms — from the applicant's
> academic profile.

## Business overview

A university admissions office evaluates undergraduate applications at the point
of submission. The decision is fully automated, taken in a single step with no
human review. Given information about the applicant, the office must decide:

- Whether the application is **admitted**, **waitlisted**, or **declined**.
- If admitted or waitlisted: the **maximum first-term credit load** the applicant may enrol in.
- If admitted or waitlisted: the **advising track** that will apply.

### Applicant information

| Field | Description |
|---|---|
| Aptitude score | An integer entrance-exam score, typically in the range 400–1600 |
| Grade average | The applicant's secondary-school grade point average (0.0–4.0) |
| Absence ratio | Days absent divided by total school days, as a decimal (e.g. 0.35 = 35%) |
| Enrolment status | One of: full-time, part-time, withdrawn |

### Decision rules

Decisions are evaluated in priority order. The first rule that matches all
conditions is applied; no further rules are evaluated.

| Priority | Aptitude Score | Enrolment Status | Absence Ratio | Decision | Max Credits | Track |
|---|---|---|---|---|---|---|
| 1 | 750 or above | Full-time or part-time | 30% or below | Admitted | 21 | Honors |
| 2 | 700 or above | Full-time or part-time | 40% or below | Admitted | 18 | Standard |
| 3 | 650 or above | Full-time or part-time | 40% or below | Waitlisted | 15 | Foundation |
| 4 | 600 or above | Full-time or part-time | 50% or below | Waitlisted | 12 | Support |
| 5 | Below 600 | Any | Any | Declined | — | — |
| 6 | Any | Withdrawn | Any | Declined | — | — |
| 7 | Any | Any | Above 50% | Declined | — | — |

Rules 5, 6, and 7 are catch-alls that ensure every application receives a
decision. A withdrawn applicant with a high aptitude score matches rule 6 before
reaching any of rules 1–4.

### Outcomes

| Decision | Meaning |
|---|---|
| **Admitted** | The university offers the applicant a place with the stated maximum credit load and advising track |
| **Waitlisted** | The application requires a secondary review (e.g. portfolio or interview) before a final offer; indicative terms are returned |
| **Declined** | The application does not meet the minimum criteria; no place is offered |

Declined applications return a maximum credit load of zero and no advising track.

### Worked examples

| Aptitude Score | GPA | Absence | Enrolment | Decision | Max Credits | Track |
|---|---|---|---|---|---|---|
| 780 | 3.9 | 25% | Full-time | Admitted | 21 | Honors |
| 710 | 3.2 | 38% | Part-time | Admitted | 18 | Standard |
| 660 | 2.8 | 39% | Full-time | Waitlisted | 15 | Foundation |
| 610 | 2.5 | 48% | Full-time | Waitlisted | 12 | Support |
| 580 | 3.0 | 30% | Full-time | Declined | — | — |
| 700 | 3.4 | 55% | Full-time | Declined | — | — |
| 730 | 3.7 | 28% | Withdrawn | Declined | — | — |

## Implementation

Define the application boundary and the typed handles used by the decision table.
GPA remains part of the application input, although this policy does not use it
as a decision column.

``` { .go .blkit-example title="main.go" }
package main

import (
	"encoding/json"
	"fmt"
	"os"

	bl "github.com/friendly-business-machines/blkit/core"
)

type AdmissionInput struct {
	AptitudeScore int    `json:"aptitude_score"`
	GPA           string `json:"gpa"`
	AbsenceRatio  string `json:"absence_ratio"`
	Enrolment     string `json:"enrolment"`
}

type AdmissionResult struct {
	Decision   string `json:"decision"`
	MaxCredits int    `json:"max_credits"`
	Track      string `json:"track,omitempty"`
}

type admissionVariables struct {
	Score     bl.Handle[bl.BlNumber] `expr:"score"`
	Absence   bl.Handle[bl.BlNumber] `expr:"absence"`
	Enrolment bl.Handle[bl.BlString] `expr:"enrolment"`
}

type admissionOutputs struct {
	Decision   bl.Handle[bl.BlString] `expr:"decision"`
	MaxCredits bl.Handle[bl.BlNumber] `expr:"max_credits"`
	Track      bl.Handle[bl.BlString] `expr:"track"`
}
```

The first matching row wins. Decline rows return zero credits and an empty track.

``` { .go .blkit-example title="main.go" }
var admissionTable = bl.NewDecisionTable[admissionVariables, admissionOutputs](bl.DecisionTableConfig{
	Id: "course-admission", Name: "Course admission", HitPolicy: bl.HitPolicyFirst,
	Columns: []bl.Column{
		{Label: "Score", Expr: `score`, Type: bl.TypeNumber},
		{Label: "Enrolment", Expr: `enrolment`, Type: bl.TypeString},
		{Label: "Absence", Expr: `absence`, Type: bl.TypeNumber},
	},
	Rules: bl.Rules{
		{`honors`, `>= 750`, `"full-time", "part-time"`, `<= 0.30`, `"Admitted"`, `21`, `"Honors"`},
		{`standard`, `>= 700`, `"full-time", "part-time"`, `<= 0.40`, `"Admitted"`, `18`, `"Standard"`},
		{`foundation`, `>= 650`, `"full-time", "part-time"`, `<= 0.40`, `"Waitlisted"`, `15`, `"Foundation"`},
		{`support`, `>= 600`, `"full-time", "part-time"`, `<= 0.50`, `"Waitlisted"`, `12`, `"Support"`},
		{`low-score`, `< 600`, `-`, `-`, `"Declined"`, `0`, `""`},
		{`withdrawn`, `-`, `"withdrawn"`, `-`, `"Declined"`, `0`, `""`},
		{`high-absence`, `-`, `-`, `> 0.50`, `"Declined"`, `0`, `""`},
	},
})
```

Convert caller values to blkit values and evaluate the table.

``` { .go .blkit-example title="main.go" }
func DecideAdmission(input AdmissionInput) (AdmissionResult, error) {
	score, err := bl.Number(input.AptitudeScore)
	if err != nil {
		return AdmissionResult{}, err
	}
	absence, err := bl.Number(input.AbsenceRatio)
	if err != nil {
		return AdmissionResult{}, err
	}
	enrolment, err := bl.String(input.Enrolment)
	if err != nil {
		return AdmissionResult{}, err
	}
	output, err := admissionTable.Evaluate(admissionVariables{
		Score: bl.NewHandle(score), Absence: bl.NewHandle(absence), Enrolment: bl.NewHandle(enrolment),
	})
	if err != nil {
		return AdmissionResult{}, err
	}
	return AdmissionResult{
		Decision:   output.Decision.Get().String(),
		MaxCredits: int(output.MaxCredits.Get().Decimal().IntPart()),
		Track:      output.Track.Get().String(),
	}, nil
}
```

The command reads one application and writes its decision.

``` { .go .blkit-example title="main.go" }
func main() {
	var input AdmissionInput
	if err := json.NewDecoder(os.Stdin).Decode(&input); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	result, err := DecideAdmission(input)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err := json.NewEncoder(os.Stdout).Encode(result); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
```

## Notes

- The rule order matters: priority rows are tested top-to-bottom and the first
  match wins, so the catch-all decline rules (5–7) only apply once no
  higher-priority admission rule has matched.
- The credit load and advising track are outcomes of the matched rule, not a
  separate calculation step.
