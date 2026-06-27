# Employee Expense Approval

> An employee submits a business expense claim; the approval route — automatic,
> manager, or finance director — is determined by amount, category, and the
> employee's seniority, then the matching review runs.

## Business overview

Employees submit expense claims for reimbursement of business costs such as
travel, meals, accommodation, and equipment. The company's expense policy
determines who must approve a claim before it can be reimbursed. Small claims are
approved automatically; larger or higher-risk claims require a human reviewer.

### Expense information

| Field | Description |
|---|---|
| Expense ID | Unique identifier for the claim |
| Employee ID | The employee submitting the claim |
| Employee level | One of: junior, senior, executive |
| Amount | Total claimed amount in company currency |
| Category | One of: meals, travel, accommodation, equipment, other |
| Description | Free-text explanation of the business purpose |
| Receipts | One or more supporting receipt attachments |

### Approval policy

Rules are evaluated in priority order; the first matching rule is applied.

| Priority | Amount | Category | Employee Level | Approval Route |
|---|---|---|---|---|
| 1 | 50 or below | Any | Any | Automatic |
| 2 | 500 or below | Meals, travel, or accommodation | Any | Manager |
| 3 | 500 or below | Equipment or other | Senior or executive | Manager |
| 4 | 2,000 or below | Any | Executive | Manager |
| 5 | 2,000 or below | Any | Any | Finance Director |
| 6 | Above 2,000 | Any | Any | Finance Director |

Rule 6 is a universal catch-all ensuring all claims above 2,000 always reach the
Finance Director regardless of category or employee level.

### Approval routes

- **Automatic approval** — no human review; the claim is approved immediately on
  submission and the employee is notified.
- **Manager review** — sent to the employee's direct line manager, who may
  approve or reject (optionally with a reason). The employee is notified.
- **Finance Director review** — escalated to the Finance Director, who may
  approve or reject. The employee is notified.

### Outcomes

| Outcome | Meaning |
|---|---|
| **Expense Approved** | Claim approved through the applicable route; employee notified; reimbursement proceeds |
| **Expense Rejected** | Claim declined by the reviewer; employee notified with reason |

### Worked examples

| Amount | Category | Employee Level | Approval Route | Example Outcome |
|---|---|---|---|---|
| 30.00 | Meals | Junior | Automatic | Approved immediately |
| 450.00 | Travel | Junior | Manager | Sent to manager for review |
| 450.00 | Equipment | Junior | Finance Director | Escalated to Finance Director |
| 1,200.00 | Other | Executive | Manager | Sent to manager (executive benefit, rule 4) |
| 3,500.00 | Equipment | Senior | Finance Director | Above 2,000; Finance Director required |

## Implementation

!!! warning "Implementation pending"
    This example combines a **first-match decision table** (route selection)
    with a **process** (the human review workflow the chosen route triggers). The
    Go implementation depends on the `decisions` and `processes` packages, which
    are still being built. This page documents the policy and routes; the
    runnable blkit code will be added once those packages land.

    In the meantime, see the authoritative
    [business spec](https://github.com/friendly-business-machines/blkit/blob/main/specs/examples/expense-approval.spec.md),
    [Getting started](../getting-started/index.md) for orientation, and the
    [Reference](../reference/blkit.md) for the expression engine available today.

## Notes

- Routing is a decision; the review itself is a process. The decision's output
  (which route applies) selects which review workflow runs.
- A junior's £450 equipment claim falls through rules 2 and 3 (equipment needs
  senior/executive) to land on rule 5 — Finance Director — showing how priority
  ordering interacts with the category and seniority conditions.
