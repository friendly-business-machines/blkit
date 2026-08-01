---
name: customer-discount
description: Business process specification for identifying all applicable discounts for a customer order and selecting the most beneficial one.
status: implemented
code:
  - docs/examples/customer-discount.md
---

# Customer Discount Eligibility

## Overview

When a customer places an order, the pricing engine evaluates the order against a set of discount rules. Multiple discounts may apply simultaneously — for example, a customer could qualify for both a loyalty discount and a bulk purchase discount. All applicable discounts are collected and the single highest percentage is applied to the order subtotal.

If no discount applies, the order is charged at full price.

Only one discount is applied to any given order. Discounts do not stack.

---

## Order Information

| Field | Description |
|---|---|
| Customer ID | Unique identifier of the placing customer |
| Customer Tier | Bronze, Silver, Gold, or Platinum |
| Account Age (months) | Number of months since the customer's account was created |
| Order Subtotal | Pre-discount total value of the order in GBP |
| Item Count | Number of individual items in the order |
| Product Category | The primary category of items in the order (e.g. Electronics, Stationery, Furniture) |
| Promotional Code | Optional code entered by the customer at checkout |
| Order Month | Calendar month of the order (used for seasonal rules) |

---

## Discount Rules

All rules are evaluated against every order. Every rule that is satisfied produces a discount entry. The final applied discount is the highest percentage found across all matching entries. If no rule matches, the discount is 0%.

| Rule | Condition | Discount |
|---|---|---|
| R1 – Loyalty (Silver) | Account age ≥ 12 months and customer tier is Silver | 5% |
| R2 – Loyalty (Gold) | Account age ≥ 12 months and customer tier is Gold | 10% |
| R3 – Loyalty (Platinum) | Account age ≥ 12 months and customer tier is Platinum | 15% |
| R4 – New customer | Account age < 3 months | 10% |
| R5 – Bulk purchase (small) | Item count ≥ 10 and item count < 25 | 5% |
| R6 – Bulk purchase (large) | Item count ≥ 25 | 12% |
| R7 – High value order | Order subtotal > £500 | 8% |
| R8 – Furniture category | Product category is Furniture | 6% |
| R9 – End-of-season | Order month is January or July | 20% |
| R10 – Promotional code: WELCOME20 | Promotional code equals WELCOME20 | 20% |
| R11 – Promotional code: LOYAL10 | Promotional code equals LOYAL10 and customer tier is Gold or Platinum | 10% |
| R12 – Promotional code: BULK15 | Promotional code equals BULK15 and item count ≥ 10 | 15% |

---

## Selecting the Applied Discount

1. Evaluate all 12 rules against the order.
2. Collect the discount percentage from every rule that matches.
3. If the collection is empty, the applied discount is 0%.
4. Otherwise, the applied discount is the highest percentage in the collection.

---

## Discount Application

The final order total is calculated as:

> Order Total = Order Subtotal × (1 − Applied Discount %)

The discount amount shown on the invoice is:

> Discount Amount = Order Subtotal × Applied Discount %

---

## Outcomes

| Applied Discount | Outcome |
|---|---|
| 0% | No discount; customer charged full subtotal |
| 1% – 19% | Discount applied; order total reduced accordingly |
| 20% | Maximum discount applied (end-of-season or WELCOME20 code) |

---

## Examples

| Customer | Tier | Account Age | Subtotal | Items | Category | Code | Month | Matching Rules | Highest Discount | Order Total |
|---|---|---|---|---|---|---|---|---|---|---|
| C-001 | Bronze | 8 months | £120 | 5 | Stationery | — | March | None | 0% | £120.00 |
| C-002 | Gold | 18 months | £350 | 15 | Electronics | — | June | R2, R5 | 10% | £315.00 |
| C-003 | Silver | 24 months | £600 | 30 | Furniture | — | October | R1, R6, R7, R8 | 12% | £528.00 |
| C-004 | Bronze | 1 month | £80 | 3 | Stationery | WELCOME20 | May | R4, R10 | 20% | £64.00 |
| C-005 | Platinum | 36 months | £900 | 28 | Electronics | BULK15 | January | R3, R6, R7, R9, R12 | 20% | £720.00 |

### Worked Example: C-003

**Order**: Silver customer, account age 24 months, subtotal £600, 30 items, Furniture category, no code, October.

**Rule evaluation**:

- R1 (Loyalty Silver): account age 24 ≥ 12 and tier is Silver → **5% matched**
- R2 (Loyalty Gold): tier is not Gold → not matched
- R5 (Bulk small): item count 30 ≥ 10 but 30 is not < 25 → not matched
- R6 (Bulk large): item count 30 ≥ 25 → **12% matched**
- R7 (High value): subtotal £600 > £500 → **8% matched**
- R8 (Furniture): category is Furniture → **6% matched**
- R9 (End-of-season): October is not January or July → not matched
- All other rules: not matched

**Matching discounts collected**: 5%, 12%, 8%, 6%.

**Highest discount**: 12% (from R6 — bulk purchase large).

**Order total**: £600 × (1 − 0.12) = £600 × 0.88 = **£528.00**. Discount amount shown: £72.00.
