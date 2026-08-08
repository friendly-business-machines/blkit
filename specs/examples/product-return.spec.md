---
name: product-return
description: Business process specification for handling a customer's request to return a purchased product and determining the appropriate refund or replacement outcome.
status: implemented
code:
  - docs/examples/product-return.md
  - internal/doctest/testdata/product-return/example_test.go
---

# Product Return and Refund

## Overview

A customer may request to return a purchased product within the return window. The process validates the request, determines whether the item is eligible for return, assesses the item's condition upon receipt, and then issues a refund, a replacement, or a store credit depending on the outcome.

The process has two time boundaries:

- **Return request window**: The customer must submit the return request within 30 calendar days of the delivery date.
- **Item receipt window**: Once a return is authorised, the physical item must be received at the warehouse within 14 calendar days of the authorisation date. If the item is not received within this window, the authorisation expires and no refund is issued.

---

## Return Request Information

| Field | Description |
|---|---|
| Order Number | The original purchase order reference |
| Customer ID | Unique identifier of the requesting customer |
| Product Code | SKU of the item being returned |
| Quantity | Number of units being returned |
| Delivery Date | Date the original order was delivered |
| Return Request Date | Date the customer submitted the return request |
| Return Reason | Reason selected from the standard reason list |
| Preferred Resolution | Refund, replacement, or store credit |

### Standard return reasons

| Reason Code | Description |
|---|---|
| DEFECTIVE | Item arrived damaged or does not function as advertised |
| WRONG_ITEM | Incorrect product was delivered |
| NOT_AS_DESCRIBED | Item does not match the product listing |
| NO_LONGER_NEEDED | Customer changed their mind |
| DUPLICATE_ORDER | Customer accidentally ordered twice |

---

## Eligibility Rules

A return request is eligible for processing only if all of the following conditions are met.

| Rule | Condition | Result if false |
|---|---|---|
| E1 | Return request date is within 30 calendar days of delivery date | Rejected: outside return window |
| E2 | Product is flagged as returnable in the product catalogue | Rejected: non-returnable item |
| E3 | Quantity requested does not exceed the quantity originally purchased on the order | Rejected: quantity exceeds original purchase |

Non-returnable categories: perishable food, personalised items, downloadable software, and hazardous materials.

---

## Return Authorisation

If all eligibility rules pass, the process issues a Return Merchandise Authorisation (RMA) number and sends the customer a prepaid return label. The 14-calendar-day receipt window begins from the RMA issue date.

---

## Item Condition Assessment

When the item arrives at the warehouse, a returns operative inspects it and records a condition grade.

| Grade | Description |
|---|---|
| A – Unopened | Item in original sealed packaging, indistinguishable from new |
| B – Like new | Item opened but unused, all original packaging and accessories present |
| C – Used | Item shows signs of use; packaging may be missing or damaged |
| D – Damaged | Item is broken, scratched, or otherwise impaired beyond normal use |

---

## Refund Calculation Rules

The refund amount depends on the return reason and the condition grade.

| Return Reason | Condition Grade | Refund Percentage |
|---|---|---|
| DEFECTIVE | Any | 100% |
| WRONG_ITEM | Any | 100% |
| NOT_AS_DESCRIBED | Any | 100% |
| NO_LONGER_NEEDED | A – Unopened | 100% |
| NO_LONGER_NEEDED | B – Like new | 100% |
| NO_LONGER_NEEDED | C – Used | 50% |
| NO_LONGER_NEEDED | D – Damaged | 0% |
| DUPLICATE_ORDER | A – Unopened | 100% |
| DUPLICATE_ORDER | B – Like new | 100% |
| DUPLICATE_ORDER | C – Used | 50% |
| DUPLICATE_ORDER | D – Damaged | 0% |

The refund amount is: **Refund = Unit Price × Quantity Returned × Refund Percentage**

---

## Resolution Rules

The actual resolution issued depends on the refund percentage, whether a replacement is in stock, and the customer's preferred resolution.

| Refund Percentage | Replacement in Stock | Preferred Resolution | Resolution Issued |
|---|---|---|---|
| 100% | Yes | Replacement | Replacement dispatched, no charge |
| 100% | No | Replacement | Full refund to original payment method |
| 100% | Yes or No | Refund | Full refund to original payment method |
| 100% | Yes or No | Store credit | Store credit issued at 110% of refund amount |
| 50% | Yes | Replacement | Replacement dispatched; 50% of unit price charged |
| 50% | No | Replacement | Partial refund at 50% to original payment method |
| 50% | Yes or No | Refund | Partial refund at 50% to original payment method |
| 50% | Yes or No | Store credit | Store credit issued at 50% of unit price |
| 0% | Any | Any | Request declined; item returned to customer at their cost or disposed of if they decline return shipment |

---

## Outcomes

| Condition | Outcome |
|---|---|
| Request outside 30-day window | Rejected; no RMA issued |
| Product is non-returnable | Rejected; no RMA issued |
| Quantity exceeds original purchase | Rejected; no RMA issued |
| RMA issued but item not received within 14 days | Authorisation expired; no refund |
| Refund 100%, customer wants replacement, stock available | Replacement dispatched |
| Refund 100%, replacement unavailable or customer wants refund | Full monetary refund |
| Refund 100%, customer wants store credit | Store credit at 110% of purchase price |
| Refund 50%, various | Partial refund or partial store credit |
| Refund 0% | Declined; item returned to customer |

---

## Examples

| Order | Product | Unit Price | Qty | Return Reason | Condition | Preferred | Stock | Refund % | Refund Amount | Resolution |
|---|---|---|---|---|---|---|---|---|---|---|
| ORD-101 | Headphones | £80 | 1 | DEFECTIVE | D – Damaged | Replacement | Yes | 100% | £80 | Replacement dispatched |
| ORD-102 | Notebook | £12 | 3 | NO_LONGER_NEEDED | C – Used | Refund | — | 50% | £18 | Partial refund £18 |
| ORD-103 | Desk Chair | £350 | 1 | NOT_AS_DESCRIBED | B – Like new | Store credit | — | 100% | £350 | Store credit £385 |
| ORD-104 | USB Cable | £8 | 2 | DUPLICATE_ORDER | A – Unopened | Refund | — | 100% | £16 | Full refund £16 |
| ORD-105 | Monitor | £450 | 1 | NO_LONGER_NEEDED | D – Damaged | Refund | — | 0% | £0 | Declined |

### Worked Example: ORD-103

The customer purchased a desk chair for £350 and requests a return with reason NOT_AS_DESCRIBED. The return request is submitted on day 12 after delivery (within the 30-day window). The desk chair is in the returnable product catalogue.

**Eligibility**: Day 12 ≤ 30 ✓. Desk chairs are returnable ✓. Quantity 1 ≤ 1 purchased ✓. RMA issued.

The item arrives at the warehouse on day 8 of the 14-day receipt window. The operative inspects the chair and assigns grade B (like new — opened but unused, all parts present).

**Refund calculation**: Reason NOT_AS_DESCRIBED with any condition grade → 100%. Refund amount = £350 × 1 × 100% = **£350**.

**Resolution**: Refund percentage is 100% and the customer's preferred resolution is store credit. Store credit is issued at 110% of £350 = **£385**.

The customer receives a store credit notification for £385, valid for 12 months.
