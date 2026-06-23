# Numbers

> Exact decimal arithmetic and the numeric functions — rounding, absolute
> value, min and max, sum, and more.

!!! note "Guide in progress"
    This page of the language guide is still being written. In the meantime,
    [Architecture → Expressions](../architecture/expressions.md) explains how the
    engine works, the generated [Reference](../reference/blkit.md) lists the API,
    and `specs/expressions/number.spec.md` defines the behaviour authoritatively.

## What this page will cover

blkit numbers are exact decimals, so business arithmetic doesn't drift; this
page covers numeric literals, operators, and functions.

- Numeric literals and exact decimal arithmetic
- Comparison and rounding behaviour
- The numeric function library
