# Sub-Decisions

> Compose decisions by invoking one decision as a step inside another, so shared
> logic is defined once and reused.

!!! warning "Coming soon"
    This guide is being written and will be published when the decisions package
    lands. The behaviour is defined authoritatively by
    `specs/decision-tasks/sub-decision-task.spec.md`.

## What this page will cover

A sub-decision is a decision invoked from within another decision, letting you
build larger decisions out of smaller, reusable ones.

- Invoking one decision from another
- Mapping the parent's data to the sub-decision's inputs
- Reusing a single decision across many callers
