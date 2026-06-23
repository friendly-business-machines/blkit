# Reference Data

> Feed decisions the lookup tables and constants they read from — rates, tiers,
> thresholds — without hard-coding them into rules.

!!! warning "Coming soon"
    This guide is being written and will be published when the decisions package
    lands. The behaviour is defined authoritatively by
    `specs/decision-tasks/reference-data.spec.md`.

## What this page will cover

Reference data is the static lookup information a decision consults — rate
cards, tier bands, code lists — kept separate from the rules that use it.

- Supplying reference datasets to a decision
- Referencing reference data from tables and expressions
- Updating reference data without rewriting rules
