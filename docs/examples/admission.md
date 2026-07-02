# Course Admission

> A university admissions office decides whether an undergraduate application is
> admitted, waitlisted, or declined — and on what terms — from the applicant's
> academic profile.

## Business overview

A university admissions office evaluates undergraduate applications at the point
of submission. The decision is fully automated, taken in a single step with no
human review. Given information about the applicant, the office must decide:

- Whether the application is **admitted**, **waitlisted**, or **declined**.
- If admitted or waitlisted: the **maximum first-term credit load** the applicant may enrol in.
- If admitted or waitlisted: the **advising track** that will apply.

### Applicant information

| Field | Description |
|---|---|
| Aptitude score | An integer entrance-exam score, typically in the range 400–1600 |
| Grade average | The applicant's secondary-school grade point average (0.0–4.0) |
| Absence ratio | Days absent divided by total school days, as a decimal (e.g. 0.35 = 35%) |
| Enrolment status | One of: full-time, part-time, withdrawn |

### Decision rules

Decisions are evaluated in priority order. The first rule that matches all
conditions is applied; no further rules are evaluated.

| Priority | Aptitude Score | Enrolment Status | Absence Ratio | Decision | Max Credits | Track |
|---|---|---|---|---|---|---|
| 1 | 750 or above | Full-time or part-time | 30% or below | Admitted | 21 | Honors |
| 2 | 700 or above | Full-time or part-time | 40% or below | Admitted | 18 | Standard |
| 3 | 650 or above | Full-time or part-time | 40% or below | Waitlisted | 15 | Foundation |
| 4 | 600 or above | Full-time or part-time | 50% or below | Waitlisted | 12 | Support |
| 5 | Below 600 | Any | Any | Declined | — | — |
| 6 | Any | Withdrawn | Any | Declined | — | — |
| 7 | Any | Any | Above 50% | Declined | — | — |

Rules 5, 6, and 7 are catch-alls that ensure every application receives a
decision. A withdrawn applicant with a high aptitude score matches rule 6 before
reaching any of rules 1–4.

### Outcomes

| Decision | Meaning |
|---|---|
| **Admitted** | The university offers the applicant a place with the stated maximum credit load and advising track |
| **Waitlisted** | The application requires a secondary review (e.g. portfolio or interview) before a final offer; indicative terms are returned |
| **Declined** | The application does not meet the minimum criteria; no place is offered |

Declined applications return a maximum credit load of zero and no advising track.

### Worked examples

| Aptitude Score | GPA | Absence | Enrolment | Decision | Max Credits | Track |
|---|---|---|---|---|---|---|
| 780 | 3.9 | 25% | Full-time | Admitted | 21 | Honors |
| 710 | 3.2 | 38% | Part-time | Admitted | 18 | Standard |
| 660 | 2.8 | 39% | Full-time | Waitlisted | 15 | Foundation |
| 610 | 2.5 | 48% | Full-time | Waitlisted | 12 | Support |
| 580 | 3.0 | 30% | Full-time | Declined | — | — |
| 700 | 3.4 | 55% | Full-time | Declined | — | — |
| 730 | 3.7 | 28% | Withdrawn | Declined | — | — |

## Implementation

!!! warning "Implementation pending"
    This is a single decision table evaluated with **first-match** (priority)
    hit policy. The Go implementation depends on the `decisions` package, which
    is still being built. This page documents the business decision; the runnable
    blkit code will be added once that package lands.

    In the meantime, see the authoritative
    [business spec](https://github.com/friendly-business-machines/blkit/blob/main/specs/examples/admission.spec.md),
    [Getting started](../getting-started/index.md) for orientation, and the
    [Reference](../reference/blkit.md) for the expression engine available today.

## Notes

- The rule order matters: priority rows are tested top-to-bottom and the first
  match wins, so the catch-all decline rules (5–7) only apply once no
  higher-priority admission rule has matched.
- The credit load and advising track are outcomes of the matched rule, not a
  separate calculation step.
