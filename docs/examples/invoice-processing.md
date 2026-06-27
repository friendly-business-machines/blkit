# Invoice Processing Workflow

> An accounts payable team runs three concurrent validation checks on a supplier
> invoice, assigns GL codes, routes for amount-based approval, and schedules
> payment.

## Business overview

When a supplier sends an invoice to accounts payable, it must be validated, coded
to the general ledger, approved if its amount is above a threshold, and then
scheduled for payment on or before the due date. The three validation tasks —
duplicate detection, vendor verification, and line-item arithmetic — are
independent and run simultaneously.

### Invoice information

| Field | Description |
|---|---|
| Invoice ID | Unique identifier for this invoice |
| Vendor ID | The supplying company |
| Invoice date | The date printed on the invoice |
| Due date | The date by which payment must be made |
| Line items | A list of items billed; each has a description, quantity, unit price, and a GL category hint |
| Total amount | The sum of all line-item totals as stated on the invoice |
| Currency | ISO 4217 currency code |
| Purchase order reference | Optional reference to an existing approved purchase order |

### Validation (three checks in parallel)

- **Duplicate check** — the invoice is a duplicate if a prior invoice exists for
  the same vendor with the same total amount and the same invoice date.
  Duplicates are rejected to prevent double-payment.
- **Vendor verification** — the vendor must exist on the approved master list, be
  active (not suspended or terminated), and be approved for the invoice currency.
- **Line item validation** — line quantities × unit prices are summed; the
  computed total must match the stated `total_amount` within a tolerance of 0.01.

All three checks must pass for processing to continue. If any fails, the invoice
is rejected and the vendor notified with the reason. Failures from all three
checks are collected and reported together — the process does not stop at the
first failure.

### GL code assignment

Once validation passes, each line item is assigned a general ledger account code
from its GL category hint and the company's chart-of-accounts mapping. This step
is performed by the finance system.

### Approval

Invoices above **10,000** in the invoice currency require manager approval before
payment scheduling. Invoices at or below this threshold proceed directly. If the
manager rejects the invoice, it is returned to the vendor as disputed.

### Payment scheduling

Approved invoices are submitted to the payment system for the next payment run on
or before the due date, and a payment batch reference is recorded.

### Outcomes

| Outcome | Meaning |
|---|---|
| **Payment Scheduled** | Invoice passed all checks and approval (if required); payment queued for the due date |
| **Invoice Rejected** | One or more validation checks failed, or the manager rejected the dispute; vendor notified with reason |

### Worked examples

| Total Amount | Duplicate? | Vendor Active? | Lines Valid? | Requires Approval? | Outcome |
|---|---|---|---|---|---|
| 6,000 | No | Yes | Yes | No | Payment Scheduled |
| 50,000 | No | Yes | Yes | Yes | Payment Scheduled (manager approved) |
| 6,000 | Yes | Yes | Yes | No | Invoice Rejected — duplicate |
| 6,000 | No | No | Yes | No | Invoice Rejected — unknown vendor |
| 6,000 | No | Yes | No | No | Invoice Rejected — line total mismatch |

## Implementation

!!! warning "Implementation pending"
    This is a process with a **parallel (fork/join) section** for the three
    validation checks, a join that aggregates their results, and an amount-based
    approval gateway. The Go implementation depends on the `processes` package,
    which is still being built. This page documents the workflow; the runnable
    blkit code will be added once that package lands.

    In the meantime, see the authoritative
    [business spec](https://github.com/friendly-business-machines/blkit/blob/main/specs/examples/invoice-processing.spec.md),
    [Getting started](../getting-started/index.md) for orientation, and the
    [Reference](../reference/blkit.md) for the expression engine available today.

## Notes

- The three validation checks fork on receipt and join before the outcome is
  decided. The join **collects all failures** rather than short-circuiting on the
  first, so the vendor gets a complete rejection reason.
- The line-item check compares a computed total against the stated total within a
  0.01 tolerance — a good fit for `bl.number()` decimal arithmetic.
