---
name: employee-onboarding
description: Business process specification for coordinating the parallel setup tasks required when a new employee joins the organisation.
status: implemented
code:
  - docs/examples/employee-onboarding.md
  - internal/doctest/testdata/employee-onboarding/example_test.go
---

# Employee Onboarding

## Overview

When a new employee is hired, three departments must each complete a set of setup tasks before the employee can start work. IT provisions accounts and equipment, HR prepares contracts and benefits enrolment, and Facilities arranges a desk and access card. All three workstreams run at the same time and the process is not complete until every department signals readiness.

Once all departments are ready, a welcome notification is sent to the employee with their start date, building access instructions, and a link to the HR self-service portal.

If any department fails to complete its tasks within five business days of the hire confirmation, the process escalates to the HR Business Partner responsible for the hiring team.

---

## Trigger

The onboarding process begins when a hire confirmation record is submitted with the following information.

| Field | Description |
|---|---|
| Employee Name | Full legal name of the new hire |
| Employee ID | System-generated unique identifier |
| Department | Business unit the employee is joining |
| Role | Job title |
| Start Date | Agreed first day of employment |
| Line Manager | Name and employee ID of the direct manager |
| Work Location | Office site or "remote" |
| Equipment Profile | Standard laptop, developer workstation, or mobile worker kit |

---

## Workstreams

The following three workstreams run in parallel from the moment the hire confirmation is received.

### IT Workstream

| Step | Description | Responsible |
|---|---|---|
| 1 | Create network account and set temporary password | IT Service Desk |
| 2 | Enrol device in device management based on equipment profile | IT Service Desk |
| 3 | Assign software licences appropriate to role and department | IT Service Desk |
| 4 | Configure email account and add to team distribution lists | IT Service Desk |
| 5 | Prepare equipment and schedule delivery or collection | IT Logistics |
| 6 | Mark IT workstream complete | IT Service Desk |

### HR Workstream

| Step | Description | Responsible |
|---|---|---|
| 1 | Generate employment contract and send for e-signature | HR Coordinator |
| 2 | Receive signed contract | HR Coordinator |
| 3 | Register employee in payroll with start date and salary | Payroll |
| 4 | Send benefits enrolment invitation | HR Coordinator |
| 5 | Confirm benefits selections received or set defaults if not responded within 48 hours | HR Coordinator |
| 6 | Mark HR workstream complete | HR Coordinator |

### Facilities Workstream

| Step | Description | Responsible |
|---|---|---|
| 1 | Assign desk or hot-desk zone based on department and work location | Facilities |
| 2 | Programme access card for building, floor, and relevant secure zones | Facilities |
| 3 | Add employee to building visitor system | Facilities |
| 4 | Mark Facilities workstream complete | Facilities |

---

## Completion Gate

The process advances only when all three workstreams are marked complete. If all three are complete, a welcome pack is sent to the employee. If any workstream is incomplete after five business days from the hire confirmation date, an escalation notification is sent to the HR Business Partner listing which workstreams are outstanding.

---

## Outcomes

| Condition | Outcome |
|---|---|
| All three workstreams complete within five business days | Welcome pack sent to employee; process closed as successful |
| One or more workstreams incomplete after five business days | Escalation raised to HR Business Partner; process remains open |

---

## Examples

| Employee | Start Date | IT Complete | HR Complete | Facilities Complete | Days Since Hire | Result |
|---|---|---|---|---|---|---|
| Alice Tan | 2024-03-11 | Day 2 | Day 3 | Day 1 | 3 | Welcome pack sent |
| Bob Okoye | 2024-03-11 | Day 4 | Day 6 | Day 3 | 6 | Escalation: HR outstanding |
| Carol Singh | 2024-03-18 | Day 5 | Day 5 | Day 5 | 5 | Welcome pack sent |
| David Lim | 2024-03-18 | Day 6 | Day 7 | Day 6 | 7 | Escalation: IT and HR outstanding |

### Worked Example: Alice Tan

Alice's hire confirmation arrives on 4 March 2024. Her start date is 11 March 2024 (five business days later).

- IT completes on 6 March (day 2). No escalation triggered.
- Facilities completes on 5 March (day 1). No escalation triggered.
- HR receives the signed contract on 5 March, completes payroll and benefits on 7 March (day 3). No escalation triggered.

All three workstreams are complete by day 3. The five-business-day window has not been exceeded. The process sends Alice a welcome pack on 7 March containing her start date, access card collection instructions, and the HR portal link.
