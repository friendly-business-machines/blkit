# Decision Tables

> Express decision logic as a table of rules — rows of input conditions mapped
> to output values — the spreadsheet-style core of DMN.

!!! warning "Coming soon"
    This guide is being written and will be published when the decisions package
    lands. The behaviour is defined authoritatively by
    `specs/decision-tasks/decision-table.spec.md`.

## What this page will cover

A decision table lays out business rules as rows, each matching a combination of
input conditions to a set of outputs.

- Input and output columns, and the unary tests in each cell
- Hit policies — which rule or rules win when several match
- Default outputs and completeness
