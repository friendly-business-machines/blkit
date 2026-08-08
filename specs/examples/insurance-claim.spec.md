---
name: insurance-claim
description: Business process specification for assessing a motor insurance claim, scoring damage severity, and determining the settlement amount.
status: implemented
code:
  - docs/examples/insurance-claim.md
  - internal/doctest/testdata/insurance-claim/example_test.go
---

# Insurance Claim Assessment

## Overview

When a motor insurance policyholder submits a claim, the insurer must assess whether the claim is valid, score the severity of the damage, and calculate a settlement amount based on the policy terms, the vehicle value, and the damage score.

The assessment follows three sequential sub-decisions:

1. **Eligibility check** — confirm the policy is active and the incident falls within cover.
2. **Damage severity scoring** — combine reported damage categories into a score from 0 to 100.
3. **Settlement calculation** — apply the severity score and policy excess to the vehicle's current market value to produce a settlement offer.

If the settlement amount exceeds £25,000 the claim is referred to a senior assessor before an offer is made.

---

## Claim Information

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
| Damage Categories | One or more categories selected from the damage table below |

---

## Eligibility Rules

A claim is eligible for assessment only if all of the following are true.

| Rule | Condition | Result if false |
|---|---|---|
| 1 | Policy status is active | Claim rejected: policy lapsed |
| 2 | Incident date is on or after policy start date | Claim rejected: incident predates cover |
| 3 | Incident date is on or before policy end date | Claim rejected: incident after cover expiry |
| 4 | Damage categories include at least one item covered by the selected cover type | Claim rejected: damage not covered |

### Cover type coverage matrix

| Damage Category | Third-Party Only | Third-Party Fire & Theft | Comprehensive |
|---|---|---|---|
| Third-party vehicle damage | Covered | Covered | Covered |
| Third-party property damage | Covered | Covered | Covered |
| Own vehicle fire damage | Not covered | Covered | Covered |
| Own vehicle theft loss | Not covered | Covered | Covered |
| Own vehicle collision damage | Not covered | Not covered | Covered |
| Own vehicle weather damage | Not covered | Not covered | Covered |
| Own vehicle vandalism | Not covered | Not covered | Covered |

---

## Damage Severity Scoring

Each reported damage category carries a base score. The total score is the sum of all applicable base scores, capped at 100.

| Damage Category | Base Score |
|---|---|
| Third-party vehicle damage | 20 |
| Third-party property damage | 15 |
| Own vehicle fire damage | 40 |
| Own vehicle theft loss | 50 |
| Own vehicle collision damage | 30 |
| Own vehicle weather damage | 15 |
| Own vehicle vandalism | 20 |

### Severity bands

| Score Range | Severity Band |
|---|---|
| 0 – 20 | Minor |
| 21 – 50 | Moderate |
| 51 – 80 | Significant |
| 81 – 100 | Total loss |

---

## Settlement Calculation

The settlement amount is calculated as follows.

1. Multiply the vehicle market value by the severity band percentage (see table below) to get the gross settlement.
2. Subtract the policy excess from the gross settlement.
3. If the result is negative, the settlement amount is £0 (the damage cost is below the excess).

### Severity band percentages

| Severity Band | Percentage of Market Value |
|---|---|
| Minor | 15% |
| Moderate | 35% |
| Significant | 65% |
| Total loss | 100% |

### Senior assessor referral

If the calculated settlement amount (after deducting the excess) exceeds £25,000, the claim is flagged for senior assessor review before any offer is issued. The offer is withheld until the senior assessor approves, adjusts, or declines it.

---

## Outcomes

| Condition | Outcome |
|---|---|
| Eligibility check fails | Claim rejected with reason code |
| Settlement ≤ £0 | Claim valid but no payment (damage below excess) |
| £0 < Settlement ≤ £25,000 | Settlement offer issued to policyholder |
| Settlement > £25,000 | Referred to senior assessor; offer withheld pending approval |

---

## Examples

| Claim | Cover Type | Damage Categories | Vehicle Value | Excess | Raw Score | Capped Score | Severity | Gross Settlement | Net Settlement | Outcome |
|---|---|---|---|---|---|---|---|---|---|---|
| CLM-001 | Comprehensive | Collision (30) + Vandalism (20) | £12,000 | £500 | 50 | 50 | Moderate | £4,200 | £3,700 | Offer issued |
| CLM-002 | Comprehensive | Fire (40) + Theft (50) | £30,000 | £1,000 | 90 | 90 | Total loss | £30,000 | £29,000 | Senior assessor referral |
| CLM-003 | Third-party only | Third-party vehicle (20) | £8,000 | £250 | 20 | 20 | Minor | £1,200 | £950 | Offer issued |
| CLM-004 | Third-party only | Collision (not covered) | £15,000 | £500 | — | — | — | — | — | Rejected: damage not covered |
| CLM-005 | Comprehensive | Weather (15) | £3,000 | £500 | 15 | 15 | Minor | £450 | £0 | Valid, no payment (below excess) |

### Worked Example: CLM-001

The policyholder holds a comprehensive policy (active). The incident date falls within the policy period.

**Eligibility**: Policy active ✓, incident within cover period ✓, collision and vandalism are both covered under comprehensive ✓. Claim is eligible.

**Damage scoring**: Collision base score 30 + vandalism base score 20 = 50. Capped score = 50. Severity band = Moderate.

**Settlement**: Gross settlement = £12,000 × 35% = £4,200. Net settlement = £4,200 − £500 excess = £3,700.

£3,700 does not exceed the £25,000 referral threshold. A settlement offer of £3,700 is issued to the policyholder.
