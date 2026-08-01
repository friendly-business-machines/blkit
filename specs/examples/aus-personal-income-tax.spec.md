---
name: Example — Australian Personal Income Tax Calculator
description: A tax agent calculates an Australian individual's annual income tax liability for the 2024–25 financial year, starting from assessable-income aggregation (salary, interest, franked and unfranked dividends with imputation gross-up, net rental, business income, foreign income, trust distributions, and net capital gains after the 50% individual CGT discount) less allowable deductions, then applying progressive bracket tax, Medicare Levy with low-income reduction, Medicare Levy Surcharge, Low Income Tax Offset, Seniors & Pensioners Tax Offset, Foreign Income Tax Offset, HELP/HECS compulsory repayment, and reconciling against PAYG withheld and refundable franking credits, across resident, foreign-resident, and working-holiday-maker residency statuses
status: implemented
code:
  - docs/examples/aus-personal-income-tax.md
---

# Example: Australian Personal Income Tax Calculator

## Overview

An accounting service calculates an Australian individual's annual income tax liability for the 2024–25 financial year. Taxable income is first built up by aggregating assessable-income components (employment, investment, rental, business, foreign, trust distributions, and net capital gains after the 50% individual CGT discount) and subtracting allowable deductions. That taxable income then drives progressive-bracket income tax, Medicare Levy, Medicare Levy Surcharge, and HELP/HECS compulsory repayment, reduced by non-refundable offsets (LITO, SAPTO, FITO) and reconciled against PAYG tax withheld and refundable franking credits to produce a refund or amount payable.

## Input Data

### Assessable income components

| Field | Type | Description |
|---|---|---|
| Salary, wages, allowances, bonuses | number (AUD) | Gross employment income from income statements / PAYG payment summaries |
| Interest income | number (AUD) | Bank interest, term-deposit interest, and similar returns (gross) |
| Unfranked dividends | number (AUD) | Cash amount of dividends paid without franking credits |
| Franked dividends — cash | number (AUD) | Cash component of franked dividends received |
| Franking credits received | number (AUD) | Imputation credits attached to franked dividends; also flows through as a refundable offset |
| Net rental income | number (AUD) | Gross rent less rental expenses (interest, depreciation, rates, repairs); may be negative |
| Business / sole trader net income | number (AUD) | Net profit from a business carried on by the individual |
| Foreign income | number (AUD) | Foreign-sourced income, converted to AUD |
| Foreign tax paid | number (AUD) | Foreign tax already paid on foreign income — used to cap FITO |
| Trust / partnership distributions | number (AUD) | Individual's share of net trust or partnership income |
| Gross capital gains | number (AUD) | Sum of current-year gross capital gains, before losses or discount |
| Current-year capital losses | number (AUD) | Capital losses realised in the current year |
| Prior-year capital loss carry-forward | number (AUD) | Unused capital losses from previous years |

### Deduction categories

| Field | Type | Description |
|---|---|---|
| D1 Work-related car expenses | number (AUD) | Cents-per-km or logbook method |
| D2 Work-related travel | number (AUD) | Overnight travel for work |
| D3 Work-related clothing and laundry | number (AUD) | Compulsory / protective / occupation-specific clothing only |
| D4 Self-education | number (AUD) | Study with a nexus to current employment |
| D5 Other work-related expenses | number (AUD) | Home office, tools, union fees, professional memberships |
| D9 Gifts and donations | number (AUD) | $2+ donations to DGRs |
| D10 Cost of managing tax affairs | number (AUD) | Agent fees, software, ATO interest |
| D12 Personal deductible super contributions | number (AUD) | Subject to the $30,000 concessional cap |
| D15 Income protection insurance premiums | number (AUD) | Cover held outside super |

### Personal and reconciliation inputs

| Field | Type | Description |
|---|---|---|
| Residency status | enum | `Resident`, `Foreign resident`, or `Working holiday maker` |
| Age at end of FY | number | Used to determine SAPTO eligibility and senior Medicare levy thresholds |
| Private hospital cover | boolean | Whether the individual held complying private hospital cover for the full year |
| Family status | enum | `Single` or `Family` (couple, sole parent, or family with dependants) — affects MLS thresholds |
| Dependant children | number | Count of dependant children; lifts MLS family threshold by $1,500 per child after the first |
| HELP/HECS debt balance | number (AUD) | Outstanding study/training loan balance at start of the year; zero if none |
| PAYG tax withheld | number (AUD) | Total tax already withheld by employers or paid as instalments during the year |

## Assessable Income

Assessable income aggregates every category of income the individual derived during the financial year. Each source is added at the appropriate amount; some categories require a "gross-up" or a separate sub-calculation before entering the total.

### Salary, wages, allowances, and bonuses

Gross employment income from income statements / PAYG payment summaries, including allowances, bonuses, and termination payments taxed at marginal rates.

### Interest income

Bank interest, term-deposit interest, and similar returns. Reported gross of any TFN-withheld amounts (the withheld tax is credited separately at reconciliation, alongside PAYG).

### Unfranked dividends

Dividends paid without attached franking credits. The cash amount is added to assessable income directly.

### Franked dividends (dividend imputation)

A franked dividend carries an imputation credit reflecting tax already paid by the company. The shareholder grosses up the dividend for assessable income, and the franking credit then flows through as a **refundable** tax offset at the reconciliation step.

> Assessable amount = Cash dividend + Franking credit

For a fully franked dividend at the 30% corporate tax rate:

> Franking credit = Cash dividend × 30 / 70

### Net rental income

Gross rent less directly attributable rental expenses (interest on the investment loan, property management fees, council rates, insurance, repairs, depreciation on the building and assets). Net rental income may be negative ("negative gearing"), in which case it reduces total assessable income.

### Sole trader / business net income

Net profit from a business carried on by the individual — business assessable income less business deductions, as reported on the Business and Professional Items schedule.

### Foreign income

Foreign-sourced employment, investment, or pension income, converted to AUD at the appropriate exchange rate. Foreign tax already paid on this income gives rise to a non-refundable **Foreign Income Tax Offset (FITO)**, capped at the Australian tax that would have been payable on the same foreign income.

### Trust and partnership distributions

The individual's share of net trust or partnership income for the year. Character is preserved where relevant — for example, franked distributions retain their attached franking credits, and capital-gain components retain their CGT treatment.

### Net capital gain

The net capital gain (after capital-loss offset and the 50% individual discount) flows into assessable income. See §Capital Gains Tax for the full mechanic.

### Summary

> Assessable income = Salary + Interest + Unfranked dividends + Grossed-up franked dividends + Net rental + Business net income + Foreign income + Trust/partnership share + Net capital gain

## Capital Gains Tax

A CGT event arises when an individual disposes of a CGT asset (shares, investment property, collectables, etc.). The net capital gain that flows into assessable income is calculated as follows.

### Cost base

> Cost base = Acquisition cost + Incidental costs + Capital improvements − Cost-base reductions

- **Acquisition cost** — the price paid (or market value where the acquisition was non-arm's-length).
- **Incidental costs** — stamp duty, brokerage, legal fees, conveyancing, agent's commission on sale.
- **Capital improvements** — non-deductible structural improvements that enhanced the asset.
- **Cost-base reductions** — amounts such as building depreciation previously claimed against rental income.

### Capital proceeds

The sale price actually received, or the market value where the transaction is non-arm's-length.

### Gross gain or loss

> Gross capital gain (or loss) = Capital proceeds − Cost base

A negative result is a capital loss.

### Loss offset and discount

1. Apply **current-year capital losses** against current-year gross capital gains.
2. Apply **prior-year capital losses carried forward** against any remaining gains.
3. For each remaining gain on an asset held by the individual for **more than 12 months**, apply the **50% individual CGT discount**.

Net capital losses cannot reduce other assessable income — they are carried forward indefinitely until offset against future capital gains.

> Net capital gain = max(0, Σ gross gains − Σ current-year losses − Σ carry-forward losses) × discount factor
>
> Discount factor = 0.5 for individual assets held > 12 months; otherwise 1.0

### Worked CGT mini-example

A taxpayer sells a share parcel held for 3 years for $50,000; cost base is $32,000; they have a $4,000 capital loss carried forward from a prior year.

- Gross gain = $50,000 − $32,000 = **$18,000**
- Apply carry-forward loss: $18,000 − $4,000 = **$14,000**
- Apply 50% discount (held > 12 months): $14,000 × 0.5 = **$7,000**
- Net capital gain added to assessable income: **$7,000**

## Allowable Deductions

The individual reduces assessable income by deductions in the categories below. Labels reference the standard ATO individual tax return.

### D1 Work-related car expenses

Two methods:

- **Cents-per-kilometre**: 88¢/km for FY 2024–25, capped at 5,000 work-related kilometres per car.
- **Logbook**: business-use percentage × total running costs (fuel, registration, insurance, repairs, depreciation).

### D2 Work-related travel expenses

Accommodation, meals, and incidentals on overnight work travel; transport fares between work-related destinations.

### D3 Work-related clothing, laundry, and dry-cleaning

Deductible only for compulsory uniforms, occupation-specific clothing (e.g. chef's checks), or protective clothing. Conventional clothing is not deductible.

### D4 Work-related self-education expenses

Course fees, textbooks, and travel for study with a sufficient nexus to current employment income.

### D5 Other work-related expenses

Includes:

- **Home office** — fixed-rate method at **70¢/hour** for FY 2024–25 (covers electricity, internet, phone, stationery, depreciation of office furniture), or the actual-cost method.
- Tools and equipment (subject to instant-write-off / depreciation rules).
- Union fees, professional association memberships, subscriptions to work-related journals.

### Investment / rental property expenses

Captured upstream inside the net rental income computation (see §Assessable Income → Net rental income). Listed here for completeness; not re-deducted at this step.

### D9 Gifts and donations

Donations of $2 or more to a deductible gift recipient (DGR) endorsed by the ATO.

### D10 Cost of managing tax affairs

Registered tax agent fees, tax software, ATO general-interest charge, and travel to obtain tax advice.

### D12 Personal deductible super contributions

Personal contributions to a complying superannuation fund where the member has lodged a valid Notice of Intent to claim a deduction. Counts toward the concessional contributions cap of **$30,000** for FY 2024–25 (combined with employer Superannuation Guarantee and any salary-sacrifice amounts).

### D15 Income protection insurance premiums

Premiums for income protection insurance held **outside** superannuation are deductible. Premiums paid through super are not separately deductible to the individual.

### Summary

> Allowable deductions = Σ (D1 … D15 amounts where applicable)

## Taxable Income

> Taxable income = Assessable income − Allowable deductions

Taxable income is the figure used by every downstream component — bracket-based income tax, Medicare Levy, MLS, LITO/SAPTO taper thresholds, and HELP/HECS repayment income.

## Income Tax Brackets (FY 2024–25)

A separate bracket schedule applies per residency status. In every schedule, tax is calculated as the cumulative tax at the bracket's lower edge plus the marginal rate applied to the portion of income within the bracket.

### Resident individuals

| Taxable income | Tax on this income |
|---|---|
| $0 – $18,200 | Nil |
| $18,201 – $45,000 | 16¢ for each $1 over $18,200 |
| $45,001 – $135,000 | $4,288 + 30¢ for each $1 over $45,000 |
| $135,001 – $190,000 | $31,288 + 37¢ for each $1 over $135,000 |
| $190,001 and over | $51,638 + 45¢ for each $1 over $190,000 |

### Foreign residents

No tax-free threshold. Not entitled to LITO or SAPTO. Not liable for Medicare Levy.

| Taxable income | Tax on this income |
|---|---|
| $0 – $135,000 | 30¢ for each $1 |
| $135,001 – $190,000 | $40,500 + 37¢ for each $1 over $135,000 |
| $190,001 and over | $60,850 + 45¢ for each $1 over $190,000 |

### Working holiday makers

Backpacker tax schedule. Not entitled to LITO or SAPTO. Not liable for Medicare Levy.

| Taxable income | Tax on this income |
|---|---|
| $0 – $45,000 | 15¢ for each $1 |
| $45,001 – $135,000 | $6,750 + 30¢ for each $1 over $45,000 |
| $135,001 – $190,000 | $33,750 + 37¢ for each $1 over $135,000 |
| $190,001 and over | $54,100 + 45¢ for each $1 over $190,000 |

## Medicare Levy

The Medicare Levy is 2% of taxable income, with a phase-in for low-income earners. Foreign residents and working holiday makers are exempt.

### Standard thresholds (non-senior)

| Taxable income | Levy |
|---|---|
| $0 – $27,222 | Nil |
| $27,223 – $34,027 | 10¢ for each $1 over $27,222 (phase-in) |
| $34,028 and over | 2% of taxable income |

### Senior and pensioner thresholds

Applied when the individual is eligible for SAPTO (see below).

| Taxable income | Levy |
|---|---|
| $0 – $43,020 | Nil |
| $43,021 – $53,775 | 10¢ for each $1 over $43,020 (phase-in) |
| $53,776 and over | 2% of taxable income |

## Medicare Levy Surcharge (MLS)

Applies only when **all** of the following hold:

1. The individual does **not** hold complying private hospital cover for the full year.
2. Income for MLS purposes exceeds the base tier threshold for their family status.
3. The individual is a Medicare-eligible resident.

For families with more than one dependant child, every dependant child after the first lifts each threshold by $1,500.

| Tier | Single income | Family income | Rate |
|---|---|---|---|
| Base (no surcharge) | ≤ $97,000 | ≤ $194,000 | 0% |
| Tier 1 | $97,001 – $113,000 | $194,001 – $226,000 | 1.0% |
| Tier 2 | $113,001 – $151,000 | $226,001 – $302,000 | 1.25% |
| Tier 3 | $151,001 and over | $302,001 and over | 1.5% |

MLS is calculated on taxable income (for MLS purposes), not just the amount above the threshold.

## Low Income Tax Offset (LITO)

A non-refundable offset for resident individuals.

| Taxable income | LITO |
|---|---|
| $0 – $37,500 | $700 |
| $37,501 – $45,000 | $700 − 5¢ for each $1 over $37,500 |
| $45,001 – $66,667 | $325 − 1.5¢ for each $1 over $45,000 |
| $66,668 and over | Nil |

## Seniors and Pensioners Tax Offset (SAPTO)

A non-refundable offset for residents who have reached pension age and meet the eligibility criteria. The maximum offset and shade-out (taper) threshold depend on family status; the taper rate is 12.5¢ for each $1 of rebate income above the shade-out threshold.

| Family status | Maximum offset | Shade-out threshold |
|---|---|---|
| Single | $2,230 | $32,279 |
| Couple (each partner) | $1,602 | $28,974 |
| Couple separated by illness (each partner) | $2,040 | $31,279 |

> SAPTO = max(0, maximum offset − 0.125 × max(0, rebate income − shade-out threshold))

## HELP / HECS Compulsory Repayment

A compulsory repayment is added to the tax liability when repayment income exceeds the first threshold. The repayment is the full rate applied to repayment income, capped at the outstanding loan balance.

| Repayment income | Rate |
|---|---|
| Below $54,435 | Nil |
| $54,435 – $62,850 | 1.0% |
| $62,851 – $66,620 | 2.0% |
| $66,621 – $70,618 | 2.5% |
| $70,619 – $74,855 | 3.0% |
| $74,856 – $79,346 | 3.5% |
| $79,347 – $84,107 | 4.0% |
| $84,108 – $89,154 | 4.5% |
| $89,155 – $94,503 | 5.0% |
| $94,504 – $100,174 | 5.5% |
| $100,175 – $106,185 | 6.0% |
| $106,186 – $112,556 | 6.5% |
| $112,557 – $119,309 | 7.0% |
| $119,310 – $126,467 | 7.5% |
| $126,468 – $134,056 | 8.0% |
| $134,057 – $142,100 | 8.5% |
| $142,101 – $150,626 | 9.0% |
| $150,627 – $159,663 | 9.5% |
| $159,664 and over | 10.0% |

> HELP repayment = min(outstanding balance, rate × repayment income)

## Calculation Steps

1. **Aggregate assessable income** — sum the components from §Assessable Income, including the grossed-up franked dividend amount and the net capital gain (post-discount) from §Capital Gains Tax.
2. **Sum allowable deductions** — sum the categories from §Allowable Deductions.
3. **Taxable income** = Assessable income − Allowable deductions.
4. **Base income tax** — select the bracket schedule by residency status and apply it to taxable income.
5. **Apply non-refundable offsets** — compute LITO, SAPTO, and FITO; sum them and subtract from base income tax. The combined offset cannot reduce the income tax component below zero, and these offsets do not reduce Medicare Levy, MLS, or HELP repayment. FITO is capped at the Australian tax that would have applied to the foreign-income portion of taxable income.
6. **Add Medicare Levy** — for residents only, using the senior threshold table when SAPTO-eligible, otherwise the standard table.
7. **Add Medicare Levy Surcharge** — only if private hospital cover is `false`, the individual is a Medicare-eligible resident, and income exceeds the base tier threshold (adjusted for dependants).
8. **Add HELP/HECS compulsory repayment** — looked up from the rate table and capped at the outstanding balance.
9. **Reconcile against amounts already paid or credited** — subtract `PAYG tax withheld + franking credits received` from the total. Franking credits are a **refundable** offset, so any excess produces (or increases) a refund. A positive result is the **amount payable**; a negative result is a **refund**.

> Total tax = max(0, base tax − LITO − SAPTO − FITO) + Medicare Levy + MLS + HELP repayment
>
> Refund or payable = Total tax − (PAYG withheld + franking credits received)

## Examples

| # | Taxable income | Residency | Age | Private cover | Family | HELP debt | PAYG withheld | Total tax | Outcome |
|---|---|---|---|---|---|---|---|---|---|
| 1 | $15,000 | Resident | 30 | No | Single | $0 | $1,200 | $0 | Refund $1,200 |
| 2 | $50,000 | Resident | 30 | Yes | Single | $0 | $7,000 | $6,538 | Refund $462 |
| 3 | $200,000 | Resident | 45 | No | Single | $0 | $65,000 | $63,138 | Refund $1,862 |
| 4 | $35,000 | Resident | 67 | Yes | Single | $0 | $0 | $98 | Payable $98 |
| 5 | $80,000 | Foreign resident | 35 | n/a | n/a | $0 | $24,000 | $24,000 | Settled |
| 6 | $75,000 | Resident | 28 | Yes | Single | $30,000 | $14,500 | $17,413 | Payable $2,913 |

### Worked example (row 3)

A single, 45-year-old Australian resident with no private hospital cover, no HELP debt, and $200,000 of taxable income; $65,000 of PAYG was withheld during the year.

- **Base income tax** (resident schedule, $190,001+ bracket):

  > $51,638 + 0.45 × ($200,000 − $190,000) = $51,638 + $4,500 = **$56,138**

- **LITO**: taxable income $200,000 is above $66,667 → **$0**.
- **SAPTO**: age 45, not eligible → **$0**.
- **Offsets applied**: $56,138 − $0 − $0 = **$56,138**.
- **Medicare Levy** (standard schedule, $34,028+ band): 0.02 × $200,000 = **$4,000**.
- **MLS**: no private cover, single, income $200,000 falls in Tier 3 ($151,001+): 0.015 × $200,000 = **$3,000**.
- **HELP repayment**: no debt → **$0**.
- **Total tax**: $56,138 + $4,000 + $3,000 = **$63,138**.
- **Reconcile against PAYG**: $63,138 − $65,000 = **−$1,862**, i.e. a refund of **$1,862**.

### Worked example: building taxable income

A 40-year-old single Australian resident with private hospital cover and no HELP debt has the following position for FY 2024–25.

#### Income sources

| Source | Amount entering assessable income |
|---|---|
| Salary and wages | $110,000 |
| Bank interest | $800 |
| Unfranked dividends | $300 |
| Franked dividends (cash $2,800 + franking credit $1,200) | $4,000 |
| Net rental (negatively geared) | −$2,500 |
| Foreign dividend (with $225 foreign tax paid) | $1,500 |
| Net capital gain (see CGT mini-example above) | $7,000 |

> Assessable income = $110,000 + $800 + $300 + $4,000 − $2,500 + $1,500 + $7,000 = **$121,100**

#### Deductions

| Category | Amount |
|---|---|
| D1 Car (cents-per-km) | $1,200 |
| D5 Home office | $600 |
| D9 Donations to DGRs | $300 |
| D10 Cost of managing tax affairs | $250 |
| D12 Personal super contribution | $5,000 |

> Allowable deductions = $1,200 + $600 + $300 + $250 + $5,000 = **$7,350**

#### Taxable income

> $121,100 − $7,350 = **$113,750**

#### Downstream — feeding the tax calculation

Assume PAYG tax withheld for the year was $28,500.

- **Base income tax** (resident schedule, $45,001 – $135,000 bracket):

  > $4,288 + 0.30 × ($113,750 − $45,000) = $4,288 + $20,625 = **$24,913**

- **LITO**: taxable income above $66,667 → **$0**.
- **SAPTO**: not eligible → **$0**.
- **FITO**: foreign tax paid $225; notional Australian tax on $1,500 of foreign income at the 30% marginal rate = $450; FITO = min($225, $450) = **$225**.
- **After non-refundable offsets**: $24,913 − $225 = **$24,688**.
- **Medicare Levy** (standard schedule, $34,028+ band): 0.02 × $113,750 = **$2,275**.
- **MLS**: private hospital cover held → **$0**.
- **HELP repayment**: no debt → **$0**.
- **Total tax**: $24,688 + $2,275 = **$26,963**.
- **Reconcile** against `PAYG withheld + franking credits` = $28,500 + $1,200 = $29,700:

  > $26,963 − $29,700 = **−$2,737**

- **Refund: $2,737.**
