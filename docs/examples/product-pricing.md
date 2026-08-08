# SaaS Subscription Pricing Engine

> A SaaS company calculates the final per-seat price and total monthly charge for
> a subscription from plan, billing cycle, seat count, loyalty tier, and an
> optional promo code — four compounding discount factors.

## Business overview

A SaaS company's commercial team needs to calculate the price a customer will
pay for a software subscription. The final price applies four compounding
factors to a plan's base per-seat price: a promotional code reduction, a volume
discount based on seat count, and a loyalty tier discount.

### Subscription plans

Each plan has a monthly and an annual billing option; annual billing applies a
fixed per-seat price lower than the equivalent monthly rate.

| Plan | Monthly per seat | Annual per seat |
|---|---|---|
| Starter | 10.00 | 8.00 |
| Pro | 29.00 | 24.00 |
| Enterprise | 99.00 | 83.00 |

### Volume discount

A percentage discount applied to the per-seat price based on total licensed
seats:

| Seat count | Discount |
|---|---|
| 1–4 | None |
| 5–24 | 5% |
| 25–99 | 10% |
| 100–499 | 15% |
| 500 or more | 20% |

### Customer tier discount

An additional percentage discount applied on top of the volume discount:

| Tier | Discount |
|---|---|
| Standard | None |
| Silver | 5% |
| Gold | 10% |
| Platinum | 20% |

### Promotional codes

An optional code reduces the per-seat price by a fixed amount **before** the
percentage discounts are calculated:

| Code | Per-seat reduction |
|---|---|
| SAVE10 | 1.00 |
| LAUNCH20 | 2.00 |
| (none) | 0.00 |

### Price calculation

The effective per-seat price is:

1. Start with the plan's base per-seat price for the chosen billing cycle.
2. Subtract the promotional code reduction (if any).
3. Apply the volume discount to the result.
4. Apply the customer tier discount to the result.
5. Round the effective per-seat price half-up to two decimal places before
   calculating totals.

> total monthly = effective per-seat price × number of seats
>
> total annual = total monthly × 12

### Worked examples

| Plan | Billing Cycle | Seats | Tier | Promo Code | Base/Seat | Effective/Seat | Total Monthly |
|---|---|---|---|---|---|---|---|
| Starter | Monthly | 3 | Standard | — | 10.00 | 10.00 | 30.00 |
| Pro | Annual | 30 | Gold | SAVE10 | 24.00 | 18.63 | 558.90 |
| Enterprise | Annual | 150 | Platinum | — | 83.00 | 56.44 | 8,466.00 |
| Pro | Monthly | 5 | Silver | LAUNCH20 | 29.00 | 24.37 | 121.85 |

Walking row 2 through the steps: base per seat 24.00 (Pro, annual) → after promo
(SAVE10 −1.00) = 23.00 → after volume discount (30 seats = 10%) =
23.00 × 0.90 = 20.70 → after tier discount (Gold = 10%) = 20.70 × 0.90 = 18.63.
Total monthly = 18.63 × 30 = **558.90**.

## Implementation

Define a caller-facing request and the small contracts shared by the lookup
tables.

``` { .go .blkit-example title="main.go" }
package main

import (
	"encoding/json"
	"fmt"
	bl "github.com/friendly-business-machines/blkit/core"
	"os"
)

type PricingInput struct {
	Plan         string `json:"plan"`
	BillingCycle string `json:"billing_cycle"`
	Seats        int    `json:"seats"`
	Tier         string `json:"tier"`
	PromoCode    string `json:"promo_code"`
}
type PricingResult struct {
	BasePerSeat      string `json:"base_per_seat"`
	EffectivePerSeat string `json:"effective_per_seat"`
	TotalMonthly     string `json:"total_monthly"`
	TotalAnnual      string `json:"total_annual"`
}
type twoStrings struct {
	First  bl.Handle[bl.BlString] `expr:"first"`
	Second bl.Handle[bl.BlString] `expr:"second"`
}
type oneString struct {
	Value bl.Handle[bl.BlString] `expr:"value"`
}
type oneNumber struct {
	Value bl.Handle[bl.BlNumber] `expr:"value"`
}
type lookupNumber struct {
	Number bl.Handle[bl.BlNumber] `expr:"number"`
}
```

Four first-match tables keep independent commercial schedules independent.

``` { .go .blkit-example title="main.go" }
var basePrices = bl.NewDecisionTable[twoStrings, lookupNumber](bl.DecisionTableConfig{
	Id: "base-prices", HitPolicy: bl.HitPolicyFirst,
	Columns: []bl.Column{
		{Label: "Plan", Expr: `first`, Type: bl.TypeString},
		{Label: "Cycle", Expr: `second`, Type: bl.TypeString},
	},
	Rules: bl.Rules{
		{`starter-monthly`,    `"Starter"`,    `"Monthly"`, `10`},
		{`starter-annual`,     `"Starter"`,    `"Annual"`,  `8`},
		{`pro-monthly`,        `"Pro"`,        `"Monthly"`, `29`},
		{`pro-annual`,         `"Pro"`,        `"Annual"`,  `24`},
		{`enterprise-monthly`, `"Enterprise"`, `"Monthly"`, `99`},
		{`enterprise-annual`,  `"Enterprise"`, `"Annual"`,  `83`},
	},
})
var volumeDiscounts = bl.NewDecisionTable[oneNumber, lookupNumber](bl.DecisionTableConfig{
	Id: "volume-discounts", HitPolicy: bl.HitPolicyFirst,
	Columns: []bl.Column{{Label: "Seats", Expr: `value`, Type: bl.TypeNumber}},
	Rules: bl.Rules{
		{`small`,      `< 5`,        `0`},
		{`team`,       `[5..25)`,    `5`},
		{`business`,   `[25..100)`,  `10`},
		{`large`,      `[100..500)`, `15`},
		{`enterprise`, `>= 500`,     `20`},
	},
})
var tierDiscounts = bl.NewDecisionTable[oneString, lookupNumber](bl.DecisionTableConfig{
	Id: "tier-discounts", HitPolicy: bl.HitPolicyFirst,
	Columns: []bl.Column{{Label: "Tier", Expr: `value`, Type: bl.TypeString}},
	Rules: bl.Rules{
		{`standard`, `"Standard"`, `0`},
		{`silver`,   `"Silver"`,   `5`},
		{`gold`,     `"Gold"`,     `10`},
		{`platinum`, `"Platinum"`, `20`},
	},
})
var promoReductions = bl.NewDecisionTable[oneString, lookupNumber](bl.DecisionTableConfig{
	Id: "promo-reductions", HitPolicy: bl.HitPolicyFirst,
	Columns: []bl.Column{{Label: "Code", Expr: `value`, Type: bl.TypeString}},
	Rules: bl.Rules{
		{`save10`,   `"SAVE10"`,   `1`},
		{`launch20`, `"LAUNCH20"`, `2`},
		{`none`,     `-`,          `0`},
	},
})
```

A decision task wires the four lookup results into the pricing calculation. The
effective seat price is rounded half-up to cents before the seat total is
calculated, making invoice totals stable and matching the worked examples.

``` { .go .blkit-example title="main.go" }
type pricingVars struct {
	Base   bl.Handle[bl.BlNumber] `expr:"base"`
	Promo  bl.Handle[bl.BlNumber] `expr:"promo"`
	Volume bl.Handle[bl.BlNumber] `expr:"volume"`
	Tier   bl.Handle[bl.BlNumber] `expr:"tier"`
	Seats  bl.Handle[bl.BlNumber] `expr:"seats"`
}
type pricingOutputs struct {
	Effective bl.Handle[bl.BlNumber] `expr:"effective"`
	Monthly   bl.Handle[bl.BlNumber] `expr:"monthly"`
	Annual    bl.Handle[bl.BlNumber] `expr:"annual"`
}
type pricingInputs struct {
	Plan  bl.Handle[bl.BlString] `expr:"plan"`
	Cycle bl.Handle[bl.BlString] `expr:"cycle"`
	Seats bl.Handle[bl.BlNumber] `expr:"seats"`
	Tier  bl.Handle[bl.BlString] `expr:"tier"`
	Promo bl.Handle[bl.BlString] `expr:"promo"`
}
type pricingResultOutputs struct {
	Base      bl.Handle[bl.BlNumber] `expr:"base"`
	Effective bl.Handle[bl.BlNumber] `expr:"effective"`
	Monthly   bl.Handle[bl.BlNumber] `expr:"monthly"`
	Annual    bl.Handle[bl.BlNumber] `expr:"annual"`
}

var pricingCalculation = bl.NewDecisionExpression[pricingVars, pricingOutputs](bl.DecisionExpressionConfig{
	Id: "pricing-calculation",
	Entries: bl.Entries{
		"effective": `round((base-promo)*(1-volume/100)*(1-tier/100),2)`,
		"monthly":   `effective*seats`,
		"annual":    `monthly*12`,
	},
})

var subscriptionPricing = bl.NewDecisionTask[pricingInputs, pricingResultOutputs](bl.DecisionTaskConfig{
	Id:   "subscription-pricing",
	Name: "Subscription pricing",
})

var _ = subscriptionPricing.Graph(
	bl.Edge(subscriptionPricing.In.Plan, basePrices.In.First),
	bl.Edge(subscriptionPricing.In.Cycle, basePrices.In.Second),
	bl.Edge(subscriptionPricing.In.Seats, volumeDiscounts.In.Value),
	bl.Edge(subscriptionPricing.In.Tier, tierDiscounts.In.Value),
	bl.Edge(subscriptionPricing.In.Promo, promoReductions.In.Value),
	bl.Edge(basePrices.Out.Number, pricingCalculation.In.Base),
	bl.Edge(promoReductions.Out.Number, pricingCalculation.In.Promo),
	bl.Edge(volumeDiscounts.Out.Number, pricingCalculation.In.Volume),
	bl.Edge(tierDiscounts.Out.Number, pricingCalculation.In.Tier),
	bl.Edge(subscriptionPricing.In.Seats, pricingCalculation.In.Seats),
	bl.Edge(basePrices.Out.Number, subscriptionPricing.Out.Base),
	bl.Edge(pricingCalculation.Out.Effective, subscriptionPricing.Out.Effective),
	bl.Edge(pricingCalculation.Out.Monthly, subscriptionPricing.Out.Monthly),
	bl.Edge(pricingCalculation.Out.Annual, subscriptionPricing.Out.Annual),
)

func strHandle(s string) bl.Handle[bl.BlString] { v, _ := bl.String(s); return bl.NewHandle(v) }
func numHandle(n int) bl.Handle[bl.BlNumber]    { v, _ := bl.Number(n); return bl.NewHandle(v) }
func PriceSubscription(input PricingInput) (PricingResult, error) {
	values, err := subscriptionPricing.Evaluate(pricingInputs{
		Plan:  strHandle(input.Plan),
		Cycle: strHandle(input.BillingCycle),
		Seats: numHandle(input.Seats),
		Tier:  strHandle(input.Tier),
		Promo: strHandle(input.PromoCode),
	})
	if err != nil {
		return PricingResult{}, err
	}
	return PricingResult{
		values.Base.Get().String(),
		values.Effective.Get().String(),
		values.Monthly.Get().String(),
		values.Annual.Get().String(),
	}, nil
}
func main() {
	var input PricingInput
	if err := json.NewDecoder(os.Stdin).Decode(&input); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	result, err := PriceSubscription(input)
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

- The discounts **compound** (multiply) rather than add: a 10% volume discount
  followed by a 10% tier discount is 0.90 × 0.90 = 0.81, a 19% net reduction —
  not 20%.
- The promo code is a fixed cash reduction applied **before** the percentage
  discounts, so its effective value depends on where it sits in the chain.
