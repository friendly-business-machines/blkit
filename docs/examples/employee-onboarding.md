# Employee Onboarding

> When a new employee is hired, IT, HR, and Facilities run their setup tasks in
> parallel; the process completes only when all three signal readiness, and
> escalates if any misses the five-business-day deadline.

## Business overview

When a new employee is hired, three departments must each complete a set of setup
tasks before the employee can start work. IT provisions accounts and equipment,
HR prepares contracts and benefits enrolment, and Facilities arranges a desk and
access card. All three workstreams run at the same time, and the process is not
complete until every department signals readiness.

Once all departments are ready, a welcome notification is sent to the employee
with their start date, building access instructions, and a link to the HR
self-service portal. If any department fails to complete its tasks within five
business days of the hire confirmation, the process escalates to the HR Business
Partner responsible for the hiring team.

### Trigger

The process begins when a hire confirmation record is submitted with:

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

### Workstreams (run in parallel)

#### IT

| Step | Description | Responsible |
|---|---|---|
| 1 | Create network account and set temporary password | IT Service Desk |
| 2 | Enrol device in device management based on equipment profile | IT Service Desk |
| 3 | Assign software licences appropriate to role and department | IT Service Desk |
| 4 | Configure email account and add to team distribution lists | IT Service Desk |
| 5 | Prepare equipment and schedule delivery or collection | IT Logistics |
| 6 | Mark IT workstream complete | IT Service Desk |

#### HR

| Step | Description | Responsible |
|---|---|---|
| 1 | Generate employment contract and send for e-signature | HR Coordinator |
| 2 | Receive signed contract | HR Coordinator |
| 3 | Register employee in payroll with start date and salary | Payroll |
| 4 | Send benefits enrolment invitation | HR Coordinator |
| 5 | Confirm benefits selections received or set defaults if not responded within 48 hours | HR Coordinator |
| 6 | Mark HR workstream complete | HR Coordinator |

#### Facilities

| Step | Description | Responsible |
|---|---|---|
| 1 | Assign desk or hot-desk zone based on department and work location | Facilities |
| 2 | Programme access card for building, floor, and relevant secure zones | Facilities |
| 3 | Add employee to building visitor system | Facilities |
| 4 | Mark Facilities workstream complete | Facilities |

### Completion gate

The process advances only when all three workstreams are marked complete. If all
three are complete, a welcome pack is sent to the employee. If any workstream is
incomplete after five business days from the hire confirmation date, an
escalation notification is sent to the HR Business Partner listing which
workstreams are outstanding.

### Outcomes

| Condition | Outcome |
|---|---|
| All three workstreams complete within five business days | Welcome pack sent to employee; process closed as successful |
| One or more workstreams incomplete after five business days | Escalation raised to HR Business Partner; process remains open |

### Worked examples

| Employee | Start Date | IT Complete | HR Complete | Facilities Complete | Days Since Hire | Result |
|---|---|---|---|---|---|---|
| Alice Tan | 2024-03-11 | Day 2 | Day 3 | Day 1 | 3 | Welcome pack sent |
| Bob Okoye | 2024-03-11 | Day 4 | Day 6 | Day 3 | 6 | Escalation: HR outstanding |
| Carol Singh | 2024-03-18 | Day 5 | Day 5 | Day 5 | 5 | Welcome pack sent |
| David Lim | 2024-03-18 | Day 6 | Day 7 | Day 6 | 7 | Escalation: IT and HR outstanding |

Alice's hire confirmation arrives on 4 March 2024; her start date is 11 March
(five business days later). IT completes on day 2, Facilities on day 1, and HR on
day 3. All three are complete by day 3 — within the five-business-day window — so
a welcome pack is sent on 7 March with her start date, access card collection
instructions, and the HR portal link.

## Implementation

!!! warning "Implementation pending"
    This is a process with a **parallel fork** into three independent
    workstreams, a **join gate** that waits for all three, and a **timer-based
    escalation** at five business days. The Go implementation depends on the
    `processes` package, which is still being built. This page documents the
    process; the runnable blkit code will be added once that package lands.

    In the meantime, see the authoritative
    [business spec](https://github.com/friendly-business-machines/blkit/blob/main/specs/examples/employee-onboarding.spec.md),
    [Getting started](../getting-started/index.md) for orientation, and the
    [Reference](../reference/blkit.md) for the expression engine available today.

## Notes

- The completion gate is an **AND-join**: it advances only when all three
  workstreams are marked complete, not when any one finishes.
- The five-business-day deadline is a **boundary timer** racing against the join.
  Whichever fires first determines the outcome — welcome pack or escalation — and
  the escalation message names the specific outstanding workstreams.
- "Five business days" is a calendar calculation that excludes weekends, a good
  fit for blkit's calendar/duration expressions.
