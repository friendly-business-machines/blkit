# Decision Native Functions

> Back a decision with a hand-written Go function when its logic is beyond — or
> better expressed outside — the expression language.

!!! warning "Coming soon"
    This guide is being written and will be published when the decisions package
    lands. The behaviour is defined authoritatively by
    `specs/decision-tasks/decision-native-fn.spec.md`.

## What this page will cover

A native function lets a decision call into typed Go code instead of, or
alongside, expression-language rules.

- Registering a Go-backed decision with typed inputs and outputs
- When to reach for native code over a table or expression
- How native decisions compose with the rest of a decision graph
