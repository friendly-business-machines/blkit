# Shipping Rate Calculator

> An e-commerce platform calculates the total shipping charge for a parcel from
> billable weight, destination zone, speed tier, and a fuel surcharge —
> composed entirely from blkit expressions.

## Business overview

An e-commerce fulfilment platform needs to calculate the shipping cost for any
outbound parcel. The cost depends on:

- **Billable weight** — carriers charge by the greater of actual weight and
  volumetric weight. Volumetric weight = (length × width × height) ÷ 5000
  (dimensions in cm, result in kg).
- **Destination zone** — three zones with different base and per-kg rates.
- **Speed tier** — standard, express, or overnight, each applying a multiplier.
- **Fuel surcharge** — a flat 8% applied to the pre-surcharge total.

Unlike most examples here, this one needs no `DecisionTable` or `Process` — it
is composed purely from blkit expressions, which are available today.

### Inputs

| Variable | Bl type | Description |
|---|---|---|
| `actual_weight_kg` | `number` | Actual measured weight of the parcel, in kilograms |
| `length_cm` | `number` | Parcel length in centimetres |
| `width_cm` | `number` | Parcel width in centimetres |
| `height_cm` | `number` | Parcel height in centimetres |
| `destination_zone` | `number` | `1` (domestic), `2` (regional), or `3` (international) |
| `speed_tier` | `string` | `"standard"`, `"express"`, or `"overnight"` |

### Rate schedule

| Zone | Name | Base rate | Per-kg rate |
|---|---|---|---|
| 1 | Domestic | 5.00 | 1.50 |
| 2 | Regional | 12.00 | 2.50 |
| 3 | International | 25.00 | 4.00 |

| Speed tier | Multiplier |
|---|---|
| `"standard"` | 1.0 |
| `"express"` | 1.5 |
| `"overnight"` | 2.5 |

The fuel surcharge is 8% of
`(base_rate + billable_weight_kg × per_kg_rate) × speed_multiplier`.

### Calculation steps

1. **Volumetric weight**: `(length_cm × width_cm × height_cm) ÷ 5000`
2. **Billable weight**: `max(actual_weight_kg, volumetric_weight)`
3. **Zone rates lookup**: conditional expression selecting base and per-kg rates by zone
4. **Speed multiplier lookup**: conditional expression selecting multiplier by tier
5. **Subtotal**: `(base_rate + billable_weight_kg × per_kg_rate) × speed_multiplier`
6. **Fuel surcharge**: `subtotal × 0.08`
7. **Total**: `subtotal + fuel_surcharge`

### Worked examples

| actual_weight | L×W×H (cm) | Volumetric | Billable | Zone | Speed | Total |
|---|---|---|---|---|---|---|
| 3.2 kg | 40×30×20 | 4.8 kg | 4.8 kg | 2 (regional) | express | 38.88 |
| 10.0 kg | 20×15×10 | 0.6 kg | 10.0 kg | 1 (domestic) | standard | 21.60 |
| 0.5 kg | 60×40×30 | 14.4 kg | 14.4 kg | 3 (international) | overnight | 223.02 |
| 2.0 kg | 25×15×10 | 0.75 kg | 2.0 kg | 1 (domestic) | overnight | 21.60 |

Walking the first row through the steps: volumetric weight =
(40 × 30 × 20) ÷ 5000 = 4.8 kg, so billable = 4.8 kg. Regional zone gives
base = 12.00 and per-kg = 2.50. Subtotal = (12.00 + 4.8 × 2.50) × 1.5 =
(12.00 + 12.00) × 1.5 = 36.00. Fuel surcharge = 36.00 × 0.08 = 2.88.
Total = **38.88**.

## Implementation

Start with request and response types. Decimal inputs are strings so conversion
to `bl.BlNumber` never passes through binary floating point.

``` { .go .blkit-example title="main.go" }
package main

import (
	"encoding/json"
	"fmt"
	"os"

	bl "github.com/friendly-business-machines/blkit/core"
)

type ShippingInput struct {
	ActualWeightKG  string `json:"actual_weight_kg"`
	LengthCM        string `json:"length_cm"`
	WidthCM         string `json:"width_cm"`
	HeightCM        string `json:"height_cm"`
	DestinationZone int    `json:"destination_zone"`
	SpeedTier       string `json:"speed_tier"`
}

type ShippingQuote struct {
	VolumetricWeightKG string `json:"volumetric_weight_kg"`
	BillableWeightKG   string `json:"billable_weight_kg"`
	Subtotal           string `json:"subtotal"`
	FuelSurcharge      string `json:"fuel_surcharge"`
	Total              string `json:"total"`
}
```

The expression is compiled once. Dictionary entries are evaluated in order, so
later entries can reuse earlier intermediate values.

``` { .go .blkit-example title="main.go" }
type shippingEnv struct {
	ActualWeightKG  bl.BlNumber `expr:"actual_weight_kg"`
	LengthCM        bl.BlNumber `expr:"length_cm"`
	WidthCM         bl.BlNumber `expr:"width_cm"`
	HeightCM        bl.BlNumber `expr:"height_cm"`
	DestinationZone bl.BlNumber `expr:"destination_zone"`
	SpeedTier       bl.BlString `expr:"speed_tier"`
}

var shippingExpression = mustExpression(bl.Expr[shippingEnv](`{
    volumetric_weight: length_cm * width_cm * height_cm / 5000,
    billable_weight: max([actual_weight_kg, volumetric_weight]),
    base_rate: (if destination_zone = 1 then 5 else if destination_zone = 2 then 12 else 25),
    per_kg_rate: (if destination_zone = 1 then 1.5 else if destination_zone = 2 then 2.5 else 4),
    speed_multiplier: (if speed_tier = "standard" then 1 else if speed_tier = "express" then 1.5 else 2.5),
    subtotal: (base_rate + billable_weight * per_kg_rate) * speed_multiplier,
    fuel_surcharge: subtotal * 0.08,
    total: subtotal + fuel_surcharge
}`))

func mustExpression(expr *bl.BlExpr[shippingEnv], err error) *bl.BlExpr[shippingEnv] {
	if err != nil {
		panic(err)
	}
	return expr
}

func number(value string) (bl.BlNumber, error) { return bl.Number(value) }
```

`CalculateShipping` converts the application boundary, evaluates the reusable
expression, and projects its dictionary result.

``` { .go .blkit-example title="main.go" }
func CalculateShipping(input ShippingInput) (ShippingQuote, error) {
	actual, err := number(input.ActualWeightKG)
	if err != nil {
		return ShippingQuote{}, err
	}
	length, err := number(input.LengthCM)
	if err != nil {
		return ShippingQuote{}, err
	}
	width, err := number(input.WidthCM)
	if err != nil {
		return ShippingQuote{}, err
	}
	height, err := number(input.HeightCM)
	if err != nil {
		return ShippingQuote{}, err
	}
	zone, err := bl.Number(input.DestinationZone)
	if err != nil {
		return ShippingQuote{}, err
	}
	tier, err := bl.String(input.SpeedTier)
	if err != nil {
		return ShippingQuote{}, err
	}
	value, err := shippingExpression.Evaluate(shippingEnv{actual, length, width, height, zone, tier})
	if err != nil {
		return ShippingQuote{}, err
	}
	values := value.(bl.BlDictionary).Native()
	get := func(name string) string { return values[name].(bl.BlNumber).String() }
	return ShippingQuote{get("volumetric_weight"), get("billable_weight"), get("subtotal"), get("fuel_surcharge"), get("total")}, nil
}
```

Finally, the command accepts a JSON request from its caller and returns the quote.

``` { .go .blkit-example title="main.go" }
func main() {
	var input ShippingInput
	if err := json.NewDecoder(os.Stdin).Decode(&input); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	quote, err := CalculateShipping(input)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err := json.NewEncoder(os.Stdout).Encode(quote); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
```

## Notes

- Expressions are pure data structures: the result expression can be built once
  and evaluated many times with different variable maps.
- Prefer `bl.Number()` (arbitrary-precision decimal) over native floating point
  for monetary arithmetic — it avoids the rounding drift that float math
  introduces in financial calculations.
