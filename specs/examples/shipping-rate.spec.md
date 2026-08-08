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

This example composes the calculation as a [`DecisionTask`](../decision-tasks/decision-task.spec.md) containing one `DecisionExpression`, without needing a `DecisionTable` or a `Process`. Named expression entries represent each intermediate result and blkit evaluates their dependencies in order.

## What This Example Demonstrates

- Defining typed input and output contracts with `bl.Handle`
- Compiling related calculations with `bl.NewDecisionExpression`
- Wiring the expression into a complete `bl.DecisionTask`
- Referencing sibling outputs from dependent expression entries
- Using `bl.Number()` for precise decimal arithmetic
- Evaluating one reusable decision task with different input values

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

## Decision Definition

The implementation defines one `DecisionTask` whose graph contains a `DecisionExpression` with entries corresponding to the calculation steps above. Inputs and outputs use typed `bl.Handle` fields. Intermediate outputs such as `base_rate`, `per_kg_rate`, and `subtotal` remain part of the task contract so later entries can depend on them. The application boundary converts JSON decimal strings with `bl.Number()` before evaluating the task.

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
4. A callout explaining the difference between `bl.Number()` values (arbitrary precision) and native floating-point — and why `Bl` arithmetic is preferable for financial calculations.
5. A short note on reusability: the decision task is built once and evaluated many times with different typed inputs.
