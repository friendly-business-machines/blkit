---
name: Example — SaaS Subscription Pricing Engine
description: A SaaS company calculates the final per-seat price and total monthly charge for a subscription based on plan, billing cycle, seat count (volume discount), customer loyalty tier, and an optional promotional code — with four compounding discount factors
status: implemented
code:
  - docs/examples/product-pricing.md
---

# Example: SaaS Subscription Pricing Engine

## Overview

A SaaS company's commercial team needs to calculate the price a customer will pay for a software subscription. The final price is the result of four compounding factors applied to a plan's base per-seat price: a volume discount based on seat count, a loyalty tier discount, and an optional promotional code reduction.

## Subscription Plans

Three plans are available, each with a monthly and an annual billing option. Annual billing applies a fixed per-seat price lower than the equivalent monthly rate.

| Plan | Monthly per seat | Annual per seat |
|---|---|---|
| Starter | 10.00 | 8.00 |
| Pro | 29.00 | 24.00 |
| Enterprise | 99.00 | 83.00 |

## Volume Discount

A percentage discount is applied to the per-seat price based on the total number of licensed seats on the subscription:

| Seat count | Discount |
|---|---|
| 1–4 | None |
| 5–24 | 5% |
| 25–99 | 10% |
| 100–499 | 15% |
| 500 or more | 20% |

## Customer Tier Discount

Customers are assigned a loyalty tier. An additional percentage discount is applied on top of the volume discount:

| Tier | Discount |
|---|---|
| Standard | None |
| Silver | 5% |
| Gold | 10% |
| Platinum | 20% |

## Promotional Codes

An optional promotional code may be applied to a subscription. Active promotional codes reduce the per-seat price by a fixed amount before other discounts are calculated:

| Code | Per-seat reduction |
|---|---|
| SAVE10 | 1.00 |
| LAUNCH20 | 2.00 |
| (none) | 0.00 |

## Price Calculation

The effective per-seat price is calculated as:

1. Start with the plan's base per-seat price for the chosen billing cycle.
2. Subtract the promotional code reduction (if any).
3. Apply the volume discount to the result.
4. Apply the customer tier discount to the result.

The total monthly charge is:

> total monthly = effective per-seat price × number of seats

The total annual charge is:

> total annual = total monthly × 12

## Examples

| Plan | Billing Cycle | Seats | Tier | Promo Code | Base/Seat | Effective/Seat | Total Monthly |
|---|---|---|---|---|---|---|---|
| Starter | Monthly | 3 | Standard | — | 10.00 | 10.00 | 30.00 |
| Pro | Annual | 30 | Gold | SAVE10 | 24.00 | 18.63 | 558.90 |
| Enterprise | Annual | 150 | Platinum | — | 83.00 | 56.44 | 8,466.00 |
| Pro | Monthly | 5 | Silver | LAUNCH20 | 29.00 | 24.37 | 121.85 |

### Worked example (row 2)

- Base per seat: 24.00 (Pro, annual)
- After promo (SAVE10 −1.00): 23.00
- After volume discount (30 seats = 10%): 23.00 × 0.90 = 20.70
- After tier discount (Gold = 10%): 20.70 × 0.90 = 18.63
- Total monthly: 18.63 × 30 = **558.90**
