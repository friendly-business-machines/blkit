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
| 10.0 kg | 20×15×10 | 0.6 kg | 10.0 kg | 1 (domestic) | standard | 18.90 |
| 0.5 kg | 60×40×30 | 14.4 kg | 14.4 kg | 3 (international) | overnight | 214.20 |
| 2.0 kg | 25×15×10 | 0.75 kg | 2.0 kg | 1 (domestic) | overnight | 20.25 |

Walking the first row through the steps: volumetric weight =
(40 × 30 × 20) ÷ 5000 = 4.8 kg, so billable = 4.8 kg. Regional zone gives
base = 12.00 and per-kg = 2.50. Subtotal = (12.00 + 4.8 × 2.50) × 1.5 =
(12.00 + 12.00) × 1.5 = 36.00. Fuel surcharge = 36.00 × 0.08 = 2.88.
Total = **38.88**.

## Implementation

Because this example uses only the expression engine (the root `blkit` package),
it can be built today. The spec captures the calculation as composable
expression fragments — the outline below mirrors that structure. The full,
CI-tested Go listing will be published here once the surrounding example
scaffolding lands.

!!! note "Expression engine available today"
    The building blocks below (`bl.var`, `bl.number`, arithmetic, `if_then_else`,
    `bl.context`, and `.evaluate`) ship in the current `blkit` package. See the
    [Reference](../reference/blkit.md) for the full expression API and the
    authoritative
    [business spec](https://github.com/friendly-business-machines/blkit/blob/main/specs/examples/shipping-rate.spec.md)
    for the complete worked outline.

```text
volumetric_weight = length_cm × width_cm × height_cm ÷ 5000
billable_weight   = max(actual_weight_kg, volumetric_weight)
zone_rates        = if zone == 1 → {base: 5.00,  per_kg: 1.50}
                    if zone == 2 → {base: 12.00, per_kg: 2.50}
                    else         → {base: 25.00, per_kg: 4.00}
speed_multiplier  = if tier == "standard" → 1.0
                    if tier == "express"  → 1.5
                    else                  → 2.5
subtotal          = (zone_rates.base + billable_weight × zone_rates.per_kg) × speed_multiplier
fuel_surcharge    = subtotal × 0.08
total             = subtotal + fuel_surcharge
```

## Notes

- Expressions are pure data structures: the result expression can be built once
  and evaluated many times with different variable maps.
- Prefer `bl.number()` (arbitrary-precision decimal) over native floating point
  for monetary arithmetic — it avoids the rounding drift that float math
  introduces in financial calculations.
