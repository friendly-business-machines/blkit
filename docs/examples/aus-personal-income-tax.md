# Australian Personal Income Tax Calculator

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

The liability schedules can be compiled into a `DecisionTask` containing one
typed `DecisionExpression` and reused for many returns. This application accepts
taxable income after the assessable-income and deduction calculation described
above.

``` { .go .blkit-example title="main.go" }
package main

import (
	"encoding/json"
	"fmt"
	bl "github.com/friendly-business-machines/blkit/core"
	"os"
)

type TaxInput struct {
	TaxableIncome   string `json:"taxable_income"`
	Residency       string `json:"residency"`
	Age             int    `json:"age"`
	PrivateCover    bool   `json:"private_cover"`
	FamilyStatus    string `json:"family_status"`
	HELPDebt        string `json:"help_debt"`
	PAYGWithheld    string `json:"payg_withheld"`
	FrankingCredits string `json:"franking_credits"`
}
type TaxResult struct {
	BaseTax  string `json:"base_tax"`
	LITO     string `json:"lito"`
	SAPTO    string `json:"sapto"`
	Medicare string `json:"medicare"`
	MLS      string `json:"mls"`
	HELP     string `json:"help"`
	TotalTax string `json:"total_tax"`
	Balance  string `json:"balance"`
	Outcome  string `json:"outcome"`
}
type taxVars struct {
	Income    bl.Handle[bl.BlNumber]  `expr:"income"`
	Residency bl.Handle[bl.BlString]  `expr:"residency"`
	Age       bl.Handle[bl.BlNumber]  `expr:"age"`
	Private   bl.Handle[bl.BlBoolean] `expr:"private_cover"`
	Family    bl.Handle[bl.BlString]  `expr:"family"`
	Debt      bl.Handle[bl.BlNumber]  `expr:"debt"`
	PAYG      bl.Handle[bl.BlNumber]  `expr:"payg"`
	Franking  bl.Handle[bl.BlNumber]  `expr:"franking"`
}
type taxOutputs struct {
	BaseTax   bl.Handle[bl.BlNumber] `expr:"base_tax"`
	LITO      bl.Handle[bl.BlNumber] `expr:"lito"`
	SAPTO     bl.Handle[bl.BlNumber] `expr:"sapto"`
	IncomeTax bl.Handle[bl.BlNumber] `expr:"income_tax"`
	Medicare  bl.Handle[bl.BlNumber] `expr:"medicare"`
	MLS       bl.Handle[bl.BlNumber] `expr:"mls"`
	HELPRate  bl.Handle[bl.BlNumber] `expr:"help_rate"`
	HELP      bl.Handle[bl.BlNumber] `expr:"help"`
	TotalTax  bl.Handle[bl.BlNumber] `expr:"total_tax"`
	Balance   bl.Handle[bl.BlNumber] `expr:"balance"`
	Outcome   bl.Handle[bl.BlString] `expr:"outcome"`
}
```

Taxable income itself can also be built with exact decimal arithmetic. The input
keeps already-calculated category totals so the example stays focused on tax
composition rather than tax-return form plumbing.

``` { .go .blkit-example title="main.go" }
type IncomeInput struct {
	Salary, Interest, UnfrankedDividends, FrankedCash, FrankingCredits, NetRental, BusinessIncome, ForeignIncome, TrustDistributions, GrossCapitalGains, CurrentCapitalLosses, PriorCapitalLosses, Deductions string
	CGTDiscountEligible                                                                                                                                                                                       bool
}
type incomeVars struct {
	Salary        bl.Handle[bl.BlNumber]  `expr:"salary"`
	Interest      bl.Handle[bl.BlNumber]  `expr:"interest"`
	Unfranked     bl.Handle[bl.BlNumber]  `expr:"unfranked"`
	FrankedCash   bl.Handle[bl.BlNumber]  `expr:"franked_cash"`
	Franking      bl.Handle[bl.BlNumber]  `expr:"franking"`
	Rental        bl.Handle[bl.BlNumber]  `expr:"rental"`
	Business      bl.Handle[bl.BlNumber]  `expr:"business"`
	Foreign       bl.Handle[bl.BlNumber]  `expr:"foreign"`
	Trust         bl.Handle[bl.BlNumber]  `expr:"trust"`
	Gains         bl.Handle[bl.BlNumber]  `expr:"gains"`
	CurrentLosses bl.Handle[bl.BlNumber]  `expr:"current_losses"`
	PriorLosses   bl.Handle[bl.BlNumber]  `expr:"prior_losses"`
	Deductions    bl.Handle[bl.BlNumber]  `expr:"deductions"`
	Discount      bl.Handle[bl.BlBoolean] `expr:"discount"`
}
type incomeOutputs struct {
	NetCapitalGain bl.Handle[bl.BlNumber] `expr:"net_capital_gain"`
	Assessable     bl.Handle[bl.BlNumber] `expr:"assessable"`
	Taxable        bl.Handle[bl.BlNumber] `expr:"taxable"`
}

var incomeExpression = bl.NewDecisionExpression[incomeVars, incomeOutputs](bl.DecisionExpressionConfig{
	Id: "taxable-income-calculation",
	Entries: bl.Entries{
		"net_capital_gain": `max([0,gains-current_losses-prior_losses])*(if discount then 0.5 else 1)`,
		"assessable":       `salary+interest+unfranked+franked_cash+franking+rental+business+foreign+trust+net_capital_gain`,
		"taxable":          `max([0,assessable-deductions])`,
	},
})

var incomeDecision = bl.NewDecisionTask[incomeVars, incomeOutputs](bl.DecisionTaskConfig{
	Id:   "taxable-income",
	Name: "Taxable income",
})

var _ = incomeDecision.Graph(
	bl.Edge(incomeDecision.In.Salary, incomeExpression.In.Salary),
	bl.Edge(incomeDecision.In.Interest, incomeExpression.In.Interest),
	bl.Edge(incomeDecision.In.Unfranked, incomeExpression.In.Unfranked),
	bl.Edge(incomeDecision.In.FrankedCash, incomeExpression.In.FrankedCash),
	bl.Edge(incomeDecision.In.Franking, incomeExpression.In.Franking),
	bl.Edge(incomeDecision.In.Rental, incomeExpression.In.Rental),
	bl.Edge(incomeDecision.In.Business, incomeExpression.In.Business),
	bl.Edge(incomeDecision.In.Foreign, incomeExpression.In.Foreign),
	bl.Edge(incomeDecision.In.Trust, incomeExpression.In.Trust),
	bl.Edge(incomeDecision.In.Gains, incomeExpression.In.Gains),
	bl.Edge(incomeDecision.In.CurrentLosses, incomeExpression.In.CurrentLosses),
	bl.Edge(incomeDecision.In.PriorLosses, incomeExpression.In.PriorLosses),
	bl.Edge(incomeDecision.In.Deductions, incomeExpression.In.Deductions),
	bl.Edge(incomeDecision.In.Discount, incomeExpression.In.Discount),
	bl.Edge(incomeExpression.Out.NetCapitalGain, incomeDecision.Out.NetCapitalGain),
	bl.Edge(incomeExpression.Out.Assessable, incomeDecision.Out.Assessable),
	bl.Edge(incomeExpression.Out.Taxable, incomeDecision.Out.Taxable),
)

func BuildTaxableIncome(in IncomeInput) (string, string, error) {
	values := []string{in.Salary, in.Interest, in.UnfrankedDividends, in.FrankedCash, in.FrankingCredits, in.NetRental, in.BusinessIncome, in.ForeignIncome, in.TrustDistributions, in.GrossCapitalGains, in.CurrentCapitalLosses, in.PriorCapitalLosses, in.Deductions}
	numbers := make([]bl.BlNumber, len(values))
	for i, s := range values {
		if s == "" {
			s = "0"
		}
		n, e := bl.Number(s)
		if e != nil {
			return "", "", e
		}
		numbers[i] = n
	}
	discount, _ := bl.Boolean(in.CGTDiscountEligible)
	v, e := incomeDecision.Evaluate(incomeVars{
		bl.NewHandle(numbers[0]), bl.NewHandle(numbers[1]), bl.NewHandle(numbers[2]),
		bl.NewHandle(numbers[3]), bl.NewHandle(numbers[4]), bl.NewHandle(numbers[5]),
		bl.NewHandle(numbers[6]), bl.NewHandle(numbers[7]), bl.NewHandle(numbers[8]),
		bl.NewHandle(numbers[9]), bl.NewHandle(numbers[10]), bl.NewHandle(numbers[11]),
		bl.NewHandle(numbers[12]), bl.NewHandle(discount),
	})
	if e != nil {
		return "", "", e
	}
	return v.NetCapitalGain.Get().String(), v.Taxable.Get().String(), nil
}
```

The entries mirror the schedules: base tax, non-refundable offsets, levies,
HELP/HECS, then reconciliation. Final tax is rounded to whole dollars, matching
the annual return examples.

``` { .go .blkit-example title="main.go" }
var taxExpression = bl.NewDecisionExpression[taxVars, taxOutputs](bl.DecisionExpressionConfig{
	Id: "income-tax-calculation",
	Entries: bl.Entries{
		"base_tax":   `if residency="foreign" then (if income<=135000 then income*0.30 else if income<=190000 then 40500+(income-135000)*0.37 else 60850+(income-190000)*0.45) else if residency="working_holiday" then (if income<=45000 then income*0.15 else if income<=135000 then 6750+(income-45000)*0.30 else if income<=190000 then 33750+(income-135000)*0.37 else 54100+(income-190000)*0.45) else (if income<=18200 then 0 else if income<=45000 then (income-18200)*0.16 else if income<=135000 then 4288+(income-45000)*0.30 else if income<=190000 then 31288+(income-135000)*0.37 else 51638+(income-190000)*0.45)`,
		"lito":       `if residency!="resident" then 0 else if income<=37500 then 700 else if income<=45000 then 700-(income-37500)*0.05 else if income<=66667 then 325-(income-45000)*0.015 else 0`,
		"sapto":      `if residency="resident" and age>=67 then max([0,2230-0.125*max([0,income-32279])]) else 0`,
		"income_tax": `max([0,base_tax-lito-sapto])`,
		"medicare":   `if residency!="resident" then 0 else if age>=67 then (if income<=43020 then 0 else if income<=53775 then (income-43020)*0.10 else income*0.02) else (if income<=27222 then 0 else if income<=34027 then (income-27222)*0.10 else income*0.02)`,
		"mls":        `if residency!="resident" or private_cover or family!="single" then 0 else if income<=97000 then 0 else if income<=113000 then income*0.01 else if income<=151000 then income*0.0125 else income*0.015`,
		"help_rate":  `if income<54435 then 0 else if income<=62850 then 1 else if income<=66620 then 2 else if income<=70618 then 2.5 else if income<=74855 then 3 else if income<=79346 then 3.5 else if income<=84107 then 4 else if income<=89154 then 4.5 else if income<=94503 then 5 else if income<=100174 then 5.5 else if income<=106185 then 6 else if income<=112556 then 6.5 else if income<=119309 then 7 else if income<=126467 then 7.5 else if income<=134056 then 8 else if income<=142100 then 8.5 else if income<=150626 then 9 else if income<=159663 then 9.5 else 10`,
		"help":       `if debt=0 then 0 else min([debt,income*help_rate/100])`,
		"total_tax":  `round(income_tax+medicare+mls+help,0)`,
		"balance":    `total_tax-payg-franking`,
		"outcome":    `if balance<0 then "refund" else if balance>0 then "payable" else "settled"`,
	},
})

var taxDecision = bl.NewDecisionTask[taxVars, taxOutputs](bl.DecisionTaskConfig{
	Id:   "income-tax",
	Name: "Income tax",
})

var _ = taxDecision.Graph(
	bl.Edge(taxDecision.In.Income, taxExpression.In.Income),
	bl.Edge(taxDecision.In.Residency, taxExpression.In.Residency),
	bl.Edge(taxDecision.In.Age, taxExpression.In.Age),
	bl.Edge(taxDecision.In.Private, taxExpression.In.Private),
	bl.Edge(taxDecision.In.Family, taxExpression.In.Family),
	bl.Edge(taxDecision.In.Debt, taxExpression.In.Debt),
	bl.Edge(taxDecision.In.PAYG, taxExpression.In.PAYG),
	bl.Edge(taxDecision.In.Franking, taxExpression.In.Franking),
	bl.Edge(taxExpression.Out.BaseTax, taxDecision.Out.BaseTax),
	bl.Edge(taxExpression.Out.LITO, taxDecision.Out.LITO),
	bl.Edge(taxExpression.Out.SAPTO, taxDecision.Out.SAPTO),
	bl.Edge(taxExpression.Out.IncomeTax, taxDecision.Out.IncomeTax),
	bl.Edge(taxExpression.Out.Medicare, taxDecision.Out.Medicare),
	bl.Edge(taxExpression.Out.MLS, taxDecision.Out.MLS),
	bl.Edge(taxExpression.Out.HELPRate, taxDecision.Out.HELPRate),
	bl.Edge(taxExpression.Out.HELP, taxDecision.Out.HELP),
	bl.Edge(taxExpression.Out.TotalTax, taxDecision.Out.TotalTax),
	bl.Edge(taxExpression.Out.Balance, taxDecision.Out.Balance),
	bl.Edge(taxExpression.Out.Outcome, taxDecision.Out.Outcome),
)
```

``` { .go .blkit-example title="main.go" }
func CalculateTax(in TaxInput) (TaxResult, error) {
	income, e := bl.Number(in.TaxableIncome)
	if e != nil {
		return TaxResult{}, e
	}
	residency, _ := bl.String(in.Residency)
	age, _ := bl.Number(in.Age)
	private, _ := bl.Boolean(in.PrivateCover)
	family, _ := bl.String(in.FamilyStatus)
	debt, e := bl.Number(in.HELPDebt)
	if e != nil {
		return TaxResult{}, e
	}
	payg, e := bl.Number(in.PAYGWithheld)
	if e != nil {
		return TaxResult{}, e
	}
	franking, e := bl.Number(in.FrankingCredits)
	if e != nil {
		return TaxResult{}, e
	}
	value, e := taxDecision.Evaluate(taxVars{
		bl.NewHandle(income), bl.NewHandle(residency), bl.NewHandle(age),
		bl.NewHandle(private), bl.NewHandle(family), bl.NewHandle(debt),
		bl.NewHandle(payg), bl.NewHandle(franking),
	})
	if e != nil {
		return TaxResult{}, e
	}
	return TaxResult{
		value.BaseTax.Get().String(), value.LITO.Get().String(),
		value.SAPTO.Get().String(), value.Medicare.Get().String(),
		value.MLS.Get().String(), value.HELP.Get().String(),
		value.TotalTax.Get().String(), value.Balance.Get().String(),
		value.Outcome.Get().String(),
	}, nil
}
func main() {
	var in TaxInput
	if e := json.NewDecoder(os.Stdin).Decode(&in); e != nil {
		fmt.Fprintln(os.Stderr, e)
		os.Exit(1)
	}
	out, e := CalculateTax(in)
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

- Named decision outputs expose each stage of the calculation, so later entries
  can depend on base tax, offsets, levy rates, and reconciliation results.
- Residency status switches the entire downstream calculation — bracket schedule,
  Medicare eligibility, and offset entitlement — so it is best modelled as a
  branch over three parallel rate schedules rather than a single table.
- All monetary arithmetic should use `bl.Number()` decimals; the tapers and
  gross-ups (e.g. franking credit `× 30 / 70`) accumulate rounding error quickly
  under native floating point.
- The franking credit is **refundable** while LITO/SAPTO/FITO are
  **non-refundable** — a distinction the reconciliation step must preserve, since
  only refundable offsets can push the result below zero into a refund.
