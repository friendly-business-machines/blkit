# Customer Discount Eligibility

> A pricing engine evaluates an order against many discount rules, collects every
> rule that matches, and applies the single highest percentage — discounts do
> not stack.

## Business overview

When a customer places an order, the pricing engine evaluates it against a set of
discount rules. Multiple discounts may apply simultaneously — for example, both a
loyalty discount and a bulk-purchase discount. All applicable discounts are
collected and the **single highest percentage** is applied to the order subtotal.
If no discount applies, the order is charged at full price. Only one discount is
ever applied to an order; discounts do not stack.

### Order information

| Field | Description |
|---|---|
| Customer ID | Unique identifier of the placing customer |
| Customer Tier | Bronze, Silver, Gold, or Platinum |
| Account Age (months) | Months since the customer's account was created |
| Order Subtotal | Pre-discount total value of the order in GBP |
| Item Count | Number of individual items in the order |
| Product Category | The primary category of items (e.g. Electronics, Stationery, Furniture) |
| Promotional Code | Optional code entered at checkout |
| Order Month | Calendar month of the order (used for seasonal rules) |

### Discount rules

Every rule is evaluated against every order; each satisfied rule produces a
discount entry. The applied discount is the highest percentage across all
matching entries. If no rule matches, the discount is 0%.

| Rule | Condition | Discount |
|---|---|---|
| R1 – Loyalty (Silver) | Account age ≥ 12 months and tier is Silver | 5% |
| R2 – Loyalty (Gold) | Account age ≥ 12 months and tier is Gold | 10% |
| R3 – Loyalty (Platinum) | Account age ≥ 12 months and tier is Platinum | 15% |
| R4 – New customer | Account age < 3 months | 10% |
| R5 – Bulk purchase (small) | Item count ≥ 10 and < 25 | 5% |
| R6 – Bulk purchase (large) | Item count ≥ 25 | 12% |
| R7 – High value order | Order subtotal > £500 | 8% |
| R8 – Furniture category | Product category is Furniture | 6% |
| R9 – End-of-season | Order month is January or July | 20% |
| R10 – Promo code: WELCOME20 | Promotional code equals WELCOME20 | 20% |
| R11 – Promo code: LOYAL10 | Code equals LOYAL10 and tier is Gold or Platinum | 10% |
| R12 – Promo code: BULK15 | Code equals BULK15 and item count ≥ 10 | 15% |

### Selecting the applied discount

1. Evaluate all 12 rules against the order.
2. Collect the discount percentage from every rule that matches.
3. If the collection is empty, the applied discount is 0%.
4. Otherwise, the applied discount is the highest percentage in the collection.

### Discount application

> Order Total = Order Subtotal × (1 − Applied Discount %)
>
> Discount Amount = Order Subtotal × Applied Discount %

### Outcomes

| Applied Discount | Outcome |
|---|---|
| 0% | No discount; customer charged full subtotal |
| 1% – 19% | Discount applied; order total reduced accordingly |
| 20% | Maximum discount applied (end-of-season or WELCOME20 code) |

### Worked examples

| Customer | Tier | Account Age | Subtotal | Items | Category | Code | Month | Matching Rules | Highest Discount | Order Total |
|---|---|---|---|---|---|---|---|---|---|---|
| C-001 | Bronze | 8 months | £120 | 5 | Stationery | — | March | None | 0% | £120.00 |
| C-002 | Gold | 18 months | £350 | 15 | Electronics | — | June | R2, R5 | 10% | £315.00 |
| C-003 | Silver | 24 months | £600 | 30 | Furniture | — | October | R1, R6, R7, R8 | 12% | £528.00 |
| C-004 | Bronze | 1 month | £80 | 3 | Stationery | WELCOME20 | May | R4, R10 | 20% | £64.00 |
| C-005 | Platinum | 36 months | £900 | 28 | Electronics | BULK15 | January | R3, R6, R7, R9, R12 | 20% | £720.00 |

For C-003, four rules match — R1 (5%), R6 (12%), R7 (8%), R8 (6%). The highest is
12% (R6, bulk large), so the order total is £600 × (1 − 0.12) = **£528.00** with a
£72.00 discount shown.

## Implementation

!!! warning "Implementation pending"
    This is a decision table evaluated with a **collect (aggregate) hit policy** —
    gather every matching rule's percentage, then take the maximum. The Go
    implementation depends on the `decisions` package, which is still being
    built. This page documents the rules; the runnable blkit code will be added
    once that package lands.

    In the meantime, see the authoritative
    [business spec](https://github.com/friendly-business-machines/blkit/blob/main/specs/examples/customer-discount.spec.md),
    [Getting started](../getting-started/index.md) for orientation, and the
    [Reference](../reference/blkit.md) for the expression engine available today.

## Notes

- This is the **collect-max** counterpart to first-match tables like
  [loan-eligibility](loan-eligibility.md): instead of stopping at the first hit,
  it evaluates all rules and reduces the matches with a `max` aggregation.
- The bands are deliberately non-overlapping at the edges — R5 is item count ≥ 10
  **and < 25**, so a 30-item order matches R6 (large) but not R5 (small).
