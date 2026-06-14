# Example: Insurance Claim Assessment

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
| CLM-002 | Comprehensive | Fire (40) + Theft (50) | £30,000 | £1,000 | 90 | 100 | Total loss | £30,000 | £29,000 | Senior assessor referral |
| CLM-003 | Third-party only | Third-party vehicle (20) | £8,000 | £250 | 20 | 20 | Minor | £1,200 | £950 | Offer issued |
| CLM-004 | Third-party only | Collision (not covered) | £15,000 | £500 | — | — | — | — | — | Rejected: damage not covered |
| CLM-005 | Comprehensive | Weather (15) | £3,000 | £500 | 15 | 15 | Minor | £450 | £0 | Valid, no payment (below excess) |

Walking CLM-001 through the steps: comprehensive policy active and within cover;
collision and vandalism both covered ✓. Damage score 30 + 20 = 50 → Moderate.
Gross = £12,000 × 35% = £4,200; net = £4,200 − £500 = **£3,700**, below the
£25,000 threshold, so a £3,700 offer is issued.

## Implementation

!!! warning "Implementation pending"
    This example chains three sub-decisions (eligibility, severity scoring,
    settlement) — naturally three linked decision tables — with a referral
    threshold at the end. The Go implementation depends on the `decisions`
    package, which is still being built. This page documents the assessment; the
    runnable blkit code will be added once that package lands.

    In the meantime, see the authoritative
    [business spec](https://github.com/friendly-business-machines/blkit/blob/main/specs/examples/insurance-claim.spec.md),
    [Getting started](../getting-started/index.md) for orientation, and the
    [Reference](../reference/blkit.md) for the expression engine available today.

## Notes

- The three sub-decisions feed forward: eligibility gates scoring, the capped
  score selects a severity band, and the band's percentage drives settlement.
- The score is **capped at 100** before banding (see CLM-002, raw 90 vs capped
  100), and a settlement that nets below £0 yields no payment rather than a
  negative amount.
