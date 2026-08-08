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

The validations, arithmetic, GL mapping, and approval decision can be implemented
without the process engine.

``` { .go .blkit-example title="main.go" }
package main

import (
	"encoding/json"
	"fmt"
	bl "github.com/friendly-business-machines/blkit/core"
	"os"
)

type InvoiceLine struct {
	Description string `json:"description"`
	Quantity    string `json:"quantity"`
	UnitPrice   string `json:"unit_price"`
	Category    string `json:"category"`
}
type InvoiceInput struct {
	Total            string        `json:"total"`
	Duplicate        bool          `json:"duplicate"`
	VendorActive     bool          `json:"vendor_active"`
	CurrencyApproved bool          `json:"currency_approved"`
	Lines            []InvoiceLine `json:"lines"`
}
type InvoiceDecision struct {
	ValidationErrors []string `json:"validation_errors"`
	ComputedTotal    string   `json:"computed_total"`
	GLCodes          []string `json:"gl_codes"`
	RequiresApproval bool     `json:"requires_approval"`
}
type lineEnv struct {
	Quantity bl.BlNumber `expr:"quantity"`
	Price    bl.BlNumber `expr:"price"`
}

var lineTotal = mustLineExpr(bl.Expr[lineEnv](`quantity*price`))

type invoiceEnv struct {
	Computed bl.BlNumber `expr:"computed"`
	Stated   bl.BlNumber `expr:"stated"`
}

var invoiceChecks = mustInvoiceExpr(bl.Expr[invoiceEnv](`{lines_valid: abs(computed-stated)<=0.01, requires_approval: stated>10000}`))

func mustLineExpr(e *bl.BlExpr[lineEnv], err error) *bl.BlExpr[lineEnv] {
	if err != nil {
		panic(err)
	}
	return e
}
func mustInvoiceExpr(e *bl.BlExpr[invoiceEnv], err error) *bl.BlExpr[invoiceEnv] {
	if err != nil {
		panic(err)
	}
	return e
}
```

Each validation contributes its own message, so callers receive all failures.

``` { .go .blkit-example title="main.go" }
func DecideInvoice(in InvoiceInput) (InvoiceDecision, error) {
	stated, err := bl.Number(in.Total)
	if err != nil {
		return InvoiceDecision{}, err
	}
	computed, _ := bl.Number(0)
	codes := []string{}
	mapping := map[string]string{"travel": "6000", "equipment": "6100", "services": "6200"}
	for _, line := range in.Lines {
		q, e := bl.Number(line.Quantity)
		if e != nil {
			return InvoiceDecision{}, e
		}
		p, e := bl.Number(line.UnitPrice)
		if e != nil {
			return InvoiceDecision{}, e
		}
		v, e := lineTotal.Evaluate(lineEnv{q, p})
		if e != nil {
			return InvoiceDecision{}, e
		}
		computed, _ = bl.Number(computed.Decimal().Add(v.(bl.BlNumber).Decimal()))
		code := mapping[line.Category]
		if code == "" {
			code = "6999"
		}
		codes = append(codes, code)
	}
	checks, err := invoiceChecks.Evaluate(invoiceEnv{computed, stated})
	if err != nil {
		return InvoiceDecision{}, err
	}
	m := checks.(bl.BlDictionary).Native()
	errors := []string{}
	if in.Duplicate {
		errors = append(errors, "duplicate invoice")
	}
	if !in.VendorActive || !in.CurrencyApproved {
		errors = append(errors, "vendor is not approved")
	}
	if !m["lines_valid"].(bl.BlBoolean).Native() {
		errors = append(errors, "line total mismatch")
	}
	return InvoiceDecision{errors, computed.String(), codes, m["requires_approval"].(bl.BlBoolean).Native()}, nil
}
```

``` { .go .blkit-example title="main.go" }
func main() {
	var in InvoiceInput
	if e := json.NewDecoder(os.Stdin).Decode(&in); e != nil {
		fmt.Fprintln(os.Stderr, e)
		os.Exit(1)
	}
	out, e := DecideInvoice(in)
	if e != nil {
		fmt.Fprintln(os.Stderr, e)
		os.Exit(1)
	}
	if e = json.NewEncoder(os.Stdout).Encode(out); e != nil {
		fmt.Fprintln(os.Stderr, e)
		os.Exit(1)
	}
}
```

Parallel fork/join execution, waiting for manager input, vendor notification, and
payment scheduling require the process engine and are not shown as Go yet.

## Notes

- The three validation checks fork on receipt and join before the outcome is
  decided. The join **collects all failures** rather than short-circuiting on the
  first, so the vendor gets a complete rejection reason.
- The line-item check compares a computed total against the stated total within a
  0.01 tolerance — a good fit for `bl.Number()` decimal arithmetic.
