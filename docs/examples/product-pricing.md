# Example: SaaS Subscription Pricing Engine

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

!!! warning "Implementation pending"
    The discount lookups can be modelled as decision tables and the four-step
    price reduction as a composed expression. The Go implementation depends on
    the `decisions` package, which is still being built. This page documents the
    pricing model; the runnable blkit code will be added once that package lands.

    In the meantime, see the authoritative
    [business spec](https://github.com/friendly-business-machines/blkit/blob/main/specs/examples/product-pricing.spec.md),
    [Getting started](../getting-started/index.md) for orientation, and the
    [Reference](../reference/blkit.md) for the expression engine available today.

## Notes

- The discounts **compound** (multiply) rather than add: a 10% volume discount
  followed by a 10% tier discount is 0.90 × 0.90 = 0.81, an 19% net reduction —
  not 20%.
- The promo code is a fixed cash reduction applied **before** the percentage
  discounts, so its effective value depends on where it sits in the chain.
