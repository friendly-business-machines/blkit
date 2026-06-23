# Decision Expressions

> Compute several named outputs from typed inputs, wiring outputs that depend on
> one another into a single evaluated unit.

!!! warning "Coming soon"
    This guide is being written and will be published when the decisions package
    lands. The behaviour is defined authoritatively by
    `specs/decision-tasks/decision-expression.spec.md`.

## What this page will cover

A decision expression turns one set of typed inputs into a set of named outputs,
where later outputs may build on earlier ones.

- Declaring inputs and multiple named outputs
- How outputs that reference each other are ordered and evaluated
- Returning a typed result object
