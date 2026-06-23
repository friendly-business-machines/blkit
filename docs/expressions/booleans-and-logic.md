# Booleans & Logic

> Boolean values and blkit's three-valued (true / false / null) logic,
> including and, or, not, and comparison results.

!!! note "Guide in progress"
    This page of the language guide is still being written. In the meantime,
    [Architecture → Expressions](../architecture/expressions.md) explains how the
    engine works, the generated [Reference](../reference/blkit.md) lists the API,
    and `specs/expressions/boolean.spec.md` defines the behaviour authoritatively.

## What this page will cover

blkit uses three-valued logic, so a missing value yields null rather than a
silent true or false; this page explains how that works.

- Boolean literals and comparison results
- and / or / not under three-valued logic
- How null short-circuits and propagates
