---
name: Example — Loan Eligibility
description: A retail bank determines whether a personal loan application is approved, conditionally approved, or declined, and what maximum loan amount and interest rate tier apply — based on credit score, income, debt-to-income ratio, and employment status
targets:
  - ../../docs/examples/loan-eligibility.md
---

# Example: Loan Eligibility

## Overview

A retail bank evaluates personal loan applications at the point of submission. Given information about the applicant, the bank must decide:

- Whether the application is approved, conditionally approved, or declined.
- If approved or conditional: the maximum loan amount the applicant may receive.
- If approved or conditional: the interest rate tier that will apply.

The decision is taken in a single step with no human review — it is fully automated based on the applicant's credit profile.

## Applicant Information

The following information is provided for each application:

| Field | Description |
|---|---|
| Credit score | An integer credit score, typically in the range 300–850 |
| Annual income | The applicant's gross annual income |
| Debt-to-income ratio | Total monthly debt obligations divided by gross monthly income, expressed as a decimal (e.g. 0.35 = 35%) |
| Employment status | One of: employed, self-employed, unemployed |

## Decision Rules

Decisions are evaluated in priority order. The first rule that matches all conditions for a given application is applied; no further rules are evaluated.

| Priority | Credit Score | Employment Status | Debt-to-Income Ratio | Decision | Max Loan | Rate Tier |
|---|---|---|---|---|---|---|
| 1 | 750 or above | Employed or self-employed | 30% or below | Approved | 5× annual income | Prime |
| 2 | 700 or above | Employed or self-employed | 40% or below | Approved | 4× annual income | Standard |
| 3 | 650 or above | Employed or self-employed | 40% or below | Conditional | 3× annual income | Elevated |
| 4 | 600 or above | Employed or self-employed | 50% or below | Conditional | 2× annual income | Subprime |
| 5 | Below 600 | Any | Any | Declined | — | — |
| 6 | Any | Unemployed | Any | Declined | — | — |
| 7 | Any | Any | Above 50% | Declined | — | — |

Rules 5, 6, and 7 are catch-alls that ensure every application receives a decision. An unemployed applicant with a high credit score matches rule 6 before reaching any of rules 1–4.

## Outcomes

| Decision | Meaning |
|---|---|
| **Approved** | The bank offers the applicant a loan up to the stated maximum at the stated rate tier |
| **Conditional** | The application requires secondary checks (e.g. affordability confirmation) before a final offer is made; the indicative terms are returned |
| **Declined** | The application does not meet the minimum criteria; no loan is offered |

Declined applications return a maximum loan amount of zero and no rate tier.

## Examples

| Credit Score | Income | DTI | Employment | Decision | Max Loan | Rate Tier |
|---|---|---|---|---|---|---|
| 780 | 100,000 | 25% | Employed | Approved | 500,000 | Prime |
| 710 | 60,000 | 38% | Self-employed | Approved | 240,000 | Standard |
| 660 | 45,000 | 39% | Employed | Conditional | 135,000 | Elevated |
| 610 | 30,000 | 48% | Employed | Conditional | 60,000 | Subprime |
| 580 | 50,000 | 30% | Employed | Declined | — | — |
| 700 | 70,000 | 55% | Employed | Declined | — | — |
| 730 | 90,000 | 28% | Unemployed | Declined | — | — |
