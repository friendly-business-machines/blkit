---
name: Example — Employee Expense Approval
description: An employee submits a business expense claim; the approval route (automatic, manager review, or finance director review) is determined by the claim amount, expense category, and the employee's seniority; the appropriate review workflow then executes and the employee is notified of the outcome
status: implemented
code:
  - docs/examples/expense-approval.md
  - internal/doctest/testdata/expense-approval/example_test.go
---

# Example: Employee Expense Approval

## Overview

Employees submit expense claims for reimbursement of business costs such as travel, meals, accommodation, and equipment purchases. The company's expense policy determines who must approve a claim before it can be reimbursed. Small claims are approved automatically; larger or higher-risk claims require a human reviewer.

## Expense Information

Each submitted claim contains:

| Field | Description |
|---|---|
| Expense ID | Unique identifier for the claim |
| Employee ID | The employee submitting the claim |
| Employee level | One of: junior, senior, executive |
| Amount | Total claimed amount in company currency |
| Category | One of: meals, travel, accommodation, equipment, other |
| Description | Free-text explanation of the business purpose |
| Receipts | One or more supporting receipt attachments |

## Approval Policy

The following policy determines which approval route applies. Rules are evaluated in priority order and the first matching rule is applied.

| Priority | Amount | Category | Employee Level | Approval Route |
|---|---|---|---|---|
| 1 | 50 or below | Any | Any | Automatic |
| 2 | 500 or below | Meals, travel, or accommodation | Any | Manager |
| 3 | 500 or below | Equipment or other | Senior or executive | Manager |
| 4 | 2,000 or below | Any | Executive | Manager |
| 5 | 2,000 or below | Any | Any | Finance Director |
| 6 | Above 2,000 | Any | Any | Finance Director |

Rule 6 is a universal catch-all that ensures all claims above 2,000 always reach the Finance Director regardless of category or employee level.

## Approval Routes

### Automatic Approval

No human review is required. The claim is approved immediately upon submission and the employee is notified.

### Manager Review

The claim is sent to the employee's direct line manager for review. The manager may approve or reject the claim, optionally providing a reason. The employee is notified of the outcome.

### Finance Director Review

The claim is escalated to the Finance Director. The Finance Director may approve or reject the claim. The employee is notified of the outcome.

## Outcomes

| Outcome | Meaning |
|---|---|
| **Expense Approved** | Claim is approved through the applicable route; employee notified; reimbursement proceeds |
| **Expense Rejected** | Claim declined by the reviewer; employee notified with reason |

## Examples

| Amount | Category | Employee Level | Approval Route | Example Outcome |
|---|---|---|---|---|
| 30.00 | Meals | Junior | Automatic | Approved immediately |
| 450.00 | Travel | Junior | Manager | Sent to manager for review |
| 450.00 | Equipment | Junior | Finance Director | Escalated to Finance Director |
| 1,200.00 | Other | Executive | Manager | Sent to manager (executive benefit, rule 4) |
| 3,500.00 | Equipment | Senior | Finance Director | Above 2,000; Finance Director required |
