# Insurance Claim Assessment

> A motor insurer assesses a claim's eligibility, scores the damage severity,
> and calculates a settlement offer — referring high-value settlements to a
> senior assessor.

## Business overview

When a motor insurance policyholder submits a claim, the insurer must assess
whether the claim is valid, score the severity of the damage, and calculate a
settlement amount based on policy terms, vehicle value, and the damage score. The
assessment follows three sequential sub-decisions:

1. **Eligibility check** — confirm the policy is active and the incident is within cover.
2. **Damage severity scoring** — combine reported damage categories into a score from 0 to 100.
3. **Settlement calculation** — apply the severity score and policy excess to the
   vehicle's current market value to produce a settlement offer.

If the settlement amount exceeds £25,000 the claim is referred to a senior
assessor before an offer is made.

### Claim information

| Field | Description |
|---|---|
| Claim Reference | Unique identifier assigned at submission |
| Policy Number | Policyholder's insurance policy identifier |
| Policy Status | Active or lapsed at the date of incident |
| Incident Date | Date the damage occurred |
| Policy Start Date | Date cover began |
| Policy End Date | Date cover expires |
| Cover Type | Third-party only, third-party fire and theft, or comprehensive |
| Vehicle Market Value | Current agreed value of the insured vehicle in GBP |
| Policy Excess | The amount the policyholder pays before the insurer contributes |
| Damage Categories | One or more categories from the damage table below |

### Eligibility rules

A claim is eligible only if **all** of the following hold:

| Rule | Condition | Result if false |
|---|---|---|
| 1 | Policy status is active | Claim rejected: policy lapsed |
| 2 | Incident date is on or after policy start date | Claim rejected: incident predates cover |
| 3 | Incident date is on or before policy end date | Claim rejected: incident after cover expiry |
| 4 | Damage categories include at least one item covered by the selected cover type | Claim rejected: damage not covered |

#### Cover type coverage matrix

| Damage Category | Third-Party Only | Third-Party Fire & Theft | Comprehensive |
|---|---|---|---|
| Third-party vehicle damage | Covered | Covered | Covered |
| Third-party property damage | Covered | Covered | Covered |
| Own vehicle fire damage | Not covered | Covered | Covered |
| Own vehicle theft loss | Not covered | Covered | Covered |
| Own vehicle collision damage | Not covered | Not covered | Covered |
| Own vehicle weather damage | Not covered | Not covered | Covered |
| Own vehicle vandalism | Not covered | Not covered | Covered |

### Damage severity scoring

Each reported damage category carries a base score. The total is the sum of all
applicable base scores, capped at 100.

| Damage Category | Base Score |
|---|---|
| Third-party vehicle damage | 20 |
| Third-party property damage | 15 |
| Own vehicle fire damage | 40 |
| Own vehicle theft loss | 50 |
| Own vehicle collision damage | 30 |
| Own vehicle weather damage | 15 |
| Own vehicle vandalism | 20 |

| Score Range | Severity Band |
|---|---|
| 0 – 20 | Minor |
| 21 – 50 | Moderate |
| 51 – 80 | Significant |
| 81 – 100 | Total loss |

### Settlement calculation

1. Multiply the vehicle market value by the severity band percentage to get the
   gross settlement.
2. Subtract the policy excess from the gross settlement.
3. If the result is negative, the settlement amount is £0 (damage below excess).

| Severity Band | Percentage of Market Value |
|---|---|
| Minor | 15% |
| Moderate | 35% |
| Significant | 65% |
| Total loss | 100% |

If the calculated settlement (after deducting the excess) exceeds £25,000, the
claim is flagged for senior assessor review and the offer is withheld pending
their decision.

### Outcomes

| Condition | Outcome |
|---|---|
| Eligibility check fails | Claim rejected with reason code |
| Settlement ≤ £0 | Claim valid but no payment (damage below excess) |
| £0 < Settlement ≤ £25,000 | Settlement offer issued to policyholder |
| Settlement > £25,000 | Referred to senior assessor; offer withheld pending approval |

### Worked examples

| Claim | Cover Type | Damage Categories | Vehicle Value | Excess | Raw Score | Capped Score | Severity | Gross | Net | Outcome |
|---|---|---|---|---|---|---|---|---|---|---|
| CLM-001 | Comprehensive | Collision (30) + Vandalism (20) | £12,000 | £500 | 50 | 50 | Moderate | £4,200 | £3,700 | Offer issued |
| CLM-002 | Comprehensive | Fire (40) + Theft (50) | £30,000 | £1,000 | 90 | 90 | Total loss | £30,000 | £29,000 | Senior assessor referral |
| CLM-003 | Third-party only | Third-party vehicle (20) | £8,000 | £250 | 20 | 20 | Minor | £1,200 | £950 | Offer issued |
| CLM-004 | Third-party only | Collision (not covered) | £15,000 | £500 | — | — | — | — | — | Rejected: damage not covered |
| CLM-005 | Comprehensive | Weather (15) | £3,000 | £500 | 15 | 15 | Minor | £450 | £0 | Valid, no payment (below excess) |

Walking CLM-001 through the steps: comprehensive policy active and within cover;
collision and vandalism both covered ✓. Damage score 30 + 20 = 50 → Moderate.
Gross = £12,000 × 35% = £4,200; net = £4,200 − £500 = **£3,700**, below the
£25,000 threshold, so a £3,700 offer is issued.

## Implementation

Define the claim boundary and the typed environment for the linked assessment.

``` { .go .blkit-example title="main.go" }
package main

import (
	"encoding/json"
	"fmt"
	bl "github.com/friendly-business-machines/blkit/core"
	"os"
)

type ClaimInput struct {
	PolicyActive bool     `json:"policy_active"`
	IncidentDate string   `json:"incident_date"`
	PolicyStart  string   `json:"policy_start"`
	PolicyEnd    string   `json:"policy_end"`
	CoverType    string   `json:"cover_type"`
	Damage       []string `json:"damage"`
	VehicleValue string   `json:"vehicle_value"`
	Excess       string   `json:"excess"`
}
type ClaimResult struct {
	Eligible    bool   `json:"eligible"`
	RawScore    string `json:"raw_score"`
	CappedScore string `json:"capped_score"`
	Severity    string `json:"severity"`
	Gross       string `json:"gross"`
	Net         string `json:"net"`
	Outcome     string `json:"outcome"`
}
type claimVars struct {
	Active   bl.Handle[bl.BlBoolean] `expr:"active"`
	Incident bl.Handle[bl.BlDate]    `expr:"incident"`
	Start    bl.Handle[bl.BlDate]    `expr:"start"`
	End      bl.Handle[bl.BlDate]    `expr:"end"`
	Covered  bl.Handle[bl.BlBoolean] `expr:"covered"`
	Raw      bl.Handle[bl.BlNumber]  `expr:"raw"`
	Value    bl.Handle[bl.BlNumber]  `expr:"value"`
	Excess   bl.Handle[bl.BlNumber]  `expr:"excess"`
}
type claimOutputs struct {
	Eligible   bl.Handle[bl.BlBoolean] `expr:"eligible"`
	Capped     bl.Handle[bl.BlNumber]  `expr:"capped"`
	Severity   bl.Handle[bl.BlString]  `expr:"severity"`
	Percentage bl.Handle[bl.BlNumber]  `expr:"percentage"`
	Gross      bl.Handle[bl.BlNumber]  `expr:"gross"`
	Net        bl.Handle[bl.BlNumber]  `expr:"net"`
	Outcome    bl.Handle[bl.BlString]  `expr:"outcome"`
}
```

The decision task contains an expression that gates eligibility, caps and bands
the score, then derives the settlement and referral outcome.

``` { .go .blkit-example title="main.go" }
var claimAssessment = bl.NewDecisionExpression[claimVars, claimOutputs](bl.DecisionExpressionConfig{
	Id: "claim-assessment-calculation",
	Entries: bl.Entries{
		"eligible":   `active and incident>=start and incident<=end and covered`,
		"capped":     `clamp(raw,0,100)`,
		"severity":   `if capped<=20 then "Minor" else if capped<=50 then "Moderate" else if capped<=80 then "Significant" else "Total loss"`,
		"percentage": `if severity="Minor" then 15 else if severity="Moderate" then 35 else if severity="Significant" then 65 else 100`,
		"gross":      `if eligible then value*percentage/100 else 0`,
		"net":        `max([0,gross-excess])`,
		"outcome":    `if not(eligible) then "Rejected: damage not covered" else if net=0 then "Valid, no payment" else if net>25000 then "Senior assessor referral" else "Offer issued"`,
	},
})

var claimDecision = bl.NewDecisionTask[claimVars, claimOutputs](bl.DecisionTaskConfig{
	Id:   "claim-assessment",
	Name: "Claim assessment",
})

var _ = claimDecision.Graph(
	bl.Edge(claimDecision.In.Active, claimAssessment.In.Active),
	bl.Edge(claimDecision.In.Incident, claimAssessment.In.Incident),
	bl.Edge(claimDecision.In.Start, claimAssessment.In.Start),
	bl.Edge(claimDecision.In.End, claimAssessment.In.End),
	bl.Edge(claimDecision.In.Covered, claimAssessment.In.Covered),
	bl.Edge(claimDecision.In.Raw, claimAssessment.In.Raw),
	bl.Edge(claimDecision.In.Value, claimAssessment.In.Value),
	bl.Edge(claimDecision.In.Excess, claimAssessment.In.Excess),
	bl.Edge(claimAssessment.Out.Eligible, claimDecision.Out.Eligible),
	bl.Edge(claimAssessment.Out.Capped, claimDecision.Out.Capped),
	bl.Edge(claimAssessment.Out.Severity, claimDecision.Out.Severity),
	bl.Edge(claimAssessment.Out.Percentage, claimDecision.Out.Percentage),
	bl.Edge(claimAssessment.Out.Gross, claimDecision.Out.Gross),
	bl.Edge(claimAssessment.Out.Net, claimDecision.Out.Net),
	bl.Edge(claimAssessment.Out.Outcome, claimDecision.Out.Outcome),
)

var damageScores = map[string]int{"third_party_vehicle": 20, "third_party_property": 15, "fire": 40, "theft": 50, "collision": 30, "weather": 15, "vandalism": 20}

func damageCovered(cover, damage string) bool {
	if cover == "comprehensive" {
		return true
	}
	if damage == "third_party_vehicle" || damage == "third_party_property" {
		return true
	}
	return cover == "third_party_fire_theft" && (damage == "fire" || damage == "theft")
}
```

``` { .go .blkit-example title="main.go" }
func AssessClaim(in ClaimInput) (ClaimResult, error) {
	incident, e := bl.Date(in.IncidentDate)
	if e != nil {
		return ClaimResult{}, e
	}
	start, e := bl.Date(in.PolicyStart)
	if e != nil {
		return ClaimResult{}, e
	}
	end, e := bl.Date(in.PolicyEnd)
	if e != nil {
		return ClaimResult{}, e
	}
	raw := 0
	covered := false
	for _, d := range in.Damage {
		raw += damageScores[d]
		covered = covered || damageCovered(in.CoverType, d)
	}
	rawNumber, _ := bl.Number(raw)
	value, e := bl.Number(in.VehicleValue)
	if e != nil {
		return ClaimResult{}, e
	}
	excess, e := bl.Number(in.Excess)
	if e != nil {
		return ClaimResult{}, e
	}
	active, _ := bl.Boolean(in.PolicyActive)
	coveredValue, _ := bl.Boolean(covered)
	result, e := claimDecision.Evaluate(claimVars{
		bl.NewHandle(active), bl.NewHandle(incident), bl.NewHandle(start),
		bl.NewHandle(end), bl.NewHandle(coveredValue), bl.NewHandle(rawNumber),
		bl.NewHandle(value), bl.NewHandle(excess),
	})
	if e != nil {
		return ClaimResult{}, e
	}
	return ClaimResult{
		result.Eligible.Get().Native(), rawNumber.String(),
		result.Capped.Get().String(), result.Severity.Get().String(),
		result.Gross.Get().String(), result.Net.Get().String(),
		result.Outcome.Get().String(),
	}, nil
}
func main() {
	var in ClaimInput
	if e := json.NewDecoder(os.Stdin).Decode(&in); e != nil {
		fmt.Fprintln(os.Stderr, e)
		os.Exit(1)
	}
	out, e := AssessClaim(in)
	if e != nil {
		fmt.Fprintln(os.Stderr, e)
		os.Exit(1)
	}
	if e = json.NewEncoder(os.Stdout).Encode(out); e != nil {
		fmt.Fprintln(os.Stderr, e)
		os.Exit(1)
	}
}
```

## Notes

- The three sub-decisions feed forward: eligibility gates scoring, the capped
  score selects a severity band, and the band's percentage drives settlement.
- The score is **capped at 100** before banding. CLM-002's raw score of 90
  remains 90 after capping, and a settlement that nets below £0 yields no payment
  rather than a negative amount.
