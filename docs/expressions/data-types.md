# Data Types

> The blkit value system — numbers, strings, booleans, null, dates and times,
> durations, lists, dictionaries, ranges, and tables — and their null-aware
> semantics.

!!! note "Guide in progress"
    This page of the language guide is still being written. In the meantime,
    [Architecture → Expressions](../architecture/expressions.md) explains how the
    engine works, the generated [Reference](../reference/blkit.md) lists the API,
    and the type specs under `specs/expressions/` define the behaviour
    authoritatively.

## What this page will cover

Every expression produces and consumes blkit values; this page introduces the
full set of types and how null threads through them.

- The complete set of value types
- The literal syntax for each type
- Null and how it propagates, and testing a value's type
