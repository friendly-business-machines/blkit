# Example: Australian Personal Income Tax Calculator

> An accounting service calculates an individual's annual income tax liability
> for FY 2024–25 — building taxable income from many sources, applying
> progressive brackets, levies, offsets, and HELP/HECS, then reconciling against
> amounts already paid.

## Business overview

This is the richest calculation example in the set. An accounting service
calculates an Australian individual's annual income tax liability for the
2024–25 financial year. Taxable income is first built by aggregating
assessable-income components (employment, investment, rental, business, foreign,
trust distributions, and net capital gains after the 50% individual CGT discount)
and subtracting allowable deductions. That taxable income then drives
progressive-bracket income tax, Medicare Levy, Medicare Levy Surcharge, and
HELP/HECS compulsory repayment, reduced by non-refundable offsets (LITO, SAPTO,
FITO) and reconciled against PAYG tax withheld and refundable franking credits to
produce a refund or amount payable.

### Input data (summary)

- **Assessable income components** — salary/wages, interest, unfranked and
  franked dividends (with imputation gross-up), franking credits, net rental
  (may be negative), business net income, foreign income and foreign tax paid,
  trust/partnership distributions, and capital gains/losses.
- **Deduction categories** — D1 car, D2 travel, D3 clothing/laundry, D4
  self-education, D5 other work-related, D9 gifts/donations, D10 cost of managing
  tax affairs, D12 personal deductible super (subject to the $30,000 concessional
  cap), D15 income-protection premiums.
- **Personal and reconciliation inputs** — residency status (resident / foreign
  resident / working holiday maker), age at end of FY, private hospital cover,
  family status, dependant children, HELP/HECS balance, PAYG tax withheld.

### Building taxable income

> Assessable income = Salary + Interest + Unfranked dividends + Grossed-up
> franked dividends + Net rental + Business net income + Foreign income +
> Trust/partnership share + Net capital gain

A franked dividend is grossed up by its imputation credit
(`assessable = cash dividend + franking credit`; for a fully franked dividend at
the 30% corporate rate, `franking credit = cash × 30 / 70`). The franking credit
later flows through as a **refundable** offset at reconciliation.

The net capital gain feeds in after capital-loss offset and the 50% individual
discount:

> Net capital gain = max(0, Σ gross gains − Σ current-year losses − Σ
> carry-forward losses) × discount factor (0.5 for assets held > 12 months,
> else 1.0)

Subtracting allowable deductions then gives the figure every downstream
component uses:

> Taxable income = Assessable income − Allowable deductions

### Income tax brackets (FY 2024–25)

A separate schedule applies per residency status; in every schedule, tax is the
cumulative tax at the bracket's lower edge plus the marginal rate on income
within the bracket.

#### Resident individuals

| Taxable income | Tax on this income |
|---|---|
| $0 – $18,200 | Nil |
| $18,201 – $45,000 | 16¢ for each $1 over $18,200 |
| $45,001 – $135,000 | $4,288 + 30¢ for each $1 over $45,000 |
| $135,001 – $190,000 | $31,288 + 37¢ for each $1 over $135,000 |
| $190,001 and over | $51,638 + 45¢ for each $1 over $190,000 |

#### Foreign residents

No tax-free threshold; not entitled to LITO/SAPTO; not liable for Medicare Levy.

| Taxable income | Tax on this income |
|---|---|
| $0 – $135,000 | 30¢ for each $1 |
| $135,001 – $190,000 | $40,500 + 37¢ for each $1 over $135,000 |
| $190,001 and over | $60,850 + 45¢ for each $1 over $190,000 |

#### Working holiday makers

Backpacker schedule; not entitled to LITO/SAPTO; not liable for Medicare Levy.

| Taxable income | Tax on this income |
|---|---|
| $0 – $45,000 | 15¢ for each $1 |
| $45,001 – $135,000 | $6,750 + 30¢ for each $1 over $45,000 |
| $135,001 – $190,000 | $33,750 + 37¢ for each $1 over $135,000 |
| $190,001 and over | $54,100 + 45¢ for each $1 over $190,000 |

### Levies and surcharge

- **Medicare Levy** — 2% of taxable income with a low-income phase-in (standard
  thresholds, or higher senior/pensioner thresholds when SAPTO-eligible). Foreign
  residents and working holiday makers are exempt.
- **Medicare Levy Surcharge (MLS)** — 1.0%–1.5% on taxable income, applied only
  when the individual lacks complying private hospital cover for the full year, is
  a Medicare-eligible resident, and income exceeds the base tier for their family
  status (each dependant child after the first lifts the threshold by $1,500).

### Offsets

- **LITO** — non-refundable offset for residents, $700 tapering to nil by
  $66,667.
- **SAPTO** — non-refundable offset for eligible seniors/pensioners;
  `SAPTO = max(0, max offset − 0.125 × max(0, rebate income − shade-out threshold))`.
- **FITO** — non-refundable foreign income tax offset, capped at the Australian
  tax that would have applied to the foreign-income portion.

### HELP / HECS compulsory repayment

A compulsory repayment is added when repayment income exceeds the first threshold
($54,435 for FY 2024–25), at a rate rising in bands from 1.0% to 10.0%, applied
to repayment income and capped at the outstanding loan balance:

> HELP repayment = min(outstanding balance, rate × repayment income)

### Putting it together

> Total tax = max(0, base tax − LITO − SAPTO − FITO) + Medicare Levy + MLS +
> HELP repayment
>
> Refund or payable = Total tax − (PAYG withheld + franking credits received)

The combined non-refundable offsets cannot reduce the income tax component below
zero and do not reduce Medicare Levy, MLS, or HELP repayment. Franking credits
are **refundable**, so any excess produces or increases a refund. A positive
result is the amount payable; a negative result is a refund.

### Worked examples

| # | Taxable income | Residency | Age | Private cover | Family | HELP debt | PAYG withheld | Total tax | Outcome |
|---|---|---|---|---|---|---|---|---|---|
| 1 | $15,000 | Resident | 30 | No | Single | $0 | $1,200 | $0 | Refund $1,200 |
| 2 | $50,000 | Resident | 30 | Yes | Single | $0 | $7,000 | $6,538 | Refund $462 |
| 3 | $200,000 | Resident | 45 | No | Single | $0 | $65,000 | $63,138 | Refund $1,862 |
| 4 | $35,000 | Resident | 67 | Yes | Single | $0 | $0 | $98 | Payable $98 |
| 5 | $80,000 | Foreign resident | 35 | n/a | n/a | $0 | $24,000 | $24,000 | Settled |
| 6 | $75,000 | Resident | 28 | Yes | Single | $30,000 | $14,500 | $17,413 | Payable $2,913 |

**Row 3** — a single, 45-year-old resident with no private hospital cover and
$200,000 taxable income:

- Base income tax (resident, $190,001+ bracket): $51,638 + 0.45 × ($200,000 −
  $190,000) = **$56,138**.
- LITO $0 (above $66,667); SAPTO $0 (not eligible).
- Medicare Levy: 0.02 × $200,000 = **$4,000**.
- MLS (no cover, single, Tier 3): 0.015 × $200,000 = **$3,000**.
- HELP repayment: $0.
- Total tax: $56,138 + $4,000 + $3,000 = **$63,138**.
- Reconcile against $65,000 PAYG: $63,138 − $65,000 = **−$1,862**, a refund of
  $1,862.

## Implementation

!!! warning "Implementation pending"
    This is the most involved example: a layered calculation combining bracketed
    rate tables (income tax, Medicare Levy, MLS, HELP/HECS), tapered offsets
    (LITO, SAPTO, FITO), and per-residency branching, all composed over the
    expression engine. The full, CI-tested Go implementation depends on the
    `decisions` package, which is still being built. This page documents the
    calculation; the runnable blkit code will be added once that package lands.

    In the meantime, see the authoritative
    [business spec](https://github.com/friendly-business-machines/blkit/blob/main/specs/examples/aus-personal-income-tax.spec.md),
    [Getting started](../getting-started/index.md) for orientation, and the
    [Reference](../reference/blkit.md) for the expression engine available today.

## Notes

- The progressive brackets are the canonical use case for a decision table keyed
  on an income range, with the marginal-rate arithmetic expressed per row.
- Residency status switches the entire downstream calculation — bracket schedule,
  Medicare eligibility, and offset entitlement — so it is best modelled as a
  branch over three parallel rate schedules rather than a single table.
- All monetary arithmetic should use `bl.number()` decimals; the tapers and
  gross-ups (e.g. franking credit `× 30 / 70`) accumulate rounding error quickly
  under native floating point.
- The franking credit is **refundable** while LITO/SAPTO/FITO are
  **non-refundable** — a distinction the reconciliation step must preserve, since
  only refundable offsets can push the result below zero into a refund.
