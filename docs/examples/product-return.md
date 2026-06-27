# Product Return and Refund

> A customer requests a return; the process validates eligibility within the
> return window, grades the item's condition on receipt, and issues a refund,
> replacement, or store credit accordingly.

## Business overview

A customer may request to return a purchased product within the return window.
The process validates the request, determines whether the item is eligible,
assesses the item's condition on receipt, and then issues a refund, a
replacement, or a store credit depending on the outcome. Two time boundaries
apply:

- **Return request window** — the customer must submit the return request within
  30 calendar days of the delivery date.
- **Item receipt window** — once a return is authorised, the physical item must be
  received at the warehouse within 14 calendar days of the authorisation date. If
  not, the authorisation expires and no refund is issued.

### Return request information

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

#### Standard return reasons

| Reason Code | Description |
|---|---|
| DEFECTIVE | Item arrived damaged or does not function as advertised |
| WRONG_ITEM | Incorrect product was delivered |
| NOT_AS_DESCRIBED | Item does not match the product listing |
| NO_LONGER_NEEDED | Customer changed their mind |
| DUPLICATE_ORDER | Customer accidentally ordered twice |

### Eligibility rules

A return is eligible only if **all** of the following hold:

| Rule | Condition | Result if false |
|---|---|---|
| E1 | Return request date is within 30 calendar days of delivery date | Rejected: outside return window |
| E2 | Product is flagged as returnable in the catalogue | Rejected: non-returnable item |
| E3 | Quantity requested does not exceed the quantity originally purchased | Rejected: quantity exceeds original purchase |

Non-returnable categories: perishable food, personalised items, downloadable
software, and hazardous materials.

### Return authorisation

If all eligibility rules pass, the process issues a Return Merchandise
Authorisation (RMA) number and sends a prepaid return label. The 14-calendar-day
receipt window begins from the RMA issue date.

### Item condition assessment

When the item arrives, a returns operative inspects it and records a grade.

| Grade | Description |
|---|---|
| A – Unopened | Item in original sealed packaging, indistinguishable from new |
| B – Like new | Item opened but unused, all original packaging and accessories present |
| C – Used | Item shows signs of use; packaging may be missing or damaged |
| D – Damaged | Item is broken, scratched, or otherwise impaired beyond normal use |

### Refund calculation rules

The refund percentage depends on the return reason and the condition grade.

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

> Refund = Unit Price × Quantity Returned × Refund Percentage

### Resolution rules

The resolution issued depends on the refund percentage, whether a replacement is
in stock, and the customer's preferred resolution.

| Refund Percentage | Replacement in Stock | Preferred Resolution | Resolution Issued |
|---|---|---|---|
| 100% | Yes | Replacement | Replacement dispatched, no charge |
| 100% | No | Replacement | Full refund to original payment method |
| 100% | Yes or No | Refund | Full refund to original payment method |
| 100% | Yes or No | Store credit | Store credit at 110% of refund amount |
| 50% | Yes | Replacement | Replacement dispatched; 50% of unit price charged |
| 50% | No | Replacement | Partial refund at 50% to original payment method |
| 50% | Yes or No | Refund | Partial refund at 50% to original payment method |
| 50% | Yes or No | Store credit | Store credit at 50% of unit price |
| 0% | Any | Any | Request declined; item returned to customer or disposed of if declined |

### Outcomes

| Condition | Outcome |
|---|---|
| Request outside 30-day window | Rejected; no RMA issued |
| Product is non-returnable | Rejected; no RMA issued |
| Quantity exceeds original purchase | Rejected; no RMA issued |
| RMA issued but item not received within 14 days | Authorisation expired; no refund |
| Refund 100%, wants replacement, stock available | Replacement dispatched |
| Refund 100%, replacement unavailable or wants refund | Full monetary refund |
| Refund 100%, wants store credit | Store credit at 110% of purchase price |
| Refund 50%, various | Partial refund or partial store credit |
| Refund 0% | Declined; item returned to customer |

### Worked examples

| Order | Product | Unit Price | Qty | Return Reason | Condition | Preferred | Stock | Refund % | Refund Amount | Resolution |
|---|---|---|---|---|---|---|---|---|---|---|
| ORD-101 | Headphones | £80 | 1 | DEFECTIVE | D – Damaged | Replacement | Yes | 100% | £80 | Replacement dispatched |
| ORD-102 | Notebook | £12 | 3 | NO_LONGER_NEEDED | C – Used | Refund | — | 50% | £18 | Partial refund £18 |
| ORD-103 | Desk Chair | £350 | 1 | NOT_AS_DESCRIBED | B – Like new | Store credit | — | 100% | £350 | Store credit £385 |
| ORD-104 | USB Cable | £8 | 2 | DUPLICATE_ORDER | A – Unopened | Refund | — | 100% | £16 | Full refund £16 |
| ORD-105 | Monitor | £450 | 1 | NO_LONGER_NEEDED | D – Damaged | Refund | — | 0% | £0 | Declined |

For ORD-103, the return is submitted on day 12 (within 30 days) for a returnable
item, quantity 1 of 1 — eligible, RMA issued. The chair arrives on day 8 of the
14-day window and is graded B. NOT_AS_DESCRIBED with any grade → 100%, so refund
= £350 × 1 × 100% = £350. The customer's preferred resolution is store credit,
issued at 110% = **£385**.

## Implementation

!!! warning "Implementation pending"
    This example combines **date-window eligibility checks**, two lookup decision
    tables (refund percentage, then resolution), and a **process** with a timer
    for the 14-day receipt window. The Go implementation depends on the
    `decisions` and `processes` packages, which are still being built. This page
    documents the process; the runnable blkit code will be added once those
    packages land.

    In the meantime, see the authoritative
    [business spec](https://github.com/friendly-business-machines/blkit/blob/main/specs/examples/product-return.spec.md),
    [Getting started](../getting-started/index.md) for orientation, and the
    [Reference](../reference/blkit.md) for the expression engine available today.

## Notes

- Two distinct timers govern the flow: a 30-calendar-day eligibility window
  before authorisation, and a 14-calendar-day receipt window after it — both
  natural fits for blkit's date and duration expressions.
- The refund percentage and the resolution are **two separate decision tables**:
  the first (reason × grade) produces a percentage, which then feeds the second
  (percentage × stock × preference) to choose the actual resolution.
