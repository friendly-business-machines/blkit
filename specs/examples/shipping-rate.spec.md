---
name: Example — Shipping Rate Calculator
description: An e-commerce platform calculates the total shipping charge for a parcel using billable weight (actual vs volumetric), destination zone base rates, a speed-tier multiplier, and a flat fuel surcharge
status: implemented
code:
  - docs/examples/shipping-rate.md
  - internal/doctest/testdata/shipping-rate/example_test.go
---

# Example: Shipping Rate Calculator

## Business Scenario

An e-commerce fulfilment platform needs to calculate the shipping cost for any outbound parcel. The cost depends on:

- **Billable weight** — carriers charge by the greater of actual weight and volumetric weight. Volumetric weight = (length × width × height) ÷ 5000 (dimensions in cm, result in kg).
- **Destination zone** — three zones with different base rates and per-kg rates.
- **Speed tier** — standard, express, or overnight, each applying a cost multiplier.
- **Fuel surcharge** — a flat 8% applied to the pre-surcharge total.

This example shows how to compose these calculations entirely using blkit expressions, without needing a `DecisionTable` or a `Process`. The expression system is a standalone feature of blkit — it can be used anywhere arithmetic, conditionals, or deferred expression evaluation is useful.

## What This Example Demonstrates

- Using `bl.number()` for precise decimal arithmetic
- Using `bl.var()` to reference runtime variables
- Arithmetic operations: `.add()`, `.sub()`, `.mul()`, `.div()`
- Comparison and conditional: `.gt()`, `.if_then_else()`
- Using `bl.context()` to build a structured output
- Calling `.evaluate(variables)` to materialise a result
- Composing a multi-step calculation from reusable expression fragments

## Data Model

### Inputs

| Variable | Bl type | Description |
|---|---|---|
| `actual_weight_kg` | `number` | Actual measured weight of the parcel, in kilograms |
| `length_cm` | `number` | Parcel length in centimetres |
| `width_cm` | `number` | Parcel width in centimetres |
| `height_cm` | `number` | Parcel height in centimetres |
| `destination_zone` | `number` | `1` (domestic), `2` (regional), or `3` (international) |
| `speed_tier` | `string` | `"standard"`, `"express"`, or `"overnight"` |

### Output

| Variable | Bl type | Description |
|---|---|---|
| `billable_weight_kg` | `number` | The weight the carrier charges for |
| `base_rate` | `number` | Zone-based fixed charge |
| `per_kg_rate` | `number` | Zone-based per-kilogram charge |
| `speed_multiplier` | `number` | Factor applied for the chosen speed tier |
| `subtotal` | `number` | Pre-surcharge cost |
| `fuel_surcharge` | `number` | 8% of subtotal |
| `total` | `number` | Final shipping cost |

## Rate Schedule

### Zone rates

| Zone | Name | Base rate | Per-kg rate |
|---|---|---|---|
| 1 | Domestic | 5.00 | 1.50 |
| 2 | Regional | 12.00 | 2.50 |
| 3 | International | 25.00 | 4.00 |

### Speed multipliers

| Tier | Multiplier |
|---|---|
| `"standard"` | 1.0 |
| `"express"` | 1.5 |
| `"overnight"` | 2.5 |

### Fuel surcharge

8% applied to `(base_rate + billable_weight_kg × per_kg_rate) × speed_multiplier`.

## Calculation Steps

1. **Volumetric weight**: `(length_cm × width_cm × height_cm) ÷ 5000`
2. **Billable weight**: `max(actual_weight_kg, volumetric_weight)`
3. **Zone rates lookup**: conditional expression selecting base and per-kg rates by zone
4. **Speed multiplier lookup**: conditional expression selecting multiplier by tier
5. **Subtotal**: `(base_rate + billable_weight_kg × per_kg_rate) × speed_multiplier`
6. **Fuel surcharge**: `subtotal × 0.08`
7. **Total**: `subtotal + fuel_surcharge`

## Implementation Outline

```python
# Step 1 — volumetric weight
volumetric_weight = (
    bl.var("length_cm")
    .mul(bl.var("width_cm"))
    .mul(bl.var("height_cm"))
    .div(bl.number(5000))
)

# Step 2 — billable weight (max of actual vs volumetric)
# bl.function_call("max", ...) invokes the built-in max() function
billable_weight = bl.function_call("max", [
    bl.var("actual_weight_kg"),
    volumetric_weight,
])

# Step 3 — zone-based rates returned as a Bl context
zone_rates = (
    bl.var("destination_zone").eq(bl.number(1))
    .if_then_else(
        bl.context({"base": bl.number(5.00), "per_kg": bl.number(1.50)}),
        bl.var("destination_zone").eq(bl.number(2))
        .if_then_else(
            bl.context({"base": bl.number(12.00), "per_kg": bl.number(2.50)}),
            bl.context({"base": bl.number(25.00), "per_kg": bl.number(4.00)}),
        ),
    )
)

# Step 4 — speed multiplier
speed_multiplier = (
    bl.var("speed_tier").eq(bl.string("standard"))
    .if_then_else(
        bl.number(1.0),
        bl.var("speed_tier").eq(bl.string("express"))
        .if_then_else(
            bl.number(1.5),
            bl.number(2.5),
        ),
    )
)

# Steps 5–7 — build the full output context
# (zone_rates.base and zone_rates.per_kg accessed via path expressions)
subtotal = (
    zone_rates.path("base")
    .add(billable_weight.mul(zone_rates.path("per_kg")))
    .mul(speed_multiplier)
)
fuel_surcharge = subtotal.mul(bl.number(0.08))
total = subtotal.add(fuel_surcharge)

# Final result as a structured context
result_expr = bl.context({
    "billable_weight_kg": billable_weight,
    "subtotal":           subtotal,
    "fuel_surcharge":     fuel_surcharge,
    "total":              total,
})

# Evaluate
result = await result_expr.evaluate({
    "actual_weight_kg": 3.2,
    "length_cm": 40,
    "width_cm": 30,
    "height_cm": 20,
    "destination_zone": 2,
    "speed_tier": "express",
})
# volumetric_weight = (40×30×20)/5000 = 4.8 kg → billable = 4.8
# base_rate = 12.00, per_kg = 2.50
# subtotal = (12.00 + 4.8×2.50) × 1.5 = (12.00 + 12.00) × 1.5 = 36.00
# fuel_surcharge = 36.00 × 0.08 = 2.88
# total = 38.88
```

## Sample Inputs and Expected Outputs

| actual_weight | L×W×H (cm) | Volumetric | Billable | Zone | Speed | Total |
|---|---|---|---|---|---|---|
| 3.2 kg | 40×30×20 | 4.8 kg | 4.8 kg | 2 (regional) | express | 38.88 |
| 10.0 kg | 20×15×10 | 0.6 kg | 10.0 kg | 1 (domestic) | standard | 21.60 |
| 0.5 kg | 60×40×30 | 14.4 kg | 14.4 kg | 3 (international) | overnight | 223.02 |
| 2.0 kg | 25×15×10 | 0.75 kg | 2.0 kg | 1 (domestic) | overnight | 21.60 |

## Documentation Page Requirements

The documentation page for this example (`docs/examples/shipping-rate.md`) must contain:

1. A short narrative introduction explaining the billing model and what the reader will build.
2. The complete, runnable Go code.
3. An explicit walkthrough: show the intermediate values for the first sample row (actual=3.2 kg, regional, express, total=38.88) with each intermediate computation labelled.
4. A callout explaining the difference between `bl.number()` values (arbitrary precision) and native floating-point — and why `Bl` arithmetic is preferable for financial calculations.
5. A short note on reusability: because the expressions are pure data structures, `result_expr` can be built once and evaluated many times with different variable maps.
