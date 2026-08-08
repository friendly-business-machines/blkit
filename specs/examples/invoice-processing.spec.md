---
name: Example — Invoice Processing Workflow
description: An accounts payable team processes incoming supplier invoices through three concurrent validation checks, GL code assignment, and amount-based approval routing before scheduling payment
status: implemented
code:
  - docs/examples/invoice-processing.md
  - internal/doctest/testdata/invoice-processing/example_test.go
---

# Example: Invoice Processing Workflow

## Overview

When a supplier sends an invoice to the accounts payable team, it must be validated, coded to the general ledger, approved if the amount is above a defined threshold, and then scheduled for payment on or before the due date. The three validation tasks — duplicate detection, vendor verification, and line-item arithmetic — are independent of one another and can be performed simultaneously.

## Invoice Information

Each invoice contains:

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

## Validation

Three checks run in parallel as soon as the invoice is received:

### Duplicate Check

The invoice is compared against previously processed invoices for the same vendor. An invoice is considered a duplicate if a prior invoice exists with the same vendor, the same total amount, and the same invoice date. A duplicate invoice is rejected to prevent double-payment.

### Vendor Verification

The vendor ID is checked against the approved vendor master list. The vendor must exist, be active (not suspended or terminated), and be approved for the currency on the invoice. An invoice from an unrecognised or inactive vendor is rejected.

### Line Item Validation

The individual line-item quantities and unit prices are multiplied together and summed. The computed total must match the stated `total_amount` on the invoice within a tolerance of 0.01 (to allow for minor rounding differences). A mismatch indicates a data entry error or manipulation and the invoice is rejected.

### Validation Outcome

All three checks must pass for processing to continue. If any check fails, the invoice is rejected and the vendor is notified with the reason. The specific failures from all three checks are collected and reported together; the process does not stop at the first failure.

## GL Code Assignment

Once validation passes, each line item is assigned a general ledger account code based on its GL category hint and the company's chart of accounts mapping. This step is performed by the finance system.

## Approval

Invoices above **10,000** in the invoice currency require approval from a manager before payment is scheduled. Invoices at or below this threshold proceed directly to payment scheduling.

If the manager rejects the invoice, it is returned to the vendor as disputed with the stated reason.

## Payment Scheduling

Approved invoices are submitted to the payment system to be included in the next payment run on or before the due date. A payment batch reference is recorded against the invoice.

## Outcomes

| Outcome | Meaning |
|---|---|
| **Payment Scheduled** | Invoice passed all checks and approval (if required); payment queued for the due date |
| **Invoice Rejected** | One or more validation checks failed, or manager rejected the dispute; vendor notified with reason |

## Examples

| Total Amount | Duplicate? | Vendor Active? | Lines Valid? | Amount Requires Approval? | Outcome |
|---|---|---|---|---|---|
| 6,000 | No | Yes | Yes | No | Payment Scheduled |
| 50,000 | No | Yes | Yes | Yes | Payment Scheduled (manager approved) |
| 6,000 | Yes | Yes | Yes | No | Invoice Rejected — duplicate |
| 6,000 | No | No | Yes | No | Invoice Rejected — unknown vendor |
| 6,000 | No | Yes | No | No | Invoice Rejected — line total mismatch |
